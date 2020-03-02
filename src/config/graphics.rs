use super::*;

pub(super) struct GraphicsMenuState {
    ui: Option<GraphicsUI>,

    paused: bool,
    current_shadow_res: i32,
    current_ssao: i32,
    current_fxaa: i32,
    next_render_update: i32,
}

#[derive(Clone)]
struct GraphicsUI {
    root: ui::Node,

    shadow_res: ui::Node,
    ssao_level: ui::Node,
    fxaa: ui::Node,
    render_scale: ui::Node,
    ui_scale: ui::Node,

    placement_valid: (ui::Node, ui::Node, ui::Node, ui::Node),
    placement_invalid: (ui::Node, ui::Node, ui::Node, ui::Node),
}

impl GraphicsMenuState {
    pub(super) fn new(paused: bool) -> GraphicsMenuState {
        GraphicsMenuState {
            ui: None,
            current_shadow_res: 0,
            current_ssao: 0,
            current_fxaa: 0,
            paused,
            next_render_update: -1,
        }
    }
}

impl state::State for GraphicsMenuState {
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(GraphicsMenuState {
            ui: self.ui.clone(),
            current_shadow_res: self.current_shadow_res,
            current_ssao: self.current_ssao,
            current_fxaa: self.current_fxaa,
            paused: self.paused,
            next_render_update: self.next_render_update,
        })
    }

    fn takes_focus(&self) -> bool {
        true
    }

    fn active(
        &mut self,
        _instance: &mut Option<GameInstance>,
        state: &mut GameState,
    ) -> state::Action {
        let node = state
            .ui_manager
            .create_node(ResourceKey::new("base", "menus/options_graphics"));

        if let Some(fullscreen) = query!(node, fullscreen).next() {
            fullscreen.set_property("pause_menu", self.paused);
        }

        let shadow_res = assume!(
            state.global_logger,
            query!(node, dropdown(id = "shadow_res")).next()
        );
        self.current_shadow_res = match state.config.render_shadow_res.get() {
            0 => 1,
            1024 => 2,
            2048 => 3,
            _ => 4,
        };
        shadow_res.set_property("value", self.current_shadow_res);

        let ssao_level = assume!(
            state.global_logger,
            query!(node, dropdown(id = "ssao_level")).next()
        );
        self.current_ssao = match state.config.render_ssao.get() {
            0 => 1,
            8 => 2,
            16 => 3,
            32 => 4,
            _ => 5,
        };
        ssao_level.set_property("value", self.current_ssao);

        let fxaa = assume!(
            state.global_logger,
            query!(node, dropdown(id = "fxaa")).next()
        );
        self.current_fxaa = if state.config.render_fxaa.get() { 2 } else { 1 };
        fxaa.set_property("value", self.current_fxaa);

        let render_scale = assume!(
            state.global_logger,
            query!(node, slider(id = "render_scale")).next()
        );
        render_scale.set_property("value", f64::from(state.config.render_scale.get() * 100.0));

        let ui_scale = assume!(
            state.global_logger,
            query!(node, slider(id = "ui_scale")).next()
        );
        ui_scale.set_property("value", f64::from(state.config.ui_scale.get()));

        let (pr, pg, pb) = state.config.placement_valid_colour.get();
        let pv_r = assume!(
            state.global_logger,
            query!(node, slider(id = "placement_valid_red")).next()
        );
        pv_r.set_property("value", i32::from(pr));
        let pv_g = assume!(
            state.global_logger,
            query!(node, slider(id = "placement_valid_green")).next()
        );
        pv_g.set_property("value", i32::from(pg));
        let pv_b = assume!(
            state.global_logger,
            query!(node, slider(id = "placement_valid_blue")).next()
        );
        pv_b.set_property("value", i32::from(pb));
        let pv = assume!(
            state.global_logger,
            query!(node, colour_display(id = "placement_valid")).next()
        );
        pv.set_property("colour", format!("#{:02x}{:02x}{:02x}", pr, pg, pb));
        let placement_valid = (pv, pv_r, pv_g, pv_b);

        let (pr, pg, pb) = state.config.placement_invalid_colour.get();
        let pv_r = assume!(
            state.global_logger,
            query!(node, slider(id = "placement_invalid_red")).next()
        );
        pv_r.set_property("value", i32::from(pr));
        let pv_g = assume!(
            state.global_logger,
            query!(node, slider(id = "placement_invalid_green")).next()
        );
        pv_g.set_property("value", i32::from(pg));
        let pv_b = assume!(
            state.global_logger,
            query!(node, slider(id = "placement_invalid_blue")).next()
        );
        pv_b.set_property("value", i32::from(pb));
        let pv = assume!(
            state.global_logger,
            query!(node, colour_display(id = "placement_invalid")).next()
        );
        pv.set_property("colour", format!("#{:02x}{:02x}{:02x}", pr, pg, pb));
        let placement_invalid = (pv, pv_r, pv_g, pv_b);

        self.ui = Some(GraphicsUI {
            root: node.clone(),
            shadow_res,
            ssao_level,
            fxaa,
            render_scale,
            ui_scale,
            placement_valid,
            placement_invalid,
        });
        state::Action::Nothing
    }

    fn tick(
        &mut self,
        _instance: &mut Option<GameInstance>,
        state: &mut GameState,
    ) -> state::Action {
        let ui = assume!(state.global_logger, self.ui.as_ref());

        let res = ui.shadow_res.get_property::<i32>("value").unwrap_or(1);
        if res != self.current_shadow_res {
            self.current_shadow_res = res;
            state.config.render_shadow_res.set(match res {
                1 => 0,
                2 => 1024,
                3 => 2048,
                _ => 4096,
            });
            self.next_render_update = 30;
        }

        let lvl = ui.ssao_level.get_property::<i32>("value").unwrap_or(1);
        if lvl != self.current_ssao {
            self.current_ssao = lvl;
            state.config.render_ssao.set(match lvl {
                1 => 0,
                2 => 8,
                3 => 16,
                4 => 32,
                _ => 64,
            });
            self.next_render_update = 30;
        }

        let fxaa = ui.fxaa.get_property::<i32>("value").unwrap_or(1);
        if fxaa != self.current_fxaa {
            self.current_fxaa = fxaa;
            state.config.render_fxaa.set(fxaa == 2);
            self.next_render_update = 30;
        }

        let render_scale =
            (ui.render_scale.get_property::<f64>("value").unwrap_or(0.0) / 100.0) as f32;
        if render_scale != state.config.render_scale.get() {
            state.config.render_scale.set(render_scale);
            self.next_render_update = 30;
        }

        let ui_scale = ((ui.ui_scale.get_property::<f64>("value").unwrap_or(0.0) * 10.0).round()
            / 10.0) as f32;
        if ui_scale != state.config.ui_scale.get() {
            state.config.ui_scale.set(ui_scale);
            self.next_render_update = 30;
        }

        let (pr, pg, pb) = state.config.placement_valid_colour.get();
        let pv_r = ui
            .placement_valid
            .1
            .get_property::<i32>("value")
            .unwrap_or(0) as u8;
        let pv_g = ui
            .placement_valid
            .2
            .get_property::<i32>("value")
            .unwrap_or(0) as u8;
        let pv_b = ui
            .placement_valid
            .3
            .get_property::<i32>("value")
            .unwrap_or(0) as u8;
        if pv_r != pr || pv_g != pg || pv_b != pb {
            state.config.placement_valid_colour.set((pv_r, pv_g, pv_b));
            ui.placement_valid
                .0
                .set_property("colour", format!("#{:02x}{:02x}{:02x}", pv_r, pv_g, pv_b));
            self.next_render_update = 30;
        }

        let (pr, pg, pb) = state.config.placement_invalid_colour.get();
        let pv_r = ui
            .placement_invalid
            .1
            .get_property::<i32>("value")
            .unwrap_or(0) as u8;
        let pv_g = ui
            .placement_invalid
            .2
            .get_property::<i32>("value")
            .unwrap_or(0) as u8;
        let pv_b = ui
            .placement_invalid
            .3
            .get_property::<i32>("value")
            .unwrap_or(0) as u8;
        if pv_r != pr || pv_g != pg || pv_b != pb {
            state
                .config
                .placement_invalid_colour
                .set((pv_r, pv_g, pv_b));
            ui.placement_invalid
                .0
                .set_property("colour", format!("#{:02x}{:02x}{:02x}", pv_r, pv_g, pv_b));
            self.next_render_update = 30;
        }

        if self.next_render_update > 0 {
            self.next_render_update -= 1;
        }
        if self.next_render_update == 0 {
            self.next_render_update = -1;
            state
                .renderer
                .set_ui_scale(1.0 / state.config.ui_scale.get());
            state
                .ui_manager
                .set_ui_scale(1.0 / state.config.ui_scale.get());
            state.renderer.rebuild_pipeline();
        }

        state::Action::Nothing
    }

    /// Called whenever a ui event is fired whilst the state is active
    fn ui_event(
        &mut self,
        _instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
        evt: &mut event::EventHandler,
    ) -> state::Action {
        let mut action = state::Action::Nothing;
        let ui = assume!(state.global_logger, self.ui.as_ref());
        evt.handle_event_if::<crate::AcceptEvent, _, _>(
            |evt| evt.0.is_same(&ui.root),
            |_| {
                action = state::Action::Switch(Box::new(super::OptionsMenuState::new(self.paused)));
            },
        );
        action
    }

    fn inactive(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState) {
        if let Some(node) = self.ui.take() {
            state.ui_manager.remove_node(node.root);
        }
        state.renderer.rebuild_pipeline();
    }
}
