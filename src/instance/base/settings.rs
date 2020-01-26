
use super::super::*;
use crate::state;
use crate::server::event;
use crate::server::assets;

pub struct SettingsState {
    ui: Option<ui::Node>,

    next_update: f64,
}

impl SettingsState {
    pub fn new() -> SettingsState {
        SettingsState {
            ui: None,
            next_update: 0.0,
        }
    }
}

impl state::State for SettingsState {
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(SettingsState {
            ui: self.ui.clone(),
            next_update: self.next_update,
        })
    }

    fn active(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) -> state::Action {
        let ui = state.ui_manager.create_node(assets::ResourceKey::new("base", "manage/settings"));
        self.ui = Some(ui.clone());

        let _instance = assume!(state.global_logger, instance.as_mut());

        state.ui_manager.events().emit(CloseWindowOthers(ui));
        state::Action::Nothing
    }

    fn tick_req(&mut self, req: &mut state::CaptureRequester, instance: &mut Option<GameInstance>, state: &mut crate::GameState) -> state::Action {
        #[allow(unused)]
        let mut needs_update = false;

        let instance = assume!(state.global_logger, instance.as_mut());
        let _ui = assume!(state.global_logger, self.ui.clone());

        if needs_update && self.next_update <= 0.0 {
            self.next_update = 60.0;
        }
        if self.next_update > 0.0 {
            self.next_update -= state.delta;
            if self.next_update <= 0.0 {
                let mut cmd: Command = UpdateConfig::new(instance.player.config.clone()).into();
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
                });
                self.next_update = 0.0;
            }
        }

        state::Action::Nothing
    }

    fn inactive_req(&mut self, req: &mut state::CaptureRequester, instance: &mut Option<GameInstance>, state: &mut crate::GameState) {
        let instance = assume!(state.global_logger, instance.as_mut());
        // Force an update on close if there is unsent changes
        if self.next_update > 0.0 {
            let mut cmd: Command = UpdateConfig::new(instance.player.config.clone()).into();
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
            });
            self.next_update = 0.0;
        }
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
        evt.handle_event_if::<super::CancelEvent, _, _>(|evt| evt.0.is_same(&ui), |_| {
            action = state::Action::Pop;
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
