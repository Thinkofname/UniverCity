//! Thin layer over the top of the lua api to provide easier and
//! safer access to lua.

#![allow(clippy::new_without_default)]

extern crate lua_sys as sys;
extern crate serde;
extern crate univercity_util as util;
#[macro_use]
#[cfg(test)]
extern crate serde_derive;
#[macro_use]
extern crate failure;

use std::any::{self, Any};
use std::cell::{Cell, RefCell};
use std::ffi::{CStr, CString};
use std::fmt::{self, Debug, Display, Formatter};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::ptr;
use std::rc::{Rc, Weak};
use util::FNVMap as HashMap;

mod serde_support;
pub use serde_support::{Deserializer, Serializer};

/// Contains a lua scripting instance with all its state.
#[derive(Clone)]
pub struct Lua {
    state: Rc<internal::LuaState>,
}

/// Marks which scope a variable should be accessable from
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum Scope {
    /// The global scope is accessible from anywhere
    /// in a lua script (assuming it has access to the
    /// default scope).
    Global,
    /// The registry is only accessible via the `Lua::get`
    /// and `Lua::set` methods.
    ///
    /// This is useful for passing data to custom functions
    ///
    /// # Warning
    ///
    /// Via the `debug` package in lua this table is accessible
    /// but this is not considered standard and should normally
    /// be disabled for production use.
    Registry,
}

impl Lua {
    /// Allocates a lua scripting instance
    pub fn new() -> Lua {
        use std::mem;
        let state = internal::LuaState(
            unsafe {
                let s = sys::luaL_newstate();
                sys::luaL_openlibs(s);
                s
            },
            None,
        );
        let lua = Lua {
            state: Rc::new(state),
        };
        unsafe {
            let userdata_store: UserdataTable = RefCell::new(HashMap::default());
            // Use lua user data to store the value in lua space
            let data = sys::lua_newuserdata(lua.state.0, mem::size_of::<UserdataTable>());
            ptr::write(data as *mut UserdataTable, userdata_store);
            sys::lua_setfield(
                lua.state.0,
                i32::from(sys::LUA_REGISTRYINDEX),
                b"userdata_store\0".as_ptr() as *const _,
            );

            let borrow_store: BorrowTable = RefCell::new(HashMap::default());
            // Use lua user data to store the value in lua space
            let data = sys::lua_newuserdata(lua.state.0, mem::size_of::<BorrowTable>());
            ptr::write(data as *mut BorrowTable, borrow_store);
            sys::lua_setfield(
                lua.state.0,
                i32::from(sys::LUA_REGISTRYINDEX),
                b"borrow_store\0".as_ptr() as *const _,
            );
        }
        lua
    }

    /// Starts building a list of borrowed values that will be accessible
    /// during the execution of the function called at the end
    pub fn with_borrows(&self) -> BorrowBuilder {
        BorrowBuilder { lua: self }
    }

    /// Invokes the named function in the global scope passing the parameters
    /// to the function and converting the result to the requested type.
    pub fn invoke_function<P: Value, Ret: Value>(
        &self,
        name: &str,
        param: P,
    ) -> Result<Ret, Error> {
        self.with_borrows().invoke_function(name, param)
    }

    /// Loads and executes the passed string converting and returning the results
    /// of executing the string.
    ///
    /// This is works like lua's `loadstring` and will execute bytecode if passed it.
    pub fn execute_string<Ret: Value>(&self, script: &str) -> Result<Ret, Error> {
        self.execute_named_string("<string>", script)
    }

    /// Loads and executes the passed string converting and returning the results
    /// of executing the string. The loaded script will have the passed name
    ///
    /// This is works like lua's `loadstring` and will execute bytecode if passed it.
    pub fn execute_named_string<Ret: Value>(&self, name: &str, script: &str) -> Result<Ret, Error> {
        let c_script = CString::new(script).unwrap();
        let c_name = CString::new(name).unwrap();
        unsafe {
            // Used to validate the stack after use
            #[cfg(debug_assertions)]
            let orig_top = sys::lua_gettop(self.state.0);

            // Load and parse the code into the vm
            let status = sys::luaL_loadbuffer(
                self.state.0,
                c_script.as_ptr(),
                script.len(),
                c_name.as_ptr(),
            );
            if status != 0 {
                // Pop the error off the stack and return it
                let ret = CStr::from_ptr(sys::lua_tolstring(self.state.0, -1, ptr::null_mut()));
                internal::lua_pop(self.state.0, 1);
                return Err(Error::Raw {
                    msg: ret.to_string_lossy().into_owned().into_boxed_str(),
                });
            }
            // Invoke the loaded script with no arguments and the return size
            // of what `Ret` holds
            let res = sys::lua_pcall(self.state.0, 0, Ret::stack_size(), 0);
            if res != 0 {
                // Pop the error off the stack and return it
                let ret: Ref<String> = match internal::InternalValue::to_rust(&self.state, -1) {
                    Ok(val) => val,
                    Err(err) => {
                        internal::lua_pop(self.state.0, 1);
                        return Err(err);
                    }
                };
                internal::lua_pop(self.state.0, 1);
                return Err(Error::Raw {
                    msg: ret.to_string().into_boxed_str(),
                });
            }
            // Try and make the type into something we can work with
            let ret = Ret::to_rust(&self.state, -Ret::stack_size());
            // Clean up the stack
            internal::lua_pop(self.state.0, Ret::stack_size());

            // Validate the stack size
            #[cfg(debug_assertions)]
            debug_assert_eq!(orig_top, sys::lua_gettop(self.state.0));

            ret
        }
    }

    /// Sets the value in the scope with the given name to the passed
    /// value.
    pub fn set<T: Value>(&self, scope: Scope, name: &str, val: T) {
        let scope = match scope {
            Scope::Global => i32::from(sys::LUA_GLOBALSINDEX),
            Scope::Registry => i32::from(sys::LUA_REGISTRYINDEX),
        };
        let c_name = CString::new(name).unwrap();
        unsafe {
            val.to_lua(&self.state).unwrap();
            sys::lua_setfield(self.state.0, scope, c_name.as_ptr());
        }
    }

    /// Same as set but requires that the name is already null terminated
    pub unsafe fn set_unsafe<T: Value>(&self, scope: Scope, name: &[u8], val: T) {
        let scope = match scope {
            Scope::Global => i32::from(sys::LUA_GLOBALSINDEX),
            Scope::Registry => i32::from(sys::LUA_REGISTRYINDEX),
        };
        val.to_lua(&self.state).unwrap();
        sys::lua_setfield(self.state.0, scope, name.as_ptr() as *const _);
    }

    /// Returns the value from the scope with the given name.
    ///
    /// Returns an error if the value doesn't match the requested type.
    pub fn get<T: Value>(&self, scope: Scope, name: &str) -> Result<T, Error> {
        let scope = match scope {
            Scope::Global => i32::from(sys::LUA_GLOBALSINDEX),
            Scope::Registry => i32::from(sys::LUA_REGISTRYINDEX),
        };
        let c_name = CString::new(name).unwrap();
        unsafe {
            sys::lua_getfield(self.state.0, scope, c_name.as_ptr());
            let val = T::to_rust(&self.state, -T::stack_size());
            internal::lua_pop(self.state.0, T::stack_size());
            val
        }
    }

    /// Same as get but requires that the name is already null terminated
    pub unsafe fn get_unsafe<T: Value>(&self, scope: Scope, name: &[u8]) -> Result<T, Error> {
        let scope = match scope {
            Scope::Global => i32::from(sys::LUA_GLOBALSINDEX),
            Scope::Registry => i32::from(sys::LUA_REGISTRYINDEX),
        };
        sys::lua_getfield(self.state.0, scope, name.as_ptr() as *const _);
        let val = T::to_rust(&self.state, -T::stack_size());
        internal::lua_pop(self.state.0, T::stack_size());
        val
    }

    /// Get an immutable reference to a value borrowed
    /// via `BorrowBuilder::borrow`
    ///
    /// # Panics
    ///
    /// Panics if the value wasn't borrowed
    pub fn get_borrow<T: Any>(&self) -> &T {
        let ty = any::TypeId::of::<T>();
        unsafe {
            sys::lua_getfield(
                self.state.0,
                i32::from(sys::LUA_REGISTRYINDEX),
                b"borrow_store\0".as_ptr() as *const _,
            );
            let borrow_store = sys::lua_touserdata(self.state.0, -1) as *mut BorrowTable;
            internal::lua_pop(self.state.0, 1);
            let borrow_store = (&*borrow_store).borrow();

            if let Some(&Borrow::Immutable(ref val)) = borrow_store.get(&ty) {
                &*(val.ptr as *const T)
            } else {
                panic!("Value not borrowed")
            }
        }
    }

