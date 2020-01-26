
mod room;
pub use self::room::*;
mod staff;
pub use self::staff::*;
mod staff_list;
pub use self::staff_list::*;
mod entity_info;
pub use self::entity_info::*;
mod stats;
pub use self::stats::*;
mod settings;
pub use self::settings::*;
mod edit_room;
pub use self::edit_room::*;
mod courses;
pub use self::courses::*;
mod system_menu;

use super::*;
use crate::state;
use crate::keybinds;
use crate::server::event;
use crate::util::*;
use crate::render;
use chrono::prelude::*;
use chrono;
use std::collections::VecDeque;

pub struct Notification {
    pub id: u32,
    pub icon: ResourceKey<'static>,
    pub title: String,
    pub description: ui::Node,
    pub ui: Option<ui::Node>,
    pub closable: bool,
    pub keep_reason: KeepReason,

    pub index: f64,
    pub target_index: f64,
    pub initial_done: bool,
    pub fly_in_done: bool,
}

pub enum KeepReason {
    /// Keep until dismissed
    None,
    /// Keep until this entity doesn't exist or isn't
    /// owned by the player
    EntityOwned(Entity),
    /// Keep until a room is active
    RoomActive(RoomId),
}

/// The base state of a running game.
///
/// When this state is removed the game instance is removed
pub struct BaseState {
    instance: Option<GameInstance>,
    hud: Option<ui::Node>,
    notification_window: Option<ui::Node>,
    current_money: UniDollar,
    // Float used for lerping
    current_rating: f64,
    first_frame: bool,
    highlighted_entity: Option<Entity>,
    highlighted_room: Option<HighlightedRoom>,
    mouse_pos: (i32, i32),
    fly_queue: VecDeque<(ResourceKey<'static>, ui::Node)>,
    current_fly: Option<(ui::Node, ui::Node)>,
}

#[derive(Clone)]
struct HighlightedRoom {
    id: RoomId,
    time: f64,
    showed: bool,
    requested: Option<network::RequestTicket<player::RoomDetails>>,
    content: Option<ui::Node>,
    loc: (i32, i32),
}

impl BaseState {
    /// Creates a new base state that will use the passed game instance
    /// at the active game.
    pub fn new(instance: GameInstance) -> BaseState {
        BaseState {
            instance: Some(instance),
            hud: None,
            notification_window: None,
            current_money: UniDollar(0),
            current_rating: 0.0,
            first_frame: true,
            highlighted_entity: None,
            highlighted_room: None,
            mouse_pos: (0, 0),
            fly_queue: VecDeque::new(),
            current_fly: None,
        }
    }
}

impl state::State for BaseState {
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(BaseState {
            instance: None,
            hud: None,
            notification_window: None,
            current_money: self.current_money,
            current_rating: self.current_rating,
            first_frame: true,
            highlighted_entity: self.highlighted_entity,
            highlighted_room: self.highlighted_room.clone(),
            mouse_pos: self.mouse_pos,
            fly_queue: VecDeque::new(),
            current_fly: None,
        })
    }

    fn takes_focus(&self) -> bool { true }

