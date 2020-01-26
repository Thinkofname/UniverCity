use super::super::*;
use crate::prelude::*;

/// Manages a student entity.
///
/// This will try ensure the student has
/// registered and follows their timetable.
pub struct StudentController {
    pub(crate) completed_lessons: FNVSet<ResourceKey<'static>>,
    pub(crate) completed_courses: FNVSet<course::CourseId>,
}
component!(StudentController => Map);

impl StudentController {
    /// Creates a new `StudentController`
    pub fn new() -> StudentController {
        StudentController {
            completed_lessons: FNVSet::default(),
            completed_courses: FNVSet::default(),
        }
    }
}

closure_system!(pub fn get_timetable(
    em: EntityManager<'_>,
    log: Read<CLogger>,
    rooms: Read<LevelRooms>,
    timetable: Read<timetable::TimeTable>,
    timetable_completed: Read<timetable::TimeTableCompleted>,
    student: Read<StudentController>,
    frozen: Read<Frozen>,
    pos: Read<Position>,
    mut goto_room: Write<GotoRoom>,
    mut room_owned: Write<RoomOwned>,
    owned: Read<Owned>,
    mut idle: Write<Idle>,
    mut rc: Write<RoomController>,
    mut quitting: Write<Quitting>,
    mut controlled: Write<Controlled>
) {
    let log = log.get_component(Container::WORLD).expect("Missing logger");
    let rooms = assume!(log.log, rooms.get_component(Container::WORLD));

    let reg_room = assets::ResourceKey::new("base", "registration_office");

    for (e, (pos, owned)) in em.group_mask((&pos, &owned), |m| m
        .and(&student)
        .and_not(&frozen)
        .and_not(&goto_room)
        .and_not(&quitting)
    ) {
        if timetable.get_component(e).is_some() && timetable_completed.get_component(e).is_none() {
            continue;
        }
        if let Some(ro) = room_owned.get_component_mut(e) {
            let info = rooms.get_room_info(ro.room_id);
            // Already in a registration office, do nothing
            if info.key == reg_room {
                continue;
            }
        }
        let owner = owned.player_id;
        let nearest_reg = rooms.room_ids()
            .map(|v| rooms.get_room_info(v))
            .filter(|v| v.state.is_done())
            .filter(|v| v.key == reg_room)
            .filter(|v| v.owner == owner)
            .filter(|v| {
                let rc = assume!(log.log, rc.get_component(v.controller));
                rc.visitors.len() + rc.waiting_list.len() + rc.potential_list.len() < rc.capacity
            })
            .min_by(|a, b| {
                let adx = ((a.area.min.x + a.area.max.x) / 2) - pos.x as i32;
                let adz = ((a.area.min.y + a.area.max.y) / 2) - pos.z as i32;
                let a_dist = adx * adx + adz * adz;
                let bdx = ((b.area.min.x + b.area.max.x) / 2) - pos.x as i32;
                let bdz = ((b.area.min.y + b.area.max.y) / 2) - pos.z as i32;
                let b_dist = bdx * bdx + bdz * bdz;
                a_dist.cmp(&b_dist)
            });
        if let Some(room) = nearest_reg {
            // Add this student to the potential list early so that
            // other students don't try and claim this spot as well.
            {
                let room = rooms.get_room_info(room.id);
                let rc = assume!(log.log, rc.get_component_mut(room.controller));
                rc.potential_list.insert(e);
            }

            // If the entity is owned by a room (e.g. idling)
            // release them first
            let c = assume!(log.log, controlled.get_component_mut(e));
            if c.by.is_some() {
                // Are we the one in control already?
                if let Some(ro) = room_owned.get_component(e) {
                    if ro.room_id != room.id {
                        c.should_release = true;
                        c.wanted = Some(Controller::Room(room.id));
                        continue;
                    }
                } else {
                    c.should_release = true;
                    c.wanted = Some(Controller::Room(room.id));
                    continue;
                }
            }
            goto_room.add_component(e, GotoRoom::new(&log.log, e, rooms, &mut rc, room.id));

        // Can't register yet, idle
        } else if idle.get_component(e).is_none() {
            idle.add_component(e, Idle::new());
            debug!(log.log, "Waiting to register"; "entity" => ?e);
        } else {
            let time = {
                let idle = assume!(log.log, idle.get_component(e));
                idle.total_idle_time
            };
            // Wait for three minutes then leave
            if time >= 20 * 60 * 3 {
                let c = assume!(log.log, controlled.get_component_mut(e));
                if c.by.is_some() {
                    c.should_release = true;
                    c.wanted = Some(Controller::Quit);
                    continue;
                }
                c.by = Some(Controller::Quit);
                quitting.add_component(e, Quitting);
            }
        }
    }
});

