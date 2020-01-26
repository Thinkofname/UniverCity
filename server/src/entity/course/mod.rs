//! Course management and structures
//!
//! Courses are sets of lessons that students will have during
//! a period of time at the university.

use crate::prelude::*;

/*
    TODO:

    Lessons need a 'name'. This could be an id to a json file which
    controls how the lesson is run.

    Scripts shouldn't be attached to rooms anymore(? or at least simplified
    like idle scripts). Instead the lesson should control the room.

    Easier option is to just not to that and let rooms get the current lesson
    and maybe handle any extra stuff in scripts with a lookup table somewhere
*/

/// Handles loading lessons
pub struct LessonManager {
    lessons: FNVMap<ResourceKey<'static>, Lesson>,
    /// A list of known lesson groups
    pub groups: Vec<String>,
}
component!(LessonManager => Vec);

impl LessonManager {

    /// Creates a lesson manager preloaded with all listed lessons
    /// loaded.
    pub fn new(log: Logger, assets: &AssetManager) -> LessonManager {
        use std::io::Read;

        let mut lessons = FNVMap::default();
        let mut groups = FNVSet::default();

        for pack in assets.get_packs() {
            let mut styles = String::new();
            let mut res = if let Ok(res) = assets.open_from_pack(pack.borrow(), "lessons/lessons.list") {
                res
            } else {
                error!(log, "Missing lesson list {:?}", pack);
                continue;
            };
            assume!(log, res.read_to_string(&mut styles));
            for line in styles.lines() {
                let line = line.trim();
                // Skip empty lines/comments
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                // Support cross module loading
                let s_key = LazyResourceKey::parse(line)
                    .or_module(pack.borrow());

                let file = match assets.open_from_pack(s_key.module_key(), &format!("lessons/{}.json", s_key.resource())) {
                    Ok(v) => v,
                    Err(err) => {
                        error!(log, "Failed to load lesson: {:?}", s_key; "error" => ?err);
                        continue;
                    }
                };
                let info: LessonInfo = match serde_json::from_reader(file) {
                    Ok(v) => v,
                    Err(err) => {
                        error!(log, "Failed to load lesson: {:?}", s_key; "error" => ?err);
                        continue;
                    }
                };

                let lkey = s_key.into_owned();
                let lesson = Lesson {
                    key: lkey.clone(),
                    name: info.name,
                    group: info.group,
                    valid_rooms: info.valid_rooms
                        .into_iter()
                        .map(|v| LazyResourceKey::parse(&v)
                            .or_module(pack.borrow())
                            .into_owned()
                        )
                        .collect(),
                    valid_staff: info.valid_staff
                        .into_iter()
                        .map(|v| LazyResourceKey::parse(&v)
                            .or_module(pack.borrow())
                            .into_owned()
                        )
                        .collect(),
                    required_lessons: info.required_lessons,
                    description: info.description,
                };

                groups.insert(lesson.group.clone());
                lessons.insert(lkey, lesson);

            }
        }

        LessonManager {
            lessons,
            groups: groups.into_iter().collect(),
        }
    }

    /// Returns the lesson information for the given key if it exists
    #[inline]
    pub fn get<'a, 'b>(&'a self, key: ResourceKey<'b>) -> Option<&'a Lesson>
        where 'b: 'a
    {
        self.lessons.get(&key)
    }

    /// Returns an iterator over all lessons in a given group
    #[inline]
    pub fn lessons_in_group<'a>(&'a self, group: &'a str) -> impl Iterator<Item=&'a Lesson> {
        self.lessons.values()
            .filter(move |v| v.group == group)
    }
}

/// A unique id for a course
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Debug, DeltaEncode, Serialize, Deserialize)]
#[delta_always]
pub struct CourseId(pub u32);

/// Contains information about a course
#[derive(Clone)]
pub struct Course {
    /// A unique id for the course
    pub uid: CourseId,
    /// The name of the course
    pub name: String,
    /// The lesson group (science/art/etc)
    pub group: String,
    /// The cost to join this course
    pub cost: UniDollar,

    /// The timetable for this course
    pub timetable: [[CourseEntry; NUM_TIMETABLE_SLOTS]; 7],

    /// Whether this cause is due to be removed once everyone has
    /// finished it
    pub deprecated: bool,
}

fn book(booked: &mut Write<Booked>, e: ecs::Entity, day: usize, p: usize, course: Option<CourseId>) {
    let booked = if let Some(booked) = booked.get_component_mut(e) {
        booked
    } else {
        booked.add_component(e, Booked::default());
        booked.get_component_mut(e).expect("Missing after adding booked component")
    };

    booked.timetable[day][p] = course;
}

