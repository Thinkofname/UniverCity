use crate::prelude::*;
use crate::ecs;
use crate::script;
use crate::server;
use crate::server::lua::{self, Ref, Table, Lua};
use crate::errors;
use crate::entity;
use crate::server::entity::free_roam;
use crate::script::LuaMatrix;
use crate::server::script::{LuaObject, ScriptTypes};
use std::marker::PhantomData;

pub struct LuaRoom {
    id: RoomId,
    area: Bound,
    properties: Ref<Table>,
}

impl LuaRoom {

    fn from_room_raw(
        _log: &Logger,
        lua: &lua::Lua,
        rooms: &LevelRooms,
        props: &mut ecs::Write<LuaRoomProperties>,
        _rc: &ecs::Read<RoomController>,
        _entity_ref: &mut ecs::Write<LuaEntityRef>,
        _living: &ecs::Read<Living>,
        _object: &ecs::Read<Object>,
        room_id: RoomId
    ) -> lua::Ref<LuaRoom> {
        let room = rooms.get_room_info(room_id);
        Ref::new(lua, LuaRoom {
            id: room.id,
            area: room.area,
            properties: LuaRoomProperties::get_or_create(props, lua, room.controller),
        })
    }
}

impl lua::LuaUsable for LuaRoom {
    fn metatable(t: &lua::TypeBuilder) {
        server::script::support_getters_setters(t);
    }

