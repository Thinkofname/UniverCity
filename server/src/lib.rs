#![recursion_limit = "4096"]
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
// Clippy bug? https://github.com/rust-lang-nursery/rust-clippy/issues/1725
#![allow(clippy::get_unwrap)]
// Sometimes things get complex
#![allow(clippy::cyclomatic_complexity)]
// I generally use this correctly. Its mostly done for
// networking.
#![allow(clippy::float_cmp)]
// Not making a library
#![allow(clippy::should_implement_trait)]
#![allow(clippy::clone_on_ref_ptr)]
#![allow(clippy::option_map_unit_fn)]
#![allow(clippy::needless_pass_by_value)]
// Unwrap makes tracking crashes harder, use `assume!`
#![cfg_attr(not(test), deny(clippy::option_unwrap_used))]
#![cfg_attr(not(test), deny(clippy::result_unwrap_used))]

//! Base of the game's server. Nothing too special

#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate bitflags;

use rand;

extern crate think_ecs as ecs;
#[macro_use]
extern crate error_chain;
pub use lua;
#[macro_use]
extern crate univercity_util as util;
#[cfg(feature = "debugutil")]
extern crate backtrace;

#[macro_use]
extern crate slog;
#[cfg(test)]
extern crate slog_async;
#[cfg(test)]
extern crate slog_term;
#[macro_use]
extern crate delta_encode;

#[cfg(feature = "steam")]
pub extern crate steamworks;

pub use serde_cbor;
pub use serde_transcode;

#[cfg(feature = "debugutil")]
extern crate png;

#[macro_use]
pub mod script;
pub mod assets;
pub mod event;
pub mod level;
#[macro_use]
pub mod network;
#[macro_use]
pub mod command;
pub mod choice;
pub mod common;
pub mod entity;
pub mod errors;
pub mod mission;
pub mod msg;
pub mod notify;
pub mod player;
pub mod prelude;
pub mod saving;
mod spawning;
pub mod steam;

pub use crate::prelude::UResult;

mod script_room;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

use crate::assets::AssetsBuilder;
use crate::entity::snapshot::Snapshots;
use crate::prelude::*;
use delta_encode::AlwaysVec;
use lua::{Ref, Table};

/// The current commit hash for this build
pub const GAME_HASH: &str = env!("GAME_HASH");

/// Registers the loaders required by the server
pub fn register_loaders(builder: AssetsBuilder) -> AssetsBuilder {
    builder
        .register::<tile::Loader>() // Shares data with ById too
        .register::<room::Loader>()
        .register::<object::Loader>()
        .register::<Loader<ServerComponent>>()
}

/// Server initial configuration
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// The type of save file to use
    pub save_type: saving::SaveType,
    /// The name of the save file.
    pub save_name: String,
    /// Controls the minimum number of players
    /// required before a game can be started
    pub min_players: u32,
    /// Controls the maximum number of players
    /// allowed on the server.
    pub max_players: u32,
    /// Controls whether the server will automatically
    /// start when the number of players reaches
    /// `min_players`
    pub autostart: bool,
    /// The width/height of the area each player will get
    pub player_area_size: u32,
    /// Prevents new players joining. Should only be set by
    /// the server
    pub locked_players: bool,
    /// The name of the mission currently controling this instance
    /// if any.
    pub mission: Option<ResourceKey<'static>>,
    /// The tick rate of the server, default: 20
    pub tick_rate: Cell<u32>,
}

type PlayerInfoMap = FNVMap<PlayerId, PlayerInfo>;

/// A single server instance
pub struct Server<S: SocketListener, Steam> {
    /// The logger used by the server
    pub log: Logger,
    /// Access to the steamworks API
    pub steam: Steam,
    config: ServerConfig,
    state: ServerState,
    fs: saving::filesystem::BoxedFileSystem,

    asset_manager: AssetManager,
    network: NetworkManager<S>,
    /// State for connected players
    players: FNVMap<<S::Socket as Socket>::Id, NetworkedPlayer<S::Socket>>,
    /// Information about players currently in the game,
    /// connected or not.
    players_info: PlayerInfoMap,

    next_uid: i16,

    shutdown_channel: mpsc::Sender<()>,
    force_save: bool,
    icon_capture: Option<Box<dyn saving::IconCapture>>,
    command_submitter: Option<mpsc::Receiver<String>>,
}

