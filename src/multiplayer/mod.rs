#[cfg(feature = "steam")]
use server::steamworks;
use std::net;
use std::time;

use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::mpsc;

use crate::server;
use crate::server::assets;
use crate::server::network::{self, packet};
use crate::server::saving::filesystem::*;

use super::{GameInstance, GameState};
use crate::errors;
use crate::instance;
use crate::prelude::*;
use crate::render;
use crate::state;
use crate::ui;

mod dedicated_server;

pub(crate) struct MultiPlayer(pub String);

pub(crate) struct MenuState {
    ui: Option<ui::Node>,
    last_refresh: time::Instant,
    error: Option<String>,
}

impl MenuState {
    pub(crate) fn new(error: Option<String>) -> MenuState {
        MenuState {
            ui: None,
            last_refresh: time::Instant::now(),
            error,
        }
    }
}

impl MultiplayerReturn for MenuState {
    fn return_ok() -> Self {
        MenuState::new(None)
    }
    fn return_error<S: Into<String>>(err: S) -> Self {
        MenuState::new(Some(err.into()))
    }
}

struct ModeDedicatedServer;
struct ModeHostSteam;
#[cfg(feature = "steam")]
struct JoinSteam(steamworks::LobbyId);

#[cfg(feature = "steam")]
fn generate_user_list(ui: &ui::Node, steam: &steamworks::Client, renderer: &mut render::Renderer) {
    if let Some(lobby_list) = query!(ui, content(lobby_list = true)).next() {
        for c in lobby_list.children() {
            lobby_list.remove_child(c);
        }
        let friends = steam.friends();
        let current = friends.get_friends(steamworks::FriendFlags::ALL);
        for (idx, friend) in current
            .into_iter()
            .filter(|friend| {
                let game = friend.game_played();
                game.map_or(false, |game| {
                    game.game.app_id() == crate::STEAM_APP_ID && game.lobby.is_valid()
                })
            })
            .enumerate()
        {
            let game = if let Some(game) = friend.game_played() {
                game
            } else {
                continue;
            };
            let icon = if let Some(data) = friend.medium_avatar() {
                let icon = ResourceKey::new("dynamic", format!("64@64@steam_icon_{}", idx));
                let s = icon.as_string();
                renderer.update_image(icon, 64, 64, data);
                s
            } else {
                "solid".to_owned()
            };
            let members = steam.matchmaking().lobby_member_count(game.lobby);
            let num_players = if members == 1 {
                "1 player".into()
            } else {
                format!("{} players", members)
            };
            let entry = node! {
                entry {
                    player_icon(icon = icon)
                    content {
                        @text(friend.name())
                        @text(" - ")
                        @text(num_players)
                    }
                    button {
                        content {
                            @text("Join")
                        }
                    }
                }
            };
            if let Some(btn) = query!(entry, button).next() {
                let lobby = game.lobby;
                btn.set_property(
                    "on_click",
                    ui::MethodDesc::<ui::MouseUpEvent>::native(move |evt, _, _| {
                        evt.emit(JoinSteam(lobby));
                        true
                    }),
                );
            }
            lobby_list.add_child(entry);
        }
    }
}