    fn fields(t: &lua::TypeBuilder) {
        t.field("get_size", lua::closure1(|_lua, this: Ref<LuaRoom>| {
            (this.area.width(), this.area.height())
        }));
        t.field("get_properties", lua::closure1(|_lua, this: Ref<LuaRoom>| {
            this.properties.clone()
        }));
        t.field("get_key", lua::closure1(|lua, this: Ref<LuaRoom>| -> UResult<_> {
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let room = rooms.try_room_info(this.id)
                .ok_or(ErrorKind::StaleScriptReference)?;
            Ok(Ref::new_string(lua, room.key.as_string()))
        }));
        t.field("get_controller", lua::closure1(|lua, this: Ref<LuaRoom>| -> UResult<_> {
            let assets = lua.get_tracked::<AssetManager>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let room = rooms.try_room_info(this.id)
                .ok_or(ErrorKind::StaleScriptReference)?;

            let ty = assets.loader_open::<room::Loader>(room.key.borrow())?;
            Ok(ty.controller.as_ref().map(|v| Ref::new_string(lua, v.as_string())))
        }));
        t.field("get_bounds", lua::closure1(|_lua, this: Ref<LuaRoom>| -> UResult<_> {
            Ok((this.area.min.x, this.area.min.y, this.area.width(), this.area.height()))
        }));
        t.field("get_room_id", lua::closure1(|_lua, this: Ref<LuaRoom>| {
            i32::from(this.id.0)
        }));
        t.field("get_sync_state", lua::closure1(|lua, this: Ref<LuaRoom>| -> UResult<_> {
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let room = rooms.try_room_info(this.id)
                .ok_or(ErrorKind::StaleScriptReference)?;
            Ok(room.tile_update_state.as_ref().and_then(|v| {
                let mut de = server::serde_cbor::de::Deserializer::from_slice(v);
                lua::with_table_serializer(lua, |se| server::serde_transcode::transcode(&mut de, se)).ok()
            }).unwrap_or_else(|| Ref::new_table(lua)))
        }));
        t.field("get_objects", lua::closure1(|lua, this: Ref<LuaRoom>| -> UResult<_> {
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let room = rooms.try_room_info(this.id)
                .ok_or(ErrorKind::StaleScriptReference)?;

            let objects = room.objects.iter()
                .enumerate()
                .filter(|&(_idx, v)| v.is_some())
                .map(|(idx, _)| LuaObject::<Types> {
                    room: this.id,
                    object: idx,
                    _types: PhantomData,
                })
                .fold((Ref::new_table(lua), 1), |(tbl, idx), t| {
                    tbl.insert(idx as i32, Ref::new(lua, t));
                    (tbl, idx + 1)
                })
                .0;

            Ok(objects)
        }));
        t.field("object_index", lua::closure2(|_lua, _this: Ref<LuaRoom>, o: Option<Ref<LuaObject<Types>>>| {
            o.map(|o| o.object as i32 + 1)
        }));
        t.field("get_position", lua::closure1(|_lua, this: Ref<LuaRoom>| -> (i32, i32) {
            (this.area.min.x, this.area.min.y)
        }));
        t.field("create_static_entity", lua::closure5(|
                lua, this: Ref<LuaRoom>,
                model: Ref<String>,
                mut x: f64, y: f64, mut z: f64,
        | -> errors::Result<_> {
            let mut entities = lua.write_borrow::<ecs::Container>();

            let key = match ResourceKey::parse(&model) {
                Some(val) => val,
                None => bail!("Invalid resource key"),
            };

            x += f64::from(this.area.min.x);
            z += f64::from(this.area.min.y);
            let loc = Location::new(x as i32, z as i32);
            if !this.area.in_bounds(loc) {
                bail!("Can't move outside room {:#?} not in {:#?}", loc, this.area);
            }

            entities.with(|
                em: EntityManager<'_>,
                mut pos: ecs::Write<Position>,
                mut rotation: ecs::Write<Rotation>,
                mut model: ecs::Write<entity::Model>,
                mut static_model: ecs::Write<entity::StaticModel>,
                mut room_owned: ecs::Write<RoomOwned>,
                mut entity_ref: ecs::Write<LuaEntityRef>,
                living: ecs::Read<Living>,
                object: ecs::Read<Object>,
                mut requires: ecs::Write<RequiresRoom>,
            | {
                let entity = em.new_entity();
                pos.add_component(entity, Position {
                    x: x as f32,
                    y: y as f32,
                    z: z as f32,
                });
                rotation.add_component(entity, Rotation {
                    rotation: Angle::new(0.0),
                });
                model.add_component(entity, entity::Model {
                    name: key.into_owned(),
                });
                static_model.add_component(entity, entity::StaticModel);
                room_owned.add_component(entity, RoomOwned::new(this.id));
                requires.add_component(entity, RequiresRoom);

                Ok(LuaEntityRef::get_or_create(
                    &mut entity_ref,
                    &living, &object,
                    lua,
                    entity,
                    Some(Controller::Room(this.id))
                ))
            })
        }));
    }
}

pub(super) struct RunningChoices {
    pub(crate) student_idle_scripts: Vec<server::choice::ScriptChoice>,
    pub(crate) student_idle: FNVMap<(PlayerId, usize), RunningChoice>,
}