#[allow(clippy::large_enum_variant)] // Other variants aren't used much anyway
enum ServerState {
    /// State for when the server is waiting for
    /// players to join.
    Lobby {
        /// Used to ignore duplicate updates
        change_id: u32,
        /// Set to true when the state of the lobby has
        /// changed
        state_dirty: bool,
    },
    BeginGame,
    Playing {
        // The name of the save file.
        save_name: String,
        level: Level,

        spawning: spawning::Spawner,
        // Whether the game is paused or not
        paused: bool,

        scripting: ScriptEngine,
        mission: Option<mission::MissionController>,
        extra_commands: Rc<RefCell<Vec<command::Command>>>,

        entities: Container,
        entity_systems: Systems,
        snapshots: Snapshots,
        choices: choice::Choices,
        running_choices: script_room::RunningChoices,

        // Used by systems to request entities
        // for a room to use
        entity_dispatcher: EntityDispatcher,
        // Pathfinding queue handler
        pathfinder: Pathfinder,
        // Used by systems to know the time
        // of day and the current day
        day_tick: DayTick,
        server_player: script_room::ServerPlayer,
    },
}

/// Loads rules and optionally setups storage for them
pub fn init_rule_vars(
    log: &Logger,
    entities: Option<&mut Container>,
    assets: &AssetManager,
) -> choice::Choices {
    use crate::choice::VariableAllocator;
    // Setup dynamic entity variables
    let mut globals = choice::BasicAlloc::new(());
    assume!(log, globals.storage_loc(choice::Type::Integer, "time"));

    let mut student_alloc = choice::BasicAlloc::new(globals);
    for stat in Stats::STUDENT.stats() {
        let id = assume!(
            log,
            student_alloc.storage_loc(choice::Type::Float, stat.as_string())
        );
        assert_eq!(id, stat.index as u16);
    }
    let student_c = choice::ChoiceSelector::<choice::ScriptChoice>::new(
        log,
        assets,
        &mut student_alloc,
        "student",
        choice::convert_script_choice,
    );
    let (student_alloc, globals) = student_alloc.remove_global();

    let mut professor_alloc = choice::BasicAlloc::new(globals);
    for stat in Stats::PROFESSOR.stats() {
        let id = assume!(
            log,
            professor_alloc.storage_loc(choice::Type::Float, stat.as_string())
        );
        assert_eq!(id, stat.index as u16);
    }
    let (professor_alloc, globals) = professor_alloc.remove_global();

    let mut office_worker_alloc = choice::BasicAlloc::new(globals);
    for stat in Stats::OFFICE_WORKER.stats() {
        let id = assume!(
            log,
            office_worker_alloc.storage_loc(choice::Type::Float, stat.as_string())
        );
        assert_eq!(id, stat.index as u16);
    }
    let (office_worker_alloc, globals) = office_worker_alloc.remove_global();

    let mut janitor_alloc = choice::BasicAlloc::new(globals);
    for stat in Stats::JANITOR.stats() {
        let id = assume!(
            log,
            janitor_alloc.storage_loc(choice::Type::Float, stat.as_string())
        );
        assert_eq!(id, stat.index as u16);
    }
    let (janitor_alloc, globals) = janitor_alloc.remove_global();

    let choices = choice::Choices {
        global: choice::GlobalMemory::new(globals),
        student_idle: student_c,
    };

    if let Some(entities) = entities {
        entities.register_component_self::<choice::StudentVars>(choice::EntityVarStorage::create(
            student_alloc,
            choice::StudentVars,
        ));
        entities.register_component_self::<choice::ProfessorVars>(
            choice::EntityVarStorage::create(professor_alloc, choice::ProfessorVars),
        );
        entities.register_component_self::<choice::OfficeWorkerVars>(
            choice::EntityVarStorage::create(office_worker_alloc, choice::OfficeWorkerVars),
        );
        entities.register_component_self::<choice::JanitorVars>(choice::EntityVarStorage::create(
            janitor_alloc,
            choice::JanitorVars,
        ));
    }

    choices
}

