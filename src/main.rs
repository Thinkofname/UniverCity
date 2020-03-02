#![windows_subsystem = "windows"]
#![recursion_limit = "128"]
#![type_length_limit = "8388608"]
#![warn(missing_docs)]
// # Clippy lints
// Mostly happens in the rendering code. Clippy's
// limit is pretty arbitrary.
#![allow(clippy::too_many_arguments)]
// Not a fan of this style
#![allow(clippy::new_without_default)]
// Not always bad, mainly style
#![allow(clippy::single_match)]
// Used to make sure debug is stripped out
#![allow(clippy::inline_always)]
// Clippy bug? https://github.com/Manishearth/rust-clippy/issues/1725
#![allow(clippy::get_unwrap)]
// Sometimes things get complex
#![allow(clippy::cyclomatic_complexity)]
// I generally use this correctly.
#![allow(clippy::float_cmp)]
// Not making a library
#![allow(clippy::should_implement_trait)]
#![allow(clippy::clone_on_ref_ptr)]
#![allow(clippy::option_map_unit_fn)]
#![allow(clippy::needless_pass_by_value)]
// Unwrap makes tracking crashes harder, use `assume!`
#![cfg_attr(not(test), deny(clippy::option_unwrap_used))]
#![cfg_attr(not(test), deny(clippy::result_unwrap_used))]

//! Base of the game. Nothing too special

#[macro_use]
extern crate serde_derive;
use sdl2;

#[macro_use]
extern crate univercity_server as server;
use think_ecs as ecs;

use png;
#[macro_use]
extern crate error_chain;
#[macro_use]
extern crate univercity_util as util;
#[macro_use]
extern crate gpuopt;

#[macro_use]
extern crate fungui;
use url;
#[macro_use]
extern crate slog;
use slog_async;
use slog_json;
use slog_term;

#[cfg(feature = "steam")]
use server::steamworks;

try_force_gpu!();

pub mod audio;
mod campaign;
pub mod config;
mod credits;
pub mod entity;
pub mod errors;
pub mod instance;
mod main_menu;
pub mod math;
mod multiplayer;
pub mod prelude;
pub mod render;
mod save_file;
pub mod script;
pub mod state;
pub mod ui;

use crate::instance::*;
pub(crate) use crate::multiplayer::MultiPlayer;

use crate::config::keybinds;
use crate::prelude::*;
use sdl2::event::Event;
use sdl2::video::FullscreenType;
use slog::Drain;
use std::path::Path;
use std::rc::Rc;
use std::sync::mpsc::channel;
use std::thread;
use std::time;

use crate::server::saving::filesystem::*;

#[cfg(feature = "system_alloc")]
#[global_allocator]
static GLOBAL: std::alloc::System = std::alloc::System;

/// The steam assigned app id for this game
#[allow(clippy::unreadable_literal)] // This is how steam presents this
#[cfg(feature = "steam")]
pub const STEAM_APP_ID: steamworks::AppId = steamworks::AppId(808160);

