use std::collections::VecDeque;
use std::fmt::{self, Display};
use std::mem;
use std::sync::Arc;

use crate::command;
use crate::common;
use crate::entity::snapshot::{EntitySnapshotState, INVALID_FRAME};
use crate::network;
use crate::notify::Notification;
use crate::prelude::*;
use crate::saving::filesystem;
use crate::script;
use crate::steam;
use crate::ServerState;
use delta_encode::AlwaysVec;
#[cfg(feature = "steam")]
use steamworks;

pub(crate) struct NetworkedPlayer<S: Socket> {
    log: Logger,
    pub id: S::Id,
    pub uid: Option<PlayerId>,

    // Remote state is the state we
    // know the client is in whilst
    // local state is the state we
    // want the client to be in.
    //
    // These may differ if we've sent
    // a packet to switch state to the
    // client but it hasn't replied yet.
    pub remote_state: PlayerState,
    pub local_state: PlayerState,

    pub last_packet: Instant,

    pub last_command: u32,
    // The id of the last failed command, don't accept
    // commands until the client has reverted its mistake
    pub failed_command: Option<u32>,

    pub commands: Vec<Command>,
    pub remote_commands: RemoteCommandList,
    pub messages: Vec<Message>,

    pub entity_state: EntitySnapshotState,
    pub player_state: u16,

    request_manager: network::RequestManager,

    /// Used in single player if the player is forcing a save
    pub wants_save: bool,
}

pub(crate) struct RemoteCommandList {
    pub next_id: u32,
    pub commands: Vec<(u32, player::Id, command::Command)>,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum PlayerState {
    Connecting,
    Lobby,
    Loading,
    Playing,
    Closed,
}

impl<S: Socket> Display for NetworkedPlayer<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(f, "Player(addr: {:?})", self.id)
    }
}

impl<S: Socket> NetworkedPlayer<S> {
    pub fn new(log: &Logger, id: S::Id) -> NetworkedPlayer<S> {
        let log = log.new(o!(
            "network_id" => format!("{:?}", id),
        ));
        NetworkedPlayer {
            log,
            id,
            uid: None,
            remote_state: PlayerState::Connecting,
            local_state: PlayerState::Connecting,
            last_packet: Instant::now(),
            last_command: 0,
            failed_command: None,
            commands: vec![],
            remote_commands: RemoteCommandList {
                next_id: 1,
                commands: vec![],
            },
            messages: Vec::new(),
            entity_state: EntitySnapshotState::new(),
            player_state: INVALID_FRAME,
            wants_save: false,
            request_manager: network::RequestManager::new(),
        }
    }

    pub fn handle_packets<Steam: steam::Steam, F: filesystem::FileSystem>(
        &mut self,
        server_state: &mut ServerState,
        asset_manager: &AssetManager,
        fs: &F,
        config: &crate::ServerConfig,
        connection: &mut Connection<S>,
        next_uid: i16,
        info: &mut FNVMap<PlayerId, PlayerInfo>,
        steam: &Steam,
    ) -> Option<PlayerInfo> {
        match self.handle_packets_err(
            server_state,
            asset_manager,
            fs,
            config,
            connection,
            next_uid,
            info,
            steam,
        ) {
            Ok(val) => val,
            Err(err) => {
                error!(self.log, "Client error: {:?}", err);
                self.local_state = PlayerState::Closed;
                None
            }
        }
    }