closure_system!(pub(crate) fn quit_sys(
    em: EntityManager<'_>,
    log: Read<CLogger>,
    rooms: Read<LevelRooms>,
    position: Read<Position>,
    mut quit: Write<Quitting>,
    path_info: Read<pathfind::PathInfo>,
    mut target: Write<pathfind::Target>,
    mut timetable: Write<TimeTable>,
    mut rc: Write<RoomController>,
    mut sc: Write<StudentController>,
    mut grades: Write<Grades>,
    mut controlled: Write<Controlled>,
    owned: Read<Owned>,
    mut players: Write<crate::PlayerInfoMap>,
    mut booked: Write<Booked>
) {
    let log = log.get_component(Container::WORLD).expect("Missing logger");
    let rooms = assume!(log.log, rooms.get_component(Container::WORLD));
    let players = assume!(log.log, players.get_component_mut(Container::WORLD));

    let road = ResourceKey::new("base", "external/road");

    for (e, position) in em.group_mask(&position, |m| m.and(&quit)) {
        if path_info.get_component(e).is_some()
            || target.get_component(e).is_some()
        {
            continue;
        }

        // Clear the student's timetable to free up space in rooms
        if let Some(t) = timetable.remove_component(e) {
            super::timetable::clear_time_table(&log.log, e, rooms, t, players, &owned, &mut rc, &mut sc, &mut grades);
        }

        // Clear the entity from any courses its assigned to
        if let (Some(owned), Some(b)) = (owned.get_component(e), booked.remove_component(e)) {
            let player = assume!(&log.log, players.get_mut(&owned.player_id));
            for (di, d) in b.timetable.iter().enumerate() {
                for (pi, p) in d.iter().enumerate() {
                    if let Some(course) = *p {
                        if let Some(course) = player.courses.get_mut(&course) {
                            if let course::CourseEntry::Lesson{ref mut rooms, ..} = course.timetable[di][pi] {
                                for r in rooms {
                                    if r.staff == e {
                                        r.staff = Entity::INVALID;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Leave the current task
        if let Some(c) = controlled.get_component_mut(e) {
            if c.by.is_some() && c.by != Some(Controller::Quit) {
                c.wanted = Some(Controller::Quit);
                c.should_release = true;
                continue;
            }
            c.by = Some(Controller::Quit);
        }
        let pos = Location::new(position.x as i32, position.z as i32);

        let ne = assume!(log.log, rooms.room_ids()
            .map(|v| rooms.get_room_info(v))
            .filter(|v| v.key == road)
            .filter(|v| v.area.min.x == 0 || v.area.min.y == 0
                || v.area.max.x + 1 == rooms.width as i32
                || v.area.max.y + 1 == rooms.height as i32)
            .min_by_key(|a| {
                let ax = a.area.min.x + (a.area.width() / 2) - pos.x;
                let ay = a.area.min.y + (a.area.height() / 2) - pos.y;
                ax * ax + ay * ay
            }));

        if ne.area.in_bounds(pos) {
            // TODO: Fade?
            em.remove_entity(e);
        } else {
            let mut tx = ne.area.min.x + (ne.area.width() / 2);
            let mut ty = ne.area.min.y + (ne.area.height() / 2);
            if ne.area.min.x == 0 {
                tx = 0;
            } else if ne.area.max.x + 1 == rooms.width as i32 {
                tx = ne.area.max.x;
            }
            if ne.area.min.y == 0 {
                ty = 0;
            } else if ne.area.max.y + 1 == rooms.height as i32 {
                ty = ne.area.max.y;
            }
            target.add_component(e, pathfind::Target::new(tx as f32 + 0.5, ty as f32 + 0.5));
        }
    }
});