    fn added(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) -> state::Action {
        use crate::server::level::ObjectPlacementAction::WallFlag;

        *instance = self.instance.take();
        let instance = assume!(state.global_logger, instance.as_mut());
        state.renderer.set_level(&instance.level);
        state.renderer.set_camera(instance.level.width as f32 / 2.0, instance.level.height as f32 / 2.0);
        state.ui_manager.set_script_engine(&state.audio, &instance.scripting);
        for pack in state.asset_manager.get_packs() {
            instance.scripting.init_pack(pack.module());
        }

        state.keybinds.add_collection(keybinds::KeyCollection::Game);

        // Focus the camera on the first building owned by us
        {
            let rooms = instance.level.rooms.borrow();
            let target = rooms.room_ids()
                .map(|v| rooms.get_room_info(v))
                // Select rooms that we own
                .filter(|v| v.owner == instance.player.id)
                // Collect objects
                .flat_map(|v| &v.objects)
                // Filter the empty object slots
                .filter_map(|v| v.as_ref())
                // Get the creation actions
                .map(|v| &v.0)
                // For doors
                .filter(|v| v.key.resource().starts_with("door"))
                .flat_map(|v| &v.actions.0)
                //  Find the door tile action and get its location and direction
                .filter_map(|v| if let WallFlag{flag: level::object::WallPlacementFlag::Door, location, direction} = *v {
                    Some((location, direction))
                } else { None })
                .next();
            if let Some((loc, dir)) = target {
                // Focus on the door
                let (ox, oy) = dir.offset();
                state.renderer.suggest_camera_position(
                    loc.x as f32 + 0.5 + ox as f32 * 0.5,
                    loc.y as f32 + 0.5 + oy as f32 * 0.5,
                    120.0,
                );
            }
        }

        match instance.player.state {
            player::State::BuildRoom{..} => {
                state::Action::Push(Box::new(build::FinalizePlacement::new()))
            },
            player::State::EditRoom{active_room} => {
                let limited = instance.level.is_blocked_edit(active_room).is_err();
                if limited {
                    let mut room = instance.level.get_room_info_mut(active_room);
                    room.limited_editing = true;
                }
                state::Action::Push(Box::new(build::BuildRoom::new(limited)))
            }
            _ => state::Action::Nothing,
        }
    }

    fn active(&mut self, _instance: &mut Option<GameInstance>, state: &mut crate::GameState) -> state::Action {
        self.hud = Some(state.ui_manager.create_node(ResourceKey::new("base", "hud")));
        self.first_frame = true;
        state.renderer.set_mouse_sprite(ResourceKey::new("base", "ui/cursor/normal"));
        state.audio.set_playlist("game");
        state::Action::Nothing
    }

    fn inactive(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) {
        let instance = assume!(state.global_logger, instance.as_mut());
        for not in &mut instance.notifications {
            not.description.parent().map(|v| v.remove_child(not.description.clone()));
            if not.ui.is_some() {
                not.ui = None;
            }
        }
        if let Some(hud) = self.hud.take() {
            state.ui_manager.remove_node(hud);
        }
        self.current_fly = None;
        self.current_money = UniDollar(0);
        self.current_rating = 0.0;
    }

    fn removed(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) {
        use std::mem;
        state.ui_manager.clear_script_engine(&state.audio);

        let (shutdown_waiter, screenshot, mut entities) = {
            let mut instance = assume!(state.global_logger, instance.take());
            (
                instance.shutdown_waiter.take(),
                instance.screenshot_helper.take(),
                mem::replace(&mut instance.entities, ecs::Container::new()),
            )
        };

        // The server is most likely going to be waiting for a screenshot
        // in order to do the final save
        if let Some(scr) = screenshot.as_ref() {
            let (width, height) = state.window.drawable_size();
            state.renderer.tick(Some(&mut entities), None, state.delta, width, height);

            let screenshot = crate::take_screenshot(&state.global_logger, width, height);
            let _ = scr.reply.send(screenshot.clone());
            // HACK: In case of an autosave at the same time send two copies
            let _ = scr.reply.send(screenshot);
        }

        if let Some(waiter) = shutdown_waiter {
            if let Err(err) = waiter.recv_timeout(time::Duration::from_secs(60)) {
                error!(state.global_logger, "Failed to wait for server shutdown: {:?}", err);
            }
        }
        state.keybinds.remove_collection(keybinds::KeyCollection::Game);
    }

