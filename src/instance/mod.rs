//! Game instance management

use crate::util::FNVMap;
#[cfg(feature = "steam")]
use server::steamworks;
use std::sync::mpsc;
use std::thread;
use std::time;

// States
mod base;
pub use self::base::BaseState;
mod build;
pub(crate) mod scripting;

use crate::ecs;
use crate::entity;
use crate::errors;
use crate::keybinds;
use crate::prelude::*;
use crate::script;
use crate::server;
use crate::server::entity::snapshot;
use crate::server::event;
use crate::server::level;
use crate::server::lua;
use crate::server::lua::{Ref, Table};
use crate::server::network::{self, packet};
use crate::server::player::State;
use crate::server::saving::filesystem::*;
use crate::state;
use crate::ui;
use delta_encode::{bitio, AlwaysVec, DeltaEncodable};

/// A instance of a game session
pub struct GameInstance {
    /// The logger for this instance of the game
    pub log: Logger,
    /// The assets for this instance
    ///
    /// Different instances may have different packs loaded
    pub asset_manager: AssetManager,
    #[cfg(feature = "steam")]
    steam: steamworks::Client,

    /// The level of the current game
    pub level: Level,
    sender: Sender,
    receiver: Receiver,
    // Generally the client doesn't care if its connected
    // to a local server or remote but a local server
    // can be paused.
    is_local: bool,
    paused: bool,

    remote_network_state: NetworkState,
    local_network_state: NetworkState,
    disconnect_reason: Option<errors::Error>,
    shutdown_waiter: Option<mpsc::Receiver<()>>,
    /// Should be cancelled on disconnect
    #[cfg(feature = "steam")]
    pub(crate) auth_ticket: Option<steamworks::AuthTicket>,

    /// Local player instance
    pub player: PlayerInfo,
    // Other players
    players: FNVMap<player::Id, RemotePlayer>,
    last_command: u32,

    last_tick: f64,

    // Number of ticks until next keep alive packet
    next_keep_alive: i8,
    last_keep_alive_reply: time::Instant,
    // Command tracking
    next_command_id: u32,
    // List of command we've executed recently.
    // We keep this in case on of them fails when the server validates
    // them so we can rollback.
    commands: Vec<(u32, Command, state::PossibleCapture)>,
    request_manager: network::RequestManager,

    /// The scripting engine for this instance
    pub scripting: script::Engine,

    /// Entities active in this instance
    pub entities: ecs::Container,
    systems: ecs::Systems,
    frame_systems: ecs::Systems,
    /// The snapshot system used to sync'ing entities over
    /// the network.
    pub snapshots: snapshot::Snapshots,
    entity_state: snapshot::EntitySnapshotState,
    player_state: u16,
    running_choices: scripting::RunningChoices,
    day_tick: DayTick,

    pathfinder: Pathfinder,

    /// The last known cursor position
    pub last_cursor_position: entity::CursorPosition,

    notifications: Vec<base::Notification>,
    delayed_notifications: Vec<server::notify::Notification>,
    notification_next_id: u32,
    chat_messages: Vec<Message>,

    pub(crate) screenshot_helper: Option<ScreenshotHelper>,

    server_player: ServerPlayer,
    mission_handler: Option<ResourceKey<'static>>,
}

pub(crate) struct ScreenshotHelper {
    pub req: mpsc::Receiver<()>,
    pub reply: mpsc::Sender<Vec<u8>>,
}

struct ServerScreenshot {
    pub req: mpsc::Sender<()>,
    pub reply: mpsc::Receiver<Vec<u8>>,
}

