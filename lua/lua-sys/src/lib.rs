#![allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    non_snake_case
)]
#![allow(clippy::all)]

pub extern crate libc;

mod lua {
    include!(concat!("lua.rs"));
}

pub const LUA_COLIBNAME: &'static str = "coroutine";
pub const LUA_MATHLIBNAME: &'static str = "math";
pub const LUA_STRLIBNAME: &'static str = "string";
pub const LUA_TABLIBNAME: &'static str = "table";
pub const LUA_IOLIBNAME: &'static str = "io";
pub const LUA_OSLIBNAME: &'static str = "os";
pub const LUA_LOADLIBNAME: &'static str = "package";
pub const LUA_DBLIBNAME: &'static str = "debug";
pub const LUA_BITLIBNAME: &'static str = "bit";
pub const LUA_JITLIBNAME: &'static str = "jit";
pub const LUA_FFILIBNAME: &'static str = "ffi";

// Some missing ones
extern "C" {
    pub fn luaopen_base(L: *mut lua_State) -> libc::c_int;
    pub fn luaopen_math(L: *mut lua_State) -> libc::c_int;
    pub fn luaopen_string(L: *mut lua_State) -> libc::c_int;
    pub fn luaopen_table(L: *mut lua_State) -> libc::c_int;
    pub fn luaopen_io(L: *mut lua_State) -> libc::c_int;
    pub fn luaopen_os(L: *mut lua_State) -> libc::c_int;
    pub fn luaopen_package(L: *mut lua_State) -> libc::c_int;
    pub fn luaopen_debug(L: *mut lua_State) -> libc::c_int;
    pub fn luaopen_bit(L: *mut lua_State) -> libc::c_int;
    pub fn luaopen_jit(L: *mut lua_State) -> libc::c_int;
    pub fn luaopen_ffi(L: *mut lua_State) -> libc::c_int;

    pub fn luaL_openlibs(L: *mut lua_State);
}

pub use self::lua::*;