impl ServerState {
    fn create_play_state<F: saving::filesystem::FileSystem>(
        log: &Logger,
        assets: &AssetManager,
        fs: &F,
        players_info: &mut PlayerInfoMap,
        config: &ServerConfig,
        players: &[PlayerId],
    ) -> ServerState {
        let mut entities = Container::new();
        entity::register_components(&mut entities);

        entities.add_component(Container::WORLD, CLogger { log: log.clone() });
        entities.add_component(
            Container::WORLD,
            course::LessonManager::new(log.clone(), assets),
        );

        let mut systems = Systems::new();
        entity::register_systems(&mut systems);
        entity::register_server_systems(&mut systems);

        let choices = init_rule_vars(log, Some(&mut entities), assets);

        let scripting = {
            let scripting = ScriptEngine::new(log, assets.clone());
            for pack in assets.get_packs() {
                scripting.init_pack(pack.module());
            }
            scripting
        };
        let mut running_choices = script_room::RunningChoices::new(&scripting);
        let mut snapshots = Snapshots::new(log, players);
        scripting.store_tracked::<snapshot::EntityMap>(snapshot::EntityMap(
            snapshots.entity_map.clone(),
        ));
        scripting.set(
            lua::Scope::Global,
            "idle_storage",
            running_choices.choice_map.clone(),
        );

        {
            let lua_players = players.iter().fold(Ref::new_table(&scripting), |tbl, v| {
                tbl.insert(tbl.length() + 1, i32::from(v.0));
                tbl
            });
            assume!(
                log,
                scripting.invoke_function::<_, ()>("set_control_players", lua_players)
            );
        }

        let mut day_tick = DayTick {
            current_tick: 0,
            day: 0,
            time: 0,
        };

        let mut mission = config.mission.as_ref().map(|v| {
            let log = log.new(o!("type" => "mission_controller"));
            mission::MissionController::new(log, scripting.clone(), v.borrow())
        });

        let level = match saving::load_game(
            fs,
            log,
            &config.save_name,
            config.save_type,
            players_info,
            assets,
            &mut entities,
            &mut snapshots,
            &scripting,
            &choices,
            &mut running_choices,
            mission.as_mut(),
            &mut day_tick,
        ) {
            Ok(sav) => sav,
            Err(UError(ErrorKind::NoSuchSave, _)) => {
                let lvl = Level::new::<ServerEntityCreator, _>(
                    log.new(o!("type" => "level")),
                    &scripting,
                    assets,
                    &mut entities,
                    players,
                    config.player_area_size,
                )
                .expect("Failed to spawn level");
                mission
                    .as_mut()
                    .map(|v| v.init(players_info, &mut entities, None));
                lvl
            }
            Err(err) => panic!("Failed to load the save file: {:?}", err),
        };
        // Let choices load
        script_room::tick_choices(
            log,
            &mut entities,
            &scripting,
            players_info,
            &choices,
            &mut running_choices,
        );

        let extra_commands = Rc::new(RefCell::new(vec![]));
        scripting.store_tracked::<script_room::ExtraCommands>(extra_commands.clone());

        ServerState::Playing {
            save_name: config.save_name.clone(),
            level,
            spawning: spawning::Spawner::new(log, players),
            paused: false,
            day_tick,
            scripting,
            mission,
            entities,
            entity_systems: systems,
            snapshots,
            choices,
            running_choices,
            entity_dispatcher: EntityDispatcher::new(),
            pathfinder: Pathfinder::new(Duration::from_millis(10)),
            server_player: script_room::ServerPlayer {
                state: player::State::None,
                config: player::PlayerConfig::default(),
            },
            extra_commands,
        }
    }
}

impl<S: SocketListener, Steam: steam::Steam> Server<S, Steam> {
    /// Creates a new server that listens for connects on the passed address
    pub fn new<A>(
        log: Logger,
        asset_manager: AssetManager,
        steam: Steam,
        fs: saving::filesystem::BoxedFileSystem,
        addr: A,
        mut config: ServerConfig,
        icon_capture: Option<Box<dyn saving::IconCapture>>,
        command_submitter: Option<mpsc::Receiver<String>>,
    ) -> UResult<(Server<S, Steam>, mpsc::Receiver<()>)>
    where
        A: Into<<S as SocketListener>::Address>,
    {
        let addr = addr.into();
        info!(log, "Starting a server at: {:?}", S::format_address(&addr));
        info!(log, "{:#?}", config);
        let network = NetworkManager::new(&log, addr)?;

        let mut players_info = Default::default();

        let (shutdown_channel, shutdown_wait) = mpsc::channel();

        let locked_players = match saving::load_players(
            &fs,
            &log,
            &config.save_name,
            config.save_type,
            &asset_manager,
            &mut players_info,
        ) {
            Ok(_) => true,
            Err(UError(ErrorKind::NoSuchSave, _)) => false,
            Err(err) => panic!("Failed to load the save file: {:?}", err),
        };
        config.locked_players = locked_players;

        Ok((
            Server {
                state: ServerState::Lobby {
                    change_id: 0,
                    state_dirty: false,
                },
                log,
                steam,
                fs,
                network,
                players: Default::default(),
                players_info,

                asset_manager,
                next_uid: 1,

                config,
                shutdown_channel,
                icon_capture,
                command_submitter,
                force_save: false,
            },
            shutdown_wait,
        ))
    }