fn main() {
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let json_drain = slog_json::Json::default(
        ::std::fs::File::create("log.json").expect("Failed to create the log file"),
    )
    .fuse();
    let (drain, _guard) =
        slog_async::Async::new(slog::Duplicate::new(drain, json_drain).fuse()).build_with_guard();
    let drain = drain.fuse();
    // TODO: Filter debug logging at some point

    let log = slog::Logger::root(drain, o!());
    log_panics(&log, server::GAME_HASH, true);

    // TODO: Make steam optional?
    #[cfg(feature = "steam")]
    let (steam, mut single_steam) = {
        if steamworks::restart_app_if_necessary(STEAM_APP_ID) {
            return;
        }

        let (steam, single_steam) = assume!(log, steamworks::Client::init());
        steam
            .utils()
            .set_overlay_notification_position(steamworks::NotificationPosition::TopRight);
        (steam, single_steam)
    };

    // HACK: Try and stop optimizations/lto from removing these
    info!(
        log,
        "Starting UniverCity. Flags: {} {}",
        NvOptimusEnablement,
        AmdPowerXpressRequestHighPerformance
    );

    let sdl = sdl2::init().expect("Failed to initialize SDL2");
    sdl2::hint::set_video_minimize_on_focus_loss(false);
    let video = sdl.video().expect("Failed to create a video backend");

    let gl_attr = video.gl_attr();
    gl_attr.set_stencil_size(8);
    gl_attr.set_depth_size(24);
    gl_attr.set_context_major_version(3);
    gl_attr.set_context_minor_version(2);
    gl_attr.set_context_profile(sdl2::video::GLProfile::Core);

    loop {
        let mut window = video
            .window("UniverCity", 800, 480)
            .position_centered()
            .opengl()
            .resizable()
            .build()
            .expect("Failed to open a window");
        window.maximize();

        let config = config::Config::default(&video);
        assume!(log, config.load());
        assume!(log, config.save());

        let mut packs = config.asset_packs.borrow().clone();
        packs.insert(0, "base".to_owned());

        // Check if we are waiting for workshop items
        let mut waiting_for_workshop = false;
        #[cfg(feature = "steam")]
        {
            let ugc = steam.ugc();
            let mut extra_packs = vec![];
            for item in ugc.subscribed_items() {
                let state = ugc.item_state(item);
                if state.contains(steamworks::ItemState::NEEDS_UPDATE) {
                    waiting_for_workshop = true;
                    // No need to check the rest
                    break;
                }
                assert!(state.contains(steamworks::ItemState::INSTALLED));
                let info = assume!(log, ugc.item_install_info(item));
                extra_packs.push(format!("workshop:{}", info.folder));
            }
            if !waiting_for_workshop {
                packs.append(&mut extra_packs);
            }
        }

        let asset_manager = server::register_loaders(AssetManager::with_packs(&log, &packs))
            .register::<render::image::Loader>()
            .register::<server::entity::Loader<entity::ClientComponent>>()
            .build();

        let audio = sdl.audio().expect("Failed to load the audio backend");
        let audio = audio::AudioManager::new(&log, audio, asset_manager.clone());

        if let Some(renderer) =
            render::Renderer::new(&log, &window, asset_manager.clone(), config.clone())
        {
            match tick_game(
                log.clone(),
                window,
                renderer,
                audio,
                &asset_manager,
                #[cfg(feature = "steam")]
                steam.clone(),
                #[cfg(feature = "steam")]
                single_steam,
                config,
                waiting_for_workshop,
            ) {
                TickExitReason::GameEnd => return,
                #[cfg(feature = "steam")]
                TickExitReason::ReloadAssets(steam) => {
                    single_steam = steam;
                    continue;
                }
                #[cfg(not(feature = "steam"))]
                TickExitReason::ReloadAssets() => continue,
            }
        } else {
            return;
        }
    }
}

/// Contains the state of the game.
pub struct Game {
    running: bool,
    instance: Option<GameInstance>,
    // Used for the main menu
    dummy_instance: Option<main_menu::DummyInstance>,

    mouse_pos: (i32, i32),
    width: u32,
    height: u32,

    state: state::StateManager,
    game_state: GameState,
}

/// Contains state of the game (on a whole not a single instance)
/// to be passed around to those that need it
pub struct GameState {
    /// The game's assets
    pub asset_manager: AssetManager,
    /// The game's renderer
    pub renderer: render::Renderer,
    /// The game's ui manager
    pub ui_manager: ui::Manager,
    /// The game's audio manager
    pub audio: audio::AudioManager,
    /// The game's keybind handler
    pub keybinds: keybinds::BindTransformer,
    /// The game's window
    pub window: sdl2::video::Window,
    /// The game's configuration
    pub config: Rc<config::Config>,
    /// The last delta value
    pub delta: f64,
    /// The logger from instance should be preferred over this
    pub global_logger: Logger,
    /// Access to the steamworks API
    #[cfg(feature = "steam")]
    pub steam: steamworks::Client,
    /// Access to the steamworks API
    #[cfg(feature = "steam")]
    pub steam_single: steamworks::SingleClient,
    /// Access to the host filesystem (e.g. for save data)
    pub filesystem: BoxedFileSystem,
    /// Whether the game should restart
    pub should_restart: bool,
}

