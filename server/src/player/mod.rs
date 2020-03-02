//! Player handling for commands and in general

mod requests;
pub use self::requests::*;
mod handle;
pub(crate) use self::handle::*;
pub use self::handle::{PlayerConfig, PlayerKey};

use crate::ecs;
use crate::level::room;
use crate::prelude::*;

/// Represents a player id
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, DeltaEncode)]
pub struct Id(pub i16);

/// An instance of the player either local or
/// remote
pub trait Player {
    /// The type used to create entities
    type EntityCreator: crate::entity::EntityCreator;
    /// The type used to load entity descriptions
    type EntityInfo: crate::entity::ComponentCreator<Creator = Self::EntityCreator>;

    /// Returns the player's unique id
    fn get_uid(&self) -> Id;

    /// Changes the state of the player
    fn set_state(&mut self, state: State);
    /// Returns a copy of the player's state
    fn get_state(&self) -> State;

    /// Whether the player can be charged money
    /// or not. Normally used for remote clients.
    fn can_charge(&self) -> bool;
    /// Returns the amount of money the player has
    fn get_money(&self) -> UniDollar;
    /// Changes the amount of money the player has by the given amount
    fn change_money(&mut self, val: UniDollar);

    /// Returns the rating of the player's university
    fn get_rating(&self) -> i16;
    /// Updates the rating of the player's university
    fn set_rating(&mut self, val: i16);

    /// Returns a copy of the current config set by the player
    fn get_config(&self) -> PlayerConfig;
    /// Modifys the player's config
    fn set_config(&mut self, cfg: PlayerConfig);
}

/// Contains the state and related information for a player
#[derive(Clone, Debug, PartialEq, DeltaEncode)]
pub enum State {
    /// Default state
    None,
    /// Building a room
    BuildRoom {
        /// The id of the room being editted
        active_room: room::Id,
    },
    /// Editting/building a room
    EditRoom {
        /// The id of the room being editted
        active_room: room::Id,
    },
    /// Editting/placing a staff member
    EditEntity {
        /// Not required by clients. Only by the
        /// server and the sender.
        #[delta_default]
        entity: Option<ecs::Entity>,
    },
}

impl State {
    /// Returns true if the current state is `None`
    pub fn is_none(&self) -> bool {
        match *self {
            State::None => true,
            _ => false,
        }
    }
}
