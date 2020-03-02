use super::super::*;
use crate::level::room;
use crate::prelude::*;
use crate::util::FNVMap;

/// Manages giving unused entities to rooms that require them
pub struct EntityDispatcher {
    requests: FNVMap<room::Id, FNVMap<ResourceKey<'static>, i32>>,
}

impl EntityDispatcher {
    /// Creates a new `EntityDispatcher`
    pub fn new() -> EntityDispatcher {
        EntityDispatcher {
            requests: FNVMap::default(),
        }
    }

    /// Updates/creates the list of entities the room wants
    pub fn set_requests(&mut self, room: room::Id, requests: FNVMap<ResourceKey<'static>, i32>) {
        self.requests.insert(room, requests);
    }

    /// Clears the list of entities the room wants
    pub fn clear_requests(&mut self, room: room::Id) {
        self.requests.remove(&room);
    }
}

/// Attempts to find an entity now instead of delayed via
/// `EntityDispatcher`
pub fn request_entity_now(
    log: &Logger,
    player_id: player::Id,
    ty: ResourceKey<'_>,
    call_position: Option<(f32, f32)>,
    wanted_for: Controller,
    entities: &mut ecs::Container,
) -> Option<ecs::Entity> {
    entities.with(
        |em: EntityManager<'_>,
         frozen: ecs::Read<Frozen>,
         position: ecs::Read<Position>,
         owned: ecs::Read<Owned>,
         room_owned: ecs::Read<RoomOwned>,
         living: ecs::Read<Living>,
         goto_room: ecs::Read<GotoRoom>,
         quitting: ecs::Read<Quitting>,
         mut controlled: ecs::Write<Controlled>| {
            let mask = living
                .mask()
                .and(&position)
                .and(&owned)
                .and_not(&goto_room)
                .and_not(&frozen)
                .and_not(&quitting);
            find_best_entity(
                log,
                player_id,
                ty,
                call_position,
                wanted_for,
                em.iter_mask(&mask),
                &position,
                &owned,
                &room_owned,
                &living,
                &mut controlled,
            )
        },
    )
}

/// This assumes that the caller will keep calling if the first request fails
/// as it may take time to release an entity to give to them
fn find_best_entity<I>(
    log: &Logger,
    player_id: PlayerId,
    ty: ResourceKey<'_>,
    call_position: Option<(f32, f32)>,
    wanted_for: Controller,
    entities: I,
    position: &ecs::Read<Position>,
    owned: &ecs::Read<Owned>,
    room_owned: &ecs::Read<RoomOwned>,
    living: &ecs::Read<Living>,
    controlled: &mut ecs::Write<Controlled>,
) -> Option<Entity>
where
    I: Iterator<Item = Entity>,
{
    use std::cmp::Ordering;
    let res = entities
        .filter(|e| {
            let owned = assume!(log, owned.get_component(*e));
            owned.player_id == player_id
        })
        .filter(|e| room_owned.get_component(*e).map_or(true, |v| !v.active))
        .filter(|e| {
            let living = assume!(log, living.get_component(*e));
            living.key == ty
        })
        .filter(|e| {
            controlled
                .get_component(*e)
                .and_then(|v| v.by)
                .map_or(true, |v| {
                    (v <= wanted_for || v.is_room()) && v != wanted_for
                })
        })
        .min_by(|a, b| {
            // Put the ones not owned by a room first
            room_owned
                .get_component(*a)
                .is_some()
                .cmp(&room_owned.get_component(*b).is_some())
                // If same sort by distance instead
                .then_with(|| {
                    if let Some(call_position) = call_position {
                        let apos = assume!(log, position.get_component(*a));
                        let bpos = assume!(log, position.get_component(*b));
                        let adx = apos.x - call_position.0;
                        let adz = apos.z - call_position.1;
                        let bdx = bpos.x - call_position.0;
                        let bdz = bpos.z - call_position.1;
                        (adx * adx - adz * adz)
                            .partial_cmp(&(bdx * bdx - bdz * bdz))
                            .unwrap_or(Ordering::Equal)
                    } else {
                        Ordering::Equal
                    }
                })
        });

    if let Some(e) = res {
        if let Some(c) = controlled.get_component_mut(e) {
            if c.by.is_none() {
                Some(e)
            } else {
                c.wanted = Some(wanted_for);
                c.should_release = true;
                None
            }
        } else {
            Some(e)
        }
    } else {
        None
    }
}

closure_system!(
    pub fn manage_entity_dispatch(
        em: EntityManager<'_>,
        log: Read<CLogger>,
        rooms: Read<LevelRooms>,
        mut entity_dispatcher: Write<EntityDispatcher>,
        living: Read<Living>,
        position: Read<Position>,
        owned: Read<Owned>,
        mut rc: Write<RoomController>,
        mut room_owned: Write<RoomOwned>,
        frozen: Read<Frozen>,
        goto_room: Read<GotoRoom>,
        quitting: Read<Quitting>,
        mut controlled: Write<Controlled>,
    ) {
        let log = log.get_component(Container::WORLD).expect("Missing logger");
        let rooms = assume!(log.log, rooms.get_component(Container::WORLD));
        let entity_dispatcher = assume!(
            log.log,
            entity_dispatcher.get_component_mut(Container::WORLD)
        );
        let mask = living
            .mask()
            .and(&position)
            .and(&owned)
            .and_not(&goto_room)
            .and_not(&frozen)
            .and_not(&quitting);

        // Try and complete requests
        for (_e, rc) in em.group(&mut rc) {
            let room = rooms.get_room_info(rc.room_id);
            if let Some(reqs) = entity_dispatcher.requests.get_mut(&rc.room_id) {
                for (req, count) in reqs.iter_mut() {
                    for _ in 0..*count {
                        if let Some(e) = find_best_entity(
                            &log.log,
                            room.owner,
                            req.borrow(),
                            Some((
                                room.area.min.x as f32 + room.area.width() as f32 / 2.0,
                                room.area.min.y as f32 + room.area.height() as f32 / 2.0,
                            )),
                            Controller::Room(rc.room_id),
                            em.iter_mask(&mask),
                            &position,
                            &owned,
                            &room_owned.read(),
                            &living,
                            &mut controlled,
                        ) {
                            *count -= 1;
                            room_owned.add_component(e, RoomOwned::new(rc.room_id));
                            controlled
                                .add_component(e, Controlled::new_by(Controller::Room(rc.room_id)));
                            rc.entities.push(e);
                        }
                    }
                }
                reqs.retain(|_, v| *v > 0);
            }
        }
        // Remove completed requests
        entity_dispatcher
            .requests
            .retain(|_, reqs| !reqs.is_empty());
    }
);
