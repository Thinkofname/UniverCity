//! Entity pathfinding components and systems

use std::cmp;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, Weak};
use std::time;

#[cfg(feature = "debugutil")]
use png;

use super::*;
use crate::ecs::{self as ecs, closure_system, Read, Write};
use crate::level;
use crate::util::{Direction, FNVMap, FNVSet, Location};

/// Registers components required by this module
pub fn register_components(c: &mut ecs::Container) {
    c.register_component::<Target>();
    c.register_component::<PathInfo>();
    c.register_component::<TargetTime>();
    c.register_component::<TargetFacing>();
    c.register_component::<Pathfinder>();
}

/// Registers systems required by this module
pub fn register_systems(sys: &mut ecs::Systems) {
    sys.add(unstuck_entity);
    sys.add(compute_path);
    sys.add(speedup_movement);
}

/// Manages creating paths whilst trying to limit
/// the amount of time spent blocked by their creation
pub struct Pathfinder {
    requests: VecDeque<Weak<PathJob>>,
    limit: time::Duration,
}
component!(Pathfinder => mut World);

struct PathJob {
    start: (f32, f32),
    end: (f32, f32),
    path: Mutex<PathResult>,
    // Debug helper
    _required: bool,
}

/// The state of a path
pub enum PathResult {
    /// The path is completed and ready for use
    Ok(PathInfo),
    /// The path failed to compute
    Failed,
    /// The path is still being processed
    Waiting,
}

impl Pathfinder {
    /// Creates a new pathfinder with the given time limit
    pub fn new(limit: time::Duration) -> Pathfinder {
        Pathfinder {
            requests: VecDeque::new(),
            limit,
        }
    }

    /// Requests a path from the start position to the end position
    pub fn create_path(
        &mut self,
        start: (f32, f32),
        end: (f32, f32),
        required: bool,
    ) -> PathRequest {
        let req = Arc::new(PathJob {
            start,
            end,
            path: Mutex::new(PathResult::Waiting),
            _required: required,
        });
        self.requests.push_back(Arc::downgrade(&req));
        PathRequest { job: req }
    }
}

closure_system!(
    /// System for updating the pathfinder
    pub fn tick_pathfinder(
        _em: ecs::EntityManager<'_>,
        log: Read<CLogger>,
        tiles: Read<level::LevelTiles>,
        rooms: Read<level::LevelRooms>,
        mut pathfinder: Write<Pathfinder>,
    ) {
        use rayon;
        use rayon::prelude::*;
        use std::cmp::min;

        let log = log.get_component(Container::WORLD).expect("Missing logger");
        let tiles = assume!(log.log, tiles.get_component(Container::WORLD));
        let rooms = assume!(log.log, rooms.get_component(Container::WORLD));
        let pathfinder = assume!(log.log, pathfinder.get_component_mut(Container::WORLD));

        let start = time::Instant::now();

        while start.elapsed() < pathfinder.limit && !pathfinder.requests.is_empty() {
            pathfinder
                .requests
                .par_iter_mut()
                .with_min_len(1)
                .with_max_len(1)
                .take(rayon::current_num_threads())
                .filter_map(|v| v.upgrade())
                .for_each(|job| {
                    let mut path = assume!(log.log, job.path.lock());
                    *path = create_path(&log.log, tiles, rooms, job.start, job.end, job._required);
                });
            let len = pathfinder.requests.len();
            pathfinder
                .requests
                .drain(..min(len, rayon::current_num_threads()));
        }

        #[cfg(feature = "debugutil")]
        {
            if !pathfinder.requests.is_empty() {
                debug!(
                    log.log,
                    "Path queue size remaining: {}",
                    pathfinder.requests.len()
                );
            }
        }
    }
);

/// A requested path, may contain the requested path
pub struct PathRequest {
    job: Arc<PathJob>,
}
impl PathRequest {
    /// Takes the path if completed, otherwise returns `None`
    pub fn take_path(&mut self) -> PathResult {
        use std::mem;
        let mut path = self
            .job
            .path
            .lock()
            .expect("Failed to get the path request lock");
        mem::replace(&mut *path, PathResult::Waiting)
    }
}

closure_system!(
    fn unstuck_entity(
        em: ecs::EntityManager<'_>,
        log: Read<CLogger>,
        tiles: Read<level::LevelTiles>,
        rooms: Read<level::LevelRooms>,
        position: Read<Position>,
        mut target_pos: Write<TargetPosition>,
        frozen: Read<Frozen>,
        living: Read<Living>,
    ) {
        let world = Container::WORLD;
        let log = log.get_component(Container::WORLD).expect("Missing logger");
        let tiles = assume!(log.log, tiles.get_component(world));
        let rooms = assume!(log.log, rooms.get_component(world));

        for (e, pos) in em.group_mask(&position, |m| {
            m.and(&living).and_not(&frozen).and_not(&target_pos)
        }) {
            if !level::can_visit(tiles, rooms, (pos.x * 4.0) as usize, (pos.z * 4.0) as usize) {
                target_pos.add_component(
                    e,
                    TargetPosition {
                        x: pos.x,
                        y: pos.y,
                        z: pos.z - 0.2,
                        ticks: 5.0,
                    },
                );
            }
        }
    }
);

