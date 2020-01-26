//! A command queue which can be executed by both the server and the client

use crate::prelude::*;
use crate::player::State;
use crate::entity::snapshot::Snapshots;
use crate::common::ScriptData;
use std::sync::Arc;

use std::fmt;
use lua;

/// The number of commands that an implementation should keep
/// in a history queue to allow rolling back.
pub const MAX_QUEUE_HISTORY: usize = 500;

/// General parameters for commands
pub struct CommandParams<'a, E: Invokable + 'static> {
    /// The logger for this command
    pub log: &'a Logger,
    /// The level for the command to execute on
    pub level: &'a mut Level,
    /// The scripting engine to be used
    pub engine: &'a E,
    /// Container of all entities
    pub entities: &'a mut Container,
    /// Snapshot storage, used to reference entities.
    pub snapshots: &'a Snapshots,
    /// The handler for the current mission if any
    pub mission_handler: Option<ResourceKey<'a>>,
}

impl ::lua::LuaUsable for Command {}

/// Tries to execute the passed command, runs the internal block on success.
/// Logs to the console when it fails.
#[macro_export]
macro_rules! try_cmd {
    ($log:expr, $cmd:expr, $body:expr) => (
        match {$cmd} {
            Ok(_) => {
                $body
            },
            Err(err) => {
                error!($log, "Failed to execute command: {:?}", err);
            }
        }
    )
}

macro_rules! invoke_event {
    (@raw $params:expr, do $event:ident for ($player:expr) with ($param:expr) get ($ret:ty, default $def:expr)) => {
        if let Some(handler) = $params.mission_handler.as_ref() {
            let param = $param;
            match $params.engine.with_borrows()
                .borrow(&crate::mission::MissionAllowed)
                .borrow_mut($params.level)
                .borrow_mut($params.entities)
                .invoke_function::<_, $ret>("invoke_module_method", (
                    lua::Ref::new_string($params.engine, handler.module()),
                    lua::Ref::new_string($params.engine, handler.resource()),
                    lua::Ref::new_string($params.engine, "on_event"),
                    lua::Ref::new_string($params.engine, stringify!($event)),
                    i32::from($player.get_uid().0),
                    param,
            )) {
                Ok(val) => Ok(val),
                Err(err) => {
                    error!($params.log, "Failed to run mission `on_event`"; "error" => %err);
                    Err(err)
                }
            }
        } else {
            Ok($def)
        }
    };
    ($params:expr, do $event:ident for ($player:expr) with (()) get ($ret:ty, default $def:expr)) => {
        invoke_event!(@raw $params, do $event for ($player) with (()) get ($ret, default $def))
    };
    ($params:expr, do $event:ident for ($player:expr) with ($param:expr) get ($ret:ty, default $def:expr)) => {
        invoke_event!(@raw $params, do $event for ($player) with (lua::to_table($params.engine, &$param)) get ($ret, default $def))
    };
    ($params:expr, do $event:ident for ($player:expr)) => {
        invoke_event!($params, do $event for ($player) with (()) get ((), default ()))
    };
    ($params:expr, do $event:ident for ($player:expr) with ($param:expr)) => {
        invoke_event!($params, do $event for ($player) with ($expr) get ((), default ()))
    };
    ($params:expr, do $event:ident for ($player:expr) get ($ret:ty, default $def:expr)) => {
        invoke_event!($params, do $event for ($player) with (()) get ($ret, default $def))
    };
}