    fn handle_packets_err<Steam: steam::Steam, F: filesystem::FileSystem>(
        &mut self,
        server_state: &mut ServerState,
        asset_manager: &AssetManager,
        fs: &F,
        config: &crate::ServerConfig,
        connection: &mut Connection<S>,
        next_uid: i16,
        info: &mut FNVMap<PlayerId, PlayerInfo>,
        steam: &Steam,
    ) -> UResult<Option<PlayerInfo>> {
        use self::PlayerState::*;
        use crate::network::packet::Packet::*;
        use crate::ServerState::Playing as SPlaying;
        use lua::Ref;

        'packets: while let Ok(pck) = connection.recv() {
            self.last_packet = Instant::now();
            match (self.remote_state, pck) {
                (Playing, SaveGame(_)) if S::is_local() => {
                    self.wants_save = true;
                }
                (Playing, ChatMessage(pck)) => {
                    let info = assume!(self.log, info.get_mut(&assume!(self.log, self.uid)));
                    if pck.message.starts_with('/') {
                        // TODO: Limit to single player?
                        match &pck.message[1..] {
                            #[cfg(feature = "debugutil")]
                            "server_crash" => {
                                panic!("Forced server crash");
                            }
                            #[cfg(feature = "debugutil")]
                            cmd if cmd.starts_with("tick ") => {
                                if let Ok(tick) = cmd["tick ".len()..].parse::<u32>() {
                                    config.tick_rate.set(tick);
                                    let msg = crate::msg::Message::new()
                                        .special()
                                        .color(255, 211, 196)
                                        .text(format!("Changing tick rate to {}", tick))
                                        .build();
                                    connection.ensure_send(packet::Message {
                                        messages: AlwaysVec(vec![msg]),
                                    })?;
                                }
                            }
                            cmd if cmd.starts_with("moneypls ") => {
                                if let Ok(money) = cmd["moneypls ".len()..].parse::<i64>() {
                                    info.change_money(UniDollar(money));
                                    let msg = crate::msg::Message::new()
                                        .special()
                                        .color(255, 211, 196)
                                        .text("Cheated in money")
                                        .build();
                                    connection.ensure_send(packet::Message {
                                        messages: AlwaysVec(vec![msg]),
                                    })?;
                                }
                            }
                            cmd if cmd.starts_with("studentspls ") => {
                                if let Ok(count) = cmd["studentspls ".len()..].parse::<u32>() {
                                    if let SPlaying {
                                        ref mut spawning, ..
                                    } = *server_state
                                    {
                                        if let Some(sp) =
                                            spawning.info.iter_mut().find(|v| v.id == info.uid)
                                        {
                                            sp.required_students = count;
                                            let msg = crate::msg::Message::new()
                                                .special()
                                                .color(255, 211, 196)
                                                .text(format!(
                                                    "Changing `required_students` to {}",
                                                    count
                                                ))
                                                .build();
                                            connection.ensure_send(packet::Message {
                                                messages: AlwaysVec(vec![msg]),
                                            })?;
                                        }
                                    }
                                }
                            }
                            "notifytest" => {
                                info.notifications.push(crate::notify::Notification::Text {
                                    icon: ResourceKey::new("base", "solid"),
                                    title: "This is a test".into(),
                                    description: "This is a test notification. Please ignore"
                                        .into(),
                                });
                            }
                            "notifytest2" => {
                                info.notifications
                                    .push(crate::notify::Notification::RoomMissing {
                                        room_id: RoomId(0),
                                        icon: ResourceKey::new("base", "solid"),
                                        title: "This is a test".into(),
                                        description: "This is a test notification. Please ignore"
                                            .into(),
                                    });
                            }
                            _ => {}
                        }
                    } else {
                        // TODO: Disabling messages is easier than handling CVAA for now
                        // let msg = crate::msg::Message::new()
                        //     .color(255, 255, 255)
                        //     .text("<")
                        //     .color(130, 237, 123)
                        //     .text(info.name.as_ref())
                        //     .color(255, 255, 255)
                        //     .text(">: ")
                        //     .text(pck.message)
                        //     .build();
                        // self.messages.push(msg);
                    }
                }
                (Playing, SetPauseGame(ref pck)) if S::is_local() => {
                    if let SPlaying { ref mut paused, .. } = *server_state {
                        *paused = pck.paused;
                    }
                }
                (Playing, EntityAckFrame(pck)) => {
                    self.entity_state.ack_entities(pck);
                }
                (Playing, PlayerAckFrame(pck)) => {
                    if !snapshot::is_previous_frame(self.player_state, pck.frame) {
                        self.player_state = pck.frame;
                    }
                }
                (Playing, AckRemoteCommands(pck)) => {
                    if let Some(pos) = self
                        .remote_commands
                        .commands
                        .iter()
                        .position(|v| v.0 == pck.accepted_id)
                    {
                        drop(self.remote_commands.commands.drain(..=pos))
                    }
                }
                (Playing, ExecutedCommands(pck)) => {
                    if let SPlaying {
                        ref mut level,
                        ref scripting,
                        ref mut entities,
                        ref snapshots,
                        ref mission,
                        ..
                    } = *server_state
                    {
                        let info = assume!(self.log, info.get_mut(&assume!(self.log, self.uid)));
                        for (i, mut cmd) in pck.commands.0.into_iter().enumerate() {
                            let id = pck.start_id + i as u32;
                            if id <= self.last_command {
                                // Already handled
                                continue;
                            }
                            // If the client fail to validate a command
                            // we drop all commands from them until the client
                            // sends a sorry command letting us know they
                            // are back in sync.
                            if let Some(failed) = self.failed_command {
                                if id != failed {
                                    continue;
                                }
                                if let Command::Sorry(..) = cmd {
                                    // Client has rolled back. Listen to it again
                                    warn!(self.log, "sorry, listening again");
                                    self.failed_command = None;
                                    continue;
                                } else {
                                    // Echo of old packet
                                    continue;
                                }
                            }
                            // The client sends the commands its executed, we
                            // need to execute the same command ourselves to
                            // validate what they did.
                            let mut h = Handler;

                            match cmd.execute(
                                &mut h,
                                info,
                                command::CommandParams {
                                    log: &self.log,
                                    level,
                                    engine: scripting,
                                    entities,
                                    snapshots,
                                    mission_handler: mission.as_ref().map(|v| v.handler.borrow()),
                                },
                            ) {
                                Ok(_) => {
                                    if cmd.should_sync() {
                                        self.commands.push(cmd)
                                    }
                                }
                                Err(err) => {
                                    error!(self.log, "failed to exec command: {:?}", err);
                                    // Command failed to validate, either lag + interaction with another
                                    // player or a cheat attempt. Roll them back and ignore them until they
                                    // do.
                                    self.failed_command = Some(id);
                                    connection.send(packet::RejectCommands {
                                        accepted_id: self.last_command,
                                        rejected_id: id,
                                    })?;
                                    continue 'packets;
                                }
                            };
                            // Mark this command as the last command handled
                            self.last_command = id;
                        }
                        // If we are still waiting for the client to roll back
                        // send the request again as the packet may have been dropped
                        // the client never received it.
                        if let Some(failed) = self.failed_command {
                            connection.send(packet::RejectCommands {
                                accepted_id: self.last_command,
                                rejected_id: failed,
                            })?;
                        } else {
                            // Let the client know its commands were accepted
                            // and it no longer needs to keep track of them
                            connection.send(packet::AckCommands {
                                accepted_id: self.last_command,
                            })?;
                        }
                    }
                }
                (Loading, LevelLoaded(..)) => {
                    self.remote_state = Playing;
                    info!(self.log, "loaded in");
                    connection.ensure_send(packet::GameStart {})?;
                }
                (Lobby, RequestGameBegin(..)) => {
                    *server_state = ServerState::BeginGame;
                }
                (Connecting, EnterLobby(..)) => {
                    self.remote_state = Lobby;
                    if let ServerState::Lobby { change_id, .. } = *server_state {
                        *server_state = ServerState::Lobby {
                            change_id,
                            state_dirty: true,
                        };
                    }
                }
                (Connecting, LocalConnectionStart(ref pck)) if S::is_local() => {
                    if let ServerState::Lobby { .. } = *server_state {
                        self.local_state = Connecting;
                        self.uid = Some(PlayerId(1));
                        *server_state = ServerState::create_play_state(
                            &self.log,
                            asset_manager,
                            fs,
                            info,
                            &config,
                            &[PlayerId(1)],
                        );
                        let self_info = if let Some(info) = info.get_mut(&PlayerId(1)) {
                            // Already loaded from save
                            info.name = pck.name.clone();
                            None
                        } else {
                            let staff_list = load_staff_list(&self.log, asset_manager);
                            #[cfg(feature = "steam")]
                            let key = PlayerKey::Steam(steamworks::SteamId::from_raw(pck.steam_id));
                            #[cfg(not(feature = "steam"))]
                            let key = PlayerKey::Username(pck.name.clone());
                            Some(PlayerInfo::new(
                                key,
                                pck.name.clone(),
                                PlayerId(1),
                                &staff_list,
                            ))
                        };
                        info!(self.log, "Player {:?} joined in", pck.name);
                        if let ServerState::Playing {
                            ref level,
                            ref mission,
                            ref mut entities,
                            ref scripting,
                            ref choices,
                            ref mut running_choices,
                            ..
                        } = *server_state
                        {
                            self.remote_state = Loading;
                            self.local_state = Playing;
                            let (lstr, lstate) = level.create_initial_state();
                            let idle = crate::script_room::create_choices_state(
                                &self.log,
                                entities,
                                scripting,
                                choices,
                                running_choices,
                            );
                            connection.ensure_send(packet::GameBegin {
                                uid: 1,
                                width: level.width,
                                height: level.height,
                                players: AlwaysVec(
                                    info.values()
                                        .map(|p| packet::PlayerEntry {
                                            uid: p.uid,
                                            username: p.name.clone(),
                                            state: p.state.clone(),
                                        })
                                        .collect(),
                                ),
                                mission_handler: mission
                                    .as_ref()
                                    .map(|v| v.handler.borrow().into_owned()),
                                strings: AlwaysVec(lstr),
                                state: lstate,
                                idle_state: idle,
                            })?;
                            return Ok(self_info);
                        } else {
                            unreachable!()
                        }
                    }
                }
                (Connecting, RemoteConnectionStart(pck)) => {
                    #[cfg(feature = "steam")]
                    let steam_id = steamworks::SteamId::from_raw(pck.steam_id);
                    #[cfg(feature = "steam")]
                    let key = {
                        if S::needs_verify() {
                            if let Err(err) =
                                steam.begin_authentication_session(steam_id, &pck.ticket.0)
                            {
                                connection.ensure_send(packet::ServerConnectionFail {
                                    reason: format!("Steam authentication failed: {}", err),
                                })?;
                                bail!("{}", err);
                            }
                        }
                        PlayerKey::Steam(steam_id)
                    };
                    #[cfg(not(feature = "steam"))]
                    let key = PlayerKey::Username(pck.name.clone());

                    let msg = crate::msg::Message::new()
                        .color(130, 237, 123)
                        .text(pck.name.as_str())
                        .color(255, 255, 0)
                        .text(" has joined the server")
                        .build();
                    self.messages.push(msg);
                    match *server_state {
                        ServerState::Lobby { change_id, .. } => {
                            if let Some(info) = info.values().find(|v| v.key == key) {
                                self.uid = Some(info.uid);
                                *server_state = ServerState::Lobby {
                                    change_id,
                                    state_dirty: true,
                                };
                                self.local_state = Lobby;

                                #[cfg(feature = "steam")]
                                info!(self.log, "Player {:?} rejoined", pck.name; "steam_id" => ?steam_id);
                                #[cfg(not(feature = "steam"))]
                                info!(self.log, "Player {:?} rejoined", pck.name);

                                connection.ensure_send(packet::ServerConnectionStart {
                                    uid: info.uid.0,
                                })?;
                                return Ok(None);
                            } else {
                                if config.locked_players {
                                    connection.ensure_send(packet::ServerConnectionFail {
                                        reason: "Server not accepting new players".into(),
                                    })?;
                                    bail!("Server not accepting new players");
                                }
                                self.uid = Some(PlayerId(next_uid));
                                *server_state = ServerState::Lobby {
                                    change_id,
                                    state_dirty: true,
                                };
                                self.local_state = Lobby;

                                #[cfg(feature = "steam")]
                                info!(self.log, "Player {:?} joined in", pck.name; "steam_id" => ?steam_id);
                                #[cfg(not(feature = "steam"))]
                                info!(self.log, "Player {:?} joined in", pck.name);

                                let staff_list = load_staff_list(&self.log, asset_manager);
                                let info = Some(PlayerInfo::new(
                                    key,
                                    pck.name.clone(),
                                    PlayerId(next_uid),
                                    &staff_list,
                                ));
                                connection
                                    .ensure_send(packet::ServerConnectionStart { uid: next_uid })?;
                                return Ok(info);
                            }
                        }
                        ServerState::Playing {
                            ref level,
                            ref mission,
                            ref mut entities,
                            ref scripting,
                            ref choices,
                            ref mut running_choices,
                            ..
                        } => {
                            if let Some(self_info) = info.values().find(|v| v.key == key) {
                                self.uid = Some(self_info.uid);
                                self.remote_state = Loading;
                                self.local_state = Playing;

                                #[cfg(feature = "steam")]
                                info!(self.log, "Player {:?} joined in", pck.name; "steam_id" => ?steam_id);
                                #[cfg(not(feature = "steam"))]
                                info!(self.log, "Player {:?} joined in", pck.name);

                                let players: Vec<_> = info
                                    .values()
                                    .map(|p| packet::PlayerEntry {
                                        uid: p.uid,
                                        username: p.name.clone(),
                                        state: p.state.clone(),
                                    })
                                    .collect();
                                let (lstr, lstate) = level.create_initial_state();
                                let idle = crate::script_room::create_choices_state(
                                    &self.log,
                                    entities,
                                    scripting,
                                    choices,
                                    running_choices,
                                );
                                connection.ensure_send(packet::GameBegin {
                                    uid: self_info.uid.0,
                                    width: level.width,
                                    height: level.height,
                                    players: AlwaysVec(players),
                                    mission_handler: mission
                                        .as_ref()
                                        .map(|v| v.handler.borrow().into_owned()),
                                    strings: AlwaysVec(lstr),
                                    state: lstate,
                                    idle_state: idle,
                                })?;
                                return Ok(None);
                            } else {
                                connection.ensure_send(packet::ServerConnectionFail {
                                    reason: "Session already started".into(),
                                })?;
                                bail!("Session already started");
                            }
                        }
                        _ => {}
                    }
                }
                (Playing, Request(req)) => {
                    self.request_manager.parse_request(req);
                }
                (Playing, Reply(_rpl)) => {
                    // TODO: Server doesn't use events yet
                    // state.ui_manager.events().emit(network::ReplyEvent(rpl));
                }
                (_, KeepAlive(..)) => {
                    connection.send(packet::KeepAlive {})?;
                }
                (_, Disconnect(..)) => {
                    self.local_state = PlayerState::Closed;
                    self.remote_state = PlayerState::Closed;
                }
                (_, pck) => {
                    error!(self.log, "Unhandled packet: {:?}", pck; "remote_state" => ?self.remote_state)
                }
            }
        }