// TODO: Make a normal system when possible
/// Entity system used to travel paths
pub fn travel_path(
    em: &ecs::EntityManager<'_>,
    log: &Read<CLogger>,
    tiles: &Read<level::LevelTiles>,
    rooms: &Read<level::LevelRooms>,
    info: &mut Write<PathInfo>,
    speed: &mut Write<MovementSpeed>,
    position: &mut Write<Position>,
    target_pos: &mut Write<TargetPosition>,
    target_rotation: &mut Write<TargetRotation>,
    door: &mut Write<Door>,
    target: &mut Write<Target>,
    adjust: &Read<LagMovementAdjust>,
) {
    let world = Container::WORLD;
    let log = log.get_component(Container::WORLD).expect("Missing logger");
    let tiles = assume!(log.log, tiles.get_component(world));
    let rooms = assume!(log.log, rooms.get_component(world));

    'entities: for (e, (pos, speed)) in em.group_mask((position, speed), |m| m.and(info)) {
        // Check if ready for another target
        let remove = if target_pos.get_component(e).is_none() {
            let info = assume!(log.log, info.get_component_mut(e));
            let adjust = adjust.get_component(e).map_or(1.0, |v| v.adjustment);

            if info.waiting_for_door.is_none() && !info.nodes.is_empty() {
                info.nodes.remove(0);
            }
            if info.waiting_for_door.is_none() && info.nodes.is_empty() {
                if let Some(end) = info.end_rotation {
                    target_rotation.add_component(
                        e,
                        TargetRotation {
                            rotation: end,
                            ticks: 8.0,
                        },
                    );
                }
                true
            } else {
                let next = &info.nodes[0];

                // Check for a door we need to open first
                let t_pos = if let Some(dpos) = info.waiting_for_door {
                    Location::new(dpos.0 as i32, dpos.1 as i32)
                } else {
                    Location::new(pos.x as i32, pos.z as i32)
                };
                let dir =
                    Direction::try_from_offset(next.x as i32 - t_pos.x, next.z as i32 - t_pos.y);
                let remove = if !level::can_visit(
                    tiles,
                    rooms,
                    (pos.x * 4.0) as usize,
                    (pos.z * 4.0) as usize,
                ) || !level::can_visit(
                    tiles,
                    rooms,
                    (next.x * 4.0) as usize,
                    (next.z * 4.0) as usize,
                ) {
                    // We are stuck, quit following this path
                    debug!(log.log, "Stuck trying to follow path, quiting the path"; "entity" => ?e);
                    let end = info.nodes.last().unwrap_or(next);
                    if target.get_component(e).is_none() {
                        target.add_component(e, Target::new(end.x, end.z));
                    }
                    true
                } else if let Some(dir) = dir {
                    match tiles.get_wall_info(t_pos, dir).map(|info| info.flag) {
                        Some(level::TileWallFlag::Door) => {
                            let to_pos = t_pos.shift(dir);
                            let room_a = level::Level::get_room_at(tiles, rooms, t_pos);
                            let room_b = level::Level::get_room_at(tiles, rooms, to_pos);
                            let same =
                                room_a.is_some() && room_a.map(|v| v.id) == room_b.map(|v| v.id);
                            let door_e = room_a
                                .iter()
                                .chain(room_b.iter())
                                // Skip one of the rooms if they both point
                                // to the same one
                                .skip(if same { 1 } else { 0 })
                                .flat_map(|v| &v.objects)
                                .filter_map(|v| v.as_ref())
                                .filter(|v| {
                                    for action in &v.0.actions.0 {
                                        if let level::ObjectPlacementAction::WallFlag {
                                            location,
                                            direction,
                                            flag: level::object::WallPlacementFlag::Door,
                                        } = *action
                                        {
                                            if (location == t_pos && direction == dir)
                                                || (location == to_pos
                                                    && direction == dir.reverse())
                                            {
                                                return true;
                                            }
                                        }
                                    }
                                    false
                                })
                                .map(|v| &v.1)
                                .flat_map(|v| v.get_entities())
                                // Search for entities tagged as doors
                                .find(|v| door.get_component(*v).is_some());
                            if let Some(door_e) = door_e {
                                let door = assume!(log.log, door.get_component_mut(door_e));
                                door.open();
                                if door.open_time < 30 {
                                    if info.waiting_for_door.is_none() {
                                        target_rotation.add_component(
                                            e,
                                            TargetRotation {
                                                rotation: Angle::new(
                                                    (pos.x - next.x).atan2(pos.z - next.z),
                                                ),
                                                ticks: 4.0,
                                            },
                                        );
                                    }
                                    info.waiting_for_door = Some((pos.x, pos.z));
                                    continue 'entities;
                                } else {
                                    info.waiting_for_door = None;
                                }
                                false
                            } else {
                                warn!(log.log, "Failed to find the door");
                                true
                            }
                        }
                        // Any other type of wall we can't pass through.
                        // This means other path is wrong and we need to recompute.
                        Some(_) => {
                            let end = info.nodes.last().unwrap_or(next);
                            if target.get_component(e).is_none() {
                                target.add_component(e, Target::new(end.x, end.z));
                            }
                            false
                        }
                        _ => false,
                    }
                } else {
                    false
                };

                if !remove {
                    speed.speed = speed.base_speed * adjust;
                    target_pos.add_component(
                        e,
                        TargetPosition {
                            x: next.x,
                            y: 0.0,
                            z: next.z,
                            ticks: ((20.0 / 4.0) * f64::from(next.time))
                                / f64::from(speed.base_speed * adjust),
                        },
                    );
                    target_rotation.add_component(
                        e,
                        TargetRotation {
                            rotation: Angle::new((pos.x - next.x).atan2(pos.z - next.z)),
                            ticks: 4.0,
                        },
                    );

                    false
                } else {
                    true
                }
            }
        } else {
            false
        };
        if remove {
            info.remove_component(e);
        }
    }
}