    /// Get an immutable reference to a mutably borrowed value
    /// that was borrwed via `BorrowBuilder::borrow_mut`
    ///
    /// # Panics
    ///
    /// Panics if the value wasn't borrowed or if the value is
    /// currently borrowed mutably
    pub fn read_borrow<T: Any>(&self) -> MBRef<T> {
        let ty = any::TypeId::of::<T>();
        unsafe {
            sys::lua_getfield(
                self.state.0,
                i32::from(sys::LUA_REGISTRYINDEX),
                b"borrow_store\0".as_ptr() as *const _,
            );
            let borrow_store = sys::lua_touserdata(self.state.0, -1) as *mut BorrowTable;
            internal::lua_pop(self.state.0, 1);
            let mut borrow_store = (&*borrow_store).borrow_mut();

            if let Some(&mut Borrow::Mutable(ref mut val)) = borrow_store.get_mut(&ty) {
                match val.state.get() {
                    BorrowState::None => {
                        val.state.set(BorrowState::Read(1));
                    }
                    BorrowState::Read(count) => {
                        val.state.set(BorrowState::Read(count + 1));
                    }
                    BorrowState::Write => {
                        panic!("Can't borrow value immutably that is borrowed mutably")
                    }
                };
                MBRef {
                    value: &*(val.ptr as *mut T),
                    // Can't borrow whilst reading so this is safe
                    state: &*(&val.state as *const std::cell::Cell<BorrowState>
                        as *const std::cell::Cell<BorrowState>),
                }
            } else {
                panic!("Value not borrowed")
            }
        }
    }

    /// Get an mutable reference to a mutably borrowed value
    /// that was borrwed via `BorrowBuilder::borrow_mut`
    ///
    /// # Panics
    ///
    /// Panics if the value wasn't borrowed or if the value is
    /// currently borrowed mutably or immutably
    pub fn write_borrow<T: Any>(&self) -> MBRefMut<T> {
        let ty = any::TypeId::of::<T>();
        unsafe {
            sys::lua_getfield(
                self.state.0,
                i32::from(sys::LUA_REGISTRYINDEX),
                b"borrow_store\0".as_ptr() as *const _,
            );
            let borrow_store = sys::lua_touserdata(self.state.0, -1) as *mut BorrowTable;
            internal::lua_pop(self.state.0, 1);
            let mut borrow_store = (&*borrow_store).borrow_mut();

            if let Some(&mut Borrow::Mutable(ref mut val)) = borrow_store.get_mut(&ty) {
                match val.state.get() {
                    BorrowState::None => {
                        val.state.set(BorrowState::Write);
                    }
                    BorrowState::Read(_) => {
                        panic!("Can't borrow value mutably that is borrowed immutably")
                    }
                    BorrowState::Write => {
                        panic!("Can't borrow value mutably that is borrowed already mutably")
                    }
                };
                MBRefMut {
                    value: &mut *(val.ptr as *mut T),
                    // Can't borrow whilst reading so this is safe
                    state: &*(&val.state as *const std::cell::Cell<BorrowState>
                        as *const std::cell::Cell<BorrowState>),
                }
            } else {
                panic!("Value not borrowed")
            }
        }
    }
}

type BorrowTable = RefCell<HashMap<any::TypeId, Borrow>>;
enum Borrow {
    Immutable(ImmutableBorrow),
    Mutable(MutableBorrow),
    Empty,
}

enum AnyType {}

struct ImmutableBorrow {
    ptr: *const AnyType,
}

struct MutableBorrow {
    ptr: *mut AnyType,
    state: Cell<BorrowState>,
}

/// A reference to a borrow. Used to keep track of when its out of use
pub struct MBRef<'a, T: 'a + ?Sized> {
    value: &'a T,
    state: &'a Cell<BorrowState>,
}

impl<'a, T: 'a> MBRef<'a, T>
where
    T: ?Sized,
{
    pub fn map<F, R>(this: MBRef<'a, T>, mfunc: F) -> MBRef<'a, R>
    where
        F: FnOnce(&'a T) -> &'a R,
        R: ?Sized,
    {
        use std::mem;
        let state = this.state;
        let val = this.value;
        // Don't run drop
        mem::forget(this);
        let nv = (mfunc)(val);
        MBRef { value: nv, state }
    }
}

impl<'a, T> Deref for MBRef<'a, T>
where
    T: ?Sized,
{
    type Target = T;
    fn deref(&self) -> &T {
        self.value
    }
}

impl<'a, T> Drop for MBRef<'a, T>
where
    T: ?Sized,
{
    fn drop(&mut self) {
        if let BorrowState::Read(mut count) = self.state.get() {
            count -= 1;
            if count == 0 {
                self.state.set(BorrowState::None);
            } else {
                self.state.set(BorrowState::Read(count));
            }
        } else {
            panic!("Invalid state");
        }
    }
}

/// A mutable reference to a borrow. Used to keep track of when its out of use
pub struct MBRefMut<'a, T: 'a> {
    value: &'a mut T,
    state: &'a Cell<BorrowState>,
}

impl<'a, T> Deref for MBRefMut<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.value
    }
}

impl<'a, T> DerefMut for MBRefMut<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.value
    }
}

impl<'a, T> Drop for MBRefMut<'a, T> {
    fn drop(&mut self) {
        if let BorrowState::Write = self.state.get() {
            self.state.set(BorrowState::None);
        } else {
            panic!("Invalid state");
        }
    }
}

#[derive(Clone, Copy)]
enum BorrowState {
    None,
    Read(usize),
    Write,
}

/// Used to pass some references into lua for the duration of a
/// function call.
///
/// Created by `Lua::with_borrows`
pub struct BorrowBuilder<'a> {
    lua: &'a Lua,
}

