
use crate::server;

use crate::prelude::*;
use super::{GameState, GameInstance};
use crate::state;
use crate::ui;
use crate::server::lua;
use crate::server::common::MissionEntry;

pub(crate) struct MenuState {
    ui: Option<ui::Node>,
    selected_item: usize,

    missions: Vec<MissionEntry>,
}

impl MenuState {
    pub(crate) fn new() -> MenuState {
        MenuState {
            ui: None,
            selected_item: 0,
            missions: vec![],
        }
    }
}

impl state::State for MenuState {
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(MenuState {
            ui: self.ui.clone(),
            selected_item: self.selected_item,
            missions: self.missions.clone(),
        })
    }

    fn takes_focus(&self) -> bool { true }

    fn active(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState) -> state::Action {
        let node = state.ui_manager.create_node(ResourceKey::new("base", "menus/campaign"));
        if let Some(new_game) = query!(node, button(id="new_game")).next() {
            new_game.set_property("on_click", ui::MethodDesc::<ui::MouseUpEvent>::native(|evt, _, _| {
                evt.emit(NewGame);
                true
            }));
        }
        if let Some(load_game) = query!(node, button(id="load_game")).next() {
            load_game.set_property("on_click", ui::MethodDesc::<ui::MouseUpEvent>::native(|evt, _, _| {
                evt.emit(LoadGame);
                true
            }));
        }

        if let Some(content) = query!(node, campaign_list > scroll_panel > content).next() {
            let missions = assume!(
                state.global_logger,
                state.ui_manager.scripting.get::<lua::Ref<lua::Table>>(lua::Scope::Global, "missions")
            );
            self.missions.clear();
            for (idx, mission) in missions.iter::<i32, lua::Ref<lua::Table>>() {
                let idx = idx as usize - 1;
                let is_valid = true;

                let mission = assume!(state.global_logger, lua::from_table::<MissionEntry>(&mission));

                let entry = node! {
                    campaign_entry(selected=idx == self.selected_item, entry=idx as i32, name=mission.name.clone(), valid=is_valid) {
                        name {
                            @text(mission.name.clone())
                        }
                    }
                };
                if is_valid {
                    entry.set_property("on_click", ui::MethodDesc::<ui::MouseUpEvent>::native(move |evt, _, _| {
                        evt.emit(SelectEntry(idx));
                        true
                    }));
                }
                content.add_child(entry);
                self.missions.push(mission);
            }
        }

        self.ui = Some(node);
        state.ui_manager.events.borrow_mut().emit(SelectEntry(0));

        state::Action::Nothing
    }

    fn inactive(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState) {
        if let Some(node) = self.ui.take() {
            state.ui_manager.remove_node(node);
        }
    }

    fn ui_event(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState, evt: &mut server::event::EventHandler) -> state::Action {
        let mut action = state::Action::Nothing;
        let ui = assume!(state.global_logger, self.ui.clone());
        evt.handle_event::<NewGame, _>(|_| {
            if let Some(cur) = query!(ui, campaign_entry(entry=self.selected_item as i32)).next() {
                let entry = assume!(state.global_logger, cur.get_property::<i32>("entry")) as usize;
                let mission = &self.missions[entry];
                let name = format!("missions/{}", mission.save_key);
                let valid = server::saving::can_load(&state.filesystem, &name, server::saving::SaveType::Mission);
                if valid.is_ok() {
                    let fs = crate::make_filesystem(#[cfg(feature = "steam")] &state.steam);
                    // Ask to restart instead
                    let events = state.ui_manager.events.clone();
                    action = state::Action::Push(Box::new(ui::prompt::Confirm::new(
                        ui::prompt::ConfirmConfig {
                            description: format!("Are you sure you wish to restart the mission \"{}\"?", mission.name),
                            accept: "Restart".into(),
                            ..ui::prompt::ConfirmConfig::default()
                        },
                        move |rpl| {
                            if rpl == ui::prompt::ConfirmResponse::Accept {
                                let _ = server::saving::delete_save(&fs, &name);
                                events.borrow_mut().emit(NewGame);
                            }
                        }
                    )));
                } else {
                    let _ = server::saving::delete_save(&state.filesystem, &name);
                    let key = mission.get_name_key().into_owned();
                    let (instance, _hosted_server) = GameInstance::single_player(
                        &state.global_logger, &state.asset_manager,
                        #[cfg(feature = "steam")] state.steam.clone(), name,
                        Some(key)
                    )
                        .expect("Failed to connect to single player instance");
                    action = state::Action::Switch(Box::new(crate::instance::BaseState::new(instance)));
                }
            }
        });
        evt.handle_event::<LoadGame, _>(|_| {
            if let Some(cur) = query!(ui, campaign_entry(entry=self.selected_item as i32)).next() {
                let entry = assume!(state.global_logger, cur.get_property::<i32>("entry")) as usize;
                let mission = &self.missions[entry];
                let key = mission.get_name_key().into_owned();
                let name = format!("missions/{}", mission.save_key);
                let (instance, _hosted_server) = GameInstance::single_player(
                    &state.global_logger, &state.asset_manager,
                    #[cfg(feature = "steam")] state.steam.clone(), name,
                    Some(key)
                )
                    .expect("Failed to connect to single player instance");
                action = state::Action::Switch(Box::new(crate::instance::BaseState::new(instance)));
            }
        });
        evt.handle_event::<SelectEntry, _>(|SelectEntry(idx)| {
            if let Some(old) = query!(ui, campaign_entry(entry=self.selected_item as i32)).next() {
                old.set_property("selected", false);
            }
            self.selected_item = idx;
            if let Some(new) = query!(ui, campaign_entry(entry=self.selected_item as i32)).next() {
                new.set_property("selected", true);
            }

            if let Some(cur) = query!(ui, campaign_entry(entry=self.selected_item as i32)).next() {
                let entry = assume!(state.global_logger, cur.get_property::<i32>("entry")) as usize;
                let mission = &self.missions[entry];

                if let Some(desc) = query!(ui, campaign_details > @text).next() {
                    desc.set_text(mission.get_description());
                }

                if let (Some(new), Some(load)) = (
                    query!(ui, button(id="new_game")).next(),
                    query!(ui, button(id="load_game")).next(),
                ) {
                    let valid = server::saving::can_load(&state.filesystem, &format!("missions/{}", mission.save_key), server::saving::SaveType::Mission);
                    if valid.is_ok() {
                        query!(new, content > @text).next().map(|v| v.set_text("Restart Mission"));
                        new.set_property("disabled", false);
                        query!(load, content > @text).next().map(|v| v.set_text("Continue Mission"));
                        load.set_property("disabled", false);
                    } else {
                        query!(new, content > @text).next().map(|v| v.set_text("Start Mission"));
                        new.set_property("disabled", false);
                        query!(load, content > @text).next().map(|v| v.set_text("Continue Mission"));
                        load.set_property("disabled", true);
                    }
                }
            }
        });
        action
    }
}

struct NewGame;
struct LoadGame;
struct SelectEntry(usize);
