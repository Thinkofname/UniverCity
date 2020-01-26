//! Handles server side scripting

use lua::{self, Scope};
use crate::assets;
use crate::level;
use crate::errors;

use crate::util::{FNVMap, FNVSet};
use std::time::SystemTime;
use std::ops::Deref;
use std::cell::RefCell;
use std::sync::Arc;
use std::io::Read;
use crate::prelude::*;
use crate::ecs;

pub use crate::script_room::LuaObject;

/// Script bootstrap code. Public so that the client can use it
pub const SCRIPT_BOOTSTRAP: &str = include_str!("bootstrap.lua");

const SERVER_SCRIPT_BOOTSTRAP: &str = include_str!("server_bootstrap.lua");

/// The registry key used to obtain the list of watched files
const WATCHED_FILES: &str = "watched_files";

/// Contains a list of scripts loaded
struct WatchedFiles {
    next_reload: i32,
    /// List of modules and script file names
    files: FNVMap<(String, String), Option<SystemTime>>,
}
impl lua::LuaUsable for WatchedFiles {}

/// Provides a basic standard library to the passed state
pub fn init_unilib(log: Logger, asset_manager: assets::AssetManager, lua: &lua::Lua) {
    // Prints using rust's `println!` macro which handles locking
    let log1 = log.clone();
    lua.set(Scope::Global, "native_print", lua::closure1(move |_, msg: lua::Ref<String>| info!(log1, "{}", msg)));
    lua.set(Scope::Global, "native_print_mod", lua::closure2(move |_, msg: lua::Ref<String>, m: lua::Ref<String>|
        info!(log, "{}", msg; "module" => %m)
    ));

    // Use registry for storing a list of loaded files
    lua.set(Scope::Registry, WATCHED_FILES, lua::Ref::new(lua, RefCell::new(WatchedFiles {
        next_reload: 120,
        files: FNVMap::default(),
    })));

    lua.set(Scope::Global, "get_module_script", lua::closure2(move |lua, m: lua::Ref<String>, ff: lua::Ref<String>| -> errors::Result<_> {
        let mut f = asset_manager.open_from_pack(assets::ModuleKey::new(&*m), &ff)?;
        let mut out = String::new();
        f.read_to_string(&mut out)?;

        let watched_files: lua::Ref<RefCell<WatchedFiles>> = lua.get(Scope::Registry, WATCHED_FILES)?;
        let mut watched_files = watched_files.borrow_mut();
        let key = (m.to_string(), ff.to_string());
        let time = asset_manager.modified_time(assets::ModuleKey::new(&*key.0), &*key.1);
        watched_files.files.insert(key, time);
        Ok(lua::Ref::new_string(lua, out))
    }));

    init_serialize(lua);
}