fn make_filesystem(#[cfg(feature = "steam")] steam: &steamworks::Client) -> BoxedFileSystem {
    #[cfg(feature = "steam")]
    {
        let rs = steam.remote_storage();
        if rs.is_cloud_enabled_for_account() && rs.is_cloud_enabled_for_app() {
            return JoinedFileSystem::new(
                SubfolderFileSystem::new(crate::server::steam::SteamCloud::new(rs), "saves"),
                NativeFileSystem::new(Path::new("./saves/")), // Fallback to legacy location
            )
            .into_boxed();
        }
    }
    return NativeFileSystem::new(Path::new("./saves/")).into_boxed();
}

fn update_window(state: &mut GameState) {
    if state.config.fullscreen_mode.get() == FullscreenType::True {
        let (w, h) = state.config.fullscreen_res.get();
        assume!(state.global_logger, state.window.set_size(w, h));
    } else {
        state.window.maximize();
    }
    assume!(
        state.global_logger,
        state
            .window
            .set_fullscreen(state.config.fullscreen_mode.get())
    );
    assume!(state.global_logger, state.window.set_display_mode(None));
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
#[inline]
unsafe fn pause_sse2(
    timer: &mut sdl2::TimerSubsystem,
    start: time::Instant,
    target: time::Duration,
) {
    let frame_time = start.elapsed();
    if frame_time < target {
        let ms = (target - frame_time).subsec_millis();
        // For anything smaller than 4ms the pause loop
        // should work better as sleeping the thread can
        // be imprecise
        if ms > 4 {
            timer.delay(ms);
        }
    }
    while start.elapsed() < target {
        #[cfg(target_arch = "x86_64")]
        {
            ::std::arch::x86_64::_mm_pause()
        }
        #[cfg(target_arch = "x86")]
        {
            ::std::arch::x86::_mm_pause()
        }
    }
}

#[inline]
fn get_pause_fn(
    log: &Logger,
) -> unsafe fn(&mut sdl2::TimerSubsystem, time::Instant, time::Duration) {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if is_x86_feature_detected!("sse2") {
            info!(log, "Using sse2 frame timing");
            return pause_sse2;
        }
    }
    info!(log, "Using standard frame timing");
    unsafe fn pause(
        timer: &mut sdl2::TimerSubsystem,
        start: time::Instant,
        target: time::Duration,
    ) {
        let frame_time = start.elapsed();
        if frame_time < target {
            let ms = (target - frame_time).subsec_millis();
            if ms > 0 {
                timer.delay(ms);
            }
        }
        while start.elapsed() < target {
            thread::yield_now();
        }
    }
    pause
}