impl Course {
    /// Updates the about the state of the course (e.g. booking rooms/staff)
    pub fn init_world(&self, lrooms: &LevelRooms, entities: &mut ecs::Container) {
        entities.with(|
            _em: EntityManager,
            mut booked: Write<Booked>,
        | {
            for (idx_day, day) in self.timetable.iter().enumerate() {
                for (idx_p, p) in day.iter().enumerate() {
                    if let CourseEntry::Lesson{ref rooms, ..} = p {
                        for room in rooms {
                            book(&mut booked, room.staff, idx_day, idx_p, Some(self.uid));
                            if let Some(room) = lrooms.try_room_info(room.room) {
                                if !room.controller.is_invalid() {
                                    book(&mut booked, room.controller, idx_day, idx_p, Some(self.uid));
                                }
                            }
                        }
                    }
                }
            }
        });
    }

    /// Updates the about the state of the course (e.g. unbooking rooms/staff)
    pub fn deinit_world(&self, lrooms: &LevelRooms, entities: &mut ecs::Container) {
        entities.with(|
            _em: EntityManager,
            mut booked: Write<Booked>,
        | {
            self.deinit_world_raw(lrooms, &mut booked);
        });
    }
    /// Updates the about the state of the course (e.g. unbooking rooms/staff)
    pub fn deinit_world_raw(&self, lrooms: &LevelRooms, booked: &mut Write<Booked>,) {
        for (idx_day, day) in self.timetable.iter().enumerate() {
            for (idx_p, p) in day.iter().enumerate() {
                if let CourseEntry::Lesson{ref rooms, ..} = p {
                    for room in rooms {
                        book(booked, room.staff, idx_day, idx_p, None);
                        if let Some(room) = lrooms.try_room_info(room.room) {
                            if !room.controller.is_invalid() {
                                book(booked, room.controller, idx_day, idx_p, None);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Converts from a network course to a normal course
    pub fn from_network(snapshots: &snapshot::Snapshots, net: NetworkCourse) -> Option<Course> {
        fn entry_conv(snapshots: &snapshot::Snapshots, n: NetworkCourseEntry) -> Option<CourseEntry> {
            Some(match n {
                NetworkCourseEntry::Free => CourseEntry::Free,
                NetworkCourseEntry::Lesson{key, rooms} => CourseEntry::Lesson {
                    key: key,
                    rooms: rooms.0.into_iter()
                        .map(|v| Ok(LessonRoom {
                            room: v.room,
                            staff: snapshots.get_entity_by_id(v.staff).ok_or(())?,
                        }))
                        .collect::<Result<Vec<_>, ()>>().ok()?,
                }
            })
        }
        fn timetable_conv(snapshots: &snapshot::Snapshots, n: [NetworkCourseEntry; 4]) -> Option<[CourseEntry; 4]> {
            let [n0, n1, n2, n3] = n;
            Some([
                entry_conv(snapshots, n0)?,
                entry_conv(snapshots, n1)?,
                entry_conv(snapshots, n2)?,
                entry_conv(snapshots, n3)?,
            ])
        }
        let [t0, t1, t2, t3, t4, t5, t6] = net.timetable;
        Some(Course {
            uid: net.uid,
            name: net.name,
            group: net.group,
            cost: net.cost,
            timetable: [
                timetable_conv(snapshots, t0)?,
                timetable_conv(snapshots, t1)?,
                timetable_conv(snapshots, t2)?,
                timetable_conv(snapshots, t3)?,
                timetable_conv(snapshots, t4)?,
                timetable_conv(snapshots, t5)?,
                timetable_conv(snapshots, t6)?,
            ],
            deprecated: false,
        })
    }

    /// Updates this course using the information from the network course
    ///
    /// Fails if the group or uid doesn't match
    pub fn update_from_network(&mut self, snapshots: &snapshot::Snapshots, net: NetworkCourse) -> UResult<()> {
        // Safety checks
        if self.uid != net.uid {
            bail!("Wrong group");
        }
        if self.group != net.group {
            bail!("Mis-matched group");
        }

        let converted = Self::from_network(snapshots, net)
            .ok_or_else(|| ErrorKind::Msg("Invalid course".into()))?;
        self.name = converted.name;
        self.cost = converted.cost;

        // TODO: May be worth validing conflicts in scheduling here?
        self.timetable = converted.timetable;

        Ok(())
    }

    /// Converts from a normal course to a network course
    pub fn as_network(&self, ids: &Read<NetworkId>) -> Option<NetworkCourse> {
        fn entry_conv(ids: &Read<NetworkId>, n: &CourseEntry) -> Option<NetworkCourseEntry> {
            Some(match n {
                CourseEntry::Free => NetworkCourseEntry::Free,
                CourseEntry::Lesson{key, rooms} => NetworkCourseEntry::Lesson {
                    key: key.clone(),
                    rooms: delta_encode::AlwaysVec(rooms.iter()
                        .map(|v| Ok(NetworkLessonRoom {
                            room: v.room,
                            staff: ids.get_component(v.staff)
                                .map(|v| v.0)
                                .ok_or(())?,
                        }))
                        .collect::<Result<Vec<_>, ()>>().ok()?),
                }
            })
        }
        fn timetable_conv(ids: &Read<NetworkId>, n: &[CourseEntry; 4]) -> Option<[NetworkCourseEntry; 4]> {
            let [n0, n1, n2, n3] = n;
            Some([
                entry_conv(ids, n0)?,
                entry_conv(ids, n1)?,
                entry_conv(ids, n2)?,
                entry_conv(ids, n3)?,
            ])
        }
        let [t0, t1, t2, t3, t4, t5, t6] = &self.timetable;
        Some(NetworkCourse {
            uid: self.uid,
            name: self.name.clone(),
            group: self.group.clone(),
            cost: self.cost,
            timetable: [
                timetable_conv(ids, t0)?,
                timetable_conv(ids, t1)?,
                timetable_conv(ids, t2)?,
                timetable_conv(ids, t3)?,
                timetable_conv(ids, t4)?,
                timetable_conv(ids, t5)?,
                timetable_conv(ids, t6)?,
            ],
        })
    }
}

/// The information about a single period on a course
#[derive(Clone, Debug)]
pub enum CourseEntry {
    /// The period has a lesson
    Lesson {
        /// The key of the lesson for this slot
        key: ResourceKey<'static>,
        /// The possible rooms for this lesson
        rooms: Vec<LessonRoom>,
    },
    /// The period is free
    Free,
}

impl CourseEntry {
    /// Returns if this course entry is for the lesson with the given key
    pub fn is_lesson_type(&self, key: ResourceKey) -> bool {
        match self {
            CourseEntry::Lesson{key: okey, ..} => *okey == key,
            _ => false,
        }
    }
    /// Returns if this course entry is for a free period
    pub fn is_free(&self) -> bool {
        match *self {
            CourseEntry::Free => true,
            _ => false,
        }
    }
}

impl Default for CourseEntry {
    fn default() -> CourseEntry {
        CourseEntry::Free
    }
}

/// A room/staff pair for a lesson
#[derive(Clone, Debug)]
pub struct LessonRoom {
    /// The room for the lesson
    pub room: RoomId,
    /// The staff member for the room
    pub staff: Entity,
}

/// A network safe version of a course
#[derive(Clone, DeltaEncode, Debug)]
#[delta_always]
pub struct NetworkCourse {
    /// A unique id for the course
    pub uid: CourseId,
    /// The name of the course
    pub name: String,
    /// The lesson group (science/art/etc)
    pub group: String,
    /// The cost to join this course
    pub cost: UniDollar,

    /// The timetable for this course
    pub timetable: [[NetworkCourseEntry; NUM_TIMETABLE_SLOTS]; 7],
}

/// The information about a single period on a course
#[derive(Clone, DeltaEncode, Debug)]
#[delta_always]
pub enum NetworkCourseEntry {
    /// The period has a lesson
    Lesson {
        /// The key of the lesson for this slot
        key: ResourceKey<'static>,
        /// The possible rooms for this lesson
        rooms: delta_encode::AlwaysVec<NetworkLessonRoom>,
    },
    /// The period is free
    Free,
}

impl NetworkCourseEntry {
    /// Returns if this course entry is for the lesson with the given key
    pub fn is_lesson_type(&self, key: ResourceKey) -> bool {
        match self {
            NetworkCourseEntry::Lesson{key: okey, ..} => *okey == key,
            _ => false,
        }
    }
    /// Returns if this course entry is for a free period
    pub fn is_free(&self) -> bool {
        match *self {
            NetworkCourseEntry::Free => true,
            _ => false,
        }
    }
}


impl Default for NetworkCourseEntry {
    fn default() -> NetworkCourseEntry {
        NetworkCourseEntry::Free
    }
}

/// A room/staff pair for a lesson
#[derive(Clone, DeltaEncode, Debug)]
#[delta_always]
pub struct NetworkLessonRoom {
    /// The room for the lesson
    pub room: RoomId,
    /// The staff member for the room
    pub staff: u32,
}

/// A lesson definition
pub struct Lesson {
    /// The key for the lesson
    pub key: ResourceKey<'static>,
    /// Display name of the lesson
    pub name: String,
    /// The lesson group (science/art/etc)
    pub group: String,
    /// A list of rooms this lesson can take place in
    pub valid_rooms: Vec<ResourceKey<'static>>,
    /// A list of staff that can teach this lesson
    pub valid_staff: Vec<ResourceKey<'static>>,
    /// The number of times this lesson is required to been
    /// taken to fully pass it
    pub required_lessons: u32,
    /// A description of the lesson for the user interface
    pub description: String,
}

#[derive(Deserialize)]
struct LessonInfo {
    name: String,
    group: String,
    valid_rooms: Vec<String>,
    valid_staff: Vec<String>,
    required_lessons: u32,
    description: String,
}
