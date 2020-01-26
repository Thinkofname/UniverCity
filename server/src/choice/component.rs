use super::*;

use std::any::Any;
use std::alloc;
use std::marker::PhantomData;
use std::fmt::{self, Debug};

/// Always returns the same value when using the normal
/// methods
///
/// Used to contain dynamic variable storage for rules
pub struct EntityVarStorage<T> {
    val: T,
    data: *mut u32,
    size: usize,
    var_size: usize,
    storage_ty: FNVMap<String, (Type, u16)>,
}

impl <T> EntityVarStorage<T> {
    /// Creates an EntityVarStorage with enough space to
    /// contain the required vars.
    #[allow(clippy::cast_ptr_alignment)]
    pub fn create<G>(alloc: BasicAlloc<G>, val: T) -> EntityVarStorage<T> {
        let var_size = alloc.storage_ty.len();
        let mem = unsafe {
            alloc::alloc(alloc::Layout::from_size_align(var_size * 4 * 4, 4).expect("Failed to layout entity memory"))
        };
        EntityVarStorage {
            val,
            data: mem as *mut u32,
            size: 4,
            var_size,
            storage_ty: alloc.storage_ty,
        }
    }
}
impl <T> Drop for EntityVarStorage<T> {
    fn drop(&mut self) {
        unsafe {
            alloc::dealloc(
                self.data as *mut _,
                alloc::Layout::from_size_align(self.var_size * self.size * 4, 4).expect("Failed to layout entity memory"),
            );
        }
    }
}

impl <T> StorageCustom for EntityVarStorage<T> {
    type Value = EntityVars<T>;

    fn get(&mut self, id: u32) -> Self::Value {
        unsafe {
            EntityVars(
                self.data.add(id as usize * self.var_size),
                &*(&self.storage_ty as *const _),
                PhantomData
            )
        }
    }
}

/// Reference to a location in memory that stores variables.
///
/// Only valid until `add_component` is called or the storage
/// is dropped.  Be cafeful.
///
/// This should have a lifetime but generic type limits prevent this
/// from happening currently.
pub struct EntityVars<T>(pub *mut u32, &'static FNVMap<String, (Type, u16)>, PhantomData<T>);

impl <T> Debug for EntityVars<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("EntityVars");
        for (key, (ty, offset)) in self.1 {
            let val = unsafe { self.0.offset(*offset as isize).read() };
            match ty {
                Type::Integer => s.field(key, &(val as i32)),
                Type::Float => s.field(key, &f32::from_bits(val)),
                Type::Boolean => s.field(key, &(val != 0)),
            };
        }
        s.finish()
    }
}

impl <T> EntityVars<T> {

    /// Removes the type information.
    ///
    /// Shouldn't really exist but meh
    #[inline]
    pub fn remove_type(self) -> EntityVars<()> {
        EntityVars(self.0, self.1, PhantomData)
    }

