use super::*;

/// A collection of a single type of components.
pub unsafe trait ComponentStorage<T: Component>: internal::BoxedStorage {
    /// Allocates a component storage for the given type
    fn new() -> Self;
    /// Adds the component to the storage at the location given by
    /// the passed id.
    fn add_component(&mut self, id: u32, val: T);
    /// Removes the component from the storage at the location given by
    /// the passed id.
    fn remove_component(&mut self, id: u32) -> Option<T>;

    /// Gets a reference component from the storage at the location given by
    /// the passed id.
    fn get_component(&self, id: u32) -> Option<&T>;
    /// Gets a mutable reference component from the storage at the location
    /// given by the passed id.
    fn get_component_mut(&mut self, id: u32) -> Option<&mut T>;

    /// Gets a reference component from the storage at the location given by
    /// the passed id.
    unsafe fn get_unchecked_component(&self, id: u32) -> &T {
        self.get_component(id).expect("Missing component")
    }
    /// Gets a mutable reference component from the storage at the location
    /// given by the passed id.
    unsafe fn get_unchecked_component_mut(&mut self, id: u32) -> &mut T {
        self.get_component_mut(id).expect("Missing component (mut)")
    }

    /// Gets the component or inserts it if it doesn't exist
    fn get_component_or_insert<F>(&mut self, id: u32, f: F) -> &mut T
        where F: FnOnce() -> T
    {
        use std::mem;
        // Transmute is used here due to weirdness with lifetimes
        if let Some(val) = unsafe { mem::transmute(self.get_component_mut(id)) } {
            val
        } else {
            unsafe {
                self.add_and_get_component(id, f())
            }
        }
    }

    /// Add the component and returns the inserted value
    unsafe fn add_and_get_component(&mut self, id: u32, v: T) -> &mut T {
        self.add_component(id, v);
        self.get_unchecked_component_mut(id)
    }

    /// Marks the storage doing its own bookkeeping.
    ///
    /// Causes the system to pass add/get/remove calls
    /// through without checking if the entity has the
    /// component first assuming the storage will do it
    /// itself.
    #[inline]
    #[doc(hidden)]
    fn self_bookkeeps() -> bool {
        false
    }
}

/// Stores components in a `HashMap`.
pub struct MapStorage<T: Component> {
    data: fnv::FnvHashMap<u32, T>,
}

unsafe impl <T: Component> ComponentStorage<T> for MapStorage<T> {
    fn new() -> Self {
        MapStorage {
            data: fnv::FnvHashMap::default(),
        }
    }
    #[inline]
    fn add_component(&mut self, id: u32, val: T) {
        self.data.insert(id, val);
    }
    #[inline]
    fn remove_component(&mut self, id: u32) -> Option<T> {
        self.data.remove(&id)
    }

    #[inline]
    fn get_component(&self, id: u32) -> Option<&T> {
        self.data.get(&id)
    }

    #[inline]
    fn get_component_mut(&mut self, id: u32) -> Option<&mut T> {
        self.data.get_mut(&id)
    }

    fn get_component_or_insert<F>(&mut self, id: u32, f: F) -> &mut T
        where F: FnOnce() -> T
    {
        self.data.entry(id).or_insert_with(f)
    }

    unsafe fn add_and_get_component(&mut self, id: u32, v: T) -> &mut T {
        self.data.entry(id).or_insert(v)
    }

    #[inline]
    fn self_bookkeeps() -> bool { true }

}

impl <T: Component> internal::BoxedStorage for MapStorage<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_mut_any(&mut self) -> &mut dyn Any {
        self
    }

    fn free_id(&mut self, id: u32) {
        self.remove_component(id);
    }
}

/// Stores components in a `Vec`.
pub struct VecStorage<T: Component> {
    data: Vec<T>,
}

impl <T: Component> Drop for VecStorage<T> {
    fn drop(&mut self) {
        unsafe {
            // Prevent it from dropping uninit data
            self.data.set_len(0);
        }
    }
}

unsafe impl <T: Component> ComponentStorage<T> for VecStorage<T> {
    fn new() -> Self {
        VecStorage {
            data: Vec::with_capacity(1),
        }
    }
    #[inline]
    fn add_component(&mut self, id: u32, val: T) {
        let id = id as usize;
        if self.data.capacity() <= id {
            self.data.reserve(id + 1);
        }
        debug_assert!(self.data.capacity() > id as usize);
        unsafe {
            self.data.as_mut_ptr().add(id).write(val);
        }
    }
    #[inline]
    fn remove_component(&mut self, id: u32) -> Option<T> {
        debug_assert!(self.data.capacity() > id as usize);
        unsafe {
            Some(self.data.as_mut_ptr().offset(id as isize).read())
        }
    }
    #[inline]
    unsafe fn get_unchecked_component(&self, id: u32) -> &T {
        debug_assert!(self.data.capacity() > id as usize);
        &*self.data.as_ptr().offset(id as isize)
    }
    #[inline]
    unsafe fn get_unchecked_component_mut(&mut self, id: u32) -> &mut T {
        debug_assert!(self.data.capacity() > id as usize);
        &mut *self.data.as_mut_ptr().offset(id as isize)
    }

