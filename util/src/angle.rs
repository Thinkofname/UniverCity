use std::fmt::{self, Display};
use std::ops::*;

#[derive(Debug, Copy, Clone, PartialEq, PartialOrd, Serialize, Deserialize, DeltaEncode)]
pub struct Angle(f32);

impl Angle {
    #[inline]
    pub fn new(angle: f32) -> Angle {
        Angle(clamp(angle))
    }

    #[inline]
    pub fn raw(self) -> f32 {
        self.0
    }

    pub fn difference<A>(self, other: A) -> Angle
    where
        A: Into<Angle>,
    {
        let a = other.into();
        let diff = self.0 - a.0;
        Angle(clamp(diff).abs())
    }
}

#[test]
fn test_diff() {
    use std::f32::consts::PI;
    let mut val = -PI;
    while val < PI * 2.5 {
        let ang = Angle::new(0.0).difference(val).raw();
        assert!(ang <= PI);
        assert!(ang >= 0.0);
        val += 0.01;
    }
}

impl From<f32> for Angle {
    fn from(v: f32) -> Angle {
        Angle::new(v)
    }
}

impl Display for Angle {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(fmt)
    }
}

impl Add for Angle {
    type Output = Angle;

    #[inline]
    fn add(self, other: Angle) -> Angle {
        Angle::new(self.0 + other.0)
    }
}

impl Add<f32> for Angle {
    type Output = Angle;

    #[inline]
    fn add(self, other: f32) -> Angle {
        Angle::new(self.0 + other)
    }
}

impl Sub for Angle {
    type Output = Angle;

    #[inline]
    fn sub(self, other: Angle) -> Angle {
        Angle::new(self.0 - other.0)
    }
}

impl Sub<f32> for Angle {
    type Output = Angle;

    #[inline]
    fn sub(self, other: f32) -> Angle {
        Angle::new(self.0 - other)
    }
}

impl Div for Angle {
    type Output = Angle;

    #[inline]
    fn div(self, other: Angle) -> Angle {
        Angle::new(self.0 / other.0)
    }
}

impl Div<f32> for Angle {
    type Output = Angle;

    #[inline]
    fn div(self, other: f32) -> Angle {
        Angle::new(self.0 / other)
    }
}

impl Mul for Angle {
    type Output = Angle;

    #[inline]
    fn mul(self, other: Angle) -> Angle {
        Angle::new(self.0 * other.0)
    }
}

impl Mul<f32> for Angle {
    type Output = Angle;

    #[inline]
    fn mul(self, other: f32) -> Angle {
        Angle::new(self.0 * other)
    }
}

impl Neg for Angle {
    type Output = Angle;

    #[inline]
    fn neg(self) -> Angle {
        // Since the inner value has to be between -PI and PI this
        // doesn't need clamping
        Angle(-self.0)
    }
}

impl AddAssign for Angle {
    #[inline]
    fn add_assign(&mut self, other: Angle) {
        *self = Angle::new(self.0 + other.0)
    }
}

impl AddAssign<f32> for Angle {
    #[inline]
    fn add_assign(&mut self, other: f32) {
        *self = Angle::new(self.0 + other)
    }
}

impl SubAssign for Angle {
    #[inline]
    fn sub_assign(&mut self, other: Angle) {
        *self = Angle::new(self.0 - other.0)
    }
}

impl SubAssign<f32> for Angle {
    #[inline]
    fn sub_assign(&mut self, other: f32) {
        *self = Angle::new(self.0 - other)
    }
}

impl DivAssign for Angle {
    #[inline]
    fn div_assign(&mut self, other: Angle) {
        *self = Angle::new(self.0 / other.0)
    }
}

impl DivAssign<f32> for Angle {
    #[inline]
    fn div_assign(&mut self, other: f32) {
        *self = Angle::new(self.0 / other)
    }
}

impl MulAssign for Angle {
    #[inline]
    fn mul_assign(&mut self, other: Angle) {
        *self = Angle::new(self.0 * other.0)
    }
}

impl MulAssign<f32> for Angle {
    #[inline]
    fn mul_assign(&mut self, other: f32) {
        *self = Angle::new(self.0 * other)
    }
}

#[inline]
fn clamp(a: f32) -> f32 {
    use std::f32::consts::PI;
    let shifted = a + PI;
    const RANGE: f32 = PI * 2.0;
    (shifted - ((shifted / RANGE).floor() * RANGE)) - PI
}
