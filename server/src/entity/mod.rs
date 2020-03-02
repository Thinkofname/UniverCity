//! Entity management and structures

pub mod course;
pub mod free_roam;
mod info;
pub mod pathfind;
pub mod snapshot;

mod timetable;

pub(crate) use self::timetable::{
    generate_time_table, Activity, TimeTable, TimeTableCompleted, TimeTableStart,
};
pub use self::timetable::{LESSON_LENGTH, NUM_TIMETABLE_SLOTS};

mod sys;
pub use self::sys::{
    follow_rot, follow_sys, request_entity_now, GotoRoom, RoomController, StudentController,
};

mod stats;
pub use self::stats::*;

pub use self::info::*;
pub use self::sys::EntityDispatcher;

use std::sync::Arc;

use crate::assets;
use crate::common;
use crate::ecs::{self, closure_system, EntityManager};
use crate::level;
use crate::level::room;
use crate::prelude::*;
use lua::{Coroutine, Lua, Ref, Table};

/// Registers components required by the server and the client
pub fn register_components(c: &mut ecs::Container) {
    pathfind::register_components(c);

    c.register_component::<Position>();
    c.register_component::<Size>();
    c.register_component::<Rotation>();
    c.register_component::<TargetPosition>();
    c.register_component::<TargetRotation>();
    c.register_component::<CatchupBuffer>();
    c.register_component::<MovementSpeed>();
    c.register_component::<LagMovementAdjust>();
    c.register_component::<NetworkId>();

    c.register_component::<Frozen>();
    c.register_component::<DoesntExist>();
    c.register_component::<SelectedEntity>();

    c.register_component::<Object>();
    c.register_component::<Living>();
    c.register_component::<InvalidPlacement>();
    c.register_component::<Door>();

    c.register_component::<RoomController>();
    c.register_component::<GotoRoom>();
    c.register_component::<RoomOwned>();
    c.register_component::<StateData>();
    c.register_component::<Owned>();
    c.register_component::<StudentController>();
    c.register_component::<timetable::TimeTable>();
    c.register_component::<Grades>();

    c.register_component::<level::LevelTiles>();
    c.register_component::<level::LevelRooms>();
    c.register_component::<sys::EntityDispatcher>();
    c.register_component::<DayTick>();
    c.register_component::<course::LessonManager>();

    c.register_component::<Lifetime>();
    c.register_component::<Velocity>();
    c.register_component::<IconEmote>();
    c.register_component::<Follow>();
    c.register_component::<Paid>();
    c.register_component::<crate::PlayerInfoMap>();
    c.register_component::<Idle>();
    c.register_component::<Tints>();
    c.register_component::<ForceLeave>();
    c.register_component::<Quitting>();
    c.register_component::<assets::AssetManager>();

    c.register_component::<free_roam::FreeRoam>();
    c.register_component::<LuaRoamEntityProperties>();
    c.register_component::<LuaRoomProperties>();
    c.register_component::<crate::script_room::LuaEntityRef>();
    c.register_component::<CLogger>();
    c.register_component::<RequiresRoom>();

    c.register_component::<AutoRest>();
    c.register_component::<Controlled>();
    c.register_component::<Booked>();
    c.register_component::<Money>();
    c.register_component::<timetable::Activity>();
    c.register_component::<timetable::TimeTableStart>();
    c.register_component::<timetable::TimeTableCompleted>();
}

/// Registers systems required by the server and the client
pub fn register_systems(sys: &mut ecs::Systems) {
    pathfind::register_systems(sys);
    sys.add(sys::lifetime_sys);
    sys.add(sys::velocity_sys);
    sys.add(sys::tick_emotes);
    sys.add(sys::require_room);
}

/// Registers systems required by the server only
pub fn register_server_systems(sys: &mut ecs::Systems) {
    sys.add(pathfind::tick_pathfinder);
    sys.add(sys::no_lerp_target_pos);
    sys.add(sys::no_lerp_target_rot);
    sys.add(sys::manage_room);
    sys.add(sys::manage_entity_dispatch);
    sys.add(sys::get_timetable);
    sys.add(sys::walk_to_room);
    sys.add(sys::open_door_server);
    sys.add(sys::leave_room);
    sys.add(timetable::manage_time_table);
    sys.add(follow_sys);
    sys.add(follow_rot);
    sys.add(sys::tick_student_stats);
    sys.add(sys::tick_professor_stats);
    sys.add(sys::rest_staff);
    sys.add(sys::pay_staff);
    sys.add(sys::quit_sys);
}