impl server::saving::IconCapture for ServerScreenshot {
    fn capture(&self) -> Option<Vec<u8>> {
        self.req.send(()).ok()?;
        self.reply.recv().ok()
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum NetworkState {
    Loading,
    Playing,
    Closed,
}

impl GameInstance {
    /// Creates a game instance with a single player server
    pub fn single_player(
        log: &Logger,
        asset_manager: &AssetManager,
        #[cfg(feature = "steam")] steam: steamworks::Client,
        name: String,
        mission: Option<ResourceKey<'static>>,
    ) -> errors::Result<(GameInstance, thread::JoinHandle<()>)> {
        let (socket_send, socket_recv) = mpsc::channel();
        let assets = asset_manager.clone();

        // Screenshot handling
        let (req_send, req_recv) = mpsc::channel();
        let (reply_send, reply_recv) = mpsc::channel();
        let screenshot_server = ServerScreenshot {
            req: req_send,
            reply: reply_recv,
        };

        let server_log = log.new(o!("server" => true, "local" => true));
        #[cfg(feature = "steam")]
        let steamworks = steam.clone();
        let server_thread = thread::spawn(move || {
            let fs = crate::make_filesystem(
                #[cfg(feature = "steam")]
                &steam,
            );
            let fs = fs.into_boxed();
            #[cfg(not(feature = "steam"))]
            let steam = ();
            let (mut server, shutdown) = server::Server::<LoopbackSocketListener, _>::new(
                server_log,
                assets,
                steam,
                fs,
                (),
                server::ServerConfig {
                    save_type: if mission.is_some() {
                        server::saving::SaveType::Mission
                    } else {
                        server::saving::SaveType::FreePlay
                    },
                    save_name: name,
                    min_players: 1,
                    max_players: 1,
                    autostart: true,
                    player_area_size: 100,
                    locked_players: false,
                    mission,
                    tick_rate: std::cell::Cell::new(20),
                },
                Some(Box::new(screenshot_server)),
                None,
            )
            .expect("Failed to start local server");
            let socket = server.client_localsocket();
            assume!(server.log, socket_send.send((socket, shutdown)));
            server.run();
        });
        let (socket, shutdown) = assume!(log, socket_recv.recv());
        let (mut sender, mut receiver) = socket.split(log);

        #[cfg(feature = "steam")]
        sender.ensure_send(packet::LocalConnectionStart {
            name: steamworks.friends().name(),
            steam_id: steamworks.user().steam_id().raw(),
        })?;
        #[cfg(not(feature = "steam"))]
        sender.ensure_send(packet::LocalConnectionStart {
            name: "Player".into(),
        })?;

        let pck = receiver.recv_timeout(time::Duration::from_secs(30))?;
        let pck = match pck {
            packet::Packet::GameBegin(pck) => pck,
            pck => bail!("wrong packet: {:?}", pck),
        };

        let mut instance = Self::multi_player(
            log,
            asset_manager,
            #[cfg(feature = "steam")]
            steamworks,
            pck,
            sender,
            receiver,
        )?;
        instance.is_local = true;
        instance.shutdown_waiter = Some(shutdown);
        instance.screenshot_helper = Some(ScreenshotHelper {
            req: req_recv,
            reply: reply_send,
        });
        Ok((instance, server_thread))
    }

    /// Creates a game instance that connects to a remote server.
    pub fn multi_player(
        log: &Logger,
        asset_manager: &AssetManager,
        #[cfg(feature = "steam")] steam: steamworks::Client,
        pck: packet::GameBegin,
        sender: Sender,
        receiver: Receiver,
    ) -> errors::Result<GameInstance> {
        use crate::server::lua::Scope;
        let mut instance = Self::create_instance(
            log,
            asset_manager,
            pck.mission_handler,
            #[cfg(feature = "steam")]
            steam,
            sender,
            receiver,
            pck.width,
            pck.height,
        );
        instance.player.id = player::Id(pck.uid);
        instance.scripting.set(
            Scope::Global,
            "control_player",
            i32::from(instance.player.id.0),
        );

        for player in pck.players.0 {
            if player.uid == instance.player.id {
                instance.player.state = player.state.clone();
            }
            let mut rplayer = RemotePlayer::new(player.uid, player.username);
            rplayer.state = player.state;
            instance.players.insert(player.uid, rplayer);
        }

        instance
            .level
            .load_initial_state::<entity::ClientEntityCreator, _>(
                &instance.scripting,
                &mut instance.entities,
                pck.strings.0,
                pck.state,
            )?;
        for packet::IdleState { player, idx, state } in pck.idle_state.0 {
            scripting::load_choice_state(
                log,
                &mut instance.entities,
                &instance.scripting,
                &mut instance.running_choices,
                player,
                idx as usize,
                state.0,
            );
        }
        instance.ensure_send(packet::LevelLoaded {})?;
        instance.local_network_state = NetworkState::Playing;

        if let Some(handler) = instance.mission_handler.as_ref() {
            if let Err(err) = instance
                .scripting
                .with_borrows()
                .borrow(&crate::server::mission::MissionAllowed)
                .borrow_mut(&mut instance.level)
                .borrow_mut(&mut instance.entities)
                .invoke_function::<_, ()>(
                    "invoke_module_method",
                    (
                        lua::Ref::new_string(&instance.scripting, handler.module()),
                        lua::Ref::new_string(&instance.scripting, handler.resource()),
                        lua::Ref::new_string(&instance.scripting, "client_init"),
                    ),
                )
            {
                warn!(instance.log, "Failed to init mission: {}", err);
            }
        }
        Ok(instance)
    }

    fn create_instance(
        log: &Logger,
        asset_manager: &AssetManager,
        mission_handler: Option<ResourceKey<'static>>,
        #[cfg(feature = "steam")] steam: steamworks::Client,
        sender: Sender,
        receiver: Receiver,
        width: u32,
        height: u32,
    ) -> GameInstance {
        let mut entities = ecs::Container::new();
        server::entity::register_components(&mut entities);
        entity::register_components(&mut entities);

        entities.add_component(Container::WORLD, CLogger { log: log.clone() });
        entities.add_component(
            Container::WORLD,
            course::LessonManager::new(log.clone(), asset_manager),
        );

        let log = log.new(o!("client" => true));
        let scripting = script::Engine::new(&log, asset_manager.clone());

        let mut systems = ecs::Systems::new();
        server::entity::register_systems(&mut systems);
        entity::register_systems(&mut systems);

        let mut frame_systems = ecs::Systems::new();
        entity::register_frame_systems(&mut frame_systems);

        let snapshots = snapshot::Snapshots::new(&log, &[player::Id(0)]);
        scripting.store_tracked::<snapshot::EntityMap>(snapshot::EntityMap(
            snapshots.entity_map.clone(),
        ));
        GameInstance {
            level: assume!(
                log,
                Level::new_raw(
                    log.new(o!("type" => "level")),
                    asset_manager,
                    &scripting,
                    width,
                    height
                )
            ),
            log: log.clone(),
            asset_manager: asset_manager.clone(),
            #[cfg(feature = "steam")]
            steam,
            sender,
            receiver,
            local_network_state: NetworkState::Loading,
            remote_network_state: NetworkState::Loading,
            disconnect_reason: None,
            shutdown_waiter: None,
            #[cfg(feature = "steam")]
            auth_ticket: None,

            is_local: false,
            paused: false,

            player: PlayerInfo::new(),
            players: Default::default(),
            last_command: 0,

            last_tick: 1.0,

            next_keep_alive: 0,
            last_keep_alive_reply: time::Instant::now(),
            next_command_id: 1,
            commands: Vec::with_capacity(MAX_QUEUE_HISTORY),
            request_manager: network::RequestManager::new(),

            scripting,
            entities,
            systems,
            frame_systems,
            snapshots,
            entity_state: snapshot::EntitySnapshotState::new(),
            player_state: server::entity::snapshot::INVALID_FRAME,
            running_choices: scripting::RunningChoices::new(&log, asset_manager),
            day_tick: DayTick::default(),

            pathfinder: Pathfinder::new(time::Duration::from_millis(3)),

            last_cursor_position: entity::CursorPosition { x: 0.0, y: 0.0 },

            notifications: Vec::new(),
            delayed_notifications: Vec::new(),
            notification_next_id: 0,
            chat_messages: vec![],

            screenshot_helper: None,

            server_player: ServerPlayer {
                state: player::State::None,
                config: player::PlayerConfig::default(),
            },
            mission_handler,
        }
    }

    /// Disconnects the client from the current server
    pub fn disconnect(&mut self) {
        self.local_network_state = NetworkState::Closed;
    }

    /// Attempts sync the game's state to the server
    pub fn tick(
        &mut self,
        state: &mut crate::GameState,
        manager: &mut state::StateManager,
        delta: f64,
    ) {
        if self.local_network_state == NetworkState::Closed
            || self.remote_network_state == NetworkState::Closed
        {
            // TODO: Should drop back to somewhere other than the main menu

            // Close all open windows/states
            while !manager.is_empty() {
                manager.pop_state();
            }
            // The base state automatically handles the dropping of this
            // instance so we just need to wait for that to happen.
            return;
        }
        if self.remote_network_state != NetworkState::Playing {
            return;
        }

        for p in self.request_manager.packets() {
            let _ = self.sender.ensure_send(p);
        }

        for cmd in &mut self.commands {
            manager.collect_capture(&mut cmd.2);
        }

        // Run the tick at 20 tps
        self.last_tick -= delta;
        let mut num_ticks = 0;
        while self.last_tick <= 0.0 && num_ticks < 3 {
            if let Err(err) = self.tick_minor(state) {
                self.local_network_state = NetworkState::Closed;
                self.remote_network_state = NetworkState::Closed;
                self.disconnect_reason = Some(err);
                return;
            }
            self.last_tick += 3.0;
            num_ticks += 1;
        }
        if self.last_tick < 0.0 {
            warn!(self.log, "Struggling to keep up"; "last_tick" => self.last_tick);
            // Assume a lag spike and prevent the
            // need for catch up. Any desync's
            // caused by this will be fixed later.
            self.last_tick = 0.0;
        }

        // Tick entities (frame systems)

        if !self.paused {
            let d = entity::Delta(delta);
            self.frame_systems
                .run_with_borrows(&mut self.entities)
                .borrow(&self.last_cursor_position)
                .borrow(&*self.level.tiles.borrow())
                .borrow(&*self.level.rooms.borrow())
                .borrow_mut(&mut self.pathfinder)
                .borrow_mut(&mut *state.audio.controller.borrow_mut())
                .borrow(&self.asset_manager)
                .borrow_mut(&mut state.renderer.animated_info.info)
                .borrow(&d)
                .run();
        }
    }

    fn tick_minor(&mut self, state: &mut crate::GameState) -> errors::Result<()> {
        use std::mem;
        if !self.paused {
            // Handle the client side of room scripting
            scripting::tick_scripts(
                &self.log,
                &self.asset_manager,
                &mut self.level,
                &mut self.entities,
                &self.scripting,
                &mut self.running_choices,
            );
            scripting::client_tick(&self.log, &mut self.entities, &self.scripting);

            if let Some(handler) = self.mission_handler.as_ref() {
                if let Err(err) = self
                    .scripting
                    .with_borrows()
                    .borrow(&crate::server::mission::MissionAllowed)
                    .borrow_mut(&mut self.level)
                    .borrow_mut(&mut self.entities)
                    .invoke_function::<_, ()>(
                        "invoke_module_method",
                        (
                            lua::Ref::new_string(&self.scripting, handler.module()),
                            lua::Ref::new_string(&self.scripting, handler.resource()),
                            lua::Ref::new_string(&self.scripting, "client_update"),
                        ),
                    )
                {
                    warn!(self.log, "Failed to tick mission: {}", err);
                }
            }

            for not in mem::replace(&mut self.delayed_notifications, Vec::new()) {
                self.do_notification(state, not);
            }

            // Tick entities
            self.systems
                .run_with_borrows(&mut self.entities)
                .borrow(&self.last_cursor_position)
                .borrow(&*self.level.tiles.borrow())
                .borrow(&*self.level.rooms.borrow())
                .borrow_mut(&mut self.pathfinder)
                .borrow_mut(&mut *state.audio.controller.borrow_mut())
                .borrow(&self.asset_manager)
                .run();
        }

        // Keep the connection open to the server
        // by firing keep alive packets at it every
        // few ticks. No need to make sure it arrives
        // as we send these often.
        self.next_keep_alive -= 1;
        if self.next_keep_alive < 0 {
            self.next_keep_alive = 60;
            self.send(packet::KeepAlive {})?;
        }

        let timeout_time = if self.is_local {
            Duration::from_secs(500)
        } else {
            Duration::from_secs(15)
        };
        if self.last_keep_alive_reply.elapsed() > timeout_time {
            warn!(self.log, "Server timed out");
            self.disconnect_reason = Some("Server timed out".into());
            self.disconnect();
        }
        if self.commands.is_empty() {
            return Ok(());
        }
        let first_id = self.commands[0].0;
        let cmds = self.commands.iter().map(|v| v.1.clone()).collect();
        // Send the server the list of commands it hasn't accepted yet
        self.send(packet::ExecutedCommands {
            start_id: first_id,
            commands: AlwaysVec(cmds),
        })
    }

    /// Handles the mouse moving
    ///
    /// Special due to the event spam it would cause
    pub fn mouse_move_event(&mut self, state: &mut crate::GameState, mouse_pos: (i32, i32)) {
        let pos = state.renderer.mouse_to_level(mouse_pos.0, mouse_pos.1);
        self.last_cursor_position.x = pos.0;
        self.last_cursor_position.y = pos.1;
    }

    /// Handles key actions if able
    pub fn handle_key_action(
        &mut self,
        _action: keybinds::KeyAction,
        _state: &mut crate::GameState,
        _mouse_pos: (i32, i32),
    ) {
    }

    /// Attempts to handle the passed event
    pub fn handle_ui_event(
        &mut self,
        evt: &mut event::EventHandler,
        state: &mut crate::GameState,
        manager: &mut state::StateManager,
    ) {
        evt.handle_event::<InspectEntity, _>(|InspectEntity(e)| {
            manager.add_state(base::EntityInfoState::new(e));
        });
        evt.handle_event::<InspectRoom, _>(|InspectRoom(room_id)| {
            if let Some(room) = self.level.try_room_info(room_id) {
                state.renderer.suggest_camera_position(
                    room.area.min.x as f32 + room.area.width() as f32 / 2.0,
                    room.area.min.y as f32 + room.area.height() as f32 / 2.0,
                    45.0,
                );
            }
        });
        evt.handle_event::<DoPayStaff, _>(|DoPayStaff(e, bonus)| {
            if let Some(id) = self.entities.get_component::<NetworkId>(e).map(|v| v.0) {
                let mut cmd: Command = PayStaff::new(id, bonus).into();
                let mut proxy = GameProxy::proxy(state);
                try_cmd!(
                    self.log,
                    cmd.execute(
                        &mut proxy,
                        &mut self.player,
                        CommandParams {
                            log: &self.log,
                            level: &mut self.level,
                            engine: &self.scripting,
                            entities: &mut self.entities,
                            snapshots: &self.snapshots,
                            mission_handler: self.mission_handler.as_ref().map(|v| v.borrow()),
                        }
                    ),
                    {
                        self.push_command(cmd, manager);
                    }
                );
            }
        });
    }

    fn do_notification(&mut self, state: &mut crate::GameState, not: server::notify::Notification) {
        use crate::server::notify;
        match not {
            notify::Notification::StaffQuit { entity_id } => {
                if let Some(entity) = self.snapshots.get_entity_by_id(entity_id) {
                    if let Some(name) = self
                        .entities
                        .get_component::<Living>(entity)
                        .map(|v| v.name.clone())
                    {
                        let desc = node! {
                            description {
                                @text(format!("{} {} has quit due to being unhappy with their job", name.0, name.1))
                            }
                        };
                        let title = "Staff Quitting";
                        self.display_notifcation(
                            ResourceKey::new("base", "ui/icons/staff_quit"),
                            title,
                            desc,
                            true,
                        );
                    }
                } else {
                    self.delayed_notifications
                        .push(notify::Notification::StaffQuit { entity_id });
                }
            }
            notify::Notification::StaffPay { entity_id, wants } => {
                if let Some(entity) = self.snapshots.get_entity_by_id(entity_id) {
                    if let Some(name) = self
                        .entities
                        .get_component::<Living>(entity)
                        .map(|v| v.name.clone())
                    {
                        let desc = node! {
                            active_notification(style="staff_pay".to_owned()) {
                                content {
                                    @text(format!("{} {} wants a pay raise to {}", name.0, name.1, wants))
                                }
                            }
                        };
                        let buttons = ui::Node::new("buttons");

                        let btn = node! {
                            button {
                                content {
                                    @text("Give Raise")
                                }
                            }
                        };
                        btn.set_property(
                            "on_click",
                            ui::MethodDesc::<ui::MouseUpEvent>::native(move |evt, node, _| {
                                evt.emit(DoPayStaff(entity, false));
                                if let Some(id) = node
                                    .parent()
                                    .and_then(|v| v.parent())
                                    .and_then(|v| v.parent())
                                    .and_then(|v| v.get_property::<i32>("id"))
                                {
                                    evt.emit(base::CloseNotification(id as u32));
                                }
                                evt.emit(base::CloseNotificationWindow);
                                true
                            }),
                        );
                        buttons.add_child(btn);

                        let btn = node! {
                            button {
                                content {
                                    @text("Give Bonus (10%)")
                                }
                            }
                        };
                        btn.set_property(
                            "on_click",
                            ui::MethodDesc::<ui::MouseUpEvent>::native(move |evt, node, _| {
                                evt.emit(DoPayStaff(entity, true));
                                if let Some(id) = node
                                    .parent()
                                    .and_then(|v| v.parent())
                                    .and_then(|v| v.parent())
                                    .and_then(|v| v.get_property::<i32>("id"))
                                {
                                    evt.emit(base::CloseNotification(id as u32));
                                }
                                evt.emit(base::CloseNotificationWindow);
                                true
                            }),
                        );

                        buttons.add_child(btn);
                        let btn = node! {
                            button {
                                content {
                                    @text("View")
                                }
                            }
                        };
                        btn.set_property(
                            "on_click",
                            ui::MethodDesc::<ui::MouseUpEvent>::native(move |evt, _, _| {
                                evt.emit(InspectEntity(entity));
                                evt.emit(base::CloseNotificationWindow);
                                true
                            }),
                        );

                        buttons.add_child(btn);
                        let btn = node! {
                            button {
                                content {
                                    @text("Ignore")
                                }
                            }
                        };
                        btn.set_property(
                            "on_click",
                            ui::MethodDesc::<ui::MouseUpEvent>::native(move |evt, node, _| {
                                if let Some(id) = node
                                    .parent()
                                    .and_then(|v| v.parent())
                                    .and_then(|v| v.parent())
                                    .and_then(|v| v.get_property::<i32>("id"))
                                {
                                    evt.emit(base::CloseNotification(id as u32));
                                }
                                evt.emit(base::CloseNotificationWindow);
                                true
                            }),
                        );
                        buttons.add_child(btn);

                        desc.add_child(buttons);
                        let title = "Staff Pay Raise";
                        self.display_notifcation_reason(
                            ResourceKey::new("base", "ui/icons/staff_raise"),
                            title,
                            desc,
                            false,
                            base::KeepReason::EntityOwned(entity),
                        );
                    }
                } else {
                    self.delayed_notifications
                        .push(notify::Notification::StaffPay { entity_id, wants });
                }
            }
            notify::Notification::Text {
                icon,
                title,
                description,
            } => {
                let desc = node! {
                    description {
                        @text(description)
                    }
                };
                self.display_notifcation(icon, title, desc, true);
            }
            notify::Notification::RoomMissingDismiss(room_id) => {
                for v in &self.notifications {
                    if let base::KeepReason::RoomActive(rid) = v.keep_reason {
                        if rid == room_id {
                            state
                                .ui_manager
                                .events
                                .borrow_mut()
                                .emit(base::ClickNotification(v.id));
                        }
                    }
                }
            }
            notify::Notification::RoomMissing {
                room_id,
                icon,
                title,
                description,
            } => {
                let desc = node! {
                    active_notification(style="room_text".to_owned()) {
                        content {
                            @text(description)
                        }
                    }
                };
                let buttons = ui::Node::new("buttons");
                let btn = node! {
                    button {
                        content {
                            @text("View Room")
                        }
                    }
                };
                btn.set_property(
                    "on_click",
                    ui::MethodDesc::<ui::MouseUpEvent>::native(move |evt, _node, _| {
                        evt.emit(InspectRoom(room_id));
                        evt.emit(base::CloseNotificationWindow);
                        true
                    }),
                );
                buttons.add_child(btn);

                let btn = node! {
                    button {
                        content {
                            @text("Ignore")
                        }
                    }
                };
                btn.set_property(
                    "on_click",
                    ui::MethodDesc::<ui::MouseUpEvent>::native(move |evt, node, _| {
                        if let Some(id) = node
                            .parent() // buttons
                            .and_then(|v| v.parent()) // active_notification
                            .and_then(|v| v.parent()) // content
                            .and_then(|v| v.get_property::<i32>("id"))
                        {
                            evt.emit(base::CloseNotification(id as u32));
                        }
                        evt.emit(base::CloseNotificationWindow);
                        true
                    }),
                );
                buttons.add_child(btn);

                desc.add_child(buttons);
                self.display_notifcation_reason(
                    icon,
                    title,
                    desc,
                    false,
                    base::KeepReason::RoomActive(room_id),
                );
            }
            notify::Notification::Script { script, func, data } => {
                let (icon, title, description) = match self
                    .scripting
                    .with_borrows()
                    .borrow_mut(&mut self.level)
                    .borrow_mut(&mut self.entities)
                    .invoke_function::<_, Ref<Table>>(
                        "invoke_module_method",
                        (
                            Ref::new_string(&self.scripting, script.module()),
                            Ref::new_string(&self.scripting, script.resource()),
                            Ref::new_string(&self.scripting, func),
                            Ref::new(&self.scripting, data.0),
                        ),
                    ) {
                    Ok(val) => {
                        let icon =
                            val.get::<_, Ref<String>>(Ref::new_string(&self.scripting, "icon"));
                        let icon = LazyResourceKey::parse(
                            icon.as_ref()
                                .map(|v| v.as_ref())
                                .unwrap_or("base:ui/icons/inspection"),
                        )
                        .or_module(script.module_key())
                        .into_owned();
                        let title: Option<Ref<String>> =
                            val.get(Ref::new_string(&self.scripting, "title"));
                        let description: Option<Ref<ui::NodeRef>> =
                            val.get(Ref::new_string(&self.scripting, "description"));
                        (icon, title, description)
                    }
                    Err(err) => {
                        error!(self.log, "Failed to decode notification"; "script" => ? script, "error" => % err);
                        return;
                    }
                };
                self.display_notifcation(
                    icon,
                    title
                        .as_ref()
                        .map(|v| v.as_ref())
                        .unwrap_or("Missing title"),
                    description
                        .map(|v| v.0.clone())
                        .unwrap_or_else(|| node!(description)),
                    true,
                );
            }
        }
    }

    /// Handles incoming packets
    pub fn handle_packets(
        &mut self,
        state: &mut crate::GameState,
        manager: &mut state::StateManager,
    ) -> errors::Result<()> {
        use self::NetworkState::*;
        use crate::server::network::packet::Packet::*;
        while let Ok(pck) = self.receiver.try_recv() {
            match (self.remote_network_state, pck) {
                (_, UpdateStats(pck)) => {
                    if pck.update_id <= self.player.update_id {
                        continue;
                    }
                    self.player.update_id = pck.update_id;
                    self.player.history = pck.history.0;
                }
                (_, Message(pck)) => {
                    self.chat_messages.extend(pck.messages.0);
                }
                (Playing, Notification(pck)) => {
                    for not in pck.notifications.0 {
                        self.do_notification(state, not);
                    }
                }
                (Playing, EntityFrame(pck)) => {
                    match self
                        .snapshots
                        .resolve_delta::<entity::ClientComponent, _, _>(
                            &mut self.level,
                            &mut self.entities,
                            &self.asset_manager,
                            &mut self.running_choices,
                            &mut self.day_tick,
                            &mut self.entity_state,
                            pck,
                            &mut self.player,
                            &mut self.player_state,
                        ) {
                        Ok((entity_ack, player_ack)) => {
                            // If we accepted the frame ack it
                            // to the server
                            if let Some(reply) = entity_ack {
                                self.send(reply)?;
                            }
                            // Frames can optionally include player
                            // state. Ack that as well if we
                            // got and accepted it.
                            if let Some(reply) = player_ack {
                                self.send(reply)?;
                            }
                        }
                        Err(err) => {
                            info!(self.log, "Ignoring frame: {:?}", err);
                        }
                    }
                }
                (Playing, RemoteExecutedCommands(pck)) => {
                    if pck.commands.0.is_empty() {
                        continue;
                    }
                    let accepted_id = {
                        let mut data = bitio::Reader::new(::std::io::Cursor::new(pck.commands.0));
                        let len = data.read_unsigned(8)?;
                        for i in 0..len {
                            let id = pck.start_id + i as u32;

                            let packet::CommandPair {
                                player_id,
                                command: mut cmd,
                            } = packet::CommandPair::decode(None, &mut data)?;

                            // Have we already processed this command in a previous
                            // packet?
                            if id <= self.last_command {
                                continue;
                            }
                            // Is this the next command we are expecting?
                            if id != self.last_command.wrapping_add(1) {
                                info!(
                                    self.log,
                                    "Ignoring command. wanted: {}, got: {}",
                                    self.last_command.wrapping_add(1),
                                    id
                                );
                                break;
                            }

                            // We are sent commands that other clients have already executed
                            // so we need to re-run what they did.

                            // Ideally should never fail at this point but might.
                            let result = match player_id {
                                // Server player
                                PlayerId(0) => {
                                    let mut proxy = ServerHandler {
                                        running_choices: &mut self.running_choices,
                                    };
                                    cmd.execute(
                                        &mut proxy,
                                        &mut self.server_player,
                                        CommandParams {
                                            log: &self.log,
                                            level: &mut self.level,
                                            engine: &self.scripting,
                                            entities: &mut self.entities,
                                            snapshots: &self.snapshots,
                                            mission_handler: self
                                                .mission_handler
                                                .as_ref()
                                                .map(|v| v.borrow()),
                                        },
                                    )
                                }
                                // Normal remote player
                                player_id => {
                                    let mut proxy = RemoteGameProxy::proxy(state);
                                    cmd.execute(
                                        &mut proxy,
                                        if let Some(player) = self.players.get_mut(&player_id) {
                                            player
                                        } else {
                                            bail!("No player for {:?}", player_id);
                                        },
                                        CommandParams {
                                            log: &self.log,
                                            level: &mut self.level,
                                            engine: &self.scripting,
                                            entities: &mut self.entities,
                                            snapshots: &self.snapshots,
                                            mission_handler: self
                                                .mission_handler
                                                .as_ref()
                                                .map(|v| v.borrow()),
                                        },
                                    )
                                }
                            };
                            if let Err(err) = result {
                                // If the command failed that should mean we are out of sync
                                // with the server and an action we have performed caused
                                // this one to fail.
                                // In this case the server should roll back the command that
                                // caused the issue for this command and resend the command.
                                error!(self.log, "Failed to execute server command: {:?}", err);
                                break;
                            }
                            // Mark this command as executed
                            self.last_command = id;
                        }
                        self.last_command
                    };
                    // Let the server know what commands we have executed so
                    // that it wont continue to send us them.
                    self.send(packet::AckRemoteCommands { accepted_id })?;
                }
                (Playing, RejectCommands(pck)) => {
                    error!(self.log, "Out of sync with the server, rolling back");
                    // Remove all the accepted commands from the queue
                    if let Some(pos) = self.commands.iter().position(|v| v.0 == pck.accepted_id) {
                        for cmd in self.commands.drain(..=pos) {
                            manager.drop_capture(cmd.2);
                        }
                    }
                    // Roll back everything else
                    let mut proxy = GameProxy::proxy(state);
                    let mut cap = None;
                    for mut cmd in self.commands.drain(..).rev() {
                        cmd.1.undo(
                            &mut proxy,
                            &mut self.player,
                            CommandParams {
                                log: &self.log,
                                level: &mut self.level,
                                engine: &self.scripting,
                                entities: &mut self.entities,
                                snapshots: &self.snapshots,
                                mission_handler: self.mission_handler.as_ref().map(|v| v.borrow()),
                            },
                        );
                        cap = Some(cmd.2);
                    }
                    if let Some(cap) = cap {
                        if let state::PossibleCapture::Captured(cap) = cap {
                            manager.restore(cap);
                        }
                    }
                    // Use the rejected command's id for the sorry command and then
                    // continue from there
                    self.next_command_id = pck.rejected_id;
                    self.push_command(Command::Sorry(Sorry {}), manager);
                }
                (Playing, AckCommands(pck)) => {
                    // Remove all the accepted commands from the queue
                    if let Some(pos) = self.commands.iter().position(|v| v.0 == pck.accepted_id) {
                        for cmd in self.commands.drain(..=pos) {
                            manager.drop_capture(cmd.2);
                        }
                    }
                }
                (Loading, GameStart(..)) => {
                    self.remote_network_state = Playing;
                }
                (_, Request(req)) => {
                    self.request_manager.parse_request(req);
                }
                (_, Reply(rpl)) => {
                    state.ui_manager.events().emit(network::ReplyEvent(rpl));
                }
                (_, KeepAlive(..)) => {
                    self.last_keep_alive_reply = time::Instant::now();
                }
                (state, pck) => error!(self.log, "Unhandled packet: {:?} -> {:?}", state, pck),
            }
        }
        Ok(())
    }

    /// Pushes a command to the command queue and allocates an id for it
    pub fn push_command<C>(&mut self, c: Command, cap: &mut C)
    where
        C: state::Capturable,
    {
        let id = self.next_command_id;
        self.next_command_id = self.next_command_id.wrapping_add(1);
        self.commands.push((id, c, cap.request_capture()));
    }

    /// Attempts to send a packet to the target.
    /// Order of the frames when recieved by the target and
    /// whether the data arrives at all isn't guaranteed.
    pub fn send<P: Into<packet::Packet>>(&mut self, data: P) -> errors::Result<()> {
        self.sender.send(data).map_err(|e| e.into())
    }

    /// Sends a single packet to the target. Order of the
    /// when recieved isn't guaranteed but the frame will arrive
    /// assuming no long-term issues.
    ///
    /// If the frame is failed to be sent within a implementation
    /// defined window then the socket should be closed and an
    /// error returned for all future `send*` and `recv` calls.
    pub fn ensure_send<P: Into<packet::Packet>>(&mut self, data: P) -> errors::Result<()> {
        self.sender.ensure_send(data).map_err(|e| e.into())
    }

    /// Displays a notification to the player on the screen
    pub fn display_notifcation<T>(
        &mut self,
        icon: ResourceKey<'_>,
        title: T,
        description: ui::Node,
        closable: bool,
    ) where
        T: Into<String>,
    {
        self.display_notifcation_reason(icon, title, description, closable, base::KeepReason::None)
    }

    /// Displays a notification to the player on the screen with
    /// a dismiss reason
    pub fn display_notifcation_reason<T>(
        &mut self,
        icon: ResourceKey<'_>,
        title: T,
        description: ui::Node,
        closable: bool,
        reason: base::KeepReason,
    ) where
        T: Into<String>,
    {
        let id = self.notification_next_id;
        self.notification_next_id = self.notification_next_id.wrapping_add(1);
        self.notifications.push(base::Notification {
            id,
            icon: icon.into_owned(),
            title: title.into(),
            description: description,
            ui: None,
            index: 0.0,
            target_index: 0.0,
            initial_done: false,
            closable,
            keep_reason: reason,
            fly_in_done: false,
        })
    }
}

impl Drop for GameInstance {
    fn drop(&mut self) {
        // Attempt to disconnet
        let _ = self.send(packet::Disconnect {});
        #[cfg(feature = "steam")]
        {
            if let Some(ticket) = self.auth_ticket.take() {
                self.steam.user().cancel_authentication_ticket(ticket);
            }
        }
    }
}

/// Player specific information
pub struct PlayerInfo {
    id: player::Id,
    state: State,
    money: UniDollar,
    rating: i16,
    first_set: bool,
    waiting_first: bool,

    update_id: u32,
    history: Vec<packet::HistoryEntry>,
    config: player::PlayerConfig,
}

impl PlayerInfo {
    fn new() -> PlayerInfo {
        PlayerInfo {
            id: player::Id(0x7FFF),
            state: State::None,
            money: UniDollar(0),
            rating: 0,
            update_id: 0,
            history: vec![packet::HistoryEntry::default(); 14],
            first_set: false,
            waiting_first: true,
            config: player::PlayerConfig::default(),
        }
    }
}

impl Player for PlayerInfo {
    type EntityCreator = entity::ClientEntityCreator;
    type EntityInfo = entity::ClientComponent;

    fn get_uid(&self) -> player::Id {
        self.id
    }

    fn set_state(&mut self, state: State) {
        self.state = state;
    }

    fn get_state(&self) -> State {
        self.state.clone()
    }

    fn get_money(&self) -> UniDollar {
        self.money
    }

    fn change_money(&mut self, val: UniDollar) {
        self.money += val;
    }

    fn get_rating(&self) -> i16 {
        self.rating
    }

    fn set_rating(&mut self, val: i16) {
        if self.waiting_first {
            self.waiting_first = false;
            self.first_set = true;
        }
        self.rating = val;
    }

    fn can_charge(&self) -> bool {
        true
    }

    fn get_config(&self) -> player::PlayerConfig {
        self.config.clone()
    }

    fn set_config(&mut self, cfg: player::PlayerConfig) {
        self.config = cfg;
    }
}

struct GameProxy<'a> {
    state: &'a mut crate::GameState,
}

impl<'a> GameProxy<'a> {
    fn proxy(state: &'a mut crate::GameState) -> GameProxy<'a> {
        GameProxy { state }
    }
}

impl<'a> CommandHandler for GameProxy<'a> {
    type Player = PlayerInfo;

