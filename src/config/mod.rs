//! Handles cofiguration for the game

mod graphics;
pub mod keybinds;

use std::cell::{Cell, RefCell};
use std::fs::File;
use std::rc::Rc;

use serde_json;

use crate::instance::GameInstance;
use crate::prelude::*;
use crate::server::event;
use crate::state;
use crate::GameState;
use sdl2;
use sdl2::video::FullscreenType;

/// The current configuration of the game
pub struct Config {
    /// The volume level (0.0, 1.0) of sound effects
    pub music_volume: Cell<f64>,
    /// The volume level (0.0, 1.0) of music
    pub sound_volume: Cell<f64>,
    /// The target fps for the game to run at
    pub target_fps: Cell<u32>,
    /// Sets the mode of the game's window
    pub fullscreen_mode: Cell<FullscreenType>,
    /// Sets the resolution of the game when fullscreen
    pub fullscreen_res: Cell<(u32, u32)>,

    /// The size of the shadow texture
    pub render_shadow_res: Cell<u32>,
    /// The number of SSAO samples
    pub render_ssao: Cell<u32>,
    /// Whether to use fxaa or not
    pub render_fxaa: Cell<bool>,
    /// The scale of the render output.
    ///
    /// e.g. 0.5 means to render at half the normal size
    /// and upscale
    pub render_scale: Cell<f32>,
    /// The scale of the UI.
    pub ui_scale: Cell<f32>,

    /// The colour of the placement grid when valid
    pub placement_valid_colour: Cell<(u8, u8, u8)>,
    /// The colour of the placement grid when invalid
    pub placement_invalid_colour: Cell<(u8, u8, u8)>,
    /// The additional asset packs to load
    pub asset_packs: RefCell<Vec<String>>,
}

#[derive(Serialize, Deserialize)]
struct ConfigFormat {
    music_volume: f64,
    sound_volume: f64,
    target_fps: u32,
    fullscreen_mode: String,
    fullscreen_res: (u32, u32),
    #[serde(default = "shadow_default")]
    render_shadow_res: u32,
    #[serde(default = "ssao_default")]
    render_ssao: u32,
    #[serde(default = "fxaa_default")]
    render_fxaa: bool,
    #[serde(default = "render_scale_default")]
    render_scale: f32,
    #[serde(default = "ui_scale_default")]
    ui_scale: f32,
    #[serde(default = "placement_valid_def")]
    placement_valid_colour: (u8, u8, u8),
    #[serde(default = "placement_invalid_def")]
    placement_invalid_colour: (u8, u8, u8),
    #[serde(default)]
    asset_packs: Vec<String>,
}

fn shadow_default() -> u32 {
    2048
}
fn ssao_default() -> u32 {
    16
}
fn fxaa_default() -> bool {
    true
}
fn render_scale_default() -> f32 {
    1.0
}
fn ui_scale_default() -> f32 {
    1.0
}

fn placement_valid_def() -> (u8, u8, u8) {
    (46, 65, 114)
}
fn placement_invalid_def() -> (u8, u8, u8) {
    (170, 57, 57)
}

impl Config {
    /// Creates a configuration with the default settings
    pub fn default(video: &sdl2::VideoSubsystem) -> Rc<Config> {
        let res = video
            .desktop_display_mode(0)
            .ok()
            .map_or((800, 480), |v| (v.w as u32, v.h as u32));
        Rc::new(Config {
            music_volume: Cell::new(0.5),
            sound_volume: Cell::new(1.0),
            target_fps: Cell::new(60),
            fullscreen_mode: Cell::new(FullscreenType::Off),
            fullscreen_res: Cell::new(res),
            render_shadow_res: Cell::new(2048),
            render_ssao: Cell::new(16),
            render_fxaa: Cell::new(true),
            render_scale: Cell::new(1.0),
            ui_scale: Cell::new(1.0),
            placement_valid_colour: Cell::new(placement_valid_def()),
            placement_invalid_colour: Cell::new(placement_invalid_def()),
            asset_packs: RefCell::new(Vec::new()),
        })
    }

