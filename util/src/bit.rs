use std::fmt::{self, Debug, Formatter};

#[derive(Clone)]
pub struct BitSet {
    pub data: Vec<u64>,
}

#[test]
fn test_bitset() {
    let mut set = BitSet::new(200);
    for i in 0..200 {
        if i % 3 == 0 {
            set.set(i, true)
        }
    }
    for i in 0..200 {
        assert_eq!(set.get(i), (i % 3 == 0));
    }
}

impl BitSet {
    #[inline]
    pub fn new(size: usize) -> BitSet {
        BitSet { data: vec![0; (size + 63) / 64] }
    }

    #[inline]
    pub fn set(&mut self, i: usize, v: bool) {
        if let Some(b) = self.data.get_mut(i >> 6) {
            if v {
                *b |= 1 << (i & 0x3F)
            } else {
                *b &= !(1 << (i & 0x3F))
            }
        }
    }

    #[inline]
    pub fn get(&self, i: usize) -> bool {
        self.data.get(i >> 6).map_or(false, |&b| (b & (1 << (i & 0x3F))) != 0)
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.data.len() * 64
    }

    #[inline]
    pub fn clear(&mut self) {
        for b in &mut self.data {
            *b = 0;
        }
    }

    #[inline]
    pub fn resize(&mut self, new_size: usize) {
        let require_size = (new_size + 63) / 64;
        if require_size > self.data.len() {
            self.data.resize(require_size, 0);
        }
    }

    #[inline]
    pub fn includes_set(&self, other: &BitSet) -> bool {
        assert!(self.data.len() == other.data.len());
        for (i, val) in self.data.iter().enumerate() {
            let o = other.data[i];
            if val & o != o {
                return false;
            }
        }
        true
    }
    #[inline]
    pub fn or(&mut self, other: &BitSet) {
        for (a, b) in self.data.iter_mut().zip(&other.data) {
            *a |= *b;
        }
    }

    #[inline]
    pub fn and(&mut self, other: &BitSet) {
        self.data.truncate(other.data.len());
        for (a, b) in self.data.iter_mut().zip(&other.data) {
            *a &= *b;
        }
    }

    #[inline]
    pub fn and_not(&mut self, other: &BitSet) {
        for (a, b) in self.data.iter_mut().zip(&other.data) {
            *a &= !*b;
        }
    }
}

impl Debug for BitSet {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "BitSet(")?;
        for v in &self.data {
            write!(f, "{:b}", v)?;
        }
        write!(f, ")")
    }
}