enum TickExitReason {
    GameEnd,
    ReloadAssets(#[cfg(feature = "steam")] steamworks::SingleClient<steamworks::ClientManager>),
}

fn tick_game(
    log: Logger,
    window: sdl2::video::Window,
    mut renderer: render::Renderer,
    audio: audio::AudioManager,
    asset_manager: &AssetManager,
    #[cfg(feature = "steam")] steam: steamworks::Client,
    #[cfg(feature = "steam")] single_steam: steamworks::SingleClient,
    config: Rc<Config>,
    waiting_for_workshop: bool,
) -> TickExitReason {
    use crate::server::network;
    use std::env;
    let sdl = window.subsystem().sdl();
    let mut timer = sdl.timer().expect("Failed to get the timer");
    let mut sdl_events = sdl.event_pump().expect("Failed to get the event pump");

    let mut ui_manager =
        ui::Manager::new(log.new(o!("source" => "ui_manager")), asset_manager.clone());
    renderer.init_ui(&mut *ui_manager.manager.borrow_mut());
    renderer.set_ui_scale(1.0 / config.ui_scale.get());
    ui_manager.set_ui_scale(1.0 / config.ui_scale.get());
    ui_manager.clear_script_engine(&audio);

    let pause_func = get_pause_fn(&log);

    for pack in asset_manager.get_packs() {
        ui_manager.load_styles(ResourceKey::new(pack, "default"));
    }

    #[cfg(feature = "steam")]
    let mut target_lobby: Option<steamworks::LobbyId> = None;

    #[cfg(feature = "steam")]
    {
        let args = env::args();
        if let Some(lobby) = args.skip_while(|v| v != "+connect_lobby").nth(1) {
            target_lobby = u64::from_str_radix(&lobby, 10)
                .ok()
                .map(steamworks::LobbyId::from_raw);
        }
    }

    #[cfg(feature = "steam")]
    let state = if let Some(lobby) = target_lobby {
        state::StateManager::new(multiplayer::ConnectingState::<
            multiplayer::MenuState,
            network::SteamClientSocket,
            _,
        >::new(move |state| {
            SteamClientSocket::connect(
                &state.global_logger,
                state.steam.clone(),
                &state.steam_single,
                lobby,
            )
        }))
    } else if waiting_for_workshop {
        state::StateManager::new(main_menu::ModDownloadWait { ui: None })
    } else {
        state::StateManager::new(main_menu::MainMenuState::new())
    };
    #[cfg(not(feature = "steam"))]
    let state = state::StateManager::new(main_menu::MainMenuState::new());

    let mut game = Game {
        instance: None,
        dummy_instance: None,
        running: true,
        mouse_pos: (0, 0),
        width: 800,
        height: 480,

        state,
        game_state: GameState {
            asset_manager: asset_manager.clone(),
            renderer,
            ui_manager,
            audio,
            keybinds: keybinds::BindTransformer::new(),
            window,
            config,
            delta: 1.0,
            global_logger: log,
            #[cfg(feature = "steam")]
            filesystem: make_filesystem(&steam),
            #[cfg(not(feature = "steam"))]
            filesystem: NativeFileSystem::new(Path::new("./saves/")).into_boxed(),
            #[cfg(feature = "steam")]
            steam,
            #[cfg(feature = "steam")]
            steam_single: single_steam,
            should_restart: false,
        },
    };

    assume!(
        game.game_state.global_logger,
        game.game_state.keybinds.save()
    );

    let mut last_frame = Instant::now();
    let mut draw_ui = true;

    // Apply loaded settings
    game.game_state
        .audio
        .update_settings(&game.game_state.config);
    update_window(&mut game.game_state);

    // Capture steam events
    #[cfg(feature = "steam")]
    let (tx, rx) = channel();
    #[cfg(feature = "steam")]
    let _cb = game
        .game_state
        .steam
        .register_callback::<steamworks::GameLobbyJoinRequested, _>(move |join| {
            let _ = tx.send(SteamRequestJoinLobby(join.lobby_steam_id));
        });

    while game.running {
        let start = Instant::now();
        let diff = last_frame.elapsed();
        last_frame = start;
        let delta = diff.as_nanos() as f64 / (1_000_000_000.0 / 60.0);
        game.game_state.delta = delta;

        let (width, height) = game.game_state.window.drawable_size();
        game.width = width;
        game.height = height;

        // If we end up without a state (e.g. game end)
        // insert the main menu as a default
        if game.state.is_empty() {
            game.state.add_state(main_menu::MainMenuState::new());
        }
        if game.instance.is_some() && game.dummy_instance.is_some() {
            game.dummy_instance = None;
        } else if game.instance.is_none() && game.dummy_instance.is_none() {
            game.create_dummy();
        }

        let (cx, cy) = game.game_state.renderer.get_camera();
        let (rot, _) = game.game_state.renderer.get_camera_info();
        game.game_state.audio.tick(cx, cy, rot);

        {
            'events: for sdlevent in sdl_events.poll_iter() {
                let sdlevent = &sdlevent;

                // Core events that can't be handled by the bind system
                match *sdlevent {
                    Event::TextInput { ref text, .. } => {
                        for c in text.chars() {
                            game.game_state
                                .ui_manager
                                .focused_event::<ui::CharInputEvent>(ui::CharInput { input: c });
                        }
                    }
                    Event::MouseMotion {
                        x,
                        y,
                        xrel,
                        yrel,
                        mousestate,
                        ..
                    } => {
                        game.mouse_pos = (x, y);
                        game.game_state.renderer.set_mouse_position(x, y);
                        // UI always is handled first
                        if game.game_state.ui_manager.mouse_move(x, y) {
                            game.state.mouse_move_ui(
                                &mut game.instance,
                                &mut game.game_state,
                                game.mouse_pos,
                            );
                            continue 'events;
                        }
                        game.state.mouse_move(
                            &mut game.instance,
                            &mut game.game_state,
                            game.mouse_pos,
                        );
                        // If the game is active move the selection cursor to where we are looking at.
                        if let Some(instance) = game.instance.as_mut() {
                            if mousestate.middle() {
                                // Move the game's focus position
                                game.game_state.renderer.move_camera(
                                    (xrel as f32 / width as f32) * 25.0,
                                    (yrel as f32 / height as f32) * 25.0,
                                );
                            }

                            instance.mouse_move_event(&mut game.game_state, game.mouse_pos);
                            let (lx, ly) = game.game_state.renderer.mouse_to_level(x, y);
                            game.game_state.renderer.cursor.model_matrix =
                                cgmath::Matrix4::from_translation(cgmath::Vector3::new(
                                    lx.floor(),
                                    0.001,
                                    ly.floor(),
                                ));
                        }
                    }
                    Event::MouseButtonDown {
                        x, y, mouse_btn, ..
                    } => {
                        if game
                            .game_state
                            .ui_manager
                            .mouse_event::<ui::MouseDownEvent>(
                                x,
                                y,
                                ui::MouseClick {
                                    button: mouse_btn.into(),
                                    x: x,
                                    y: y,
                                },
                            )
                        {
                            continue 'events;
                        }
                    }
                    Event::MouseButtonUp {
                        x, y, mouse_btn, ..
                    } => {
                        if game.game_state.ui_manager.mouse_event::<ui::MouseUpEvent>(
                            x,
                            y,
                            ui::MouseClick {
                                button: mouse_btn.into(),
                                x: x,
                                y: y,
                            },
                        ) {
                            continue 'events;
                        }
                    }
                    Event::MouseWheel { y, .. } => {
                        if game
                            .game_state
                            .ui_manager
                            .mouse_event::<ui::MouseScrollEvent>(
                                game.mouse_pos.0,
                                game.mouse_pos.1,
                                ui::MouseScroll {
                                    x: game.mouse_pos.0,
                                    y: game.mouse_pos.1,
                                    scroll_amount: y,
                                },
                            )
                        {
                            continue 'events;
                        }
                    }
                    Event::KeyDown {
                        scancode: Some(sdl2::keyboard::Scancode::Grave),
                        ..
                    } => {
                        for pack in game.game_state.asset_manager.get_packs() {
                            game.game_state
                                .ui_manager
                                .load_styles(ResourceKey::new(pack, "default"));
                        }
                        continue 'events;
                    }
                    Event::KeyUp {
                        scancode: Some(sdl2::keyboard::Scancode::Grave),
                        ..
                    } => {
                        continue 'events;
                    }
                    Event::KeyDown {
                        keycode: Some(sdl2::keyboard::Keycode::Tab),
                        ..
                    } => {
                        game.game_state.ui_manager.cycle_focus();
                    }
                    Event::KeyDown {
                        scancode: Some(sdl2::keyboard::Scancode::F10),
                        ..
                    } => draw_ui = !draw_ui,
                    Event::KeyUp {
                        keycode: Some(key), ..
                    } => {
                        if game
                            .game_state
                            .ui_manager
                            .focused_event::<ui::KeyUpEvent>(ui::KeyInput { input: key })
                        {
                            continue 'events;
                        }
                    }
                    Event::KeyDown {
                        keycode: Some(key), ..
                    } => {
                        if game
                            .game_state
                            .ui_manager
                            .focused_event::<ui::KeyDownEvent>(ui::KeyInput { input: key })
                        {
                            continue 'events;
                        }
                    }
                    Event::Quit { .. } => {
                        game.running = false;
                        return TickExitReason::GameEnd;
                    }
                    _ => {}
                }
                // Handle general input actions
                if let Some(actions) = game.game_state.keybinds.transform(sdlevent) {
                    for action in actions {
                        game.handle_key_action(action);
                        game.game_state.renderer.handle_key_action(action);
                    }
                }
            }
        }

        if let Err(err) = game.handle_packets() {
            // TODO: This duplicates instance's disconnect
            //       handling.
            info!(
                game.game_state.global_logger,
                "Disconnected from the game: {:?}", err
            );
            // Disconnect the instance
            while !game.state.is_empty() {
                game.state.pop_state();
            }
            game.state.add_state(main_menu::MainMenuState::new());
        }
        game.tick(delta);

        if let Some(level) = game.instance.as_mut().map(|v| &mut v.level) {
            game.game_state.renderer.update_level(level);
        } else if let Some(level) = game.dummy_instance.as_mut().map(|v| &mut v.level) {
            game.game_state.renderer.update_level(level);
        }

        game.game_state
            .ui_manager
            .update(&mut game.game_state.renderer, delta);

        {
            let dummy = game.dummy_instance.as_mut();
            let entities = game
                .instance
                .as_mut()
                .map(|v| &mut v.entities)
                .or_else(|| dummy.map(|v| &mut v.entities));
            game.game_state.renderer.tick(
                entities,
                Some(&mut *game.game_state.ui_manager.manager.borrow_mut()),
                delta,
                width,
                height,
            );
        }

        // Take a screenshot before the UI is rendered for save icons
        if let Some(scr) = game
            .instance
            .as_mut()
            .and_then(|v| v.screenshot_helper.as_mut())
        {
            if scr.req.try_recv().is_ok() {
                let screenshot = take_screenshot(&game.game_state.global_logger, width, height);
                let _ = scr.reply.send(screenshot);
            }
        }

        if draw_ui {
            game.game_state
                .renderer
                .draw_ui(&mut *game.game_state.ui_manager.manager.borrow_mut());
        }

        game.game_state.window.gl_swap_window();
        #[cfg(feature = "steam")]
        game.game_state.steam_single.run_callbacks();

        #[cfg(feature = "steam")]
        for steam_evt in rx.try_iter() {
            game.game_state.ui_manager.events().emit(steam_evt);
        }

        if game.game_state.should_restart {
            return TickExitReason::ReloadAssets(
                #[cfg(feature = "steam")]
                game.game_state.steam_single,
            );
        }

        let target = game.game_state.config.target_fps.get();

        if target != i32::max_value() as u32 {
            let target_frame_time = Duration::from_secs(1) / target;
            unsafe {
                pause_func(&mut timer, start, target_frame_time);
            }
        }
    }
    TickExitReason::GameEnd
}