    fn tick(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) -> state::Action {
        use rand::{thread_rng, Rng};
        let instance = assume!(state.global_logger, instance.as_mut());

        let hud = assume!(state.global_logger, self.hud.clone());

        if instance.player.money != self.current_money || self.first_frame {
            self.current_money = instance.player.money;
            if let Some(text) = query!(hud, stats > money > @text)
                .next()
            {
                text.set_text(format!("Money: {}", instance.player.money));
            }
        }

        if instance.player.first_set {
            self.first_frame = true;
            instance.player.first_set = false;
        }
        if instance.player.rating != self.current_rating as i16 || self.first_frame {
            if self.first_frame {
                self.current_rating = f64::from(instance.player.rating)
            } else {
                self.current_rating += (f64::from(instance.player.rating) - self.current_rating).signum() * state.delta;
            }
            if let Some(rating_bar) = query!(hud, stats > rating > rating_clip)
                .next()
            {
                rating_bar.set_property("value", (self.current_rating + 30000.0) / 60000.0);
            }
        }
        self.first_frame = false;
        let mut rng = thread_rng();

        for (i, not) in instance.notifications.iter_mut().enumerate() {
            if not.ui.is_none() {
                not.index = i as f64;
                if let Some(target) = query!(hud, notifications)
                    .next()
                {
                    let (r, g, b) = hsl_to_rgb(rng.gen_range(0.0, 1.0), 1.0, 0.8);
                    let id = not.id;
                    let ui = node!{
                        notification(index = i as f64, slide_in=-50.0, r=i32::from(r), g=i32::from(g), b=i32::from(b), id=id as i32) {
                            background
                            content {
                                icon(img=not.icon.as_string().to_owned())
                                title {
                                    @text(not.title.clone())
                                }
                            }
                        }
                    };
                    ui.set_property("on_click", ui::MethodDesc::<ui::MouseUpEvent>::native(move |evt, _, _| {
                        evt.emit(ClickNotification(id));
                        true
                    }));
                    target.add_child_first(ui.clone());
                    if not.initial_done {
                        ui.set_property("slide_in", 0.0f64);
                    }
                    if !not.fly_in_done {
                        self.fly_queue.push_back((not.icon.clone(), ui.clone()));
                        not.fly_in_done = true;
                    }
                    not.ui = Some(ui);
                }
            }

            not.target_index = i as f64;
            if let Some(ui) = not.ui.as_ref() {
                if !not.initial_done {
                    let cur_slide = ui.get_property::<f64>("slide_in").unwrap_or(0.0);
                    if cur_slide >= -1.0 {
                        ui.set_property("slide_in", 0.0f64);
                        not.initial_done = true;
                    } else {
                        ui.set_property("slide_in", cur_slide + state.delta);
                    }
                }
                let diff = (not.index - not.target_index).abs();
                if diff > 0.001 {
                    if diff <= 0.1 {
                        ui.set_property("index", not.index);
                    } else {
                        not.index += (not.target_index - not.index).signum() * state.delta * 0.03;
                        ui.set_property("index", not.index);
                    }
                }
            }
            match not.keep_reason {
                KeepReason::None | KeepReason::RoomActive(_) => {},
                KeepReason::EntityOwned(e) => {
                    let player_id = instance.player.id;
                    if !instance.entities.is_valid(e) || !instance.entities.get_component::<Owned>(e).map_or(false, |v| v.player_id == player_id) {
                        state.ui_manager.events.borrow_mut().emit(CloseNotification(not.id));
                    }
                },
            }
        }

        // Fly-in notifications
        if self.current_fly.is_none() {
            if let Some(next) = self.fly_queue.pop_front() {
                if let Some(target) = query!(hud, notifications_fly_in).next() {
                    let node = node! {
                        notification_icon(img=next.0.as_string().to_owned(), time = 0.0, target_index=next.1.get_property::<i32>("index").unwrap_or(0))
                    };
                    target.add_child(node.clone());
                    self.current_fly = Some((node, next.1));
                }
            }
        }
        if let Some(fly) = self.current_fly.take() {
            fly.0.set_property("target_index", fly.1.get_property::<i32>("index").unwrap_or(0));
            if fly.0.get_property::<f64>("time").unwrap_or(1.0) < 1.0 {
                self.current_fly = Some(fly);
            } else {
                fly.0.parent().map(|v| v.remove_child(fly.0));
            }
        }

        if let Some(chat_area) = query!(hud, chat_area).next() {
            for message in instance.chat_messages.drain(..) {
                let msg = node!(chat_message);
                let mut last_color = "rgb(255, 255, 255)".to_owned();
                for part in message.parts {
                    match part {
                        MsgPart::Image(key) => {
                            msg.add_child(node!{
                                chat_image(img=key.as_string())
                            });
                        }
                        MsgPart::Text{text, color, special} => {
                            if let Some(col) = color {
                                last_color = format!("rgb({}, {}, {})", col.r, col.g, col.b);
                            }
                            let txt = ui::Node::new_text(text);
                            txt.set_property("color", last_color.clone());
                            txt.set_property("special", special);
                            msg.add_child(txt);
                        }
                    }
                }
                chat_area.add_child_first(msg);
            }
            for n in chat_area.children() {
                let pos = n.raw_position();
                if pos.y < 0 {
                    chat_area.remove_child(n);
                }
            }
        }

        if let State::EditRoom{..} | State::EditEntity{..} = instance.player.state {
        } else {
            if let Some(hroom) = self.highlighted_room.as_mut() {
                hroom.time += state.delta;
                if hroom.time > 60.0 * 0.35 && !hroom.showed && hroom.requested.is_none() {
                    // Request slightly before showing to hide any lag
                    hroom.requested = Some(instance.request_manager.request(player::RoomDetails {
                        room_id: hroom.id,
                    }));
                } else if hroom.time > 60.0 * 0.5 && !hroom.showed {
                    let cur = state.ui_manager.current_tooltip();
                    if cur.map_or(true, |v| v.starts_with("room_")) {
                        if let Some(room_info) = instance.level.try_room_info(hroom.id) {
                            let ty = assume!(state.global_logger,
                                instance.asset_manager.loader_open::<crate::server::level::room::Loader>(room_info.key.borrow())
                            );
                            if ty.allow_edit && room_info.owner == instance.player.id {
                                let content = hroom.content.take()
                                    .unwrap_or_else(|| node! {
                                        content {
                                            @text(ty.name.clone())
                                        }
                                    });
                                state.ui_manager.show_tooltip(&format!("room_{:?}", hroom.id), content, self.mouse_pos.0, self.mouse_pos.1);
                            }
                        }
                    }
                    hroom.showed = true;
                }
            }
        }

        // TODO: Optimize?
        if let Some(day) = query!(hud, day > @text).next() {
            let sim_day = NaiveDate::from_ymd(2018, 1, 1) + chrono::Duration::days(i64::from(instance.day_tick.day));
            day.set_text(format!("{}{} {}", sim_day.format("%B %e"), match sim_day.day() {
                1 | 21 | 31 => "st",
                2 | 22 => "nd",
                3 | 23 => "rd",
                _ => "th",
            }, sim_day.format("%a")));
        }
        if let Some(period) = query!(hud, period > @text).next() {
            let time = instance.day_tick.current_tick;
            let activity_slot = (time / LESSON_LENGTH) as usize;
            period.set_text(format!("{}{} Period", activity_slot + 1, match activity_slot + 1 {
                1 => "st",
                2 => "nd",
                3 => "rd",
                _ => "th",
            }));
        }
        if let Some(time) = query!(hud, time > @text).next() {
            let tick = instance.day_tick.current_tick;
            // 2 Hours per a lesson, scale the tick to that
            let minutes = (tick * 2 * 60 * NUM_TIMETABLE_SLOTS as i32) / (LESSON_LENGTH * NUM_TIMETABLE_SLOTS as i32);
            time.set_text(format!("{:02}:{:02}", 8 + (minutes / 60), minutes % 60))
        }

        state::Action::Nothing
    }