/// Handles creating entities in a server/client indepentant way
pub trait EntityCreator {
    /// The types used for lua scripting
    type ScriptTypes: script::ScriptTypes;
    /// Creates an entity which is a static model
    fn static_model(
        c: &mut ecs::Container,
        model: assets::ResourceKey<'_>,
        texture: Option<assets::ResourceKey<'_>>,
    ) -> ecs::Entity;
    /// Creates an entity which is a animated model
    fn animated_model(
        c: &mut ecs::Container,
        model: assets::ResourceKey<'_>,
        texture: Option<assets::ResourceKey<'_>>,
        animation_set: common::AnimationSet,
        animation: &str,
    ) -> ecs::Entity;
}

/// Handles creating entities for the server
pub enum ServerEntityCreator {}

impl EntityCreator for ServerEntityCreator {
    type ScriptTypes = crate::script_room::Types;

    fn static_model(
        c: &mut ecs::Container,
        _model: assets::ResourceKey<'_>,
        _texture: Option<assets::ResourceKey<'_>>,
    ) -> ecs::Entity {
        let e = c.new_entity();
        c.add_component(
            e,
            Position {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        );
        c.add_component(
            e,
            Rotation {
                rotation: Angle::new(0.0),
            },
        );
        e
    }
    fn animated_model(
        c: &mut ecs::Container,
        _model: assets::ResourceKey<'_>,
        _texture: Option<assets::ResourceKey<'_>>,
        _animation_set: common::AnimationSet,
        _animation: &str,
    ) -> ecs::Entity {
        use std::f32::consts::PI;
        let e = c.new_entity();
        c.add_component(
            e,
            Position {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        );
        c.add_component(
            e,
            Rotation {
                rotation: Angle::new(PI * 0.5),
            },
        );
        e
    }
}

component!(level::LevelTiles => const World);
component!(level::LevelRooms => const World);
component!(sys::EntityDispatcher => mut World);
component!(assets::AssetManager => const World);

/// Contains the current time of day in ticks
#[derive(Debug, Serialize, Deserialize, DeltaEncode, Clone, Copy, Default)]
pub struct DayTick {
    /// Current time of day in ticks
    #[delta_diff]
    pub current_tick: i32,
    /// The current day
    #[delta_diff]
    pub day: u32,
    /// Global increasing timer in ticks
    #[delta_diff]
    pub time: u32,
}
component!(DayTick => const World);

/// Access to the logger
pub struct CLogger {
    /// The logger
    pub log: Logger,
}
component!(CLogger => Vec);

/// Contains the network id of an entity
#[derive(Debug, Clone, Copy, DeltaEncode)]
#[delta_always]
pub struct NetworkId(pub u32);
component!(NetworkId => Vec);

/// Contains the position of an entity
#[derive(Debug, Clone)]
pub struct Position {
    /// Position on the x axis
    pub x: f32,
    /// Position on the y axis
    pub y: f32,
    /// Position on the z axis
    pub z: f32,
}
component!(Position => Vec);

/// Contains the target position of an entity
///
/// The entity will be moved towards the target
/// over the specified number ticks every frame
/// allowing for smooth movement.
#[derive(Debug)]
pub struct TargetPosition {
    /// Position on the x axis
    pub x: f32,
    /// Position on the y axis
    pub y: f32,
    /// Position on the z axis
    pub z: f32,
    /// The number of ticks to smooth over
    pub ticks: f64,
}
component!(TargetPosition => Vec);

/// Used to handle non-integer movements
pub struct CatchupBuffer {
    /// The number of ticks overran
    pub catchup_time: f64,
}
component!(CatchupBuffer => Vec);

/// Contains the size of an entity
pub struct Size {
    /// The width (x axis)
    pub width: f32,
    /// The height (y axis)
    pub height: f32,
    /// The depth (z axis)
    pub depth: f32,
}
component!(Size => Vec);

/// Contains the rotation (facing direction) of an entity
pub struct Rotation {
    /// Rotation in radians around the y axis
    pub rotation: Angle,
}
component!(Rotation => Vec);

/// Contains the target rotation (facing direction) of an
/// entity
///
/// The entity will be rotated towards the target
/// over the specified number ticks every frame
/// allowing for smooth rotation.
pub struct TargetRotation {
    /// Rotation in radians around the y axis
    pub rotation: Angle,
    /// The number of ticks to smooth over
    pub ticks: f64,
}
component!(TargetRotation => Vec);

/// Marks an entity as being a/part of a object
pub struct Object {
    /// The object key
    pub key: assets::ResourceKey<'static>,
    /// Optional type tag for the object
    pub ty: Option<String>,
}
component!(Object => Map);

/// Marks an object as having an invalid placement position
///
/// These objects should be treated as not existing except for
/// displaying.
#[derive(Default)]
pub struct InvalidPlacement;
component!(InvalidPlacement => Marker);

/// Marks an object as a door that can be opened to pass through
pub struct Door {
    /// If the door is open this is greater than
    /// 0. The value will be the number of ticks
    /// the door should be open for.
    pub open: i32,
    /// Used to handle animations
    pub was_open: bool,
    /// How long the door has been open for
    pub open_time: i32,
}
component!(Door => Map);

impl Door {
    /// Creates a door component with the initial state
    pub fn new(open: bool) -> Door {
        Door {
            open: if open { 20 } else { 0 },
            was_open: open,
            open_time: 0,
        }
    }

    /// Opens the door for 1 second
    ///
    /// If the door is already open then this will
    /// reset the timer back to 1 second
    pub fn open(&mut self) {
        self.open = 20;
    }
}

/// Marks an object as being alive (with walking/idle/etc
/// animations)
#[derive(Debug)]
pub struct Living {
    /// The entity key
    pub key: assets::ResourceKey<'static>,
    /// The variant type of the entity
    pub variant: usize,
    /// The first and second name of the entity
    pub name: (Arc<str>, Arc<str>),
}
component!(Living => Vec);

/// Controls the movement speed of an entity
pub struct MovementSpeed {
    /// The current movement speed of the entity.
    /// May be modified to handle lag.
    ///
    ///(1.0 == 1 tile) per a tick
    pub speed: f32,
    /// The movement speed of the entity
    ///
    ///(1.0 == 1 tile) per a tick
    pub base_speed: f32,
}
component!(MovementSpeed => Map);

/// Controls the movement speed of an entity
pub struct LagMovementAdjust {
    /// The adjustment level of the entity's speed.
    ///
    /// With no lag this would be 1.0. Multiplied
    /// speed to handle the client falling behind.
    pub adjustment: f32,
}
component!(LagMovementAdjust => Map);

/// Marks an entity as frozen. This should prevent
/// all movement and AI from acting on the entity.
#[derive(Default)]
pub struct Frozen;
component!(Frozen => Marker);

/// Marks an entity as not existing yet (aka being placed
/// for the first time)
#[derive(Default)]
pub struct DoesntExist;
component!(DoesntExist => Marker);

/// Used on entities selected/held by a player.
pub struct SelectedEntity {
    /// The player id of the holder
    pub holder: player::Id,
}
component!(SelectedEntity => Map);

/// Marks an entity as controlled by a room
#[derive(Debug)]
pub struct RoomOwned {
    /// The id of the owning room
    pub room_id: room::Id,
    /// Whether the script controlling the room should release the
    /// entity as soon as the room is inactive
    pub should_release_inactive: bool,
    /// Whether the room is activity using this
    /// entity as a staff member
    pub active: bool,
}
component!(RoomOwned => Vec);

impl RoomOwned {
    /// Creates a new `RoomOwned` component
    pub fn new(room_id: room::Id) -> RoomOwned {
        RoomOwned {
            room_id,
            should_release_inactive: false,
            active: true,
        }
    }
}

/// Serialized script state information
pub struct StateData {
    /// Serialized script state information
    pub data: Arc<bitio::Writer<Vec<u8>>>,
    /// The controller that the data is for
    pub controller: Option<Controller>,
}
component!(StateData => Vec);

impl StateData {
    /// Creates a new `StateData` component
    pub fn new(controller: Option<Controller>) -> StateData {
        StateData {
            data: Arc::new(bitio::Writer::new(Vec::new())),
            controller,
        }
    }
}

/// Marks the entity of requiring the room that owns it to exist.
///
/// Destroys itself if the room is missing
#[derive(Default)]
pub struct RequiresRoom;
component!(RequiresRoom => Marker);

/// Marks an entity as owned by a player
#[derive(Debug)]
pub struct Owned {
    /// The id of the player that owns this entity
    pub player_id: player::Id,
}
component!(Owned => Vec);

/// Limits the lifetime of an entity
pub struct Lifetime {
    /// The remaining lifetime of this entity
    pub time: i32,
    /// The max (initial) lifetime of this entity
    pub max_life: i32,
}
component!(Lifetime => Map);

impl Lifetime {
    /// Creates a lifetime limit component
    pub fn new(time: i32) -> Lifetime {
        Lifetime {
            time,
            max_life: time,
        }
    }
}

/// Applies velocity to the entity's position
pub struct Velocity {
    /// The velocity to apply
    pub velocity: (f32, f32, f32),
    /// The amount to reduce the velocity by
    /// each tick
    pub friction: (f32, f32, f32),
}
component!(Velocity => Map);

/// Contains emotes that the entity is currently displaying
pub struct IconEmote {
    /// List of emotes to display
    pub icons: Vec<(u8, Emote)>,
    /// Time spent displaying the current emote
    pub time: i32,
    /// The current emote
    ///
    /// Used by the client
    pub current: Option<Emote>,
    next_id: u8,
}
component!(IconEmote => Map);

impl IconEmote {
    pub(super) fn from_existing<I>(list: I) -> IconEmote
    where
        I: Iterator<Item = (u8, Emote)>,
    {
        IconEmote {
            icons: list.collect(),
            time: 0,
            current: None,
            next_id: 0,
        }
    }
    /// Adds the pass emote to the list.
    ///
    /// Creates the list if it doesn't already exist
    pub fn add(ie: &mut ecs::Write<IconEmote>, entity: ecs::Entity, e: Emote) {
        if let Some(ie) = ie.get_component_mut(entity) {
            ie.icons.push((ie.next_id, e));
            ie.next_id = ie.next_id.wrapping_add(1);
            return;
        }
        ie.add_component(
            entity,
            IconEmote {
                icons: vec![(0, e)],
                time: 0,
                current: None,
                next_id: 1,
            },
        );
    }
}

/// An emote that an entity can display above its head
#[derive(Copy, Clone, PartialEq, Eq, Hash, DeltaEncode)]
pub enum Emote {
    /// Displays confusion above the entity's head
    Confused,
    /// Displays that the entity paid for something
    Paid,
}

/// Causes the entity to follow the target
/// (ignoring the y axis)
pub struct Follow {
    /// The entity to follow
    pub target: ecs::Entity,
    /// The offset from the target's position this
    /// entity should be placed at
    pub offset: (f32, f32, f32),
}
component!(Follow => Map);

/// An entity that requires payment
pub struct Paid {
    /// The cost of this entity every term
    pub cost: UniDollar,
    /// The cost this entity wants, may cause
    /// them to ask for a raise if the difference is too high
    pub wanted_cost: UniDollar,
    /// The last time this entity was paid
    pub last_payment: Option<u32>,
}
component!(Paid => Map);

/// An entity that is idle
#[derive(Debug)]
pub struct Idle {
    /// How long the entity has spent idling
    pub total_idle_time: i32,
    /// The current choice being run
    pub current_choice: Option<usize>,
    /// Whether a script has released this entity
    pub released: bool,
}
component!(Idle => Vec);

impl Idle {
    /// Creates a new idle component
    pub fn new() -> Idle {
        Idle {
            total_idle_time: 0,
            current_choice: None,
            released: false,
        }
    }
}

/// Marks an entity as being controlled by something
#[derive(Debug)]
pub struct Controlled {
    /// The current controller
    pub by: Option<Controller>,
    /// The controller that wishes to take over
    pub wanted: Option<Controller>,
    /// Whether the current controller should release
    /// the entity
    pub should_release: bool,
}
component!(Controlled => Vec);

impl Controlled {
    /// Creates a new idle component
    pub fn new() -> Controlled {
        Controlled {
            by: None,
            wanted: None,
            should_release: false,
        }
    }
    /// Creates a new idle component
    pub fn new_by(by: Controller) -> Controlled {
        Controlled {
            by: Some(by),
            wanted: None,
            should_release: false,
        }
    }
}

/// Types of controller, in order of priority
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum Controller {
    /// Idle script
    Idle(usize),
    /// Room script
    Room(RoomId),
    /// Free roam script
    FreeRoam,
    /// A quitting entity
    Quit,
}

impl Controller {
    /// Returns whether this controller is for an idle task
    #[inline]
    pub fn is_idle(self) -> bool {
        match self {
            Controller::Idle(_) => true,
            _ => false,
        }
    }
    /// Returns whether this controller is for an room task
    #[inline]
    pub fn is_room(self) -> bool {
        match self {
            Controller::Room(_) => true,
            _ => false,
        }
    }
}

/// List of tints for a model
#[derive(Debug)]
pub struct Tints {
    /// Tint per a part
    pub tints: Vec<(u8, u8, u8, u8)>,
}
component!(Tints => Map);

/// Forces the entity to leave the room
/// even if the room normally would allow
/// them to stay
pub struct ForceLeave {
    /// The id of the room to leave
    pub room_id: room::Id,
}
component!(ForceLeave => Map);

/// Marks an entity as quitting.
#[derive(Default)]
pub struct Quitting;
component!(Quitting => Marker);

/// Tracks a student's grades
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Grades {
    /// The grades the student has currently earned
    pub grades: Vec<GradeEntry>,
    /// The grades the student has for their current classes
    pub timetable_grades: [[Option<Grade>; 4]; 7],
}
component!(Grades => Vec);

/// An entry in the list of grades
#[derive(DeltaEncode, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct GradeEntry {
    /// The course
    pub course: course::CourseId,
    /// The final grade
    pub grade: Grade,
}

impl Grades {
    fn new() -> Grades {
        Grades {
            grades: Vec::new(),
            timetable_grades: Default::default(),
        }
    }
}

/// A grade for a class
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize, DeltaEncode,
)]
pub enum Grade {
    /// The highest grade a student can get
    ///
    /// Passing grade
    A,
    /// The second highest grade a student can get
    ///
    /// Passing grade
    B,
    /// The third highest grade a student can get
    ///
    /// Passing grade
    C,
    /// The forth highest grade a student can get
    ///
    /// Passing grade (barely)
    D,
    /// The second worst grade a student can get
    ///
    /// Failing grade
    E,
    /// The worst grade a student can get
    ///
    /// Failing grade
    F,
}