impl Game {
    fn tick(&mut self, delta: f64) {
        use crate::server::network;
        let events = { self.game_state.ui_manager.events().handle_events() };
        for mut evt in events {
            evt.handle_event::<ui::FocusNode, _>(|f| self.game_state.ui_manager.focus_node(f.0));
            evt.handle_event::<ExitGame, _>(|_| self.running = false);
            evt.handle_event::<SwitchMenu, _>(|e| self.switch_menu(&e.0));
            evt.handle_event::<SetCursor, _>(|s| self.game_state.renderer.set_mouse_sprite(s.0));
            #[cfg(feature = "steam")]
            evt.handle_event::<SteamRequestJoinLobby, _>(|e| {
                let lobby = e.0;
                self.state.pop_all();
                self.state.add_state(multiplayer::ConnectingState::<
                    multiplayer::MenuState,
                    network::SteamClientSocket,
                    _,
                >::new(move |state| {
                    SteamClientSocket::connect(
                        &state.global_logger,
                        state.steam.clone(),
                        &state.steam_single,
                        lobby,
                    )
                }));
            });
            self.state
                .ui_event(&mut self.instance, &mut self.game_state, &mut evt);
            if let Some(instance) = self.instance.as_mut() {
                instance.handle_ui_event(&mut evt, &mut self.game_state, &mut self.state);
            }
        }
        self.state.tick(&mut self.instance, &mut self.game_state);
        if let Some(instance) = self.instance.as_mut() {
            instance.tick(&mut self.game_state, &mut self.state, delta);
        }
    }