    fn get_component_or_insert<F>(&mut self, _id: u32, _f: F) -> &mut T
        where F: FnOnce() -> T
    {
        panic!("Shouldn't be used")
    }

    unsafe fn add_and_get_component(&mut self, id: u32, val: T) -> &mut T {
        let id = id as usize;
        if self.data.capacity() <= id {
            self.data.reserve(id + 1);
        }
        debug_assert!(self.data.capacity() > id as usize);
        let ptr = self.data.as_mut_ptr().add(id);
        ptr.write(val);
        &mut *ptr
    }

    #[inline]
    fn get_component(&self, _id: u32) -> Option<&T> {
        panic!("Shouldn't be used")
    }

    #[inline]
    fn get_component_mut(&mut self, _id: u32) -> Option<&mut T> {
        panic!("Shouldn't be used")
    }

    #[inline]
    fn self_bookkeeps() -> bool { false }
}

impl <T: Component> internal::BoxedStorage for VecStorage<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_mut_any(&mut self) -> &mut dyn Any {
        self
    }

    fn free_id(&mut self, id: u32) {
        debug_assert!(self.data.capacity() > id as usize);
        unsafe {
            self.data.as_mut_ptr().offset(id as isize).drop_in_place();
        }
    }
}

/// Always returns the `Default::default()` value for a component.
pub struct DefaultStorage<T: Component> {
    val: T
}

unsafe impl <T: Component + Default> ComponentStorage<T> for DefaultStorage<T> {
    fn new() -> Self {
        DefaultStorage {
            val: T::default(),
        }
    }
    #[inline]
    fn add_component(&mut self, _id: u32, _val: T) {

    }
    #[inline]
    fn remove_component(&mut self, _id: u32) -> Option<T> {
        None
    }

    #[inline]
    fn get_component(&self, _id: u32) -> Option<&T> {
        Some(&self.val)
    }

    #[inline]
    fn get_component_mut(&mut self, _id: u32) -> Option<&mut T> {
        Some(&mut self.val)
    }
}

impl <T: Component> internal::BoxedStorage for DefaultStorage<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_mut_any(&mut self) -> &mut dyn Any {
        self
    }

    fn free_id(&mut self, _id: u32) {
    }
}

/// Stores a const ptr for the world entity
pub struct ConstWorldStore<T: Component> {
    pub(crate) val: *const T
}

unsafe impl <T: Component> ComponentStorage<T> for ConstWorldStore<T> {
    fn new() -> Self {
        use std::ptr;
        ConstWorldStore {
            val: ptr::null(),
        }
    }
    #[inline]
    fn add_component(&mut self, _id: u32, _val: T) {
        panic!("Unsupported operation")
    }
    #[inline]
    fn remove_component(&mut self, _id: u32) -> Option<T> {
        panic!("Unsupported operation")
    }

    #[inline]
    fn get_component(&self, id: u32) -> Option<&T> {
        if id == 0 {
            Some(unsafe { &*self.val })
        } else {
            None
        }
    }

    #[inline]
    fn get_component_mut(&mut self, _id: u32) -> Option<&mut T> {
        panic!("Unsupported operation")
    }
}

impl <T: Component> internal::BoxedStorage for ConstWorldStore<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_mut_any(&mut self) -> &mut dyn Any {
        self
    }

    fn free_id(&mut self, _id: u32) {}
}

/// Stores a mut ptr for the world entity
pub struct MutWorldStore<T: Component> {
    pub(crate) val: *mut T
}

unsafe impl <T: Component> ComponentStorage<T> for MutWorldStore<T> {
    fn new() -> Self {
        use std::ptr;
        MutWorldStore {
            val: ptr::null_mut(),
        }
    }
    #[inline]
    fn add_component(&mut self, _id: u32, _val: T) {
        panic!("Unsupported operation")
    }
    #[inline]
    fn remove_component(&mut self, _id: u32) -> Option<T> {
        panic!("Unsupported operation")
    }

    #[inline]
    fn get_component(&self, id: u32) -> Option<&T> {
        if id == 0 {
            Some(unsafe { &*self.val })
        } else {
            None
        }
    }

    #[inline]
    fn get_component_mut(&mut self, id: u32) -> Option<&mut T> {
        if id == 0 {
            Some(unsafe { &mut*self.val })
        } else {
            None
        }
    }

    #[inline]
    fn self_bookkeeps() -> bool { true }
}

impl <T: Component> internal::BoxedStorage for MutWorldStore<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_mut_any(&mut self) -> &mut dyn Any {
        self
    }

    fn free_id(&mut self, _id: u32) {}
}