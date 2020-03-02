use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::ops::*;

/// Represents the currency used by the game
#[derive(Clone, Copy, Hash, Debug, Default, PartialEq, Eq, PartialOrd, Ord, DeltaEncode)]
pub struct UniDollar(pub i64);

impl Add for UniDollar {
    type Output = UniDollar;
    fn add(self, rhs: Self) -> Self {
        UniDollar(self.0 + rhs.0)
    }
}

impl AddAssign for UniDollar {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl Sub for UniDollar {
    type Output = UniDollar;
    fn sub(self, rhs: Self) -> Self {
        UniDollar(self.0 - rhs.0)
    }
}

impl SubAssign for UniDollar {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl Div for UniDollar {
    type Output = i64;
    fn div(self, rhs: UniDollar) -> i64 {
        self.0 / rhs.0
    }
}

macro_rules! impl_unidollar_num_ty {
    ($ty:ty) => {
        impl Div<$ty> for UniDollar {
            type Output = UniDollar;
            fn div(self, rhs: $ty) -> Self {
                UniDollar(self.0 / (i64::from(rhs)))
            }
        }

        impl DivAssign<$ty> for UniDollar {
            fn div_assign(&mut self, rhs: $ty) {
                self.0 /= i64::from(rhs);
            }
        }

        impl Mul<$ty> for UniDollar {
            type Output = UniDollar;
            fn mul(self, rhs: $ty) -> Self {
                UniDollar(self.0 * i64::from(rhs))
            }
        }

        impl MulAssign<$ty> for UniDollar {
            fn mul_assign(&mut self, rhs: $ty) {
                self.0 *= i64::from(rhs);
            }
        }

        impl Mul<UniDollar> for $ty {
            type Output = UniDollar;
            fn mul(self, rhs: UniDollar) -> UniDollar {
                UniDollar(i64::from(self) * rhs.0)
            }
        }
    };
}
impl_unidollar_num_ty!(i8);
impl_unidollar_num_ty!(i16);
impl_unidollar_num_ty!(i32);
impl_unidollar_num_ty!(i64);
impl_unidollar_num_ty!(u8);
impl_unidollar_num_ty!(u16);
impl_unidollar_num_ty!(u32);
// Not safe to implement for u64

impl Neg for UniDollar {
    type Output = Self;
    fn neg(self) -> Self {
        UniDollar(-self.0)
    }
}

impl Serialize for UniDollar {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_i64(self.0)
    }
}

impl<'de> Deserialize<'de> for UniDollar {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_i64(UniDollarVisitor)
    }
}

struct UniDollarVisitor;

macro_rules! impl_vist {
    ($name:ident, $ty:ty) => {
        fn $name<E>(self, val: $ty) -> Result<UniDollar, E>
        where
            E: de::Error,
        {
            Ok(UniDollar(i64::from(val)))
        }
    };
}

impl<'de> Visitor<'de> for UniDollarVisitor {
    type Value = UniDollar;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an integer")
    }

    impl_vist!(visit_i8, i8);
    impl_vist!(visit_i16, i16);
    impl_vist!(visit_i32, i32);
    impl_vist!(visit_i64, i64);
    impl_vist!(visit_u8, u8);
    impl_vist!(visit_u16, u16);
    impl_vist!(visit_u32, u32);

    fn visit_u64<E>(self, val: u64) -> Result<UniDollar, E>
    where
        E: de::Error,
    {
        Ok(UniDollar(val as i64))
    }
}

impl fmt::Display for UniDollar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "${}", self.0)
    }
}