    fn ui_event(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState, evt: &mut event::EventHandler) -> state::Action {
        let mut action = state::Action::Nothing;
        let instance = assume!(state.global_logger, instance.as_mut());
        evt.handle_event::<super::OpenBuyRoomMenu, _>(|_| {
            action = state::Action::Toggle(Box::new(BuyRoomState::new()));
        });
        evt.handle_event::<super::OpenBuyStaffMenu, _>(|_| {
            action = state::Action::Toggle(Box::new(BuyStaffState::new()));
        });
        evt.handle_event::<super::OpenStaffListMenu, _>(|_| {
            action = state::Action::Toggle(Box::new(StaffListState::new()));
        });
        evt.handle_event::<super::OpenStatsMenu, _>(|_| {
            action = state::Action::Toggle(Box::new(StatsState::new()));
        });
        evt.handle_event::<super::OpenSettingsMenu, _>(|_| {
            action = state::Action::Toggle(Box::new(SettingsState::new()));
        });
        evt.handle_event::<super::OpenEditRoom, _>(|_| {
            action = state::Action::Toggle(Box::new(EditRoomState::new()));
        });
        evt.handle_event::<super::OpenCoursesMenu, _>(|_| {
            action = state::Action::Toggle(Box::new(CourseList::new()));
        });
        if let Some(hroom) = self.highlighted_room.as_mut() {
            if let Some(req) = hroom.requested {
                network::RequestManager::handle_reply(evt, req, |pck| {
                    if hroom.id == pck.room_id {
                        let ty = if let Some(room_info) = instance.level.try_room_info(hroom.id) {
                            assume!(state.global_logger,
                                instance.asset_manager.loader_open::<crate::server::level::room::Loader>(room_info.key.borrow())
                            )
                        } else {
                            return;
                        };
                        if let Some(script) = ty.controller.as_ref() {
                            hroom.showed = false;
                            match instance.scripting.with_borrows()
                                .borrow_mut(&mut instance.level)
                                .borrow_mut(&mut instance.entities)
                                .invoke_function::<_, Ref<ui::NodeRef>>("invoke_module_method", (
                                    Ref::new_string(&instance.scripting, script.module()),
                                    Ref::new_string(&instance.scripting, script.resource()),
                                    Ref::new_string(&instance.scripting, "decode_tooltip"),
                                    Ref::new(&instance.scripting, pck.data.0),
                                )) {
                                Ok(val) => {
                                    hroom.content = Some(val.0.clone());
                                },
                                Err(err) => {
                                    error!(state.global_logger, "Failed to decode room tooltip"; "script" => ?script, "error" => % err, "room" => %ty.name);
                                },
                            };
                        }
                    }
                });
            }
        }
        evt.handle_event::<ClickNotification, _>(|c| {
            for not in &instance.notifications {
                not.description.parent().map(|v| v.remove_child(not.description.clone()));
            }
            if let Some(not) = instance.notifications.iter_mut()
                .find(|v| v.id == c.0)
            {
                if let Some(old) = self.notification_window.take() {
                    old.parent().map(|v| v.remove_child(old));
                }
                let window = node! {
                    full_center {
                        window(
                            width=500, height=300
                        ) {
                            content(style="notification_window".to_owned(), id=c.0 as i32) {
                                title {
                                    @text(not.title.as_str())
                                }
                            }
                        }
                    }
                };
                if let Some(content) = query!(window, window > content).next() {
                    content.add_child(not.description.clone());

                    let id = not.id;
                    let closable = not.closable;

                    let btn = node! {
                        button {
                            content {
                                @text(if closable {
                                    "Dismiss"
                                } else {
                                    "Close"
                                })
                            }
                        }
                    };
                    btn.set_property("on_click", ui::MethodDesc::<ui::MouseUpEvent>::native(move |evt, _, _| {
                        if closable {
                            evt.emit(CloseNotification(id));
                        }
                        evt.emit(CloseNotificationWindow);
                        true
                    }));
                    content.add_child(btn);
                }
                self.notification_window = Some(window.clone());
                state.ui_manager.add_node(window);
            }
        });
        evt.handle_event::<CloseNotification, _>(|c| {
            instance.notifications.retain(|v| if v.id == c.0 {
                if let Some(ui) = v.ui.clone() {
                    if let Some(target) = query!(assume!(state.global_logger, self.hud.as_ref()), notifications)
                        .next()
                    {
                        target.remove_child(ui);
                    }
                }
                false
            } else {
                true
            });
        });
        evt.handle_event::<CloseNotificationWindow, _>(|_| {
            for not in &instance.notifications {
                not.description.parent().map(|v| v.remove_child(not.description.clone()));
            }
            if let Some(old) = self.notification_window.take() {
                old.parent().map(|v| v.remove_child(old));
            }
        });
        evt.handle_event::<ChatMessage, _>(|ChatMessage(msg)| {
            match msg.as_str() {
                "/prebuild" => state.renderer.rebuild_pipeline(),
                "/crashme" => panic!("Forced crash"),
                _ => {
                    let _ = instance.ensure_send(packet::ChatMessage {
                        message: msg,
                    });
                }
            };
        });
        action
    }