        // If they've connected start sending commands
        if self.remote_state == Playing {
            if let Some(info) = info.get_mut(&assume!(self.log, self.uid)) {
                if !info.notifications.is_empty() {
                    connection.ensure_send(packet::Notification {
                        notifications: AlwaysVec(mem::replace(&mut info.notifications, vec![])),
                    })?;
                }
            }
            // Only send commands if we have something to send
            if !self.remote_commands.commands.is_empty() {
                use delta_encode::DeltaEncodable;
                let mut start_id = self.remote_commands.commands[0].0;
                let base_id = start_id;
                let mut buffer = bitio::Writer::new(Vec::new());
                let mut current = bitio::Writer::new(Vec::new());
                let mut len = 0;

                // Length placeholder
                let _ = buffer.write_unsigned(0, 8);

                for (idx, cmd) in self.remote_commands.commands.iter().enumerate() {
                    let pair = packet::CommandPair {
                        player_id: cmd.1,
                        command: cmd.2.clone(),
                    };
                    let _ = pair.encode(None, &mut current);
                    if buffer.bit_len() + current.bit_len() > 1000 * 8 || len == 255 {
                        let mut data = assume!(
                            self.log,
                            mem::replace(&mut buffer, bitio::Writer::new(vec![])).finish()
                        );
                        data[0] = len;
                        warn!(self.log, "Command buffer full, splitting");
                        connection.send(packet::RemoteExecutedCommands {
                            start_id,
                            commands: packet::Raw(data),
                        })?;
                        start_id = base_id + idx as u32;
                        buffer.clear();
                        len = 0;
                        // Length placeholder
                        let _ = buffer.write_unsigned(0, 8);
                    }
                    len += 1;
                    let _ = current.copy_into(&mut buffer);
                    current.clear();
                }
                let mut data = assume!(
                    self.log,
                    mem::replace(&mut buffer, bitio::Writer::new(vec![])).finish()
                );
                data[0] = len;
                connection.send(packet::RemoteExecutedCommands {
                    start_id,
                    commands: packet::Raw(data),
                })?;
            }
        }