impl RunningChoices {
    pub fn new(log: &Logger, assets: &AssetManager) -> RunningChoices {
        let choices = server::init_rule_vars(log, None, assets);
        let student_idle_scripts = choices.student_idle.iter()
                .map(|v| v.1.clone())
                .collect();
        RunningChoices {
            student_idle_scripts,
            student_idle: FNVMap::default(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct RunningChoice {
    pub(crate) entities: Vec<Entity>,
    pub(crate) handle: Option<Ref<IdleScriptHandle>>,
}

impl snapshot::EntityMarker for RunningChoices {
    fn mark_idle_choice(&mut self, entity: Entity, player: PlayerId, idx: usize) {
        self.student_idle.entry((player, idx))
            .or_insert_with(|| RunningChoice { entities: Vec::new(), handle: None })
            .entities.push(entity);
    }
    fn clear_idle_choice(&mut self, entity: Entity, player: PlayerId, idx: usize) {
        if let Some(c) = self.student_idle.get_mut(&(player, idx)) {
            c.entities.retain(|v| *v != entity);
        }
    }
}
pub(crate) struct IdleScriptHandle {
    pub(crate) player: PlayerId,
    pub(crate) props: Ref<Table>,
}

impl lua::LuaUsable for IdleScriptHandle {
    fn metatable(t: &lua::TypeBuilder) {
        server::script::support_getters_setters(t);
    }

    fn fields(t: &lua::TypeBuilder) {
        // Returns rooms owned by the player
        t.field("get_rooms", lua::closure1(|lua, this: Ref<IdleScriptHandle>| {
            get_rooms_for_player::<Types>(lua, this.player)
        }));
        // Returns the script property storage
        t.field("get_properties", lua::closure1(|_lua, this: Ref<IdleScriptHandle>| {
            this.props.clone()
        }));
        // Returns the room with the given id
        t.field("get_room_by_id", lua::closure2(|lua, this: Ref<IdleScriptHandle>, id: i32| -> UResult<_> {
            let log = lua.get_tracked::<Logger>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let mut entities = lua.write_borrow::<Container>();

            let id = RoomId(id as i16);

            entities.with(|
                _em: EntityManager<'_>,
                rc: ecs::Read<RoomController>,
                mut entity_ref: ecs::Write<<Types as ScriptTypes>::EntityRef>,
                mut room_ref: ecs::Write<<Types as ScriptTypes>::RoomRef>,
                living: ecs::Read<Living>,
                object: ecs::Read<Object>,
            | {
                if rooms.try_room_info(id).map_or(false, |v| v.owner == this.player) {
                    Ok(Types::from_room(
                        &log,
                        lua,
                        &rooms,
                        &mut room_ref,
                        &rc,
                        &mut entity_ref,
                        &living,
                        &object,
                        id
                    ))
                } else {
                    bail!("Invalid room id")
                }
            })
        }));
    }
}

pub(super) fn load_choice_state(
    log: &Logger,
    entities: &mut ecs::Container,
    scripting: &script::Engine,
    running_choices: &mut RunningChoices,
    player: PlayerId,
    idx: usize,
    state: Vec<u8>,
) {
    use crate::server::lua::*;

    let rc = running_choices.student_idle.entry((player, idx))
            .or_insert_with(|| RunningChoice { entities: Vec::new(), handle: None });
    let script = assume!(log, running_choices.student_idle_scripts.get(idx));
    let handle = rc.handle.get_or_insert_with(|| Ref::new(scripting, IdleScriptHandle {
        player: player,
        props: Ref::new_table(scripting),
    }));
    let mut de = serde_cbor::de::Deserializer::from_slice(&state);
    let state = assume!(log, with_table_serializer(scripting, |se| serde_transcode::transcode(&mut de, se)));
    if let Err(err) = scripting.with_borrows()
        .borrow_mut(entities)
        .invoke_function::<_, ()>("invoke_module_method", (
            Ref::new_string(scripting, script.script.module()),
            Ref::new_string(scripting, script.script.resource()),
            Ref::new_string(scripting, "apply_state"),
            handle.clone(),
            state
    )) {
        error!(log, "Failed to load script state"; "script" => ?script.script, "error" => %err);
    }
}

pub(super) fn tick_scripts(
    log: &Logger,
    asset_manager: &AssetManager,
    level: &mut Level,
    entities: &mut ecs::Container,
    scripting: &script::Engine,
    running_choices: &mut RunningChoices,
) {
    for ((player, idx), ref mut rc) in &mut running_choices.student_idle {
        let script = assume!(log, running_choices.student_idle_scripts.get(*idx));
        let lua_entities = entities.with(|
            _em: EntityManager<'_>,
            mut entity_ref: ecs::Write<LuaEntityRef>,
            living: ecs::Read<Living>,
            object: ecs::Read<Object>,
        | {
            rc.entities.iter()
                .fold(Ref::new_table(scripting), |tbl, e| {
                    tbl.insert(tbl.length() + 1, LuaEntityRef::get_or_create(&mut entity_ref, &living, &object, scripting, *e, Some(Controller::Idle(*idx))));
                    tbl
                })
        });
        let handle = rc.handle.get_or_insert_with(|| Ref::new(scripting, IdleScriptHandle {
            player: *player,
            props: Ref::new_table(scripting),
        }));
        if let Err(err) = scripting.with_borrows()
            .borrow_mut(entities)
            .invoke_function::<_, ()>("invoke_module_method", (
                Ref::new_string(scripting, script.script.module()),
                Ref::new_string(scripting, script.script.resource()),
                Ref::new_string(scripting, "update_client"),
                handle.clone(),
                lua_entities
        )) {
            error!(log, "Failed to tick script"; "script" => ?script.script, "error" => %err);
        }
    }

    for room in level.room_ids() {
        let ty = {
            let room = level.get_room_info_mut(room);
            if room.controller.is_invalid() {
                continue;
            }
            assume!(log, asset_manager.loader_open::<room::Loader>(room.key.borrow()))
        };
        if let Some(controller) = ty.controller.as_ref() {
            let (lua_room, room_entities) = entities.with(|
                _em: EntityManager<'_>,
                mut entity_ref: ecs::Write<LuaEntityRef>,
                mut room_props: ecs::Write<LuaRoomProperties>,
                living: ecs::Read<Living>,
                object: ecs::Read<Object>,
                rc: ecs::Read<RoomController>,
            | {
                let lua_room = LuaRoom::from_room_raw(
                    log,
                    scripting,
                    &*level.rooms.borrow(),
                    &mut room_props,
                    &rc,
                    &mut entity_ref,
                    &living,
                    &object,
                    room,
                );
                let room = level.get_room_info(room);
                let room_entities = rc.get_component(room.controller)
                    .into_iter()
                    .flat_map(|v| v.entities.iter())
                    .fold(Ref::new_table(scripting), |tbl, e| {
                        tbl.insert(tbl.length() + 1, LuaEntityRef::get_or_create(&mut entity_ref, &living, &object, scripting, *e, Some(Controller::Room(room.id))));
                        tbl
                    });
                (lua_room, room_entities)
            });

            if let Err(err) = scripting.with_borrows()
                .borrow_mut(entities)
                .invoke_function::<_, ()>("invoke_module_method", (
                    Ref::new_string(scripting, controller.module()),
                    Ref::new_string(scripting, controller.resource()),
                    Ref::new_string(scripting, "client"),
                    lua_room,
                    room_entities
            )) {
                error!(log, "Failed to tick room"; "room" => ?room, "type" => &ty.name, "error" => %err);
            }
        }
    }
}

/// Ticks all free roaming entities for the client
pub fn client_tick(
        log: &Logger,
        entities: &mut Container,
        scripting: &script::Engine,
) {
    free_roam::tick::<Types, _>(
        log,
        entities, scripting,
        &mut (),
        "client_handler"
    );
}

/// Script reference for an entity belonging to a room
pub struct LuaEntityRef {
    /// The controller that these properties are for
    pub controller: Option<Controller>,
    /// The lua reference
    pub(crate) lua: Ref<LuaEntity>,
}
component!(LuaEntityRef => Vec);

impl LuaEntityRef {
    /// Creates a new lua reference for an entity in the given room
    pub fn new(
        living: &ecs::Read<Living>,
        object: &ecs::Read<Object>,
        controller: Option<Controller>, entity: Entity, lua: &Lua,
        key: Option<Ref<String>>,
    ) -> LuaEntityRef {
        let mut client_entity = false;
        let key = key.unwrap_or_else(|| if let Some(living) = living.get_component(entity) {
            Ref::new_string(lua, living.key.as_string())
        } else if let Some(object) = object.get_component(entity) {
            Ref::new_string(lua, object.key.as_string())
        } else {
            client_entity = true;
            Ref::new_string(lua, "Client Entity")
        });
        LuaEntityRef {
            controller,
            lua: Ref::new(lua, LuaEntity{
                entity,
                props: Ref::new_table(lua),
                key,
                client_entity,
            }),
        }
    }

    /// Returns the lua reference for the entity.
    ///
    /// If the entity has no reference or the one it has
    /// is for another room this will create a new set.
    pub(crate) fn get_or_create(
        props: &mut ecs::Write<Self>,
        living: &ecs::Read<Living>,
        object: &ecs::Read<Object>,
        lua: &Lua, e: Entity, controller: Option<Controller>
    ) -> Ref<LuaEntity> {
        let key = if let Some(props) = props.get_component(e) {
            if props.controller == controller {
                return props.lua.clone()
            }
            Some(props.lua.key.clone())
        } else {
            None
        };
        let p = LuaEntityRef::new(living, object, controller, e, lua, key);
        let r = p.lua.clone();
        props.add_component(e, p);
        r
    }
}

pub struct LuaEntity {
    entity: ecs::Entity,
    props: Ref<Table>,
    key: Ref<String>,
    // Marks this entity as only existing on the client
    // and not the server.
    client_entity: bool,
}

pub enum Types {}

impl ScriptTypes for Types {
    type EntityRef = LuaEntityRef;
    type Entity = LuaEntity;
    type RoomRef = LuaRoomProperties;
    type Room = LuaRoom;

    fn from_entity(
        lua: &Lua,
        props: &mut ecs::Write<Self::EntityRef>,
        living: &ecs::Read<Living>,
        object: &ecs::Read<Object>,
        e: Entity, controller: Option<Controller>
    ) -> Ref<Self::Entity> {
        Self::EntityRef::get_or_create(props, living, object, lua, e, controller)
    }

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
    ) -> Ref<Self::Room> {
        LuaRoom::from_room_raw(log, lua, rooms, props, rc, entity_ref, living, object, room_id)
    }
}

impl lua::LuaUsable for LuaEntity {
    fn metatable(t: &lua::TypeBuilder) {
        server::script::support_getters_setters(t);
    }

    fn fields(t: &lua::TypeBuilder) {
        t.field("get_valid", lua::closure1(|lua, this: Ref<LuaEntity>| {
            let entities = lua.read_borrow::<Container>();
            if !entities.is_valid(this.entity) {
                return false;
            }
            if let Some(props) = entities.get_component::<LuaEntityRef>(this.entity) {
                if props.lua != this {
                    return false;
                }
                if let Some(c) = entities.get_component::<Controlled>(this.entity) {
                    props.controller == c.by
                } else {
                    false
                }
            } else {
                false
            }
        }));
        t.field("get_is_walking", lua::closure1(|lua, this: Ref<LuaEntity>| {
            let entities = lua.read_borrow::<ecs::Container>();
            entities.get_component::<server::entity::pathfind::PathInfo>(this.entity).is_some()
        }));
        t.field("get_key", lua::closure1(|_lua, this: Ref<LuaEntity>| -> Ref<String> {
            this.key.clone()
        }));
        t.field("get_position", lua::closure1(|lua, this: Ref<LuaEntity>| -> errors::Result<(f64, f64)> {
            let entities = lua.read_borrow::<ecs::Container>();
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let bound = if let Some(ro) = entities.get_component::<RoomOwned>(this.entity) {
                let room = rooms.get_room_info(ro.room_id);
                room.area
            } else {
                rooms.level_bounds
            };
            if let Some(pos) = entities.get_component::<server::entity::Position>(this.entity) {
                Ok((f64::from(pos.x) - f64::from(bound.min.x), f64::from(pos.z) - f64::from(bound.min.y)))
            } else {
                bail!("Invalid entity")
            }
        }));
        t.field("get_rotation", lua::closure1(|lua, this: Ref<LuaEntity>| -> Option<f64> {
            let entities = lua.read_borrow::<ecs::Container>();
            if let Some(rot) = entities.get_component::<server::entity::Rotation>(this.entity) {
                Some(f64::from(rot.rotation.raw()))
            } else {
                None
            }
        }));
        t.field("get_size", lua::closure1(|lua, this: Ref<LuaEntity>| -> Option<(f64, f64)> {
            let entities = lua.read_borrow::<ecs::Container>();
            if let Some(size) = entities.get_component::<server::entity::Size>(this.entity) {
                Some((f64::from(size.width), f64::from(size.depth)))
            } else {
                None
            }
        }));
        t.field("get_properties", lua::closure1(|_lua, this: Ref<LuaEntity>| {
            this.props.clone()
        }));
        t.field("get_state", lua::closure1(|lua, this: Ref<LuaEntity>| {
            let entities = lua.read_borrow::<ecs::Container>();
            if let Some(state) = entities.get_component::<server::entity::StateData>(this.entity) {
                if state.data.bit_len() == 0 {
                    None
                } else {
                    Some(Ref::new(lua, state.data.clone()))
                }
            } else {
                None
            }
        }));
        t.field("get_model", lua::closure1(|lua, this: Ref<LuaEntity>| -> errors::Result<Ref<String>> {
            let entities = lua.write_borrow::<ecs::Container>();
            if let Some(mdl) = entities.get_component::<entity::Model>(this.entity) {
                Ok(Ref::new_string(lua, mdl.name.as_string()))
            } else {
                bail!("Invalid entity")
            }
        }));
        t.field("set_model", lua::closure2(|lua, this: Ref<LuaEntity>, model: Ref<String>| -> errors::Result<()> {
            let mut entities = lua.write_borrow::<ecs::Container>();
            let key = match ResourceKey::parse(&model) {
                Some(val) => val,
                None => bail!("Invalid resource key"),
            };
            if let Some(mdl) = entities.get_component_mut::<entity::Model>(this.entity) {
                if mdl.name != key {
                    mdl.name = key.into_owned();
                }
            } else {
                bail!("Invalid entity")
            }
            Ok(())
        }));
        t.field("set_model_texture", lua::closure2(|lua, this: Ref<LuaEntity>, tex: Ref<String>| -> errors::Result<()> {
            let mut entities = lua.write_borrow::<ecs::Container>();
            let key = match ResourceKey::parse(&tex) {
                Some(val) => val,
                None => bail!("Invalid resource key"),
            };
            if let Some(mdl) = entities.get_component_mut::<entity::ModelTexture>(this.entity) {
                if mdl.name != key {
                    mdl.name = key.into_owned();
                }
                return Ok(())
            }
            entities.add_component(this.entity, entity::ModelTexture {
                name: key.into_owned(),
            });
            Ok(())
        }));
        t.field("get_animation", lua::closure1(|lua, this: Ref<LuaEntity>| -> errors::Result<_> {
            let mut entities = lua.write_borrow::<ecs::Container>();
            if let Some(ani) = entities.get_component_mut::<entity::AnimatedModel>(this.entity) {
                Ok(ani.current_animation()
                    .map(|v| Ref::new_string(lua, v)))
            } else {
                bail!("Invalid entity")
            }
        }));
        t.field("play_animation", lua::closure2(|lua, this: Ref<LuaEntity>, name: Ref<String>| -> errors::Result<()> {
            let mut entities = lua.write_borrow::<ecs::Container>();
            if let Some(ani) = entities.get_component_mut::<entity::AnimatedModel>(this.entity) {
                ani.set_animation(&*name);
            } else {
                bail!("Invalid entity")
            }
            Ok(())
        }));
        t.field("queue_animation", lua::closure2(|lua, this: Ref<LuaEntity>, name: Ref<String>| -> errors::Result<()> {
            let mut entities = lua.write_borrow::<ecs::Container>();
            if let Some(ani) = entities.get_component_mut::<entity::AnimatedModel>(this.entity) {
                ani.queue_animation(&*name);
            } else {
                bail!("Invalid entity")
            }
            Ok(())
        }));

        t.field("add_attachment", lua::closure4(|lua, this: Ref<LuaEntity>, model: Ref<String>, bone: Ref<String>, matrix: Ref<LuaMatrix>| -> UResult<_> {
            let mut entities = lua.write_borrow::<ecs::Container>();

            let key = match ResourceKey::parse(&model) {
                Some(val) => val,
                None => bail!("Invalid resource key"),
            };

            let attachment = entities.new_entity();
            entities.add_component(attachment, Position {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            });
            entities.add_component(attachment, crate::server::entity::Rotation {
                rotation: Angle::new(0.0),
            });
            entities.add_component(attachment, entity::Model {
                name: key.into_owned(),
            });
            entities.add_component(attachment, entity::StaticModel);
            entities.add_component(attachment, entity::AttachedTo {
                target: this.entity,
                bone: bone.to_string(),
                offset: matrix.mat.get(),
            });
            entities.add_component(attachment, Follow {
                target: this.entity,
                offset: (0.0, 0.0, 0.0),
            });

            if let Some(room_id) = entities.get_component::<RoomOwned>(this.entity).map(|v| v.room_id) {
                entities.add_component(attachment, RoomOwned::new(room_id));
                entities.add_component(attachment, RequiresRoom);
            }

            Ok(Ref::new(lua, LuaAttachment {
                attachment,
            }))
        }));

        // Client side only

        t.field("move", lua::closure5(|lua, this: Ref<LuaEntity>, mut x: f64, y: f64, mut z: f64, ticks: f64| -> errors::Result<_> {
            if !this.client_entity {
                bail!("Can't move non-client entities")
            }
            let mut entities = lua.write_borrow::<ecs::Container>();
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let bound = if let Some(ro) = entities.get_component::<RoomOwned>(this.entity) {
                let room = rooms.get_room_info(ro.room_id);
                room.area
            } else {
                rooms.level_bounds
            };
            x += f64::from(bound.min.x);
            z += f64::from(bound.min.y);
            let loc = Location::new(x as i32, z as i32);
            if !bound.in_bounds(loc) {
                bail!("Can't move outside room {:#?} not in {:#?}", loc, bound);
            }
            entities.add_component::<server::entity::TargetPosition>(this.entity, server::entity::TargetPosition {
                x: x as f32,
                y: y as f32,
                z: z as f32,
                ticks,
            });
            Ok(())
        }));
        t.field("rotate", lua::closure3(|lua, this: Ref<LuaEntity>, target: f64, ticks: f64| -> errors::Result<_> {
            if !this.client_entity {
                bail!("Can't rotate non-client entities")
            }
            let mut entities = lua.write_borrow::<ecs::Container>();
            entities.add_component::<server::entity::TargetRotation>(this.entity, server::entity::TargetRotation {
                rotation: Angle::new(target as f32),
                ticks,
            });
            Ok(())
        }));
        t.field("remove", lua::closure1(|lua, this: Ref<LuaEntity>| -> errors::Result<_> {
            if !this.client_entity {
                bail!("Can't remove non-client entities")
            }
            let mut entities = lua.write_borrow::<ecs::Container>();
            entities.remove_entity(this.entity);
            Ok(())
        }));
    }
}

struct LuaAttachment {
    attachment: Entity,
}

impl lua::LuaUsable for LuaAttachment {
    fn fields(t: &lua::TypeBuilder) {
        t.field("remove", lua::closure1(|lua, this: Ref<LuaAttachment>| {
            let mut entities = lua.write_borrow::<ecs::Container>();
            entities.remove_entity(this.attachment);
        }));
    }
}