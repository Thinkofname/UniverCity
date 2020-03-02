use super::super::*;
use crate::entity;
use crate::level::room;
use crate::util::*;

// TODO: Split this up, not everything needs every part of this
//       (e.g. teaching rooms vs idle rooms)
/// Used on the entity that controls and manages
/// a room.
pub struct RoomController {
    /// The room id this controller controls
    pub room_id: room::Id,
    /// Whether this room has the required entities
    /// inside it.
    pub active: bool,
    /// List of entities waiting for this room to become active
    pub waiting_list: Vec<ecs::Entity>,
    /// Set of entities heading to this room
    pub potential_list: FNVSet<ecs::Entity>,
    /// Entities that this room currently controls
    pub entities: Vec<ecs::Entity>,
    /// Entities that are visiting this room
    pub visitors: Vec<ecs::Entity>,
    /// Visitors
    pub timetabled_visitors: [[FNVSet<ecs::Entity>; entity::timetable::NUM_TIMETABLE_SLOTS]; 7],

    /// The active staff member
    pub active_staff: Option<ecs::Entity>,

    /// The number of visitors this room can have
    pub capacity: usize,

    /// Count of script requested extra staff
    pub script_requests: FNVMap<ResourceKey<'static>, i32>,
    /// Number of ticks this room has been missing required staff for
    pub ticks_missing_staff: u32,
    /// Whether this room has warned about missing staff yet
    pub have_warned_missing: bool,
}
component!(RoomController => Map);