impl<'a> BorrowBuilder<'a> {
    /// Borrows an immutable reference and makes it accessible
    /// to lua for the duration of the call.
    pub fn borrow<T>(self, val: &'a T) -> Self
    where
        T: Any,
    {
        unsafe {
            let ty = any::TypeId::of::<T>();

            sys::lua_getfield(
                self.lua.state.0,
                i32::from(sys::LUA_REGISTRYINDEX),
                b"borrow_store\0".as_ptr() as *const _,
            );
            let borrow_store = sys::lua_touserdata(self.lua.state.0, -1) as *mut BorrowTable;
            internal::lua_pop(self.lua.state.0, 1);
            let mut borrow_store = (&*borrow_store).borrow_mut();
            borrow_store.insert(
                ty,
                Borrow::Immutable(ImmutableBorrow {
                    ptr: val as *const T as *const _,
                }),
            );
        }
        self
    }

    /// Borrows an mutable reference and makes it accessible
    /// to lua for the duration of the call.
    pub fn borrow_mut<T>(self, val: &'a mut T) -> Self
    where
        T: Any,
    {
        unsafe {
            let ty = any::TypeId::of::<T>();

            sys::lua_getfield(
                self.lua.state.0,
                i32::from(sys::LUA_REGISTRYINDEX),
                b"borrow_store\0".as_ptr() as *const _,
            );
            let borrow_store = sys::lua_touserdata(self.lua.state.0, -1) as *mut BorrowTable;
            internal::lua_pop(self.lua.state.0, 1);
            let mut borrow_store = (&*borrow_store).borrow_mut();
            borrow_store.insert(
                ty,
                Borrow::Mutable(MutableBorrow {
                    ptr: val as *mut _ as *mut AnyType,
                    state: Cell::new(BorrowState::None),
                }),
            );
        }
        self
    }

    /// Invokes the named function in the global scope passing the parameters
    /// to the function and converting the result to the requested type.
    pub fn invoke_function<P: Value, Ret: Value>(self, name: &str, param: P) -> Result<Ret, Error> {
        let c_name = CString::new(name).unwrap();
        unsafe {
            // Used to validate the stack after use
            #[cfg(debug_assertions)]
            let orig_top = sys::lua_gettop(self.lua.state.0);

            sys::lua_getfield(
                self.lua.state.0,
                i32::from(sys::LUA_GLOBALSINDEX),
                c_name.as_ptr(),
            );
            param.to_lua(&self.lua.state).unwrap();
            let res = sys::lua_pcall(self.lua.state.0, P::stack_size(), Ret::stack_size(), 0);
            if res != 0 {
                // Pop the error off the stack and return it
                let ret: Ref<String> = match internal::InternalValue::to_rust(&self.lua.state, -1) {
                    Ok(val) => val,
                    Err(err) => {
                        internal::lua_pop(self.lua.state.0, 1);
                        return Err(err);
                    }
                };
                internal::lua_pop(self.lua.state.0, 1);
                return Err(Error::Raw {
                    msg: ret.to_string().into_boxed_str(),
                });
            }
            // Try and make the type into something we can work with
            let ret = Ret::to_rust(&self.lua.state, -Ret::stack_size());
            // Clean up the stack
            internal::lua_pop(self.lua.state.0, Ret::stack_size());

            // Validate the stack size
            #[cfg(debug_assertions)]
            debug_assert_eq!(orig_top, sys::lua_gettop(self.lua.state.0));
            ret
        }
    }
}

impl<'a> Drop for BorrowBuilder<'a> {
    fn drop(&mut self) {
        unsafe {
            sys::lua_getfield(
                self.lua.state.0,
                i32::from(sys::LUA_REGISTRYINDEX),
                b"borrow_store\0".as_ptr() as *const _,
            );
            let borrow_store = sys::lua_touserdata(self.lua.state.0, -1) as *mut BorrowTable;
            internal::lua_pop(self.lua.state.0, 1);
            let mut borrow_store = (&*borrow_store).borrow_mut();
            for val in borrow_store.values_mut() {
                *val = Borrow::Empty;
            }
        }
    }
}

/// A value that lives on the stack.
///
/// Copy of the value is used when transfering between
/// lua and rust.
pub trait Value: internal::InternalValue {}

// Type impls

impl Value for () {}
unsafe impl internal::InternalValue for () {
    unsafe fn to_rust(_state: &Rc<internal::LuaState>, _idx: i32) -> Result<Self, Error> {
        Ok(())
    }

    fn stack_size() -> i32 {
        0
    }

    unsafe fn to_lua(self, _state: &Rc<internal::LuaState>) -> Result<(), Error> {
        Ok(())
    }
}

impl Value for f64 {}
unsafe impl internal::InternalValue for f64 {
    unsafe fn to_rust(state: &Rc<internal::LuaState>, idx: i32) -> Result<Self, Error> {
        if sys::lua_isnumber(state.0, idx) != 0 {
            Ok(sys::lua_tonumber(state.0, idx))
        } else {
            Err(Error::TypeMismatch { wanted: "Number" })
        }
    }

    fn stack_size() -> i32 {
        1
    }

    unsafe fn to_lua(self, state: &Rc<internal::LuaState>) -> Result<(), Error> {
        sys::lua_pushnumber(state.0, self);
        Ok(())
    }
}

impl Value for i32 {}
unsafe impl internal::InternalValue for i32 {
    unsafe fn to_rust(state: &Rc<internal::LuaState>, idx: i32) -> Result<Self, Error> {
        if sys::lua_isnumber(state.0, idx) != 0 {
            Ok(sys::lua_tonumber(state.0, idx) as i32)
        } else {
            Err(Error::TypeMismatch { wanted: "Number" })
        }
    }

    fn stack_size() -> i32 {
        1
    }

    unsafe fn to_lua(self, state: &Rc<internal::LuaState>) -> Result<(), Error> {
        sys::lua_pushnumber(state.0, f64::from(self));
        Ok(())
    }
}

impl Value for bool {}
unsafe impl internal::InternalValue for bool {
    unsafe fn to_rust(state: &Rc<internal::LuaState>, idx: i32) -> Result<Self, Error> {
        Ok(sys::lua_toboolean(state.0, idx) != 0)
    }

    fn stack_size() -> i32 {
        1
    }

    unsafe fn to_lua(self, state: &Rc<internal::LuaState>) -> Result<(), Error> {
        sys::lua_pushboolean(state.0, self as i32);
        Ok(())
    }
}

impl<T> Value for Option<T> where T: Value {}
unsafe impl<T> internal::InternalValue for Option<T>
where
    T: internal::InternalValue,
{
    unsafe fn to_rust(state: &Rc<internal::LuaState>, idx: i32) -> Result<Self, Error> {
        if sys::lua_type(state.0, idx) == i32::from(sys::LUA_TNIL) {
            Ok(None)
        } else {
            Ok(Some(T::to_rust(state, idx)?))
        }
    }

    fn stack_size() -> i32 {
        T::stack_size()
    }

    unsafe fn to_lua(self, state: &Rc<internal::LuaState>) -> Result<(), Error> {
        match self {
            Some(val) => val.to_lua(state)?,
            None => {
                for _ in 0..Self::stack_size() {
                    sys::lua_pushnil(state.0)
                }
            }
        };
        Ok(())
    }
}

impl<T, E> Value for Result<T, E>
where
    T: Value,
    E: Display,
{
}
unsafe impl<T, E> internal::InternalValue for Result<T, E>
where
    T: internal::InternalValue,
    E: Display,
{
    unsafe fn to_rust(state: &Rc<internal::LuaState>, idx: i32) -> Result<Self, Error> {
        Ok(Ok(T::to_rust(state, idx)?))
    }

    fn stack_size() -> i32 {
        T::stack_size()
    }

    unsafe fn to_lua(self, state: &Rc<internal::LuaState>) -> Result<(), Error> {
        match self {
            Ok(val) => val.to_lua(state)?,
            Err(err) => {
                return Err(Error::External {
                    err: format!("{}", err).into_boxed_str(),
                })
            }
        };
        Ok(())
    }
}

/// Reference to a value in a lua engine
pub struct Ref<T> {
    value: i32,
    state: Weak<internal::LuaState>,
    _t: PhantomData<T>,
}

impl<T> Value for Ref<T> where Ref<T>: internal::InternalValue {}

impl<T> PartialEq for Ref<T> {
    fn eq(&self, other: &Self) -> bool {
        unsafe {
            let state = if let Some(state) = self.state.upgrade() {
                state
            } else {
                return false;
            };
            sys::lua_rawgeti(state.0, i32::from(sys::LUA_REGISTRYINDEX), self.value);
            sys::lua_rawgeti(state.0, i32::from(sys::LUA_REGISTRYINDEX), other.value);
            let ret = sys::lua_rawequal(state.0, -1, -2);
            internal::lua_pop(state.0, 2);
            ret != 0
        }
    }
}

/// Any lua type
pub enum Unknown {}
impl<T> Ref<T> {
    /// Removes the type information about this reference
    pub fn into_unknown(mut self) -> Ref<Unknown> {
        use std::mem;
        let r = Ref {
            value: self.value,
            state: mem::replace(&mut self.state, Weak::new()),
            _t: PhantomData,
        };
        // We are reusing the reference this has and need to prevent
        // the old reference from freeing it
        mem::forget(self);
        r
    }
}

impl Ref<Unknown> {
    /// Creates a reference to a nil value
    pub fn new_nil(lua: &Lua) -> Ref<Unknown> {
        unsafe {
            let state = internal::LuaState::root(lua.state.clone());
            sys::lua_pushnil(state.0);
            let r = sys::luaL_ref(state.0, i32::from(sys::LUA_REGISTRYINDEX));
            Ref {
                value: r,
                state: Rc::downgrade(&state),
                _t: PhantomData,
            }
        }
    }

    /// Creates a reference to any lua type
    pub fn new_unknown<V>(lua: &Lua, v: V) -> Ref<Unknown>
    where
        V: Value,
    {
        unsafe {
            let state = internal::LuaState::root(lua.state.clone());
            v.to_lua(&state)
                .expect("Failed to push value on to the lua stack");
            let r = sys::luaL_ref(state.0, i32::from(sys::LUA_REGISTRYINDEX));
            Ref {
                value: r,
                state: Rc::downgrade(&state),
                _t: PhantomData,
            }
        }
    }

    /// Returns whether this value is nil
    pub fn is_nil(&self) -> bool {
        unsafe {
            let state = if let Some(state) = self.state.upgrade() {
                state
            } else {
                return false;
            };
            sys::lua_rawgeti(state.0, i32::from(sys::LUA_REGISTRYINDEX), self.value);
            let ret = sys::lua_type(state.0, -1) == i32::from(sys::LUA_TNIL);
            internal::lua_pop(state.0, 1);
            ret
        }
    }

    /// Tries to convert the unknown type into the target type
    pub fn try_convert<T: Value>(&self) -> Result<T, Error> {
        unsafe {
            let state = if let Some(state) = self.state.upgrade() {
                state
            } else {
                return Err(Error::Shutdown);
            };
            sys::lua_rawgeti(state.0, i32::from(sys::LUA_REGISTRYINDEX), self.value);
            let val = T::to_rust(&state, -T::stack_size());
            internal::lua_pop(state.0, T::stack_size());
            val
        }
    }
}

unsafe impl internal::InternalValue for Ref<Unknown> {
    unsafe fn to_rust(state: &Rc<internal::LuaState>, idx: i32) -> Result<Self, Error> {
        sys::lua_pushvalue(state.0, idx);
        let r = sys::luaL_ref(state.0, i32::from(sys::LUA_REGISTRYINDEX));
        let state = internal::LuaState::root(state.clone());
        Ok(Ref {
            value: r,
            state: Rc::downgrade(&state),
            _t: PhantomData,
        })
    }

    fn stack_size() -> i32 {
        1
    }

    unsafe fn to_lua(self, state: &Rc<internal::LuaState>) -> Result<(), Error> {
        sys::lua_rawgeti(state.0, i32::from(sys::LUA_REGISTRYINDEX), self.value);
        Ok(())
    }
}

// Strings

impl Ref<String> {
    /// Places the passed string onto the lua heap and returns a
    /// reference to it.
    #[inline]
    pub fn new_string<S: Into<Vec<u8>>>(lua: &Lua, s: S) -> Ref<String> {
        unsafe {
            let s = CString::new(s).unwrap();
            let state = internal::LuaState::root(lua.state.clone());
            sys::lua_pushstring(state.0, s.as_ptr());
            let r = sys::luaL_ref(state.0, i32::from(sys::LUA_REGISTRYINDEX));
            Ref {
                value: r,
                state: Rc::downgrade(&state),
                _t: PhantomData,
            }
        }
    }
    /// Places the passed buffer onto the lua heap and returns a
    /// reference to it.
    #[inline]
    pub fn new_string_buf(lua: &Lua, buf: &[u8]) -> Ref<String> {
        unsafe {
            assert!(buf.last() == Some(&0));
            let state = internal::LuaState::root(lua.state.clone());
            sys::lua_pushstring(state.0, buf.as_ptr() as *const _);
            let r = sys::luaL_ref(state.0, i32::from(sys::LUA_REGISTRYINDEX));
            Ref {
                value: r,
                state: Rc::downgrade(&state),
                _t: PhantomData,
            }
        }
    }
}

unsafe impl internal::InternalValue for Ref<String> {
    unsafe fn to_rust(state: &Rc<internal::LuaState>, idx: i32) -> Result<Self, Error> {
        if sys::lua_isstring(state.0, idx) != 0 {
            sys::lua_pushvalue(state.0, idx);
            let r = sys::luaL_ref(state.0, i32::from(sys::LUA_REGISTRYINDEX));
            let state = internal::LuaState::root(state.clone());
            Ok(Ref {
                value: r,
                state: Rc::downgrade(&state),
                _t: PhantomData,
            })
        } else {
            Err(Error::TypeMismatch { wanted: "String" })
        }
    }

    fn stack_size() -> i32 {
        1
    }

    unsafe fn to_lua(self, state: &Rc<internal::LuaState>) -> Result<(), Error> {
        sys::lua_rawgeti(state.0, i32::from(sys::LUA_REGISTRYINDEX), self.value);
        Ok(())
    }
}

impl Deref for Ref<String> {
    type Target = str;
    fn deref(&self) -> &str {
        unsafe {
            let state = if let Some(state) = self.state.upgrade() {
                state
            } else {
                return "";
            };
            sys::lua_rawgeti(state.0, i32::from(sys::LUA_REGISTRYINDEX), self.value);
            let cstr = CStr::from_ptr(sys::lua_tolstring(state.0, -1, ptr::null_mut()));
            internal::lua_pop(state.0, 1);
            cstr.to_str().unwrap_or("")
        }
    }
}

impl Display for Ref<String> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        Display::fmt(self.deref(), f)
    }
}

