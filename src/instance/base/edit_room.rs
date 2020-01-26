
use super::super::*;
use crate::state;
use crate::server::event;
use crate::prelude::*;
use server::network;

pub struct EditRoomState {
    ui: Option<ui::Node>,
    last_room: Option<RoomId>,

    request: Option<(RoomId, network::RequestTicket<player::RoomBooked>)>,
}

impl EditRoomState {
    pub fn new() -> EditRoomState {
        EditRoomState {
            ui: None,
            last_room: None,
            request: None,
        }
    }
}

fn update_room_ui(log: &Logger, assets: &AssetManager, instance: &mut GameInstance, ui: &ui::Node, room_id: Option<RoomId>) {
    if let (Some(name), Some(limited)) = (query!(ui, room_name > @text).next(), query!(ui, limited > @text).next()) {
        if let Some(room_id) = room_id {
            let room = instance.level.get_room_info(room_id);
            let rinfo = assume!(log, assets.loader_open::<room::Loader>(room.key.borrow()));

            if !room.state.is_done() || room.owner != instance.player.get_uid() || !rinfo.allow_edit {
                name.set_text("Room: None");
                limited.set_text("");
            } else {
                name.set_text(format!("Room: {}", rinfo.name));
                if let Err(reason) = instance.level.is_blocked_edit(room_id) {
                    limited.set_text(format!("Limited editing - {}", reason));
                } else if !room.controller.is_invalid() && instance.entities.get_component::<entity::ClientBooked>(room.controller).is_some() {
                    limited.set_text("Limited editing - Room is booked for courses");
                } else {
                    limited.set_text("");
                }
            }
        } else {
            name.set_text("Room: None");
            limited.set_text("");
        }
    }
}

impl state::State for EditRoomState {
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(EditRoomState {
            ui: self.ui.clone(),
            last_room: self.last_room,
            request: None,
        })
    }

    fn added(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());
        if !instance.player.state.is_none() {
            // Can't place a room whilst one is already in progress
            return state::Action::Pop;
        }
        state::Action::Nothing
    }

    fn active(&mut self, _instance: &mut Option<GameInstance>, state: &mut crate::GameState) -> state::Action {
        let ui = state.ui_manager.create_node(ResourceKey::new("base", "manage/edit_room"));
        self.ui = Some(ui.clone());
        state.keybinds.add_collection(keybinds::KeyCollection::EditRoom);
        state.renderer.set_mouse_sprite(ResourceKey::new("base", "ui/cursor/question"));
        state.ui_manager.events().emit(CloseWindowOthers(ui));
        state::Action::Nothing
    }

    fn tick(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());
        if !instance.player.state.is_none() {
            // Can't place a room whilst one is already in progress
            state::Action::Pop
        } else {
            state::Action::Nothing
        }
    }

    fn inactive(&mut self, _instance: &mut Option<GameInstance>, state: &mut crate::GameState) {
        if let Some(ui) = self.ui.take() {
            state.ui_manager.remove_node(ui);
        }
        state.keybinds.remove_collection(keybinds::KeyCollection::EditRoom);
        state.renderer.set_mouse_sprite(ResourceKey::new("base", "ui/cursor/normal"));
        self.last_room = None;
    }

    fn ui_event(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState, evt: &mut event::EventHandler) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());
        let mut action = state::Action::Nothing;
        let ui = assume!(state.global_logger, self.ui.clone());
        evt.handle_event_if::<super::CloseWindowOthers, _, _>(|evt| {
            // Handle in event_if in order to not consume the event and let
            // other windows read it too
            if !evt.0.is_same(&ui) {
                action = state::Action::Pop;
            }
            false
        }, |_| {});
        evt.handle_event_if::<super::CancelEvent, _, _>(|evt| evt.0.is_same(&ui), |_| {
            action = state::Action::Pop;
        });
        if let Some((room_id, req)) = self.request {
            network::RequestManager::handle_reply(evt, req, |res| {
                if let Some(room) = instance.level.try_room_info(res.room_id).filter(|v| !v.controller.is_invalid()) {
                    if res.booked {
                        instance.entities.add_component(room.controller, entity::ClientBooked);
                    } else {
                        instance.entities.remove_component::<entity::ClientBooked>(room.controller);
                    }
                }
                update_room_ui(&state.global_logger, &state.asset_manager, instance, &ui, Some(room_id));
            });
        }
        action
    }

    fn mouse_move(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState,  mouse_pos: (i32, i32)) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());
        let ui = assume!(state.global_logger, self.ui.clone());

        let (lx, ly) = state.renderer.mouse_to_level(mouse_pos.0, mouse_pos.1);
        let (lx, ly) = (lx.floor() as i32, ly.floor() as i32);
        let loc = Location::new(lx, ly);
        let room_id = instance.level.get_room_owner(loc);
        if room_id != self.last_room {
            self.last_room = room_id;
            if let Some(room_id) = room_id {
                if self.request.as_ref().map_or(true, |v| v.0 != room_id) {
                    self.request = Some((room_id, instance.request_manager.request(player::RoomBooked {
                        room_id: room_id,
                    })));
                }
            }
            update_room_ui(&state.global_logger, &state.asset_manager, instance, &ui, room_id);
        }

        state::Action::Nothing
    }

    fn key_action_req(&mut self, req: &mut state::CaptureRequester, instance: &mut Option<GameInstance>, state: &mut crate::GameState, action: keybinds::KeyAction, mouse_pos: (i32, i32)) -> state::Action {
        use crate::keybinds::KeyAction::*;
        let instance = assume!(state.global_logger, instance.as_mut());

        match action {
            SystemMenu => return state::Action::Pop,
            SelectEditRoom => {
                let (lx, ly) = state.renderer.mouse_to_level(mouse_pos.0, mouse_pos.1);
                let (lx, ly) = (lx.floor() as i32, ly.floor() as i32);
                let loc = Location::new(lx, ly);
                if let Some(room_id) = instance.level.get_room_owner(loc) {

                    let mut cmd: Command = crate::server::command::EditRoom::new(
                        room_id
                    ).into();
                    let mut proxy = super::GameProxy::proxy(state);

                    match cmd.execute(&mut proxy, &mut instance.player, CommandParams {
                        log: &instance.log,
                        level: &mut instance.level,
                        engine: &instance.scripting,
                        entities: &mut instance.entities,
                        snapshots: &instance.snapshots,
                        mission_handler: instance.mission_handler.as_ref().map(|v| v.borrow()),
                    }) {
                        Ok(_) => {
                            instance.push_command(cmd, req);
                            return state::Action::Switch(Box::new(
                                super::build::BuildRoom::new(false)
                            ));
                        },
                        Err(server::errors::Error(server::errors::ErrorKind::RoomNoFullOwnership, _)) => {
                            let mut cmd: Command = EditRoomLimited::new(
                                room_id
                            ).into();

                            try_cmd!(instance.log, cmd.execute(&mut proxy, &mut instance.player, CommandParams {
                                log: &instance.log,
                                level: &mut instance.level,
                                engine: &instance.scripting,
                                entities: &mut instance.entities,
                                snapshots: &instance.snapshots,
                                mission_handler: instance.mission_handler.as_ref().map(|v| v.borrow()),
                            }), {
                                instance.push_command(cmd, req);
                                return state::Action::Switch(Box::new(
                                    super::build::BuildRoom::new(true)
                                ));
                            });
                        },
                        Err(_err) => {
                        },
                    }
                }
            }
            _ => {},
        }
        state::Action::Nothing
    }
}