
use super::super::*;
use crate::state;
use crate::server::event;
use crate::server::assets;
use crate::server::network;
use crate::prelude::*;


pub struct EntityInfoState {
    ui: Option<ui::Node>,

    target: ecs::Entity,
    request_ticket: Option<network::RequestTicket<player::EntityResults>>,
    next_request: i32,

    timetable_info: Option<Vec<player::TimetableEntryState>>,
    grades_info: Vec<player::NamedGradeEntry>,
    stats_lerp: f64,
    stats: [(f32, f32); Stats::MAX],
    variant: Option<StatVariant>,

    fire_event: Option<mpsc::Receiver<ui::prompt::ConfirmResponse>>,
    // Always `Overview` for staff members
    current_tab: Tab,
}

#[derive(Clone, Copy, Ord, PartialOrd, Eq, PartialEq)]
enum Tab {
    Overview,
    History,
}

impl Tab {
    fn all() -> &'static [Tab] {
        &[Tab::Overview, Tab::History]
    }

    fn index(self) -> usize {
        match self {
            Tab::Overview => 0,
            Tab::History => 1,
        }
    }
    fn image(self) -> &'static str {
        match self {
            Tab::Overview => "base:ui/page_tab_overview",
            Tab::History => "base:ui/page_tab_history",
        }
    }
}

impl EntityInfoState {
    pub fn new(target: ecs::Entity) -> EntityInfoState {
        EntityInfoState {
            ui: None,
            target,
            request_ticket: None,
            next_request: 0,
            timetable_info: None,
            grades_info: Vec::new(),
            stats_lerp: 1.0,
            stats: [(0.0, 0.0); Stats::MAX],
            variant: None,
            fire_event: None,
            current_tab: Tab::Overview,
        }
    }
}