    fn key_action(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState, action: keybinds::KeyAction, mouse_pos: (i32, i32)) -> state::Action {
        use crate::keybinds::KeyAction::*;

        let instance = assume!(state.global_logger, instance.as_mut());

        let hud = assume!(state.global_logger, self.hud.clone());

        match action {
            SystemMenu => {
                return state::Action::Push(Box::new(system_menu::SystemMenu::new()));
            },
            BeginChat => {
                if query!(hud, textbox(id="chat_sendbox")).next().is_none() {
                    let txt = node!(
                        textbox(id="chat_sendbox".to_owned()) {
                            content {
                                @text("")
                            }
                        }
                    );
                    let logger = state.global_logger.clone();
                    txt.set_property("on_key", ui::MethodDesc::<ui::KeyUpEvent>::native(move |evts, node, param| {
                        if param.input == ::sdl2::keyboard::Keycode::Return {
                            if let Some(txt) = query!(node, content > @text).next() {
                                evts.emit(ChatMessage(assume!(logger, txt.text()).to_owned()));
                            }
                            node.parent().map(|v| v.remove_child(node));
                            true
                        } else {
                            false
                        }
                    }));
                    hud.add_child_first(txt);
                }
            },
            InspectMember => {
                if let Some(entity) = find_entity_at(&state.renderer, &mut instance.entities, mouse_pos) {
                    if !instance.entities.get_component::<Owned>(entity).map_or(false, |v| v.player_id == instance.player.id) {
                        return state::Action::Nothing;
                    }

                    if instance.entities.get_component::<Paid>(entity).is_some()
                        || instance.entities.get_component::<StudentController>(entity).is_some() {
                        // Student
                        return state::Action::Push(Box::new(EntityInfoState::new(entity)));
                    }
                }
            },
            _ => {},
        }
        state::Action::Nothing
    }

