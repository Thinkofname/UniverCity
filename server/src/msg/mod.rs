//! Formatted message handling

use crate::prelude::*;

/// Helper to create messages
pub struct MessageBuilder {
    parts: Vec<MsgPart>,
    last_color: Option<MsgColor>,
    color: Option<MsgColor>,
    special: bool,
}

impl MessageBuilder {

    /// Changes the color of the following parts
    pub fn color(mut self, r: u8, g: u8, b: u8) -> Self {
        self.color = Some(MsgColor {
            r,
            g,
            b,
        });
        self
    }

    /// Flags the next part as special
    pub fn special(mut self) -> Self {
        self.special = true;
        self
    }

    /// Appends an image part to the message
    pub fn image(mut self, key: ResourceKey<'_>) -> Self {
        self.parts.push(MsgPart::Image(key.into_owned()));
        self
    }

    /// Append a text part to the message based on the current settings
    pub fn text<S: Into<String>>(mut self, text: S) -> Self {
        self.parts.push(MsgPart::Text {
            text: text.into(),
            color: if self.color.is_none() || self.color == self.last_color {
                None
            } else {
                self.color
            },
            special: self.special,
        });
        self.last_color = self.color;
        self.color = None;
        self.special = false;
        self
    }

    /// Builds the message
    pub fn build(self) -> Message {
        Message {
            parts: self.parts,
        }
    }
}

/// A single message that can be send to a client and
/// displayed to the user
#[derive(Debug, Clone, PartialEq, DeltaEncode)]
pub struct Message {
    /// The parts of this message
    pub parts: Vec<MsgPart>,
}

impl Message {
    /// Begins building a message
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> MessageBuilder {
        MessageBuilder {
            parts: Vec::new(),
            last_color: None,
            color: None,
            special: false,
        }
    }
}

/// A single part of this message
#[derive(Debug, Clone, PartialEq, DeltaEncode)]
pub enum MsgPart {
    /// An embedded image into the message
    Image(ResourceKey<'static>),
    /// Text with optional formatting
    Text {
        /// The text to display
        text: String,
        /// Optional color for this message.
        /// Defaults to the previous color if none
        color: Option<MsgColor>,
        /// Whether this part should be highlighted by
        /// the client.
        special: bool,
    },
}

/// A color for a message part
#[derive(Debug, Clone, Copy, PartialEq, Eq, DeltaEncode)]
pub struct MsgColor {
    /// The red component of the color
    pub r: u8,
    /// The green component of the color
    pub g: u8,
    /// The blue component of the color
    pub b: u8,
}
