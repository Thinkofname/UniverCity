use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::cmp::Ordering;
use std::fmt::{self, Debug, Display, Formatter};
use std::hash::{Hash, Hasher};
use std::ops::*;
use std::sync::Arc;

#[derive(Clone)]
pub enum ArcStr<'a> {
    Borrowed(&'a str),
    BorrowedArc(&'a Arc<str>),
    Owned(Arc<str>),
}

impl<'a> ArcStr<'a> {
    #[inline]
    pub fn into_owned(self) -> ArcStr<'static> {
        match self {
            ArcStr::Borrowed(v) => ArcStr::Owned(v.into()),
            ArcStr::BorrowedArc(v) => ArcStr::Owned(v.clone()),
            ArcStr::Owned(v) => ArcStr::Owned(v),
        }
    }

    #[inline]
    pub fn borrow(&self) -> ArcStr<'_> {
        match *self {
            ArcStr::Borrowed(v) => ArcStr::Borrowed(v),
            ArcStr::BorrowedArc(v) => ArcStr::BorrowedArc(v),
            ArcStr::Owned(ref v) => ArcStr::BorrowedArc(&v),
        }
    }
}

impl<'a> Serialize for ArcStr<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let a = self.deref();
        a.serialize(serializer)
    }
}

impl<'a, 'de> Deserialize<'de> for ArcStr<'a> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(|v| v.into())
    }
}

impl<'a> PartialEq for ArcStr<'a> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        let a = self.deref();
        let b = other.deref();
        a.eq(b)
    }
}

impl<'a> Eq for ArcStr<'a> {}

impl<'a> PartialOrd for ArcStr<'a> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let a = self.deref();
        let b = other.deref();
        a.partial_cmp(b)
    }
}
impl<'a> Ord for ArcStr<'a> {
    fn cmp(&self, other: &Self) -> Ordering {
        let a = self.deref();
        let b = other.deref();
        a.cmp(b)
    }
}
impl<'a> Hash for ArcStr<'a> {
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        let a = self.deref();
        a.hash(state)
    }
}

impl<'a> Debug for ArcStr<'a> {
    #[inline]
    fn fmt(&self, fmt: &mut Formatter<'_>) -> fmt::Result {
        match *self {
            ArcStr::Borrowed(v) => Debug::fmt(v, fmt),
            ArcStr::BorrowedArc(v) => Debug::fmt(v, fmt),
            ArcStr::Owned(ref v) => Debug::fmt(v, fmt),
        }
    }
}

impl<'a> Display for ArcStr<'a> {
    #[inline]
    fn fmt(&self, fmt: &mut Formatter<'_>) -> fmt::Result {
        match *self {
            ArcStr::Borrowed(v) => Display::fmt(v, fmt),
            ArcStr::BorrowedArc(v) => Display::fmt(v, fmt),
            ArcStr::Owned(ref v) => Display::fmt(v, fmt),
        }
    }
}

impl<'a> From<&'a str> for ArcStr<'a> {
    #[inline]
    fn from(v: &'a str) -> ArcStr<'a> {
        ArcStr::Borrowed(v)
    }
}
impl From<String> for ArcStr<'static> {
    #[inline]
    fn from(v: String) -> ArcStr<'static> {
        ArcStr::Owned(v.into())
    }
}
impl From<Arc<str>> for ArcStr<'static> {
    #[inline]
    fn from(v: Arc<str>) -> ArcStr<'static> {
        ArcStr::Owned(v)
    }
}

impl<'a> Deref for ArcStr<'a> {
    type Target = str;
    #[inline]
    fn deref(&self) -> &str {
        match *self {
            ArcStr::Borrowed(v) => v,
            ArcStr::BorrowedArc(v) => v.deref(),
            ArcStr::Owned(ref v) => v.deref(),
        }
    }
}