impl Grade {
    /// Returns a displayable string of this grade
    pub fn as_str(self) -> &'static str {
        match self {
            Grade::A => "A",
            Grade::B => "B",
            Grade::C => "C",
            Grade::D => "D",
            Grade::E => "E",
            Grade::F => "F",
        }
    }

    /// Returns an index for the given grade
    pub fn as_index(self) -> usize {
        match self {
            Grade::A => 0,
            Grade::B => 1,
            Grade::C => 2,
            Grade::D => 3,
            Grade::E => 4,
            Grade::F => 5,
        }
    }
}

/// Script properties for an entity not attached to a room
pub struct LuaRoamEntityProperties {
    /// The coroutine running this entity
    pub coroutine: Option<Ref<Coroutine>>,
}
component!(LuaRoamEntityProperties => Map);

impl LuaRoamEntityProperties {
    /// Creates a new set of properties for an entity in the given room
    pub fn new() -> LuaRoamEntityProperties {
        LuaRoamEntityProperties { coroutine: None }
    }

    /// Returns the room properties for the entity.
    ///
    /// If the entity has no properties or the ones it has
    /// are for another room this create a new set.
    pub fn get_or_create(props: &mut ecs::Write<Self>, e: Entity) -> Option<Ref<Coroutine>> {
        if let Some(props) = props.get_component(e) {
            return props.coroutine.clone();
        }
        let p = LuaRoamEntityProperties::new();
        props.add_component(e, p);
        None
    }
}

