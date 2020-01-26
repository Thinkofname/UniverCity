
use std::ops::{RangeInclusive, Sub, SubAssign, Add, AddAssign};
use super::Direction;

/// A location in the level
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, DeltaEncode, Serialize, Deserialize)]
pub struct Location {
    /// The location along the x axis
    pub x: i32,
    /// The location along the y axis
    pub y: i32,
}

impl Location {
    /// Creates a new location for the given coordinates.
    pub fn new(x: i32, y: i32) -> Location {
        Location {
            x,
            y,
        }
    }

    /// Returns a location for 0, 0
    pub fn zero() -> Location {
        Location::new(0, 0)
    }

    /// Shifts the location in the passed direction
    pub fn shift(self, dir: Direction) -> Location {
        let (ox, oy) = dir.offset();
        Location::new(self.x + ox, self.y + oy)
    }

    /// Offsets the location by the given values
    pub fn offset(self, x: i32, y: i32) -> Location {
        Location::new(self.x + x, self.y + y)
    }
}

impl SubAssign<(i32, i32)> for Location {
    fn sub_assign(&mut self, other: (i32, i32)) {
        self.x -= other.0;
        self.y -= other.1;
    }
}

impl Sub<(i32, i32)> for Location {
    type Output = Location;
    fn sub(self, other: (i32, i32)) -> Location {
        Location::new(self.x - other.0, self.y - other.1)
    }
}

impl AddAssign<(i32, i32)> for Location {
    fn add_assign(&mut self, other: (i32, i32)) {
        self.x += other.0;
        self.y += other.1;
    }
}

impl Add<(i32, i32)> for Location {
    type Output = Location;
    fn add(self, other: (i32, i32)) -> Location {
        Location::new(self.x + other.0, self.y + other.1)
    }
}

/// A box between two locations
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, DeltaEncode, Serialize, Deserialize)]
pub struct Bound {
    /// The min bound of the box
    pub min: Location,
    /// The max bound of the box
    pub max: Location,
}

impl Bound {
    /// Creates a new bound using the two locations. This will find the
    /// min and max of the two locations to create the bound.
    #[inline]
    pub fn new(loc_a: Location, loc_b: Location) -> Bound {
        use std::cmp;
        let min = Location::new(cmp::min(loc_a.x, loc_b.x), cmp::min(loc_a.y, loc_b.y));
        let max = Location::new(cmp::max(loc_a.x, loc_b.x), cmp::max(loc_a.y, loc_b.y));
        Bound {
            min,
            max,
        }
    }

    /// Insets the bound by the passed amount
    #[inline]
    pub fn inset(mut self, amount: i32) -> Bound {
        self.min += (amount, amount);
        self.max -= (amount, amount);
        self
    }

    /// Extends the bound by the passed amount
    #[inline]
    pub fn extend(mut self, amount: i32) -> Bound {
        self.min -= (amount, amount);
        self.max += (amount, amount);
        self
    }

    /// Returns a range of all the tiles coordinates along the x axis
    #[inline]
    pub fn x_range(self) -> RangeInclusive<i32> {
        self.min.x ..= self.max.x
    }

    /// Returns a range of all the tiles coordinates along the y axis
    #[inline]
    pub fn y_range(self) -> RangeInclusive<i32> {
        self.min.y ..= self.max.y
    }

    /// Returns an iterator that iterates over all the locations within the bound.
    #[inline]
    pub fn iter(self) -> BoundIter {
        BoundIter {
            x: self.x_range(),
            last_x: 0,
            y_orig: self.y_range(),
            y: self.y_range(),
            first: true,
        }
    }

    /// Returns the width of the bound
    #[inline]
    pub fn width(self) -> i32 {
        (self.max.x - self.min.x) + 1
    }

    /// Returns the height of the bound
    #[inline]
    pub fn height(self) -> i32 {
        (self.max.y - self.min.y) + 1
    }

    /// Returns whether the given location is in bounds
    #[inline]
    pub fn in_bounds(self, loc: Location) -> bool {
        loc.x >= self.min.x && loc.x <= self.max.x
            && loc.y >= self.min.y && loc.y <= self.max.y
    }

    /// Returns whether the given bound is fully contained
    /// within this bound.
    #[inline]
    pub fn contains_bound(self, other: Bound) -> bool {
        other.min.x >= self.min.x && other.max.x <= self.max.x
            && other.min.y >= self.min.y && other.max.y <= self.max.y
    }
}

impl IntoIterator for Bound {
    type Item = Location;
    type IntoIter = BoundIter;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Iterates over every location within the bound
pub struct BoundIter {
    x: RangeInclusive<i32>,
    last_x: i32,
    y_orig: RangeInclusive<i32>,
    y: RangeInclusive<i32>,
    first: bool,
}

impl Iterator for BoundIter {
    type Item = Location;

    #[inline]
    fn next(&mut self) -> Option<Location> {
        if !self.first {
            if let Some(y) = self.y.next() {
                return Some(Location::new(self.last_x, y));
            }
        }
        self.first = false;
        if let Some(x) = self.x.next() {
            self.y = self.y_orig.clone();
            self.last_x = x;
            if let Some(y) = self.y.next() {
                return Some(Location::new(self.last_x, y));
            }
        }
        None
    }
}