/// Adds __index and __newindex fields to the type
/// to support getters and setters. To be used with
/// `TypeBuilder::metatable`
pub fn support_getters_setters(t: &lua::TypeBuilder) {
    use lua::{Ref, Function, Table};
    let orig = t.get_field::<Ref<Table>>("__index");
    t.field("method_store", orig);
    t.field("__index", Ref::new_function(&t.lua, r#"
        return function(self, index)
            local meta = debug.getmetatable(self).method_store
            local raw = rawget(meta, index)
            if raw ~= nil then
                return raw
            end
            local getter = rawget(meta, "get_" .. index)
            if getter ~= nil then
                return getter(self)
            end
            return nil
        end
    "#).invoke::<(), Ref<Function>>(()));
    t.field("__newindex", Ref::new_function(&t.lua, r#"
        return function(self, index, value)
            local meta = debug.getmetatable(self).method_store
            local setter = rawget(meta, "set_" .. index)
            if setter ~= nil then
                return setter(self, value)
            end
            error("No such field")
        end
    "#).invoke::<(), Ref<Function>>(()));
}

/// Indirect access to a scripting engine allowing for methods
/// to be invoked.
///
/// Used to allow for systems to work with both a client and
/// server scripting engine
pub trait Invokable: Deref<Target=lua::Lua> {
}

/// A place where trackable variables can be stored
pub trait TrackStore {
    /// Stores a trackable variable in lua
    fn store_tracked<T: LuaTracked>(&self, val: T::Storage);

    /// Returns a tracked variable from lua
    fn get_tracked<T: LuaTracked>(&self) -> Option<T::Output>;
}

impl <I: Invokable> TrackStore for I {
    fn store_tracked<T: LuaTracked>(&self, val: T::Storage) {
        use lua::*;
        unsafe {
            self.set_unsafe::<Ref<T::Storage>>(Scope::Registry, T::KEY.0.as_bytes(), Ref::new(&self, val));
        }
    }
    fn get_tracked<T: LuaTracked>(&self) -> Option<T::Output> {
        use lua::*;
        unsafe {
            self.get_unsafe::<Ref<T::Storage>>(Scope::Registry, T::KEY.0.as_bytes())
                .ok()
                .and_then(|v| T::try_convert(&v))
        }
    }
}

impl TrackStore for lua::Lua {
    fn store_tracked<T: LuaTracked>(&self, val: T::Storage) {
        use lua::*;
        unsafe {
            self.set_unsafe::<Ref<T::Storage>>(Scope::Registry, T::KEY.0.as_bytes(), Ref::new(&self, val));
        }
    }
    fn get_tracked<T: LuaTracked>(&self) -> Option<T::Output> {
        use lua::*;
        unsafe {
            self.get_unsafe::<Ref<T::Storage>>(Scope::Registry, T::KEY.0.as_bytes())
                .ok()
                .and_then(|v| T::try_convert(&v))
        }
    }
}

/// A null terminated string
pub struct NulledString(
    #[doc(hidden)]
    pub &'static str
);

/// Macro helper to create a constant null terminated string
#[macro_export]
macro_rules! nul_str {
    ($s:expr) => (
        $crate::script::NulledString(concat!($s, "\0"))
    )
}

/// A value that is managed/stored in lua
pub trait LuaTracked {
    /// The key in the registry where this value is stored
    const KEY: NulledString;
    /// The type of the value that will actually be stored
    type Storage: lua::LuaUsable;
    /// The value that will actually be returned
    type Output;

    /// Tries to convert the storage into the output
    fn try_convert(s: &Self::Storage) -> Option<Self::Output>;
}

/// Lua usable wrapper for the logger
pub struct LuaLogger(pub Logger);
impl lua::LuaUsable for LuaLogger {}
impl LuaTracked for Logger {
    const KEY: script::NulledString = nul_str!("logger");
    type Storage = LuaLogger;
    type Output = Logger;
    fn try_convert(s: &Self::Storage) -> Option<Self::Output> {
        Some(s.0.clone())
    }
}
impl lua::LuaUsable for AssetManager {}
impl LuaTracked for AssetManager {
    const KEY: script::NulledString = nul_str!("asset_manager");
    type Storage = AssetManager;
    type Output = AssetManager;
    fn try_convert(s: &Self::Storage) -> Option<Self::Output> {
        Some(s.clone())
    }
}

/// Scripting engine that handles communication between the game
/// and external scripts
#[derive(Clone)]
pub struct Engine {
    /// Raw access to the lua engine. Use with care
    pub lua: lua::Lua,
    log: Logger,
}

impl Deref for Engine {
    type Target = lua::Lua;
    fn deref(&self) -> &lua::Lua {
        &self.lua
    }
}

impl Invokable for Engine {
}

/// Handles files that need reloading
pub fn handle_reloads<I: Invokable>(log: &Logger, engine: &I, asset_manager: &assets::AssetManager) {
    let to_reload = {
        let watched_files: lua::Ref<RefCell<WatchedFiles>> = assume!(log, engine.get(Scope::Registry, WATCHED_FILES));
        let mut watched_files = watched_files.borrow_mut();
        watched_files.next_reload -= 1;
        if watched_files.next_reload > 0 {
            return;
        }
        watched_files.next_reload = 120;
        let mut to_reload = FNVSet::default();
        for (file, otime) in &mut watched_files.files {
            let modi = asset_manager.modified_time(assets::ModuleKey::new(&*file.0), &*file.1);
            if *otime != modi {
                *otime = modi;
                to_reload.insert(file.0.clone());
            }
        }
        to_reload
    };
    for reload in to_reload {
        assume!(log, engine.invoke_function::<_, ()>("reload_module", lua::Ref::new_string(engine, reload)));
    }
}

impl Engine {
    /// Creates a standard lua scripting engine
    pub fn new(log: &Logger, asset_manager: assets::AssetManager) -> Engine {
        let log = log.new(o!(
            "lua" => true,
        ));
        let engine = Engine {
            lua: lua::Lua::new(),
            log: log.clone(),
        };
        init_unilib(log.clone(), asset_manager.clone(), &engine);
        level::init_levellib::<crate::script_room::Types>(&engine);
        crate::mission::init_missionlib(&engine);
        crate::mission::init_commandlib(&engine);

        engine.store_tracked::<Logger>(LuaLogger(log.clone()));
        engine.store_tracked::<AssetManager>(asset_manager);

        assume!(log, engine.lua.execute_named_string::<()>("bootstrap", SCRIPT_BOOTSTRAP));
        assume!(log, engine.lua.execute_named_string::<()>("server_bootstrap", SERVER_SCRIPT_BOOTSTRAP));
        assume!(log, engine.lua.invoke_function::<(), ()>("setup", ()));
        engine
    }

    /// Loads and inits the named pack's scripts.
    ///
    /// Currently panics when it fails to load
    pub fn init_pack(&self, pack: &str) {
        if !assume!(self.log, self.invoke_function::<lua::Ref<String>, bool>("load_module", lua::Ref::new_string(self, pack))) {
            panic!("Failed to load module {}", pack);
        }
    }
}

/// Client/Server specific types that can be created/used
pub trait ScriptTypes {
    /// The type of the component used to store a reference
    /// to an entity's data
    type EntityRef: Component;
    /// The lua usable type for an entity
    type Entity: lua::LuaUsable;
    /// The type of the component used to store a reference
    /// to a room's data
    type RoomRef: Component;
    /// The lua usable type for aa room
    type Room: lua::LuaUsable;

    /// Converts an entity reference into a scripting reference
    /// type.
    fn from_entity(
        lua: &lua::Lua,
        props: &mut ecs::Write<Self::EntityRef>,
        living: &ecs::Read<Living>,
        object: &ecs::Read<Object>,
        e: Entity, controller: Option<Controller>
    ) -> lua::Ref<Self::Entity>;

    /// Converts a room reference into a scripting reference
    /// type.
    fn from_room(
        log: &Logger,
        lua: &lua::Lua,
        rooms: &LevelRooms,
        props: &mut ecs::Write<Self::RoomRef>,
        rc: &ecs::Read<RoomController>,
        entity_ref: &mut ecs::Write<Self::EntityRef>,
        living: &ecs::Read<Living>,
        object: &ecs::Read<Object>,
        room_id: RoomId
    ) -> lua::Ref<Self::Room>;
}

fn init_serialize(lua: &lua::Lua) {
    use lua::{Ref, Table};
    use delta_encode::bitio::{write_str, read_string};

    struct TableDesc {
        fields: Vec<(Ref<String>, Type)>,
    }
    impl lua::LuaUsable for TableDesc {}

    #[derive(Clone, Copy)]
    enum Type {
        Signed(u8),
        Unsigned(u8),
        Bool,
        F32,
        F64,
        String,
    }

    macro_rules! require {
        ($ex:expr) => (
            if let Some(val) = { $ex } {
                val
            } else {
                bail!("Missing required field");
            }
        )
    }

    lua.set(Scope::Global, "serialize_create_desc", lua::closure1(|lua, desc: Ref<Table>| -> errors::Result<Ref<TableDesc>> {
        let mut tdesc = TableDesc {
            fields: vec![],
        };

        let len = desc.length();
        for i in 1 ..= len {
            let f = require!(desc.get::<i32, Ref<Table>>(i));
            let name: Ref<String> = require!(f.get(1));
            let ty: Ref<String> = require!(f.get(2));
            tdesc.fields.push((
                name,
                match &*ty {
                    "string" => Type::String,
                    "bool" => Type::Bool,
                    "f32" => Type::F32,
                    "f64" => Type::F64,
                    ty if ty.starts_with('i') => Type::Signed(ty[1..].parse()?),
                    ty if ty.starts_with('u') => Type::Unsigned(ty[1..].parse()?),
                    ty => bail!("Unknown type {:?}", ty),
                }
            ))
        }

        Ok(Ref::new(lua, tdesc))
    }));

    lua.set(Scope::Global, "serialize_encode", lua::closure2(|lua, desc: Ref<TableDesc>, data: Ref<Table>| -> errors::Result<Ref<Arc<bitio::Writer<Vec<u8>>>>> {
        let mut out = bitio::Writer::new(vec![]);
        for f in &desc.fields {
            match f.1 {
                Type::Signed(size) => {
                    let val: i32 = require!(data.get(f.0.clone()));
                    out.write_signed(i64::from(val), size)?;
                },
                Type::Unsigned(size) => {
                    let val: i32 = require!(data.get(f.0.clone()));
                    out.write_unsigned(val as u64, size)?;
                },
                Type::Bool => {
                    let val: bool = require!(data.get(f.0.clone()));
                    out.write_bool(val)?;
                },
                Type::F32 => {
                    let val: f64 = require!(data.get(f.0.clone()));
                    out.write_f32(val as f32)?;
                },
                Type::F64 => {
                    let val: f64 = require!(data.get(f.0.clone()));
                    out.write_f64(val)?;
                },
                Type::String => {
                    let val: Ref<String> = require!(data.get(f.0.clone()));
                    write_str(&val, &mut out)?;
                },
            }
        }
        Ok(Ref::new(lua, Arc::new(out)))
    }));

    lua.set(Scope::Global, "serialize_decode", lua::closure2(|lua, desc: Ref<TableDesc>, data: Ref<Arc<bitio::Writer<Vec<u8>>>>| -> errors::Result<Ref<Table>> {
        let mut input = bitio::Reader::new(data.read_view());
        let out = Ref::new_table(lua);
        for f in &desc.fields {
            match f.1 {
                Type::Signed(size) => {
                    let val = input.read_signed(size)?;
                    out.insert(f.0.clone(), val as i32);
                },
                Type::Unsigned(size) => {
                    let val = input.read_unsigned(size)?;
                    out.insert(f.0.clone(), val as i32);
                },
                Type::Bool => {
                    let val = input.read_bool()?;
                    out.insert(f.0.clone(), val);
                },
                Type::F32 => {
                    let val = input.read_f32()?;
                    out.insert(f.0.clone(), f64::from(val));
                },
                Type::F64 => {
                    let val = input.read_f64()?;
                    out.insert(f.0.clone(), val);
                },
                Type::String => {
                    let val = read_string(&mut input)?;
                    out.insert(f.0.clone(), Ref::new_string(lua, val));
                },
            }
        }
        Ok(out)
    }));
}