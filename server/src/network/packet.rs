//! Packet definitions and serializing

use crate::command;
use crate::notify;
use crate::player;
use crate::prelude::*;
use delta_encode::{bitio, AlwaysVec};
use std::io::{self, Read, Write};

// Compat due to the move
pub use delta_encode::bitio::{read_len_bits, write_len_bits};

#[doc(hidden)]
#[macro_export]
macro_rules! create_ids {
    (prev($prev:ident), $name:ident) => (
        #[allow(non_upper_case_globals)]
        pub const $name: usize = $prev + 1;
    );
    (prev($prev:ident), $name:ident, $($n:ident),+) => (
        #[allow(non_upper_case_globals)]
        pub const $name: usize = $prev + 1;
        create_ids!(prev($name), $($n),+);
    );
    ($name:ident, $($n:ident),+) => (
        #[allow(non_upper_case_globals)]
        pub const $name: usize = 0;
        create_ids!(prev($name), $($n),+);
    );
    ($name:ident) => (
        #[allow(non_upper_case_globals)]
        pub const $name: usize = 0;
    );
}

macro_rules! define_packets {
    (
        $(
            $(#[$pattr:meta])*
            packet $name:ident {
                $(
                    $(#[$fattr:meta])*
                    field $fname:ident : $ftype:ty,
                )*
            }
        )*
    ) => (
        /// Contains every packet type
        #[derive(Debug, DeltaEncode)]
        pub enum Packet {
            $(
                $(#[$pattr])*
                $name($name),
            )*
        }

        $(
            $(#[$pattr])*
            #[derive(Debug, DeltaEncode)]
            pub struct $name {
                $(
                    $(#[$fattr])*
                    pub $fname : $ftype,
                )*
            }

            impl From<$name> for Packet {
                fn from(p: $name) -> Packet {
                    Packet::$name(p)
                }
            }
        )*
    )
}

define_packets! {
    /// A packet that must be received by the other side and
    /// cannot be dropped.
    packet Ensured {
        /// The id of this fragment. Used to ignore late/duplicate
        /// packets.
        field fragment_id: u16,
        /// The part id of this fragment
        field fragment_part: u16,
        /// The number of fragments minus 1
        field fragment_max_parts: u16,
        /// The packet inside this packet or at least part of it.
        field internal_packet: Raw,
    }
    /// Ack used to tell the other side that the packet fragment
    /// was received
    packet EnsuredAck {
        /// The id of this fragment. Used to ignore late/duplicate
        /// packets.
        field fragment_id: u16,
        /// The part id of this fragment
        field fragment_part: u16,
    }
    /// Sent by the player to disconnect from the server.
    ///
    /// Due to udp there is a chance the server will never get this
    packet Disconnect {}
    /// Begins a local connection. Only works when in
    /// loopback mode
    packet LocalConnectionStart {
        /// The username of the client
        field name: String,
        /// The steam id of the connecting client
        #[cfg(feature = "steam")]
        field steam_id: u64,
    }
    /// Begins a remote connection
    packet RemoteConnectionStart {
        /// The username of the client
        field name: String,
        /// The steam id of the connecting client
        #[cfg(feature = "steam")]
        field steam_id: u64,
        /// The steam auth ticket to verify
        #[cfg(feature = "steam")]
        field ticket: Raw,
    }
    /// Sent by the server after one of the previous connection
    /// packets to begin the game.
    ///
    /// This sets some of the initial state of the game namely
    /// level size. The actual contents of the level will be sent
    /// after this due to its size.
    packet ServerConnectionStart {
        /// The unique id for the player
        field uid: i16,
    }
    /// Sent by the server if the player was blocked from connecting
    /// for some reason.
    packet ServerConnectionFail {
        /// The reason the connection failed,
        field reason: String,
    }

    /// Sent by the client when it has entered the
    /// lobby
    packet EnterLobby {}
    /// Sent to update clients to changes in the lobby
    packet UpdateLobby {
        /// The unique id for this change. Higher is newer
        field change_id: u32,
        /// List of players currently in the lobby
        field players: AlwaysVec<LobbyEntry>,
        /// Whether the game can be started
        field can_start: bool,
    }
    /// Sent by the client to request the game to begin
    /// when in the lobby.
    packet RequestGameBegin {}
    /// Sent by the server to make clients either exit
    /// the lobby or instantly join a game already in
    /// progress.
    packet GameBegin {
        /// The unique id for the player.
        /// Used if rejoining a started game
        field uid: i16,
        /// The width of the level
        field width: u32,
        /// The height of the level
        field height: u32,
        /// List of players in the game.
        /// At this point this cannot change
        field players: AlwaysVec<PlayerEntry>,
        /// The mission handler for the current game if any
        field mission_handler: Option<ResourceKey<'static>>,
        /// Strings used in the data
        field strings: AlwaysVec<String>,
        /// The serialized state of idle tasks
        field idle_state: AlwaysVec<IdleState>,
        /// The serialized state of the level
        field state: Raw,
    }

    /// Sent to keep the connection open
    packet KeepAlive {}
    /// Sets the pause state of the server.
    /// Only works in loopback mode.
    packet SetPauseGame {
        /// Whether the game is paused or not.
        field paused: bool,
    }
    /// Requests for the save to save the current game
    /// Only works in loopback mode
    packet SaveGame {}

    // Level packets

    /// Sent by the client when it has loaded the level
    packet LevelLoaded {}
    /// Sent by the server to let the client start playing
    packet GameStart {}

    // Command packets

    /// List of commands executed by the client
    packet ExecutedCommands {
        /// The first id of the command in the queue.
        field start_id: u32,
        /// The list of commands sent by the client
        field commands: AlwaysVec<command::Command>,
    }
    /// Sent by the server to accept that it received
    /// the commands up to the `accepted_id` and validated them.
    packet AckCommands {
        /// The id of the last command accepted
        field accepted_id: u32,
    }
    /// Sent by the server when one of the commands fails to validate
    packet RejectCommands {
        /// The id of the last command accepted
        field accepted_id: u32,
        /// The id of the first rejected command.
        /// All after this are ignored
        field rejected_id: u32,
    }

    /// List of commands executed by different client
    packet RemoteExecutedCommands {
        /// The first id of the command in the queue.
        field start_id: u32,
        /// The list of commands sent by the client
        field commands: Raw,
    }
    /// Sent by the server to accept that it received
    /// the commands up to the `accepted_id` and validated them.
    packet AckRemoteCommands {
        /// The id of the last command accepted
        field accepted_id: u32,
    }

    // Entity packets

    /// Contains a delta frame of entity state
    packet EntityFrame {
        /// Raw entity delta state
        field data: Raw,
    }
    /// Ack's a range of entities as having their
    /// state
    packet EntityAckFrame {
        /// The frame that is being ack'd
        #[delta_bits = "14"]
        field frame: u16,
        /// The first entity id being ack'd
        #[delta_bits = "20"]
        field entity_offset: u32,
        /// The number of entities being acked
        #[delta_bits = "14"]
        field entity_count: u16,
    }
    /// Ack's the player state from the server
    packet PlayerAckFrame {
        /// The frame that is being ack'd
        field frame: u16,
    }

    /// A collection of notifications sent by the server
    packet Notification {
        /// The notifications
        field notifications: AlwaysVec<notify::Notification>,
    }
    /// A message from the server
    packet Message {
        /// The formatted message from the server
        field messages: AlwaysVec<crate::msg::Message>,
    }
    /// A message to the server
    packet ChatMessage {
        /// The unformatted message from the client
        field message: String,
    }
    /// Updates the collected stats for the player
    packet UpdateStats {
        /// The update id.
        ///
        /// Used to ignore old/duplicate updates
        field update_id: u32,
        /// The last 28 minutes of the player's
        /// stat history.
        field history: AlwaysVec<HistoryEntry>,
    }

    /// Generic request container
    packet Request {
        /// The type of request
        field ty: [u8; 4],
        /// The request id
        field id: u32,
        /// The request data
        field data: Raw,
    }
    /// Generic reply container
    packet Reply {
        /// The type of request
        field ty: [u8; 4],
        /// The request id
        field id: u32,
        /// The reply data
        field data: Raw,
    }
}

/// Serialized state for an idle task
#[derive(Debug, Clone, DeltaEncode)]
pub struct IdleState {
    /// The player who this task is for
    pub player: PlayerId,
    /// The task id
    pub idx: u32,
    /// The serialized state
    pub state: Raw,
}

/// An entry containing one step of a player's history
#[derive(Debug, Clone, DeltaEncode, Default, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Total money (income/outcome)
    pub total: UniDollar,
    /// Current income
    pub income: UniDollar,
    /// Current outcome
    pub outcome: UniDollar,
    /// Current number of students
    pub students: u32,
    /// Current grade stats
    #[serde(default)]
    pub grades: [u32; 6],
}

/// Consumes the remainder of the packet if read.
///
/// Writing sends the byte array without a prefix.
#[derive(Debug, Clone)]
pub struct Raw(pub Vec<u8>);

impl delta_encode::DeltaEncodable for Raw {
    #[inline]
    fn encode<W>(&self, _base: Option<&Self>, w: &mut bitio::Writer<W>) -> io::Result<()>
    where
        W: Write,
    {
        for b in &self.0 {
            w.write_unsigned(u64::from(*b), 8)?;
        }
        Ok(())
    }

    #[inline]
    fn decode<R>(_base: Option<&Self>, r: &mut bitio::Reader<R>) -> io::Result<Self>
    where
        R: Read,
    {
        let mut data = Vec::with_capacity(256);
        loop {
            let val = match r.read_unsigned(8) {
                Ok(val) => val as u8,
                Err(err) => {
                    if err.kind() == io::ErrorKind::UnexpectedEof {
                        break;
                    } else {
                        return Err(err);
                    }
                }
            };
            data.push(val);
        }
        Ok(Raw(data))
    }
}

/// A player id/command pair that can be serialized in a packet
#[derive(Debug, Clone, DeltaEncode)]
#[delta_always]
pub struct CommandPair {
    /// The id of the player executing the command
    #[delta_always]
    pub player_id: player::Id,
    /// The command to execute
    #[delta_always]
    pub command: command::Command,
}

/// A player in a lobby
#[derive(Debug, Clone, DeltaEncode, PartialEq)]
pub struct LobbyEntry {
    /// The player's steam id
    #[cfg(feature = "steam")]
    pub steam_id: u64,
    /// The player's uid
    pub uid: player::Id,
    /// Whether the player has finished connecting
    pub ready: bool,
}

/// A player in a lobby
#[derive(Debug, Clone, DeltaEncode, PartialEq)]
pub struct PlayerEntry {
    /// The player's username
    pub username: String,
    /// The player's uid
    pub uid: player::Id,
    /// The player's current state
    pub state: player::State,
}