impl Debug for Ref<String> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        Debug::fmt(self.deref(), f)
    }
}

// Tables

/// A lua table.
///
/// This type is created by the `Ref::new_table` method
pub enum Table {}

impl Ref<Table> {
    /// Creates an empty table on the lua heap
    #[inline]
    pub fn new_table(lua: &Lua) -> Ref<Table> {
        unsafe {
            sys::lua_createtable(lua.state.0, 0, 0);
            let r = sys::luaL_ref(lua.state.0, i32::from(sys::LUA_REGISTRYINDEX));
            Ref {
                value: r,
                state: Rc::downgrade(&internal::LuaState::root(lua.state.clone())),
                _t: PhantomData,
            }
        }
    }

    /// Inserts the passed value into the table with the given key
    #[inline]
    pub fn insert<K, V>(&self, k: K, v: V)
    where
        K: Value,
        V: Value,
    {
        unsafe {
            let state = if let Some(state) = self.state.upgrade() {
                state
            } else {
                return;
            };
            sys::lua_rawgeti(state.0, i32::from(sys::LUA_REGISTRYINDEX), self.value);
            k.to_lua(&state).unwrap();
            v.to_lua(&state).unwrap();
            sys::lua_rawset(state.0, -3);
            internal::lua_pop(state.0, 1);
        }
    }

    /// Gets the value with the given key from the table.
    ///
    /// Returns `None` if the value doesn't exist or the
    /// value can't be converted into the required type
    #[inline]
    pub fn get<K, V>(&self, k: K) -> Option<V>
    where
        K: Value,
        V: Value,
    {
        unsafe {
            let state = if let Some(state) = self.state.upgrade() {
                state
            } else {
                return None;
            };
            sys::lua_rawgeti(state.0, i32::from(sys::LUA_REGISTRYINDEX), self.value);
            k.to_lua(&state).unwrap();
            sys::lua_rawget(state.0, -2);
            let val = V::to_rust(&state, -1);
            internal::lua_pop(state.0, 2);
            val.ok()
        }
    }

    /// Returns the 'length' of this table.
    ///
    /// This is the same as lua's `#` operator. Only returns
    /// useful results when the table is structured as an
    /// array.
    pub fn length(&self) -> i32 {
        unsafe {
            let state = if let Some(state) = self.state.upgrade() {
                state
            } else {
                return 0;
            };
            sys::lua_rawgeti(state.0, i32::from(sys::LUA_REGISTRYINDEX), self.value);
            let len = sys::lua_objlen(state.0, -1);
            internal::lua_pop(state.0, 1);
            len as i32
        }
    }

    /// Returns an iterator over the table's values
    pub fn iter<K, V>(&self) -> TableIterator<K, V>
    where
        K: Value,
        V: Value,
    {
        let state = if let Some(state) = self.state.upgrade() {
            state
        } else {
            panic!("Table::iter on a shutdown lua instance")
        };
        unsafe {
            sys::lua_rawgeti(state.0, i32::from(sys::LUA_REGISTRYINDEX), self.value);
            sys::lua_pushnil(state.0);
        }
        TableIterator {
            state: state,
            ended: false,
            _l: PhantomData,
            _k: PhantomData,
            _v: PhantomData,
        }
    }
}

/// Deserializes the given type from the table
#[allow(clippy::redundant_closure)]
pub fn from_table<T>(tbl: &Ref<Table>) -> Result<T, Error>
where
    T: for<'a> serde::Deserialize<'a>,
{
    with_table_deserializer(tbl, |de| T::deserialize(de)).map_err(|v| v.0)
}

/// Deserializes from a table by running the passed function
/// with a deserializer
pub fn with_table_deserializer<F, Ret, E>(tbl: &Ref<Table>, f: F) -> Result<Ret, E>
where
    F: for<'a> FnOnce(&mut serde_support::Deserializer<'a>) -> Result<Ret, E>,
{
    unsafe {
        let state = if let Some(state) = tbl.state.upgrade() {
            state
        } else {
            panic!("Lua instance shutdown")
        };
        sys::lua_rawgeti(state.0, i32::from(sys::LUA_REGISTRYINDEX), tbl.value);
        let tbl_idx = sys::lua_gettop(state.0);
        let mut de = serde_support::Deserializer {
            state: &state,
            idx: tbl_idx,
        };
        let v = f(&mut de);
        internal::lua_pop(state.0, 1);
        v
    }
}

/// Serializes the given value to a lua table
pub fn to_table<T>(lua: &Lua, value: &T) -> Result<Ref<Table>, Error>
where
    T: serde::Serialize,
{
    with_table_serializer(lua, |s| value.serialize(s))
}

/// Serializes to a table by running the passed function
/// with a serializer
pub fn with_table_serializer<F>(lua: &Lua, f: F) -> Result<Ref<Table>, Error>
where
    F: for<'se> FnOnce(&mut serde_support::Serializer<'se>) -> Result<(), serde_support::SError>,
{
    unsafe {
        let mut se = serde_support::Serializer { state: &lua.state };
        f(&mut se).map_err(|v| v.0)?;
        if sys::lua_type(lua.state.0, -1) != i32::from(sys::LUA_TTABLE) {
            internal::lua_pop(lua.state.0, 1);
            return Err(Error::Raw {
                msg: "failed to serialize as a table".into(),
            });
        }
        let r = sys::luaL_ref(lua.state.0, i32::from(sys::LUA_REGISTRYINDEX));
        Ok(Ref {
            value: r,
            state: Rc::downgrade(&internal::LuaState::root(lua.state.clone())),
            _t: PhantomData,
        })
    }
}

pub struct TableIterator<'a, K, V>
where
    K: Value,
    V: Value,
{
    state: Rc<internal::LuaState>,
    ended: bool,
    _l: PhantomData<&'a ()>,
    _k: PhantomData<K>,
    _v: PhantomData<V>,
}

impl<'a, K, V> Iterator for TableIterator<'a, K, V>
where
    K: Value,
    V: Value,
{
    type Item = (K, V);
    fn next(&mut self) -> Option<(K, V)> {
        if self.ended {
            return None;
        }
        unsafe {
            loop {
                if sys::lua_next(self.state.0, -2) != 0 {
                    let key = K::to_rust(&self.state, -2);
                    let val = V::to_rust(&self.state, -1);
                    internal::lua_pop(self.state.0, 1);
                    if let (Ok(key), Ok(val)) = (key, val) {
                        return Some((key, val));
                    } else {
                        continue;
                    }
                } else {
                    internal::lua_pop(self.state.0, 1);
                    self.ended = true;
                    return None;
                }
            }
        }
    }
}
impl<'a, K, V> Drop for TableIterator<'a, K, V>
where
    K: Value,
    V: Value,
{
    fn drop(&mut self) {
        if !self.ended {
            unsafe {
                internal::lua_pop(self.state.0, 1);
            }
        }
    }
}

unsafe impl internal::InternalValue for Ref<Table> {
    unsafe fn to_rust(state: &Rc<internal::LuaState>, idx: i32) -> Result<Self, Error> {
        if sys::lua_type(state.0, idx) == i32::from(sys::LUA_TTABLE) {
            sys::lua_pushvalue(state.0, idx);
            let r = sys::luaL_ref(state.0, i32::from(sys::LUA_REGISTRYINDEX));
            Ok(Ref {
                value: r,
                state: Rc::downgrade(&internal::LuaState::root(state.clone())),
                _t: PhantomData,
            })
        } else {
            Err(Error::TypeMismatch { wanted: "Table" })
        }
    }

    fn stack_size() -> i32 {
        1
    }

    unsafe fn to_lua(self, state: &Rc<internal::LuaState>) -> Result<(), Error> {
        sys::lua_rawgeti(state.0, i32::from(sys::LUA_REGISTRYINDEX), self.value);
        Ok(())
    }
}