        for req in self.request_manager.requests() {
            let uid = self.uid;
            let log = &self.log;
            req.handle::<super::RoomBooked, _>(|pck, rpl| {
                if let SPlaying {
                    ref mut level,
                    ref mut entities,
                    ..
                } = *server_state
                {
                    let mut is_booked = false;
                    let room = level.get_room_info(pck.room_id);
                    if !room.controller.is_invalid() {
                        if let Some(booked) = entities.get_component::<Booked>(room.controller) {
                            is_booked =
                                booked.timetable.iter().flat_map(|v| v).any(|v| v.is_some());
                        }
                    }
                    rpl.reply(super::RoomBookedReply {
                        room_id: pck.room_id,
                        booked: is_booked,
                    })
                }
            });
            req.handle::<super::RoomDetails, _>(|pck, rpl| {
                if let SPlaying{
                    ref mut level, ref mut entities,
                    ref scripting,
                    ..
                } = *server_state {
                    let ty = if let Some(room) = level.try_room_info(pck.room_id) {
                        if room.owner != assume!(log, uid) || room.controller.is_invalid() {
                            return;
                        }
                        assume!(log, asset_manager.loader_open::<room::Loader>(room.key.borrow()))
                    } else {
                        return;
                    };
                    if let Some(controller) = ty.controller.as_ref() {
                        let lua_room = crate::script_room::LuaRoom::from_room(log, &*level.rooms.borrow(), entities, pck.room_id, scripting);
                        match scripting.with_borrows()
                            .borrow_mut(entities)
                            .borrow_mut(info)
                            .invoke_function::<_, Ref<Arc<bitio::Writer<Vec<u8>>>>>("invoke_module_method", (
                                Ref::new_string(scripting, controller.module()),
                                Ref::new_string(scripting, controller.resource()),
                                Ref::new_string(scripting, "encode_tooltip"),
                                lua_room
                            )) {
                            Ok(val) => {
                                rpl.reply(super::RoomDetailsReply {
                                    room_id: pck.room_id,
                                    data: common::ScriptData(Arc::clone(&val)),
                                });
                            },
                            Err(err) => warn!(log, "Failed to encode room tooltip"; "error" => % err, "room" => %ty.name),
                        }
                    }
                }
            });
            req.handle::<super::EntityResults, _>(|pck, rpl| {
                if let SPlaying {
                    ref mut entities,
                    ref snapshots,
                    ..
                } = *server_state
                {
                    let info = assume!(log, info.get_mut(&assume!(log, uid)));
                    let e = if let Some(e) = snapshots.get_entity_by_id(pck.entity_id) {
                        e
                    } else {
                        rpl.reply(super::EntityResultsReply {
                            entity_id: pck.entity_id,
                            timetable: None,
                            grades: AlwaysVec(Vec::new()),
                            stats: AlwaysVec(Vec::new()),
                            #[cfg(feature = "debugutil")]
                            entity_debug: String::new(),
                        });
                        return;
                    };
                    let uid = assume!(log, uid);
                    if entities
                        .get_component::<Owned>(e)
                        .map_or(true, |v| v.player_id != uid)
                    {
                        rpl.reply(super::EntityResultsReply {
                            entity_id: pck.entity_id,
                            timetable: None,
                            grades: AlwaysVec(Vec::new()),
                            stats: AlwaysVec(Vec::new()),
                            #[cfg(feature = "debugutil")]
                            entity_debug: String::new(),
                        });
                        return;
                    }

                    let timetable = if let (Some(t), Some(g)) = (
                        entities.get_component::<TimeTable>(e),
                        entities.get_component::<Grades>(e),
                    ) {
                        let course = assume!(log, info.courses.get(&t.course));
                        course
                            .timetable
                            .iter()
                            .zip(&g.timetable_grades)
                            .flat_map(|(t, g)| t.iter().zip(g))
                            .map(|(t, g)| match t {
                                course::CourseEntry::Free => super::TimetableEntryState::Free,
                                course::CourseEntry::Lesson { .. } => match g {
                                    Some(v) => super::TimetableEntryState::Completed(*v),
                                    None => super::TimetableEntryState::Lesson,
                                },
                            })
                            .collect()
                    } else {
                        Vec::new()
                    };

                    let grades = if let Some(g) = entities.get_component::<Grades>(e) {
                        g.grades
                            .iter()
                            .filter_map(|v| {
                                Some(super::NamedGradeEntry {
                                    course_name: info.courses.get(&v.course)?.name.clone(),
                                    grade: v.grade,
                                })
                            })
                            .collect()
                    } else {
                        Vec::new()
                    };
                    let variant = {
                        let living = assume!(log, entities.get_component::<Living>(e));
                        let ty = assume!(
                            log,
                            asset_manager
                                .loader_open::<Loader<ServerComponent>>(living.key.borrow())
                        );
                        entity_variant(&ty)
                    };

                    let stats = if let Some(vars) = get_vars(entities, e) {
                        let mut vals = Vec::new();
                        for stat in variant.stats() {
                            vals.push(vars.get_float(stat.as_string()).unwrap_or(0.0));
                        }
                        AlwaysVec(vals)
                    } else {
                        AlwaysVec(Vec::new())
                    };

                    #[cfg(feature = "debugutil")]
                    let entity_debug = {
                        let debug = format!("{:?}", e);
                        debug!(log, "{:#?}", entities.get_component::<NetworkId>(e));
                        debug!(log, "{:#?}", entities.get_component::<RoomOwned>(e));
                        debug!(log, "{:#?}", entities.get_component::<Living>(e));
                        debug!(log, "{:#?}", entities.get_component::<Owned>(e));
                        debug!(log, "{:#?}", entities.get_component::<Idle>(e));
                        debug!(log, "{:#?}", entities.get_component::<Controlled>(e));
                        debug!(log, "{:?}", entities.get_component::<TimeTable>(e));
                        debug!(log, "{:#?}", entities.get_custom::<StudentVars>(e));
                        debug
                    };

                    rpl.reply(super::EntityResultsReply {
                        entity_id: pck.entity_id,
                        timetable: Some(AlwaysVec(timetable)),
                        grades: AlwaysVec(grades),
                        stats,
                        #[cfg(feature = "debugutil")]
                        entity_debug,
                    });
                }
            });
            req.handle::<super::StaffPage, _>(|pck, rpl| {
                let info = assume!(log, info.get_mut(&assume!(log, uid)));
                if let Some(staff) = info.staff_for_hire.get(&pck.staff_key) {
                    if staff.is_empty() {
                        rpl.reply(super::StaffPageReply {
                            num_pages: 0,
                            info: None,
                        });
                        return;
                    }
                    let page = if (pck.page as usize) < staff.len() {
                        pck.page as usize
                    } else {
                        staff.len() - 1
                    };
                    let member = &staff[page];
                    rpl.reply(super::StaffPageReply {
                        num_pages: staff.len() as _,
                        info: Some(super::StaffPageInfo {
                            unique_id: member.unique_id,
                            page: page as u8,
                            variant: member.variant as u16,
                            first_name: member.name.0.clone(),
                            surname: member.name.1.clone(),
                            description: member.description.clone(),
                            stats: member.stats,
                            hire_price: member.hire_price,
                        }),
                    });
                }
            });
            req.handle::<super::CourseList, _>(|_pck, rpl| {
                let info = assume!(log, info.get_mut(&assume!(log, uid)));
                fn timetable_data(i: &[course::CourseEntry; 4]) -> [bool; 4] {
                    [
                        !i[0].is_free(),
                        !i[1].is_free(),
                        !i[2].is_free(),
                        !i[3].is_free(),
                    ]
                }
                rpl.reply(super::CourseListReply {
                    courses: AlwaysVec(
                        info.courses
                            .values()
                            .map(|v| super::CourseOverview {
                                uid: v.uid,
                                name: v.name.clone(),
                                students: 0,             // TODO:
                                average_grade: Grade::A, // TODO:
                                cost: v.cost,
                                problems: "NYI".into(), // TODO:
                                timetable: [
                                    timetable_data(&v.timetable[0]),
                                    timetable_data(&v.timetable[1]),
                                    timetable_data(&v.timetable[2]),
                                    timetable_data(&v.timetable[3]),
                                    timetable_data(&v.timetable[4]),
                                    timetable_data(&v.timetable[5]),
                                    timetable_data(&v.timetable[6]),
                                ],
                                deprecated: v.deprecated,
                            })
                            .collect(),
                    ),
                });
            });
            req.handle::<super::CourseInfo, _>(|pck, rpl| {
                if let ServerState::Playing {
                    ref mut entities, ..
                } = *server_state
                {
                    entities.with(|_em: EntityManager, ids: ecs::Read<NetworkId>| {
                        let info = assume!(log, info.get_mut(&assume!(log, uid)));
                        if let Some(course) =
                            info.courses.get(&pck.uid).and_then(|v| v.as_network(&ids))
                        {
                            rpl.reply(super::CourseInfoReply { course });
                        }
                    });
                }
            });
            req.handle::<super::LessonValidOptions, _>(|pck, rpl| {
                if let ServerState::Playing {
                    ref mut entities,
                    ref level,
                    ..
                } = *server_state
                {
                    entities.with(
                        |em: EntityManager,
                         lm: ecs::Read<course::LessonManager>,
                         owned: ecs::Read<Owned>,
                         living: ecs::Read<Living>,
                         paid: ecs::Read<Paid>,
                         network_id: ecs::Read<NetworkId>,
                         booked: ecs::Read<Booked>| {
                            let course = pck.course;
                            let day = pck.day;
                            let period = pck.period;
                            let player = assume!(log, uid);
                            let lm = assume!(log, lm.get_component(Container::WORLD));
                            if let Some(lesson) = lm.get(pck.key) {
                                let rooms = level
                                    .room_ids()
                                    .into_iter()
                                    .map(|v| level.get_room_info(v))
                                    .filter(|v| v.owner == player)
                                    .filter(|v| lesson.valid_rooms.contains(&v.key))
                                    .filter(|v| v.state.is_done())
                                    .filter(|v| !v.controller.is_invalid())
                                    .map(|v| (v.id, booked.get_component(v.controller)))
                                    .filter(|(_id, b)| {
                                        b.map_or(true, |b| {
                                            b.timetable[day as usize][period as usize]
                                                .map_or(true, |v| v == course)
                                        })
                                    })
                                    .map(|(id, _b)| id)
                                    .collect::<Vec<_>>();

                                let staff = em
                                    .group_mask((&living, &network_id), |m| {
                                        m.and(&owned).and(&paid)
                                    })
                                    .filter(|(_e, (l, _id))| lesson.valid_staff.contains(&l.key))
                                    // Allow unbooked staff and staff already booked for this course
                                    .filter(|(e, (_l, _id))| {
                                        booked.get_component(*e).map_or(true, |b| {
                                            b.timetable[day as usize][period as usize]
                                                .map_or(true, |v| v == course)
                                        })
                                    })
                                    .map(|(_e, (_, id))| *id)
                                    .collect::<Vec<_>>();

                                rpl.reply(super::LessonValidOptionsReply {
                                    staff: AlwaysVec(staff),
                                    rooms: AlwaysVec(rooms),
                                })
                            } else {
                                rpl.reply(super::LessonValidOptionsReply {
                                    staff: AlwaysVec(Vec::new()),
                                    rooms: AlwaysVec(Vec::new()),
                                })
                            }
                        },
                    );
                }
            });
        }