impl state::State for MenuState {
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(MenuState {
            ui: self.ui.clone(),
            last_refresh: self.last_refresh,
            error: self.error.clone(),
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
            .create_node(ResourceKey::new("base", "menus/multiplayer"));
        if let Some(server) = query!(node, button(id = "server")).next() {
            server.set_property(
                "on_click",
                ui::MethodDesc::<ui::MouseUpEvent>::native(|evt, _, _| {
                    evt.emit(ModeDedicatedServer);
                    true
                }),
            );
        }
        if let Some(server) = query!(node, button(id = "friends")).next() {
            server.set_property(
                "on_click",
                ui::MethodDesc::<ui::MouseUpEvent>::native(|evt, _, _| {
                    evt.emit(ModeHostSteam);
                    true
                }),
            );
        }
        #[cfg(feature = "steam")]
        generate_user_list(&node, &state.steam, &mut state.renderer);
        if let Some(error) = self.error.as_ref() {
            if let Some(error_box) = query!(node, server_connect_error).next() {
                error_box.set_property("show", true);
                let txt = assume!(state.global_logger, query!(error_box, @text).next());
                txt.set_text(error.as_str());
            }
        }

        self.ui = Some(node);

        state::Action::Nothing
    }

    fn inactive(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState) {
        if let Some(node) = self.ui.take() {
            state.ui_manager.remove_node(node);
        }
    }

    fn tick(
        &mut self,
        _instance: &mut Option<GameInstance>,
        state: &mut GameState,
    ) -> state::Action {
        if self.last_refresh.elapsed() > time::Duration::from_secs(5) {
            let ui = assume!(state.global_logger, self.ui.clone());
            self.last_refresh = time::Instant::now();
            #[cfg(feature = "steam")]
            generate_user_list(&ui, &state.steam, &mut state.renderer);
        }
        state::Action::Nothing
    }

    fn ui_event(
        &mut self,
        _instance: &mut Option<GameInstance>,
        _state: &mut GameState,
        evt: &mut server::event::EventHandler,
    ) -> state::Action {
        use std::thread;
        let mut action = state::Action::Nothing;
        evt.handle_event::<ModeDedicatedServer, _>(|_| {
            action = state::Action::Switch(Box::new(dedicated_server::MenuState::new(None)));
        });
        #[cfg(feature = "steam")]
        evt.handle_event::<ModeHostSteam, _>(|_| {
            action = state::Action::Switch(Box::new(crate::save_file::MenuState::new(
                server::saving::SaveType::ServerFreePlay,
                |state, name| {
                    let (socket_send, socket_recv) = mpsc::channel();
                    let assets = state.asset_manager.clone();

                    let server_log = state
                        .global_logger
                        .new(o!("server" => true, "local" => true));
                    let steam = state.steam.clone();
                    let name = name.to_owned();
                    let _server_thread = thread::spawn(move || {
                        let fs = crate::make_filesystem(
                            #[cfg(feature = "steam")]
                            &steam,
                        );
                        let fs = fs.into_boxed();
                        let (mut server, shutdown) = server::Server::<SteamSocketListener, _>::new(
                            server_log,
                            assets,
                            steam.clone(),
                            fs,
                            steam,
                            server::ServerConfig {
                                save_type: server::saving::SaveType::ServerFreePlay,
                                save_name: name,
                                min_players: 1,
                                max_players: 32,
                                autostart: false,
                                player_area_size: 100,
                                locked_players: false,
                                mission: None,
                                tick_rate: std::cell::Cell::new(20),
                            },
                            None,
                            None,
                        )
                        .expect("Failed to start local server");
                        let socket = server.client_localsocket();
                        assume!(server.log, socket_send.send((socket, shutdown)));
                        server.run();
                    });
                    // TODO: Handle the shutdown waiter
                    Box::new(ConnectingState::<MenuState, network::SteamSocket, _>::new(
                        move |state| {
                            let (socket, _shutdown) =
                                assume!(state.global_logger, socket_recv.recv());
                            Ok(socket)
                        },
                    ))
                },
            )))
        });
        #[cfg(feature = "steam")]
        evt.handle_event::<JoinSteam, _>(|JoinSteam(lobby)| {
            action = state::Action::Switch(Box::new(ConnectingState::<
                MenuState,
                network::SteamClientSocket,
                _,
            >::new(move |state| {
                SteamClientSocket::connect(
                    &state.global_logger,
                    state.steam.clone(),
                    &state.steam_single,
                    lobby,
                )
            })))
        });
        action
    }
}

struct ConnectInfo {
    sender: network::Sender,
    receiver: network::Receiver,
    #[cfg(feature = "steam")]
    auth_ticket: Option<steamworks::AuthTicket>,
}

pub(crate) trait MultiplayerReturn: state::State {
    fn return_ok() -> Self;
    fn return_error<S: Into<String>>(err: S) -> Self;
}

pub(crate) struct ConnectingState<R, S, F> {
    _return: PhantomData<(R, S)>,
    connect_func: Rc<F>,

