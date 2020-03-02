//! Handles client-side scripting

use std::cell::Cell;
use std::ops::Deref;

use crate::server::assets;
use crate::server::level;
use crate::server::lua::{self, Ref};
use crate::server::mission;
use crate::server::script;
pub use crate::server::script::{Invokable, LuaTracked, NulledString, TrackStore};

use crate::audio;
use crate::instance;
use crate::prelude::*;
use crate::ui;
use cgmath::Matrix4;

const CLIENT_SCRIPT_BOOTSTRAP: &str = include_str!("client_bootstrap.lua");

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

impl script::Invokable for Engine {}

impl Engine {
    /// Creates an empty scripting engine
    pub fn empty(log: &Logger) -> Engine {
        let log = log.new(o!(
            "lua" => true,
        ));
        Engine {
            log: log.clone(),
            lua: lua::Lua::new(),
        }
    }
    /// Creates a standard lua scripting engine
    pub fn new(log: &Logger, asset_manager: assets::AssetManager) -> Engine {
        let log = log.new(o!(
            "lua" => true,
        ));
        let engine = Engine {
            log: log.clone(),
            lua: lua::Lua::new(),
        };

        script::init_unilib(log, asset_manager.clone(), &engine);
        ui::init_uilib(&engine);
        level::init_levellib::<instance::scripting::Types>(&engine);
        audio::init_audiolib(&engine);
        mission::init_commandlib(&engine);
        clientlib(&engine);

        engine.store_tracked::<Logger>(script::LuaLogger(engine.log.clone()));
        engine.store_tracked::<AssetManager>(asset_manager);

        assume!(
            engine.log,
            engine
                .lua
                .execute_named_string::<()>("bootstrap", script::SCRIPT_BOOTSTRAP)
        );
        assume!(
            engine.log,
            engine
                .lua
                .execute_named_string::<()>("client_bootstrap", CLIENT_SCRIPT_BOOTSTRAP)
        );
        assume!(
            engine.log,
            engine.lua.invoke_function::<(), ()>("setup", ())
        );
        engine
    }

    /// Loads and inits the named pack's scripts.
    ///
    /// Currently panics when it fails to load
    pub fn init_pack(&self, pack: &str) {
        if !assume!(
            self.log,
            self.lua
                .invoke_function::<Ref<String>, bool>("load_module", Ref::new_string(self, pack))
        ) {
            panic!("Failed to load module {}", pack);
        }
    }
}

fn clientlib(lua: &lua::Lua) {
    use crate::entity::{Model, StaticModel};
    use crate::instance::scripting::*;
    lua.set(
        lua::Scope::Global,
        "try_open_url",
        lua::closure1(|_, url: Ref<String>| {
            let url = ::url::Url::parse(&url)?;
            crate::open_url(&url)
        }),
    );
    lua.set(
        lua::Scope::Global,
        "new_matrix",
        lua::closure(|lua| {
            use cgmath::prelude::*;
            Ref::new(
                lua,
                LuaMatrix {
                    mat: Cell::new(Matrix4::identity()),
                },
            )
        }),
    );

    lua.set(
        lua::Scope::Global,
        "create_static_entity",
        lua::closure4(
            |lua, model: Ref<String>, x: f64, y: f64, z: f64| -> UResult<_> {
                let mut entities = lua.write_borrow::<Container>();

                let key = match ResourceKey::parse(&model) {
                    Some(val) => val,
                    None => bail!("Invalid resource key"),
                };

                entities.with(
                    |em: EntityManager<'_>,
                     mut pos: Write<Position>,
                     mut rotation: Write<Rotation>,
                     mut model: Write<Model>,
                     mut static_model: Write<StaticModel>,
                     mut entity_ref: Write<LuaEntityRef>,
                     living: Read<Living>,
                     object: Read<Object>| {
                        let entity = em.new_entity();
                        pos.add_component(
                            entity,
                            Position {
                                x: x as f32,
                                y: y as f32,
                                z: z as f32,
                            },
                        );
                        rotation.add_component(
                            entity,
                            Rotation {
                                rotation: Angle::new(0.0),
                            },
                        );
                        model.add_component(
                            entity,
                            Model {
                                name: key.into_owned(),
                            },
                        );
                        static_model.add_component(entity, StaticModel);

                        Ok(LuaEntityRef::get_or_create(
                            &mut entity_ref,
                            &living,
                            &object,
                            lua,
                            entity,
                            None,
                        ))
                    },
                )
            },
        ),
    );
}

/// A matrix constructed by a lua script
pub struct LuaMatrix {
    /// The native matrix value
    pub mat: Cell<Matrix4<f32>>,
}

impl lua::LuaUsable for LuaMatrix {
    fn fields(t: &lua::TypeBuilder) {
        use cgmath::{Rad, Vector3};
        t.field(
            "rotate_x",
            lua::closure2(|_, this: Ref<LuaMatrix>, angle: f64| {
                this.mat
                    .set(this.mat.get() * Matrix4::from_angle_x(Rad(angle as f32)));
                this
            }),
        );
        t.field(
            "rotate_y",
            lua::closure2(|_, this: Ref<LuaMatrix>, angle: f64| {
                this.mat
                    .set(this.mat.get() * Matrix4::from_angle_y(Rad(angle as f32)));
                this
            }),
        );
        t.field(
            "rotate_z",
            lua::closure2(|_, this: Ref<LuaMatrix>, angle: f64| {
                this.mat
                    .set(this.mat.get() * Matrix4::from_angle_z(Rad(angle as f32)));
                this
            }),
        );
        t.field(
            "translate",
            lua::closure4(|_, this: Ref<LuaMatrix>, x: f64, y: f64, z: f64| {
                this.mat.set(
                    this.mat.get()
                        * Matrix4::from_translation(Vector3::new(x as f32, y as f32, z as f32)),
                );
                this
            }),
        );
    }
}