closure_system!(
    fn speedup_movement(
        em: ecs::EntityManager<'_>,
        log: Read<CLogger>,
        mut info: Write<PathInfo>,
        mut target: Write<TargetTime>,
        mut adjust: Write<LagMovementAdjust>,
    ) {
        let log = log.get_component(Container::WORLD).expect("Missing logger");
        for (e, info) in em.group_mask(&mut info, |m| m.and(&target)) {
            {
                let target = assume!(log.log, target.get_component_mut(e));

                let adjustment = if info.time == 0.0 || target.time == 0.0 {
                    1.0
                } else {
                    info.time / target.time
                };
                adjust.add_component(e, LagMovementAdjust { adjustment })
            }
            target.remove_component(e);
        }
    }
);

closure_system!(
    fn compute_path(
        em: ecs::EntityManager<'_>,
        log: Read<CLogger>,
        tiles: Read<level::LevelTiles>,
        rooms: Read<level::LevelRooms>,
        mut pathfinder: Write<Pathfinder>,
        position: Read<Position>,
        mut target: Write<Target>,
        mut target_f: Write<TargetFacing>,
        mut info: Write<PathInfo>,
    ) {
        let mask = target.mask().and(&position);
        let world = Container::WORLD;
        let log = log.get_component(Container::WORLD).expect("Missing logger");
        let tiles = assume!(log.log, tiles.get_component(world));
        let rooms = assume!(log.log, rooms.get_component(world));
        let pathfinder = assume!(log.log, pathfinder.get_component_mut(world));

        for e in em.iter_mask(&mask) {
            // Remove the existing path (if any)
            info.remove_component(e);

            {
                let pos = assume!(log.log, position.get_component(e));
                let tar = assume!(log.log, target.get_component_mut(e));

                // Stuck, wait for unstuck
                if !level::can_visit(tiles, rooms, (pos.x * 4.0) as usize, (pos.z * 4.0) as usize) {
                    continue;
                }

                let mut req = tar.request.take().unwrap_or_else(|| {
                    pathfinder.create_path((pos.x, pos.z), tar.location, tar.required)
                });

                match req.take_path() {
                    PathResult::Ok(mut path) => {
                        if let Some(tf) = target_f.get_component(e) {
                            path.end_rotation = Some(tf.rotation);
                        }
                        info.add_component(e, path);
                    }
                    PathResult::Failed => {
                        if tar.required {
                            #[cfg(not(feature = "debugutil"))]
                            {
                                warn!(log.log, "Compute: Failed to create path for {:?}", e; b!(
                                    "start" => ?(pos.x, pos.z),
                                    "end" => ?tar.location,
                                ));
                            }
                            #[cfg(feature = "debugutil")]
                            {
                                warn!(log.log, "Compute: Failed to create path for {:?}", e; b!(
                                    "start" => ?(pos.x, pos.z),
                                    "end" => ?tar.location,
                                    "creation_trace" => ?tar.creation_trace
                                ));
                            }
                        }
                    }
                    PathResult::Waiting => {
                        tar.request = Some(req);
                        continue;
                    }
                }
            }

            // Remove the target so the path isn't computed again
            // next tick
            target.remove_component(e);
            target_f.remove_component(e);
        }
    }
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PathPos {
    x: i32,
    y: i32,
}

impl PathPos {
    fn new(x: f32, y: f32) -> PathPos {
        PathPos {
            x: (x * 4.0) as i32,
            y: (y * 4.0) as i32,
        }
    }

    fn shift(self, dir: PathDir) -> PathPos {
        let (x, y) = dir.offset();
        PathPos {
            x: self.x + x,
            y: self.y + y,
        }
    }

    fn loc(self) -> Location {
        Location::new(self.x / 4, self.y / 4)
    }
}

fn compute_cost(
    tiles: &level::LevelTiles,
    rooms: &level::LevelRooms,
    pos: PathPos,
    dir: PathDir,
) -> Option<i32> {
    if level::can_visit(tiles, rooms, pos.x as usize, pos.y as usize) {
        let extra_cost = if dir.standard() == None {
            let (ox, oy) = dir.offset();
            if !level::can_visit(tiles, rooms, (pos.x - ox) as usize, pos.y as usize)
                || !level::can_visit(tiles, rooms, pos.x as usize, (pos.y - oy) as usize)
            {
                return None;
            }
            6
        } else {
            0
        };
        // Get the cost
        let rx = pos.x & 0b11;
        let ry = pos.y & 0b11;
        let flag = match (dir, rx, ry) {
            (PathDir::North, _, 0)
            | (PathDir::South, _, 3)
            | (PathDir::East, 0, _)
            | (PathDir::West, 3, _) => tiles
                .get_wall_info(
                    pos.loc(),
                    dir.standard().expect("Invalid direction in compute cost"),
                )
                .map(|v| v.flag),

            (PathDir::NorthEast, _, 0) | (PathDir::NorthWest, _, 0) => tiles
                .get_wall_info(pos.loc(), Direction::North)
                .map(|v| v.flag),
            (PathDir::SouthEast, _, 0) | (PathDir::SouthWest, _, 0) => tiles
                .get_wall_info(pos.loc(), Direction::South)
                .map(|v| v.flag),

            (PathDir::NorthEast, 0, _) | (PathDir::SouthEast, 0, _) => tiles
                .get_wall_info(pos.loc(), Direction::East)
                .map(|v| v.flag),
            (PathDir::NorthWest, 0, _) | (PathDir::SouthWest, 0, _) => tiles
                .get_wall_info(pos.loc(), Direction::West)
                .map(|v| v.flag),
            _ => None,
        };

        match flag {
            Some(level::TileWallFlag::Door) => {
                Some(tiles.get_tile(pos.loc()).movement_cost + 40 + extra_cost)
            }
            _ => Some({
                let tile = tiles.get_tile(pos.loc());
                if rx == 0 || rx == 3 || ry == 0 || ry == 3 {
                    tile.movement_edge_cost + extra_cost
                } else {
                    tile.movement_cost + extra_cost
                }
            }),
        }
    } else {
        None
    }
}

fn estimate_distance(a: PathPos, b: PathPos) -> i32 {
    (a.x - b.x).abs() + (a.y - b.y).abs()
}

#[derive(PartialEq)]
struct PLoc<T> {
    score: f64,
    pos: T,
}

impl<T: PartialEq> Eq for PLoc<T> {}
impl<T: PartialEq> Ord for PLoc<T> {
    fn cmp(&self, other: &PLoc<T>) -> cmp::Ordering {
        self.score
            .partial_cmp(&other.score)
            .expect("PLoc compared NaN")
            .reverse()
    }
}
impl<T: PartialEq> PartialOrd for PLoc<T> {
    fn partial_cmp(&self, other: &PLoc<T>) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Hash, PartialEq, Eq, Debug)]