    fn handle_packets(&mut self) -> errors::Result<()> {
        if let Some(instance) = self.instance.as_mut() {
            return instance.handle_packets(&mut self.game_state, &mut self.state);
        }
        Ok(())
    }

    fn handle_key_action(&mut self, action: keybinds::KeyAction) {
        self.state.key_action(
            &mut self.instance,
            &mut self.game_state,
            action,
            self.mouse_pos,
        );
        if let Some(instance) = self.instance.as_mut() {
            instance.handle_key_action(action, &mut self.game_state, self.mouse_pos);
        }
    }

    fn switch_menu(&mut self, menu: &str) {
        // Remove the previous state
        self.state.pop_state();
        match menu {
            "main_menu" => self.state.add_state(main_menu::MainMenuState::new()),
            #[cfg(feature = "steam")]
            "modding" => self.state.add_state(main_menu::ModMenuState::new()),
            "options" => self.state.add_state(config::OptionsMenuState::new(false)),
            "singleplayer" => self.state.add_state(save_file::MenuState::new(
                server::saving::SaveType::FreePlay,
                |state, name| {
                    let (instance, _hosted_server) = GameInstance::single_player(
                        &state.global_logger,
                        &state.asset_manager,
                        #[cfg(feature = "steam")]
                        state.steam.clone(),
                        name.to_owned(),
                        None,
                    )
                    .expect("Failed to connect to single player instance");
                    Box::new(instance::BaseState::new(instance))
                },
            )),
            "campaign" => self.state.add_state(campaign::MenuState::new()),
            "multiplayer" => self.state.add_state(multiplayer::MenuState::new(None)),
            "credits" => self.state.add_state(credits::MenuState::new()),
            _ => error!(self.game_state.global_logger, "Unknown menu: {}", menu),
        }
    }