/// Script properties for room
pub struct LuaRoomProperties {
    /// The lua properties
    pub properties: Ref<Table>,
    /// Properties for objects
    pub object_properties: Vec<Option<Ref<Table>>>,
}
component!(LuaRoomProperties => Map);

impl LuaRoomProperties {
    /// Creates a new set of properties for an entity in the given room
    pub fn new(lua: &Lua) -> LuaRoomProperties {
        LuaRoomProperties {
            properties: Ref::new_table(lua),
            object_properties: Vec::new(),
        }
    }

    /// Returns the room properties for the entity.
    ///
    /// If the entity has no properties or the ones it has
    /// are for another room this create a new set.
    pub fn get_or_create(props: &mut ecs::Write<Self>, lua: &Lua, e: Entity) -> Ref<Table> {
        if let Some(props) = props.get_component(e) {
            return props.properties.clone();
        }
        let p = LuaRoomProperties::new(lua);
        let tbl = p.properties.clone();
        props.add_component(e, p);
        tbl
    }
}

/// Marks the member of staff as being automatically
/// made to rest when idle
#[derive(Default)]
pub struct AutoRest;
component!(AutoRest => Marker);

/// Contains information about when an entity is booked for a
/// lesson
#[derive(Default)]
pub struct Booked {
    /// Booking information for every period of a day, every day
    /// of the week
    pub timetable: [[Option<course::CourseId>; 4]; 7],
}
component!(Booked => Vec);

/// How much money the entity owns
pub struct Money {
    /// The amount of money the entity currently has
    pub money: UniDollar,
}
component!(Money => Vec);