        for p in self.request_manager.packets() {
            connection.ensure_send(p)?;
        }

        Ok(None)
    }
}

pub(crate) struct Handler;
impl CommandHandler for Handler {
    type Player = PlayerInfo;

    fn execute_edit_room<E>(
        &mut self,
        cmd: &mut EditRoom,
        _player: &mut PlayerInfo,
        params: &mut CommandParams<'_, E>,
    ) -> UResult<()>
    where
        E: Invokable,
    {
        let room = params.level.get_room_info(cmd.room_id);
        if !room.controller.is_invalid() {
            if let Some(booked) = params.entities.get_component::<Booked>(room.controller) {
                if booked.timetable.iter().flat_map(|v| v).any(|v| v.is_some()) {
                    bail!("Room is booked for courses")
                }
            }
        }
        Ok(())
    }

    fn execute_fire_staff<E>(
        &mut self,
        cmd: &mut FireStaff,
        _player: &mut PlayerInfo,
        params: &mut CommandParams<'_, E>,
    ) -> UResult<()>
    where
        E: Invokable,
    {
        if let Some(entity) = params.snapshots.get_entity_by_id(cmd.target) {
            params.entities.add_component(entity, Quitting);
        }
        Ok(())
    }

    fn execute_place_staff<E>(
        &mut self,
        cmd: &mut PlaceStaff,
        player: &mut PlayerInfo,
        params: &mut CommandParams<'_, E>,
    ) -> UResult<()>
    where
        E: Invokable,
    {
        if let player::State::EditEntity { entity: None } = player.state {
            let staff = if let Some(v) = player.staff_for_hire.get_mut(&cmd.key) {
                v
            } else {
                bail!("Invalid staff type")
            };
            let member = if let Some(v) = staff.iter().position(|v| v.unique_id == cmd.unique_id) {
                staff.remove(v)
            } else {
                bail!("Stale staff unique id")
            };
            if player.money < member.hire_price && member.hire_price != UniDollar(0) {
                bail!("Not enough money")
            }
            let ty = params
                .level
                .asset_manager
                .loader_open::<Loader<ServerComponent>>(cmd.key.borrow())?;
            let e = ty.create_entity(params.entities, member.variant, Some(member.name));
            {
                let pos = assume!(params.log, params.entities.get_component_mut::<Position>(e));
                pos.x = cmd.location.x;
                pos.y = 0.2;
                pos.z = cmd.location.y;
            }
            {
                if let Some(vars) = get_vars(params.entities, e) {
                    for (stat, val) in entity_variant(&ty).stats().iter().zip(&member.stats) {
                        vars.set_float(stat.as_string(), *val);
                    }
                }
            }
            {
                let paid = assume!(params.log, params.entities.get_component_mut::<Paid>(e));
                paid.cost = member.hire_price;
                paid.wanted_cost = paid.cost;
            }
            params.entities.add_component(e, Frozen);
            params.entities.add_component(
                e,
                Owned {
                    player_id: player.uid,
                },
            );
            params.entities.add_component(e, DoesntExist);
            player.state = player::State::EditEntity { entity: Some(e) };
            cmd.rev = Some(e);
            params
                .entities
                .add_component(e, SelectedEntity { holder: player.uid });
            Ok(())
        } else {
            bail!("incorrect state")
        }
    }

