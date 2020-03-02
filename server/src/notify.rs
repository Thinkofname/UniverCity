//! Notification related types

use crate::common::ScriptData;
use crate::prelude::*;

/// A notification that can be displayed to the player
#[derive(Debug, DeltaEncode, PartialEq, Clone)]
pub enum Notification {
    /// A staff member quitting
    StaffQuit {
        /// The network ID of the quitting entity
        entity_id: u32,
    },
    /// A staff member asking for a raise
    StaffPay {
        /// The network ID of the staff member
        entity_id: u32,
        /// The amount they want
        wants: UniDollar,
    },
    /// A text based notification that can focus a room
    RoomMissing {
        /// The room to focus
        room_id: RoomId,
        /// The icon to use
        icon: ResourceKey<'static>,
        /// The title of the notification box
        title: String,
        /// The description of the notification box
        description: String,
    },
    /// Dismisses a room missing notification
    RoomMissingDismiss(RoomId),
    /// A text based notification
    Text {
        /// The icon to use
        icon: ResourceKey<'static>,
        /// The title of the notification box
        title: String,
        /// The description of the notification box
        description: String,
    },
    /// A script controlled notification
    ///
    /// Needs to be deserialized by a script before displaying
    Script {
        /// The script to call
        script: ResourceKey<'static>,
        /// The function to call in the script
        func: String,
        /// The serialized data to pass to the script
        data: ScriptData,
    },
}