impl state::State for EntityInfoState {
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(EntityInfoState {
            ui: self.ui.clone(),
            target: self.target,
            request_ticket: self.request_ticket,
            next_request: self.next_request,
            timetable_info: self.timetable_info.clone(),
            grades_info: self.grades_info.clone(),
            stats_lerp: self.stats_lerp,
            stats: self.stats,
            variant: self.variant,
            fire_event: None,
            current_tab: self.current_tab,
        })
    }

    fn added(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());

        let id = assume!(state.global_logger, instance.entities.get_component::<NetworkId>(self.target).map(|v| v.0));
        self.request_ticket = Some(instance.request_manager.request(player::EntityResults {
            entity_id: id,
        }));
        state::Action::Nothing
    }

    fn active_req(&mut self, req: &mut state::CaptureRequester, instance: &mut Option<GameInstance>, state: &mut crate::GameState) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());

        // Handle the reply from a prompt if any
        if let Some(ui::prompt::ConfirmResponse::Accept) = self.fire_event.take()
            .and_then(|v| v.recv().ok())
        {
            let mut cmd: Command = FireStaff::new(
                assume!(state.global_logger, instance.entities.get_component::<NetworkId>(self.target).map(|v| v.0))
            ).into();
            let mut proxy = super::GameProxy::proxy(state);

            try_cmd!(instance.log, cmd.execute(&mut proxy, &mut instance.player, CommandParams {
                log: &instance.log,
                level: &mut instance.level,
                engine: &instance.scripting,
                entities: &mut instance.entities,
                snapshots: &instance.snapshots,
                mission_handler: instance.mission_handler.as_ref().map(|v| v.borrow()),
            }), {
                instance.push_command(cmd, req);
                proxy.state.audio
                    .controller
                    .borrow_mut()
                    .play_sound(ResourceKey::new("base", "slapsound"));
                return state::Action::Pop;
            });
        }

        self.variant = instance.entities.get_component::<Living>(self.target)
            .and_then(|v| state.asset_manager.loader_open::<Loader<ServerComponent>>(v.key.borrow()).ok())
            .map(|v| entity_variant(&*v));
        let ui = state.ui_manager.create_node(assets::ResourceKey::new("base", if self.variant == Some(Stats::STUDENT) {
            "manage/student_info"
        } else {
            "manage/staff_info"
        }));
        self.ui = Some(ui.clone());

        #[cfg(feature = "debugutil")]
        {
            if let Some(content) = query!(ui, window > content).next() {
                content.add_child(node!{
                    debug_local {
                        @text("Local:")
                    }
                });
                content.add_child(node!{
                    debug_remote {
                        @text("Remote:")
                    }
                });
            }
        }

        rebuild_page(
            instance,
            state,
            &ui,
            self.current_tab,
            &self.timetable_info,
            &self.grades_info,
            &self.stats,
            self.variant,
        );

        let living = assume!(state.global_logger, instance.entities.get_component::<Living>(self.target));

        if let Some(name) = query!(ui, name > @text).next() {
            name.set_text(format!("{} {}", living.name.0, living.name.1));
        }
        if let Some(btn) = query!(ui, button(id="locate")).next() {
            btn.set_property("on_click", ui::MethodDesc::<ui::MouseUpEvent>::native(|evt, _, _| {
                evt.emit(FocusEntity);
                true
            }));
        }
        if let Some(btn) = query!(ui, button(id="move")).next() {
            btn.set_property("on_click", ui::MethodDesc::<ui::MouseUpEvent>::native(|evt, _, _| {
                evt.emit(MoveEntity);
                true
            }));
        }
        if let Some(btn) = query!(ui, button(id="fire")).next() {
            btn.set_property("on_click", ui::MethodDesc::<ui::MouseUpEvent>::native(|evt, _, _| {
                evt.emit(FireEntity);
                true
            }));
        }
        for t in query!(ui, timetable > timetable_entry > @text).matches() {
            t.set_text("Loading...");
        }
        state.ui_manager.events().emit(super::CloseOtherInfos(ui));
        state.ui_manager.events().emit(FocusEntity);

        state::Action::Nothing
    }

    fn tick(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) -> state::Action {
        if let Tab::Overview = self.current_tab {
            self.stats_lerp = (self.stats_lerp + state.delta * 0.05).min(1.0);
            let ui = assume!(state.global_logger, self.ui.clone());
            let stats: Vec<_> = query!(ui, skills > skill).matches().collect();
            for (n, &(base, target)) in stats.into_iter().rev().zip(self.stats.iter()) {
                if let Some(skill_bar) = query!(n, skill_clip).next() {
                    let cur = skill_bar.get_property::<f64>("value").unwrap_or(0.0);
                    let val = f64::from(base) + f64::from(target - base) * self.stats_lerp;
                    if (cur - val).abs() > 0.001 {
                        skill_bar.set_property("value", val as f64);
                    }
                }
            }
        }

        if self.request_ticket.is_some() {
            return state::Action::Nothing;
        }
        let instance = assume!(state.global_logger, instance.as_mut());
        self.next_request -= 1;
        if self.next_request <= 0 {
            let id = assume!(state.global_logger, instance.entities.get_component::<NetworkId>(self.target).map(|v| v.0));
            self.request_ticket = Some(instance.request_manager.request(player::EntityResults {
                entity_id: id,
            }));
            state::Action::Nothing
        } else {
            state::Action::Nothing
        }
    }

    fn inactive(&mut self, _instance: &mut Option<GameInstance>, state: &mut crate::GameState) {
        if let Some(ui) = self.ui.take() {
            state.ui_manager.remove_node(ui);
        }
    }

    fn ui_event_req(&mut self, req: &mut state::CaptureRequester, instance: &mut Option<GameInstance>, state: &mut crate::GameState, evt: &mut event::EventHandler) -> state::Action {
        let mut action = state::Action::Nothing;
        let ui = assume!(state.global_logger, self.ui.clone());
        let instance = assume!(state.global_logger, instance.as_mut());
        evt.handle_event_if::<super::CancelEvent, _, _>(|evt| evt.0.is_same(&ui), |_| {
            action = state::Action::Pop;
        });
        evt.inspect_event::<super::CloseOtherInfos, _>(|evt| {
            if !evt.0.is_same(&ui) {
                action = state::Action::Pop;
            }
        });
        evt.handle_event::<FocusRoom, _>(|FocusRoom(room)| {
            let room = instance.level.get_room_info(room);
            state.renderer.suggest_camera_position(
                room.area.min.x as f32 + room.area.width() as f32 / 2.0,
                room.area.min.y as f32 + room.area.height() as f32 / 2.0,
                45.0
            );
        });
        evt.handle_event::<FocusEntity, _>(|_| {
            if instance.entities.is_valid(self.target) {
                let pos = if let Some(pos) = instance.entities.get_component::<Position>(self.target) {
                    pos
                } else { return };
                state.renderer.suggest_camera_position(pos.x, pos.z, 45.0);
            }
        });
        evt.handle_event::<ChangeTab, _>(|c| {
            self.current_tab = c.0;

            rebuild_page(
                instance,
                state,
                &ui,
                self.current_tab,
                &self.timetable_info,
                &self.grades_info,
                &self.stats,
                self.variant,
            );
        });
        evt.handle_event::<MoveEntity, _>(|_| {
            if instance.entities.is_valid(self.target) {

                let mut cmd: Command = StartMoveStaff::new(
                    assume!(state.global_logger, instance.entities.get_component::<NetworkId>(self.target).map(|v| v.0))
                ).into();
                let mut proxy = super::GameProxy::proxy(state);

                try_cmd!(instance.log, cmd.execute(&mut proxy, &mut instance.player, CommandParams {
                    log: &instance.log,
                    level: &mut instance.level,
                    engine: &instance.scripting,
                    entities: &mut instance.entities,
                    snapshots: &instance.snapshots,
                    mission_handler: instance.mission_handler.as_ref().map(|v| v.borrow()),
                }), {
                    instance.push_command(cmd, req);
                    action = state::Action::Switch(Box::new(
                        super::PlaceStaffState::new(None, false)
                    ));
                });
            }
        });
        evt.handle_event::<FireEntity, _>(|_| {
            if instance.entities.is_valid(self.target) {
                let (send, recv) = mpsc::channel();
                self.fire_event = Some(recv);
                action = state::Action::Push(Box::new(ui::prompt::Confirm::new(
                    ui::prompt::ConfirmConfig {
                        description: "Are you sure you wish to fire this staff member?".into(),
                        accept: "Fire".into(),
                        ..ui::prompt::ConfirmConfig::default()
                    },
                    move |rpl| {
                        let _ = send.send(rpl);
                    }
                )));
            }
        });
        if let Some(req) = self.request_ticket {
            let id = assume!(state.global_logger, instance.entities.get_component::<NetworkId>(self.target).map(|v| v.0));
            network::RequestManager::handle_reply(evt, req, |res| {
                if id != res.entity_id {
                    return;
                }
                self.request_ticket = None;
                self.next_request = 20 * 5;
                self.timetable_info = res.timetable.map(|v| v.0);
                self.grades_info = res.grades.0;

                rebuild_page(
                    instance,
                    state,
                    &ui,
                    self.current_tab,
                    &self.timetable_info,
                    &self.grades_info,
                    &self.stats,
                    self.variant,
                );

                let skills: Vec<_> = query!(ui, skills > skill).matches().collect();
                if skills.is_empty() {
                    for (idx, f) in res.stats.0.iter().enumerate() {
                        self.stats[idx] = (
                            *f,
                            *f
                        );
                    }
                } else {
                    for (idx, (fui, f)) in skills.into_iter().rev().zip(res.stats.0.iter()).enumerate()  {
                        if let Some(skill_bar) = query!(fui, skill > skill_clip).next() {
                            self.stats[idx] = (
                                skill_bar.get_property::<f64>("value").unwrap_or(0.0) as f32,
                                *f
                            );
                        }
                    }

                }
                self.stats_lerp = 0.0;

                #[cfg(feature = "debugutil")]
                {
                    if let Some(content) = query!(ui, window > content).next() {
                        if let Some(local) = query!(content, debug_local > @text).next() {
                            let ani = instance.entities.get_component::<entity::AnimatedModel>(self.target);
                            local.set_text(format!("Local: {:?} {:?}", self.target, ani.map_or(&[] as &[_], |v| &*v.animation_queue)));
                        }
                        if let Some(remote) = query!(content, debug_remote > @text).next() {
                            remote.set_text(format!("Remote: {}", res.entity_debug))
                        }
                    }
                }
            });
        }
        action
    }

    fn key_action(&mut self, _instance: &mut Option<GameInstance>, _state: &mut crate::GameState, action: keybinds::KeyAction, _mouse_pos: (i32, i32)) -> state::Action {
        use crate::keybinds::KeyAction::*;

        match action {
            SystemMenu => state::Action::Pop,
            _ => state::Action::Nothing,
        }
    }

    fn can_have_duplicates(&self) -> bool { true }
}

