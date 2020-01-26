
use std::marker::PhantomData;

#[derive(Clone)]
pub struct IntMap<K, V> {
    data: Vec<Option<V>>,
    offset: isize,
    _k: PhantomData<K>,
}

pub trait IntKey: Sized + Clone + Copy {
    fn should_resize(offset: isize, len: usize, new: Self) -> Option<(isize, usize, bool)>;
    fn index(offset: isize, key: Self) -> usize;
    fn to_key(offset: isize, index: usize) -> Self;
}

impl <K, V> IntMap<K, V>
    where K: IntKey
{
    #[inline]
    pub fn new() -> IntMap<K, V> {
        IntMap {
            data: Vec::new(),
            offset: 0,
            _k: PhantomData,
        }
    }

    #[inline]
    pub fn insert(&mut self, key: K, val: V) {
        use std::ptr;
        if let Some((offset, new_size, before)) = K::should_resize(self.offset, self.data.len(), key) {
            let diff = new_size - self.data.len();
            self.data.reserve(diff);
            if before {
                unsafe {
                    let old_len = self.data.len();
                    self.data.set_len(new_size);
                    let ptr = self.data.as_mut_ptr();
                    ptr::copy(ptr, ptr.add(diff), old_len);
                    for i in 0 .. diff {
                        ptr::write(ptr.add(i), None);
                    }
                }
            } else {
                for _ in 0 .. diff {
                    self.data.push(None);
                }
            }
            self.offset = offset;
        }
        self.data[K::index(self.offset, key)] = Some(val);
    }

    #[inline]
    pub fn get(&self, key: K) -> Option<&V> {
        self.data.get(K::index(self.offset, key))
            .and_then(|v| v.as_ref())
    }

    #[inline]
    pub fn get_mut(&mut self, key: K) -> Option<&mut V> {
        self.data.get_mut(K::index(self.offset, key))
            .and_then(|v| v.as_mut())
    }

    #[inline]
    pub fn remove(&mut self, key: K) -> Option<V> {
        self.data.get_mut(K::index(self.offset, key))
            .map(|v| v.take())
            .and_then(|v| v)
    }

    #[inline]
    pub fn keys<'a>(&'a self) -> impl Iterator<Item=K> + 'a {
        let offset = self.offset;
        self.data.iter()
            .enumerate()
            .filter(|v| v.1.is_some())
            .map(|v| v.0)
            .map(move |v| K::to_key(offset, v))
    }
}
macro_rules! int_key {
    ($ty:ty) => (
        impl IntKey for $ty {
            #[inline]
            fn should_resize(offset: isize, len: usize, new: Self) -> Option<(isize, usize, bool)> {
                let new = new as isize;
                if new + offset >= 0 && new + offset < len as isize {
                    None
                } else {
                    if new + offset < 0 {
                        let diff = (new + offset).abs();
                        Some((new.abs(), len + diff as usize, true))
                    } else {
                        Some((offset, (new + offset + 1) as usize, false))
                    }
                }
            }

            #[inline]
            fn index(offset: isize, key: Self) -> usize {
                (key as isize + offset) as usize
            }

            #[inline]
            fn to_key(offset: isize, index: usize) -> Self {
                (index as isize - offset) as Self
            }
        }
    )
}

int_key!(i8);
int_key!(i16);
int_key!(i32);

int_key!(u8);
int_key!(u16);
int_key!(u32);

#[test]
fn test() {
    let mut map: IntMap<i8, i32> = IntMap::new();

    map.insert(5, 32);
    assert_eq!(map.get(5).cloned(), Some(32));
    map.insert(6, 66);
    assert_eq!(map.get(5).cloned(), Some(32));
    assert_eq!(map.get(6).cloned(), Some(66));

    map.insert(-22, 12);
    assert_eq!(map.get(5).cloned(), Some(32));
    assert_eq!(map.get(6).cloned(), Some(66));
    assert_eq!(map.get(-22).cloned(), Some(12));
}