    start_time: time::Instant,

    ui: Option<ui::Node>,
    info: Option<ConnectInfo>,
}

impl<R, S, F> ConnectingState<R, S, F>
where
    S: network::Socket + 'static,
    F: Fn(&mut crate::GameState) -> server::UResult<S> + 'static,
{
    pub(crate) fn new(connect: F) -> ConnectingState<R, S, F> {
        ConnectingState {
            _return: PhantomData,
            connect_func: Rc::new(connect),

            start_time: time::Instant::now(),

            ui: None,
            info: None,
        }
    }
}

impl<R, S, F> state::State for ConnectingState<R, S, F>
where
    R: MultiplayerReturn,
    S: network::Socket + 'static,
    F: Fn(&mut crate::GameState) -> server::UResult<S> + 'static,
{
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(ConnectingState {
            _return: self._return,
            connect_func: self.connect_func.clone(),

            start_time: self.start_time,

            ui: self.ui.clone(),
            info: None,
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
        let node = state.ui_manager.create_node(assets::ResourceKey::new(
            "base",
            "menus/multiplayer_connecting",
        ));
        self.ui = Some(node.clone());

        let socket = match (self.connect_func)(state) {
            Ok(val) => val,
            Err(err) => {
                return state::Action::Switch(Box::new(R::return_error(format!("{}", err))))
            }
        };

        let (mut sender, receiver) = socket.split(&state.global_logger);

        #[cfg(feature = "steam")]
        let (auth_ticket, ticket) = state.steam.user().authentication_session_ticket();

        if let Err(err) = sender.ensure_send(packet::RemoteConnectionStart {
            #[cfg(feature = "steam")]
            name: state.steam.friends().name(),
            #[cfg(feature = "steam")]
            steam_id: state.steam.user().steam_id().raw(),
            #[cfg(not(feature = "steam"))]
            name: "Player".into(),
            #[cfg(feature = "steam")]
            ticket: packet::Raw(ticket),
        }) {
            return state::Action::Switch(Box::new(R::return_error(format!("{}", err))));
        }
        self.info = Some(ConnectInfo {
            sender,
            receiver,
            #[cfg(feature = "steam")]
            auth_ticket: Some(auth_ticket),
        });
        state::Action::Nothing
    }

    fn inactive(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState) {
        if let Some(node) = self.ui.take() {
            state.ui_manager.remove_node(node);
        }
    }

    fn tick(
        &mut self,
        _instance: &mut Option<GameInstance>,
        state: &mut GameState,
    ) -> state::Action {
        if let Some(mut info) = self.info.take() {
            match info.receiver.try_recv() {
                Ok(packet::Packet::ServerConnectionStart(pck)) => {
                    return state::Action::Switch(Box::new(LobbyState::<R>::new(pck.uid, info)))
                }
                Ok(packet::Packet::ServerConnectionFail(pck)) => {
                    return state::Action::Switch(Box::new(R::return_error(pck.reason)));
                }
                Ok(packet::Packet::GameBegin(pck)) => {
                    match GameInstance::multi_player(
                        &state.global_logger,
                        &state.asset_manager,
                        #[cfg(feature = "steam")]
                        state.steam.clone(),
                        pck,
                        info.sender,
                        info.receiver,
                    ) {
                        Ok(mut instance) => {
                            #[cfg(feature = "steam")]
                            {
                                instance.auth_ticket = info.auth_ticket;
                            }
                            return state::Action::Switch(Box::new(instance::BaseState::new(
                                instance,
                            )));
                        }
                        Err(err) => {
                            return state::Action::Switch(Box::new(R::return_error(format!(
                                "{}",
                                err
                            ))))
                        }
                    }
                }
                Ok(_) => {
                    return state::Action::Switch(Box::new(R::return_error(
                        "Incorrect packet".to_owned(),
                    )))
                }
                Err(server::errors::Error(server::errors::ErrorKind::NoData, _)) => {}
                Err(err) => {
                    return state::Action::Switch(Box::new(R::return_error(format!("{}", err))))
                }
            }
            self.info = Some(info);
        }
        if self.start_time.elapsed() > time::Duration::from_secs(15) {
            state::Action::Switch(Box::new(R::return_error(
                "Timed out connecting to server".to_owned(),
            )))
        } else {
            state::Action::Nothing
        }
    }

    fn ui_event(
        &mut self,
        _instance: &mut Option<GameInstance>,
        state: &mut GameState,
        evt: &mut server::event::EventHandler,
    ) -> state::Action {
        let mut action = state::Action::Nothing;
        let ui = assume!(state.global_logger, self.ui.clone());
        evt.handle_event_if::<crate::CancelEvent, _, _>(
            |evt| evt.0.is_same(&ui),
            |_| action = state::Action::Switch(Box::new(R::return_ok())),
        );
        action
    }
}

struct LobbyState<R> {
    _return: PhantomData<R>,
    uid: i16,

    last_ping: time::Instant,
    current_players: Vec<packet::LobbyEntry>,
    can_start: bool,

    ui: Option<ui::Node>,
    info: Option<ConnectInfo>,
}

impl<R> LobbyState<R> {
    fn new(uid: i16, info: ConnectInfo) -> LobbyState<R> {
        LobbyState {
            _return: PhantomData,
            uid,

            last_ping: time::Instant::now(),
            current_players: vec![],
            can_start: false,

            ui: None,
            info: Some(info),
        }
    }

    fn rebuild_player_list(
        &mut self,
        #[cfg(feature = "steam")] steam: &steamworks::Client,
        renderer: &mut render::Renderer,
    ) {
        let ui = self.ui.as_ref().expect("UI not created");
        #[cfg(feature = "steam")]
        let friends = steam.friends();
        #[cfg(feature = "steam")]
        {
            let lobby = friends
                .get_friend(steam.user().steam_id())
                .game_played()
                .map(|v| v.lobby)
                .filter(|v| v.is_valid());
            if let Some(lobby) = lobby {
                if let Some(btn) = query!(ui, button(id = "invite_button")).next() {
                    btn.set_property("disabled", false);
                    let steam = steam.clone();
                    btn.set_property(
                        "on_click",
                        ui::MethodDesc::<ui::MouseUpEvent>::native(move |_evt, _, _| {
                            steam.friends().activate_invite_dialog(lobby);
                            true
                        }),
                    );
                }
            }
        }
        if let Some(lobby_list) = query!(ui, content(lobby_list = true)).next() {
            for c in lobby_list.children() {
                lobby_list.remove_child(c);
            }
            for (idx, player) in self.current_players.iter().enumerate() {
                #[cfg(feature = "steam")]
                {
                    let friend = friends.get_friend(steamworks::SteamId::from_raw(player.steam_id));
                    let icon = if let Some(data) = friend.medium_avatar() {
                        let icon = ResourceKey::new("dynamic", format!("64@64@steam_icon_{}", idx));
                        let s = icon.as_string();
                        renderer.update_image(icon, 64, 64, data);
                        s
                    } else {
                        "solid".to_owned()
                    };
                    lobby_list.add_child(node! {
                        entry(ready = player.ready) {
                            player_icon(icon = icon)
                            content {
                                @text(friend.name())
                            }
                        }
                    });
                }
                #[cfg(not(feature = "steam"))]
                {
                    lobby_list.add_child(node! {
                        entry(ready = player.ready) {
                            player_icon(icon = "solid".to_owned())
                            content {
                                @text("Player".to_owned())
                            }
                        }
                    });
                }
            }
        }
    }
}

impl<R> state::State for LobbyState<R>
where
    R: MultiplayerReturn,
{
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(LobbyState {
            _return: self._return,
            uid: self.uid,

            last_ping: self.last_ping,
            current_players: self.current_players.clone(),
            can_start: self.can_start,

            ui: self.ui.clone(),
            info: None,
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
        let ui = state
            .ui_manager
            .create_node(assets::ResourceKey::new("base", "menus/multiplayer_lobby"));
        if let Some(info) = self.info.as_mut() {
            if let Err(err) = info.sender.ensure_send(packet::EnterLobby {}) {
                return state::Action::Switch(Box::new(R::return_error(format!("{}", err))));
            }
        }

        self.ui = Some(ui);
        state::Action::Nothing
    }

    fn inactive(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState) {
        if let Some(ui) = self.ui.take() {
            state.ui_manager.remove_node(ui);
        }
    }

    fn tick(
        &mut self,
        _instance: &mut Option<GameInstance>,
        state: &mut GameState,
    ) -> state::Action {
        use crate::server::network::packet::Packet;
        let ui = self.ui.clone().expect("UI not created");
        if let Some(mut info) = self.info.take() {
            match info.receiver.try_recv() {
                Ok(Packet::UpdateLobby(pck)) => {
                    self.current_players = pck.players.0;
                    self.rebuild_player_list(
                        #[cfg(feature = "steam")]
                        &state.steam,
                        &mut state.renderer,
                    );
                    self.can_start = pck.can_start;
                    if let Some(btn) = query!(ui, button(id = "start_button")).next() {
                        btn.set_property("disabled", !self.can_start);
                    }
                }
                Ok(Packet::GameBegin(pck)) => {
                    match GameInstance::multi_player(
                        &state.global_logger,
                        &state.asset_manager,
                        #[cfg(feature = "steam")]
                        state.steam.clone(),
                        pck,
                        info.sender,
                        info.receiver,
                    ) {
                        Ok(mut instance) => {
                            #[cfg(feature = "steam")]
                            {
                                instance.auth_ticket = info.auth_ticket;
                            }
                            return state::Action::Switch(Box::new(instance::BaseState::new(
                                instance,
                            )));
                        }
                        Err(err) => {
                            return state::Action::Switch(Box::new(R::return_error(format!(
                                "{}",
                                err
                            ))))
                        }
                    }
                }
                Ok(Packet::KeepAlive(..))
                | Err(server::errors::Error(server::errors::ErrorKind::NoData, _)) => {}
                Ok(pck) => warn!(state.global_logger, "Incorrect packet: {:?}", pck),
                Err(err) => {
                    return state::Action::Switch(Box::new(R::return_error(format!("{}", err))))
                }
            }
            if self.last_ping.elapsed() > time::Duration::from_secs(3) {
                self.last_ping = time::Instant::now();
                self.rebuild_player_list(
                    #[cfg(feature = "steam")]
                    &state.steam,
                    &mut state.renderer,
                );
                if let Err(err) = info.sender.send(packet::KeepAlive {}) {
                    return state::Action::Switch(Box::new(R::return_error(format!("{}", err))));
                }
            }
            self.info = Some(info);
        }

        state::Action::Nothing
    }

    fn ui_event(
        &mut self,
        _instance: &mut Option<GameInstance>,
        state: &mut GameState,
        evt: &mut server::event::EventHandler,
    ) -> state::Action {
        let mut action = state::Action::Nothing;
        let ui = assume!(state.global_logger, self.ui.clone());
        evt.handle_event_if::<crate::CancelEvent, _, _>(
            |evt| evt.0.is_same(&ui),
            |_| action = state::Action::Switch(Box::new(R::return_ok())),
        );
        let mut info = self.info.take();
        evt.handle_event_if::<crate::AcceptEvent, _, _>(
            |evt| evt.0.is_same(&ui),
            |_| {
                if self.can_start {
                    if let Some(info) = info.as_mut() {
                        let _ = info.sender.ensure_send(packet::RequestGameBegin {});
                    }
                }
            },
        );
        self.info = info;
        action
    }
}