closure_system!(
    pub(crate) fn manage_room(
        em: EntityManager<'_>,
        log: Read<CLogger>,
        assets: Read<AssetManager>,
        rooms: Read<LevelRooms>,
        mut rc: Write<RoomController>,
        mut room_owned: Write<RoomOwned>,
        living: Read<Living>,
        mut entity_dispatcher: Write<EntityDispatcher>,
        mut goto_room: Write<GotoRoom>,
        position: Read<Position>,
        mut players: Write<crate::PlayerInfoMap>,
        owned: Read<Owned>,
        mut controlled: Write<Controlled>,
        day_tick: Read<DayTick>,
        booked: Read<Booked>,
        quitting: Read<Quitting>,
    ) {
        let log = log.get_component(Container::WORLD).expect("Missing logger");
        let rooms = assume!(log.log, rooms.get_component(Container::WORLD));
        let assets = assume!(log.log, assets.get_component(Container::WORLD));
        let players = assume!(log.log, players.get_component_mut(Container::WORLD));
        let entity_dispatcher = assume!(
            log.log,
            entity_dispatcher.get_component_mut(Container::WORLD)
        );

        let day_tick = assume!(log.log, day_tick.get_component(Container::WORLD));
        let time = day_tick.current_tick;
        let activity_slot = (time / LESSON_LENGTH) as usize;
        let day = (day_tick.day % 7) as usize;

        for (e, (rc, owned)) in em.group((&mut rc, &owned)) {
            let room_id = rc.room_id;
            let room_info = rooms.get_room_info(rc.room_id);

            // Remove 'dead' entities from the entity/waiting list
            rc.waiting_list.retain(|e| {
                em.is_valid(*e)
                    && goto_room
                        .get_component(*e)
                        .map_or(false, |v| v.room_id == room_id)
            });
            rc.entities.retain(|e| em.is_valid(*e));
            rc.visitors.retain(|e| em.is_valid(*e));
            rc.potential_list.retain(|e| {
                em.is_valid(*e)
                    && (goto_room
                        .get_component(*e)
                        .map_or(false, |v| v.room_id == room_id)
                        || controlled
                            .get_component(*e)
                            .map_or(false, |v| v.should_release))
            });
            let room = assume!(
                log.log,
                assets.loader_open::<room::Loader>(room_info.key.borrow())
            );

            if rc.visitors.is_empty() {
                rc.active = false;
            }
            if room.can_idle || !rc.visitors.is_empty() {
                rc.active = true;
            }

            let mut waiting_for_staff = false;
            let mut missing_staff = false;
            if let Some(course) = booked
                .get_component(e)
                .and_then(|v| v.timetable[day][activity_slot])
            {
                let player = assume!(log.log, players.get(&owned.player_id));
                let course = assume!(log.log, player.courses.get(&course));
                if let course::CourseEntry::Lesson { ref rooms, .. } =
                    course.timetable[day][activity_slot]
                {
                    let new_staff = rooms
                        .iter()
                        .filter(|v| v.room == room_id)
                        .map(|v| v.staff)
                        .next();
                    if rc.active_staff != new_staff {
                        if let Some(staff) = rc.active_staff.take() {
                            if em.is_valid(staff) {
                                let c = assume!(log.log, controlled.get_component_mut(staff));
                                c.should_release = true;
                            }
                        }
                        if let Some(staff) = new_staff {
                            if em.is_valid(staff) {
                                if quitting.get_component(staff).is_some() {
                                    missing_staff = true;
                                } else {
                                    let mut ok = true;
                                    if let Some(c) = controlled.get_component_mut(staff) {
                                        if c.by.is_some() && c.by != Some(Controller::Room(room_id))
                                        {
                                            c.wanted = Some(Controller::Room(room_id));
                                            c.should_release = true;
                                            ok = false;
                                            waiting_for_staff = true;
                                        }
                                    }
                                    if ok {
                                        rc.active_staff = Some(staff);
                                        rc.entities.push(staff);
                                        room_owned.add_component(staff, RoomOwned::new(room_id));
                                        controlled.add_component(
                                            staff,
                                            Controlled::new_by(Controller::Room(room_id)),
                                        );
                                    }
                                }
                            } else {
                                missing_staff = true;
                            }
                        } else {
                            missing_staff = true;
                        }
                    }
                }
            } else {
                if let Some(staff) = rc.active_staff.take() {
                    if em.is_valid(staff) {
                        let c = assume!(log.log, controlled.get_component_mut(staff));
                        c.should_release = true;
                    }
                }
            }

            // If the room isn't done then everyone should be kicked out
            if !room_info.state.is_done() || (!rc.visitors.is_empty() && missing_staff) {
                for e in &rc.entities {
                    let c = assume!(log.log, controlled.get_component_mut(*e));
                    c.should_release = true;
                }
                for e in &rc.visitors {
                    let c = assume!(log.log, controlled.get_component_mut(*e));
                    c.should_release = true;
                }
                for waiting in rc.waiting_list.drain(..) {
                    let remove = if let Some(goto_room) = goto_room.get_component(waiting) {
                        if goto_room.room_id != rc.room_id {
                            continue;
                        }
                        true
                    } else {
                        false
                    };
                    if remove {
                        goto_room.remove_component(waiting);
                    }
                }
                if rc.have_warned_missing {
                    players.get_mut(&owned.player_id).map(|v| {
                        v.notifications
                            .push(crate::notify::Notification::RoomMissingDismiss(rc.room_id))
                    });
                }
                rc.have_warned_missing = false;
                rc.ticks_missing_staff = 0;
                continue;
            }

            // If entities are waiting to use the room but can't
            // due it not being active (e.g. missing entities)
            // put out a request for the missing entities.
            if !rc.waiting_list.is_empty() || rc.active {
                // Work out which entities we are missing
                let mut requirements = room.required_entities.clone();
                for e in &rc.entities {
                    let living = if let Some(v) = living.get_component(*e) {
                        v
                    } else {
                        continue;
                    };
                    if let Some(c) = controlled.get_component(*e) {
                        // Don't count entities that are being removed
                        if c.should_release {
                            continue;
                        }
                    }
                    if let Some(count) = requirements.get_mut(&living.key) {
                        *count -= 1;
                    }
                }
                requirements.retain(|_, v| *v > 0);

                // Have everything we need, become active
                if requirements.is_empty() && !waiting_for_staff && !missing_staff {
                    let all_in_room = rc.entities.iter().all(|v| {
                        let pos = assume!(log.log, position.get_component(*v));
                        room_info
                            .area
                            .in_bounds(Location::new(pos.x as i32, pos.z as i32))
                    });
                    if all_in_room {
                        rc.active = true;
                    }
                    if rc.script_requests.is_empty() {
                        if rc.have_warned_missing {
                            players.get_mut(&owned.player_id).map(|v| {
                                v.notifications.push(
                                    crate::notify::Notification::RoomMissingDismiss(rc.room_id),
                                )
                            });
                        }
                        rc.have_warned_missing = false;
                        rc.ticks_missing_staff = 0;
                    }
                }

                if !requirements.is_empty() || !rc.script_requests.is_empty() || missing_staff {
                    rc.ticks_missing_staff += 1;
                    if rc.ticks_missing_staff > 20 * 15 && !rc.have_warned_missing {
                        rc.have_warned_missing = true;
                        if let Some(player) = players.get_mut(&owned.player_id) {
                            let mut description = format!("{} is currently missing: ", room.name);

                            for (idx, (entity, count)) in requirements
                                .iter()
                                .chain(rc.script_requests.iter())
                                .enumerate()
                            {
                                let ety = assume!(
                                    log.log,
                                    assets.loader_open::<Loader<entity::ServerComponent>>(
                                        entity.borrow()
                                    )
                                );
                                if idx != 0 {
                                    description.push_str(", ");
                                }
                                if *count == 1 {
                                    let first = ety
                                        .display_name
                                        .chars()
                                        .next()
                                        .map(|v| v.to_ascii_lowercase());
                                    description.push_str(&format!(
                                        "{} {}\n",
                                        match first {
                                            Some('a') | Some('e') | Some('i') | Some('o')
                                            | Some('u') => "an",
                                            _ => "a",
                                        },
                                        ety.display_name
                                    ))
                                } else {
                                    description
                                        .push_str(&format!("{} {}\n", *count, ety.display_name))
                                }
                            }
                            if missing_staff {
                                description.push_str("A professor. The booked one has left");
                            }
                            player
                                .notifications
                                .push(crate::notify::Notification::RoomMissing {
                                    room_id: rc.room_id,
                                    title: "Missing Staff".into(),
                                    description: description,
                                    icon: ResourceKey::new("base", "ui/icons/lost"),
                                });
                        }
                    }
                    entity_dispatcher.set_requests(rc.room_id, requirements);
                }
            } else {
                if rc.have_warned_missing {
                    players.get_mut(&owned.player_id).map(|v| {
                        v.notifications
                            .push(crate::notify::Notification::RoomMissingDismiss(rc.room_id))
                    });
                }
                rc.have_warned_missing = false;
                rc.ticks_missing_staff = 0;
            }
            if !rc.waiting_list.is_empty() && rc.active {
                for waiting in rc.waiting_list.drain(..) {
                    if let Some(goto_room) = goto_room.get_component_mut(waiting) {
                        if goto_room.room_id != rc.room_id {
                            continue;
                        }
                        goto_room.state = GotoRoomState::Done;
                    }
                    if room_owned.get_component(waiting).is_none() {
                        room_owned.add_component(waiting, RoomOwned::new(rc.room_id));
                        controlled.add_component(
                            waiting,
                            Controlled::new_by(Controller::Room(rc.room_id)),
                        );
                        rc.visitors.push(waiting);
                    } else {
                        let room_owned = assume!(log.log, room_owned.get_component(waiting));
                        error!(log.log, "Owned entity on the waiting list for a room";
                            "entity" => ?waiting,
                            "entity_room" => ?room_owned.room_id,
                            "room" => ?rc.room_id,
                        );
                    }
                }
            }
            rc.script_requests.clear();
        }
    }
);

