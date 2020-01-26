
use crate::errors;
use crate::level;
use crate::entity;
use crate::ecs;
use crate::util::*;
use super::*;
use rand::{self, Rng};


/// Number of lessons in a single day
pub const NUM_TIMETABLE_SLOTS: usize = 4;
/// Length in ticks of a single lesson
pub const LESSON_LENGTH: i32 = 20 * 60 * 2;

pub(crate) fn clear_time_table(
    log: &Logger,
    e: ecs::Entity,
    level: &level::LevelRooms,
    timetable: TimeTable,
    players: &crate::PlayerInfoMap,
    owned: &ecs::Read<Owned>,
    rc: &mut ecs::Write<RoomController>,
    sc: &mut ecs::Write<StudentController>,
    grades: &mut ecs::Write<Grades>,
) {
    let student = assume!(log, sc.get_component_mut(e));
    let grades = assume!(log, grades.get_component_mut(e));
    let owned = assume!(log, owned.get_component(e));

    let player = assume!(log, players.get(&owned.player_id));

    let course = assume!(log, player.courses.get(&timetable.course));

    student.completed_courses.insert(timetable.course);

    let mut lgrades = Vec::new();
    for (di, day) in course.timetable.iter().enumerate() {
        for (pi, p) in day.iter().enumerate() {
            match p {
                course::CourseEntry::Lesson{key, rooms} => {
                    // Can be false if the lesson was added after the student took the course
                    let mut took_lesson = false;
                    for room in rooms {
                        let room = if let Some(room) = level.try_room_info(room.room) {
                            room
                        } else {
                            continue
                        };
                        if !room.controller.is_invalid() {
                            let con = assume!(log, rc.get_component_mut(room.controller));
                            took_lesson |= con.timetabled_visitors[di][pi].remove(&e);
                        }
                    }
                    if !took_lesson {
                        continue;
                    }
                    student.completed_lessons.insert(key.clone());

                    if let Some(grade) = grades.timetable_grades[di][pi] {
                        lgrades.push(grade);
                    }
                },
                _ => {},
            }
        }
    }

    let mut total_grade = 0.0;
    for grade in &lgrades {
        total_grade += match grade {
            Grade::A => 10.0,
            Grade::B => 6.5,
            Grade::C => 4.0,
            Grade::D => 2.0,
            Grade::E => 1.0,
            Grade::F => 0.0,
        }
    }
    let grade = total_grade / (lgrades.len() as f32);
    let grade = if grade <= 0.5 {
        Grade::F
    } else if grade <= 1.5 {
        Grade::E
    } else if grade <= 3.5 {
        Grade::D
    } else if grade <= 6.0 {
        Grade::C
    } else if grade <= 9.0 {
        Grade::B
    } else {
        Grade::A
    };
    debug!(log, "Given {:?} a grade of {:?} for {:?} [{}]", e, grade, course.name,
        lgrades.iter()
            .map(|v| format!("{:?},", v))
            .collect::<String>()
    );

    grades.timetable_grades = Default::default();
    grades.grades.push(GradeEntry {
        course: timetable.course,
        grade,
    })
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TimeTable {
    pub course: course::CourseId,
}
component!(TimeTable => Vec);

pub(crate) fn generate_time_table(
    log: &Logger,
    _assets: &AssetManager,
    players: &mut crate::PlayerInfoMap,
    level_rooms: &level::LevelRooms,
    entities: &mut ecs::Container,
    e: ecs::Entity
) -> errors::Result<UniDollar> {
    use rand::seq::SliceRandom as _;

    entities.with(|
        _em: EntityManager,
        owned: Read<entity::Owned>,
        money: Read<entity::Money>,
        mut rc: Write<RoomController>,
        mut timetable: Write<TimeTable>,
        mut timetable_completed: ecs::Write<TimeTableCompleted>,
    | -> errors::Result<UniDollar> {
        let owner = owned.get_component(e)
            .ok_or_else(|| ErrorKind::Static("Invalid entity"))?
            .player_id;
        let money = money.get_component(e)
            .ok_or_else(|| ErrorKind::Static("Invalid entity"))?;


        // TODO: If the student is already on a course look for follow up courses?
        if timetable.get_component(e).is_some() {
            bail!("Already completed a course")
        }

        let player = players.get_mut(&owner)
            .ok_or_else(|| ErrorKind::Static("Missing player info"))?;
        if player.courses.is_empty() {
            bail!("No courses");
        }

        // Select a subject group that the student is interested in
        // TODO: Allow the spawning script to override this

        // This uses the created courses for the groups making this biased to
        // the group with the most courses.
        // TODO: Ideally this would be based on how well the courses are doing
        // instead.
        let mut rng = rand::thread_rng();
        let selection = rng.gen_range(0, player.courses.len());
        let group = assume!(log, player.courses.values()
            .nth(selection)
            .map(|v| v.group.clone()));

        let mut courses = player.courses.values()
            .filter(|v| !v.deprecated)
            .collect::<Vec<_>>();
        courses.shuffle(&mut rng);
        'courses:
        for course in courses {
            if course.group != group {
                continue;
            }
            // Make sure the student can afford this course
            if course.cost > money.money {
                continue;
            }

            // Make sure there is space on this course
            // TODO: This may be a bit too slow as its a lot of checking

            for (di, day) in course.timetable.iter().enumerate() {
                for (pi, p) in day.iter().enumerate() {
                    if let course::CourseEntry::Lesson{ref rooms, ..} = p {
                        let mut used = 0;
                        let mut total = 0;

                        for lm in rooms {
                            let rm = level_rooms.get_room_info(lm.room);
                            let rc = if let Some(c) = rc.get_component(rm.controller) {
                                c
                            } else {
                                continue 'courses;
                            };
                            used += rc.timetabled_visitors[di][pi].len();
                            total += rc.capacity;
                        }

                        if used >= total {
                            continue 'courses;
                        }
                    }
                }
            }

            // If we made it this far then the course has space for the student

            debug!(log, "Booked {:?} for {:?}", e, course.name);
            timetable.add_component(e, TimeTable {
                course: course.uid,
            });
            timetable_completed.remove_component(e);
            book_into_course(log, level_rooms, course, &mut rc, e);

            return Ok(course.cost);
        }


        Err(ErrorKind::Static("No courses").into())
    })
}

pub(crate) fn book_into_course(
    log: &Logger,
    level_rooms: &LevelRooms, course: &course::Course,
    rc: &mut Write<RoomController>, e: ecs::Entity
) {
    for (di, day) in course.timetable.iter().enumerate() {
        'day:
            for (pi, p) in day.iter().enumerate() {
                if let course::CourseEntry::Lesson{ref rooms, ..} = p {
                    for lm in rooms {
                        let rm = level_rooms.get_room_info(lm.room);
                        let rc = assume!(log, rc.get_component_mut(rm.controller));
                        if rc.timetabled_visitors[di][pi].len() < rc.capacity {
                            rc.timetabled_visitors[di][pi].insert(e);
                            continue 'day;
                        }
                    }
                    panic!("Failed to book the student into a room")
                }
            }
    }
}

/// Contains the current activity of a student
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Activity {
    pub(crate) day: u8,
    pub(crate) slot: u8,
}
component!(Activity => Vec);

/// Contains the day that the student started on
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeTableStart {
    day: u8,
    started: bool,
}
component!(TimeTableStart => Vec);
#[derive(Default)]
pub struct TimeTableCompleted;
component!(TimeTableCompleted => Marker);

// BUG: ?
// * Editing the rooms whilst students are on a course
//   causes the missing room part to trigger because they
//   are not registered into the new lesson

closure_system!(pub(crate) fn manage_time_table(
    em: EntityManager<'_>,
    log: Read<CLogger>,
    rooms: Read<level::LevelRooms>,
    day_tick: Read<DayTick>,
    players: Read<crate::PlayerInfoMap>,

    mut timetable: Write<TimeTable>,
    owned: Read<Owned>,
    student: Read<StudentController>,
    frozen: Read<Frozen>,
    mut goto_room: Write<GotoRoom>,
    mut activity: Write<Activity>,
    mut timetable_start: Write<TimeTableStart>,
    mut timetable_completed: Write<TimeTableCompleted>,

    mut rc: Write<RoomController>,
    mut idle: Write<Idle>,
    mut controlled: Write<Controlled>,
    mut emotes: Write<IconEmote>
) {
    let log = log.get_component(Container::WORLD).expect("Missing logger");
    let rooms = assume!(log.log, rooms.get_component(Container::WORLD));
    let players = assume!(log.log, players.get_component(Container::WORLD));

    let day_tick = assume!(log.log, day_tick.get_component(Container::WORLD));
    let time = day_tick.current_tick;
    let activity_slot = (time / LESSON_LENGTH) as usize;
    let day = (day_tick.day % 7) as usize;

    for (e, (timetable, owned)) in em.group_mask((&mut timetable, &owned), |m| m
        .and(&student)
        .and_not(&frozen)
        .and_not(&goto_room)
        .and_not(&timetable_completed)
    ) {
        let timetable_start = timetable_start.get_component_or_insert(e, || TimeTableStart {
            day: (day as u8 + 1) % 7,
            started: false,
        });
        // Don't start on the same day as registering otherwise students end up
        // half lessons
        if !timetable_start.started && !(timetable_start.day == day as u8 && activity_slot == 0) {
            if idle.get_component(e).is_none() {
                debug!(log.log, "Entity waiting to start, idling"; "entity" => ?e, "course" => ?timetable.course);
                idle.add_component(e, Idle::new());
            }
            continue;
        }

        if !timetable_start.started {
            debug!(log.log, "Entity starting course {:?}", timetable_start;
                "entity" => ?e, "course" => ?timetable.course, "day" => %day, "slot" => %activity_slot
            );
        }

        if let Some(activity) = activity.get_component_mut(e) {
            if activity.day == day as u8 && activity.slot == activity_slot as u8 {
                continue;
            }
        }

        if timetable_start.started && timetable_start.day == day as u8 && activity_slot == 0 {
            debug!(log.log, "Entity finished course {:?}", timetable_start; "entity" => ?e, "course" => ?timetable.course);
            timetable_completed.add_component(e, TimeTableCompleted);
            continue;
        }

        let player = assume!(log.log, players.get(&owned.player_id));
        let course = if let Some(c) = player.courses.get(&timetable.course) {
            c
        } else {
            debug!(log.log, "Entity missing course"; "entity" => ?e, "course" => ?timetable.course);
            continue;
        };

        let entry = &course.timetable[day][activity_slot];

        if let course::CourseEntry::Lesson{rooms: rms, ..} = entry {
            // Find the assigned room
            if let Some(room_id) = rms.iter()
                .filter_map(|v| rooms.try_room_info(v.room))
                .filter(|v| !v.controller.is_invalid())
                .filter_map(|v| rc.get_component(v.controller))
                .filter(|v| v.timetabled_visitors[day][activity_slot].contains(&e))
                .map(|v| v.room_id)
                .next()
            {
                // A room owns this entity. Ask them to free it
                let c = assume!(log.log, controlled.get_component_mut(e));
                if c.by.is_some() {
                    c.should_release = true;
                    c.wanted = Some(Controller::Room(room_id));
                    continue;
                }
                debug!(log.log, "Sending entity to room"; "room" => ?room_id, "entity" => ?e, "course" => ?timetable.course);
                goto_room.add_component(e, GotoRoom::new(&log.log, e, rooms, &mut rc, room_id));
            } else {
                debug!(log.log, "Entity Confused, missing room"; "entity" => ?e, "course" => ?timetable.course);
                IconEmote::add(&mut emotes, e, Emote::Confused);
                idle.add_component(e, Idle::new());
            }
        // Free period, find something to do
        } else if idle.get_component(e).is_none() {
            let c = assume!(log.log, controlled.get_component_mut(e));
            if c.by.is_some() {
                c.should_release = true;
                c.wanted = Some(Controller::Idle(0));
                continue;
            }
            debug!(log.log, "Entity free, idling"; "entity" => ?e, "course" => ?timetable.course);
            idle.add_component(e, Idle::new());
        }

        activity.add_component(e, Activity {
            day: day as u8,
            slot: activity_slot as u8,
        });
        timetable_start.started = true;
    }
});
