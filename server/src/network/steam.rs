//! Loopback networking

use super::*;

use std::sync::{mpsc, Arc, Mutex};
use std::fmt::{self, Debug};
use std::time::Duration;
use crate::prelude::*;
use crate::errors;
use steamworks;

/// A client steam socket connection
pub struct SteamClientSocket {
    steam: steamworks::Client,
    /// The id of the lobby that this socket is connecting too
    pub lobby: steamworks::LobbyId,
}

impl SteamClientSocket {
    /// Creates a udp socket client
    pub fn connect(log: &Logger, steam: steamworks::Client, single: &steamworks::SingleClient, lobby: steamworks::LobbyId) -> UResult<SteamClientSocket> {
        {
            let log = log.clone();
            let (lobby_send, lobby_read) = mpsc::channel();
            steam.matchmaking().join_lobby(lobby, move |res| {
                match res {
                    Ok(lobby_id) => {
                        info!(log, "Lobby joined"; "lobby" => ?lobby_id);
                        assume!(log, lobby_send.send(true));
                    },
                    Err(_) => {
                        error!(log, "Failed to join a lobby");
                        assume!(log, lobby_send.send(false));
                    },
                }
            });
            loop {
                if let Ok(res) = lobby_read.recv_timeout(Duration::from_millis(50)) {
                    if res {
                        break;
                    } else {
                        bail!("Failed to connect to the steam lobby");
                    }
                }
                single.run_callbacks();
            }
        }
        Ok(SteamClientSocket {
            steam,
            lobby,
        })
    }
}

impl Debug for SteamClientSocket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(f, "SteamSocket({:?})", self.lobby)
    }
}

impl Drop for SteamClientSocket {
    fn drop(&mut self) {
        self.steam.matchmaking().leave_lobby(self.lobby);
    }
}

impl Socket for SteamClientSocket {
    type Id = SteamKey;

    fn is_local() -> bool {
        false
    }
    fn needs_verify() -> bool { true }

    /// Returns the unique id for this connection
    fn id(&mut self) -> Self::Id {
        SteamKey(self.steam.steam_id())
    }

    fn split(self, log: &Logger) -> (Sender, Receiver) {
        use std::thread;
        let (input_send, input_read) = mpsc::channel();
        let (output_send, output_read) = mpsc::channel();

        let log = log.new(o!(
            "lobby" => format!("{:?}", self.lobby),
        ));
        let owner = self.steam.matchmaking().lobby_owner(self.lobby);

        let req_cb = {
            let steam = self.steam.clone();
            self.steam.register_callback::<steamworks::P2PSessionRequest, _>(move |req| {
                if owner == req.remote {
                    steam.networking().accept_p2p_session(req.remote);
                }
            })
        };
        {
            let log = log.clone();
            let steam = self.steam.clone();
            thread::spawn(move || {
                let mut buf = vec![0; 1_000_000];
                let _req_cb = req_cb; // Keep alive whilst the thread is running
                'main:
                loop {
                    while let Some((remote, count)) = steam.networking().read_p2p_packet(&mut buf) {
                        if remote != owner {
                            continue;
                        }
                        let read_data = &buf[..count];
                        match packet_from_bytes(read_data) {
                            Ok(val) => {
                                if input_send.send(val).is_err() {
                                    break 'main;
                                }
                            },
                            Err(e) => {
                                error!(log, "Failed to decode packet: {}", e);
                                break 'main;
                            }
                        }
                    }
                    thread::sleep(Duration::from_millis(30));
                }
            });
        }

        {
            let steam = self.steam.clone();
            thread::spawn(move || {
                while let Ok((ensure, pck)) = output_read.recv() {
                    let data = if let Ok(data) = packet_to_bytes(pck, if ensure {
                        1_000_000
                    } else {
                        1200
                    }) {
                        data
                    } else {
                        break
                    };
                    if !steam.networking().send_p2p_packet(owner, if ensure {
                        steamworks::SendType::Reliable
                    } else {
                        steamworks::SendType::Unreliable
                    }, &data) {
                        break
                    }
                }
            });
        }

        (Sender::Unreliable {
            inner: output_send,
        }, Receiver {
            inner: input_read
        })
    }
}

/// Steam connection
pub enum SteamSocket {
    /// Local socket for the host
    Local {
        #[doc(hidden)]
        steam_id: steamworks::SteamId,
        #[doc(hidden)]
        send: mpsc::Sender<packet::Packet>,
        #[doc(hidden)]
        recv: mpsc::Receiver<packet::Packet>,
    },
    /// Remote steam user
    Remote {
        #[doc(hidden)]
        steam_id: steamworks::SteamId,
        #[doc(hidden)]
        send: mpsc::Sender<(bool, packet::Packet)>,
        #[doc(hidden)]
        recv: mpsc::Receiver<packet::Packet>,
    }
}

impl Debug for SteamSocket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match *self {
            SteamSocket::Local{..} => write!(f, "SteamSocket(localhost)"),
            SteamSocket::Remote{steam_id, ..} => write!(f, "SteamSocket({:?})", steam_id),
        }

    }
}

/// Unique key for steam connections
#[derive(PartialEq, Eq, Hash, Clone, Debug)]
pub struct SteamKey(steamworks::SteamId);

impl Socket for SteamSocket {
    type Id = SteamKey;

    fn is_local() -> bool { false }
    fn needs_verify() -> bool { true }

    fn id(&mut self) -> SteamKey {
        match *self {
            SteamSocket::Local{steam_id, ..} => SteamKey(steam_id),
            SteamSocket::Remote{steam_id, ..} => SteamKey(steam_id),
        }
    }