    fn execute_start_move_staff<E>(
        &mut self,
        cmd: &mut StartMoveStaff,
        player: &mut PlayerInfo,
        params: &mut CommandParams<'_, E>,
    ) -> UResult<()>
    where
        E: Invokable,
    {
        if let player::State::EditEntity { entity: None } = player.state {
            let e = assume!(params.log, cmd.rev);
            player.state = player::State::EditEntity { entity: Some(e) };
            params.entities.add_component(e, Frozen);
            params.entities.add_component(
                e,
                Owned {
                    player_id: player.uid,
                },
            );
            params
                .entities
                .add_component(e, SelectedEntity { holder: player.uid });

            let room_id = params
                .entities
                .get_component::<RoomOwned>(e)
                .map(|v| v.room_id);

            if let Some(room_id) = room_id {
                let assets = params.level.asset_manager.clone();
                let ty = {
                    let room = params.level.get_room_info_mut(room_id);
                    if room.controller.is_invalid() {
                        None
                    } else {
                        assets.loader_open::<room::Loader>(room.key.borrow()).ok()
                    }
                };
                if let Some(controller) = ty.as_ref().and_then(|v| v.controller.as_ref()) {
                    let lua_room = crate::script_room::LuaRoom::from_room(
                        params.log,
                        &params.level.rooms.borrow(),
                        params.entities,
                        room_id,
                        &*params.engine,
                    );

                    let lua: &lua::Lua = &*params.engine;
                    let lua_entity = params.entities.with(
                        |_em: EntityManager<'_>,
                         mut entity_ref: ecs::Write<crate::script_room::LuaEntityRef>,
                         living: ecs::Read<Living>,
                         object: ecs::Read<Object>| {
                            crate::script_room::LuaEntityRef::get_or_create(
                                &mut entity_ref,
                                &living,
                                &object,
                                lua,
                                e,
                                Some(Controller::Room(room_id)),
                            )
                        },
                    );

                    lua.with_borrows()
                        .borrow_mut(params.entities)
                        .invoke_function::<_, lua::Ref<lua::Unknown>>(
                            "invoke_module_method",
                            (
                                lua::Ref::new_string(lua, controller.module()),
                                lua::Ref::new_string(lua, controller.resource()),
                                lua::Ref::new_string(lua, "force_entity_release"),
                                lua_room,
                                lua_entity,
                            ),
                        )?;
                }
            }

            // Release the entity from the room that currently controls them
            if let Some(owner) = params.entities.remove_component::<RoomOwned>(e) {
                params.entities.add_component(e, Controlled::new());
                let room = assume!(params.log, params.level.try_room_info(owner.room_id));
                let controller = assume!(
                    params.log,
                    params
                        .entities
                        .get_component_mut::<RoomController>(room.controller)
                );
                controller.entities.retain(|v| *v != e);
                controller.visitors.retain(|v| *v != e);
            }

            Ok(())
        } else {
            bail!("incorrect state")
        }
    }

    fn execute_finalize_staff_place<E>(
        &mut self,
        cmd: &mut FinalizeStaffPlace,
        _player: &mut PlayerInfo,
        params: &mut CommandParams<'_, E>,
    ) -> UResult<()>
    where
        E: Invokable,
    {
        if let Some((e, _, _)) = cmd.rev {
            params.entities.remove_component::<SelectedEntity>(e);
            params.entities.remove_component::<DoesntExist>(e);
            {
                let pos = assume!(params.log, params.entities.get_component_mut::<Position>(e));
                cmd.rev = Some((e, pos.x, pos.z));
                pos.x = cmd.location.x;
                pos.y = 0.0;
                pos.z = cmd.location.y;
            }
            if params
                .entities
                .get_component::<free_roam::FreeRoam>(e)
                .is_none()
            {
                let assets = params.level.asset_manager.clone();
                if let Some(room_id) = params
                    .level
                    .get_room_owner(Location::new(cmd.location.x as i32, cmd.location.y as i32))
                {
                    let room = params.level.get_room_info_mut(room_id);
                    let ty = assets.loader_open::<room::Loader>(room.key.borrow())?;
                    if room.state.is_done() && ty.controller.is_some() {
                        params.entities.add_component(e, RoomOwned::new(room_id));
                        params
                            .entities
                            .add_component(e, Controlled::new_by(Controller::Room(room_id)));
                        let rc = assume!(
                            params.log,
                            params
                                .entities
                                .get_component_mut::<RoomController>(room.controller)
                        );
                        rc.entities.push(e);
                    }
                }
            }
            Ok(())
        } else {
            bail!("incorrect state")
        }
    }
    fn execute_cancel_place_staff<E>(
        &mut self,
        cmd: &mut CancelPlaceStaff,
        _player: &mut PlayerInfo,
        _params: &mut CommandParams<'_, E>,
    ) -> UResult<()>
    where
        E: Invokable,
    {
        if let Some(prev) = cmd.rev.as_ref() {
            if prev.existed {
                bail!("Can't remove entity");
            }
        }
        Ok(())
    }

    fn execute_pay_staff<E>(
        &mut self,
        cmd: &mut PayStaff,
        player: &mut PlayerInfo,
        params: &mut CommandParams<'_, E>,
    ) -> UResult<()>
    where
        E: Invokable,
    {
        if let Some(entity) = params.snapshots.get_entity_by_id(cmd.target) {
            player.staff_issues.remove(&entity);
            if let Some(vars) = params.entities.get_custom::<ProfessorVars>(entity) {
                vars.set_stat(Stats::PROFESSOR_JOB_SATISFACTION, 1.0);
            }
        }
        Ok(())
    }