/// Makes an entity walk to a room making sure
/// it is active.
pub struct GotoRoom {
    state: GotoRoomState,
    pub(crate) room_id: room::Id,
    request: Option<(pathfind::PathRequest, (f32, f32))>,
}
component!(GotoRoom => Map);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum GotoRoomState {
    FindRoom,
    WalkingToRoom,
    WaitingForRoom,
    Done,
}

impl GotoRoom {
    /// Creates a component that will make an entity
    /// walk into a room.
    pub fn new(
        log: &Logger,
        e: ecs::Entity,
        rooms: &LevelRooms,
        room_controller: &mut ecs::Write<RoomController>,
        room_id: room::Id,
    ) -> GotoRoom {
        let room = rooms.get_room_info(room_id);
        let rc = assume!(log, room_controller.get_component_mut(room.controller));
        rc.potential_list.insert(e);
        GotoRoom {
            state: GotoRoomState::FindRoom,
            room_id,
            request: None,
        }
    }
}

closure_system!(
    pub fn walk_to_room(
        em: EntityManager<'_>,
        log: Read<CLogger>,
        tiles: Read<LevelTiles>,
        rooms: Read<LevelRooms>,
        mut pathfinder: Write<pathfind::Pathfinder>,
        mut goto_room: Write<GotoRoom>,
        position: Read<Position>,
        mut info: Write<pathfind::PathInfo>,
        mut rc: Write<RoomController>,
        mut room_owned: Write<RoomOwned>,
        mut controlled: Write<Controlled>,
    ) {
        use rand::seq::SliceRandom;
        use rand::thread_rng;
        let log = log.get_component(Container::WORLD).expect("Missing logger");
        let tiles = assume!(log.log, tiles.get_component(Container::WORLD));
        let rooms = assume!(log.log, rooms.get_component(Container::WORLD));
        let pathfinder = assume!(log.log, pathfinder.get_component_mut(Container::WORLD));
        let mask = goto_room.mask();

        let mut rng = thread_rng();
        for e in em.iter_mask(&mask) {
            let remove = goto_room.get_component_mut(e).map_or(false, |v| {
                !rooms.room_exists(v.room_id)
                    || rooms.get_room_info(v.room_id).controller.is_invalid()
            });
            let remove = remove
                || {
                    let goto_room = assume!(log.log, goto_room.get_component_mut(e));

                    let room = rooms.get_room_info(goto_room.room_id);

                    // Check here to see if we can save a tick

                    match goto_room.state {
                        GotoRoomState::FindRoom => {
                            let pos = assume!(log.log, position.get_component(e));

                            // Stuck, wait for unstuck
                            if !level::can_visit(
                                tiles,
                                rooms,
                                (pos.x * 4.0) as usize,
                                (pos.z * 4.0) as usize,
                            ) {
                                continue;
                            }

                            if goto_room.request.is_none() {
                                let target = (
                                    room.area.min.x as f32 + (room.area.width() as f32 / 2.0),
                                    room.area.min.y as f32 + (room.area.height() as f32 / 2.0),
                                );

                                let doors = room
                                    .objects
                                    .iter()
                                    .filter_map(|v| v.as_ref())
                                    .filter_map(|v| {
                                        for action in &v.0.actions.0 {
                                            if let level::ObjectPlacementAction::WallFlag {
                                                location,
                                                direction,
                                                flag: level::object::WallPlacementFlag::Door,
                                            } = *action
                                            {
                                                return Some((location, direction));
                                            }
                                        }
                                        None
                                    })
                                    .collect::<Vec<_>>();

                                let target = if let Some(&(loc, _dir)) = doors.choose(&mut rng) {
                                    Some((loc.x as f32 + 0.5, loc.y as f32 + 0.5))
                                } else {
                                    room.find_free_point(tiles, rooms, target)
                                };
                                if let Some(target) = target {
                                    goto_room.request = Some((
                                        pathfinder.create_path((pos.x, pos.z), target, true),
                                        target,
                                    ));
                                }
                            }

                            if let Some((mut req, target)) = goto_room.request.take() {
                                match req.take_path() {
                                    PathResult::Ok(mut path) => {
                                        path.nodes = path
                                            .nodes
                                            .into_iter()
                                            .take_while(|v| {
                                                !room.area.in_bounds(Location::new(
                                                    v.x as i32, v.z as i32,
                                                ))
                                            })
                                            .collect();
                                        path.update_time();
                                        info.add_component(e, path);
                                        goto_room.state = GotoRoomState::WalkingToRoom;
                                    }
                                    PathResult::Failed => {
                                        warn!(log.log, "Goto(FindRoom): Failed to create path for {:?}", e; "start" => ?(pos.x, pos.z), "end" => ?target, "room" => ?room.key.borrow());
                                        goto_room.state = GotoRoomState::WalkingToRoom;
                                    }
                                    PathResult::Waiting => {
                                        goto_room.request = Some((req, target));
                                        continue;
                                    }
                                }
                            } else {
                                goto_room.state = GotoRoomState::WalkingToRoom;
                            }
                        }
                        GotoRoomState::WalkingToRoom => {
                            if info.get_component(e).is_none() {
                                let rc = assume!(log.log, rc.get_component_mut(room.controller));
                                if rc.active {
                                    goto_room.state = GotoRoomState::Done;
                                } else {
                                    rc.waiting_list.push(e);
                                    goto_room.state = GotoRoomState::WaitingForRoom;
                                }
                            }
                        }
                        GotoRoomState::WaitingForRoom | GotoRoomState::Done => {}
                    }
                    goto_room.state == GotoRoomState::Done
                };
            if remove {
                let gr = assume!(log.log, goto_room.remove_component(e));
                if rooms.room_exists(gr.room_id) {
                    change_ownership(
                        &log.log,
                        rooms,
                        e,
                        &mut rc,
                        &mut room_owned,
                        &mut controlled,
                        gr.room_id,
                    );
                }
            }
        }
    }
);