    fn split(self, _log: &Logger) -> (Sender, Receiver) {
        match self {
            SteamSocket::Local{send, recv, ..} =>
                (Sender::Reliable {
                    inner: send,
                }, Receiver {
                    inner: recv,
                }),
            SteamSocket::Remote{send, recv, ..} =>
                (Sender::Unreliable {
                    inner: send,
                }, Receiver {
                    inner: recv,
                }),
        }
    }
}

/// Local connection
pub struct SteamSocketListener {
    steam: steamworks::Client,
    /// The client half of this connection
    pub client: Option<SteamSocket>,
    server: Option<SteamSocket>,
    lobby: Arc<Mutex<Option<steamworks::LobbyId>>>,

    recv_socket: mpsc::Receiver<SteamSocket>,
    drop_send: mpsc::Sender<steamworks::SteamId>,
    _req_cb: steamworks::CallbackHandle,
}

impl SocketListener for SteamSocketListener {
    type Address = steamworks::Client;
    type Socket = SteamSocket;

    fn listen<A: Into<Self::Address>>(log: &Logger, steam: A) -> errors::Result<SteamSocketListener> {
        use std::thread;
        let (to_server, from_client) = mpsc::channel();
        let (to_client, from_server) = mpsc::channel();
        let steam = steam.into();
        let steam_id = steam.steam_id();

        let lobby = Arc::new(Mutex::new(None));
        let client = SteamSocket::Local {
            steam_id,
            send: to_server,
            recv: from_server,
        };
        let server = SteamSocket::Local {
            steam_id,
            send: to_client,
            recv: from_client,
        };

        {
            let lobby = lobby.clone();
            let log = log.clone();
            steam.matchmaking().create_lobby(steamworks::LobbyType::Public, 32, move |res| {
                match res {
                    Ok(lobby_id) => {
                        info!(log, "Lobby created"; "lobby" => ?lobby_id);
                        *assume!(log, lobby.lock()) = Some(lobby_id);
                    },
                    Err(err) => {
                        error!(log, "Failed to create a lobby"; "error" => %err);
                    },
                }
            });
        }

        let req_cb = {
            let lobby = lobby.clone();
            let log = log.clone();
            let steam2 = steam.clone();
            steam.register_callback::<steamworks::P2PSessionRequest, _>(move |req| {
                let lobby = assume!(log, lobby.lock());
                if lobby.is_some() {
                    steam2.networking().accept_p2p_session(req.remote);
                }
            })
        };
        let (send_socket, recv_socket) = mpsc::channel();
        let (send_drop, read_drop) = mpsc::channel();
        {
            let log = log.clone();
            let steam2 = steam.clone();
            thread::spawn(move || {
                let mut buf = vec![0; 1_000_000];
                let mut active_sockets: FNVMap<steamworks::SteamId, _> = FNVMap::default();
            'main:
                loop {
                    loop {
                        match read_drop.try_recv() {
                            Ok(d) => {
                                active_sockets.remove(&d);
                                steam2.networking().close_p2p_session(d);
                            },
                            Err(mpsc::TryRecvError::Empty) => break,
                            Err(mpsc::TryRecvError::Disconnected) => break 'main,
                        }
                    }
                    while let Some((remote, count)) = steam2.networking().read_p2p_packet(&mut buf) {
                        let entry = active_sockets.entry(remote).or_insert_with(|| {
                            let (input_send, input_read) = mpsc::channel();
                            let (output_send, output_read) = mpsc::channel();
                            let socket = SteamSocket::Remote {
                                steam_id: remote,
                                send: output_send,
                                recv: input_read,
                            };
                            let _ = send_socket.send(socket);

                            {
                                let steam = steam2.clone();
                                thread::spawn(move || {
                                    while let Ok((ensure, pck)) = output_read.recv() {
                                        let data = if let Ok(data) = packet_to_bytes(pck, if ensure {
                                            1_000_000
                                        } else {
                                            1200
                                        }) {
                                            data
                                        } else {
                                            break
                                        };
                                        if !steam.networking().send_p2p_packet(remote, if ensure {
                                            steamworks::SendType::Reliable
                                        } else {
                                            steamworks::SendType::Unreliable
                                        }, &data) {
                                            break
                                        }
                                    }
                                });
                            }

                            input_send
                        });

                        let read_data = &buf[..count];
                        match packet_from_bytes(read_data) {
                            Ok(val) => {
                                if entry.send(val).is_err() {
                                    continue;
                                }
                            },
                            Err(e) => {
                                error!(log, "Failed to decode packet: {}", e);
                                continue;
                            }
                        }
                    }
                    thread::sleep(Duration::from_millis(30));
                }
            });
        }

        Ok(SteamSocketListener {
            steam,
            client: Some(client),
            server: Some(server),
            lobby,
            recv_socket,
            drop_send: send_drop,
            _req_cb: req_cb,
        })
    }

    fn next_socket(&mut self) -> Option<Self::Socket> {
        if self.server.is_some() {
            self.server.take()
        } else {
            self.recv_socket.try_recv().ok()
        }
    }

    fn format_address(_addr: &Self::Address) -> String {
        "Steam Networking".into()
    }
    fn host(&self) -> Option<<Self::Socket as Socket>::Id> {
        Some(SteamKey(self.steam.steam_id()))
    }

    fn drop_socket(&mut self, id: &<Self::Socket as Socket>::Id) {
        let _ = self.drop_send.send(id.0);
    }
}

impl Drop for SteamSocketListener {
    fn drop(&mut self) {
        let lobby = self.lobby.lock().expect("Failed to lock the lobby handle");
        if let Some(lobby) = *lobby {
            self.steam.matchmaking().leave_lobby(lobby);
        }
    }
}