    /// Tries to load the configuration from the default location
    pub fn load(&self) -> UResult<()> {
        let f = if let Ok(f) = File::open("./config.json") {
            f
        } else {
            return Ok(());
        };
        let config: ConfigFormat = serde_json::from_reader(f)?;

        self.music_volume.set(config.music_volume);
        self.sound_volume.set(config.sound_volume);
        self.target_fps.set(config.target_fps);
        self.fullscreen_mode
            .set(match config.fullscreen_mode.as_str() {
                "borderless" => FullscreenType::Desktop,
                _ => FullscreenType::Off,
            });
        self.fullscreen_res.set(config.fullscreen_res);
        self.render_shadow_res.set(config.render_shadow_res);
        self.render_ssao.set(config.render_ssao);
        self.render_fxaa.set(config.render_fxaa);
        self.render_scale.set(config.render_scale);
        self.placement_valid_colour
            .set(config.placement_valid_colour);
        self.placement_invalid_colour
            .set(config.placement_invalid_colour);
        self.ui_scale.set(config.ui_scale.max(0.1));
        self.asset_packs.replace(config.asset_packs);
        Ok(())
    }

    /// Tries to save the configuration from the default location
    pub fn save(&self) -> UResult<()> {
        let f = File::create("./config.json")?;
        serde_json::to_writer_pretty(
            f,
            &ConfigFormat {
                music_volume: self.music_volume.get(),
                sound_volume: self.sound_volume.get(),
                target_fps: self.target_fps.get(),
                fullscreen_mode: match self.fullscreen_mode.get() {
                    FullscreenType::Off => "windowed",
                    FullscreenType::Desktop => "borderless",
                    FullscreenType::True => "fullscreen", // Disabled for now so shouldn't happen
                }
                .to_owned(),
                fullscreen_res: self.fullscreen_res.get(),
                render_shadow_res: self.render_shadow_res.get(),
                render_ssao: self.render_ssao.get(),
                render_fxaa: self.render_fxaa.get(),
                render_scale: self.render_scale.get(),
                ui_scale: self.ui_scale.get(),
                placement_valid_colour: self.placement_valid_colour.get(),
                placement_invalid_colour: self.placement_invalid_colour.get(),
                asset_packs: self.asset_packs.borrow().clone(),
            },
        )?;
        Ok(())
    }
}

pub(super) struct OptionsMenuState {
    ui: Option<OptionsUI>,

    paused: bool,
    current_mode: i32,
}

#[derive(Clone)]
struct OptionsUI {
    root: ui::Node,

    sound_volume: ui::Node,
    music_volume: ui::Node,
    fps: ui::Node,
    fullscreen: ui::Node,
    // res: ui::Node,
}

impl OptionsMenuState {
    pub(super) fn new(paused: bool) -> OptionsMenuState {
        OptionsMenuState {
            ui: None,
            current_mode: 0,
            paused,
        }
    }
}