    /// Iterators over raw values
    #[inline]
    pub fn iter<'a>(&'a self) -> impl Iterator<Item=(&'a str, u32)> + 'a {
        self.1.iter()
            .map(move |v| (v.0.as_str(), unsafe { self.0.offset((v.1).1 as isize).read() }))
    }
    /// Sets the value of the variable if it exists
    #[inline]
    pub fn set_raw(&self, name: &str, val: u32) -> bool {
        unsafe {
            self.1.get(name)
                .map(|v| self.0.offset(v.1 as isize))
                .map(|v| v.write(val))
                .is_some()
        }
    }

    /// Returns the var memory as a slice
    #[inline]
    pub fn slice(&self) -> &[u32] {
        unsafe {
            ::std::slice::from_raw_parts(self.0, self.1.len())
        }
    }

    /// Returns the value of the variable if it exists
    #[inline]
    pub fn get_integer(&self, name: &str) -> Option<i32> {
        unsafe {
            self.1.get(name)
                .filter(|v| v.0 == Type::Integer)
                .map(|v| self.0.offset(v.1 as isize))
                .map(|v| v.read() as i32)
        }
    }
    /// Returns the value of the variable if it exists
    #[inline]
    pub fn get_float(&self, name: &str) -> Option<f32> {
        unsafe {
            self.1.get(name)
                .filter(|v| v.0 == Type::Float)
                .map(|v| self.0.offset(v.1 as isize))
                .map(|v| f32::from_bits(v.read()))
        }
    }
    /// Returns the value of the variable if it exists
    #[inline]
    pub fn get_boolean(&self, name: &str) -> Option<bool> {
        unsafe {
            self.1.get(name)
                .filter(|v| v.0 == Type::Boolean)
                .map(|v| self.0.offset(v.1 as isize))
                .map(|v| v.read() != 0)
        }
    }
    /// Sets the value of the variable if it exists
    #[inline]
    pub fn set_integer(&self, name: &str, val: i32) -> bool {
        unsafe {
            self.1.get(name)
                .filter(|v| v.0 == Type::Integer)
                .map(|v| self.0.offset(v.1 as isize))
                .map(|v| v.write(val as u32))
                .is_some()
        }
    }
    /// Sets the value of the variable if it exists
    #[inline]
    pub fn set_float(&self, name: &str, val: f32) -> bool {
        unsafe {
            self.1.get(name)
                .filter(|v| v.0 == Type::Float)
                .map(|v| self.0.offset(v.1 as isize))
                .map(|v| v.write(val.to_bits()))
                .is_some()
        }
    }
    /// Sets the value of the variable if it exists
    #[inline]
    pub fn set_boolean(&self, name: &str, val: bool) -> bool {
        unsafe {
            self.1.get(name)
                .filter(|v| v.0 == Type::Boolean)
                .map(|v| self.0.offset(v.1 as isize))
                .map(|v| v.write(val as u32))
                .is_some()
        }
    }

    /// Returns the type of the variable if it exists
    #[inline]
    pub fn get_type(&self, name: &str) -> Option<Type> {
        self.1.get(name).map(|v| v.0)
    }

    /// Reads a stat from the entity
    #[inline]
    pub fn get_stat(&self, stat: Stat) -> f32 {
        unsafe {
            f32::from_bits(self.0.add(stat.index).read()).min(1.0).max(0.0)
        }
    }

    /// Sets a stat to the entity
    #[inline]
    pub fn set_stat(&self, stat: Stat, val: f32) {
        unsafe {
            self.0.add(stat.index)
                .write((val.min(1.0).max(0.0)).to_bits())
        }
    }
}

unsafe impl <T: Component> ComponentStorage<T> for EntityVarStorage<T> {
    fn new() -> Self {
        panic!("Cannot be constructed automatically")
    }
    #[allow(clippy::cast_ptr_alignment)]
    fn add_component(&mut self, id: u32, _val: T) {
        if self.size <= id as usize {
            let old_size = self.size;
            while self.size <= id as usize {
                self.size <<= 1;
            }
            let mem = unsafe {alloc::realloc(
                self.data as *mut _,
                alloc::Layout::from_size_align(self.var_size * old_size * 4, 4).expect("Failed to layout entity memory"),
                self.size * self.var_size * 4,
            )};
            self.data = mem as *mut u32;
        }
        unsafe {
            self.data.add(id as usize * self.var_size).write_bytes(
                0,
                self.var_size
            );
        }
    }
    fn remove_component(&mut self, _id: u32) -> Option<T> {
        None
    }

    fn get_component(&self, _id: u32) -> Option<&T> {
        Some(&self.val)
    }

    fn get_component_mut(&mut self, _id: u32) -> Option<&mut T> {
        Some(&mut self.val)
    }
}

impl <T: Component> InternalBoxedStorage for EntityVarStorage<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_mut_any(&mut self) -> &mut dyn Any {
        self
    }

    fn free_id(&mut self, _id: u32) {
    }
}