    fn create_dummy(&mut self) {
        let mut entities = ecs::Container::new();
        server::entity::register_components(&mut entities);
        entity::register_components(&mut entities);

        let level = assume!(
            self.game_state.global_logger,
            main_menu::create_level(
                &self.game_state.global_logger,
                &self.game_state.asset_manager,
                &mut self.game_state.ui_manager,
                &mut entities,
            )
        );
        self.game_state.renderer.set_level(&level);
        self.game_state
            .renderer
            .set_camera(32.0 + 17.0, 32.0 + 17.0 + 50.0);

        self.dummy_instance = Some(main_menu::DummyInstance { entities, level });
    }
}

fn take_screenshot(log: &Logger, width: u32, height: u32) -> Vec<u8> {
    use crate::render::gl;
    let mut buffer = vec![0; (width * height * 3) as usize];
    unsafe {
        gl::read_pixels(
            0,
            0,
            width as _,
            height as _,
            gl::TextureFormat::Rgb,
            gl::Type::UnsignedByte,
            &mut buffer,
        );
    }
    let mut output = vec![];
    {
        let out_size = server::saving::SAVE_ICON_SIZE;
        let mut enc = png::Encoder::new(&mut output, out_size.0, out_size.1);
        enc.set_color(png::ColorType::RGB);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = assume!(log, enc.write_header());

        let mut out_pix = vec![0; (out_size.0 * out_size.1 * 3) as usize];
        for y in 0..out_size.1 {
            for x in 0..out_size.0 {
                let o_idx = ((x + y * out_size.0) * 3) as usize;
                let i_x = (x * width) / out_size.0;
                let i_y = height - 1 - ((y * height) / out_size.1);
                let i_idx = ((i_x + i_y * width) * 3) as usize;

                out_pix[o_idx + 2] = buffer[i_idx + 2];
                out_pix[o_idx + 1] = buffer[i_idx + 1];
                out_pix[o_idx] = buffer[i_idx];
            }
        }
        assume!(log, writer.write_image_data(&out_pix));
    }

    output
}

/// Attempts to open the passed url in the user's default
/// browser.
#[cfg(target_os = "linux")]
pub fn open_url(url: &url::Url) -> UResult<()> {
    use std::process::Command;
    Command::new("xdg-open").arg(url.to_string()).status()?;
    Ok(())
}

/// Attempts to open the passed url in the user's default
/// browser.
#[cfg(target_os = "windows")]
pub fn open_url(url: &url::Url) -> UResult<()> {
    use std::process::Command;
    Command::new("cmd")
        .arg("/c")
        .arg("start")
        .arg(url.to_string())
        .status()?;
    Ok(())
}

/// Attempts to open the passed url in the user's default
/// browser.
#[cfg(target_os = "macos")]
pub fn open_url(url: &url::Url) -> UResult<()> {
    use std::process::Command;
    Command::new("open").arg(url.to_string()).status()?;
    Ok(())
}

struct ExitGame;
struct SwitchMenu(String);
#[cfg(feature = "steam")]
struct SteamRequestJoinLobby(steamworks::LobbyId);
struct SetCursor(ResourceKey<'static>);