    fn execute_edit_room<E>(
        &mut self,
        cmd: &mut EditRoom,
        _player: &mut PlayerInfo,
        params: &mut CommandParams<'_, E>,
    ) -> server::errors::Result<()>
    where
        E: server::script::Invokable,
    {
        let room = params.level.get_room_info(cmd.room_id);
        if !room.controller.is_invalid()
            && params
                .entities
                .get_component::<entity::ClientBooked>(room.controller)
                .is_some()
        {
            Err(server::errors::ErrorKind::RoomNoFullOwnership.into())
        } else {
            Ok(())
        }
    }
}

struct RemotePlayer {
    id: player::Id,
    _name: String,
    state: State,
}

impl RemotePlayer {
    fn new(id: player::Id, name: String) -> RemotePlayer {
        RemotePlayer {
            id,
            _name: name,
            state: State::None,
        }
    }
}

struct RemoteGameProxy<'a> {
    _state: &'a mut crate::GameState,
}

impl<'a> RemoteGameProxy<'a> {
    fn proxy(state: &'a mut crate::GameState) -> RemoteGameProxy<'a> {
        RemoteGameProxy { _state: state }
    }
}

impl<'a> CommandHandler for RemoteGameProxy<'a> {
    type Player = RemotePlayer;

    fn execute_place_staff<E>(
        &mut self,
        _cmd: &mut PlaceStaff,
        player: &mut RemotePlayer,
        params: &mut CommandParams<'_, E>,
    ) -> server::errors::Result<()>
    where
        E: server::script::Invokable,
    {
        if let State::EditEntity { entity: Some(e) } = player.state {
            params.entities.remove_entity(e);
            player.set_state(State::None);
            Ok(())
        } else {
            bail!("incorrect state")
        }
    }
}

impl Player for RemotePlayer {
    type EntityCreator = entity::ClientEntityCreator;
    type EntityInfo = entity::ClientComponent;