    /// Runs the server's ticking logic. Returns when the server is closing
    pub fn run(&mut self) {
        'server_loop: loop {
            let start = Instant::now();

            self.tick();
            if let ServerState::Playing {
                ref save_name,
                ref mut entities,
                ref mut entity_systems,
                ref mut level,
                ref mut day_tick,
                ref mut snapshots,
                ref scripting,
                ref mut mission,
                ref mut entity_dispatcher,
                ref mut pathfinder,
                ref mut spawning,
                ref paused,
                ref mut choices,
                ref mut running_choices,
                ..
            } = self.state
            {
                script::handle_reloads(&self.log, scripting, &self.asset_manager);
                if !*paused {
                    script_room::tick_rooms(
                        &self.log,
                        level,
                        entities,
                        scripting,
                        &mut self.players_info,
                    );
                    entity::free_roam::server_tick(
                        &self.log,
                        entities,
                        scripting,
                        &mut self.players_info,
                    );

                    for player in self.players_info.values_mut() {
                        let network = &mut self.network;
                        let networked_player = self
                            .players
                            .iter_mut()
                            .find(|v| v.1.uid == Some(player.uid))
                            .and_then(|v| network.get_connection(v.0).map(move |c| (v.1, c)));

                        player.tick(
                            &self.log,
                            networked_player,
                            &self.asset_manager,
                            scripting,
                            level,
                            entities,
                            day_tick,
                        );
                    }

                    day_tick.current_tick += 1;
                    if day_tick.current_tick >= LESSON_LENGTH * 4 {
                        day_tick.day = day_tick.day.wrapping_add(1);
                        day_tick.current_tick -= LESSON_LENGTH * 4;
                    }
                    day_tick.time = day_tick.time.wrapping_add(1);
                    choices.global.set_int("time", day_tick.time as i32);
                    scripting.set(lua::Scope::Global, "global_time", day_tick.time as i32);

                    if day_tick.current_tick % LESSON_LENGTH == 0 || self.force_save {
                        self.force_save = false;
                        info!(self.log, "Saving the game");
                        saving::save_game(
                            &mut self.fs,
                            save_name,
                            self.config.save_type,
                            &mut self.players_info,
                            level,
                            entities,
                            scripting,
                            choices,
                            running_choices,
                            mission.as_mut(),
                            day_tick,
                            self.icon_capture.as_ref().map(|v| v.as_ref()),
                        )
                        .expect("Failed to save the game");
                    }

                    spawning.handle_spawning(
                        &self.asset_manager,
                        &self.players_info,
                        level,
                        entities,
                        scripting,
                    );
                    {
                        let pi = &mut self.players_info;
                        mission.as_mut().map(|v| v.update(pi, entities));
                    }

                    script_room::tick_choices(
                        &self.log,
                        entities,
                        scripting,
                        &mut self.players_info,
                        choices,
                        running_choices,
                    );
                    entity_systems
                        .run_with_borrows(entities)
                        .borrow(&*level.tiles.borrow())
                        .borrow(&*level.rooms.borrow())
                        .borrow(&self.asset_manager)
                        .borrow_mut(entity_dispatcher)
                        .borrow_mut(pathfinder)
                        .borrow_mut(&mut self.players_info)
                        .borrow(day_tick)
                        .run();
                    Self::sync_state(
                        entities,
                        *day_tick,
                        snapshots,
                        &mut self.network,
                        &self.players,
                        &self.players_info,
                    );
                }
            }

            if <S::Socket as Socket>::is_local() && self.players.is_empty() {
                // Single player has disconnected
                info!(&self.log, "Single player(local) shutdown");
                break;
            }

            if let Some(host) = self.network.get_host() {
                if !self.network.is_connection_open(&host) {
                    info!(&self.log, "Hosted server shutdown");
                    break;
                }
            }

            if let Some(cmds) = self.command_submitter.as_ref() {
                for cmd in cmds.try_iter() {
                    match cmd.as_str() {
                        "quit" => {
                            info!(self.log, "Server shutdown");
                            break 'server_loop;
                        }
                        "save" => {
                            self.force_save = true;
                            info!(self.log, "Forcing a save");
                        }
                        cmd => warn!(self.log, "Invalid command: {}", cmd),
                    }
                }
            }

            let target_frame_time = Duration::from_secs(1) / self.config.tick_rate.get();
            let frame_time = start.elapsed();
            if frame_time < target_frame_time {
                thread::sleep(target_frame_time - frame_time);
            }
        }