// Coroutine

/// A lua coroutine
pub enum Coroutine {}

unsafe impl internal::InternalValue for Ref<Coroutine> {
    unsafe fn to_rust(state: &Rc<internal::LuaState>, idx: i32) -> Result<Self, Error> {
        if sys::lua_type(state.0, idx) == i32::from(sys::LUA_TTHREAD) {
            sys::lua_pushvalue(state.0, idx);
            let r = sys::luaL_ref(state.0, i32::from(sys::LUA_REGISTRYINDEX));
            Ok(Ref {
                value: r,
                state: Rc::downgrade(&internal::LuaState::root(state.clone())),
                _t: PhantomData,
            })
        } else {
            Err(Error::TypeMismatch {
                wanted: "Coroutine",
            })
        }
    }

    fn stack_size() -> i32 {
        1
    }

    unsafe fn to_lua(self, state: &Rc<internal::LuaState>) -> Result<(), Error> {
        sys::lua_rawgeti(state.0, i32::from(sys::LUA_REGISTRYINDEX), self.value);
        Ok(())
    }
}

// Functions

/// A lua function.
///
/// This type is created by the `Ref::new_function` method
pub enum Function {}

impl Ref<Function> {
    /// Creates a function from the given lua source
    pub fn new_function(lua: &Lua, source: &str) -> Ref<Function> {
        let c_script = CString::new(source).unwrap();
        let c_name = CString::new("<inline function>").unwrap();
        unsafe {
            // Load and parse the code into the vm
            let status = sys::luaL_loadbuffer(
                lua.state.0,
                c_script.as_ptr(),
                source.len(),
                c_name.as_ptr(),
            );
            if status != 0 {
                // Pop the error off the stack and return it
                let ret = CStr::from_ptr(sys::lua_tolstring(lua.state.0, -1, ptr::null_mut()));
                internal::lua_pop(lua.state.0, 1);
                panic!("{}", ret.to_string_lossy());
            }
            let r = sys::luaL_ref(lua.state.0, i32::from(sys::LUA_REGISTRYINDEX));
            Ref {
                value: r,
                state: Rc::downgrade(&internal::LuaState::root(lua.state.clone())),
                _t: PhantomData,
            }
        }
    }

    /// Invokes the stored function passing the parameters
    /// to the function and converting the result to the requested type.
    pub fn invoke<P: Value, Ret: Value>(&self, param: P) -> Result<Ret, Error> {
        unsafe {
            let state = if let Some(state) = self.state.upgrade() {
                state
            } else {
                return Err(Error::Shutdown);
            };
            // Used to validate the stack after use
            #[cfg(debug_assertions)]
            let orig_top = sys::lua_gettop(state.0);

            sys::lua_rawgeti(state.0, i32::from(sys::LUA_REGISTRYINDEX), self.value);
            param.to_lua(&state).unwrap();
            let res = sys::lua_pcall(state.0, P::stack_size(), Ret::stack_size(), 0);
            if res != 0 {
                // Pop the error off the stack and return it
                let ret: Ref<String> = match internal::InternalValue::to_rust(&state, -1) {
                    Ok(val) => val,
                    Err(err) => {
                        internal::lua_pop(state.0, 1);
                        return Err(err);
                    }
                };
                internal::lua_pop(state.0, 1);
                return Err(Error::Raw {
                    msg: ret.to_string().into_boxed_str(),
                });
            }
            // Try and make the type into something we can work with
            let ret = Ret::to_rust(&state, -Ret::stack_size());
            // Clean up the stack
            internal::lua_pop(state.0, Ret::stack_size());

            // Validate the stack size
            #[cfg(debug_assertions)]
            debug_assert_eq!(orig_top, sys::lua_gettop(state.0));
            ret
        }
    }
}

unsafe impl internal::InternalValue for Ref<Function> {
    unsafe fn to_rust(state: &Rc<internal::LuaState>, idx: i32) -> Result<Self, Error> {
        if sys::lua_type(state.0, idx) == i32::from(sys::LUA_TFUNCTION) {
            sys::lua_pushvalue(state.0, idx);
            let r = sys::luaL_ref(state.0, i32::from(sys::LUA_REGISTRYINDEX));
            Ok(Ref {
                value: r,
                state: Rc::downgrade(&internal::LuaState::root(state.clone())),
                _t: PhantomData,
            })
        } else {
            Err(Error::TypeMismatch { wanted: "Function" })
        }
    }

    fn stack_size() -> i32 {
        1
    }

    unsafe fn to_lua(self, state: &Rc<internal::LuaState>) -> Result<(), Error> {
        sys::lua_rawgeti(state.0, i32::from(sys::LUA_REGISTRYINDEX), self.value);
        Ok(())
    }
}

// Userdata

/// Marks a type as safe to pass to and from lua via
/// `Ref`
pub trait LuaUsable: Any {
    /// Adds fields to the type that can be used
    /// in lua
    fn fields(_t: &TypeBuilder) {}

    /// Adds fields to the type's metatable
    fn metatable(_t: &TypeBuilder) {}
}

impl<T> LuaUsable for RefCell<T>
where
    T: LuaUsable,
{
    fn fields(t: &TypeBuilder) {
        T::fields(t)
    }
    fn metatable(t: &TypeBuilder) {
        T::metatable(t)
    }
}

impl<T> LuaUsable for Option<T>
where
    T: LuaUsable,
{
    fn fields(t: &TypeBuilder) {
        T::fields(t)
    }
    fn metatable(t: &TypeBuilder) {
        T::metatable(t)
    }
}
impl LuaUsable for i8 {}
impl LuaUsable for i16 {}
impl LuaUsable for i32 {}
impl LuaUsable for i64 {}
impl LuaUsable for u8 {}
impl LuaUsable for u16 {}
impl LuaUsable for u32 {}
impl LuaUsable for u64 {}
impl LuaUsable for f32 {}
impl LuaUsable for f64 {}
impl LuaUsable for bool {}

impl<K: 'static, V: 'static, H: 'static> LuaUsable for ::std::collections::HashMap<K, V, H> {}
impl<T: 'static> LuaUsable for Vec<T> {}
impl<T: 'static> LuaUsable for ::std::sync::Arc<T> {}
impl<T: 'static> LuaUsable for ::std::rc::Rc<T> {}
impl<T: 'static> LuaUsable for ::std::rc::Weak<T> {}

/// Used to append fields to a custom lua type
pub struct TypeBuilder {
    /// Access to the lua engine
    pub lua: Lua,
}

impl TypeBuilder {
    /// Adds the field to the type currently being built
    pub fn field<T>(&self, name: &str, val: T)
    where
        T: Value,
    {
        unsafe {
            internal::push_string(self.lua.state.0, name);
            val.to_lua(&self.lua.state).unwrap();
            sys::lua_rawset(self.lua.state.0, -3);
        }
    }

    /// Gets the field from the type currently being built
    pub fn get_field<T>(&self, name: &str) -> T
    where
        T: Value,
    {
        unsafe {
            internal::push_string(self.lua.state.0, name);
            sys::lua_rawget(self.lua.state.0, -2);
            let val = T::to_rust(&self.lua.state, -1);
            internal::lua_pop(self.lua.state.0, 1);
            val.unwrap()
        }
    }

    pub fn metatable<F>(&self, f: F)
    where
        F: FnOnce(&TypeBuilder),
    {
        unsafe {
            sys::lua_createtable(self.lua.state.0, 0, 0);
            f(&TypeBuilder {
                lua: Lua {
                    state: self.lua.state.clone(),
                },
            });
            sys::lua_setmetatable(self.lua.state.0, -2);
        }
    }
}

type UserdataTable = RefCell<HashMap<any::TypeId, i32>>;