    fn get_uid(&self) -> player::Id {
        self.id
    }

    fn set_state(&mut self, state: State) {
        self.state = state;
    }

    fn get_state(&self) -> State {
        self.state.clone()
    }

    fn get_money(&self) -> UniDollar {
        UniDollar(0)
    }

    fn change_money(&mut self, _val: UniDollar) {}

    fn get_rating(&self) -> i16 {
        0
    }

    fn set_rating(&mut self, _val: i16) {}

    fn can_charge(&self) -> bool {
        false
    }

    fn get_config(&self) -> player::PlayerConfig {
        player::PlayerConfig::default()
    }

    fn set_config(&mut self, _cfg: player::PlayerConfig) {}
}

/// Fake player used by the server
pub(crate) struct ServerPlayer {
    state: player::State,
    config: player::PlayerConfig,
}

impl player::Player for ServerPlayer {
    type EntityCreator = entity::ClientEntityCreator;
    type EntityInfo = entity::ClientComponent;

    fn get_uid(&self) -> PlayerId {
        PlayerId(0)
    }
    fn set_state(&mut self, state: player::State) {
        self.state = state;
    }
    fn get_state(&self) -> player::State {
        self.state.clone()
    }
    fn can_charge(&self) -> bool {
        false
    }
    fn get_money(&self) -> UniDollar {
        UniDollar(99_999_999)
    }
    fn change_money(&mut self, _val: UniDollar) {}

