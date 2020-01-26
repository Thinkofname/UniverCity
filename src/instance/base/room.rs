
use super::super::*;
use crate::state;
use crate::server::event;
use crate::server::assets;
use serde_json;


pub struct BuyRoomState {
    ui: Option<ui::Node>,
    room_list: Vec<RoomInfo>,
    selected_room: usize,
}

#[derive(Clone, Debug)]
struct RoomInfo {
    room: assets::ResourceKey<'static>,
    display_name: String,
    icon: assets::ResourceKey<'static>,
    description: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct RoomInfoJson {
    room: String,
    display_name: String,
    icon: String,
    description: Vec<String>,
}

impl BuyRoomState {
    pub fn new() -> BuyRoomState {
        BuyRoomState {
            ui: None,
            room_list: vec![],
            selected_room: 0,
        }
    }
}

impl state::State for BuyRoomState {
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(BuyRoomState {
            ui: self.ui.clone(),
            room_list: self.room_list.clone(),
            selected_room: self.selected_room,
        })
    }

    fn added(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());
        if !instance.player.state.is_none() {
            // Can't place a room whilst one is already in progress
            return state::Action::Pop;
        }
        let assets = instance.asset_manager.clone();
        for module in assets.get_packs() {
            let room_file = match assets.open_from_pack(module.borrow(), "rooms/rooms.json") {
                Ok(val) => val,
                Err(_) => continue,
            };
            let rooms_raw: Vec<RoomInfoJson> = match serde_json::from_reader(room_file) {
                Ok(val) => val,
                Err(err) => {
                    error!(instance.log, "Failed to parse rooms.json for pack {:?}: {}", module, err);
                    continue
                }
            };
            self.room_list.extend(rooms_raw.into_iter()
                .map(|v| RoomInfo {
                    room: assets::LazyResourceKey::parse(&v.room)
                        .or_module(module.borrow())
                        .into_owned(),
                    display_name: v.display_name,
                    icon: assets::LazyResourceKey::parse(&v.icon)
                        .or_module(module.borrow())
                        .into_owned(),
                    description: v.description.join(" "),
                }));
        }
        assert!(!self.room_list.is_empty()); // Can't work with no rooms
        state::Action::Nothing
    }

    fn active(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) -> state::Action {
        let ui = state.ui_manager.create_node(assets::ResourceKey::new("base", "buy/rooms"));
        self.ui = Some(ui.clone());

        let instance = assume!(state.global_logger, instance.as_mut());
        if let Some(scroll_panel) = query!(ui, scroll_panel).next() {
            if let Some(content) = query!(scroll_panel, scroll_panel > content).next() {
                for (i, room) in self.room_list.iter().enumerate() {
                    let ty = assume!(state.global_logger, state.asset_manager.loader_open::<room::Loader>(room.room.borrow()));
                    let can_build = ty.check_requirements(&instance.level, instance.player.id);
                    let n = node!{
                        button(id=i as i32, disabled=!can_build) {
                            content {
                                @text(room.display_name.clone())
                            }
                        }
                    };
                    n.set_property("key", room.room.as_string());
                    if can_build {
                        n.set_property("on_click", ui::MethodDesc::<ui::MouseUpEvent>::native(move |evt, _, _| {
                            evt.emit(SelectRoomEvent(i));
                            true
                        }));
                    } else {
                        let requirements = Ref::new_table(&instance.scripting);
                        let requirement_met = Ref::new_string(&instance.scripting, "met");
                        let requirement_info = Ref::new_string(&instance.scripting, "info");
                        for req in &ty.requirements {
                            let mut s = String::new();
                            let _ = req.print_tooltip(&instance.asset_manager, &mut s);
                            let ret = Ref::new_table(&instance.scripting);
                            ret.insert(requirement_met.clone(), req.check_requirement(&instance.level, instance.player.id));
                            ret.insert(requirement_info.clone(), Ref::new_string(&instance.scripting, s));
                            requirements.insert(requirements.length() + 1, ret);
                        }
                        n.set_property("requirements", ui::LuaTable(requirements));
                    }
                    content.add_child(n);
                }

                scroll_panel.set_property::<i32>("rows", self.room_list.len() as i32);
            }
        }

        // Focus the first entry
        state.ui_manager.events().emit(SelectRoomEvent(0));
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
    }

    fn ui_event(&mut self, _instance: &mut Option<GameInstance>, state: &mut crate::GameState, evt: &mut event::EventHandler) -> state::Action {
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
        evt.handle_event::<SelectRoomEvent, _>(|evt| {
            if let Some(btn) = query!(ui, button(id=self.selected_room as i32)).next() {
                btn.set_property("selected", false);
            }
            self.selected_room = evt.0;
            if let Some(btn) = query!(ui, button(id=self.selected_room as i32)).next() {
                btn.set_property("selected", true);
            }
            let room = &self.room_list[evt.0];
            if let Some(icon) = query!(ui, preview > icon).next() {
                icon.set_property("img", room.icon.as_string());
            }
            if let Some(description) = query!(ui, preview > description).next() {
                for c in description.children() {
                    description.remove_child(c);
                }
                description.add_child(ui::Node::new_text(room.description.as_str()));

                let ty = assume!(state.global_logger, state.asset_manager.loader_open::<room::Loader>(room.room.borrow()));

                if !ty.required_entities.is_empty() {
                    let text = ui::Node::new_text("\nTo be used this requires:\n");
                    text.set_property("entity_requirement_info", true);
                    description.add_child(text);
                    for (entity, count) in &ty.required_entities {
                        let ety = assume!(state.global_logger, state.asset_manager.loader_open::<Loader<entity::ClientComponent>>(entity.borrow()));
                        description.add_child(ui::Node::new("bullet_point"));
                        let text = if *count == 1 {
                            let first = ety.display_name.chars()
                                .next()
                                .map(|v| v.to_ascii_lowercase());
                            ui::Node::new_text(format!("{} {}\n", match first {
                                Some('a') | Some('e') | Some('i') | Some('o') | Some('u') => "an",
                                _ => "a",
                            }, ety.display_name))
                        } else {
                            ui::Node::new_text(format!("{} {}\n", *count, ety.display_name))
                        };
                        text.set_property("entity_requirement", true);
                        description.add_child(text);
                    }
                }
            }
        });
        evt.handle_event_if::<super::CancelEvent, _, _>(|evt| evt.0.is_same(&ui), |_| {
            action = state::Action::Pop;
        });
        evt.handle_event_if::<super::AcceptEvent, _, _>(|evt| evt.0.is_same(&ui), |_| {
            action = state::Action::Switch(Box::new(super::build::BuildState::new(
                self.room_list[self.selected_room].room.borrow()
            )))
        });
        action
    }

    fn key_action(&mut self, _instance: &mut Option<GameInstance>, _state: &mut crate::GameState, action: keybinds::KeyAction, _mouse_pos: (i32, i32)) -> state::Action {
        use crate::keybinds::KeyAction::*;

        match action {
            SystemMenu => state::Action::Pop,
            _ => state::Action::Nothing,
        }
    }
}

struct SelectRoomEvent(usize);