        if let ServerState::Playing {
            ref save_name,
            ref mut level,
            ref mut entities,
            ref scripting,
            ref mut day_tick,
            ref mut mission,
            ref choices,
            ref running_choices,
            ..
        } = self.state
        {
            saving::save_game(
                &mut self.fs,
                save_name,
                self.config.save_type,
                &mut self.players_info,
                level,
                entities,
                scripting,
                choices,
                running_choices,
                mission.as_mut(),
                day_tick,
                self.icon_capture.as_ref().map(|v| v.as_ref()),
            )
            .expect("Failed to save the game");
        }
        // Don't care about the error here as not all users of the server
        // wait on the channel.
        let _ = self.shutdown_channel.send(());
    }

    fn sync_state(
        entities: &mut Container,
        day_tick: DayTick,
        snapshots: &mut entity::snapshot::Snapshots,
        network: &mut NetworkManager<S>,
        players: &FNVMap<<S::Socket as Socket>::Id, NetworkedPlayer<S::Socket>>,
        player_info: &FNVMap<PlayerId, PlayerInfo>,
    ) {
        snapshots.capture(entities, day_tick, player_info.iter());
        'sync: for connection in network.connections() {
            let player = match players.get(&connection.id) {
                Some(val) => val,
                None => continue,
            };
            if player.remote_state != PlayerState::Playing {
                continue;
            }
            let packets = snapshots.create_delta(player);
            for packet in packets {
                if connection.send(packet).is_err() {
                    continue 'sync;
                }
            }
        }
    }

    fn tick(&mut self) {
        use std::mem;
        self.network.tick();
        let mut new_commands = Vec::with_capacity(4);

        if let ServerState::Playing {
            ref mut mission,
            ref mut level,
            ref scripting,
            ref mut entities,
            ref mut snapshots,
            ref mut server_player,
            ref extra_commands,
            ref running_choices,
            ref choices,
            ..
        } = self.state
        {
            let mut list = vec![];
            let handler = if let Some(mission) = mission.as_mut() {
                let mut glist = mission.generated_commands.borrow_mut();
                list.append(&mut *glist);
                Some(mission.handler.clone())
            } else {
                None
            };
            {
                let mut cmds = extra_commands.borrow_mut();
                list.append(&mut *cmds);
            }
            if !list.is_empty() {
                let mut done_cmds = Vec::with_capacity(list.len());
                for mut cmd in list.drain(..) {
                    let mut h = script_room::Handler {
                        running_choices,
                        choices,
                    };

                    match cmd.execute(
                        &mut h,
                        server_player,
                        command::CommandParams {
                            log: &self.log,
                            level,
                            engine: scripting,
                            entities,
                            snapshots,
                            mission_handler: handler.as_ref().map(|v| v.borrow()),
                        },
                    ) {
                        Ok(_) => {
                            if cmd.should_sync() {
                                done_cmds.push(cmd)
                            }
                        }
                        Err(err) => {
                            error!(self.log, "failed to exec command: {:?}", err);
                            break;
                        }
                    };
                }
                new_commands.push((PlayerId(0), done_cmds));
            }
        }
        let mut messages = vec![];
        let log = &self.log;
        for connection in self.network.connections() {
            let id = connection.id.clone();
            let player = self
                .players
                .entry(id.clone())
                .or_insert_with(|| NetworkedPlayer::new(log, id));
            if let Some(info) = player.handle_packets(
                &mut self.state,
                &self.asset_manager,
                &mut self.fs,
                &self.config,
                connection,
                self.next_uid,
                &mut self.players_info,
                &self.steam,
            ) {
                self.next_uid += 1;
                self.players_info.insert(info.uid, info);
            }
            if player.wants_save {
                self.force_save = true;
                player.wants_save = false;
            }
            // Timeout check
            if !<S::Socket as Socket>::is_local()
                && Instant::now().duration_since(player.last_packet) > Duration::from_secs(15)
            {
                player.local_state = PlayerState::Closed;
                player.remote_state = PlayerState::Closed;
            }
            if player.local_state == PlayerState::Closed
                || player.remote_state == PlayerState::Closed
            {
                connection.close();
            }
            if let Some(uid) = player.uid {
                if let Some(info) = self.players_info.get(&uid) {
                    // Collect commands from the player
                    let cmds = mem::replace(&mut player.commands, vec![]);
                    new_commands.push((info.uid, cmds));
                    messages.append(&mut player.messages);
                }
            }
        }

        {
            let network = &self.network;
            let players_info = &mut self.players_info;
            let state = &mut self.state;
            let log = &self.log;
            #[cfg(feature = "steam")]
            let steam = &self.steam;
            self.players.retain(|id, player| {
                if !network.is_connection_open(id) || player.local_state == PlayerState::Closed {
                    if let Some(uid) = player.uid {
                        let info = assume!(log, players_info.get_mut(&uid));
                        // If playing via steam drop the session
                        match info.key {
                            #[cfg(feature = "steam")]
                            player::PlayerKey::Steam(id) => {
                                info!(log, "Player disconnected"; "steam_id" => ?id);
                                steam.end_authentication_session(id)
                            }
                            #[cfg(not(feature = "steam"))]
                            player::PlayerKey::Username(ref name) => {
                                info!(log, "Player disconnected"; "username" => name)
                            }
                        }
                    }
                    if let Some(uid) = player.uid {
                        let info = assume!(log, players_info.get_mut(&uid));
                        let msg = crate::msg::Message::new()
                            .color(130, 237, 123)
                            .text(info.name.as_str())
                            .color(255, 255, 0)
                            .text(" has left the server")
                            .build();
                        messages.push(msg);
                    }
                    // Players can freely drop in the lobby.
                    // The game however we keep their spot.
                    match *state {
                        ServerState::Lobby { change_id, .. } => {
                            if let Some(uid) = player.uid {
                                players_info.remove(&uid);
                                *state = ServerState::Lobby {
                                    change_id,
                                    state_dirty: true,
                                };
                            }
                        }
                        ServerState::BeginGame => {}
                        ServerState::Playing { .. } => {
                            // If they were building something, revert that
                            if let Some(uid) = player.uid {
                                let info = assume!(log, players_info.get_mut(&uid));
                                match info.state {
                                    player::State::EditEntity { .. } => {
                                        // TODO: Can't cancel staff placement yet
                                        info.state = player::State::None;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    false
                } else {
                    true
                }
            });
        }

        if !messages.is_empty() {
            for connection in self.network.connections() {
                let id = connection.id.clone();
                let player = self
                    .players
                    .entry(id.clone())
                    .or_insert_with(|| NetworkedPlayer::new(log, id));
                if let Some(uid) = player.uid {
                    if self.players_info.get(&uid).is_some() {
                        let _ = connection.ensure_send(packet::Message {
                            messages: AlwaysVec(messages.clone()),
                        });
                    }
                }
            }
        }

        match self.state {
            // If there is a change in the lobby update players
            ServerState::Lobby {
                change_id,
                state_dirty: true,
            } => {
                let id = change_id + 1;
                self.state = ServerState::Lobby {
                    change_id: id,
                    state_dirty: false,
                };

                let players: Vec<_> = self
                    .players
                    .values()
                    .filter_map(|p| p.uid.map(|v| (v, p)))
                    .filter_map(|(uid, p)| self.players_info.get(&uid).map(|v| (uid, v, p)))
                    .map(|(uid, info, p)| packet::LobbyEntry {
                        #[cfg(feature = "steam")]
                        steam_id: match info.key {
                            player::PlayerKey::Steam(key) => key.raw(),
                        },
                        uid,
                        ready: p.remote_state == PlayerState::Lobby,
                    })
                    .collect();

                let player_count = self
                    .players
                    .values()
                    .filter(|p| p.remote_state == PlayerState::Lobby)
                    .count();
                let can_start = player_count as u32 >= self.config.min_players
                    && player_count as u32 <= self.config.max_players;

                // Update players in the lobby with the new info
                for connection in self.network.connections() {
                    if let Some(&mut NetworkedPlayer {
                        remote_state: PlayerState::Lobby,
                        ..
                    }) = self.players.get_mut(&connection.id)
                    {
                        // The error will be handled later
                        let _ = connection.ensure_send(packet::UpdateLobby {
                            change_id: id,
                            players: AlwaysVec(players.clone()),
                            can_start,
                        });
                    }
                }

                if can_start && self.config.autostart {
                    self.state = ServerState::BeginGame;
                }
            }
            ServerState::BeginGame => {
                let players: Vec<_> = self
                    .players_info
                    .values()
                    .map(|p| packet::PlayerEntry {
                        uid: p.uid,
                        username: p.name.clone(),
                        state: p.state.clone(),
                    })
                    .collect();

                let player_ids: Vec<PlayerId> = players.iter().map(|v| v.uid).collect();
                self.state = ServerState::create_play_state(
                    &self.log,
                    &self.asset_manager,
                    &mut self.fs,
                    &mut self.players_info,
                    &self.config,
                    &player_ids,
                );

                if let ServerState::Playing {
                    ref level,
                    ref mission,
                    ref mut entities,
                    ref scripting,
                    ref choices,
                    ref mut running_choices,
                    ..
                } = self.state
                {
                    let (lstr, lstate) = level.create_initial_state();
                    let idle = script_room::create_choices_state(
                        log,
                        entities,
                        scripting,
                        choices,
                        running_choices,
                    );
                    for connection in self.network.connections() {
                        if let Some(player) = self.players.get_mut(&connection.id) {
                            if player.remote_state == PlayerState::Lobby && player.uid.is_some() {
                                let _ = connection.ensure_send(packet::GameBegin {
                                    uid: assume!(self.log, player.uid).0,
                                    width: level.width,
                                    height: level.height,
                                    players: AlwaysVec(players.clone()),
                                    mission_handler: mission
                                        .as_ref()
                                        .map(|v| v.handler.borrow().into_owned()),
                                    strings: AlwaysVec(lstr.clone()),
                                    state: lstate.clone(),
                                    idle_state: idle.clone(),
                                });
                                player.remote_state = PlayerState::Loading;
                                player.local_state = PlayerState::Playing;
                            }
                        }
                    }
                }
            }
            ServerState::Playing { .. } => {
                // Copy commands from each player to all the other players
                for player in self.players.values_mut() {
                    // Don't send commands to players that are still connecting
                    if player.remote_state == PlayerState::Connecting
                        || player.remote_state == PlayerState::Lobby
                    {
                        continue;
                    }
                    let uid = if let Some(uid) = player.uid {
                        uid
                    } else {
                        // They might be rejoining, ignore for now
                        continue;
                    };
                    for ncmds in &new_commands {
                        // Don't send the player their own commands
                        if ncmds.0 == uid {
                            continue;
                        }
                        for cmd in &ncmds.1 {
                            player.remote_commands.commands.push((
                                player.remote_commands.next_id,
                                ncmds.0,
                                cmd.clone(),
                            ));
                            player.remote_commands.next_id =
                                player.remote_commands.next_id.wrapping_add(1);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

impl<Steam: steam::Steam> Server<LoopbackSocketListener, Steam> {
    /// Returns the socket to be used by the local client
    pub fn client_localsocket(&mut self) -> LoopbackSocket {
        self.network.client_localsocket()
    }
}

#[cfg(feature = "steam")]
impl<Steam: steam::Steam> Server<SteamSocketListener, Steam> {
    /// Returns the socket to be used by the local client
    pub fn client_localsocket(&mut self) -> SteamSocket {
        self.network.client_localsocket()
    }
}

/// Information about a mod
#[derive(Debug, Serialize, Deserialize)]
pub struct ModMeta {
    /// The main name of the mod
    pub main: String,
    /// The steamworks id of the mod
    #[cfg(feature = "steam")]
    pub workshop_id: steamworks::PublishedFileId,
}