impl state::State for OptionsMenuState {
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(OptionsMenuState {
            ui: self.ui.clone(),
            current_mode: self.current_mode,
            paused: self.paused,
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
            .create_node(ResourceKey::new("base", "menus/options"));

        if let Some(fullscreen) = query!(node, fullscreen).next() {
            fullscreen.set_property("pause_menu", self.paused);
        }

        // Fill in the options
        let mut frame_rates = Vec::new();
        let mut res = Vec::new();

        let display = assume!(state.global_logger, state.window.display_index());
        let vid = state.window.subsystem().clone();
        let num_modes = assume!(state.global_logger, vid.num_display_modes(display));
        for i in 0..num_modes {
            let mode = assume!(state.global_logger, vid.display_mode(display, i));
            frame_rates.push(mode.refresh_rate);
            res.push((mode.w, mode.h));
        }

        // Throw in some standard ones
        frame_rates.push(30);
        frame_rates.push(60);
        frame_rates.push(i32::max_value());
        res.push((800, 480));

        frame_rates.sort();
        frame_rates.dedup();
        res.sort();
        res.dedup();

        let fullscreen = assume!(
            state.global_logger,
            query!(node, dropdown(id = "fullscreen")).next()
        );
        self.current_mode = match state.config.fullscreen_mode.get() {
            FullscreenType::Off => 1,
            FullscreenType::Desktop => 2,
            FullscreenType::True => 3,
        };
        fullscreen.set_property("value", self.current_mode);

        // TODO: FIXME
        // let dres = assume!(state.global_logger, query!(node, dropdown(id="resolution")).next());
        // dres.set_property("options", res.len() as i32);
        // for (i, r) in res.into_iter().enumerate() {
        //     if (r.0 as u32, r.1 as u32) == state.config.fullscreen_res.get() {
        //         dres.set_property("value", (i + 1) as i32);
        //     }
        //     dres.set_property(&format!("option{}", i + 1), format!("{}x{}", r.0, r.1));
        //     dres.set_property(&format!("option{}_width", i + 1), r.0);
        //     dres.set_property(&format!("option{}_height", i + 1), r.1);
        // }

        let dfps = assume!(
            state.global_logger,
            query!(node, dropdown(id = "fps")).next()
        );
        dfps.set_property("options", frame_rates.len() as i32);
        for (i, f) in frame_rates.into_iter().enumerate() {
            if f == state.config.target_fps.get() as i32 {
                dfps.set_property("value", (i + 1) as i32);
            }
            dfps.set_property(
                &format!("option{}", i + 1),
                if f == i32::max_value() {
                    "Unlimited".to_owned()
                } else {
                    format!("{}", f)
                },
            );
            dfps.set_property(&format!("option{}_value", i + 1), f as i32);
        }

        let music_volume = assume!(
            state.global_logger,
            query!(node, slider(id = "music_volume")).next()
        );
        music_volume.set_property("value", state.config.music_volume.get() * 100.0);
        let sound_volume = assume!(
            state.global_logger,
            query!(node, slider(id = "sound_volume")).next()
        );
        sound_volume.set_property("value", state.config.sound_volume.get() * 100.0);

        if let Some(list) = query!(node, options_list > scroll_panel > content).next() {
            for col in [
                keybinds::KeyCollection::Root,
                keybinds::KeyCollection::Game,
                keybinds::KeyCollection::RoomPlacement,
                keybinds::KeyCollection::RoomResize,
                keybinds::KeyCollection::BuildRoom,
                keybinds::KeyCollection::EditRoom,
                keybinds::KeyCollection::PlaceObject,
                keybinds::KeyCollection::PlaceStaff,
            ]
            .iter()
            .cloned()
            {
                let collection = state.keybinds.collections.get(&col).unwrap();
                if collection.binds.is_empty() {
                    continue;
                }
                let header = node! {
                    opt_sub_header {
                        cell {
                            line
                        }
                        cell_text {
                            header {
                                @text(format!(" {} ", col.as_str()))
                            }
                        }
                        cell {
                            line
                        }
                    }
                };
                list.add_child(header);

                let mut used_actions = FNVSet::default();
                for &(down, up) in collection.binds.values() {
                    down.map(|a| used_actions.insert(a));
                    up.map(|a| used_actions.insert(a));
                }

                let mut used_actions = used_actions
                    .into_iter()
                    .filter(|a| !a.hidden())
                    .collect::<Vec<_>>();
                used_actions.sort();

                for action in used_actions {
                    let mut first_bind: Option<(bool, keybinds::BindType)> = None;
                    let mut second_bind: Option<(bool, keybinds::BindType)> = None;
                    for (key, &(down, up)) in &collection.binds {
                        if down == Some(action) {
                            if first_bind.is_none() {
                                first_bind = Some((false, *key));
                            } else if second_bind.is_none() {
                                second_bind = Some((false, *key));
                                break;
                            }
                        }
                        if up == Some(action) {
                            if first_bind.is_none() {
                                first_bind = Some((true, *key));
                            } else if second_bind.is_none() {
                                second_bind = Some((true, *key));
                                break;
                            }
                        }
                    }

                    if let (Some(first), Some(second)) = (first_bind.as_mut(), second_bind.as_mut())
                    {
                        if first.1.as_string().len() > second.1.as_string().len() {
                            ::std::mem::swap(first, second);
                        }
                    }

                    fn format(
                        action: keybinds::KeyAction,
                        bind: Option<(bool, keybinds::BindType)>,
                    ) -> String {
                        if let Some(bind) = bind {
                            if action.standard_direction().is_some() {
                                bind.1.as_string()
                            } else {
                                format!(
                                    "{} - {}",
                                    bind.1.as_string(),
                                    if bind.0 { "Up" } else { "Down" }
                                )
                            }
                        } else {
                            "Unset".to_owned()
                        }
                    }

                    let node = node! {
                        option(is_key=true) {
                            label_center(
                                key = action.as_str().to_owned(),
                                tooltip = action.as_tooltip().to_owned()
                            ) {
                                label {
                                    @text(action.as_str())
                                }
                            }
                            button(first=true, unset=first_bind.is_none()) {
                                content {
                                    @text(format(action, first_bind))
                                }
                            }
                            button(second=true, unset=second_bind.is_none()) {
                                content {
                                    @text(format(action, second_bind))
                                }
                            }
                        }
                    };

                    if let Some(btn) = query!(node, button(first = true)).next() {
                        btn.set_property(
                            "on_click",
                            ui::MethodDesc::<ui::MouseUpEvent>::native(move |evts, _node, _evt| {
                                evts.emit(ChangeBindingEvent(col, action, first_bind));
                                true
                            }),
                        );
                    }
                    if let Some(btn) = query!(node, button(second = true)).next() {
                        btn.set_property(
                            "on_click",
                            ui::MethodDesc::<ui::MouseUpEvent>::native(move |evts, _node, _evt| {
                                evts.emit(ChangeBindingEvent(col, action, second_bind));
                                true
                            }),
                        );
                    }

                    list.add_child(node);
                }
            }
        }

        if let Some(btn) = query!(node, button(id = "advance")).next() {
            btn.set_property(
                "on_click",
                ui::MethodDesc::<ui::MouseUpEvent>::native(move |evts, _node, _evt| {
                    evts.emit(GraphicsSettings);
                    true
                }),
            );
        }

        self.ui = Some(OptionsUI {
            root: node.clone(),

            music_volume,
            sound_volume,
            fps: dfps,
            fullscreen,
            // res: dres,
        });
        state::Action::Nothing
    }

    fn tick(
        &mut self,
        _instance: &mut Option<GameInstance>,
        state: &mut GameState,
    ) -> state::Action {
        let ui = assume!(state.global_logger, self.ui.as_ref());

        let music_volume = ui.music_volume.get_property::<f64>("value").unwrap_or(0.0) / 100.0;
        if music_volume != state.config.music_volume.get() {
            state.config.music_volume.set(music_volume);
            state.audio.update_settings(&state.config);
        }

        let sound_volume = ui.sound_volume.get_property::<f64>("value").unwrap_or(0.0) / 100.0;
        if sound_volume != state.config.sound_volume.get() {
            state.config.sound_volume.set(sound_volume);
            state.audio.update_settings(&state.config);
        }

        let fps = ui
            .fps
            .get_property::<i32>("value")
            .and_then(|v| ui.fps.get_property::<i32>(&format!("option{}_value", v)))
            .unwrap_or(60) as u32;
        state.config.target_fps.set(fps);

        let mode = ui.fullscreen.get_property::<i32>("value").unwrap_or(1);
        if mode != self.current_mode {
            self.current_mode = mode;
            state.config.fullscreen_mode.set(match mode {
                2 => FullscreenType::Desktop,
                _ => FullscreenType::Off,
            });
            crate::update_window(state);
        }
        // TODO: FIXME
        // let res = ui.res.get_property::<i32>("value")
        //     .map(|v| (
        //         ui.res.get_property::<i32>(&format!("option{}_width", v)).unwrap_or(800) as u32,
        //         ui.res.get_property::<i32>(&format!("option{}_height", v)).unwrap_or(480) as u32,
        //     ))
        //     .unwrap_or((800, 480));
        // if res != state.config.fullscreen_res.get() {
        //     state.config.fullscreen_res.set(res);
        //     ::update_window(state);
        // }

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
        evt.handle_event::<ChangeBindingEvent, _>(|ChangeBindingEvent(col, act, bind)| {
            action =
                state::Action::Switch(Box::new(KeyCaptureState::new(col, act, bind, self.paused)));
        });
        evt.handle_event::<GraphicsSettings, _>(|_| {
            action = state::Action::Switch(Box::new(graphics::GraphicsMenuState::new(self.paused)));
        });
        evt.handle_event_if::<super::AcceptEvent, _, _>(
            |evt| evt.0.is_same(&ui.root),
            |_| {
                action = state::Action::Pop;
            },
        );
        action
    }

    fn inactive(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState) {
        if let Some(node) = self.ui.take() {
            state.ui_manager.remove_node(node.root);
        }
        assume!(state.global_logger, state.config.save());
        assume!(state.global_logger, state.keybinds.save());
    }
}

struct GraphicsSettings;
struct ChangeBindingEvent(
    keybinds::KeyCollection,
    keybinds::KeyAction,
    Option<(bool, keybinds::BindType)>,
);

struct KeyCaptureState {
    ui: Option<ui::Node>,