impl<T> Ref<T>
where
    T: LuaUsable,
{
    /// Places the value on the lua heap
    pub fn new(lua: &Lua, val: T) -> Ref<T> {
        use std::mem;

        unsafe {
            sys::lua_getfield(
                lua.state.0,
                i32::from(sys::LUA_REGISTRYINDEX),
                b"userdata_store\0".as_ptr() as *const _,
            );
            let userdata_store = sys::lua_touserdata(lua.state.0, -1) as *mut UserdataTable;
            internal::lua_pop(lua.state.0, 1);

            let ty = any::TypeId::of::<T>();
            // Use lua user data to store the value in lua space
            let data = sys::lua_newuserdata(lua.state.0, mem::size_of::<T>());
            ptr::write(data as *mut T, val);
            unsafe extern "C" fn free_value<T: Any>(
                state: *mut sys::lua_State,
            ) -> sys::libc::c_int {
                let val: *mut T = sys::lua_touserdata(state, 1) as *mut T;
                ptr::drop_in_place(val);
                0
            }
            {
                let user_data_map = (&*userdata_store).borrow();
                if let Some(user_data) = user_data_map.get(&ty) {
                    // Install the metatable
                    sys::lua_rawgeti(lua.state.0, i32::from(sys::LUA_REGISTRYINDEX), *user_data);
                    sys::lua_setmetatable(lua.state.0, -2);
                    let r = sys::luaL_ref(lua.state.0, i32::from(sys::LUA_REGISTRYINDEX));

                    return Ref {
                        value: r,
                        state: Rc::downgrade(&internal::LuaState::root(lua.state.clone())),
                        _t: PhantomData,
                    };
                }
            }
            // Create/get a metatable so that we can free the userdata once the value isn't
            // in use any more.
            let user_data = {
                sys::lua_createtable(lua.state.0, 0, 3);

                internal::push_string(lua.state.0, "__index");
                sys::lua_createtable(lua.state.0, 0, 0);
                T::fields(&TypeBuilder {
                    lua: Lua {
                        state: lua.state.clone(),
                    },
                });
                sys::lua_settable(lua.state.0, -3);

                internal::push_string(lua.state.0, "__gc");
                sys::lua_pushcclosure(lua.state.0, Some(free_value::<T>), 0);
                sys::lua_settable(lua.state.0, -3);

                T::metatable(&TypeBuilder {
                    lua: Lua {
                        state: lua.state.clone(),
                    },
                });

                // Lock the table
                internal::push_string(lua.state.0, "__metatable");
                internal::InternalValue::to_lua(false, &lua.state).unwrap();
                sys::lua_settable(lua.state.0, -3);
                sys::luaL_ref(lua.state.0, i32::from(sys::LUA_REGISTRYINDEX))
            };

            let mut user_data_map = (&*userdata_store).borrow_mut();
            user_data_map.insert(ty, user_data);
            // Install the metatable
            sys::lua_rawgeti(lua.state.0, i32::from(sys::LUA_REGISTRYINDEX), user_data);
            sys::lua_setmetatable(lua.state.0, -2);
            let r = sys::luaL_ref(lua.state.0, i32::from(sys::LUA_REGISTRYINDEX));

            Ref {
                value: r,
                state: Rc::downgrade(&internal::LuaState::root(lua.state.clone())),
                _t: PhantomData,
            }
        }
    }
}

impl<T> Deref for Ref<T>
where
    T: LuaUsable,
{
    type Target = T;
    #[inline]
    fn deref(&self) -> &T {
        unsafe {
            let state = if let Some(state) = self.state.upgrade() {
                state
            } else {
                panic!("Lua instance shutdown")
            };
            sys::lua_rawgeti(state.0, i32::from(sys::LUA_REGISTRYINDEX), self.value);
            let val = sys::lua_touserdata(state.0, -1) as *mut T;
            assert!(!val.is_null());
            internal::lua_pop(state.0, 1);
            &*val
        }
    }
}

unsafe impl<T> internal::InternalValue for Ref<T>
where
    T: LuaUsable,
{
    unsafe fn to_rust(state: &Rc<internal::LuaState>, idx: i32) -> Result<Self, Error> {
        if sys::lua_type(state.0, idx) != i32::from(sys::LUA_TUSERDATA) {
            return Err(Error::TypeMismatch {
                wanted: "<native type>",
            });
        }
        let ty = any::TypeId::of::<T>();
        if sys::lua_getmetatable(state.0, idx) == 0 {
            return Err(Error::TypeMismatch {
                wanted: "<native type>",
            });
        }
        sys::lua_getfield(
            state.0,
            i32::from(sys::LUA_REGISTRYINDEX),
            b"userdata_store\0".as_ptr() as *const _,
        );
        let userdata_store = sys::lua_touserdata(state.0, -1) as *mut UserdataTable;
        internal::lua_pop(state.0, 1);

        let user_data_map = (&*userdata_store).borrow();
        let user_data = user_data_map[&ty];

        sys::lua_rawgeti(state.0, i32::from(sys::LUA_REGISTRYINDEX), user_data);
        let equal = sys::lua_rawequal(state.0, -1, -2);
        internal::lua_pop(state.0, 2);
        if equal != 0 {
            sys::lua_pushvalue(state.0, idx);
            let r = sys::luaL_ref(state.0, i32::from(sys::LUA_REGISTRYINDEX));
            Ok(Ref {
                value: r,
                state: Rc::downgrade(&internal::LuaState::root(state.clone())),
                _t: PhantomData,
            })
        } else {
            Err(Error::TypeMismatch {
                wanted: "<native type>",
            })
        }
    }

    fn stack_size() -> i32 {
        1
    }

    unsafe fn to_lua(self, state: &Rc<internal::LuaState>) -> Result<(), Error> {
        sys::lua_rawgeti(state.0, i32::from(sys::LUA_REGISTRYINDEX), self.value);
        Ok(())
    }
}

// Common

impl<T> Clone for Ref<T> {
    fn clone(&self) -> Self {
        unsafe {
            let state = if let Some(state) = self.state.upgrade() {
                state
            } else {
                panic!("Lua instance shutdown")
            };
            sys::lua_rawgeti(state.0, i32::from(sys::LUA_REGISTRYINDEX), self.value);
            let r = sys::luaL_ref(state.0, i32::from(sys::LUA_REGISTRYINDEX));
            Ref {
                value: r,
                state: self.state.clone(),
                _t: PhantomData,
            }
        }
    }
}

impl<T> Drop for Ref<T> {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            if let Some(state) = self.state.upgrade() {
                sys::luaL_unref(state.0, i32::from(sys::LUA_REGISTRYINDEX), self.value);
            }
        }
    }
}