    fn execute_update_course<E>(
        &mut self,
        cmd: &mut UpdateCourse,
        player: &mut PlayerInfo,
        params: &mut CommandParams<'_, E>,
    ) -> UResult<()>
    where
        E: Invokable,
    {
        if cmd.course.uid == course::CourseId(0) {
            // New course, generate id
            while player
                .courses
                .contains_key(&course::CourseId(player.next_course_id))
            {
                player.next_course_id += 1;
            }
            let id = course::CourseId(player.next_course_id);
            player.next_course_id += 1;
            cmd.course.uid = id;

            let course = course::Course::from_network(params.snapshots, cmd.course.clone())
                .ok_or_else(|| ErrorKind::Msg("Invalid course".to_string()))?;
            course.init_world(&*params.level.rooms.borrow(), params.entities);
            player.courses.insert(id, course);
        } else if let Some(course) = player.courses.get_mut(&cmd.course.uid) {
            course.deinit_world(&*params.level.rooms.borrow(), params.entities);
            // Instead of handling the error straight away we wait until after the
            // course has been re-inited.
            //
            // This is done so that if `update_from_network` fails the original
            // course data is put back in place so that we don't end up with a
            // course in an invalid state
            let ret = course.update_from_network(params.snapshots, cmd.course.clone());
            course.init_world(&*params.level.rooms.borrow(), params.entities);
            return ret;
        } else {
            bail!("Invalid course id")
        }
        Ok(())
    }

    fn execute_deprecate_course<E>(
        &mut self,
        cmd: &mut DeprecateCourse,
        player: &mut PlayerInfo,
        _params: &mut CommandParams<'_, E>,
    ) -> UResult<()>
    where
        E: Invokable,
    {
        if let Some(course) = player.courses.get_mut(&cmd.course) {
            course.deprecated = true;
        }
        Ok(())
    }
}

/// A player key is used to uniquely identify a player
/// between games/saves&loads.
///
/// This shouldn't change for a player whenever possible
/// (e.g. steam id instead of steam name).
#[derive(Debug, PartialEq)]
pub enum PlayerKey {
    /// User authenticated with steam
    #[cfg(feature = "steam")]
    Steam(steamworks::SteamId),
    #[cfg(not(feature = "steam"))]
    Username(String),
}

pub(crate) struct PlayerInfo {
    pub uid: PlayerId,
    /// Only should be used for displaying
    pub name: String,
    /// Should be unique and never change
    pub key: PlayerKey,
    pub state: player::State,

    pub money: UniDollar,
    pub rating: i16,

    pub notifications: Vec<Notification>,
    pub staff_issues: FNVMap<Entity, IssueState>,

    pub courses: FNVMap<course::CourseId, course::Course>,
    pub next_course_id: u32,

    pub next_stat_collection: i32,
    pub update_id: u32,
    pub history: VecDeque<packet::HistoryEntry>,
    pub current_income: UniDollar,
    pub current_outcome: UniDollar,
    pub grades: [u32; 6],

    next_rating_update: i32,
    next_course_update: i32,

    staff_for_hire: FNVMap<ResourceKey<'static>, Vec<PossibleStaff>>,
    next_staff_rebuild: i32,

    pub config: PlayerConfig,
}

#[derive(Debug)]
pub(crate) enum IssueState {
    WantsPay,
    AskedForPay(UniDollar),
    Quit,
}

/// Settings set by the player to modify their UniverCity
/// or other parts of the gameplay.
#[derive(Debug, DeltaEncode, Serialize, Deserialize, PartialEq, Clone)]
#[delta_complete]
pub struct PlayerConfig {}

impl Default for PlayerConfig {
    fn default() -> PlayerConfig {
        PlayerConfig {}
    }
}

#[derive(Debug)]
struct PossibleStaff {
    unique_id: u32,

    name: (Arc<str>, Arc<str>),
    description: String,
    variant: usize,
    stats: [f32; Stats::MAX],
    hire_price: UniDollar,
}

impl PlayerInfo {
    pub fn new(
        key: PlayerKey,
        name: String,
        uid: PlayerId,
        staff_types: &[StaffInfo],
    ) -> PlayerInfo {
        PlayerInfo {
            uid,
            name,
            key,
            state: player::State::None,
            money: UniDollar(50_000),
            rating: 0,

            notifications: vec![],
            staff_issues: FNVMap::default(),

            courses: FNVMap::default(),
            // The 0 id is reserved
            next_course_id: 1,

            next_stat_collection: 0,
            update_id: 0,
            history: (0..14).map(|_| packet::HistoryEntry::default()).collect(),
            current_income: UniDollar(0),
            current_outcome: UniDollar(0),
            grades: [0; 6],

            next_rating_update: 20,
            next_course_update: 20,

            next_staff_rebuild: 0,
            staff_for_hire: staff_types
                .iter()
                .map(|v| (v.entity.clone(), Vec::new()))
                .collect(),
            config: PlayerConfig::default(),
        }
    }