macro_rules! commands {
    (
        $(
            $(#[$cattr:meta])*
            command $name:ident {
                $struc:item,
                $($imp:item),*
                exec {
                    execute $exname:ident $exfun:item,
                    undo $unname:ident $unfun:item,
                }
                $(sync $sync:expr)*
            }
        )*
    ) => (
        /// A command to execute
        #[allow(clippy::large_enum_variant)]
        #[derive(Debug, Clone, DeltaEncode)]
        #[delta_always]
        pub enum Command {
            $(
                $(#[$cattr])*
                $name($name),
            )*
        }

        impl Command {
            /// Returns whether this command should be sent to other players
            /// or not.
            #[allow(unreachable_code)]
            pub fn should_sync(&self) -> bool {
                match *self {
                    $(
                        Command::$name(_) => {
                            $(return $sync;)*
                            true
                        },
                    )*
                }
            }
        }

        impl Command {
            /// Attempts to execute the command. Returns an error
            /// if the command could not be executed for any reason.
            ///
            /// Commands will change no state (or return it to the state
            /// it was before) when erroring.
            pub fn execute<C, E>(&mut self, handler: &mut C, player: &mut C::Player, mut params: CommandParams<'_, E>) -> UResult<()>
                where C: CommandHandler,
                      E: Invokable,
            {
                match *self {
                    $(
                        Command::$name(ref mut c) => {
                            $exfun;
                            $exname(c, player, &mut params)?;
                            match handler.$exname(c, player, &mut params) {
                                Ok(..) => {},
                                Err(err) => {
                                    $unfun;
                                    // We have to reverse the command here
                                    // due to the fact the first handler already
                                    // executed.
                                    $unname(c, player, &mut params);
                                    return Err(err);
                                }
                            }
                            Ok(())
                        },
                    )*
                }
            }

            /// Reverses the result of executing this command.
            pub fn undo<C, E>(&mut self, handler: &mut C, player: &mut C::Player, mut params: CommandParams<'_, E>)
                where C: CommandHandler,
                      E: Invokable
            {
                match *self {
                    $(
                        Command::$name(ref mut c) => {
                            $unfun;
                            $unname(c, player, &mut params);
                            handler.$unname(c, player, &mut params);
                        },
                    )*
                }
            }
        }

        /// Allows for implementation specific handling of commands if required
        pub trait CommandHandler {
            /// The type of player this handle can handle
            type Player: Player;
            $(
                /// Implementation specific executing of this command
                fn $exname<E>(&mut self, _: &mut $name, _: &mut Self::Player, _params: &mut CommandParams<'_, E>) -> UResult<()>
                    where E: Invokable,
                {
                    Ok(())
                }

                /// Implementation specific undoing of this command
                fn $unname<E>(&mut self, _: &mut $name, _: &mut Self::Player, _params: &mut CommandParams<'_, E>)
                    where E: Invokable,
                {

                }
            )*
        }

        $(
            $(#[$cattr])*
            #[derive(Debug, DeltaEncode)]
            #[delta_always]
            $struc
            impl From<$name> for Command {
                fn from(c: $name) -> Command {
                    Command::$name(c)
                }
            }

            $($imp)*
        )*
    )
}

commands! {
    /// Not a real command.
    ///
    /// Sent by the client when it gets something wrong to
    /// ask the server to start listening to it again
    command Sorry {
        #[derive(Clone)]
        pub struct Sorry {

        },
        impl Sorry {}
        exec {
            execute execute_sorry fn execute_sorry<P, E>(_: &mut Sorry, _: &mut P, _params: &mut CommandParams<'_, E>) -> UResult<()>
                where P: Player,
                      E: Invokable,
            {
                Ok(())
            },
            undo undo_sorry fn undo_sorry<P, E>(_: &mut Sorry, _: &mut P, _params: &mut CommandParams<'_, E>)
                where P: Player,
                      E: Invokable,
            {
            },
        }
    }
    /// Stops the current selection and places the room in
    /// the area
    command PlaceSelection {
        #[derive(Clone)]
        pub struct PlaceSelection {
            /// The resource that defined the room
            pub key: ResourceKey<'static>,
            /// The location the selection is starting at
            pub start: Location,
            /// The location the selection is ending at
            pub end: Location,
        },
        impl PlaceSelection {
            /// Creates a new place selection command that ends the
            /// selection at the passed location
            pub fn new(key: ResourceKey<'_>, start: Location, end: Location) -> PlaceSelection {
                PlaceSelection {
                    key: key.into_owned(),
                    start,
                    end,
                }
            }
        },
        #[derive(Serialize)]
        struct PlaceSelectionParam {
            key: String,
            area: Bound,
        }
        exec {
            execute execute_place_selection fn execute_place_selection<P, E>(cmd: &mut PlaceSelection, player: &mut P, params: &mut CommandParams<'_, E>) -> UResult<()>
                where P: Player,
                      E: Invokable,
            {
                if let State::None = player.get_state() {
                    let ty = params.level.asset_manager.loader_open::<room::Loader>(cmd.key.borrow())?;
                    if !ty.check_requirements(params.level, player.get_uid()) {
                        return Err(ErrorKind::UnmetRoomRequirements.into());
                    }
                    let selection_area = Bound::new(cmd.start, cmd.end);
                    if let Some(active) = params.level.place_room::<P::EntityCreator, _, _>(params.engine, params.entities, player, cmd.key.borrow(), selection_area) {
                        if invoke_event!(params, do place_selection for (player) with (PlaceSelectionParam {
                            key: cmd.key.as_string(),
                            area: selection_area,
                        }) get (Option<bool>, default None)).unwrap_or(None).unwrap_or(true) {
                            player.set_state(State::BuildRoom {
                                active_room: active,
                            });
                            Ok(())
                        } else {
                            params.level.cancel_placement::<P::EntityCreator, _>(params.engine, params.entities, active);
                            bail!("Blocked by script")
                        }
                    } else {
                        return Err(ErrorKind::UnplaceableArea.into())
                    }
                } else {
                    return Err(ErrorKind::InvalidPlayerState.into())
                }
            },
            undo undo_place_selection fn undo_place_selection<P, E>(_: &mut PlaceSelection, player: &mut P, params: &mut CommandParams<'_, E>)
                where P: Player,
                      E: Invokable,
            {
                if let State::BuildRoom{active_room} = player.get_state() {
                    params.level.cancel_placement::<P::EntityCreator, _>(params.engine, params.entities, active_room);
                    player.set_state(State::None);
                }
            },
        }
    }
    /// Finalizes the current room placement
    command FinalizeRoomPlacement {
        #[derive(Default, Clone)]
        pub struct FinalizeRoomPlacement {
            room_id: Option<room::Id>,
        },
        impl FinalizeRoomPlacement {}
        exec {
            execute execute_finalize_room_placement fn execute_finalize_room_placement<P, E>(cmd: &mut FinalizeRoomPlacement, player: &mut P, params: &mut CommandParams<'_, E>) -> UResult<()>
                where P: Player,
                      E: Invokable,
            {
                if let State::BuildRoom{active_room} = player.get_state() {
                    if !invoke_event!(params, do finalize_selection for (player)
                        get (Option<bool>, default None)).unwrap_or(None).unwrap_or(true) {
                        bail!("Blocked by script")
                    }
                    let id = params.level.finalize_placement(active_room);
                    cmd.room_id = Some(id);
                    player.set_state(State::EditRoom{
                        active_room: id
                    });
                    Ok(())
                } else {
                    Err(ErrorKind::NoActiveRoom.into())
                }
            },
            undo undo_finalize_room_placement fn undo_finalize_room_placement<P, E>(cmd: &mut FinalizeRoomPlacement, player: &mut P, params: &mut CommandParams<'_, E>)
                where P: Player,
                      E: Invokable,
            {
                let room_id = assume!(params.log, cmd.room_id);
                let id = params.level.undo_placement::<P, P::EntityCreator>(player, &mut params.entities, room_id);
                player.set_state(State::BuildRoom{active_room: id});
            },
        }
    }
    /// Cancels the current room placement
    command CancelRoomPlacement {
        #[derive(Default)]
        pub struct CancelRoomPlacement {
            #[delta_default]
            old_room: Option<(room::Id, ResourceKey<'static>, Bound)>,
        },
        impl Clone for CancelRoomPlacement {
            fn clone(&self) -> Self {
                CancelRoomPlacement { old_room: None }
            }
        }
        exec {
            execute execute_cancel_room_placement fn execute_cancel_room_placement<P, E>(cmd: &mut CancelRoomPlacement, player: &mut P, params: &mut CommandParams<'_, E>) -> UResult<()>
                where P: Player,
                      E: Invokable,
            {
                if let State::BuildRoom{active_room} = player.get_state() {
                    player.set_state(State::None);
                    let RoomPlacement{id, key, area, ..} = params.level.cancel_placement::<P::EntityCreator, _>(params.engine, params.entities, active_room);
                    cmd.old_room = Some((id, key, area));
                    Ok(())
                } else {
                    Err(ErrorKind::NoActiveRoom.into())
                }
            },
            undo undo_cancel_room_placement fn undo_cancel_room_placement<P, E>(cmd: &mut CancelRoomPlacement, player: &mut P, params: &mut CommandParams<'_, E>)
                where P: Player,
                      E: Invokable,
            {
                let old = assume!(params.log, cmd.old_room.take());
                if let Some(id) = params.level.place_room_id::<P::EntityCreator, _>(params.engine, params.entities, old.0, player.get_uid(), old.1, old.2) {
                    player.set_state(State::BuildRoom{active_room: id});
                }
            },
        }
    }
    /// Resizes the active room
    command ResizeRoom {
        #[derive(Clone)]
        pub struct ResizeRoom {
            new_bound: Bound,
            #[delta_default]
            old_bound: Option<Bound>,
        },
        impl ResizeRoom {
            /// Creates a ResizeRoom request that will change the active room's
            /// size to the passed bounds.
            pub fn new(bound: Bound) -> ResizeRoom {
                ResizeRoom {
                    new_bound: bound,
                    old_bound: None,
                }
            }
        }
        exec {
            execute execute_resize_room fn execute_resize_room<P, E>(cmd: &mut ResizeRoom, player: &mut P, params: &mut CommandParams<'_, E>) -> UResult<()>
                where P: Player,
                      E: Invokable,
            {
                if let State::BuildRoom{active_room} = player.get_state() {
                    let old_bound = params.level.get_room_info(active_room).area;
                    cmd.old_bound = Some(old_bound);
                    if params.level.resize_room_id::<P::EntityCreator, _>(params.engine, params.entities, player.get_uid(), active_room, cmd.new_bound) {
                        Ok(())
                    } else {
                        Err("Invalid resize".into())
                    }
                } else {
                    Err(ErrorKind::NoActiveRoom.into())
                }
            },
            undo undo_resize_room fn undo_resize_room<P, E>(cmd: &mut ResizeRoom, player: &mut P, params: &mut CommandParams<'_, E>)
                where P: Player,
                      E: Invokable,
            {
                if let Some(old) = cmd.old_bound {
                    if let State::BuildRoom{active_room} = player.get_state() {
                        assert!(params.level.resize_room_id::<P::EntityCreator, _>(params.engine, params.entities, player.get_uid(), active_room, old));
                    }
                }
            },
        }
    }
    /// Finalizes the current room
    command FinalizeRoom {
        #[derive(Default, Clone)]
        pub struct FinalizeRoom {
            #[delta_default]
            old_active_room: Option<room::Id>,
        },
        impl FinalizeRoom {},
        #[derive(Serialize)]
        struct FinalizeRoomParam {
            key: String,
            area: Bound,
        }
        exec {
            execute execute_finalize_room fn execute_finalize_room<P, E>(cmd: &mut FinalizeRoom, player: &mut P, params: &mut CommandParams<'_, E>) -> UResult<()>
                where P: Player,
                      E: Invokable,
            {
                if let State::EditRoom{active_room} = player.get_state() {
                    let (room_info, old_cost) = {
                        let room = params.level.get_room_info(active_room);
                        if !room.state.is_building() || room.limited_editing {
                            return Err(ErrorKind::InvalidRoomState.into());
                        }
                        (
                            params.level.asset_manager.loader_open::<room::Loader>(room.key.borrow())?,
                            room.placement_cost
                        )
                    };

                    let cost = if player.can_charge() {
                        let money = player.get_money();
                        let cost = room_info.cost_for_room(params.level, active_room) - old_cost;
                        let cost = if cost < UniDollar(0) {
                            UniDollar(0)
                        } else {
                            cost
                        };
                        if money < cost && cost != UniDollar(0) {
                            return Err(ErrorKind::NotEnoughMoney.into());
                        }

                        if !room_info.is_valid_placement(params.level, active_room) {
                            return Err(ErrorKind::UnmetRoomRequirements.into());
                        }

                        -cost
                    } else { UniDollar(0) };

                    if !invoke_event!(params, do finalize_room for (player) with ({
                        let room = params.level.get_room_info(active_room);
                        FinalizeRoomParam {
                            key: room.key.as_string(),
                            area: room.area,
                        }
                    })
                        get (Option<bool>, default None)).unwrap_or(None).unwrap_or(true) {
                        bail!("Blocked by script")
                    }

                    params.level.finalize_room::<P::EntityCreator, _>(params.engine, params.entities, active_room)?;
                    player.change_money(cost);
                    player.set_state(State::None);

                    cmd.old_active_room = Some(active_room);
                    Ok(())
                } else {
                    Err(ErrorKind::NoActiveRoom.into())
                }
            },
            undo undo_finalize_room fn undo_finalize_room<P, E>(cmd: &mut FinalizeRoom, player: &mut P, params: &mut CommandParams<'_, E>)
                where P: Player,
                      E: Invokable,
            {
                let old_id = assume!(params.log, cmd.old_active_room);
                params.level.undo_room_build::<P::EntityCreator, _>(params.engine, params.entities, old_id);
                player.set_state(State::EditRoom{
                    active_room: old_id
                });
            },
        }
    }
    /// Cancels the current room reverting to planning
    command CancelRoom {
        #[derive(Default, Clone)]
        pub struct CancelRoom {
        },
        impl CancelRoom {}
        exec {
            execute execute_cancel_room fn execute_cancel_room<P, E>(_: &mut CancelRoom, player: &mut P, params: &mut CommandParams<'_, E>) -> UResult<()>
                where P: Player,
                      E: Invokable,
            {
                if let State::EditRoom{active_room} = player.get_state() {
                    {
                        let room = params.level.get_room_info(active_room);
                        if !room.state.is_building() || room.limited_editing {
                            return Err(ErrorKind::InvalidRoomState.into());
                        }
                    }
                    let id = params.level.undo_placement::<P, P::EntityCreator>(player, &mut params.entities, active_room);
                    player.set_state(State::BuildRoom {
                        active_room: id
                    });
                    Ok(())
                } else {
                    Err(ErrorKind::NoActiveRoom.into())
                }
            },
            undo undo_cancel_room fn undo_cancel_room<P, E>(_: &mut CancelRoom, player: &mut P, params: &mut CommandParams<'_, E>)
                where P: Player,
                      E: Invokable,
            {
                if let State::BuildRoom{active_room} = player.get_state() {
                    let id = params.level.finalize_placement(active_room);
                    player.set_state(State::EditRoom {
                        active_room: id,
                    });
                } else {
                    unreachable!()
                }
            },
        }
    }
    /// Edits a placed room
    command EditRoom {
        #[derive(Clone)]
        pub struct EditRoom {
            /// The target room
            pub room_id: room::Id,
        },
        impl EditRoom {
            /// Creates an edit room command
            pub fn new(room: room::Id) -> EditRoom {
                EditRoom {
                    room_id: room,
                }
            }
        }
        exec {
            execute execute_edit_room fn execute_edit_room<P, E>(cmd: &mut EditRoom, player: &mut P, params: &mut CommandParams<'_, E>) -> UResult<()>
                where P: Player,
                      E: Invokable,
            {
                if let State::None = player.get_state() {
                    {
                        let room = params.level.try_room_info(cmd.room_id)
                            .ok_or_else(|| ErrorKind::InvalidRoomState)?;
                        let rinfo = params.level.asset_manager.loader_open::<room::Loader>(room.key.borrow())?;
                        if !room.state.is_done() || room.owner != player.get_uid() || !rinfo.allow_edit {
                            return Err(ErrorKind::InvalidRoomState.into());
                        }
                    }
                    if params.level.is_blocked_edit(cmd.room_id).is_err() {
                        return Err(ErrorKind::RoomNoFullOwnership.into());
                    }
                    params.level.undo_room_build::<P::EntityCreator, _>(params.engine, params.entities, cmd.room_id);
                    player.set_state(State::EditRoom{
                        active_room: cmd.room_id
                    });
                    Ok(())
                } else {
                    Err("Already have an active room".into())
                }
            },
            undo undo_edit_room fn undo_edit_room<P, E>(_cmd: &mut EditRoom, player: &mut P, params: &mut CommandParams<'_, E>)
                where P: Player,
                      E: Invokable,
            {
                if let State::EditRoom{active_room} = player.get_state() {
                    player.set_state(State::None);
                    assume!(params.log, params.level.finalize_room::<P::EntityCreator, _>(params.engine, params.entities, active_room));
                }
            },
        }
    }
    /// Edits a placed room in limited mode
    command EditRoomLimited {
        #[derive(Clone)]
        pub struct EditRoomLimited {
            room_id: room::Id,
        },
        impl EditRoomLimited {
            /// Creates an edit room command
            pub fn new(room: room::Id) -> EditRoomLimited {
                EditRoomLimited {
                    room_id: room,
                }
            }
        }
        exec {
            execute execute_edit_room_limited fn execute_edit_room_limited<P, E>(cmd: &mut EditRoomLimited, player: &mut P, params: &mut CommandParams<'_, E>) -> UResult<()>
                where P: Player,
                      E: Invokable,
            {
                if let State::None = player.get_state() {
                    {
                        let room = params.level.try_room_info(cmd.room_id)
                            .ok_or_else(|| ErrorKind::InvalidRoomState)?;
                        let rinfo = params.level.asset_manager.loader_open::<room::Loader>(room.key.borrow())?;
                        if !room.state.is_done() || room.owner != player.get_uid()
                                || !rinfo.allow_edit
                                || !rinfo.allow_limited_edit {
                            return Err(ErrorKind::InvalidRoomState.into());
                        }
                    }
                    {
                        let mut room = params.level.get_room_info_mut(cmd.room_id);
                        room.limited_editing = true;
                    }
                    player.set_state(State::EditRoom{
                        active_room: cmd.room_id
                    });
                    Ok(())
                } else {
                    Err("Already have an active room".into())
                }
            },
            undo undo_edit_room_limited fn undo_edit_room_limited<P, E>(_cmd: &mut EditRoomLimited, player: &mut P, params: &mut CommandParams<'_, E>)
                where P: Player,
                      E: Invokable,
            {
                if let State::EditRoom{active_room} = player.get_state() {
                    player.set_state(State::None);
                    {
                        let mut room = params.level.get_room_info_mut(active_room);
                        room.limited_editing = false;
                    }
                    assume!(params.log, params.level.finalize_room::<P::EntityCreator, _>(params.engine, params.entities, active_room));
                }
            },
        }
    }
    /// Finalizes the current room
    command FinalizeLimitedEdit {
        #[derive(Default, Clone)]
        pub struct FinalizeLimitedEdit {
            #[delta_default]
            old_active_room: Option<room::Id>,
        },
        impl FinalizeLimitedEdit {}
        exec {
            execute execute_finalize_limited_edit fn execute_finalize_limited_edit<P, E>(cmd: &mut FinalizeLimitedEdit, player: &mut P, params: &mut CommandParams<'_, E>) -> UResult<()>
                where P: Player,
                      E: Invokable,
            {
                if let State::EditRoom{active_room} = player.get_state() {
                    let (room_info, old_cost) = {
                        let room = params.level.try_room_info(active_room)
                            .ok_or_else(|| ErrorKind::InvalidRoomState)?;
                        if !room.limited_editing {
                            return Err(ErrorKind::InvalidRoomState.into());
                        }
                        (
                            params.level.asset_manager.loader_open::<room::Loader>(room.key.borrow())?,
                            room.placement_cost
                        )
                    };

                    let room_cost = room_info.cost_for_room(params.level, active_room);
                    let cost = if player.can_charge() {
                        let money = player.get_money();
                        let cost = room_cost - old_cost;
                        let cost = if cost < UniDollar(0) {
                            UniDollar(0)
                        } else {
                            cost
                        };
                        if money < cost && cost != UniDollar(0) {
                            return Err(ErrorKind::NotEnoughMoney.into());
                        }
                        -cost
                    } else { UniDollar(0) };
                    {
                        let mut room = params.level.get_room_info_mut(active_room);
                        room.limited_editing = false;
                        room.placement_cost = room_cost;
                        room.needs_update = true;
                    }
                    player.set_state(State::None);
                    player.change_money(cost);
                    cmd.old_active_room = Some(active_room);
                    Ok(())
                } else {
                    Err(ErrorKind::NoActiveRoom.into())
                }
            },
            undo undo_finalize_limited_edit fn undo_finalize_limited_edit<P, E>(cmd: &mut FinalizeLimitedEdit, player: &mut P, params: &mut CommandParams<'_, E>)
                where P: Player,
                      E: Invokable,
            {
                let old_id = assume!(params.log, cmd.old_active_room);
                {
                    let mut room = params.level.get_room_info_mut(old_id);
                    room.limited_editing = true;
                }
                player.set_state(State::EditRoom{
                    active_room: old_id
                });
            },
        }
    }
    /// Places the named object at the location in the active room
    command PlaceObject {
        pub struct PlaceObject {
            key: ResourceKey<'static>,
            location: InWorldPosition,
            rotation: i16,
            #[delta_default]
            rev: Option<usize>,
        },
        impl Clone for PlaceObject {
            fn clone(&self) -> PlaceObject {
                PlaceObject {
                    key: self.key.clone(),
                    location: self.location,
                    rotation: self.rotation,
                    rev: None,
                }
            }
        },
        impl PlaceObject {
            /// Creates a new place object command
            pub fn new(key: ResourceKey<'_>, location: (f32, f32), rotation: i16) -> PlaceObject {
                PlaceObject {
                    key: key.into_owned(),
                    location: InWorldPosition {
                        x: location.0,
                        y: location.1,
                    },
                    rotation,
                    rev: None,
                }
            }
        }
        exec {
            execute execute_place_object fn execute_place_object<P, E>(cmd: &mut PlaceObject, player: &mut P, params: &mut CommandParams<'_, E>) -> UResult<()>
                where P: Player,
                      E: Invokable,
            {
                if let State::EditRoom{active_room} = player.get_state() {
                    {
                        let room = params.level.get_room_info(active_room);
                        if !room.state.is_building() && !room.limited_editing {
                            return Err(ErrorKind::InvalidRoomState.into());
                        }
                    }
                    params.level.cancel_object_placement::<P::EntityCreator>(active_room, params.entities);
                    params.level.begin_object_placement::<_, P::EntityCreator>(active_room, params.engine, params.entities, cmd.key.borrow(), None)?;
                    params.level.move_active_object::<_, P::EntityCreator>(
                        active_room, params.engine,
                        params.entities,
                        (cmd.location.x, cmd.location.y),
                        None,
                        cmd.rotation
                    )?;
                    let rev = params.level.finalize_object_placement::<_, P::EntityCreator>(active_room, params.engine, params.entities, None, cmd.rotation)?;
                    cmd.rev = Some(rev);
                    Ok(())
                } else {
                    Err(ErrorKind::NoActiveRoom.into())
                }
            },
            undo undo_place_object fn undo_place_object<P, E>(cmd: &mut PlaceObject, player: &mut P, params: &mut CommandParams<'_, E>)
                where P: Player,
                      E: Invokable,
            {
                if let State::EditRoom{active_room} = player.get_state() {
                    assume!(params.log,
                        params.level.remove_object::<P::EntityCreator>(params.entities, active_room, assume!(params.log, cmd.rev))
                    );
                }
            },
        }
    }
    /// Places the active entity to the target location
    command RemoveObject {
        pub struct RemoveObject {
            id: u32,
            #[delta_default]
            rev: Option<ObjectPlacement>,
        },
        impl Clone for RemoveObject {
            fn clone(&self) -> RemoveObject {
                RemoveObject {
                    id: self.id,
                    rev: None,
                }
            }
        },
        impl RemoveObject {
            /// Creates a new remove object command
            pub fn new(id: usize) -> RemoveObject {
                RemoveObject {
                    id: id as u32,
                    rev: None,
                }
            }
        }
        exec {
            execute execute_remove_object fn execute_remove_object<P, E>(cmd: &mut RemoveObject, player: &mut P, params: &mut CommandParams<'_, E>) -> UResult<()>
                where P: Player,
                      E: Invokable,
            {
                if let State::EditRoom{active_room: room_id} = player.get_state() {
                    {
                        let room = params.level.get_room_info(room_id);
                        if !room.state.is_building() && !room.limited_editing {
                            return Err(ErrorKind::InvalidRoomState.into());
                        }
                    }
                    let rev = params.level.remove_object::<P::EntityCreator>(&mut params.entities, room_id, cmd.id as usize)?;
                    cmd.rev = Some(rev);
                    Ok(())
                } else {
                    bail!("No active room");
                }
            },
            undo undo_remove_object fn undo_remove_object<P, E>(cmd: &mut RemoveObject, player: &mut P, params: &mut CommandParams<'_, E>)
                where P: Player,
                      E: Invokable,
            {
                if let State::EditRoom{active_room: room_id} = player.get_state() {
                    params.level.replace_object::<P::EntityCreator>(&mut params.entities, room_id, cmd.id as usize, assume!(params.log, cmd.rev.take()));
                }
            },
        }
    }
    /// Places the named entity at the location
    command PlaceStaff {
        pub struct PlaceStaff {
            /// The type of entity to hire
            pub key: ResourceKey<'static>,
            /// The unique id of the staff member
            pub unique_id: u32,
            /// The location to spawn at
            pub(crate) location: InWorldPosition,
            /// The reference to the spawned entity to reverse if this fails
            #[delta_default]
            pub rev: Option<Entity>,
        },
        impl Clone for PlaceStaff {
            fn clone(&self) -> PlaceStaff {
                PlaceStaff {
                    key: self.key.clone(),
                    unique_id: self.unique_id,
                    location: self.location,
                    rev: None,
                }
            }
        },
        impl PlaceStaff {
            /// Creates a new place staff command
            pub fn new(key: ResourceKey<'_>, id: u32, location: (f32, f32)) -> PlaceStaff {
                PlaceStaff {
                    key: key.into_owned(),
                    unique_id: id,
                    location: InWorldPosition {
                        x: location.0,
                        y: location.1,
                    },
                    rev: None,
                }
            }
        }
        exec {
            execute execute_place_staff fn execute_place_staff<P, E>(_cmd: &mut PlaceStaff, player: &mut P, _params: &mut CommandParams<'_, E>) -> UResult<()>
                where P: Player,
                      E: Invokable,
            {
                if let State::None = player.get_state() {
                    player.set_state(State::EditEntity {
                        entity: None,
                    });
                    Ok(())
                } else {
                    bail!("incorrect state")
                }
            },
            undo undo_place_staff fn undo_place_staff<P, E>(cmd: &mut PlaceStaff, player: &mut P, params: &mut CommandParams<'_, E>)
                where P: Player,
                      E: Invokable,
            {
                player.set_state(State::None);
                if let Some(e) = cmd.rev {
                    params.entities.remove_entity(e);
                }
            },
        }
        sync false
    }
    /// Moves the active entity to the target location
    command MoveStaff {
        pub struct MoveStaff {
            location: InWorldPosition,
            #[delta_default]
            rev: Option<(f32, f32)>,
        },
        impl Clone for MoveStaff {
            fn clone(&self) -> MoveStaff {
                MoveStaff {
                    location: self.location,
                    rev: None,
                }
            }
        },
        impl MoveStaff {
            /// Creates a new place object command
            pub fn new(location: (f32, f32)) -> MoveStaff {
                MoveStaff {
                    location: InWorldPosition {
                        x: location.0,
                        y: location.1,
                    },
                    rev: None,
                }
            }
        }
        exec {
            execute execute_move_staff fn execute_move_staff<P, E>(cmd: &mut MoveStaff, player: &mut P, params: &mut CommandParams<'_, E>) -> UResult<()>
                where P: Player,
                      E: Invokable,
            {
                if let State::EditEntity{entity:Some(e)} = player.get_state() {
                    {
                        let pos = assume!(params.log, params.entities.get_component_mut::<Position>(e));
                        cmd.rev = Some((pos.x, pos.z));
                        pos.x = cmd.location.x;
                        pos.z = cmd.location.y;
                    }
                    Ok(())
                } else {
                    // Allow this for remote players which wont have this entity
                    Ok(())
                }
            },
            undo undo_move_staff fn undo_move_staff<P, E>(cmd: &mut MoveStaff, player: &mut P, params: &mut CommandParams<'_, E>)
                where P: Player,
                      E: Invokable,
            {
                if let (State::EditEntity{entity: Some(e)}, Some(target)) = (player.get_state(), cmd.rev) {
                    {
                        let pos = assume!(params.log, params.entities.get_component_mut::<Position>(e));
                        pos.x = target.0;
                        pos.z = target.1;
                    }
                }
            },
        }
        sync false
    }
    /// Places the active entity to the target location
    command FinalizeStaffPlace {
        pub struct FinalizeStaffPlace {
            #[doc(hidden)]
            pub(crate) location: InWorldPosition,
            #[doc(hidden)]
            #[delta_default]
            pub rev: Option<(Entity, f32, f32)>,
        },
        impl Clone for FinalizeStaffPlace {
            fn clone(&self) -> FinalizeStaffPlace {
                FinalizeStaffPlace {
                    location: self.location,
                    rev: None,
                }
            }
        },
        impl FinalizeStaffPlace {
            /// Creates a new place staff command
            pub fn new(location: (f32, f32)) -> FinalizeStaffPlace {
                FinalizeStaffPlace {
                    location: InWorldPosition {
                        x: location.0,
                        y: location.1,
                    },
                    rev: None,
                }
            }
        },
        #[derive(Serialize)]
        struct StaffPlaceParam {
            key: String,
        }
        exec {
            execute execute_finalize_staff_place fn execute_finalize_staff_place<P, E>(cmd: &mut FinalizeStaffPlace, player: &mut P, params: &mut CommandParams<'_, E>) -> UResult<()>
                where P: Player,
                      E: Invokable,
            {
                if let State::EditEntity{entity:Some(e)} = player.get_state() {
                    if !can_visit(
                        &*params.level.tiles.borrow(), &*params.level.rooms.borrow(),
                        (cmd.location.x * 4.0) as usize, (cmd.location.y * 4.0) as usize
                    ) {
                        return Err(ErrorKind::UnplaceableArea.into());
                    }
                    if !invoke_event!(params, do finalize_staff_place for (player) with ({
                        let l = assume!(params.log, params.entities.get_component_mut::<Living>(e));
                        StaffPlaceParam {
                            key: l.key.as_string(),
                        }
                    })
                        get (Option<bool>, default None)).unwrap_or(None).unwrap_or(true) {
                        bail!("Blocked by script")
                    }
                    {
                        let pos = assume!(params.log, params.entities.get_component_mut::<Position>(e));
                        cmd.rev = Some((e, pos.x, pos.z));
                    }
                    params.entities.remove_component::<Frozen>(e);
                    player.set_state(State::None);
                    Ok(())
                } else {
                    // Allow this for remote players which wont have this entity
                    Ok(())
                }
            },
            undo undo_finalize_staff_place fn undo_finalize_staff_place<P, E>(cmd: &mut FinalizeStaffPlace, player: &mut P, params: &mut CommandParams<'_, E>)
                where P: Player,
                      E: Invokable,
            {
                if let (State::None, Some(target)) = (player.get_state(), cmd.rev) {
                    params.entities.add_component(target.0, Frozen);
                }
            },
        }
        sync false
    }
    /// Picks up the staff member for moving
    command StartMoveStaff {
        pub struct StartMoveStaff {
            /// The target entity's network id
            pub target: u32,
            /// The info to reverse this command
            #[delta_default]
            pub rev: Option<Entity>,
        },
        impl Clone for StartMoveStaff {
            fn clone(&self) -> StartMoveStaff {
                StartMoveStaff {
                    target: self.target,
                    rev: None,
                }
            }
        },
        impl StartMoveStaff {
            /// Creates a start move staff command for the entity with
            /// the given network id
            pub fn new(target: u32) -> StartMoveStaff {
                StartMoveStaff {
                    target,
                    rev: None,
                }
            }
        }
        exec {
            execute execute_start_move_staff fn execute_start_move_staff<P, E>(cmd: &mut StartMoveStaff, player: &mut P, params: &mut CommandParams<'_, E>) -> UResult<()>
                where P: Player,
                      E: Invokable,
            {
                if let State::None = player.get_state() {
                    if let Some(entity) = params.snapshots.get_entity_by_id(cmd.target) {
                        let player_id = player.get_uid();
                        if !params.entities.get_component::<Owned>(entity).map_or(false, |v| v.player_id == player_id) {
                            bail!("Entity not owned by player");
                        }
                        if params.entities.get_component::<Paid>(entity).is_none() {
                            bail!("Entity not controlled by player");
                        }
                        player.set_state(player::State::EditEntity {entity: None});
                        cmd.rev = Some(entity);
                        Ok(())
                    } else {
                        bail!("Missing entity")
                    }
                } else {
                    bail!("Incorrect state")
                }
            },
            undo undo_start_move_staff fn undo_start_move_staff<P, E>(_cmd: &mut StartMoveStaff, player: &mut P, _params: &mut CommandParams<'_, E>)
                where P: Player,
                      E: Invokable,
            {
                player.set_state(player::State::None);
            },
        }
        sync false
    }
    /// Places the named entity at the location
    command CancelPlaceStaff {
        pub struct CancelPlaceStaff {
            #[delta_default]
            pub(crate) rev: Option<OldEntityState>,
        },
        pub(crate) struct OldEntityState {
            living: Living,
            position: Position,
            // FIXME: stats: Stats,
            tints: Tints,
            pub(crate) existed: bool,
        },
        impl fmt::Debug for OldEntityState {
            fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
                Ok(())
            }
        },
        impl Clone for CancelPlaceStaff {
            fn clone(&self) -> Self { CancelPlaceStaff { rev: None } }
        },
        impl CancelPlaceStaff {
            /// Creates a new place staff command
            pub fn new() -> CancelPlaceStaff {
                CancelPlaceStaff {
                    rev: None,
                }
            }
        }
        exec {
            execute execute_cancel_place_staff fn execute_cancel_place_staff<P, E>(cmd: &mut CancelPlaceStaff, player: &mut P, params: &mut CommandParams<'_, E>) -> UResult<()>
                where P: Player,
                      E: Invokable,
            {
                if let State::EditEntity{entity: Some(entity)} = player.get_state() {
                    {
                        cmd.rev = Some(OldEntityState {
                            living: assume!(params.log, params.entities.remove_component::<Living>(entity)),
                            position: assume!(params.log, params.entities.remove_component::<Position>(entity)),
                            // FIXME: stats: assume!(params.log, params.entities.remove_component::<Stats>(entity)),
                            tints: assume!(params.log, params.entities.remove_component::<Tints>(entity)),
                            existed: params.entities.get_component::<DoesntExist>(entity).is_none(),
                        });
                    }
                    params.entities.remove_entity(entity);
                    player.set_state(State::None);
                    Ok(())
                } else {
                    bail!("incorrect state")
                }
            },
            undo undo_cancel_place_staff fn undo_cancel_place_staff<P, E>(cmd: &mut CancelPlaceStaff, player: &mut P, params: &mut CommandParams<'_, E>)
                where P: Player,
                      E: Invokable,
            {
                if let Some(cmd) = cmd.rev.take() {
                    let ty = assume!(params.log, params.level.asset_manager.loader_open::<Loader<P::EntityInfo>>(cmd.living.key.borrow()));
                    let e = ty.create_entity(params.entities, cmd.living.variant, Some(cmd.living.name));
                    params.entities.add_component(e, cmd.position);
                    // FIXME: params.entities.add_component(e, cmd.stats);
                    params.entities.add_component(e, cmd.tints);
                    params.entities.add_component(e, Frozen);
                    params.entities.add_component(e, Owned {
                        player_id: player.get_uid(),
                    });
                    if !cmd.existed {
                        params.entities.add_component(e, DoesntExist);
                    }
                    player.set_state(State::EditEntity {
                        entity: Some(e),
                    });
                }
            },
        }
        sync false
    }
    /// Fires the staff member
    command FireStaff {
        pub struct FireStaff {
            /// The target entity's network id
            pub target: u32,
        },
        impl Clone for FireStaff {
            fn clone(&self) -> FireStaff {
                FireStaff {
                    target: self.target,
                }
            }
        },
        impl FireStaff {
            /// Creates a fire staff command for the entity with
            /// the given network id
            pub fn new(target: u32) -> FireStaff {
                FireStaff {
                    target,
                }
            }
        }
        exec {
            execute execute_fire_staff fn execute_fire_staff<P, E>(cmd: &mut FireStaff, player: &mut P, params: &mut CommandParams<'_, E>) -> UResult<()>
                where P: Player,
                      E: Invokable,
            {
                if let State::None = player.get_state() {
                    if let Some(entity) = params.snapshots.get_entity_by_id(cmd.target) {
                        let player_id = player.get_uid();
                        if !params.entities.get_component::<Owned>(entity).map_or(false, |v| v.player_id == player_id) {
                            bail!("Entity not owned by player");
                        }
                        if params.entities.get_component::<Paid>(entity).is_none() {
                            bail!("Entity not controlled by player");
                        }
                        params.entities.remove_component::<Owned>(entity);
                        Ok(())
                    } else {
                        bail!("Missing entity")
                    }
                } else {
                    bail!("Incorrect state")
                }
            },
            undo undo_fire_staff fn undo_fire_staff<P, E>(_cmd: &mut FireStaff, _player: &mut P, _params: &mut CommandParams<'_, E>)
                where P: Player,
                      E: Invokable,
            {
            },
        }
        sync false
    }
    /// Gives the staff member a pay raise or a bonus
    command PayStaff {
        pub struct PayStaff {
            /// The target entity's network id
            pub target: u32,
            bonus: bool,
        },
        impl Clone for PayStaff {
            fn clone(&self) -> PayStaff {
                PayStaff {
                    target: self.target,
                    bonus: self.bonus,
                }
            }
        },
        impl PayStaff {
            /// Creates a pay staff command for the entity with
            /// the given network id
            pub fn new(target: u32, bonus: bool) -> PayStaff {
                PayStaff {
                    target,
                    bonus,
                }
            }
        }
        exec {
            execute execute_pay_staff fn execute_pay_staff<P, E>(cmd: &mut PayStaff, player: &mut P, params: &mut CommandParams<'_, E>) -> UResult<()>
                where P: Player,
                      E: Invokable,
            {
                if let Some(entity) = params.snapshots.get_entity_by_id(cmd.target) {
                    let player_id = player.get_uid();
                    if !params.entities.get_component::<Owned>(entity).map_or(false, |v| v.player_id == player_id) {
                        bail!("Entity not owned by player");
                    }
                    if let Some(paid) = params.entities.get_component_mut::<Paid>(entity) {
                        if cmd.bonus {
                            player.change_money(-paid.cost / 10);
                        } else {
                            paid.cost = paid.wanted_cost;
                        }
                    } else {
                        bail!("Entity not controlled by player");
                    }

                    Ok(())
                } else {
                    bail!("Missing entity")
                }
            },
            undo undo_pay_staff fn undo_pay_staff<P, E>(_cmd: &mut PayStaff, _player: &mut P, _params: &mut CommandParams<'_, E>)
                where P: Player,
                      E: Invokable,
            {
            },
        }
        sync false
    }
    /// Update's the player's config
    command UpdateConfig {
        #[derive(Clone)]
        pub struct UpdateConfig {
            config: player::PlayerConfig,
            rev: Option<player::PlayerConfig>,
        },
        impl UpdateConfig {
            /// Creates a update config command
            pub fn new(config: player::PlayerConfig) -> UpdateConfig {
                UpdateConfig {
                    config,
                    rev: None,
                }
            }
        }
        exec {
            execute execute_update_config fn execute_update_config<P, E>(cmd: &mut UpdateConfig, player: &mut P, _params: &mut CommandParams<'_, E>) -> UResult<()>
                where P: Player,
                      E: Invokable,
            {
                let old = player.get_config();
                player.set_config(cmd.config.clone());
                cmd.rev = Some(old);
                Ok(())
            },
            undo undo_update_config fn undo_update_config<P, E>(cmd: &mut UpdateConfig, player: &mut P, _params: &mut CommandParams<'_, E>)
                where P: Player,
                      E: Invokable,
            {
                if let Some(cfg) = cmd.rev.take() {
                    player.set_config(cfg);
                }
            },
        }
        sync false
    }
    /// Runs the current mission script with the passed data
    command ExecMission {
        #[derive(Clone)]
        pub struct ExecMission {
            data: ScriptData,
        },
        impl ExecMission {
            /// Creates a update config command
            pub fn new(data: ScriptData) -> ExecMission {
                ExecMission {
                    data,
                }
            }
        }
        exec {
            execute execute_exec_mission fn execute_exec_mission<P, E>(cmd: &mut ExecMission, player: &mut P, params: &mut CommandParams<'_, E>) -> UResult<()>
                where P: Player,
                      E: Invokable,
            {
                use lua;
                if player.get_uid() != PlayerId(0) {
                    bail!("Only the server may execute this command");
                }
                let handler = if let Some(mh) = params.mission_handler.as_ref() {
                    mh.borrow()
                } else {
                    bail!("Not in a mission")
                };
                if let Err(err) = params.engine.with_borrows()
                    .borrow(&crate::mission::MissionAllowed)
                    .borrow_mut(params.entities)
                    .invoke_function::<_, ()>("invoke_module_method", (
                        lua::Ref::new_string(params.engine, handler.module()),
                        lua::Ref::new_string(params.engine, handler.resource()),
                        lua::Ref::new_string(params.engine, "on_exec"),
                        lua::Ref::new(params.engine, Arc::clone(&cmd.data.0))
                )) {
                    error!(params.log, "Failed to run mission `on_exec`"; "error" => %err);
                }
                Ok(())
            },
            undo undo_exec_mission fn undo_exec_mission<P, E>(cmd: &mut ExecMission, player: &mut P, params: &mut CommandParams<'_, E>)
                where P: Player,
                      E: Invokable,
            {
                use lua;
                if player.get_uid() != PlayerId(0) {
                    return;
                }
                let handler = if let Some(mh) = params.mission_handler.as_ref() {
                    mh.borrow()
                } else {
                    return;
                };
                if let Err(err) = params.engine.with_borrows()
                    .borrow(&crate::mission::MissionAllowed)
                    .borrow_mut(params.entities)
                    .invoke_function::<_, ()>("invoke_module_method", (
                        lua::Ref::new_string(params.engine, handler.module()),
                        lua::Ref::new_string(params.engine, handler.resource()),
                        lua::Ref::new_string(params.engine, "undo_exec"),
                        lua::Ref::new(params.engine, Arc::clone(&cmd.data.0))
                )) {
                    error!(params.log, "Failed to run mission `on_exec`"; "error" => %err);
                }
            },
        }
    }
    /// Runs a command handler for the given room
    command ExecRoom {
        #[derive(Clone)]
        pub struct ExecRoom {
            room: RoomId,
            method: String,
            data: ScriptData,
        },
        impl ExecRoom {
            /// Creates a update config command
            pub fn new(room: RoomId, method: String, data: ScriptData) -> ExecRoom {
                ExecRoom {
                    room,
                    method,
                    data,
                }
            }
        }
        exec {
            execute execute_exec_room fn execute_exec_room<P, E>(cmd: &mut ExecRoom, player: &mut P, params: &mut CommandParams<'_, E>) -> UResult<()>
                where P: Player,
                      E: Invokable,
            {
                use lua;
                if player.get_uid() != PlayerId(0) {
                    bail!("Only the server may execute this command");
                }
                let room = params.level.get_room_info(cmd.room);
                if room.controller.is_invalid() {
                    bail!("Invalid room");
                }
                let ty = assume!(params.log, params.level.asset_manager.loader_open::<room::Loader>(room.key.borrow()));
                if let Some(controller) = ty.controller.as_ref() {
                    let log = params.log;
                    let engine = params.engine;
                    let level = &params.level;
                    let lua_room = params.entities.with(|
                        _em: EntityManager<'_>,
                        mut props: Write<<<P::EntityCreator as crate::entity::EntityCreator>::ScriptTypes as script::ScriptTypes>::RoomRef>,
                        rc: Read<RoomController>,
                        mut entity_ref: Write<<<P::EntityCreator as crate::entity::EntityCreator>::ScriptTypes as script::ScriptTypes>::EntityRef>,
                        living: Read<Living>,
                        object: Read<Object>,
                    | {
                        <<P::EntityCreator as crate::entity::EntityCreator>::ScriptTypes as script::ScriptTypes>::from_room(
                            log,
                            engine,
                            &*level.rooms.borrow(),
                            &mut props,
                            &rc,
                            &mut entity_ref,
                            &living,
                            &object,
                            cmd.room,
                        )
                    });
                    if let Err(err) = params.engine.with_borrows()
                        .borrow_mut(params.entities)
                        .invoke_function::<_, ()>("invoke_module_method", (
                            lua::Ref::new_string(params.engine, controller.module()),
                            lua::Ref::new_string(params.engine, controller.resource()),
                            lua::Ref::new_string(params.engine, "on_exec"),
                            lua_room,
                            lua::Ref::new_string(params.engine, cmd.method.as_str()),
                            lua::Ref::new(params.engine, Arc::clone(&cmd.data.0))
                    )) {
                        error!(params.log, "Failed to run room `on_exec`"; "error" => %err);
                    }
                }
                Ok(())
            },
            undo undo_exec_room fn undo_exec_room<P, E>(_cmd: &mut ExecRoom, player: &mut P, _params: &mut CommandParams<'_, E>)
                where P: Player,
                      E: Invokable,
            {
                if player.get_uid() != PlayerId(0) {
                    return;
                }
                panic!("Rewiding a server command isn't supported yet");
            },
        }
    }
    /// Runs a command handler for the given idle task
    command ExecIdle {
        #[derive(Clone)]
        pub struct ExecIdle {
            /// The player that the script is for
            pub player: PlayerId,
            /// The idle task id
            pub idx: u32,
            /// The method to call
            pub method: String,
            /// The data to pass
            pub data: ScriptData,
        },
        impl ExecIdle {
            /// Creates a update config command
            pub fn new(player: PlayerId, idx: usize, method: String, data: ScriptData) -> ExecIdle {
                ExecIdle {
                    player,
                    idx: idx as u32,
                    method,
                    data,
                }
            }
        }
        exec {
            execute execute_exec_idle fn execute_exec_idle<P, E>(_cmd: &mut ExecIdle, player: &mut P, _params: &mut CommandParams<'_, E>) -> UResult<()>
                where P: Player,
                      E: Invokable,
            {
                if player.get_uid() != PlayerId(0) {
                    bail!("Only the server may execute this command");
                }
                Ok(())
            },
            undo undo_exec_idle fn undo_exec_idle<P, E>(_cmd: &mut ExecIdle, player: &mut P, _params: &mut CommandParams<'_, E>)
                where P: Player,
                      E: Invokable,
            {
                if player.get_uid() != PlayerId(0) {
                    return;
                }
                panic!("Rewiding a server command isn't supported yet");
            },
        }
    }
    /// Updates/Creates a course
    command UpdateCourse {
        #[derive(Clone)]
        pub struct UpdateCourse {
            pub(crate) course: course::NetworkCourse,
        },
        impl UpdateCourse {
            /// Creates a update config command
            pub fn new(course: course::NetworkCourse) -> UpdateCourse {
                UpdateCourse {
                    course,
                }
            }
        }
        exec {
            execute execute_update_course fn execute_update_course<P, E>(_cmd: &mut UpdateCourse, _player: &mut P, _params: &mut CommandParams<'_, E>) -> UResult<()>
                where P: Player,
                      E: Invokable,
            {
                Ok(())
            },
            undo undo_update_course fn undo_update_course<P, E>(_cmd: &mut UpdateCourse, _player: &mut P, _params: &mut CommandParams<'_, E>)
                where P: Player,
                      E: Invokable,
            {
            },
        }
        sync false
    }
    /// Deprecates a course
    command DeprecateCourse {
        #[derive(Clone)]
        pub struct DeprecateCourse {
            pub(crate) course: course::CourseId,
        },
        impl DeprecateCourse {
            /// Creates a deprecate course command
            pub fn new(course: course::CourseId) -> DeprecateCourse {
                DeprecateCourse {
                    course,
                }
            }
        }
        exec {
            execute execute_deprecate_course fn execute_deprecate_course<P, E>(_cmd: &mut DeprecateCourse, _player: &mut P, _params: &mut CommandParams<'_, E>) -> UResult<()>
                where P: Player,
                      E: Invokable,
            {
                Ok(())
            },
            undo undo_deprecate_course fn undo_deprecate_course<P, E>(_cmd: &mut DeprecateCourse, _player: &mut P, _params: &mut CommandParams<'_, E>)
                where P: Player,
                      E: Invokable,
            {
            },
        }
        sync false
    }
}

#[derive(DeltaEncode, Debug, Clone, Copy, PartialEq)]
pub(crate) struct InWorldPosition {
    pub(crate) x: f32,
    pub(crate) y: f32,
}