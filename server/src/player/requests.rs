use crate::prelude::*;
use delta_encode::AlwaysVec;
use std::sync::Arc;

/// Request whether the room is booked
#[derive(DeltaEncode)]
#[delta_always]
pub struct RoomBooked {
    /// The id of the room being requested
    pub room_id: room::Id,
}

/// Returns encoded information about whether a room is
/// booked
#[derive(DeltaEncode)]
#[delta_always]
pub struct RoomBookedReply {
    /// The id of the room being requested
    pub room_id: room::Id,
    /// If the room is booked or not
    pub booked: bool,
}

impl Requestable for RoomBooked {
    const ID: [u8; 4] = *b"robo";
    type Reply = RoomBookedReply;
}

/// Request the tooltip for a room owned by the player
#[derive(DeltaEncode)]
#[delta_always]
pub struct RoomDetails {
    /// The id of the room being requested
    pub room_id: room::Id,
}

/// Returns encoded information about a room to be decoded by a
/// script
#[derive(DeltaEncode)]
#[delta_always]
pub struct RoomDetailsReply {
    /// The id of the room being requested
    pub room_id: room::Id,
    /// The encoded script data for the room
    pub data: ScriptData,
}

impl Requestable for RoomDetails {
    const ID: [u8; 4] = *b"rode";
    type Reply = RoomDetailsReply;
}

/// Requests information on an entity
#[derive(DeltaEncode)]
#[delta_always]
pub struct EntityResults {
    /// The network id of the entity
    #[delta_bits = "20"]
    pub entity_id: u32,
}

/// The results of the entity request
#[derive(DeltaEncode)]
#[delta_always]
pub struct EntityResultsReply {
    /// The network id of the entity
    #[delta_bits = "20"]
    pub entity_id: u32,
    /// The entity's timetable if any
    pub timetable: Option<AlwaysVec<TimetableEntryState>>,
    /// The entity's grades for completed lessons
    pub grades: AlwaysVec<NamedGradeEntry>,
    /// The entity's current stats
    pub stats: AlwaysVec<f32>,
    /// The entities debug information
    #[cfg(feature = "debugutil")]
    pub entity_debug: String,
}

/// An entry in the list of grades
#[derive(DeltaEncode, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct NamedGradeEntry {
    /// The course name
    pub course_name: String,
    /// The final grade
    pub grade: Grade,
}

/// The state of a single timetable period
#[derive(DeltaEncode, Clone, Copy, PartialEq, Eq, Debug)]
#[delta_always]
pub enum TimetableEntryState {
    /// Free period
    Free,
    /// Uncompleted lesson
    Lesson,
    /// Completed lesson
    Completed(Grade),
}

impl Requestable for EntityResults {
    const ID: [u8; 4] = *b"enre";
    type Reply = EntityResultsReply;
}

/// Requests information for a staff member that can be hired
#[derive(DeltaEncode)]
#[delta_always]
pub struct StaffPage {
    /// The type of staff member being requested
    pub staff_key: ResourceKey<'static>,
    /// The page number of the staff member.
    ///
    /// The first page will be returned if this is out of bounds
    pub page: u8,
}

/// Information about a staff member that can be hired
#[derive(DeltaEncode)]
#[delta_always]
pub struct StaffPageReply {
    /// The number of staff pages for this type
    pub num_pages: u8,
    /// The info of the staff member of the requested page
    /// if any.
    pub info: Option<StaffPageInfo>,
}

/// Information about a staff member that can be hired
#[derive(DeltaEncode, Clone)]
#[delta_always]
pub struct StaffPageInfo {
    /// A unique id for this staff member
    pub unique_id: u32,
    /// The page number of this entity.
    ///
    /// Should be equal to the one requested but may differ if
    /// the request was out of bounds.
    pub page: u8,
    /// The variant of the entity
    pub variant: u16,
    /// The entity's first name
    pub first_name: Arc<str>,
    /// The entity's surname
    pub surname: Arc<str>,
    /// Generated entity description
    pub description: String,
    /// The entity's stats if hired
    pub stats: [f32; Stats::MAX],
    /// The price to hire the entity
    pub hire_price: UniDollar,
}

impl Requestable for StaffPage {
    const ID: [u8; 4] = *b"stpa";
    type Reply = StaffPageReply;
}

/// Requests the list of current courses
#[derive(DeltaEncode)]
#[delta_always]
pub struct CourseList {}

/// The list of courses that currently exist
#[derive(DeltaEncode)]
#[delta_always]
pub struct CourseListReply {
    /// The courses
    pub courses: AlwaysVec<CourseOverview>,
}

/// A small overview of a course
#[derive(DeltaEncode)]
#[delta_always]
pub struct CourseOverview {
    /// A unique id for the course
    pub uid: course::CourseId,
    /// The name of the course
    pub name: String,
    /// The number of students on the course
    pub students: u32,
    /// The average grade of students on the course
    pub average_grade: Grade,
    /// Info of any problems on the course
    pub problems: String,
    /// The cost of the course
    pub cost: UniDollar,
    /// The timetable for this course
    pub timetable: [[bool; 4]; 7],
    /// Whether this cause is due to be removed once everyone has
    /// finished it
    pub deprecated: bool,
}

impl Requestable for CourseList {
    const ID: [u8; 4] = *b"coli";
    type Reply = CourseListReply;
}

/// Requests the full information about a course
#[derive(DeltaEncode)]
#[delta_always]
pub struct CourseInfo {
    /// The unique id of the course
    pub uid: course::CourseId,
}

/// The requested information about the course
#[derive(DeltaEncode)]
#[delta_always]
pub struct CourseInfoReply {
    /// The requested course information
    pub course: course::NetworkCourse,
}

impl Requestable for CourseInfo {
    const ID: [u8; 4] = *b"coin";
    type Reply = CourseInfoReply;
}

/// Requests valid staff/rooms for a lesson
#[derive(DeltaEncode)]
#[delta_always]
pub struct LessonValidOptions {
    /// The course asking for the list or 0 for a new
    /// course
    pub course: course::CourseId,
    /// The key of the lesson
    pub key: ResourceKey<'static>,
    /// The day of the lesson
    pub day: u8,
    /// The period of the lesson
    pub period: u8,
}

/// The requested staff/rooms for the lesson
#[derive(DeltaEncode)]
#[delta_always]
pub struct LessonValidOptionsReply {
    /// List of valid staff
    pub staff: AlwaysVec<NetworkId>,
    /// List of valid rooms
    pub rooms: AlwaysVec<room::Id>,
}

impl Requestable for LessonValidOptions {
    const ID: [u8; 4] = *b"levo";
    type Reply = LessonValidOptionsReply;
}