    fn mouse_move_ui(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState,  _mouse_pos: (i32, i32)) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());
        if let Some(entity) = self.highlighted_entity.take() {
            instance.entities.remove_component::<entity::Highlighted>(entity);
        }
        if let Some(room) = self.highlighted_room.take() {
            state.ui_manager.hide_tooltip(&format!("room_{:?}", room.id));
        }
        state::Action::Nothing
    }

    fn mouse_move(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState,  mouse_pos: (i32, i32)) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());
        self.mouse_pos = mouse_pos;

        // TODO: Tidy this up
        //       Its really messy
        if let Some(entity) = find_entity_at(&state.renderer, &mut instance.entities, mouse_pos) {
            let ty = {
                instance.entities.get_component::<Living>(entity)
                    .and_then(|v| state.asset_manager.loader_open::<Loader<entity::ClientComponent>>(v.key.borrow()).ok())
            };
            if let Some(highlight) = ty.as_ref().and_then(|v| v.highlight.as_ref()) {
                if self.highlighted_entity != Some(entity) {
                    if let Some(entity) = self.highlighted_entity.take() {
                        instance.entities.remove_component::<entity::Highlighted>(entity);
                    }
                } else {
                    return state::Action::Nothing;
                }
                if let Some(HighlightedRoom{id: room, showed: true, ..}) = self.highlighted_room.take() {
                    state.ui_manager.hide_tooltip(&format!("room_{:?}", room));
                }
                instance.entities.add_component(entity, entity::Highlighted {
                    color: highlight.color,
                });
                if let Some(label) = highlight.label.as_ref() {
                    let can_interact = instance.entities.get_component::<Owned>(entity).map_or(false, |v| v.player_id == instance.player.id)
                        && (instance.entities.get_component::<Paid>(entity).is_some()
                            || instance.entities.get_component::<StudentController>(entity).is_some());
                    let content = node! {
                        content {
                            @text(label.clone())
                        }
                    };
                    if can_interact {
                        content.add_child(ui::Node::new_text("\n"));
                        let buttons = state.keybinds.keys_for_action(keybinds::KeyAction::InspectMember)
                            .map(|(_, btn)| format!("<{}>", btn.as_string()))
                            .collect::<Vec<_>>();
                        let btn = ui::Node::new_text(buttons.join(", "));
                        btn.set_property("key_btn", true);
                        content.add_child(btn);
                        content.add_child(ui::Node::new_text(" - Inspect"));
                    }
                    state.ui_manager.show_tooltip(&format!("entity_{:?}", entity), content, mouse_pos.0, mouse_pos.1);
                }
                self.highlighted_entity = Some(entity);
            }
        } else {
            if let Some(entity) = self.highlighted_entity.take() {
                instance.entities.remove_component::<entity::Highlighted>(entity);
                state.ui_manager.hide_tooltip(&format!("entity_{:?}", entity));
            }

            let (lx, ly) = state.renderer.mouse_to_level(mouse_pos.0, mouse_pos.1);
            if let Some(room) = instance.level.get_room_owner(Location::new(lx as i32, ly as i32)) {
                if let Some(hroom) = self.highlighted_room.as_mut() {
                    if hroom.id == room {
                        let dx = hroom.loc.0 - mouse_pos.0;
                        let dy = hroom.loc.1 - mouse_pos.1;
                        if dx*dx + dy*dy > 10*10 {
                            hroom.time = 0.0;
                            if hroom.showed {
                                state.ui_manager.hide_tooltip(&format!("room_{:?}", hroom.id));
                            }
                            hroom.showed = false;
                            hroom.requested = None;
                            hroom.loc = mouse_pos;
                        }
                        state.ui_manager.move_tooltip(&format!("room_{:?}", hroom.id), mouse_pos.0, mouse_pos.1);
                        return state::Action::Nothing;
                    }
                    state.ui_manager.hide_tooltip(&format!("room_{:?}", hroom.id));
                }
                self.highlighted_room = Some(HighlightedRoom {
                    id: room,
                    time: 0.0,
                    showed: false,
                    requested: None,
                    loc: mouse_pos,
                    content: None,
                });
            } else if let Some(room) = self.highlighted_room.take() {
                state.ui_manager.hide_tooltip(&format!("room_{:?}", room.id));
            }
        }

        state::Action::Nothing
    }
}

struct ChatMessage(String);
pub(crate) struct ClickNotification(pub(crate) u32);
pub(crate) struct CloseNotification(pub(crate) u32);
pub(crate) struct CloseNotificationWindow;

fn find_entity_at(renderer: &render::Renderer, entities: &mut Container, pos: (i32, i32)) -> Option<Entity> {
    let ray = renderer.get_mouse_ray(pos.0, pos.1);

    entities.with(|em: EntityManager<'_>,
        living: ecs::Read<Living>,
        position: ecs::Read<Position>,
        size: ecs::Read<Size>
    | {
        em.group_mask((&position, &size), |m| m.and(&living))
            .find(|&(_e, (pos, size))| {
                let vp = cgmath::Vector3::new(pos.x, pos.y, pos.z);
                let bound = AABB {
                    min: vp - cgmath::Vector3::new(size.width / 2.0, 0.0, size.depth / 2.0),
                    max: vp + cgmath::Vector3::new(size.width / 2.0, size.height, size.depth / 2.0),
                };

                bound.intersects_ray(ray)
            })
            .map(|v| v.0)
    })
}

struct CloseOtherInfos(ui::Node);