    pub fn tick<S>(
        &mut self,
        log: &Logger,
        np: Option<(&mut NetworkedPlayer<S>, &mut Connection<S>)>,
        assets: &AssetManager,
        scripting: &script::Engine,
        level: &Level,
        entities: &mut Container,
        day_tick: &DayTick,
    ) where
        S: Socket,
    {
        use lua::{Ref, Table};
        use rand::seq::SliceRandom;
        use rand::{thread_rng, Rng};
        use std::cmp;

        self.staff_issues.retain(|e, _| entities.is_valid(*e));
        for (e, state) in &mut self.staff_issues {
            match *state {
                IssueState::WantsPay => {
                    let nid = entities.get_component::<NetworkId>(*e).map(|v| v.0);
                    if let Some(paid) = entities.get_component_mut::<Paid>(*e) {
                        // Always increase the wanted a amount incase payment wasn't the
                        // trigger for this
                        paid.wanted_cost += paid.cost / 100;

                        *state = IssueState::AskedForPay(paid.wanted_cost);
                        if let Some(id) = nid {
                            self.notifications.push(Notification::StaffPay {
                                entity_id: id,
                                wants: paid.wanted_cost,
                            });
                        }
                    }
                }
                IssueState::AskedForPay(_) => {}
                IssueState::Quit => {
                    if entities.get_component::<Quitting>(*e).is_none() {
                        entities.add_component(*e, Quitting);
                        entities.remove_component::<Owned>(*e);
                        if let Some(id) = entities.get_component::<NetworkId>(*e).map(|v| v.0) {
                            self.notifications
                                .push(Notification::StaffQuit { entity_id: id });
                        }
                    }
                }
            }
        }

        self.next_stat_collection -= 1;
        if self.next_stat_collection <= 0 {
            self.update_id += 1;
            // Every 2 minutes
            self.next_stat_collection = 20 * 60 * 2;
            // Remove the oldest entry
            self.history.pop_front();
            let students = entities.with(
                |em: EntityManager<'_>, owned: Read<Owned>, student: Read<StudentController>| {
                    let mask = student.mask().and(&owned);
                    em.iter_mask(&mask)
                        .filter(|e| assume!(log, owned.get_component(*e)).player_id == self.uid)
                        .count()
                },
            );
            self.history.push_back(packet::HistoryEntry {
                total: self.money,
                income: self.current_income,
                outcome: self.current_outcome,
                students: students as u32,
                grades: self.grades,
            });
            self.grades = [0; 6];
            self.current_income = Default::default();
            self.current_outcome = Default::default();

            if let Some((_np, con)) = np {
                let _ = con.ensure_send(packet::UpdateStats {
                    update_id: self.update_id,
                    history: AlwaysVec(self.history.clone().into()),
                });
            }
        }
        self.next_rating_update -= 1;
        if self.next_rating_update <= 0 {
            self.next_rating_update = 20;

            let mut total: f64 = 0.0;
            let mut count = 0;
            entities.with(
                |em: EntityManager<'_>,
                 s_info: Read<StudentController>,
                 mut student_vars: Write<StudentVars>,
                 owned: Read<Owned>| {
                    for (e, owned) in em.group_mask(&owned, |m| m.and(&s_info).and(&student_vars)) {
                        let vars = assume!(log, student_vars.get_custom(e));
                        if owned.player_id == self.uid {
                            count += 1;
                            total += f64::from(vars.get_stat(Stats::STUDENT_HAPPINESS));
                        }
                    }
                },
            );

            if count > 0 {
                let avg = total / f64::from(count);
                let add_rating = (avg - 0.25) * 25.0;
                self.rating += add_rating as i16;

                self.rating = cmp::min(cmp::max(self.rating, -30_000), 30_000);
            }
        }
        self.next_staff_rebuild -= 1;
        if self.next_staff_rebuild <= 0 {
            self.next_staff_rebuild = 20 * 60 * 2; // Two minutes

            let generate = Ref::new_string(scripting, "generate");
            let first_name = Ref::new_string(scripting, "first_name");
            let surname = Ref::new_string(scripting, "surname");
            let description = Ref::new_string(scripting, "description");
            let price = Ref::new_string(scripting, "price");
            let stats_key = Ref::new_string(scripting, "stats");
            let variant_key = Ref::new_string(scripting, "variant");
            let mut rng = thread_rng();

            #[derive(Serialize)]
            struct PlayerInfo {
                day: u32,
            }
            let player_script_info = assume!(
                log,
                lua::to_table(scripting, &PlayerInfo { day: day_tick.day })
            );

            for (key, staff) in &mut self.staff_for_hire {
                let ty = assume!(
                    log,
                    assets.loader_open::<Loader<ServerComponent>>(key.borrow())
                );
                if let Some(gen) = ty.generator.as_ref() {
                    staff.retain(|_v| rng.gen_bool(1.0 / 3.0));

                    let wanted_size = rng.gen_range(4, 10);
                    let current_size = staff.len();
                    let variant = entity_variant(&ty);

                    for _ in current_size..wanted_size {
                        let staff_info = match scripting
                            .with_borrows()
                            .borrow_mut(entities)
                            .invoke_function::<_, Ref<Table>>(
                                "invoke_module_method",
                                (
                                    Ref::new_string(scripting, gen.module()),
                                    Ref::new_string(scripting, gen.resource()),
                                    generate.clone(),
                                    player_script_info.clone(),
                                ),
                            ) {
                            Err(err) => {
                                error!(log, "Failed to generate staff"; "ty" => ? key, "error" => % err);
                                continue;
                            }
                            Ok(val) => val,
                        };

                        let mut stats: [f32; Stats::MAX] = [-99.0; Stats::MAX];
                        for (val, stat) in stats.iter_mut().zip(variant.stats()) {
                            *val = stat.default_value();
                        }
                        let lua_stats =
                            if let Some(v) = staff_info.get::<_, Ref<Table>>(stats_key.clone()) {
                                v
                            } else {
                                Ref::new_table(scripting)
                            };
                        for (k, v) in lua_stats.iter::<Ref<String>, f64>() {
                            if let Some(stat) = Stat::from_str(variant, &k) {
                                stats[stat.index] = v as f32;
                            } else {
                                warn!(log, "Invalid stat: {:?}", k);
                            }
                        }

                        let e_variant =
                            staff_info.get::<_, i32>(variant_key.clone()).unwrap_or(0) as usize;

                        let name = if let (Some(f), Some(s)) = (
                            staff_info.get::<_, Ref<String>>(first_name.clone()),
                            staff_info.get::<_, Ref<String>>(surname.clone()),
                        ) {
                            ((*f).into(), (*s).into())
                        } else {
                            let variant = &ty.variants[e_variant];
                            (
                                variant
                                    .name_list
                                    .first
                                    .choose(&mut rng)
                                    .cloned()
                                    .unwrap_or_else(|| "Missing".into()),
                                variant
                                    .name_list
                                    .second
                                    .choose(&mut rng)
                                    .cloned()
                                    .unwrap_or_else(|| "Name".into()),
                            )
                        };

                        let ps = PossibleStaff {
                            unique_id: rng.gen(),

                            name,
                            description: if let Some(v) =
                                staff_info.get::<_, Ref<String>>(description.clone())
                            {
                                v
                            } else {
                                error!(log, "Missing description for staff"; "ty" => ?key);
                                continue;
                            }
                            .to_string(),
                            hire_price: UniDollar(i64::from(
                                staff_info.get::<_, i32>(price.clone()).unwrap_or(1),
                            )),
                            stats,
                            variant: e_variant,
                        };
                        staff.push(ps);
                    }
                } else {
                    error!(log, "Missing generator"; "ty" => ?key);
                }
            }
        }

        self.next_course_update -= 1;
        if self.next_course_update <= 0 {
            self.next_course_update = 20 * 10;
            entities.with(
                |em: EntityManager,
                 timetable: ecs::Read<TimeTable>,
                 mut booked: ecs::Write<Booked>| {
                    self.courses.retain(|k, v| {
                        if v.deprecated {
                            if !em.group(&timetable).any(|(_, v)| v.course == *k) {
                                v.deinit_world_raw(&*level.rooms.borrow(), &mut booked);
                                return false;
                            }
                        }
                        true
                    });
                },
            );
        }
    }
}

impl player::Player for PlayerInfo {
    type EntityCreator = ServerEntityCreator;
    type EntityInfo = ServerComponent;

    fn get_uid(&self) -> PlayerId {
        self.uid
    }

    fn set_state(&mut self, state: player::State) {
        self.state = state;
    }
    fn get_state(&self) -> player::State {
        self.state.clone()
    }

    fn get_money(&self) -> UniDollar {
        self.money
    }

    fn change_money(&mut self, val: UniDollar) {
        self.money += val;
        if val < UniDollar(0) {
            self.current_outcome += -val;
        } else {
            self.current_income += val;
        }
    }

    fn get_rating(&self) -> i16 {
        self.rating
    }

    fn set_rating(&mut self, val: i16) {
        self.rating = val;
    }

    fn can_charge(&self) -> bool {
        true
    }

    fn get_config(&self) -> PlayerConfig {
        self.config.clone()
    }

    fn set_config(&mut self, cfg: PlayerConfig) {
        self.config = cfg;
    }
}
