
/// A direction along one of the 2 axis (x, z)
#[derive(Clone, Copy, PartialEq, Eq, Debug, DeltaEncode)]
pub enum Direction {
    /// Negative along the z axis
    North,
    /// Positive along the z axis
    South,
    /// Negative along the x axis
    East,
    /// Positive along the x axis
    West
}

/// All possible directions
pub const ALL_DIRECTIONS: [Direction; 4] = [
    Direction::North,
    Direction::South,
    Direction::East,
    Direction::West,
];

impl Direction {
    /// Returns an offset for the direction
    #[inline]
    pub fn offset(self) -> (i32, i32) {
        match self {
            Direction::North => (0, -1),
            Direction::South => (0, 1),
            Direction::East => (-1, 0),
            Direction::West => (1, 0),
        }
    }

    /// Returns the reverse of the direction
    #[inline]
    pub fn reverse(self) -> Direction {
        match self {
            Direction::North => Direction::South,
            Direction::South => Direction::North,
            Direction::East => Direction::West,
            Direction::West => Direction::East,
        }
    }

    /// Converts an offset into a direction. Panics
    /// if the offset is invalid
    #[inline]
    pub fn from_offset(x: i32, y: i32) -> Direction {
        match (x, y) {
            (x, 0) if x < 0 => Direction::East,
            (x, 0) if x > 0 => Direction::West,
            (0, y) if y < 0 => Direction::North,
            (0, y) if y > 0 => Direction::South,
            _ => panic!("Invalid offset {}, {}", x, y),
        }
    }

    /// Converts an offset into a direction.
    #[inline]
    pub fn try_from_offset(x: i32, y: i32) -> Option<Direction> {
        match (x, y) {
            (x, 0) if x < 0 => Some(Direction::East),
            (x, 0) if x > 0 => Some(Direction::West),
            (0, y) if y < 0 => Some(Direction::North),
            (0, y) if y > 0 => Some(Direction::South),
            _ => None,
        }
    }

    /// Returns a unique id for the direction
    #[inline]
    pub fn as_usize(self) -> usize {
        match self {
            Direction::North => 0,
            Direction::South => 1,
            Direction::East => 2,
            Direction::West => 3,
        }
    }

    /// Converts the unique id from `as_usize` back
    /// into a direction if valid.
    #[inline]
    pub fn from_usize(id: usize) -> Option<Direction> {
        match id {
            0 => Some(Direction::North),
            1 => Some(Direction::South),
            2 => Some(Direction::East),
            3 => Some(Direction::West),
            _ => None,
        }
    }

    /// Attempts to convert the passed string into a direction.
    ///
    /// Supports lower case and upper case
    #[inline]
    pub fn from_str(dir: &str) -> Result<Direction, String> {
        match dir {
            "north" | "NORTH" => Ok(Direction::North),
            "south" | "SOUTH" => Ok(Direction::South),
            "east" | "EAST" => Ok(Direction::East),
            "west" | "WEST" => Ok(Direction::West),
            dir => Err(format!("{:?} isn't a valid direction", dir)),
        }
    }

    /// Returns this direction as a string reference
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::North => "north",
            Direction::South => "south",
            Direction::East => "east",
            Direction::West => "west",
        }
    }
}