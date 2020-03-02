#![allow(deprecated)]
//! Common error handling

error_chain! {
    foreign_links {
        Io(::std::io::Error)
        /// Generic IO error
        ;
        Json(::serde_json::Error)
        /// Json decoding/encoding error
        ;
        Cbor(::serde_cbor::error::Error)
        /// Json decoding/encoding error
        ;
        ParseInt(::std::num::ParseIntError)
        /// Error parsing an integer
        ;
        StringDecode(::std::string::FromUtf8Error)
        /// Error decoding a string
        ;
    }

    errors {
        /// Errors from the lua scripting engine
        Lua(err: ::lua::Error) {
            description("lua scripting error")
            display("{}", err)
        }
        /// Returned when an object's placement was invalid
        InvalidPlacement(reason: String) {
            description("invalid placement location")
            display("{}", reason)
        }
        /// Returned when an object's placement was invalid
        /// and should be removed
        RemoveInvalidPlacement(reason: String) {
            description("invalid placement location")
            display("{}", reason)
        }
        /// Returned when a command was invalid
        InvalidCommand {}
        /// Returned when loading a level from the server state fails
        FailedLevelRecreation {}
        /// Returned when a room has an invalid state
        InvalidRoomState {}
        /// Returned when a room doesn't own all of the area
        /// in its bound
        RoomNoFullOwnership {}

        /// Returned when the requested asset doesn't exist
        NoSuchAsset {}
        /// Returned when the requested save doesn't exist
        NoSuchSave {}

        /// Invalid scripting state
        InvalidState {}
        /// Returned when the engine believes that a stale
        /// reference is being used.
        ///
        /// A stale reference is one that a lua script kept
        /// hold of longer than intended. This may not
        /// always be detected.
        StaleScriptReference {}

        /// Returned when an object/entity is attemped to be placed in
        /// an unplaceable area
        UnplaceableArea {}
        /// Returned when an object that doesn't exist is attempted to
        /// be removed
        MissingObject {}
        /// Returned when the player hasn't got an active room
        NoActiveRoom {}
        /// Returned when the player isn't in the correct state
        InvalidPlayerState {}
        /// Returned when the player doesn't have enough money
        NotEnoughMoney {}
        /// Returned when the room requirements aren't met
        UnmetRoomRequirements {}

        /// Returned when a student can't be given a tabletime
        /// due to no rooms
        NoTimetableRooms {}
        /// Returned when a student can't be given a tabletime
        /// because all the courses are full
        NoTimetableSpace {}

        // Networking

        /// Returned when trying to act on a closed connection
        ConnectionClosed {
            description("connection was closed/failed to open")
            display("Connection failed")
        }
        /// Returned when a function has no data to return currently
        /// but may in the future.
        NoData {}
        /// Returned when the id of the packet isn't one the game knows about
        InvalidPacketId(id: usize) {
            description("invalid packet id")
            display("Invalid packet id: {}", id)
        }
        /// Returned when the 'thing' had to much of something to fit
        /// within the bit limit.
        NotEnoughBits(items: usize, max: usize, thing: &'static str) {
            description("not enough bits")
            display("Not enough bits for {}: had {} items but a limit of {}", thing, items, max)
        }
        /// Returned when an id for a direction doesn't match up with a
        /// valid direction
        InvalidDirectionId {}
        /// Returned when a resource key fails to parse
        InvalidResourceKey {}

        // UDP Networking
        /// Returned when a fragment id doesn't match the one we
        /// were expecting.
        FragmentResponseFail {
            description("failed to respond to a fragment in time")
        }
        /// Returned when a fragment part is recvied which is
        /// outside the expected number of parts
        InvalidFragment {
            description("invalid fragment")
        }
        /// Returned when the max part count changed between parts
        MaxFragmentPartChanged {
            description("max fragment parts changed size")
        }
        /// Returned when the packet has more data than it should
        /// have.
        DataTooLarge {
            description("internal data too large")
        }
        /// Returned when the packet has less data than it should
        /// have.
        DataTooSmall {
            description("internal data too small")
        }
        /// Returned when the checksum of the packet doesn't match
        /// its data.
        ChecksumMismatch(expected: u32, got: u32) {
            description("CRC checksum mismatch")
            display("CRC checksum mismatch: expected: {}, got: {}", expected, got)
        }
        /// Returned when a packet is too large to be sent
        PacketTooLarge {
            description("packet too large")
        }
        /// Returned when an 'ensured' packet couldn't be sent as
        /// all the tracking slots are in use.
        NoPacketSlots {
            description("no free packet slots for the packet")
        }
        /// Returned when a rule wasn't completely parsed
        IncompleteParse(remaining: String) {
            display("Failed to fully parse rule: {:?} was remaining", remaining)
        }
        /// Returned when a rule fails to parse
        ParseFailed(rule: String) {
            display("Failed to parse rule: {:?}", rule)
        }
        /// A static error message
        Static(msg: &'static str) {
            display("{}", msg)
        }
    }
}

impl From<::lua::Error> for Error {
    fn from(p: ::lua::Error) -> Error {
        ErrorKind::Lua(p).into()
    }
}
