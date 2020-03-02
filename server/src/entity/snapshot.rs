//! System for storing, serializing and recreating entity
//! state.

use std::cell::RefCell;
use std::io;
use std::mem;
use std::rc::Rc;
use std::sync::Arc;

use crate::assets;
use crate::common::ScriptData;
use crate::ecs;
use crate::entity::{self, Emote};
use crate::errors;
use crate::level::room;
use crate::network;
use crate::network::packet;
use crate::prelude::*;
use crate::util::*;
use delta_encode::{bitio, DeltaEncodable};
use lua;

// 128 * 50 ms gives a 6.4 second history buffer to work
// with before having to use expensive 'full frames' instead
// of 'delta frames'.
const HISTORY_MAX_SIZE: usize = 128;
/// Marker used for an invalid entity frame.
pub const INVALID_FRAME: u16 = 0x3FFF;

type EntityList = Vec<Option<ecs::Entity>>;

/// Stores a previously obtained snapshots up to a set history
/// limit.
pub struct Snapshots {
    log: Logger,
    frames: Box<[Option<Snapshot>]>,
    player_frames: FNVMap<player::Id, Box<[Option<PlayerSnapshot>]>>,
    current_frame: u16, // 14 bit

    /// A network id to entity map
    pub entity_map: Rc<RefCell<EntityList>>,
    next_entity_id: usize,
}

/// Lua usable wrapper for the entity map
pub struct EntityMap(pub Rc<RefCell<EntityList>>);
impl lua::LuaUsable for EntityMap {}
impl script::LuaTracked for EntityMap {
    const KEY: script::NulledString = nul_str!("entity_map");
    type Storage = EntityMap;
    type Output = Rc<RefCell<EntityList>>;
    fn try_convert(s: &Self::Storage) -> Option<Self::Output> {
        Some(s.0.clone())
    }
}

/// Used by resolve to mark entities with certain
/// states.
///
/// Useful for optimisations
pub trait EntityMarker {
    /// Marks the entity as having the idle choice
    fn mark_idle_choice(&mut self, entity: Entity, player_id: PlayerId, idx: usize);
    /// Marks the entity as having no idle choice
    fn clear_idle_choice(&mut self, entity: Entity, player_id: PlayerId, idx: usize);
}

impl Snapshots {
    /// Creates a snapshot buffer with the default
    /// history size.
    pub fn new(log: &Logger, players: &[player::Id]) -> Snapshots {
        let log = log.new(o!("source" => "snapshots"));
        let mut frames = Vec::with_capacity(HISTORY_MAX_SIZE + 1);
        for _ in 0..HISTORY_MAX_SIZE {
            frames.push(None);
        }
        frames.push(Some(Snapshot {
            frame_id: INVALID_FRAME,
            entities: vec![],
        }));

        let mut player_frames = FNVMap::default();
        for player in players {
            let mut frames = Vec::with_capacity(HISTORY_MAX_SIZE + 1);
            for _ in 0..HISTORY_MAX_SIZE {
                frames.push(None);
            }
            frames.push(Some(PlayerSnapshot {
                frame_id: INVALID_FRAME,
                day_tick: DayTick::default(),
                money: UniDollar(0),
                rating: 0,
                config: player::PlayerConfig::default(),
            }));
            player_frames.insert(*player, frames.into_boxed_slice());
        }

        Snapshots {
            log,
            frames: frames.into_boxed_slice(),
            current_frame: 0,

            entity_map: Rc::new(RefCell::new(vec![])),
            next_entity_id: 0,
            player_frames,
        }
    }

    /// Returns the entity with the given network id (if any)
    pub fn get_entity_by_id(&self, network_id: u32) -> Option<ecs::Entity> {
        self.entity_map
            .borrow()
            .get(network_id as usize)
            .and_then(|v| *v)
    }

    /// Assigns the entity to the given network id
    pub fn assign_network_id(&mut self, entities: &mut Container, e: ecs::Entity, network_id: u32) {
        let mut entity_map = self.entity_map.borrow_mut();
        while network_id as usize >= entity_map.len() {
            entity_map.push(None);
        }
        entity_map[network_id as usize] = Some(e);
        entities.add_component(e, NetworkId(network_id as u32));
    }