    collection: keybinds::KeyCollection,
    action: keybinds::KeyAction,
    previous: Option<(bool, keybinds::BindType)>,
    paused: bool,
}

impl KeyCaptureState {
    fn new(
        collection: keybinds::KeyCollection,
        action: keybinds::KeyAction,
        previous: Option<(bool, keybinds::BindType)>,
        paused: bool,
    ) -> KeyCaptureState {
        KeyCaptureState {
            ui: None,
            collection,
            action,
            previous,
            paused,
        }
    }
}

impl state::State for KeyCaptureState {
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(KeyCaptureState {
            ui: self.ui.clone(),
            collection: self.collection,
            action: self.action,
            previous: self.previous,
            paused: self.paused,
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
            .create_node(ResourceKey::new("base", "menus/options_key_capture"));
        if let Some(fullscreen) = query!(node, full_center).next() {
            fullscreen.set_property("pause_menu", self.paused);
        }
        if let Some(name) = query!(node, message_line(name=true) > @text).next() {
            name.set_text(self.action.as_str());
        }
        self.ui = Some(node.clone());
        state.keybinds.capture = true;
        state::Action::Nothing
    }

    fn tick(
        &mut self,
        _instance: &mut Option<GameInstance>,
        state: &mut GameState,
    ) -> state::Action {
        use sdl2::keyboard::Keycode;
        if let Some(bind) = state.keybinds.captured_bind.take() {
            // Remove the old binding

            if bind != keybinds::BindType::Key(Keycode::Backspace) {
                let collection = state
                    .keybinds
                    .collections
                    .get_mut(&self.collection)
                    .unwrap();

                if let Some(prev) = self.previous {
                    if let Some(key) = collection.binds.get_mut(&prev.1) {
                        if prev.0 {
                            key.1 = None;
                        } else {
                            key.0 = None;
                        }
                        if let Some(linked) = self.action.linked_action() {
                            // Inverse
                            if prev.0 && key.0 == Some(linked) {
                                key.0 = None;
                            } else if !prev.0 && key.1 == Some(linked) {
                                key.1 = None;
                            }
                        }
                    }
                }
                let new = collection.binds.entry(bind).or_insert((None, None));
                if let Some(dir) = self.action.standard_direction() {
                    if dir {
                        new.1 = Some(self.action);
                    } else {
                        new.0 = Some(self.action);
                    }
                    if let Some(linked) = self.action.linked_action() {
                        // Inverse
                        if dir {
                            new.0 = Some(linked);
                        } else {
                            new.1 = Some(linked);
                        }
                    }
                } else {
                    unimplemented!()
                }
            }
            state::Action::Switch(Box::new(OptionsMenuState::new(self.paused)))
        } else {
            state::Action::Nothing
        }
    }

    fn inactive(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState) {
        if let Some(node) = self.ui.take() {
            state.ui_manager.remove_node(node);
        }
        state.keybinds.capture = false;
        assume!(state.global_logger, state.keybinds.save());
    }
}