    fn get_rating(&self) -> i16 {
        0x1FFF
    }
    fn set_rating(&mut self, _val: i16) {}
    fn get_config(&self) -> player::PlayerConfig {
        self.config.clone()
    }
    fn set_config(&mut self, cfg: player::PlayerConfig) {
        self.config = cfg;
    }
}

struct ServerHandler<'a> {
    running_choices: &'a mut scripting::RunningChoices,
}
impl<'a> CommandHandler for ServerHandler<'a> {
    type Player = ServerPlayer;

    fn execute_place_staff<E>(
        &mut self,
        _cmd: &mut PlaceStaff,
        player: &mut ServerPlayer,
        params: &mut CommandParams<'_, E>,
    ) -> server::errors::Result<()>
    where
        E: server::script::Invokable,
    {
        if let State::EditEntity { entity: Some(e) } = player.state {
            params.entities.remove_entity(e);
            player.set_state(State::None);
            Ok(())
        } else {
            bail!("incorrect state")
        }
    }

    fn execute_exec_idle<E>(
        &mut self,
        cmd: &mut ExecIdle,
        _player: &mut ServerPlayer,
        params: &mut CommandParams<'_, E>,
    ) -> server::errors::Result<()>
    where
        E: Invokable,
    {
        use std::sync::Arc;
        let script = assume!(
            params.log,
            self.running_choices
                .student_idle_scripts
                .get(cmd.idx as usize)
        );
        let rc = self
            .running_choices
            .student_idle
            .entry((cmd.player, cmd.idx as usize))
            .or_insert_with(|| scripting::RunningChoice {
                entities: Vec::new(),
                handle: None,
            });
        let handle = rc.handle.get_or_insert_with(|| {
            Ref::new(
                params.engine,
                scripting::IdleScriptHandle {
                    player: cmd.player,
                    props: Ref::new_table(params.engine),
                },
            )
        });
        if let Err(err) = params
            .engine
            .with_borrows()
            .borrow_mut(params.entities)
            .invoke_function::<_, ()>(
                "invoke_module_method",
                (
                    Ref::new_string(params.engine, script.script.module()),
                    Ref::new_string(params.engine, script.script.resource()),
                    Ref::new_string(params.engine, "on_exec"),
                    handle.clone(),
                    Ref::new_string(params.engine, cmd.method.as_str()),
                    Ref::new(params.engine, Arc::clone(&cmd.data.0)),
                ),
            )
        {
            error!(params.log, "Failed to on_exec idle script"; "error" => %err);
        }
        Ok(())
    }
}

// UI events

pub(crate) struct OpenBuyRoomMenu;
pub(crate) struct OpenBuyStaffMenu;
pub(crate) struct OpenStaffListMenu;
pub(crate) struct OpenStatsMenu;
pub(crate) struct OpenSettingsMenu;
pub(crate) struct OpenCoursesMenu;
pub(crate) struct OpenEditRoom;

pub(crate) struct CancelEvent(pub(crate) ui::Node);
pub(crate) struct AcceptEvent(pub(crate) ui::Node);
struct CloseWindowOthers(pub(crate) ui::Node);
struct InspectEntity(Entity);
struct DoPayStaff(Entity, bool);
struct InspectRoom(RoomId);