struct PathArea {
    min: (i32, i32),
    max: (i32, i32),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PathDir {
    /// Negative along the z axis
    North,
    /// Positive along the z axis
    South,
    /// Negative along the x axis
    East,
    /// Positive along the x axis
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}

impl PathDir {
    pub fn standard(self) -> Option<Direction> {
        match self {
            PathDir::North => Some(Direction::North),
            PathDir::South => Some(Direction::South),
            PathDir::East => Some(Direction::East),
            PathDir::West => Some(Direction::West),
            _ => None,
        }
    }
    pub fn offset(self) -> (i32, i32) {
        match self {
            PathDir::North => (0, -1),
            PathDir::South => (0, 1),
            PathDir::East => (-1, 0),
            PathDir::West => (1, 0),
            PathDir::NorthEast => (-1, -1),
            PathDir::NorthWest => (1, -1),
            PathDir::SouthEast => (-1, 1),
            PathDir::SouthWest => (1, 1),
        }
    }
    pub fn reverse(self) -> PathDir {
        match self {
            PathDir::North => PathDir::South,
            PathDir::South => PathDir::North,
            PathDir::East => PathDir::West,
            PathDir::West => PathDir::East,
            PathDir::NorthEast => PathDir::SouthWest,
            PathDir::NorthWest => PathDir::SouthEast,
            PathDir::SouthEast => PathDir::NorthWest,
            PathDir::SouthWest => PathDir::NorthEast,
        }
    }
}
const ALL_PATH_DIRECTIONS: [PathDir; 8] = [
    PathDir::North,
    PathDir::South,
    PathDir::East,
    PathDir::West,
    PathDir::NorthEast,
    PathDir::NorthWest,
    PathDir::SouthEast,
    PathDir::SouthWest,
];

/// Returns a list of areas which the path is within. Used for
/// reducing the search area required for a more detailed search.
///
/// This works by having the level preprocess 4x4 sections of the map
/// (16x16 if going by the collision map's scale). This preprocessing
/// uses a flood fill to find which edge tiles are connected to each
/// other. This information is stored in a bitmap (u64) to reduce
/// the memory requirements (as this information is needed per an
/// edge tile).
///
/// ## Example of connections
///
/// ```ignore
///     1 1 1 1 1 1 1 1
///     1 1 1 1 1 # # 1
///     1 1 1 # # # # #
///     1 # # 2 2 2 2 2
///     # # 2 2 # # # #
///     # 2 2 # # 3 3 3
///     # 2 2 # # 3 3 3
///     # 2 2 # # 3 3 3
/// ```
///
/// When creating the rough path a-star pathfinding is performed on the
/// edges created during preprocessing, going between connections until
/// we end up in the end section. The initial edges to start from are
/// found via a flood fill from the starting position within the current
/// section.
///
/// This should always find its way to the goal if a path exists making
/// this also a quicker way to find invalid paths as well. The results
/// can be used to limit the search space because the connection
/// preprocessing guarantees that a path exists between all the areas,
/// it just doesn't provide the path itself.
fn create_rough_path(
    log: &Logger,
    tiles: &level::LevelTiles,
    rooms: &level::LevelRooms,
    start: (f32, f32),
    end: (f32, f32),
    _required: bool,
) -> Option<Vec<PathArea>> {
    use std::collections::hash_map::Entry;
    use std::collections::BinaryHeap;

    #[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
    struct SectionEdge {
        x: usize,
        y: usize,
        edge: usize,
    }

    let (sx, sy) = (start.0 as usize / 4, start.1 as usize / 4);
    let (end_sx, end_sy) = (end.0 as usize / 4, end.1 as usize / 4);
    let (end_x, end_y) = ((end.0 * 4.0) as usize, (end.1 * 4.0) as usize);

    let mut to_visit = BinaryHeap::with_capacity(2048);

    // Find the edges of the start position
    let touched = level::flood_fill(
        &mut BitSet::new(16 * 16),
        tiles,
        rooms,
        (start.0 * 4.0) as usize,
        (start.1 * 4.0) as usize,
        sx,
        sy,
    );
    // And the end position
    let touched_end = level::flood_fill(
        &mut BitSet::new(16 * 16),
        tiles,
        rooms,
        (end.0 * 4.0) as usize,
        (end.1 * 4.0) as usize,
        end_sx,
        end_sy,
    );

    // Short cut if the start and the end are the same
    if sx == end_sx && sy == end_sy && touched == touched_end {
        return Some(vec![PathArea {
            min: (sx as i32 * 16, sy as i32 * 16),
            max: (sx as i32 * 16 + 15, sy as i32 * 16 + 15),
        }]);
    }

    let mut visited: FNVMap<SectionEdge, (i32, SectionEdge)> = FNVMap::default();
    // Optimization to skip visiting edges when we've visited a
    // connected edge already. All connected edges would search
    // the same neighbor edges so can be skipped.
    let mut visited_links: FNVMap<(usize, usize), u64> = FNVMap::default();

    {
        let links = visited_links.entry((sx, sy)).or_insert(0);
        // Add the start node
        for i in 0..15 * 4 {
            if touched & (1 << i) != 0 {
                let edge = tiles.get_pathable_edges(sx as usize, sy as usize, i);
                *links |= edge;
                to_visit.push(PLoc {
                    score: 0.0,
                    pos: SectionEdge {
                        x: sx,
                        y: sy,
                        edge: i,
                    },
                });
                // Only have to add the first edge as the rest
                // are connected which can be skipped.
                break;
            }
        }
    }

    // Helper for handling bounds checks
    fn get_pathable_edges(
        tiles: &level::LevelTiles,
        sx: isize,
        sy: isize,
        edge: usize,
    ) -> Option<u64> {
        if sx >= 0
            && sy >= 0
            && sx < (tiles.width as isize + 3) / 4
            && sy < (tiles.height as isize + 3) / 4
        {
            Some(tiles.get_pathable_edges(sx as usize, sy as usize, edge))
        } else {
            None
        }
    }

    let mut end_node = None;

    while let Some(current) = to_visit.pop() {
        let edges = tiles.get_pathable_edges(current.pos.x, current.pos.y, current.pos.edge);

        // Stop if we've found the target. Edges have to be checked due
        // to the fact a section can be split multiple ways
        if current.pos.x == end_sx && current.pos.y == end_sy && edges == touched_end {
            end_node = Some(current.pos);
            break;
        }
        let (c_cost, from) = visited
            .get(&current.pos)
            .map_or((0, None), |v| (v.0, Some(v.1)));
        // Tries to add the edge (x, y) in the section offset (sx, sy)
        // from the current position to the `to_visit` list
        let mut handle_edge = |x: usize, y: usize, sx: isize, sy: isize| {
            let oedge = assume!(log, level::edge_to_id(x, y));
            if let Some(other) = get_pathable_edges(
                tiles,
                current.pos.x as isize + sx,
                current.pos.y as isize + sy,
                oedge,
            ) {
                let pos = SectionEdge {
                    x: (current.pos.x as isize + sx) as usize,
                    y: (current.pos.y as isize + sy) as usize,
                    edge: oedge,
                };
                // Check and make sure the edge can actually be
                // visited
                if other & (1 << oedge) != 0 {
                    let links = visited_links.entry((pos.x, pos.y)).or_insert(0);
                    // Skip if we've tried a connected edge
                    if *links & other == other {
                        return;
                    }
                    *links |= other;
                    let entry = visited.entry(pos);
                    let p_cost = if let Entry::Occupied(ref c) = entry {
                        let cost = c.get().0;
                        // Quick skip for a slightly common
                        // case
                        if cost > c_cost {
                            return;
                        }
                        Some(cost)
                    } else {
                        None
                    };

                    // Rough distance checks. Not majorlly
                    // important for the result due to the scale
                    let cost = c_cost
                        + if let Some(from) = from {
                            let (fx, fy) = level::id_to_edge(from.edge);

                            let est_cost = tiles.get_section_cost(pos.x, pos.y);

                            let fx = from.x * 16 + fx;
                            let fy = from.y * 16 + fy;
                            (fx as i32 - (pos.x * 16 + x) as i32).abs()
                                + (fy as i32 - (pos.y * 16 + y) as i32).abs()
                                + est_cost
                        } else {
                            0
                        };

                    if p_cost.map_or(true, |v| v > cost) {
                        match entry {
                            Entry::Occupied(mut val) => {
                                val.insert((cost, current.pos));
                            }
                            Entry::Vacant(val) => {
                                val.insert((cost, current.pos));
                            }
                        }
                        let est = ((pos.x * 16 + x) as i32 - end_x as i32).abs()
                            + ((pos.y * 16 + y) as i32 - end_y as i32).abs();
                        let est = f64::from(est) * 1.01;
                        to_visit.push(PLoc {
                            score: f64::from(cost) + est,
                            pos,
                        });
                        return;
                    }
                }
            }
        };
        for i in 0..15 * 4 {
            if edges & (1 << i) != 0 {
                let (x, y) = level::id_to_edge(i);
                // Try and go to the neighbor edge
                // (or edges for the corners)
                if x == 0 {
                    handle_edge(15, y, -1, 0);
                } else if x == 15 {
                    handle_edge(0, y, 1, 0);
                }
                if y == 0 {
                    handle_edge(x, 15, 0, -1);
                } else if y == 15 {
                    handle_edge(x, 0, 0, 1);
                }
            }
        }
    }

    // If found, recreate the path
    if let Some(end_info) = end_node.and_then(|v| visited.remove(&v)) {
        let mut sections = FNVSet::default();

        sections.insert(PathArea {
            min: (end_info.1.x as i32 * 16, end_info.1.y as i32 * 16),
            max: (end_info.1.x as i32 * 16 + 15, end_info.1.y as i32 * 16 + 15),
        });
        sections.insert(PathArea {
            min: (end_sx as i32 * 16, end_sy as i32 * 16),
            max: (end_sx as i32 * 16 + 15, end_sy as i32 * 16 + 15),
        });
        let mut cur = end_info;
        while let Some(next) = visited.remove(&cur.1) {
            sections.insert(PathArea {
                min: (next.1.x as i32 * 16, next.1.y as i32 * 16),
                max: (next.1.x as i32 * 16 + 15, next.1.y as i32 * 16 + 15),
            });
            cur = next;
        }

        Some(sections.into_iter().collect())
    } else {
        #[cfg(feature = "debugutil")]
        {
            use rand::{thread_rng, Rng};
            if _required {
                let id = thread_rng().gen::<u32>();
                let debug_img =
                    ::std::fs::File::create(&format!("debug_path_rough_{}.png", id)).unwrap();
                let mut img = png::Encoder::new(debug_img, tiles.width * 4, tiles.height * 4);
                img.set_color(png::ColorType::RGBA);
                img.set_depth(png::BitDepth::Eight);
                let mut writer = img.write_header().unwrap();
                let mut data = vec![0u8; ((tiles.width * 4 * tiles.height * 4) * 4) as usize];
                let idx = |x: usize, y: usize| -> usize { (x + y * tiles.width as usize * 4) * 4 };
                for y in 0..tiles.height / 4 {
                    for x in 0..tiles.width / 4 {
                        let mut total = 0;
                        for edge in 0..15 * 4 {
                            let (ex, ey) = level::id_to_edge(edge);
                            let i = idx(x as usize * 16 + ex, y as usize * 16 + ey);
                            let pathable = tiles.get_pathable_edges(x as usize, y as usize, edge);
                            data[i + 0] = pathable.count_ones() as u8;
                            data[i + 3] = 255;
                            total += pathable.count_ones();
                        }
                        for ex in 1..15 {
                            for ey in 1..15 {
                                let i = idx(x as usize * 16 + ex, y as usize * 16 + ey);
                                data[i + 0] = (total / (15 * 4)) as u8;
                                data[i + 3] = 255;
                            }
                        }
                    }
                }
                let highest_cost = visited.values().map(|v| v.0).max().unwrap_or(1);

                for (p, v) in visited.iter() {
                    let (ex, ey) = level::id_to_edge(p.edge);
                    let i = idx(p.x as usize * 16 + ex, p.y as usize * 16 + ey);
                    let c = v.0 as f32 / highest_cost as f32;
                    data[i + 1] = 20 + ((255.0 - 20.0) * c) as u8;
                    data[i + 3] = 255;
                }

                // Start
                {
                    let i = idx((start.0 * 4.0) as usize, (start.1 * 4.0) as usize);
                    data[i + 2] = 255;
                    data[i + 3] = 255;
                }

                // End
                {
                    let i = idx((end.0 * 4.0) as usize, (end.1 * 4.0) as usize);
                    data[i + 2] = 120;
                    data[i + 3] = 255;
                }

                writer.write_image_data(&data).unwrap();

                let debug_img =
                    ::std::fs::File::create(&format!("debug_path_rough_{}_map.png", id)).unwrap();
                let mut img = png::Encoder::new(debug_img, tiles.width * 4, tiles.height * 4);
                img.set_color(png::ColorType::RGBA);
                img.set_depth(png::BitDepth::Eight);
                let mut writer = img.write_header().unwrap();
                let mut data = vec![0; ((tiles.width * 4 * tiles.height * 4) * 4) as usize];
                let idx = |x: usize, y: usize| -> usize { (x + y * tiles.width as usize * 4) * 4 };
                for y in 0..tiles.height * 4 {
                    for x in 0..tiles.width * 4 {
                        let i = idx(x as usize, y as usize);
                        if !level::can_visit(tiles, rooms, x as usize, y as usize) {
                            data[i + 0] = 255;
                        }
                        data[i + 3] = 255;
                    }
                }

                writer.write_image_data(&data).unwrap();
            }
        }
        None
    }
}

/// Tries to create a path between the two positions using a-star
/// pathfinding.
///
/// See `create_rough_path` for detailed information on how
/// the search space is optimizated.
fn create_path(
    log: &Logger,
    tiles: &level::LevelTiles,
    rooms: &level::LevelRooms,
    start: (f32, f32),
    end: (f32, f32),
    required: bool,
) -> PathResult {
    use std::cmp::max;
    use std::collections::BinaryHeap;

    if !tiles
        .level_bounds
        .in_bounds(Location::new(start.0 as i32, start.1 as i32))
        || !tiles
            .level_bounds
            .in_bounds(Location::new(end.0 as i32, end.1 as i32))
    {
        return PathResult::Failed;
    }

    let areas = if let Some(areas) = create_rough_path(log, tiles, rooms, start, end, required) {
        areas
    } else {
        return PathResult::Failed;
    };

    // Start off with only working with rounded coordinates.
    // The float part will be added in once the path is found
    let start_node = PathPos::new(start.0, start.1);
    let end_node = PathPos::new(end.0, end.1);
    let mut to_visit = BinaryHeap::with_capacity(2048);
    to_visit.push(PLoc {
        score: 0.0,
        pos: start_node,
    });

    let mut visited: FNVMap<PathPos, (i32, PathPos, Option<PathDir>)> = FNVMap::default();
    visited.insert(start_node, (0, start_node, None));

    let max_path = f64::from(max(tiles.width * 4, tiles.height * 4));

    while let Some(current) = to_visit.pop() {
        if current.pos == end_node {
            break;
        }
        let (c_cost, from) = visited.get(&current.pos).map_or((0, None), |v| (v.0, v.2));

        for dir in &ALL_PATH_DIRECTIONS {
            if from.map_or(false, |from| *dir == from.reverse()) {
                continue;
            }
            let pos = current.pos.shift(*dir);
            if !tiles.level_bounds.in_bounds(pos.loc()) {
                continue;
            }

            let p_cost = if let Some(c) = visited.get(&pos).map(|v| v.0) {
                // Quick skip for a slightly common
                // case
                if c > c_cost {
                    continue;
                }
                Some(c)
            } else {
                None
            };

            // Make sure the node is within the
            // rough search areas
            let in_search_area = areas.iter().any(|v| {
                pos.x >= v.min.0 && pos.x <= v.max.0 && pos.y >= v.min.1 && pos.y <= v.max.1
            });
            if !in_search_area {
                continue;
            }

            let cost = c_cost
                + if let Some(cost) = compute_cost(tiles, rooms, current.pos, *dir) {
                    cost
                } else {
                    continue;
                };

            if p_cost.map_or(true, |v| v > cost) {
                visited.insert(pos, (cost, current.pos, Some(*dir)));

                let est = estimate_distance(pos, end_node);
                let est = f64::from(est) * (1.0 + (1.0 / max_path));

                to_visit.push(PLoc {
                    score: f64::from(cost) + est,
                    pos,
                });
            }
        }
    }

    // Recreate the path if found
    if let Some(end_info) = visited.remove(&end_node) {
        let mut path = PathInfo {
            time: 0.0,
            nodes: vec![Node {
                dir: end_info.2.and_then(|v| v.standard()),
                x: end.0,
                z: end.1,
                time: 1.0,
            }],
            waiting_for_door: None,
            end_rotation: None,
        };

        let mut cur = end_info;
        while cur.1 != start_node {
            path.nodes.push(Node {
                dir: cur.2.and_then(|v| v.standard()),
                x: (cur.1.x as f32 + 0.5) / 4.0,
                z: (cur.1.y as f32 + 0.5) / 4.0,
                time: if cur.2.and_then(|v| v.standard()) == None {
                    1.5
                } else {
                    1.0
                },
            });
            cur = assume!(log, visited.remove(&cur.1));
        }

        // Include the start
        path.nodes.push(Node {
            dir: None,
            x: start.0,
            z: start.1,
            time: 1.0,
        });

        // Flip the order back to what we expect
        // (start -> end)
        path.nodes.reverse();

        // Door path improvements
        while let Some(pos) = path
            .nodes
            .windows(2)
            .enumerate()
            .find(|&(_, nodes)| {
                let prev = &nodes[0];
                let next = &nodes[1];
                if prev.time > 1.0 {
                    return false;
                }
                if let Some(dir) = Direction::try_from_offset(
                    next.x as i32 - prev.x as i32,
                    next.z as i32 - prev.z as i32,
                ) {
                    match tiles
                        .get_wall_info(Location::new(prev.x as i32, prev.z as i32), dir)
                        .map(|info| info.flag)
                    {
                        Some(level::TileWallFlag::Door) => true,
                        _ => false,
                    }
                } else {
                    false
                }
            })
            .map(|v| v.0)
        {
            let (a, b, fdir, dir) = {
                let prev = &path.nodes[pos];
                let next = &path.nodes[pos + 1];
                let dir = assume!(
                    log,
                    Direction::try_from_offset(
                        next.x as i32 - prev.x as i32,
                        next.z as i32 - prev.z as i32
                    )
                );
                let a = Location::new(prev.x as i32, prev.z as i32);
                (a, a.shift(dir), prev.dir, dir)
            };

            path.nodes.insert(
                pos,
                Node {
                    dir: fdir,
                    x: a.x as f32 + 0.5,
                    z: a.y as f32 + 0.5,
                    time: 2.0,
                },
            );
            path.nodes.insert(
                pos + 1,
                Node {
                    dir: Some(dir),
                    x: b.x as f32 + 0.5,
                    z: b.y as f32 + 0.5,
                    time: 4.0,
                },
            );

            // Find the first node outside the door area and adjust its time
            if let Some(after) = path.nodes.iter_mut().skip(pos).find(|v| {
                let vl = Location::new(v.x as i32, v.z as i32);
                vl != a && vl != b
            }) {
                after.time = 4.0;
            }

            path.nodes.retain(|v| {
                let vl = Location::new(v.x as i32, v.z as i32);
                v.time > 1.0 || (vl != a && vl != b)
            });
        }

        path.update_time();

        PathResult::Ok(path)
    } else {
        #[cfg(feature = "debugutil")]
        {
            use rand::{thread_rng, Rng};
            if required {
                let debug_img = ::std::fs::File::create(&format!(
                    "debug_path_{}.png",
                    thread_rng().gen::<u32>()
                ))
                .unwrap();
                let mut img = png::Encoder::new(debug_img, tiles.width * 4, tiles.height * 4);
                img.set_color(png::ColorType::RGBA);
                img.set_depth(png::BitDepth::Eight);
                let mut writer = img.write_header().unwrap();
                let mut data = vec![0; ((tiles.width * 4 * tiles.height * 4) * 4) as usize];
                let idx = |x: usize, y: usize| -> usize { (x + y * tiles.width as usize * 4) * 4 };
                for y in 0..tiles.height * 4 {
                    for x in 0..tiles.width * 4 {
                        let i = idx(x as usize, y as usize);
                        if !level::can_visit(tiles, rooms, x as usize, y as usize) {
                            data[i + 0] = 255;
                        }
                        data[i + 3] = 255;
                    }
                }
                for area in &areas {
                    for y in area.min.1..area.max.1 + 1 {
                        for x in area.min.0..area.max.0 + 1 {
                            let i = idx(x as usize, y as usize);
                            data[i + 2] = 255;
                        }
                    }
                }
                let highest_cost = visited.values().map(|v| v.0).max().unwrap_or(1);

                for (p, v) in visited.iter() {
                    let i = idx(p.x as usize, p.y as usize);
                    let c = v.0 as f32 / highest_cost as f32;
                    data[i + 1] = 20 + ((255.0 - 20.0) * c) as u8;
                }

                writer.write_image_data(&data).unwrap();
            }
        }
        PathResult::Failed
    }
}

/// Target to pathfind to.
pub struct Target {
    /// The location pathfind to (as close as possible)
    pub location: (f32, f32),
    /// The path request for this target if any
    pub request: Option<PathRequest>,
    /// Whether it doesn't matter if this path is successful.
    /// Really only useful for debugging
    required: bool,
    // Backtrace of this target's creation to help
    // with tracking down failed pathfinding requests.
    #[cfg(feature = "debugutil")]
    creation_trace: ::backtrace::Backtrace,
}
component!(Target => Map);
impl Target {
    /// Creates a new target component for the given position
    pub fn new(x: f32, y: f32) -> Target {
        Target {
            location: (x, y),
            request: None,
            required: true,
            #[cfg(feature = "debugutil")]
            creation_trace: ::backtrace::Backtrace::new(),
        }
    }