    /// Captures the current state of the entities and
    /// stores it as the current frame.
    pub fn capture<'a, I, P: 'a>(
        &mut self,
        entities: &mut ecs::Container,
        day_tick: DayTick,
        players: I,
    ) where
        P: player::Player,
        I: Iterator<Item = (&'a player::Id, &'a P)>,
    {
        let frame_id = self.next_frame_id();

        // Player information update
        for (id, player) in players {
            let player_frames = assume!(self.log, self.player_frames.get_mut(id));

            let snapshot = PlayerSnapshot {
                frame_id,
                day_tick,
                money: player.get_money(),
                rating: player.get_rating(),
                config: player.get_config(),
            };
            player_frames[frame_id as usize % HISTORY_MAX_SIZE] = Some(snapshot);
        }
        {
            let entity_map: &mut EntityList = &mut *self.entity_map.borrow_mut();

            // Update the entity map
            //
            // Check if the entities we know about still exist
            for (id, e) in entity_map.iter_mut().enumerate() {
                if let Some(ee) = e.as_mut().cloned() {
                    if !entities.is_valid(ee) {
                        *e = None;
                        if id < self.next_entity_id {
                            self.next_entity_id = id;
                        }
                    }
                }
            }
        }

        entities.with(
            |em: ecs::EntityManager<'_>,
             living: ecs::Read<super::Living>,
             position: ecs::Read<super::Position>,
             rotation: ecs::Read<super::Rotation>,
             target_rotation: ecs::Read<super::TargetRotation>,
             path: ecs::Read<super::pathfind::PathInfo>,
             selected: ecs::Read<super::SelectedEntity>,
             owned: ecs::Read<super::Owned>,
             state_data: ecs::Read<super::StateData>,
             idle: ecs::Read<Idle>,
             room_owned: ecs::Read<super::RoomOwned>,
             icon_emotes: ecs::Read<super::IconEmote>,
             tints: ecs::Read<super::Tints>,
             mut network_id: ecs::Write<NetworkId>,
             controlled: ecs::Read<Controlled>| {
                let entity_map: &mut EntityList = &mut *self.entity_map.borrow_mut();
                let mut snapshot = Snapshot {
                    frame_id,
                    entities: Vec::with_capacity(entity_map.len()),
                };

                for (e, _) in em.group_mask(&living, |v| v.and_not(&network_id)) {
                    let mut id = self.next_entity_id;
                    while entity_map.get(id).and_then(|v| *v).is_some() {
                        self.next_entity_id += 1;
                        id = self.next_entity_id;
                    }
                    if id >= entity_map.len() {
                        entity_map.push(None);
                    }
                    entity_map[id] = Some(e);
                    network_id.add_component(e, NetworkId(id as u32));
                }

                // Build a snapshot of the state of each entity (at least
                // the parts we care about).
                for e in &*entity_map {
                    if let Some(e) = *e {
                        // Target can either be the current position of the
                        // entity or the end of the path that has been computed.
                        // Clients will handle pathfinding themselves to save
                        // on bandwidth.
                        let target = path.get_component(e).filter(|v| !v.is_empty()).map_or_else(
                            || {
                                let pos = assume!(self.log, position.get_component(e));
                                ETarget {
                                    time: 0.0,
                                    x: pos.x,
                                    z: pos.z,
                                    facing: target_rotation
                                        .get_component(e)
                                        .map(|v| v.rotation)
                                        .or_else(|| rotation.get_component(e).map(|v| v.rotation))
                                        .map(|v| EntityAngle(v.raw())),
                                }
                            },
                            |v| {
                                let (x, z) = v.last();
                                ETarget {
                                    time: v.time,
                                    x,
                                    z,
                                    facing: v
                                        .end_rotation
                                        .or_else(|| {
                                            target_rotation
                                                .get_component(e)
                                                .map(|v| v.rotation)
                                                .or_else(|| {
                                                    rotation.get_component(e).map(|v| v.rotation)
                                                })
                                        })
                                        .map(|v| EntityAngle(v.raw())),
                                }
                            },
                        );

                        let room = room_owned
                            .get_component(e)
                            .map(|v| ERoom { room_id: v.room_id });

                        let data = if let Some(sd) = state_data.get_component(e) {
                            if sd.controller == controlled.get_component(e).and_then(|v| v.by)
                                || sd.controller.is_none()
                            {
                                Some(ScriptData(sd.data.clone()))
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        let idle = if let Some(idle) = idle.get_component(e) {
                            if controlled
                                .get_component(e)
                                .and_then(|v| v.by)
                                .map_or(false, |v| v.is_idle())
                            {
                                idle.current_choice
                                    .map(|v| v as u16)
                                    .map(|v| IdleChoice { idx: v })
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        let emotes = icon_emotes.get_component(e).map_or_else(Vec::new, |v| {
                            v.icons.iter().map(|v| EEmote(v.0, v.1)).collect()
                        });
                        let tints = tints.get_component(e).map_or_else(Vec::new, |v| {
                            v.tints.iter().map(|v| EColor(v.0, v.1, v.2, v.3)).collect()
                        });

                        let selected = selected.get_component(e).map(|v| v.holder);
                        let living = assume!(self.log, living.get_component(e));
                        snapshot.entities.push(Some(EntitySnapshot {
                            info: EntityInfo {
                                key: living.key.clone(),
                                variant: living.variant as u8,
                                name: EName(living.name.0.clone(), living.name.1.clone()),
                            },

                            owner: owned.get_component(e).map(|v| v.player_id),

                            entity: e,
                            target,
                            selected,
                            room,
                            data,
                            idle,
                            emotes,
                            tints,
                        }));
                    } else {
                        snapshot.entities.push(None);
                    }
                }

                // Store the captured frame in the history buffer.
                // This will overwrite any old frame that was stored there.
                self.frames[frame_id as usize % HISTORY_MAX_SIZE] = Some(snapshot);
            },
        );
    }

    fn write_player_state<W, S>(
        log: &Logger,
        player_frames: &FNVMap<player::Id, Box<[Option<PlayerSnapshot>]>>,
        player_info: &NetworkedPlayer<S>,
        current_frame: u16,
        current: &mut bitio::Writer<W>,
    ) where
        W: io::Write,
        S: network::Socket,
    {
        let player = assume!(log, player_frames.get(&assume!(log, player_info.uid)));
        let player_base = player
            .get(player_info.player_state as usize % HISTORY_MAX_SIZE)
            .and_then(|v| v.as_ref())
            .filter(|v| v.frame_id == player_info.player_state);
        let player_cur = assume!(
            log,
            player
                .get(current_frame as usize % HISTORY_MAX_SIZE)
                .and_then(|v| v.as_ref())
        );

        let _ = current.write_unsigned(
            u64::from(player_base.as_ref().map_or(INVALID_FRAME, |v| v.frame_id)),
            14,
        );
        let _ = player_cur.encode(player_base, current);
    }

    /// Creates (and splits) either a delta frame based on the
    /// last ack'd frames per an entity.
    pub(crate) fn create_delta<S>(&self, player: &NetworkedPlayer<S>) -> Vec<packet::EntityFrame>
    where
        S: network::Socket,
    {
        let entity_state = &player.entity_state;
        let entity_map: &EntityList = &*self.entity_map.borrow();
        let mut packets = vec![];
        let mut current = bitio::Writer::new(vec![]);
        // Buffer used for writing entity data before writing
        // to `current`. Cleared and reused for every entity.
        let mut entity_data = bitio::Writer::new(vec![]);

        // Used to force the first valid entity to
        // cause the start of a packet (with headers).
        // Also used to mark as being able to skip over
        // inactive entities
        let mut first = true;
        // The starting point of the current packet's entities
        let mut offset = 0;
        // The number of entities in the current packet
        let mut count = 0;
        // The base frame of the current packet
        let mut base_frame = INVALID_FRAME;
        let current_frame = assume!(
            self.log,
            self.frames[self.current_frame as usize % HISTORY_MAX_SIZE].as_ref()
        );
        for (id, e) in entity_map.iter().enumerate() {
            if first
                && e.is_none()
                && entity_state
                    .entities
                    .get(id)
                    .cloned()
                    .unwrap_or(INVALID_FRAME)
                    == INVALID_FRAME
            {
                offset += 1;
                continue;
            }

            let mut entity_frame = entity_state
                .entities
                .get(id)
                .cloned()
                .unwrap_or(INVALID_FRAME);
            // If the frame in our history buffer isn't the entities last frame
            // then the frame was dropped for a newer frame and the client is
            // lagging behind. We treat this entity as never have been sync'd
            // in the first place and start from fresh.
            if self.frames[entity_frame as usize % HISTORY_MAX_SIZE]
                .as_ref()
                .map_or(true, |v| v.frame_id != entity_frame)
            {
                entity_frame = INVALID_FRAME;
            }

            let same_entity = {
                if entity_frame == INVALID_FRAME {
                    false
                } else {
                    let old_frame = assume!(
                        self.log,
                        self.frames[entity_frame as usize % HISTORY_MAX_SIZE].as_ref()
                    );
                    let oe = old_frame.entities[id].as_ref();
                    let ne = current_frame.entities[id].as_ref();
                    if let (Some(ne), Some(oe)) = (ne, oe) {
                        ne.entity == oe.entity
                    } else {
                        false
                    }
                }
            };

            // Serialize the entity state based on the previous frame.

            let state = match (entity_frame, *e) {
                // Didn't exist before, exists now
                (INVALID_FRAME, Some(_)) => EntityStateFlag::Add,
                // Didn't exist before and still doesn't
                (INVALID_FRAME, None) => EntityStateFlag::Empty,
                // Existed but now removed
                (_, None) => EntityStateFlag::Removed,
                // Reused entity id
                (_, Some(_)) if !same_entity => EntityStateFlag::Add,
                // Existed and still exists
                (_, Some(_)) => EntityStateFlag::Update,
            };
            let _ = entity_data.write_unsigned(u64::from(state.as_u8()), 2);
            match state {
                EntityStateFlag::Add => {
                    let e = assume!(self.log, current_frame.entities[id].as_ref());
                    let _ = e.encode(None, &mut entity_data);
                }
                EntityStateFlag::Update => {
                    let old_frame = assume!(
                        self.log,
                        self.frames[entity_frame as usize % HISTORY_MAX_SIZE].as_ref()
                    );
                    let oe = assume!(self.log, old_frame.entities[id].as_ref());
                    let ne = assume!(self.log, current_frame.entities[id].as_ref());
                    let _ = ne.encode(Some(oe), &mut entity_data);
                }
                // No need to send any state for these as they don't/no longer
                // exist.
                EntityStateFlag::Removed | EntityStateFlag::Empty => {}
            }

            // - The first entity needs to set the header, no avoiding that
            // - Also split if the entity has a different base from the last entity.
            //   This can happen due to the size limit below.
            // - The packet is limited to 1000 bytes in size. If writing this
            //   entity would set us over this limit then when split.
            //   The system should be able to handle this.
            // - The number of entities would overflow the u8 count
            if first
                || entity_frame != base_frame
                || count >= 255
                || entity_data.bit_len() + current.bit_len() > 1000 * 8
            {
                // Finish and write out the previous packet (if there was
                // one).
                if !first {
                    let mut data = assume!(
                        self.log,
                        mem::replace(&mut current, bitio::Writer::new(vec![])).finish()
                    );
                    // Set the entity count in the space we
                    // reserved.
                    data[6] = count as u8;
                    packets.push(packet::EntityFrame {
                        data: packet::Raw(data),
                    });
                    offset += count;
                    count = 0;
                }

                // Update the base frame. Follows the same
                // rules as `entity_frame` above.
                base_frame = entity_state
                    .entities
                    .get(id)
                    .cloned()
                    .unwrap_or(INVALID_FRAME);
                if self.frames[base_frame as usize % HISTORY_MAX_SIZE]
                    .as_ref()
                    .map_or(true, |v| v.frame_id != base_frame)
                {
                    base_frame = INVALID_FRAME;
                }

                // Write the header
                let _ = current.write_unsigned(u64::from(self.current_frame), 14);
                let _ = current.write_unsigned(u64::from(base_frame), 14);
                let _ = current.write_unsigned(offset as u64, 20);
                let _ = current.write_unsigned(0, 8); // Entity count placeholder

                // Player state is special as the player isn't
                // an entity (and even if it was it doesn't require
                // the same information as other entities).
                // We packet this into the first entity state
                // packet instead of requiring another packet
                // just for this
                if first {
                    let _ = current.write_bool(true);
                    Self::write_player_state(
                        &self.log,
                        &self.player_frames,
                        player,
                        self.current_frame,
                        &mut current,
                    );
                } else {
                    let _ = current.write_bool(false);
                }

                first = false;
            }

            let _ = entity_data.copy_into(&mut current);
            entity_data.clear();
            count += 1;
        }

        // Finish and write out the last packet (if there was
        // one).
        if count > 0 || first {
            if first {
                let _ = current.write_unsigned(u64::from(self.current_frame), 14);
                let _ = current.write_unsigned(u64::from(base_frame), 14);
                let _ = current.write_unsigned(offset as u64, 20);
                let _ = current.write_unsigned(0, 8);
                if first {
                    let _ = current.write_bool(true);
                    Self::write_player_state(
                        &self.log,
                        &self.player_frames,
                        player,
                        self.current_frame,
                        &mut current,
                    );
                } else {
                    let _ = current.write_bool(false);
                }
            }
            let mut data = assume!(self.log, current.finish());
            // Set the entity count in the space we
            // reserved.
            data[6] = count as u8;
            packets.push(packet::EntityFrame {
                data: packet::Raw(data),
            });
        }

        packets
    }

    /// Updates the entities using the passed frame (as long as its
    /// new).
    ///
    /// If the frame was usable this returns a packet that can be used
    /// to ack the frame.
    pub fn resolve_delta<CC, P, EM>(
        &mut self,
        level: &mut Level,
        entities: &mut ecs::Container,
        assets: &assets::AssetManager,
        marker: &mut EM,
        day_tick: &mut DayTick,
        entity_state: &mut EntitySnapshotState,
        frame: packet::EntityFrame,
        player: &mut P,
        player_state: &mut u16,
    ) -> errors::Result<(
        Option<packet::EntityAckFrame>,
        Option<packet::PlayerAckFrame>,
    )>
    where
        CC: entity::ComponentCreator,
        P: player::Player,
        EM: EntityMarker,
    {
        use std::cmp::max;
        let entity_map: &mut EntityList = &mut *self.entity_map.borrow_mut();

        let mut r = bitio::Reader::new(io::Cursor::new(frame.data.0));
        // Packet header
        let frame = r.read_unsigned(14)? as u16;
        let base_frame = r.read_unsigned(14)? as u16;
        let entity_offset = r.read_unsigned(20)? as usize;
        let entity_count = r.read_unsigned(8)? as usize;

        // Check for player state
        let player_ack = if r.read_bool()? {
            let player_base = r.read_unsigned(14)? as u16;
            let player_frames = assume!(self.log, self.player_frames.get_mut(&player::Id(0)));

            if player_base == INVALID_FRAME
                || player_frames[player_base as usize % HISTORY_MAX_SIZE]
                    .as_ref()
                    .map_or(false, |v| v.frame_id == player_base)
            {
                let mut player_frame = {
                    PlayerSnapshot::decode(
                        if player_base == INVALID_FRAME {
                            None
                        } else {
                            Some(
                                if let Some(frame) =
                                    player_frames[player_base as usize % HISTORY_MAX_SIZE].as_ref()
                                {
                                    frame
                                } else {
                                    bail!("Missing frame");
                                },
                            )
                        },
                        &mut r,
                    )?
                };
                player_frame.frame_id = frame;

                if *player_state == INVALID_FRAME || !is_previous_frame(*player_state, frame) {
                    *player_state = frame;
                    let cur_money = player.get_money();
                    player.change_money(player_frame.money - cur_money);
                    player.set_rating(player_frame.rating);
                    player.set_config(player_frame.config.clone());
                    *day_tick = player_frame.day_tick;
                }
                player_frames[frame as usize % HISTORY_MAX_SIZE] = Some(player_frame);
            } else {
                bail!("Old frame");
            }
            Some(packet::PlayerAckFrame { frame })
        } else {
            None
        };

        // Make sure we can actually work with this frame.
        if base_frame != INVALID_FRAME
            && self.frames[base_frame as usize % HISTORY_MAX_SIZE]
                .as_ref()
                .map_or(true, |v| v.frame_id != base_frame)
        {
            return Ok((None, player_ack));
        }

        // If there isn't already a frame started, create one
        if self.frames[frame as usize % HISTORY_MAX_SIZE]
            .as_ref()
            .map_or(true, |v| v.frame_id != frame)
        {
            // Don't replace newer frames with an old one. Bail out
            if let Some(frm) = self.frames[frame as usize % HISTORY_MAX_SIZE].as_ref() {
                if !is_previous_frame(frame, frm.frame_id) {
                    bail!("Old frame");
                }
            }
            self.frames[frame as usize % HISTORY_MAX_SIZE] = Some(Snapshot {
                frame_id: frame,
                entities: vec![],
            })
        }
        let f_idx = frame as usize % HISTORY_MAX_SIZE;
        let b_idx = if base_frame == INVALID_FRAME {
            // Special dummy frame that is always empty
            HISTORY_MAX_SIZE
        } else {
            base_frame as usize % HISTORY_MAX_SIZE
        };
        // Get both the base and the current frame.
        // This is needed to because one is borrowed
        // mutably which would prevent any other borrows.
        let (snapshot, base_snap) = if f_idx < b_idx {
            let (low, high) = self.frames.split_at_mut(b_idx);
            (
                assume!(self.log, low[f_idx].as_mut()),
                assume!(self.log, high[0].as_mut()),
            )
        } else {
            let (low, high) = self.frames.split_at_mut(f_idx);
            (
                assume!(self.log, high[0].as_mut()),
                assume!(self.log, low[b_idx].as_mut()),
            )
        };

        // Allocate space for the entities we have.
        let num_entities = entity_map.len();
        entity_state.entities.resize(
            max(num_entities, entity_offset + entity_count),
            INVALID_FRAME,
        );
        snapshot
            .entities
            .resize_with(max(num_entities, entity_offset + entity_count), || None);
        entity_map.resize(max(num_entities, entity_offset + entity_count), None);

        #[allow(clippy::needless_range_loop)]
        for id in entity_offset..entity_offset + entity_count {
            // Parse the entity state information from the packet
            // without applying it to an entity.
            let mut state = EntityStateFlag::from_u8(r.read_unsigned(2)? as u8);

            // Marker to optimize changing an entities target.
            let mut update_target = false;
            match state {
                EntityStateFlag::Add => {
                    let en = EntitySnapshot::decode(None, &mut r)?;
                    update_target = true;
                    snapshot.entities[id] = Some(en);
                }
                EntityStateFlag::Update => {
                    let old = assume!(self.log, base_snap.entities[id].as_ref());
                    let en = EntitySnapshot::decode(Some(old), &mut r)?;
                    update_target = en.target.x != old.target.x
                        || en.target.z != old.target.z
                        || (en.target.time != old.target.time && en.target.time != 0.0);
                    snapshot.entities[id] = Some(en);
                }
                EntityStateFlag::Removed => {
                    snapshot.entities[id] = None;
                }
                EntityStateFlag::Empty => {}
            }

            // Don't use this frame if the entity state we have already is newer
            let cur_frame = entity_state
                .entities
                .get(id)
                .cloned()
                .unwrap_or(INVALID_FRAME);
            if cur_frame != INVALID_FRAME && is_previous_frame(cur_frame, frame) {
                continue;
            }
            entity_state.entities[id] = frame;

            // Add can be sent to replace an existing entity.
            // Clear out the previous one if that is the case.
            if state == EntityStateFlag::Add {
                let e = assume!(self.log, snapshot.entities[id].as_ref());
                if let Some(entity) = entity_map[id] {
                    // If the entity is the same type as the existing one
                    // reuse it as the add is most likely caused by a
                    // lagged frame.
                    if entities
                        .get_component::<super::Living>(entity)
                        .map_or(false, |v| v.key == e.info.key)
                    {
                        update_target = true;
                        state = EntityStateFlag::Update;
                    } else {
                        entities.remove_entity(entity);
                    }
                }
            }

            match state {
                EntityStateFlag::Add => {
                    let e = assume!(self.log, snapshot.entities[id].as_ref());
                    let ty = assets.loader_open::<entity::Loader<CC>>(e.info.key.borrow())?;
                    let new_entity = ty.create_entity(
                        entities,
                        e.info.variant as usize,
                        Some((e.info.name.0.clone(), e.info.name.1.clone())),
                    );
                    entity_map[id] = Some(new_entity);
                    entities.add_component(new_entity, NetworkId(id as u32));
                    {
                        let pos = assume!(
                            self.log,
                            entities.get_component_mut::<entity::Position>(new_entity)
                        );
                        pos.x = e.target.x;
                        pos.z = e.target.z;
                    }
                    if let (Some(rot), Some(face)) = (
                        entities.get_component_mut::<entity::Rotation>(new_entity),
                        e.target.facing,
                    ) {
                        rot.rotation = Angle::new(face.0);
                    }
                    if let Some(owner) = e.owner {
                        entities.add_component(new_entity, entity::Owned { player_id: owner });
                    }
                    if let Some(holder) = e.selected {
                        entities.add_component(new_entity, entity::SelectedEntity { holder })
                    }
                    if let Some(room) = e.room.as_ref() {
                        if let Some(rm) = level.try_room_info(room.room_id) {
                            entities.add_component(
                                new_entity,
                                entity::RoomOwned {
                                    room_id: room.room_id,
                                    should_release_inactive: false,
                                    active: true,
                                },
                            );
                            if let Some(rc) =
                                entities.get_component_mut::<RoomController>(rm.controller)
                            {
                                rc.entities.push(new_entity);
                            }
                            assume!(
                                self.log,
                                entities.get_component_mut::<Controlled>(new_entity)
                            )
                            .by = Some(Controller::Room(room.room_id));
                        } else {
                            assume!(
                                self.log,
                                entities.get_component_mut::<Controlled>(new_entity)
                            )
                            .by = None;
                        }
                    } else if let (Some(owner), Some(idle)) = (e.owner, e.idle.as_ref()) {
                        marker.mark_idle_choice(new_entity, owner, idle.idx as usize);
                        entities.add_component(
                            new_entity,
                            Idle {
                                total_idle_time: 0,
                                current_choice: Some(idle.idx as usize),
                                released: false,
                            },
                        );
                        assume!(
                            self.log,
                            entities.get_component_mut::<Controlled>(new_entity)
                        )
                        .by = Some(Controller::Idle(idle.idx as usize));
                    } else {
                        assume!(
                            self.log,
                            entities.get_component_mut::<Controlled>(new_entity)
                        )
                        .by = None;
                    }
                    if let Some(data) = e.data.as_ref() {
                        entities.add_component(
                            new_entity,
                            StateData {
                                controller: None,
                                data: data.0.clone(),
                            },
                        );
                    } else {
                        entities.remove_component::<StateData>(new_entity);
                    }
                    if !e.emotes.is_empty() {
                        entities.add_component(
                            new_entity,
                            entity::IconEmote::from_existing(e.emotes.iter().map(|v| (v.0, v.1))),
                        )
                    }
                    if !e.tints.is_empty() {
                        entities.add_component(
                            new_entity,
                            entity::Tints {
                                tints: e.tints.iter().map(|v| (v.0, v.1, v.2, v.3)).collect(),
                            },
                        );
                    }
                }
                EntityStateFlag::Update => {
                    let e = assume!(self.log, snapshot.entities[id].as_ref());
                    let entity = if let Some(e) = entity_map[id] {
                        e
                    } else {
                        continue;
                    };
                    if !entities.is_valid(entity) {
                        entity_map[id] = None;
                        continue;
                    }
                    if update_target && e.selected != Some(player.get_uid()) {
                        let keep_pos = {
                            let pos = assume!(
                                self.log,
                                entities.get_component_mut::<entity::Position>(entity)
                            );
                            let dx = pos.x - e.target.x;
                            let dy = pos.z - e.target.z;
                            pos.y = e.selected.map_or(0.0, |_| 0.2);
                            if dx * dx + dy * dy < 0.05 * 0.05 || e.target.time < 0.05 {
                                pos.x = e.target.x;
                                pos.z = e.target.z;
                                true
                            } else {
                                false
                            }
                        };
                        if keep_pos {
                            // Clear existing pathfinding information
                            entities.remove_component::<entity::pathfind::Target>(entity);
                            entities.remove_component::<entity::pathfind::TargetTime>(entity);
                            entities.remove_component::<entity::pathfind::PathInfo>(entity);
                        } else {
                            entities.remove_component::<entity::pathfind::PathInfo>(entity);
                            entities.add_component(
                                entity,
                                entity::pathfind::Target::new(e.target.x, e.target.z),
                            );
                            entities.add_component(
                                entity,
                                entity::pathfind::TargetTime {
                                    time: e.target.time,
                                },
                            );
                        }
                    } else {
                        let pos = assume!(
                            self.log,
                            entities.get_component_mut::<entity::Position>(entity)
                        );
                        pos.y = e.selected.map_or(0.0, |_| 0.2);
                    }
                    let keep_rot = {
                        let rot =
                            assume!(self.log, entities.get_component::<entity::Rotation>(entity));
                        e.target
                            .facing
                            .map_or(true, |f| rot.rotation.difference(f.0).raw() < 0.005)
                    };
                    if !keep_rot {
                        if let Some(face) = e.target.facing {
                            let face = Angle::new(face.0);
                            let manual = if entities
                                .get_component::<entity::pathfind::Target>(entity)
                                .is_some()
                            {
                                entities.add_component(
                                    entity,
                                    entity::pathfind::TargetFacing { rotation: face },
                                );
                                false
                            } else if let Some(path) =
                                entities.get_component_mut::<entity::pathfind::PathInfo>(entity)
                            {
                                path.end_rotation = Some(face);
                                false
                            } else {
                                true
                            };
                            if manual {
                                let set = if let Some(target) =
                                    entities.get_component::<entity::TargetRotation>(entity)
                                {
                                    target.rotation.difference(face).raw() > 0.005
                                } else {
                                    true
                                };
                                if set {
                                    entities.add_component(
                                        entity,
                                        entity::TargetRotation {
                                            rotation: face,
                                            ticks: 8.0,
                                        },
                                    );
                                }
                            }
                        } else {
                            unreachable!()
                        }
                    }

                    // TODO: Optimize
                    if let Some(holder) = e.selected {
                        entities.add_component(entity, entity::SelectedEntity { holder })
                    } else {
                        entities.remove_component::<entity::SelectedEntity>(entity);
                    }
                    if let Some(owner) = e.owner {
                        entities.add_component(entity, entity::Owned { player_id: owner });
                    } else {
                        entities.remove_component::<entity::Owned>(entity);
                    }
                    if let Some(room) = e.room.as_ref() {
                        let (same, prev) =
                            if let Some(ro) = entities.get_component::<RoomOwned>(entity) {
                                if ro.room_id == room.room_id {
                                    (true, Some(ro.room_id))
                                } else {
                                    (false, Some(ro.room_id))
                                }
                            } else {
                                (false, None)
                            };
                        assume!(self.log, entities.get_component_mut::<Controlled>(entity)).by =
                            Some(Controller::Room(room.room_id));
                        if !same {
                            if let Some(rm) = level.try_room_info(room.room_id) {
                                if let Some(rm) = prev.and_then(|v| level.try_room_info(v)) {
                                    if let Some(rc) =
                                        entities.get_component_mut::<RoomController>(rm.controller)
                                    {
                                        rc.entities.retain(|v| *v != entity);
                                    }
                                }
                                if let Some(rc) =
                                    entities.get_component_mut::<RoomController>(rm.controller)
                                {
                                    rc.entities.push(entity);
                                }
                                entities.add_component(
                                    entity,
                                    entity::RoomOwned {
                                        room_id: room.room_id,
                                        should_release_inactive: false,
                                        active: true,
                                    },
                                );
                            }
                        }
                    } else {
                        let prev = entities
                            .remove_component::<RoomOwned>(entity)
                            .map(|v| v.room_id);
                        if let Some(rm) = prev.and_then(|v| level.try_room_info(v)) {
                            if let Some(rc) =
                                entities.get_component_mut::<RoomController>(rm.controller)
                            {
                                rc.entities.retain(|v| *v != entity);
                            }
                        }
                    }
                    if let Some(data) = e.data.as_ref() {
                        entities.add_component(
                            entity,
                            StateData {
                                controller: None,
                                data: data.0.clone(),
                            },
                        );
                    } else {
                        entities.remove_component::<StateData>(entity);
                    }
                    if let (Some(owner), Some(idle)) = (e.owner, e.idle.as_ref()) {
                        if let Some(i) = entities.get_component::<Idle>(entity) {
                            if Some(idle.idx as usize) != i.current_choice {
                                i.current_choice
                                    .map(|v| marker.clear_idle_choice(entity, owner, v));
                                marker.mark_idle_choice(entity, owner, idle.idx as usize);
                            }
                        } else {
                            marker.mark_idle_choice(entity, owner, idle.idx as usize);
                        }
                        entities.add_component(
                            entity,
                            Idle {
                                total_idle_time: 0,
                                current_choice: Some(idle.idx as usize),
                                released: false,
                            },
                        );
                        if e.room.is_none() {
                            assume!(self.log, entities.get_component_mut::<Controlled>(entity))
                                .by = Some(Controller::Idle(idle.idx as usize));
                        }
                    } else if let (Some(i), Some(owned)) = (
                        entities.remove_component::<Idle>(entity),
                        entities.get_component::<Owned>(entity),
                    ) {
                        i.current_choice
                            .map(|v| marker.clear_idle_choice(entity, owned.player_id, v));
                    }

                    if e.room.is_none() && e.idle.is_none() {
                        assume!(self.log, entities.get_component_mut::<Controlled>(entity)).by =
                            None;
                    }

                    if !e.emotes.is_empty() {
                        // TODO: Can cause duplicate icons in some cases
                        let skip = if let Some(icons) =
                            entities.get_component_mut::<entity::IconEmote>(entity)
                        {
                            icons.icons.extend(e.emotes.iter().map(|v| (v.0, v.1)));
                            true
                        } else {
                            false
                        };
                        if !skip {
                            entities.add_component(
                                entity,
                                entity::IconEmote::from_existing(
                                    e.emotes.iter().map(|v| (v.0, v.1)),
                                ),
                            )
                        }
                    }
                }
                EntityStateFlag::Removed => {
                    let entity = if let Some(e) = entity_map[id].take() {
                        e
                    } else {
                        continue;
                    };
                    if entities.is_valid(entity) {
                        entities.remove_entity(entity);
                    }
                }
                EntityStateFlag::Empty => {}
            }
        }
        Ok((
            Some(packet::EntityAckFrame {
                frame,
                entity_offset: entity_offset as _,
                entity_count: entity_count as _,
            }),
            player_ack,
        ))
    }

    fn next_frame_id(&mut self) -> u16 {
        self.current_frame += 1;
        // INVALID_FRAME (max 14 bit value) is used to
        // specify a 'full frame' so we must
        // never produce it as a normal frame
        if self.current_frame == INVALID_FRAME {
            self.current_frame = 0;
        }
        self.current_frame
    }
}

#[derive(DeltaEncode, Clone)]
struct PlayerSnapshot {
    #[delta_default]
    frame_id: u16,
    day_tick: DayTick,
    money: UniDollar,
    rating: i16,
    config: player::PlayerConfig,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum EntityStateFlag {
    Empty,   // Unused location
    Removed, // Entity existed previously, now removed
    Add,     // Entity added/replaced with another entity
    Update,  // Entity being updated
}

impl EntityStateFlag {
    fn as_u8(self) -> u8 {
        match self {
            EntityStateFlag::Empty => 0,
            EntityStateFlag::Removed => 1,
            EntityStateFlag::Add => 2,
            EntityStateFlag::Update => 3,
        }
    }

    fn from_u8(val: u8) -> EntityStateFlag {
        match val {
            0 => EntityStateFlag::Empty,
            1 => EntityStateFlag::Removed,
            2 => EntityStateFlag::Add,
            3 => EntityStateFlag::Update,
            _ => unreachable!(),
        }
    }
}

struct Snapshot {
    frame_id: u16,
    entities: Vec<Option<EntitySnapshot>>,
}

#[derive(DeltaEncode)]
struct EntitySnapshot {
    info: EntityInfo,
    #[delta_default]
    entity: ecs::Entity,

    owner: Option<player::Id>,

    target: ETarget,
    selected: Option<player::Id>,
    room: Option<ERoom>,
    data: Option<ScriptData>,
    idle: Option<IdleChoice>,
    emotes: Vec<EEmote>,
    tints: Vec<EColor>,
}

#[derive(DeltaEncode, PartialEq, Clone)]
struct IdleChoice {
    idx: u16,
}

#[derive(DeltaEncode, PartialEq, Clone)]
#[delta_complete]
struct EntityInfo {
    key: assets::ResourceKey<'static>,
    variant: u8,
    name: EName,
}

#[derive(DeltaEncode, Clone, PartialEq)]
struct EColor(u8, u8, u8, u8);

#[derive(DeltaEncode, Clone, PartialEq)]
struct EEmote(u8, Emote);

#[derive(DeltaEncode, Clone)]
struct EName(Arc<str>, Arc<str>);

impl PartialEq for EName {
    fn eq(&self, other: &EName) -> bool {
        Arc::ptr_eq(&self.0, &other.0) && Arc::ptr_eq(&self.1, &other.1)
    }
}

#[derive(Debug, Clone, DeltaEncode, PartialEq)]
struct ETarget {
    #[delta_fixed]
    #[delta_subbits = "4:5,6:5,10:5,16:5,-1:-1"]
    time: f32,
    #[delta_fixed]
    #[delta_diff]
    #[delta_subbits = "4:7,6:7,10:7,16:7,-1:-1"]
    x: f32,
    #[delta_fixed]
    #[delta_diff]
    #[delta_subbits = "4:7,6:7,10:7,16:7,-1:-1"]
    z: f32,
    facing: Option<EntityAngle>,
}

#[derive(Debug, Clone, Copy, DeltaEncode, PartialEq)]
struct EntityAngle(
    #[delta_fixed]
    #[delta_bits = "5:5"]
    f32,
);

#[derive(Clone, DeltaEncode, PartialEq)]
struct ERoom {
    room_id: room::Id,
}

/// Stores what frame each entity was
/// last based on.
pub struct EntitySnapshotState {
    entities: Vec<u16>,
}

impl EntitySnapshotState {
    /// Creates a new entity snapshot state
    pub fn new() -> EntitySnapshotState {
        EntitySnapshotState { entities: vec![] }
    }
    /// Updates the state with the ack (if usable)
    pub fn ack_entities(&mut self, ack: packet::EntityAckFrame) {
        let end = ack.entity_offset as usize + ack.entity_count as usize;
        while end >= self.entities.len() {
            self.entities.push(INVALID_FRAME);
        }
        for i in &mut self.entities[ack.entity_offset as usize..end] {
            if !is_previous_frame(*i, ack.frame) || *i == INVALID_FRAME {
                *i = ack.frame;
            }
        }
    }
}

/// Returns whether the frame id is an old frame and should be
/// ignored. Due to the looping nature of the frame id this
/// uses a threshold to make sure its old enough and not just
/// it wrapping around.
pub fn is_previous_frame(cur: u16, new: u16) -> bool {
    if new > cur {
        (new - cur) > 0x1FF
    } else {
        (cur - new) <= 0x1FF
    }
}

#[test]
fn test_previous_frame() {
    let test_data = &[(0, 5, false), (5, 0, true), (900, 0, false), (0, 900, true)];
    for &(c, n, r) in test_data {
        let val = is_previous_frame(c, n);
        assert_eq!(val, r, "{}<{} = {}, wanted: {}", c, n, val, r);
    }
}
