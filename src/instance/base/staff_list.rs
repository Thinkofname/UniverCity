use super::super::*;
use crate::server::assets;
use crate::server::event;
use crate::state;

pub struct StaffListState {
    ui: Option<ui::Node>,
}

impl StaffListState {
    pub fn new() -> StaffListState {
        StaffListState { ui: None }
    }
}

impl state::State for StaffListState {
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(StaffListState {
            ui: self.ui.clone(),
        })
    }

    fn active(
        &mut self,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
    ) -> state::Action {
        let ui = state
            .ui_manager
            .create_node(assets::ResourceKey::new("base", "manage/staff_list"));
        self.ui = Some(ui.clone());

        // Find owned entities
        let instance = assume!(state.global_logger, instance.as_mut());
        let player = instance.player.id;

        let mask = instance
            .entities
            .mask_for::<Owned>()
            .and_component::<Living>(&instance.entities)
            .and_component::<Paid>(&instance.entities);

        let list = assume!(
            state.global_logger,
            query!(ui, content(style="staff_list") > scroll_panel > content).next()
        );

        for e in instance.entities.iter_mask(&mask).filter(|v| {
            assume!(
                state.global_logger,
                instance.entities.get_component::<Owned>(*v)
            )
            .player_id
                == player
        }) {
            let living = assume!(
                state.global_logger,
                instance.entities.get_component::<Living>(e)
            );
            let ty = assume!(
                state.global_logger,
                state
                    .asset_manager
                    .loader_open::<server::entity::Loader<entity::ClientComponent>>(
                        living.key.borrow()
                    )
            );
            let sub_ty = &ty.variants[living.variant];
            let n = node! {
                staff_entry {
                    icon(img=sub_ty.icon.as_string())
                    name {
                        @text(format!("{} {}", living.name.0, living.name.1))
                    }
                    buttons {
                        button(id="find".to_owned()) {
                            content {
                                @text("Find")
                            }
                        }
                    }
                }
            };
            if let Some(btn) = query!(n, button(id = "find")).next() {
                btn.set_property(
                    "on_click",
                    ui::MethodDesc::<ui::MouseUpEvent>::native(move |evt, _, _| {
                        evt.emit(FocusEntity(e));
                        true
                    }),
                );
            }
            list.add_child(n);
            list.add_child(ui::Node::new("seperator"));
        }

        state.ui_manager.events().emit(CloseWindowOthers(ui));
        state::Action::Nothing
    }

    fn tick(
        &mut self,
        _instance: &mut Option<GameInstance>,
        _state: &mut crate::GameState,
    ) -> state::Action {
        state::Action::Nothing
    }

    fn inactive(&mut self, _instance: &mut Option<GameInstance>, state: &mut crate::GameState) {
        if let Some(ui) = self.ui.take() {
            state.ui_manager.remove_node(ui);
        }
    }

    fn ui_event(
        &mut self,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
        evt: &mut event::EventHandler,
    ) -> state::Action {
        let mut action = state::Action::Nothing;
        let ui = assume!(state.global_logger, self.ui.clone());
        let instance = assume!(state.global_logger, instance.as_mut());
        evt.handle_event_if::<super::CloseWindowOthers, _, _>(
            |evt| {
                // Handle in event_if in order to not consume the event and let
                // other windows read it too
                if !evt.0.is_same(&ui) {
                    action = state::Action::Pop;
                }
                false
            },
            |_| {},
        );
        evt.handle_event_if::<super::CancelEvent, _, _>(
            |evt| evt.0.is_same(&ui),
            |_| {
                action = state::Action::Pop;
            },
        );
        evt.handle_event(|FocusEntity(e)| {
            if instance.entities.is_valid(e) {
                action = state::Action::Push(Box::new(super::EntityInfoState::new(e)));
            }
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

struct FocusEntity(ecs::Entity);