    /// Creates a new target component for the given position
    pub fn try_new(x: f32, y: f32) -> Target {
        Target {
            location: (x, y),
            request: None,
            required: false,
            #[cfg(feature = "debugutil")]
            creation_trace: ::backtrace::Backtrace::new(),
        }
    }
}

/// A target time to travel the path in
pub struct TargetTime {
    /// The target time
    pub time: f32,
}
component!(TargetTime => Map);

/// The direction to face at the end of a path
pub struct TargetFacing {
    /// The target rotation
    pub rotation: Angle,
}
component!(TargetFacing => Map);

/// Information about the current path being traversed
pub struct PathInfo {
    pub(super) nodes: Vec<Node>,
    /// The estimated amount of time to travel the full path
    pub time: f32,
    waiting_for_door: Option<(f32, f32)>,
    /// The direction to face at the end of the path
    pub end_rotation: Option<Angle>,
}
component!(PathInfo => Vec);

impl PathInfo {
    /// Returns whether this path is empty
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
    /// Returns the last position in the path
    pub fn last(&self) -> (f32, f32) {
        let l = self.nodes.last().expect("PathInfo empty");
        (l.x, l.z)
    }

    /// Recomputes the estimate travel time
    pub fn update_time(&mut self) {
        if self.nodes.len() <= 1 {
            self.time = 1.0;
        } else {
            self.time = self.nodes.iter().map(|v| v.time).sum();
        }
    }

    /// Returns whether the path is being travelled
    /// or is currently waiting for some reason
    pub fn is_moving(&self) -> bool {
        self.waiting_for_door.is_none()
    }
}

#[derive(Debug, Clone)]
pub(super) struct Node {
    pub(super) dir: Option<Direction>,
    pub(super) x: f32,
    pub(super) z: f32,
    time: f32,
}
