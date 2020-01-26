//! Loopback networking

use super::*;

use std::sync::mpsc;
use std::fmt::{self, Debug};
use crate::prelude::*;
use crate::errors;

/// Local connection
pub struct LoopbackSocket {
    send: mpsc::Sender<packet::Packet>,
    recv: mpsc::Receiver<packet::Packet>,
}

impl Debug for LoopbackSocket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(f, "LoopbackSocket(localhost)")
    }
}

/// Unique key for the only loopback connection
#[derive(PartialEq, Eq, Hash, Clone, Debug)]
pub struct LoopbackKey;

impl Socket for LoopbackSocket {
    type Id = LoopbackKey;

    fn is_local() -> bool { true }
    fn needs_verify() -> bool { false }

    fn id(&mut self) -> LoopbackKey {
        LoopbackKey
    }

    fn split(self, _log: &Logger) -> (Sender, Receiver) {
        (Sender::Reliable {
            inner: self.send,
        }, Receiver {
            inner: self.recv,
        })
    }
}

/// Local connection
pub struct LoopbackSocketListener {
    /// The client half of this connection
    pub client: Option<LoopbackSocket>,
    server: Option<LoopbackSocket>,
}

impl SocketListener for LoopbackSocketListener {
    type Address = ();
    type Socket = LoopbackSocket;

    fn listen<A: Into<()>>(_log: &Logger, _: A) -> errors::Result<LoopbackSocketListener> {
        let (to_server, from_client) = mpsc::channel();
        let (to_client, from_server) = mpsc::channel();
        let client = LoopbackSocket {
            send: to_server,
            recv: from_server,
        };
        let server = LoopbackSocket {
            send: to_client,
            recv: from_client,
        };
        Ok(LoopbackSocketListener {
            client: Some(client),
            server: Some(server),
        })
    }

    fn next_socket(&mut self) -> Option<Self::Socket> {
        self.server.take()
    }
    fn format_address(_addr: &Self::Address) -> String {
        "Loopback".into()
    }
}