fn rebuild_page(
    _instance: &mut GameInstance,
    state: &mut crate::GameState,
    ui: &ui::Node,
    tab: Tab,
    timetable: &Option<Vec<player::TimetableEntryState>>,
    grades: &[player::NamedGradeEntry],
    stats: &[(f32, f32); Stats::MAX],
    variant: Option<StatVariant>,
) {
    let cont = assume!(state.global_logger, query!(ui, window > content).next());
    for c in cont.children() {
        let name = c.name();
        match name.as_ref().map(|v| v.as_str()) {
            Some("tabs_inside")
            | Some("title")
            | Some("name")
            | Some("buttons")
            | Some("debug_local")
            | Some("debug_remote") => {},
            _ => {
                cont.remove_child(c);
            }
        }

    }

    if let Some(outside) = query!(ui, tabs_outside).next() {
        for t in outside.children() {
            outside.remove_child(t);
        }
        for t in Tab::all() {
            if *t != tab {
                let node = node! {
                    page_tab(index=t.index() as i32, tab=t.image().to_owned())
                };
                node.set_property("on_click", ui::MethodDesc::<ui::MouseUpEvent>::native(move |evt, _, _| {
                    evt.emit(ChangeTab(*t));
                    true
                }));
                outside.add_child(node);
            }
        }
    }
    if let Some(inside) = query!(ui, tabs_inside).next() {
        for t in inside.children() {
            inside.remove_child(t);
        }
        inside.add_child(node! {
            page_tab(index=tab.index() as i32, tab=tab.image().to_owned())
        });
    }

    match tab {
        Tab::Overview => {
            if variant == Some(Stats::STUDENT) {
                let tt = node! {
                    timetable {

                    }
                };

                if let Some(time) = timetable.as_ref() {
                    for (day, info) in super::courses::DAYS.iter().zip(time.chunks_exact(4)) {
                        let d = node! {
                            course_day {
                                center {
                                    label {
                                        @text(*day)
                                    }
                                }
                            }
                        };
                        for p in info {
                            let n = match p {
                                player::TimetableEntryState::Free => node!(course_period(booked=false)),
                                player::TimetableEntryState::Lesson => node!(course_period(booked=true)),
                                player::TimetableEntryState::Completed(grade) =>
                                    node!(course_period(booked=true, grade=grade.as_str().to_owned())),
                            };
                            d.add_child(n);
                        }

                        tt.add_child(d);
                    }
                } else {

                }
                cont.add_child(tt);
            }
            cont.add_child(node! {
                skills {
                    header {
                        @text("Stats")
                    }
                    seperator
                }
            });

            if let (Some(skills), Some(variant)) = (query!(ui, skills).next(), variant) {
                for (stat, val) in variant.stats().iter().zip(stats) {
                    let tooltip = stat.tooltip_string();
                    if tooltip == "*" && cfg!(not(feature = "debugutil")) {
                        continue;
                    }
                    let s = stat.as_string();
                    let title = fix_case(s);
                    let n = node!(skill(key=s.to_owned(), title = title.clone(), tooltip=tooltip.to_owned()) {
                        skill_clip(value=f64::from(val.1)) {
                            skill_bar
                        }
                        label {
                            @text(title)
                        }
                    });
                    skills.add_child(n);
                }
            }
        },
        Tab::History => {
            let history = node!(student_history);
            cont.add_child(history.clone());
            for entry in grades {
                history.add_child(node! {
                    history_entry {
                        name {
                            @text(entry.course_name.as_str())
                        }
                        grade {
                            @text(entry.grade.as_str())
                        }
                    }
                });
            }
        },
    }
}

pub (super) fn fix_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut first = true;
    for c in s.chars() {
        if c == '_' {
            out.push(' ');
            first = true;
            continue;
        }
        if first {
            out.extend(c.to_uppercase());
            first = false;
        } else {
            out.push(c);
        }
    }
    out
}

struct FocusEntity;
struct MoveEntity;
struct FireEntity;
struct FocusRoom(room::Id);
struct ChangeTab(Tab);