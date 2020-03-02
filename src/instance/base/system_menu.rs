use super::*;

/// The system menu (pause screen in single player)
pub struct SystemMenu {
    ui: Option<ui::Node>,
}

impl SystemMenu {
    pub(crate) fn new() -> SystemMenu {
        SystemMenu { ui: None }
    }
}

impl state::State for SystemMenu {
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(SystemMenu {
            ui: self.ui.clone(),
        })
    }

    fn takes_focus(&self) -> bool {
        true
    }

    fn added(
        &mut self,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
    ) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());
        if instance.is_local {
            instance.paused = true;
            state.renderer.paused = true;
            assume!(
                state.global_logger,
                instance.ensure_send(packet::SetPauseGame { paused: true })
            );
        }
        state::Action::Nothing
    }

    fn active(
        &mut self,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
    ) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());
        let ui = state
            .ui_manager
            .create_node(ResourceKey::new("base", "menus/pause"));
        if let Some(quit) = query!(ui, button(id = "quit")).next() {
            quit.set_property(
                "on_click",
                ui::MethodDesc::<ui::MouseUpEvent>::native(|evt, _, _| {
                    evt.emit(Disconnect);
                    true
                }),
            );
        }
        if let Some(save) = query!(ui, button(id = "save")).next() {
            if instance.is_local {
                save.set_property(
                    "on_click",
                    ui::MethodDesc::<ui::MouseUpEvent>::native(|evt, _, _| {
                        evt.emit(SaveGame);
                        true
                    }),
                );
            } else if let Some(parent) = save.parent() {
                parent.remove_child(save);
                parent.set_property("rows", parent.get_property::<i32>("rows").unwrap_or(4) - 1);
            }
        }
        if let Some(options) = query!(ui, button(id = "options")).next() {
            options.set_property(
                "on_click",
                ui::MethodDesc::<ui::MouseUpEvent>::native(|evt, _, _| {
                    evt.emit(OptionsMenu);
                    true
                }),
            );
        }
        self.ui = Some(ui);
        state::Action::Nothing
    }

    fn inactive(&mut self, _instance: &mut Option<GameInstance>, state: &mut crate::GameState) {
        if let Some(ui) = self.ui.take() {
            state.ui_manager.remove_node(ui);
        }
    }

    fn removed(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) {
        let instance = assume!(state.global_logger, instance.as_mut());
        if instance.is_local {
            instance.paused = false;
            state.renderer.paused = false;
            assume!(
                state.global_logger,
                instance.ensure_send(packet::SetPauseGame { paused: false })
            );
        }
    }

    fn ui_event(
        &mut self,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
        evt: &mut event::EventHandler,
    ) -> state::Action {
        let mut action = state::Action::Nothing;
        let instance = assume!(state.global_logger, instance.as_mut());
        let ui = assume!(state.global_logger, self.ui.clone());
        evt.handle_event_if::<super::CancelEvent, _, _>(
            |evt| evt.0.is_same(&ui),
            |_| {
                action = state::Action::Pop;
            },
        );
        evt.handle_event::<SaveGame, _>(|_| {
            assume!(
                state.global_logger,
                instance.ensure_send(packet::SaveGame {})
            );
            action = state::Action::Pop;
        });
        evt.handle_event::<Disconnect, _>(|_| {
            instance.disconnect();
            action = state::Action::Pop;
        });
        evt.handle_event::<OptionsMenu, _>(|_| {
            action = state::Action::Push(Box::new(crate::config::OptionsMenuState::new(true)));
        });
        action
    }

    fn key_action(
        &mut self,
        _instance: &mut Option<GameInstance>,
        _state: &mut crate::GameState,
        action: keybinds::KeyAction,
        _mouse_pos: (i32, i32),
    ) -> state::Action {
        use crate::keybinds::KeyAction::*;

        match action {
            SystemMenu => state::Action::Pop,
            _ => state::Action::Nothing,
        }
    }
}

struct Disconnect;
struct SaveGame;
struct OptionsMenu;