/// Standard lua error type
#[derive(Debug, PartialEq, Eq, Fail)]
pub enum Error {
    /// An error caused by a value not being the expected type.
    ///
    /// Contains the name of the expected type
    #[fail(display = "type mismatch: wanted {}", wanted)]
    TypeMismatch { wanted: &'static str },
    /// A raw error normally from lua itself
    #[fail(display = "{}", msg)]
    Raw { msg: Box<str> },
    /// An error from an external source e.g. a panicking
    /// closure.
    #[fail(display = "external error: {}", err)]
    External { err: Box<str> },
    /// Unsupported (de)serialization type
    #[fail(display = "unsupported type: {}", ty)]
    UnsupportedType { ty: &'static str },
    /// Unsupported (de)serialization type
    #[fail(display = "unsupported type: {}", ty)]
    UnsupportedDynamicType { ty: String },
    #[fail(display = "the lua instance has be shutdown")]
    Shutdown,
}

macro_rules! impl_tuple {
    ($($param:ident),+) => (
        impl <$($param: Value),+> Value for ($($param),+) {}

        unsafe impl <$($param: Value),+> internal::InternalValue for ($($param),+) {

            #[allow(unused_assignments, clippy::eval_order_dependence)]
            unsafe fn to_rust(state: &Rc<internal::LuaState>, idx: i32) -> Result<Self, Error> {
                let mut idx = idx;
                Ok(($(
                    {
                        let val = $param::to_rust(state, idx)?;
                        idx += $param::stack_size();
                        val

                    }
                ),*))
            }

            #[inline]
            fn stack_size() -> i32 {
                let mut size = 0;
                $(
                    size += $param::stack_size();
                )*
                size
            }

            #[allow(non_snake_case)]
            unsafe fn to_lua(self, state: &Rc<internal::LuaState>) -> Result<(), Error> {
                let ($($param),*) = self;
                $(
                    $param.to_lua(state)?;
                )*
                Ok(())
            }
        }
    )
}

impl_tuple!(A, B);
impl_tuple!(A, B, C);
impl_tuple!(A, B, C, D);
impl_tuple!(A, B, C, D, E);
impl_tuple!(A, B, C, D, E, F);
impl_tuple!(A, B, C, D, E, F, G);
impl_tuple!(A, B, C, D, E, F, G, H);
impl_tuple!(A, B, C, D, E, F, G, H, I);
impl_tuple!(A, B, C, D, E, F, G, H, I, J);
impl_tuple!(A, B, C, D, E, F, G, H, I, J, K);
impl_tuple!(A, B, C, D, E, F, G, H, I, J, K, L);

macro_rules! impl_closure {
    ($name:ident $num:expr, $($param:ident),*) => (
        #[allow(non_camel_case_types)]
        #[allow(non_snake_case)]
        #[doc(hidden)]
        pub struct $name<$($param,)* Fun, Ret> {
            fun: Fun,
            $($param: PhantomData<$param>,)*
            _ret: PhantomData<Ret>,
        }
        impl <$($param: Value,)* Ret: Value, Fun: FnMut(&Lua, $($param),*) -> Ret + Any +> Value for $name<$($param,)* Fun, Ret> {}
        unsafe impl <$($param: Value,)* Ret: Value, Fun: FnMut(&Lua, $($param),*) -> Ret + Any +> internal::InternalValue for $name<$($param,)* Fun, Ret> {
            unsafe fn to_lua(self, state: &Rc<internal::LuaState>) -> Result<(), Error> {
                use std::mem;

                #[repr(C)]
                struct ClosureData<F> {
                    cfunc: Option<unsafe extern "C" fn(state: *mut sys::lua_State) -> sys::libc::c_int>,
                    fun: F,
                    lua: Weak<internal::LuaState>,
                }
                let cdata = ClosureData {
                    cfunc: Some(invoke_closure::<$($param,)* Ret, Fun>),
                    fun: self.fun,
                    lua: Rc::downgrade(&internal::LuaState::root(state.clone())),
                };

                extern "C" {
                    fn invoke_rust_closure(state: *mut sys::lua_State) -> sys::libc::c_int;
                }

                let ty = any::TypeId::of::<ClosureData<Fun>>();
                #[allow(unused_variables, unused_mut, unused_assignments, non_snake_case, clippy::redundant_closure_call)]
                unsafe extern "C" fn invoke_closure<$($param: Value,)* Ret: Value, Fun: FnMut(&Lua, $($param),*) -> Ret + Any>(state: *mut sys::lua_State) -> sys::libc::c_int {
                    let result = (|| {
                        if sys::lua_gettop(state) != $num {
                            return Err(Error::Raw {
                                msg: format!("Incorrect number of parameters, wanted: {}", $num).into_boxed_str(),
                            });
                        }
                        let ty = any::TypeId::of::<ClosureData<Fun>>();
                        let func: &mut ClosureData<Fun> = &mut *(sys::lua_touserdata(state, i32::from(sys::LUA_GLOBALSINDEX) - 1) as *mut ClosureData<Fun>);
                        let parent = func.lua.upgrade().ok_or_else(|| Error::Raw { msg: "engine shutting down".into() })?;
                        let lua = if parent.0 == state {
                            Lua {state: parent}
                        } else {
                            Lua {state: Rc::new(internal::LuaState(
                                state,
                                Some(parent)
                            ))}
                        };

                        let mut idx = 1;
                        $(
                            let $param = $param::to_rust(&lua.state, idx)?;
                            idx += $param::stack_size();
                        )*
                        let ret = (func.fun)(&lua, $($param),*);
                        ret.to_lua(&lua.state)?;
                        Ok(Ret::stack_size())
                    })();
                    match result {
                        Ok(val) => val,
                        Err(err) => {
                            // Get the current script location
                            sys::luaL_where(state, 1);
                            internal::push_string(state, format!(" {}", err));
                            sys::lua_concat(state, 2);
                            // Signal to the c wrapper to throw a lua error
                            -1
                        }
                    }
                }
                unsafe extern "C" fn free_closure<$($param: Value,)* Ret: Value, Fun: FnMut(&Lua, $($param),*) -> Ret + Any>(state: *mut sys::lua_State) -> sys::libc::c_int {
                    let func: *mut ClosureData<Fun> = sys::lua_touserdata(state, 1) as *mut ClosureData<Fun>;
                    ptr::drop_in_place(func);
                    0
                }
                // Use lua user data to store the closure in lua space
                let data = sys::lua_newuserdata(state.0, mem::size_of::<ClosureData<Fun>>());
                ptr::write(data as *mut ClosureData<Fun>, cdata);

                // Create/get a metatable so that we can free the closure once the func isn't
                // in use any more.


                sys::lua_getfield(state.0, i32::from(sys::LUA_REGISTRYINDEX), b"userdata_store\0".as_ptr() as *const _);
                let userdata_store = sys::lua_touserdata(state.0, -1) as *mut UserdataTable;
                internal::lua_pop(state.0, 1);
                let mut user_data_map = (&*userdata_store).borrow_mut();

                let user_data = user_data_map.entry(ty).or_insert_with(|| {
                    sys::lua_createtable(state.0, 0, 2);
                    internal::push_string(state.0, "__gc");
                    sys::lua_pushcclosure(state.0, Some(free_closure::<$($param,)* Ret, Fun>), 0);
                    sys::lua_settable(state.0, -3);
                    // Lock the table
                    internal::push_string(state.0, "__metatable");
                    false.to_lua(state).unwrap();
                    sys::lua_settable(state.0, -3);
                    sys::luaL_ref(state.0, i32::from(sys::LUA_REGISTRYINDEX))
                });
                // Install the metatable
                sys::lua_rawgeti(state.0, i32::from(sys::LUA_REGISTRYINDEX), *user_data);
                sys::lua_setmetatable(state.0, -2);
                sys::lua_pushcclosure(state.0, Some(invoke_rust_closure), 1);
                Ok(())
            }

            unsafe fn to_rust(_state: &Rc<internal::LuaState>, _idx: i32) -> Result<Self, Error> {
                panic!("Can't convert a closure back to rust")
            }

            #[inline]
            fn stack_size() -> i32 { 1 }
        }
        /// Wrapper for closures to allow them to be passed to lua
        pub fn $name<$($param: Value,)* Ret: Value, Fun: FnMut(&Lua, $($param),*) -> Ret + Any>(f: Fun) -> impl Value {
            $name {
                fun: f,
                $($param: PhantomData,)*
                _ret: PhantomData,
            }
        }
    )
}

impl_closure!(closure  0, );
impl_closure!(closure1 1, A);
impl_closure!(closure2 2, A, B);
impl_closure!(closure3 3, A, B, C);
impl_closure!(closure4 4, A, B, C, D);
impl_closure!(closure5 5, A, B, C, D, E);
impl_closure!(closure6 6, A, B, C, D, E, F);
impl_closure!(closure7 7, A, B, C, D, E, F, G);
impl_closure!(closure8 8, A, B, C, D, E, F, G, H);

mod internal {
    use super::*;

    pub struct LuaState(pub *mut sys::lua_State, pub Option<Rc<LuaState>>);
    impl LuaState {
        pub fn root(node: Rc<LuaState>) -> Rc<LuaState> {
            if let Some(p) = node.1.clone() {
                Self::root(p)
            } else {
                node
            }
        }
    }
    impl Drop for LuaState {
        fn drop(&mut self) {
            unsafe {
                if self.1.is_none() {
                    sys::lua_getfield(
                        self.0,
                        i32::from(sys::LUA_REGISTRYINDEX),
                        b"userdata_store\0".as_ptr() as *const _,
                    );
                    ptr::drop_in_place(sys::lua_touserdata(self.0, -1) as *mut UserdataTable);
                    internal::lua_pop(self.0, 1);
                    sys::lua_close(self.0);
                }
            }
        }
    }

    pub unsafe trait InternalRef {}

    pub unsafe trait InternalValue: Sized {
        unsafe fn to_rust(state: &Rc<LuaState>, idx: i32) -> Result<Self, Error>;
        // Size of the type on the stack
        fn stack_size() -> i32;

        unsafe fn to_lua(self, state: &Rc<LuaState>) -> Result<(), Error>;
    }

    pub unsafe fn lua_pop(state: *mut sys::lua_State, num: i32) {
        sys::lua_settop(state, -num - 1);
    }

    pub unsafe fn push_string<S>(state: *mut sys::lua_State, s: S)
    where
        S: Into<Vec<u8>>,
    {
        let s = CString::new(s).unwrap();
        sys::lua_pushstring(state, s.as_ptr());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {
        let lua = Lua::new();
        let test: i32 = lua
            .execute_string(
                r#"
return 5 + 5
        "#,
            )
            .unwrap();
        assert_eq!(test, 10);
    }

    #[test]
    fn test_string() {
        let lua = Lua::new();
        let test: Ref<String> = lua
            .execute_string(
                r#"
return "hello world"
        "#,
            )
            .unwrap();
        assert_eq!(&*test, "hello world");
    }

    #[test]
    fn test_invoke() {
        let lua = Lua::new();
        lua.execute_string::<()>(
            r#"
function test_num(a)
    assert(a == 5)
    return a * 3
end

function test_str(a)
    assert(a == "hello world")
end
        "#,
        )
        .unwrap();
        assert_eq!(lua.invoke_function::<_, i32>("test_num", 5), Ok(15));
        let s = Ref::new_string(&lua, "hello world");
        lua.invoke_function::<_, ()>("test_str", s).unwrap();
    }

    #[test]
    fn test_types() {
        let state = Lua::new();

        state.set(Scope::Global, "float_val", 5.0);
        state.set(Scope::Global, "int_val", 76);
        state.set(Scope::Global, "bool_val", true);
        state.set(Scope::Global, "bool_val2", false);
        state.set(
            Scope::Global,
            "string_val",
            Ref::new_string(&state, "hello world"),
        );
        state.set(Scope::Global, "option_val", Some(3.0));
        state.set(Scope::Global, "option_val2", None as Option<f64>);

        let ret = state.execute_string::<(i32, Ref<String>)>(
            r#"
    assert(float_val == 5.0);
    assert(int_val == 76);
    assert(bool_val);
    assert(not bool_val2);
    assert(string_val == "hello world");
    assert(option_val == 3.0);
    assert(option_val2 == nil);
    return 3, "hi";
        "#,
        );
        let ret = ret.as_ref().map(|&(i, ref s)| (i, s.deref()));
        assert_eq!(ret, Ok((3, "hi")));
    }

    #[test]
    fn test_registry() {
        let state = Lua::new();
        state.set(Scope::Registry, "testing", 5);
        assert_eq!(state.get(Scope::Registry, "testing"), Ok(5));
    }

    #[test]
    fn test_return() {
        let state = Lua::new();
        let (a, b) = state
            .execute_string::<(i32, i32)>(
                r#"
    return 5, 7
    "#,
            )
            .unwrap();
        assert_eq!(a, 5);
        assert_eq!(b, 7);
    }

    #[test]
    fn test_return_multi() {
        let state = Lua::new();
        state.set(
            Scope::Global,
            "test",
            closure(|_lua| -> Result<(i32, i32), Error> { Ok((5, 7)) }),
        );
        state
            .execute_string::<()>(
                r#"
    local a, b = test()
    assert(a == 5)
    assert(b == 7)
    "#,
            )
            .unwrap();
        state.set(
            Scope::Global,
            "test2",
            closure(|_lua| -> Option<(i32, i32)> { Some((5, 7)) }),
        );
        state
            .execute_string::<()>(
                r#"
    local a, b = test2()
    assert(a == 5)
    assert(b == 7)
    "#,
            )
            .unwrap();
        state.set(
            Scope::Global,
            "test3",
            closure(|_lua| -> Option<(i32, i32)> { None }),
        );
        state
            .execute_string::<()>(
                r#"
    local a, b = test3()
    assert(a == nil)
    assert(b == nil)
    "#,
            )
            .unwrap();
    }

    #[test]
    fn test_closure() {
        let state = Lua::new();

        state.set(
            Scope::Global,
            "my_func",
            closure1(|_, i: f64| (i * 3.0, i * 5.0)),
        );

        state
            .execute_string::<()>(
                r#"
    print("Start");
    local a, b = my_func(5);
    print("ret: " .. a .. " + " .. b);
    print("End");
    "#,
            )
            .unwrap();
    }

    #[test]
    fn test_closure_error() {
        let state = Lua::new();

        state.set(
            Scope::Global,
            "will_fail",
            closure(|_| -> Result<(), Error> {
                Err(Error::External {
                    err: "Failed".into(),
                })
            }),
        );

        assert!(state
            .execute_string::<()>(
                r#"
    will_fail();
        "#
            )
            .is_err());
    }

    #[test]
    fn test_userdata() {
        use std::cell::RefCell;
        let state = Lua::new();
        struct CustomType {
            thing: i32,
        }
        impl LuaUsable for CustomType {
            fn fields(t: &TypeBuilder) {
                t.field(
                    "change",
                    closure2(|_, c: Ref<RefCell<CustomType>>, val: i32| {
                        let mut c = c.borrow_mut();
                        c.thing = val;
                    }),
                );
            }
        }

        state.set(
            Scope::Global,
            "custom",
            Ref::new(&state, RefCell::new(CustomType { thing: -5 })),
        );
        {
            let c = state
                .get::<Ref<RefCell<CustomType>>>(Scope::Global, "custom")
                .unwrap();
            let c = c.borrow();
            assert_eq!(c.thing, -5);
        }

        state
            .execute_string::<()>(
                r#"
for k, v in pairs(debug.getmetatable(custom)) do
    print(k, v);
end
for k, v in pairs(debug.getregistry()) do
    print(k, v);
end

custom:change(22);
        "#,
            )
            .unwrap();

        {
            let c = state
                .get::<Ref<RefCell<CustomType>>>(Scope::Global, "custom")
                .unwrap();
            let c = c.borrow();
            assert_eq!(c.thing, 22);
        }
    }

    #[test]
    fn test_userdata_data() {
        struct CustomType {
            drop_check: *mut i32,
        }
        impl LuaUsable for CustomType {}
        impl Drop for CustomType {
            fn drop(&mut self) {
                unsafe {
                    (*self.drop_check) += 1;
                }
            }
        }

        let mut drop_check = 0;
        {
            let state = Lua::new();
            state.set(
                Scope::Global,
                "custom",
                Ref::new(
                    &state,
                    CustomType {
                        drop_check: &mut drop_check,
                    },
                ),
            );

            state
                .execute_string::<()>(
                    r#"
    local temp = custom;
            "#,
                )
                .unwrap();
        }

        assert_eq!(drop_check, 1);
    }

    #[test]
    fn test_env() {
        let state = Lua::new();
        state
            .execute_string::<()>(
                r#"
    function trapped()
        print("Halp")
        lib_print("Halp")
        test = "hello"
    end

    local libs = {
        lib_print = function(msg) print("lib: " .. msg) end,
    }
    local trap = {
        print = function(msg) print("TRAPPED: " .. msg) end,
    }
    setmetatable(trap, {
        __index = libs,
        __metatable = false,
    })
    setfenv(trapped, trap)
    debug.sethook(function() error("timeout") end, "", 10000000);
    trapped()
    debug.sethook()
    print("trap global: " .. trap.test)
    "#,
            )
            .unwrap();
    }

    #[test]
    fn test_table() {
        let state = Lua::new();

        state.set(
            Scope::Global,
            "invert_color",
            closure1(|lua, col: Ref<Table>| {
                let ret = Ref::new_table(lua);

                let rs = Ref::new_string(lua, "r");
                if let Some(r) = col.get::<_, i32>(rs.clone()) {
                    ret.insert(rs, 255 - r);
                }

                let gs = Ref::new_string(lua, "g");
                if let Some(g) = col.get::<_, i32>(gs.clone()) {
                    ret.insert(gs, 255 - g);
                }

                let bs = Ref::new_string(lua, "b");
                if let Some(b) = col.get::<_, i32>(bs.clone()) {
                    ret.insert(bs, 255 - b);
                }
                ret
            }),
        );

        state.set(
            Scope::Global,
            "assert",
            closure1(|_, val: bool| -> Result<(), Error> {
                if val {
                    Ok(())
                } else {
                    Err(Error::External {
                        err: "Lua Assert failed".into(),
                    })
                }
            }),
        );

        state
            .execute_string::<()>(
                r#"
    local color = {r = 120, g = 50, b = 255};
    local new_color = invert_color(color);
    assert(new_color.r == 255 - color.r);
    assert(new_color.g == 255 - color.g);
    assert(new_color.b == 255 - color.b);
        "#,
            )
            .unwrap();
    }

    #[test]
    fn test_table_ref() {
        let state = Lua::new();

        state.set(
            Scope::Global,
            "mut_table",
            closure1(|lua, table: Ref<Table>| {
                table.insert(Ref::new_string(lua, "hello"), 55);
            }),
        );

        state.set(
            Scope::Global,
            "assert",
            closure1(|_, val: bool| -> Result<(), Error> {
                if val {
                    Ok(())
                } else {
                    Err(Error::External {
                        err: "Lua Assert failed".into(),
                    })
                }
            }),
        );

        state
            .execute_string::<()>(
                r#"
    local test_table = {hello = 5, world = 6}
    assert(test_table.hello == 5);
    assert(test_table.world == 6);

    mut_table(test_table)

    assert(test_table.hello == 55);
    assert(test_table.world == 6);
        "#,
            )
            .unwrap();
    }

    #[test]
    fn test_borrow() {
        let state = Lua::new();

        let c = 55i32;
        let mut s = String::from("hello");

        state.set(
            Scope::Global,
            "check_borrow",
            closure(|lua| {
                let c = lua.get_borrow::<i32>();
                assert_eq!(*c, 55);
                {
                    let s = lua.read_borrow::<String>();
                    if &*s != "hello" && &*s != "cake" {
                        panic!("String error");
                    }
                }
                {
                    let mut s = lua.write_borrow::<String>();
                    s.push_str(" world");
                }
            }),
        );

        state
            .execute_string::<()>(
                r#"
    function test()
        check_borrow()
    end
        "#,
            )
            .unwrap();

        state
            .with_borrows()
            .borrow(&c)
            .borrow_mut(&mut s)
            .invoke_function::<(), ()>("test", ())
            .unwrap();

        assert_eq!(c, 55);
        assert_eq!(s, "hello world");

        let mut s2 = String::from("cake");

        state
            .with_borrows()
            .borrow(&c)
            .borrow_mut(&mut s2)
            .invoke_function::<(), ()>("test", ())
            .unwrap();

        assert_eq!(c, 55);
        assert_eq!(s2, "cake world");
    }

    #[test]
    fn test_lua_func() {
        let state = Lua::new();

        let func = Ref::new_function(&state, r#"return 67"#);
        assert_eq!(func.invoke::<(), i32>(()).unwrap(), 67);
    }

    #[test]
    fn test_lua_func_ret() {
        let state = Lua::new();

        let func = state
            .execute_string::<Ref<Function>>(
                r#"
        return function(a)
            return function(b)
                return a * b
            end
        end
        "#,
            )
            .unwrap();

        let mul = func.invoke::<i32, Ref<Function>>(6).unwrap();
        assert_eq!(mul.invoke::<i32, i32>(7).unwrap(), 6 * 7);
    }

    #[test]
    fn test_userdata_custom_meta() {
        use std::cell::RefCell;
        let state = Lua::new();
        struct CustomType {
            thing: i32,
        }
        impl LuaUsable for CustomType {
            fn fields(t: &TypeBuilder) {
                t.metatable(|t| {
                    t.field(
                        "__index",
                        Ref::new_function(
                            &t.lua,
                            r#"
                        return function (self, index)
                            return 15
                        end
                    "#,
                        )
                        .invoke::<(), Ref<Function>>(()),
                    );
                })
            }
        }

        state.set(
            Scope::Global,
            "custom",
            Ref::new(&state, RefCell::new(CustomType { thing: -5 })),
        );
        {
            let c = state
                .get::<Ref<RefCell<CustomType>>>(Scope::Global, "custom")
                .unwrap();
            let c = c.borrow();
            assert_eq!(c.thing, -5);
        }

        state
            .execute_string::<()>(
                r#"
        print("custom " .. tostring(custom.thing))
        assert(custom.thing, 15)
        assert(custom.test, 15)
        "#,
            )
            .unwrap();

        {
            let c = state
                .get::<Ref<RefCell<CustomType>>>(Scope::Global, "custom")
                .unwrap();
            let c = c.borrow();
            assert_eq!(c.thing, -5);
        }
    }
}