pub(super) fn change_ownership(
    log: &Logger,
    rooms: &level::LevelRooms,
    e: ecs::Entity,
    rc: &mut ecs::Write<RoomController>,
    room_owned: &mut ecs::Write<RoomOwned>,
    controlled: &mut ecs::Write<Controlled>,
    room_id: room::Id,
) {
    if let Some(old) = room_owned.remove_component(e) {
        if old.room_id == room_id {
            room_owned.add_component(e, old);
            return;
        }
        let room = rooms.get_room_info(old.room_id);
        let rc = assume!(log, rc.get_component_mut(room.controller));
        if let Some(pos) = rc.entities.iter().position(|v| *v == e) {
            rc.entities.swap_remove(pos);
        }
        if let Some(pos) = rc.visitors.iter().position(|v| *v == e) {
            rc.visitors.swap_remove(pos);
        }
    }
    room_owned.add_component(e, RoomOwned::new(room_id));
    controlled.add_component(e, Controlled::new_by(Controller::Room(room_id)));
    let room = rooms.get_room_info(room_id);
    if !room.controller.is_invalid() {
        let rc = assume!(log, rc.get_component_mut(room.controller));
        rc.visitors.push(e);
    }
}

closure_system!(
    pub fn leave_room(
        em: EntityManager<'_>,
        log: Read<CLogger>,
        assets: Read<AssetManager>,
        tiles: Read<LevelTiles>,
        rooms: Read<LevelRooms>,
        goto_room: Read<GotoRoom>,
        living: Read<Living>,
        frozen: Read<Frozen>,
        position: Read<Position>,
        mut info: Write<pathfind::PathInfo>,
        mut room_owned: Write<RoomOwned>,
        mut p_target: Write<pathfind::Target>,
        mut force_leave: Write<ForceLeave>,
        free_roam: Read<free_roam::FreeRoam>,
    ) {
        use rand::{thread_rng, Rng};
        let log = log.get_component(Container::WORLD).expect("Missing logger");
        let assets = assume!(log.log, assets.get_component(Container::WORLD));
        let tiles = assume!(log.log, tiles.get_component(Container::WORLD));
        let rooms = assume!(log.log, rooms.get_component(Container::WORLD));

        let mut rng = thread_rng();

        for (e, pos) in em.group_mask(&position, |m| {
            m.and(&living)
                .and_not(&p_target)
                .and_not(&frozen)
                .and_not(&info)
                .and_not(&goto_room)
                .and_not(&free_roam)
        }) {
            let eowner = room_owned.get_component(e).map(|v| v.room_id);
            let lpos = Location::new(pos.x as i32, pos.z as i32);
            let owner = if let Some(owner) = tiles.get_room_owner(lpos) {
                owner
            } else {
                continue;
            };
            // Is the entity in the room that owns it
            if eowner.map_or(false, |v| v == owner) {
                // Yes, then everything is fine
                continue;
            }
            let room = rooms.get_room_info(owner);
            let force = force_leave
                .get_component(e)
                .map_or(false, |v| v.room_id == room.id);
            // Only remove entities from rooms with scripting or
            // ones that don't allow for idling)
            // This cuts out rooms like roads which is fine for
            // anything to walk in/on
            let ty = assume!(
                log.log,
                assets.loader_open::<room::Loader>(room.key.borrow())
            );
            if !force && (ty.controller.is_none() || ty.can_idle) && room.state.is_done() {
                continue;
            }
            force_leave.remove_component(e);

            // Find a door and leave
            let mut target = None;
            'find_door: for obj in room.objects.iter().filter_map(|v| v.as_ref()) {
                for action in &obj.0.actions.0 {
                    if let level::ObjectPlacementAction::WallFlag {
                        location,
                        direction,
                        flag: level::object::WallPlacementFlag::Door,
                    } = *action
                    {
                        if tiles.get_room_owner(location).map_or(true, |v| v != owner) {
                            target = Some(location);
                            break 'find_door;
                        }
                        if tiles
                            .get_room_owner(location.shift(direction))
                            .map_or(true, |v| v != owner)
                        {
                            target = Some(location.shift(direction));
                            break 'find_door;
                        }
                    }
                }
            }

            if let Some(target) = target {
                // Stuck, wait for unstuck
                if !level::can_visit(tiles, rooms, (pos.x * 4.0) as usize, (pos.z * 4.0) as usize) {
                    continue;
                }

                let end = (target.x as f32 + 0.5, target.y as f32 + 0.5);
                p_target.add_component(e, pathfind::Target::new(end.0, end.1));
            } else {
                // Try a random location
                let end = (
                    pos.x + rng.gen_range(-20.0, 20.0),
                    pos.z + rng.gen_range(-20.0, 20.0),
                );
                p_target.add_component(e, pathfind::Target::new(end.0, end.1));
            }
        }
    }
);
