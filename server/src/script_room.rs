use super::*;

use std::marker::PhantomData;
use lua::Lua;

/// Proxy to a room in the level that can
/// used via a lua script.
///
/// Only valid for the duration of the event,
/// after that the reference may become invalid.
pub struct LuaRoom {
    id: room::Id,
    width: i32,
    height: i32,
    owned_entities: Ref<Table>,
    visitors: Ref<Table>,
    properties: Ref<Table>,
    closing: bool,
}

impl LuaRoom {
    fn from_room_raw(
        _log: &Logger,
        lua: &lua::Lua,
        rooms: &LevelRooms,
        props: &mut ecs::Write<LuaRoomProperties>,
        rc: &ecs::Read<RoomController>,
        entity_ref: &mut ecs::Write<LuaEntityRef>,
        living: &ecs::Read<Living>,
        object: &ecs::Read<Object>,
        room_id: RoomId
    ) -> lua::Ref<LuaRoom> {
        let room = rooms.get_room_info(room_id);
        let rc = rc.get_component(room.controller);
        let owned_entities = rc.iter()
                .flat_map(|rc| rc.entities.iter())
                .fold(Ref::new_table(lua), |tbl, e| {
                    tbl.insert(tbl.length() + 1, LuaEntityRef::get_or_create(entity_ref, living, object, lua, *e, Some(Controller::Room(room.id))));
                    tbl
                });
        let visitors = rc.iter()
                .flat_map(|rc| rc.visitors.iter())
                .fold(Ref::new_table(lua), |tbl, e| {
                    tbl.insert(tbl.length() + 1, LuaEntityRef::get_or_create(entity_ref, living, object, lua, *e, Some(Controller::Room(room.id))));
                    tbl
                });
        let props = LuaRoomProperties::get_or_create(props, lua, room.controller);
        Ref::new(lua, LuaRoom {
            id: room.id,
            width: room.area.width(),
            height: room.area.height(),
            owned_entities,
            visitors,
            closing: !room.state.is_done(),
            properties: props,
        })
    }
    pub fn from_room(
        log: &Logger,
        rooms: &LevelRooms, entities: &mut Container,
        room: RoomId,
        scripting: &lua::Lua,
    ) -> Ref<LuaRoom>
    {
        entities.with(|
            _em: EntityManager<'_>,
            mut props: ecs::Write<LuaRoomProperties>,
            rc: ecs::Read<RoomController>,
            mut entity_ref: ecs::Write<LuaEntityRef>,
            living: ecs::Read<Living>,
            object: ecs::Read<Object>,
        | {
            Self::from_room_raw(log, scripting, rooms, &mut props, &rc, &mut entity_ref, &living, &object, room)
        })
    }
}

impl lua::LuaUsable for LuaRoom {
    fn metatable(t: &lua::TypeBuilder) {
        script::support_getters_setters(t);
    }

    fn fields(t: &lua::TypeBuilder) {
        // Returns the room's id
        t.field("get_room_id", lua::closure1(|_lua, this: Ref<LuaRoom>| {
            i32::from(this.id.0)
        }));
        // Returns whether the room is closing
        t.field("get_closing", lua::closure1(|_lua, this: Ref<LuaRoom>| {
            this.closing
        }));
        // Returns the size of the room in tiles.
        t.field("get_size", lua::closure1(|_lua, this: Ref<LuaRoom>| {
            (this.width, this.height)
        }));
        t.field("get_bounds", lua::closure1(|lua, this: Ref<LuaRoom>| -> UResult<_> {
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let room = rooms.try_room_info(this.id)
                .ok_or(ErrorKind::StaleScriptReference)?;

            Ok((room.area.min.x, room.area.min.y, room.area.width(), room.area.height()))
        }));
        // Returns the position of the room within the level
        t.field("get_position", lua::closure1(|lua, this: Ref<LuaRoom>| -> UResult<_> {
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let room = rooms.try_room_info(this.id)
                .ok_or(ErrorKind::StaleScriptReference)?;
            Ok((
                room.area.min.x,
                room.area.min.y,
            ))
        }));
        t.field("get_valid", lua::closure1(|lua, this: Ref<LuaRoom>| -> UResult<_> {
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let room = rooms.try_room_info(this.id);
            Ok(room.map_or(false, |v| v.state.is_done()))
        }));
        t.field("get_used_for_teaching", lua::closure1(|lua, this: Ref<LuaRoom>| -> UResult<_> {
            let assets = lua.get_tracked::<AssetManager>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let room = rooms.try_room_info(this.id)
                .ok_or(ErrorKind::StaleScriptReference)?;

            let ty = assets.loader_open::<room::Loader>(room.key.borrow())?;

            Ok(ty.used_for_teaching)
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
        t.field("get_idle_task", lua::closure2(|lua, this: Ref<LuaRoom>, name: Ref<String>| -> UResult<Option<_>> {
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let room = rooms.try_room_info(this.id)
                .ok_or(ErrorKind::StaleScriptReference)?;
            Ok(unsafe {lua.get_unsafe::<lua::Ref<Table>>(lua::Scope::Global, b"idle_storage\0") }
                .ok()
                .and_then(|v| v.get::<i32, Ref<Table>>(i32::from(room.owner.0)))
                .and_then(|v| v.get::<Ref<String>, Ref<Table>>(name)))
        }));
        // Returns the room's properties.
        //
        // Can be used to store state about the room
        t.field("get_properties", lua::closure1(|_lua, this: Ref<LuaRoom>| {
            this.properties.clone()
        }));
        // Returns the state object sent to initialize the room
        //
        // This is expensive to call
        t.field("get_sync_state", lua::closure1(|lua, this: Ref<LuaRoom>| -> UResult<_> {
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let room = rooms.try_room_info(this.id)
                .ok_or(ErrorKind::StaleScriptReference)?;
            Ok(room.tile_update_state.as_ref().and_then(|v| {
                let mut de = serde_cbor::de::Deserializer::from_slice(v);
                lua::with_table_serializer(lua, |se| serde_transcode::transcode(&mut de, se)).ok()
            }).unwrap_or_else(|| Ref::new_table(lua)))
        }));
        // Sets the state object sent to initialize the room
        //
        // This is expensive to call
        t.field("set_sync_state", lua::closure2(|lua, this: Ref<LuaRoom>, state: Ref<Table>| -> UResult<_> {
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let mut rooms = rooms.borrow_mut();
            let room = rooms.try_room_info_mut(this.id)
                .ok_or(ErrorKind::StaleScriptReference)?;
            let mut se = serde_cbor::ser::Serializer::new(vec![]);
            let _ = lua::with_table_deserializer(&state, |de| {
                serde_transcode::transcode(de, &mut se)
            });
            let data = se.into_inner();
            room.tile_update_state = Some(data);
            Ok(())
        }));
        // Submits a command that will be executed by all clients and the server
        t.field("execute_command", lua::closure3(|lua, this: Ref<LuaRoom>, method: Ref<String>, data: Ref<Arc<bitio::Writer<Vec<u8>>>>| -> UResult<_> {
            let commands = lua.get_tracked::<ExtraCommands>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let mut commands = commands.borrow_mut();
            commands.push(command::ExecRoom::new(this.id, method.to_string(), common::ScriptData(Arc::clone(&data))).into());
            Ok(())
        }));
        // Attempts to find an entity with the required tag.
        // Returns nil if one can't be found otherwise it
        // returns a proxy to the entity.
        t.field("request_entity", lua::closure2(|lua, this: Ref<LuaRoom>, ty: Ref<String>| -> UResult<_> {
            let mut entities = lua.write_borrow::<Container>();
            let log = lua.get_tracked::<Logger>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let assets = lua.get_tracked::<AssetManager>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let room = rooms.try_room_info(this.id)
                .ok_or(ErrorKind::StaleScriptReference)?;

            let pos = (
                room.area.min.x as f32 + room.area.width() as f32 / 2.0,
                room.area.min.y as f32 + room.area.height() as f32 / 2.0,
            );

            let rty = assume!(log, assets.loader_open::<room::Loader>(room.key.borrow()));
            let controller = assume!(log, rty.controller.as_ref());
            let ty = assets::LazyResourceKey::parse(&ty)
                .or_module(controller.module_key());

            Ok(if let Some(entity) = request_entity_now(&log, room.owner, ty.borrow(), Some(pos), Controller::Room(room.id), &mut *entities) {
                {
                    let rc = entities.get_component_mut::<RoomController>(room.controller)
                        .ok_or(ErrorKind::StaleScriptReference)?;
                    rc.entities.push(entity);
                }
                entities.add_component(entity, RoomOwned::new(room.id));
                entities.add_component(entity, Controlled::new_by(Controller::Room(room.id)));
                entities.with(|
                    _em: EntityManager<'_>,
                    mut entity_ref: ecs::Write<LuaEntityRef>,
                    living: ecs::Read<Living>,
                    object: ecs::Read<Object>,
                | {
                    Some(LuaEntityRef::get_or_create(&mut entity_ref, &living, &object, lua, entity, Some(Controller::Room(room.id))))
                })
            } else {
                {
                    let rc = entities.get_component_mut::<RoomController>(room.controller)
                        .ok_or(ErrorKind::StaleScriptReference)?;
                    *rc.script_requests.entry(ty.into_owned()).or_insert(0) += 1;
                }
                None
            })
        }));
        // Returns a list of entities owned by the room.
        t.field("get_owned", lua::closure1(|_lua, this: Ref<LuaRoom>| {
            this.owned_entities.clone()
        }));
        // Returns a list of entities that are visiting the room
        // (e.g. students).
        t.field("get_visitors", lua::closure1(|_lua, this: Ref<LuaRoom>| {
            this.visitors.clone()
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
        // Updates the capacity that the room supports
        t.field("set_capacity", lua::closure2(|lua, this: Ref<LuaRoom>, val: i32| -> UResult<_> {
            let mut entities = lua.write_borrow::<Container>();
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let room = rooms.try_room_info(this.id)
                .ok_or(ErrorKind::StaleScriptReference)?;
            let rc = entities.get_component_mut::<RoomController>(room.controller)
                .ok_or(ErrorKind::StaleScriptReference)?;
            rc.capacity = val as usize;
            Ok(())
        }));
        // Returns the capacity of the room
        t.field("get_capacity", lua::closure1(|lua, this: Ref<LuaRoom>| -> UResult<i32> {
            let entities = lua.read_borrow::<Container>();
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let room = rooms.try_room_info(this.id)
                .ok_or(ErrorKind::StaleScriptReference)?;
            let rc = entities.get_component::<RoomController>(room.controller)
                .ok_or(ErrorKind::StaleScriptReference)?;
            Ok(rc.capacity as i32)
        }));
        // Returns whether the room has entities waiting to enter
        // the room
        t.field("get_has_waiting", lua::closure1(|lua, this: Ref<LuaRoom>| -> UResult<bool> {
            let entities = lua.read_borrow::<Container>();
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let room = rooms.try_room_info(this.id)
                .ok_or(ErrorKind::StaleScriptReference)?;
            let rc = entities.get_component::<RoomController>(room.controller)
                .ok_or(ErrorKind::StaleScriptReference)?;
            Ok(!rc.waiting_list.is_empty())
        }));
        // Returns whether the room owns the tile or not
        t.field("owns_tile", lua::closure3(|lua, this: Ref<LuaRoom>, x: i32, y: i32| -> UResult<bool> {
            let tiles = lua.get_tracked::<LevelTiles>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let tiles = tiles.borrow();
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let room = rooms.try_room_info(this.id)
                .ok_or(ErrorKind::StaleScriptReference)?;
            let owner = tiles.get_room_owner(room.area.min + (x, y));
            Ok(owner.map_or(false, |v| v == this.id))
        }));
        t.field("notify_player", lua::closure3(move |lua, this: Ref<LuaRoom>, de_func: Ref<String>, data: Ref<Arc<bitio::Writer<Vec<u8>>>>| -> UResult<()> {
            let assets = lua.get_tracked::<AssetManager>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let log = lua.get_tracked::<Logger>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let mut players = lua.write_borrow::<crate::PlayerInfoMap>();
            let room = rooms.try_room_info(this.id)
                .ok_or(ErrorKind::StaleScriptReference)?;
            let player = assume!(log, players.get_mut(&room.owner));
            let ty = assume!(log, assets.loader_open::<room::Loader>(room.key.borrow()));
            let controller = assume!(log, ty.controller.as_ref());

            let (script, method) = if let Some(pos) = de_func.char_indices().find(|v| v.1 == '#') {
                de_func.split_at(pos.0)
            } else {
                bail!("invalid method description")
            };
            let script = assets::LazyResourceKey::parse(&script)
                .or_module(controller.module_key())
                .into_owned();
            let func = method[1..].into();

            player.notifications.push(crate::notify::Notification::Script {
                script,
                func,
                data: common::ScriptData(Arc::clone(&data)),
            });
            Ok(())
        }));
    }
}

/// Script reference for an object belonging to a room
pub struct LuaObject<T> {
    /// The id of the room
    pub room: RoomId,
    /// The id of the object
    pub object: usize,
    /// The type of entities used
    pub _types: PhantomData<T>,
}

impl <T> lua::LuaUsable for LuaObject<T>
    where T: script::ScriptTypes + 'static
{
    fn metatable(t: &lua::TypeBuilder) {
        script::support_getters_setters(t);
    }

    fn fields(t: &lua::TypeBuilder) {
        t.field("get_id", lua::closure1(|_lua, this: Ref<LuaObject<T>>| {
            this.object as i32 + 1
        }));
        t.field("get_valid", lua::closure1(|lua, this: Ref<LuaObject<T>>| -> UResult<_> {
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            Ok(rooms.try_room_info(this.room)
                .and_then(|room| room.objects.get(this.object)
                .and_then(|v| v.as_ref()))
                .map_or(false, |_| true))
        }));
        t.field("get_key", lua::closure1(|lua, this: Ref<LuaObject<T>>| -> UResult<_> {
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let room = rooms.try_room_info(this.room)
                .ok_or(ErrorKind::StaleScriptReference)?;
            let object = room.objects.get(this.object)
                .and_then(|v| v.as_ref())
                .map(|v| &v.0)
                .ok_or(ErrorKind::StaleScriptReference)?;
            let mut buf = [0; 1024];
            object.key.store_string_buf(&mut buf);
            Ok(Ref::new_string_buf(lua, &buf))
        }));
        t.field("get_room_id", lua::closure1(|_lua, this: Ref<LuaObject<T>>| -> UResult<_> {
            Ok(i32::from(this.room.0))
        }));
        t.field("get_properties", lua::closure1(|lua, this: Ref<LuaObject<T>>| -> UResult<_> {
            let mut entities = lua.write_borrow::<Container>();
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let room = rooms.try_room_info(this.room)
                .ok_or(ErrorKind::StaleScriptReference)?;
            let props = entities.get_component_mut::<LuaRoomProperties>(room.controller)
                .ok_or(ErrorKind::StaleScriptReference)?;
            while props.object_properties.len() <= this.object {
                props.object_properties.push(None);
            }
            Ok(props.object_properties.get_mut(this.object)
                .map(|v| v.get_or_insert_with(|| Ref::new_table(lua)).clone()))
        }));
        t.field("get_entities", lua::closure1(|lua, this: Ref<LuaObject<T>>| -> UResult<_> {
            let mut entities = lua.write_borrow::<Container>();
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let room = rooms.try_room_info(this.room)
                .ok_or(ErrorKind::StaleScriptReference)?;
            let obj = room.objects.get(this.object)
                .and_then(|v| v.as_ref())
                .map(|v| &v.1)
                .ok_or(ErrorKind::StaleScriptReference)?;
            entities.with(|
                _em: EntityManager<'_>,
                mut entity_ref: ecs::Write<T::EntityRef>,
                living: ecs::Read<Living>,
                object: ecs::Read<Object>,
            | {
                Ok(obj.get_entities()
                    .into_iter()
                    .fold(Ref::new_table(lua), |tbl, e| {
                        tbl.insert(tbl.length() + 1, T::from_entity(lua, &mut entity_ref, &living, &object, e, Some(Controller::Room(room.id))));
                        tbl
                    }))
            })
        }));
        t.field("get_actions", lua::closure1(|lua, this: Ref<LuaObject<T>>| -> UResult<_> {
            use crate::level::object::WallPlacementFlag;
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let room = rooms.try_room_info(this.room)
                .ok_or(ErrorKind::StaleScriptReference)?;
            let object = room.objects.get(this.object)
                .and_then(|v| v.as_ref())
                .map(|v| &v.0)
                .ok_or(ErrorKind::StaleScriptReference)?;
            Ok(object.actions.0.iter()
                .filter_map(|v| {
                    let out = Ref::new_table(lua);
                    match *v {
                        ObjectPlacementAction::WallFlag{location, direction, ref flag} => {
                            out.insert(Ref::new_string(lua, "type"), Ref::new_string(lua, "wall"));
                            out.insert(Ref::new_string(lua, "location"), {
                                let loc = Ref::new_table(lua);
                                loc.insert(Ref::new_string(lua, "x"), location.x as i32);
                                loc.insert(Ref::new_string(lua, "y"), location.y as i32);
                                loc
                            });
                            out.insert(Ref::new_string(lua, "direction"), Ref::new_string(lua, direction.as_str()));
                            out.insert(Ref::new_string(lua, "flag"), Ref::new_string(lua, match *flag {
                                WallPlacementFlag::None => "none",
                                WallPlacementFlag::Window{..} => "window",
                                WallPlacementFlag::Door => "door",
                            }));
                        },
                        ObjectPlacementAction::Tile{location, ref key, ..} => {
                            out.insert(Ref::new_string(lua, "type"), Ref::new_string(lua, "tile"));
                            out.insert(Ref::new_string(lua, "location"), {
                                let loc = Ref::new_table(lua);
                                loc.insert(Ref::new_string(lua, "x"), location.x as i32);
                                loc.insert(Ref::new_string(lua, "y"), location.y as i32);
                                loc
                            });
                            out.insert(Ref::new_string(lua, "tile"), Ref::new_string(lua, key.as_string()));
                        },
                        _ => return None,
                    }
                    Some(out)
                })
                .fold((Ref::new_table(lua), 1), |(tbl, idx), t| {
                    tbl.insert(idx as i32, t);
                    (tbl, idx + 1)
                })
                .0)
        }))
    }
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
    fn new(
        living: &ecs::Read<Living>,
        object: &ecs::Read<Object>,
        controller: Option<Controller>, entity: Entity, lua: &Lua,
        key: Option<Ref<String>>,
    ) -> LuaEntityRef {
        LuaEntityRef {
            controller,
            lua: Ref::new(lua, LuaEntity {
                entity,
                props: Ref::new_table(lua),
                key: key.unwrap_or_else(|| if let Some(living) = living.get_component(entity) {
                    Ref::new_string(lua, living.key.as_string())
                } else if let Some(object) = object.get_component(entity) {
                    Ref::new_string(lua, object.key.as_string())
                } else {
                    panic!("Invalid entity")
                }),
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

/// Proxy to a entity in the level that can
/// used via a lua script.
///
/// Only valid for the duration of the event,
/// after that the reference may become invalid.
pub struct LuaEntity {
    pub(crate) entity: Entity,
    pub(crate) props: Ref<Table>,
    pub(crate) key: Ref<String>,
}

impl lua::LuaUsable for LuaEntity {
    fn metatable(t: &lua::TypeBuilder) {
        script::support_getters_setters(t);
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
                props.controller == entities.get_component::<Controlled>(this.entity).and_then(|v| v.by)
            } else {
                false
            }
        }));
        // Returns whether this entity is currently walking
        t.field("get_is_walking", lua::closure1(|lua, this: Ref<LuaEntity>| {
            let entities = lua.read_borrow::<Container>();
            entities.get_component::<pathfind::PathInfo>(this.entity).is_some()
            || entities.get_component::<pathfind::Target>(this.entity).map_or(false, |v| v.request.is_some())
        }));
        // Returns the network id for this entity
        //
        // Should be constant between reloads making it useful for saving
        t.field("get_id", lua::closure1(|lua, this: Ref<LuaEntity>| {
            let entities = lua.read_borrow::<Container>();
            entities.get_component::<NetworkId>(this.entity).map(|v| v.0 as i32)
        }));
        // Returns whether this entity is in the room
        t.field("get_in_room", lua::closure1(|lua, this: Ref<LuaEntity>| -> UResult<_> {
            let entities = lua.read_borrow::<Container>();
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let bound = if let Some(ro) = entities.get_component::<RoomOwned>(this.entity) {
                let room = rooms.get_room_info(ro.room_id);
                room.area
            } else {
                rooms.level_bounds
            };
            let pos = if let Some(pos) = entities.get_component::<Position>(this.entity) {
                pos
            } else {
                return Ok(true);
            };
            let loc = util::Location::new(pos.x as i32, pos.z as i32);
            Ok(bound.in_bounds(loc))
        }));
        // Causes the entity to walk to the location
        // within the room
        t.field("walk_to", lua::closure3(|lua, this: Ref<LuaEntity>, mut x: f64, mut y: f64| -> UResult<()> {
            let mut entities = lua.write_borrow::<Container>();
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let bound = if let Some(Controller::Room(room_id)) = entities.get_component::<Controlled>(this.entity).and_then(|v| v.by) {
                let room = rooms.get_room_info(room_id);
                room.area
            } else {
                rooms.level_bounds
            };
            x += f64::from(bound.min.x);
            y += f64::from(bound.min.y);
            let loc = util::Location::new(x as i32, y as i32);
            if !bound.in_bounds(loc) {
                bail!("Can't path outside room {:#?} not in {:#?}. {:?}", loc, bound, entities.get_component::<Controlled>(this.entity));
            }
            entities.add_component(this.entity, pathfind::Target::try_new(x as f32, y as f32));
            Ok(())
        }));
        // Causes the entity to walk to the location
        // within the room and end up facing in
        // passed direction at the end
        t.field("walk_to_face", lua::closure4(|lua, this: Ref<LuaEntity>, mut x: f64, mut y: f64, facing: f64| -> UResult<()> {
            let mut entities = lua.write_borrow::<Container>();
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let bound = if let Some(Controller::Room(room_id)) = entities.get_component::<Controlled>(this.entity).and_then(|v| v.by) {
                let room = rooms.get_room_info(room_id);
                room.area
            } else {
                rooms.level_bounds
            };
            x += f64::from(bound.min.x);
            y += f64::from(bound.min.y);
            let loc = util::Location::new(x as i32, y as i32);
            if !bound.in_bounds(loc) {
                bail!("Can't path outside room {:#?} not in {:#?}. {:?}", loc, bound, entities.get_component::<Controlled>(this.entity));
            }
            entities.add_component(this.entity, pathfind::Target::try_new(x as f32, y as f32));
            entities.add_component(this.entity, pathfind::TargetFacing {
                rotation: Angle::new(facing as f32),
            });
            Ok(())
        }));
        // Returns the type key of this entity
        t.field("get_key", lua::closure1(|_lua, this: Ref<LuaEntity>| -> Ref<String> {
            this.key.clone()
        }));
        // Returns the position of this entity
        t.field("get_position", lua::closure1(|lua, this: Ref<LuaEntity>| -> UResult<(f64, f64)> {
            let entities = lua.read_borrow::<Container>();
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let bound = if let Some(ro) = entities.get_component::<RoomOwned>(this.entity) {
                let room = rooms.get_room_info(ro.room_id);
                room.area
            } else {
                rooms.level_bounds
            };
            if let Some(pos) = entities.get_component::<Position>(this.entity) {
                Ok((f64::from(pos.x) - f64::from(bound.min.x), f64::from(pos.z) - f64::from(bound.min.y)))
            } else {
                bail!("Invalid entity")
            }
        }));
        // Returns the rotation of this entity
        t.field("get_rotation", lua::closure1(|lua, this: Ref<LuaEntity>| -> Option<f64> {
            let entities = lua.read_borrow::<Container>();
            if let Some(rot) = entities.get_component::<Rotation>(this.entity) {
                Some(f64::from(rot.rotation.raw()))
            } else {
                None
            }
        }));
        // Returns the size of this entity
        t.field("get_size", lua::closure1(|lua, this: Ref<LuaEntity>| -> Option<(f64, f64)> {
            let entities = lua.read_borrow::<Container>();
            if let Some(size) = entities.get_component::<Size>(this.entity) {
                Some((f64::from(size.width), f64::from(size.depth)))
            } else {
                None
            }
        }));
        // Allows storage of lua values on this entity.
        //
        // These values will last whilst this entity is owned by this
        // room.
        t.field("get_properties", lua::closure1(|_lua, this: Ref<LuaEntity>| {
            this.props.clone()
        }));
        // Returns state data that is stored for this entity.
        //
        // This data is sync'd from the server to the client.
        t.field("get_state", lua::closure1(|lua, this: Ref<LuaEntity>| {
            let entities = lua.read_borrow::<Container>();
            let c = entities.get_component::<Controlled>(this.entity).and_then(|v| v.by);
            if let Some(state) = entities.get_component::<StateData>(this.entity) {
                if state.data.bit_len() == 0 || (state.controller != c && state.controller.is_some()) {
                    None
                } else {
                    Some(Ref::new(lua, state.data.clone()))
                }
            } else {
                None
            }
        }));
        // Updates the state data that is stored for this entity.
        //
        // This data is sync'd from the server to the client.
        t.field("set_state", lua::closure2(|lua, this: Ref<LuaEntity>, data: Ref<Arc<bitio::Writer<Vec<u8>>>>| -> UResult<()> {
            let mut entities = lua.write_borrow::<Container>();
            let c = entities.get_component::<Controlled>(this.entity).and_then(|v| v.by);
            if let Some(state) = entities.get_component_mut::<StateData>(this.entity) {
                state.controller = c;
                if !Arc::ptr_eq(&state.data, &*data) || state.data != *data {
                    state.data = (*data).clone();
                }
                return Ok(())
            }
            entities.add_component(this.entity, StateData {
                controller: c,
                data: (*data).clone(),
            });
            Ok(())
        }));
        // Generates a time table for this entity
        t.field("generate_time_table", lua::closure1(|lua, this: Ref<LuaEntity>| -> UResult<(i32, Option<Ref<String>>)> {
            let mut entities = lua.write_borrow::<Container>();
            let mut players = lua.write_borrow::<PlayerInfoMap>();
            let log = lua.get_tracked::<Logger>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let assets = lua.get_tracked::<AssetManager>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            Ok(match generate_time_table(&log, &assets, &mut players, &rooms, &mut entities, this.entity) {
                Ok(v) => (v.0 as i32, None),
                Err(err) => (0, Some(Ref::new_string(lua, format!("{}", err)))),
            })
        }));
        // Returns whether the game wishes for the current room to
        // release the entity
        t.field("get_should_release", lua::closure1(|lua, this: Ref<LuaEntity>| -> bool {
            let entities = lua.read_borrow::<Container>();
            entities.get_component::<Controlled>(this.entity)
                .map_or(true, |c| c.should_release)
        }));
        t.field("set_should_release", lua::closure2(|lua, this: Ref<LuaEntity>, should_release: bool| {
            let mut entities = lua.write_borrow::<Container>();
            entities.get_component_mut::<Controlled>(this.entity)
                .map(|c| c.should_release = should_release);
        }));
        // Returns whether the game wishes for the current room to
        // release the entity when the room is inactive
        t.field("get_should_release_inactive", lua::closure1(|lua, this: Ref<LuaEntity>| -> bool {
            let entities = lua.read_borrow::<Container>();
            entities.get_component::<RoomOwned>(this.entity)
                .map_or(true, |owner| owner.should_release_inactive)
        }));
        // Releases control of the entity
        t.field("release", lua::closure1(|lua, this: Ref<LuaEntity>| -> UResult<_>{
            let mut entities = lua.write_borrow::<Container>();
            if let Some(c) = entities.get_component_mut::<Controlled>(this.entity) {
                c.by = None;
                c.should_release = false;
            }
            if let Some(props) = entities.get_component_mut::<LuaEntityRef>(this.entity) {
                props.controller = None;
            }
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            if let Some(owner) = entities.remove_component::<RoomOwned>(this.entity) {
                let room = rooms.try_room_info(owner.room_id)
                    .ok_or(ErrorKind::StaleScriptReference)?;
                let controller = entities.get_component_mut::<RoomController>(room.controller)
                    .ok_or(ErrorKind::StaleScriptReference)?;
                controller.entities.retain(|e| *e != this.entity);
                controller.visitors.retain(|e| *e != this.entity);
            } else if let Some(idle) = entities.get_component_mut::<Idle>(this.entity) {
                idle.released = true;
            }
            Ok(())
        }));
        // Releases control of the entity and forces them
        // to leave the room
        t.field("force_release", lua::closure1(|lua, this: Ref<LuaEntity>| -> UResult<_>{
            let mut entities = lua.write_borrow::<Container>();
            if let Some(c) = entities.get_component_mut::<Controlled>(this.entity) {
                c.by = None;
                c.should_release = false;
            }
            if let Some(props) = entities.get_component_mut::<LuaEntityRef>(this.entity) {
                props.controller = None;
            }
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            if let Some(owner) = entities.remove_component::<RoomOwned>(this.entity) {
                let room = rooms.try_room_info(owner.room_id)
                    .ok_or(ErrorKind::StaleScriptReference)?;
                {
                    let controller = entities.get_component_mut::<RoomController>(room.controller)
                        .ok_or(ErrorKind::StaleScriptReference)?;
                    controller.entities.retain(|e| *e != this.entity);
                    controller.visitors.retain(|e| *e != this.entity);
                }
                entities.add_component(this.entity, ForceLeave {
                    room_id: owner.room_id,
                });
            } else if let Some(idle) = entities.get_component_mut::<Idle>(this.entity) {
                idle.released = true;
            }
            Ok(())
        }));
        // Releases control of the entity and forces them
        // to leave the game
        t.field("quit", lua::closure1(|lua, this: Ref<LuaEntity>| -> UResult<_>{
            let mut entities = lua.write_borrow::<Container>();
            if let Some(c) = entities.get_component_mut::<Controlled>(this.entity) {
                c.by = None;
                c.should_release = false;
            }
            if let Some(props) = entities.get_component_mut::<LuaEntityRef>(this.entity) {
                props.controller = None;
            }
            let rooms = lua.get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            if let Some(owner) = entities.remove_component::<RoomOwned>(this.entity) {
                let room = rooms.try_room_info(owner.room_id)
                    .ok_or(ErrorKind::StaleScriptReference)?;
                let controller = entities.get_component_mut::<RoomController>(room.controller)
                    .ok_or(ErrorKind::StaleScriptReference)?;
                controller.entities.retain(|e| *e != this.entity);
                controller.visitors.retain(|e| *e != this.entity);
            } else if let Some(idle) = entities.get_component_mut::<Idle>(this.entity) {
                idle.released = true;
            }
            entities.add_component(this.entity, Quitting);
            Ok(())
        }));
        // Returns whether this entity is actively being used by
        // the room.
        t.field("get_active", lua::closure1(|lua, this: Ref<LuaEntity>| -> UResult<bool> {
            let entities = lua.read_borrow::<Container>();
            let owner = entities.get_component::<RoomOwned>(this.entity)
                .ok_or(ErrorKind::StaleScriptReference)?;
            Ok(owner.active)
        }));
        // Sets whether this entity is actively being used by
        // the room.
        //
        // Inactive entities may be released to be used else where
        // if needed.
        t.field("set_active", lua::closure2(|lua, this: Ref<LuaEntity>, val: bool| -> UResult<_>{
            let mut entities = lua.write_borrow::<Container>();
            let owner = entities.get_component_mut::<RoomOwned>(this.entity)
                .ok_or(ErrorKind::StaleScriptReference)?;
            owner.active = val;
            Ok(())
        }));
        // Charges the entity for a service
        t.field("charge", lua::closure3(|lua, this: Ref<LuaEntity>, service: Ref<String>, money: i32| -> UResult<_>{
            let mut entities = lua.write_borrow::<Container>();
            let log = lua.get_tracked::<Logger>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let _ = service; // TODO: Log somewhere
            let mut players = lua.write_borrow::<PlayerInfoMap>();
            if let Some(owner) = entities.get_component::<Owned>(this.entity).map(|v| v.player_id) {
                let player = assume!(log, players.get_mut(&owner));
                let money = UniDollar(i64::from(money));
                player.change_money(money);
                entities.with(|
                    _em: EntityManager<'_>,
                    mut emotes: Write<IconEmote>
                | {
                    IconEmote::add(&mut emotes, this.entity, Emote::Paid);
                });
            }
            Ok(())
        }));
        // Modifies an entity's stats
        t.field("change_stat", lua::closure3(|lua, this: Ref<LuaEntity>, name: Ref<String>, modi: f64| -> UResult<_> {
            let mut entities = lua.write_borrow::<Container>();
            let vars = get_vars(&mut *entities, this.entity);
            if let Some(vars) = vars {
                let val = vars.get_float(&name)
                    .ok_or_else(|| ErrorKind::Msg(format!("Unknown stat {}", name)))?;
                vars.set_float(&name, (val + modi as f32).max(0.0).min(1.0));
            } else {
                bail!("Entity doesn't have stats")
            }
            Ok(())
        }));

        // Returns the current value of a stat
        t.field("get_var_float", lua::closure2(|lua, this: Ref<LuaEntity>, name: Ref<String>| -> UResult<_> {
            let mut entities = lua.write_borrow::<Container>();

            let vars = get_vars(&mut *entities, this.entity);
            if let Some(vars) = vars {
                vars.get_float(&name)
                    .ok_or_else(|| ErrorKind::Msg(format!("Unknown var {}", name)))
                    .map(f64::from)
                    .map_err(Into::into)
            } else {
                bail!("Entity doesn't have vars")
            }
        }));
        // Sets the current value of a stat
        t.field("set_var_float", lua::closure3(|lua, this: Ref<LuaEntity>, name: Ref<String>, val: f64| -> UResult<_> {
            let mut entities = lua.write_borrow::<Container>();
            let vars = get_vars(&mut *entities, this.entity);
            if let Some(vars) = vars {
                vars.set_float(&name, val as f32);
                Ok(())
            } else {
                bail!("Entity doesn't have vars")
            }
        }));
        // Returns the current value of a stat
        t.field("get_var_int", lua::closure2(|lua, this: Ref<LuaEntity>, name: Ref<String>| -> UResult<_> {
            let mut entities = lua.write_borrow::<Container>();

            let vars = get_vars(&mut *entities, this.entity);
            if let Some(vars) = vars {
                vars.get_integer(&name)
                    .ok_or_else(|| ErrorKind::Msg(format!("Unknown var {}", name)))
                    .map_err(Into::into)
            } else {
                bail!("Entity doesn't have vars")
            }
        }));
        // Sets the current value of a stat
        t.field("set_var_int", lua::closure3(|lua, this: Ref<LuaEntity>, name: Ref<String>, val: i32| -> UResult<_> {
            let mut entities = lua.write_borrow::<Container>();
            let vars = get_vars(&mut *entities, this.entity);
            if let Some(vars) = vars {
                vars.set_integer(&name, val);
                Ok(())
            } else {
                bail!("Entity doesn't have vars")
            }
        }));
        // Returns the current value of a stat
        t.field("get_var_bool", lua::closure2(|lua, this: Ref<LuaEntity>, name: Ref<String>| -> UResult<_> {
            let mut entities = lua.write_borrow::<Container>();

            let vars = get_vars(&mut *entities, this.entity);
            if let Some(vars) = vars {
                vars.get_boolean(&name)
                    .ok_or_else(|| ErrorKind::Msg(format!("Unknown var {}", name)))
                    .map_err(Into::into)
            } else {
                bail!("Entity doesn't have vars")
            }
        }));
        // Sets the current value of a stat
        t.field("set_var_bool", lua::closure3(|lua, this: Ref<LuaEntity>, name: Ref<String>, val: bool| -> UResult<_> {
            let mut entities = lua.write_borrow::<Container>();
            let vars = get_vars(&mut *entities, this.entity);
            if let Some(vars) = vars {
                vars.set_boolean(&name, val);
                Ok(())
            } else {
                bail!("Entity doesn't have vars")
            }
        }));

        // Charges the entity for a service
        t.field("give_money", lua::closure3(|lua, this: Ref<LuaEntity>, reason: Ref<String>, money: i32| -> UResult<_>{
            let entities = lua.write_borrow::<Container>();
            let log = lua.get_tracked::<Logger>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let _ = reason; // TODO: Log somewhere
            let mut players = lua.write_borrow::<crate::PlayerInfoMap>();
            if let Some(owner) = entities.get_component::<Owned>(this.entity).map(|v| v.player_id) {
                let player = assume!(log, players.get_mut(&owner));
                let money = UniDollar(i64::from(money));
                player.change_money(money);
            }
            Ok(())
        }));
        // Charges the entity for a service
        t.field("give_rating", lua::closure3(|lua, this: Ref<LuaEntity>, reason: Ref<String>, rating: i32| -> UResult<_>{
            use std::cmp;
            let entities = lua.read_borrow::<Container>();
            let log = lua.get_tracked::<Logger>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let _ = reason; // TODO: Log somewhere
            let mut players = lua.write_borrow::<crate::PlayerInfoMap>();
            if let Some(owner) = entities.get_component::<Owned>(this.entity).map(|v| v.player_id) {
                let player = assume!(log, players.get_mut(&owner));
                player.rating = player.rating.saturating_add(rating as i16);
                player.rating = cmp::min(cmp::max(player.rating, -30_000), 30_000);
            }
            Ok(())
        }));
        // Adds a grade to the student's current lesson
        t.field("give_grade", lua::closure2(|lua, this: Ref<LuaEntity>, grade: Ref<String>| -> UResult<bool>{
            let mut entities = lua.write_borrow::<Container>();
            let log = lua.get_tracked::<Logger>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let grade = match &*grade {
                "A" => Grade::A,
                "B" => Grade::B,
                "C" => Grade::C,
                "D" => Grade::D,
                "E" => Grade::E,
                "F" => Grade::F,
                _ => bail!("Invalid grade"),
            };
            let player = entities.get_component::<Owned>(this.entity).map(|v| v.player_id);

            if let Some(owner) = player {
                let mut players = lua.write_borrow::<crate::PlayerInfoMap>();
                let player = assume!(log, players.get_mut(&owner));
                player.grades[grade.as_index()] += 1;
            }

            if let Some((day, slot)) = entities.get_component::<Activity>(this.entity).map(|v| (v.day, v.slot)) {
                if let Some(grades) = entities.get_component_mut::<Grades>(this.entity) {
                    grades.timetable_grades[day as usize][slot as usize] = Some(grade);
                    return Ok(true);
                } else {
                    bail!("Missing grades")
                }
            } else {
                bail!("Missing activity")
            }
        }));
    }
}

pub(crate) struct RunningChoices {
    pub(crate) choices: FNVMap<(PlayerId, usize), RunningChoice>,
    pub(crate) choice_map: Ref<Table>,
}

impl RunningChoices {
    pub fn new(lua: &lua::Lua) -> RunningChoices {
        RunningChoices {
            choices: FNVMap::default(),
            choice_map: Ref::new_table(lua),
        }
    }
}

pub(crate) struct RunningChoice {
    _player: PlayerId,
    pub(crate) pending_entity: Vec<ecs::Entity>,
    entities: Vec<ecs::Entity>,

    pub(crate) handle: Ref<IdleScriptHandle>,
    pub(crate) load_data: Option<Ref<Table>>,
}

impl RunningChoice {
    pub(crate) fn new(log: &Logger, scripting: &lua::Lua, player: PlayerId, name: ResourceKey<'_>, idx: usize) -> RunningChoice {
        let map = assume!(log, scripting.get::<lua::Ref<Table>>(lua::Scope::Global, "idle_storage"));
        let player_map = if let Some(player_map) = map.get::<i32, Ref<Table>>(i32::from(player.0)) {
            player_map
        } else {
            let player_map = Ref::new_table(scripting);
            map.insert(i32::from(player.0), player_map.clone());
            player_map
        };
        let handle = Ref::new(scripting, IdleScriptHandle {
            idx,
            player,
            props: Ref::new_table(scripting),
        });
        player_map.insert(Ref::new_string(scripting, name.as_string()), handle.props.clone());
        RunningChoice {
            _player: player,
            pending_entity: Vec::new(),
            entities: Vec::new(),
            handle,
            load_data: None,
        }
    }
}

pub(crate) struct IdleScriptHandle {
    player: PlayerId,
    idx: usize,
    props: Ref<Table>,
}

impl lua::LuaUsable for IdleScriptHandle {
    fn metatable(t: &lua::TypeBuilder) {
        script::support_getters_setters(t);
    }

    fn fields(t: &lua::TypeBuilder) {
        // // Returns the room's id
        t.field("get_rooms", lua::closure1(|lua, this: Ref<IdleScriptHandle>| {
            get_rooms_for_player::<Types>(lua, this.player)
        }));
        // // Returns property storage
        t.field("get_properties", lua::closure1(|_lua, this: Ref<IdleScriptHandle>| {
            this.props.clone()
        }));
        // // Returns the room's id
        t.field("get_room_by_id", lua::closure2(|lua, this: Ref<IdleScriptHandle>, id: i32| -> UResult<_> {
            use crate::script::ScriptTypes;
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
        // Submits a command that will be executed by all clients and the server
        t.field("execute_command", lua::closure3(|lua, this: Ref<IdleScriptHandle>, method: Ref<String>, data: Ref<Arc<bitio::Writer<Vec<u8>>>>| -> UResult<_> {
            let commands = lua.get_tracked::<ExtraCommands>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let mut commands = commands.borrow_mut();
            commands.push(command::ExecIdle::new(this.player, this.idx, method.to_string(), common::ScriptData(Arc::clone(&data))).into());
            Ok(())
        }));
    }
}

pub(crate) fn create_choices_state(
    log: &Logger,
    entities: &mut Container,
    scripting: &script::Engine,
    choices: &choice::Choices,
    running_choices: &mut RunningChoices,
) -> AlwaysVec<packet::IdleState>
{
    use lua::*;
    let mut state = Vec::with_capacity(running_choices.choices.len());

    for ((player, idx), ref mut rc) in &mut running_choices.choices {
        let script = assume!(log, choices.student_idle.get_choice_by_index(*idx));
        match scripting.with_borrows()
            .borrow_mut(entities)
            .invoke_function::<_, Ref<Table>>("invoke_module_method", (
                Ref::new_string(scripting, script.script.module()),
                Ref::new_string(scripting, script.script.resource()),
                Ref::new_string(scripting, "create_state"),
                rc.handle.clone(),
        )) {
            Ok(val) => {
                let mut se = serde_cbor::ser::Serializer::new(vec![]);
                assume!(log, with_table_deserializer(&val, |de| {
                    serde_transcode::transcode(de, &mut se)
                }));
                let data = se.into_inner();
                state.push(packet::IdleState {
                    player: *player,
                    idx: *idx as u32,
                    state: packet::Raw(data),
                })
            }
            Err(err) => error!(log, "Failed to sync idle script"; "script" => ?script, "error" => %err),
        }
    }

    AlwaysVec(state)
}
pub(super) fn tick_choices(
    log: &Logger,
    entities: &mut Container,
    scripting: &script::Engine,
    players: &mut PlayerInfoMap,
    choices: &choice::Choices,
    running_choices: &mut RunningChoices,
) {
    use rand::thread_rng;

    // First iterate over idle entities, find entities that
    // haven't got a choice selected and generate one for
    // them. Then create a running choice (if one doesn't
    // exist) and add the entity to a list to be passed into
    // the choice's script.

    let mut rng = thread_rng();
    entities.with(|
        em: EntityManager<'_>,
        living: Read<Living>,
        owned: Read<Owned>,
        mut idle: Write<Idle>,
        mut vars: Write<StudentVars>,
        mut controlled: Write<Controlled>,
    |{
        let mut to_remove = vec![];
        for (e, (idle, _living, owned, c)) in em.group_mask((&mut idle, &living, &owned, &mut controlled), |m| m.and(&vars)) {
            // A room owns this entity. Ask them to free it
            if !c.by.map_or(false, Controller::is_idle) && c.by.is_some() {
                c.should_release = true;
                c.wanted = Some(Controller::Idle(0));
                continue;
            }

            if idle.released {
                idle.released = false;
                if let Some(rc) = idle.current_choice.take()
                    .and_then(|v| running_choices.choices.get_mut(&(owned.player_id, v)))
                {
                    rc.entities.retain(|v| *v != e);
                }
                if c.wanted.map_or(false, |v| v > Controller::Idle(0) && !v.is_idle()) {
                    c.by = None;
                    c.should_release = false;
                    to_remove.push(e);
                    continue;
                }
            }

            if idle.current_choice.is_none() {
                let vars = assume!(log, vars.get_custom(e));
                let selected = choices.student_idle.choose(&mut rng, &choices.global.wrap(&vars));
                if let Some((idx, _selected)) = selected {
                    let rc = running_choices.choices.entry((
                        owned.player_id,
                        idx,
                    )).or_insert_with(|| {
                        let name = assume!(log, choices.student_idle.get_choice_name_by_index(idx));
                        RunningChoice::new(log, scripting, owned.player_id, name, idx)
                    });
                    rc.pending_entity.push(e);
                    idle.current_choice = Some(idx);
                    c.by = Some(Controller::Idle(idx));
                }
            } else {
                idle.current_choice.map(|v| c.by = Some(Controller::Idle(v)));
                idle.total_idle_time += 1;
            }
        }
        for e in to_remove {
            idle.remove_component(e);
        }
    });

    // For each choice add any entities in the queue.
    // If the choice has any entities run its tick script

    for ((_player, idx), ref mut rc) in &mut running_choices.choices {
        let script = assume!(log, choices.student_idle.get_choice_by_index(*idx));
        for e in rc.pending_entity.drain(..) {
            let le = entities.with(|
                _em: EntityManager<'_>,
                living: Read<Living>,
                object: Read<Object>,
                mut entity_ref: Write<LuaEntityRef>,
            |{
                LuaEntityRef::get_or_create(&mut entity_ref, &living, &object, scripting, e, Some(Controller::Idle(*idx)))
            });
            if let Err(err) = scripting.with_borrows()
                .borrow_mut(entities)
                .borrow_mut(players)
                .invoke_function::<_, ()>("invoke_module_method", (
                    Ref::new_string(scripting, script.script.module()),
                    Ref::new_string(scripting, script.script.resource()),
                    Ref::new_string(scripting, "add_entity"),
                    rc.handle.clone(),
                    le
            )) {
                error!(log, "Failed to add entity to script"; "script" => ?script, "error" => %err);
            }
            rc.entities.push(e);
        }
        if let Some(load_data) = rc.load_data.take() {
            if let Err(err) = scripting.with_borrows()
                .borrow_mut(entities)
                .borrow_mut(players)
                .invoke_function::<_, ()>("invoke_module_method", (
                    Ref::new_string(scripting, script.script.module()),
                    Ref::new_string(scripting, script.script.resource()),
                    Ref::new_string(scripting, "load"),
                    rc.handle.clone(),
                    load_data,
            )) {
                error!(log, "Failed to load script"; "script" => ?script, "error" => %err);
            }
        }
        if let Err(err) = scripting.with_borrows()
            .borrow_mut(entities)
            .borrow_mut(players)
            .invoke_function::<_, ()>("invoke_module_method", (
                Ref::new_string(scripting, script.script.module()),
                Ref::new_string(scripting, script.script.resource()),
                Ref::new_string(scripting, "update"),
                rc.handle.clone(),
        )) {
            error!(log, "Failed to update script"; "script" => ?script, "error" => %err);
        }
    }
}

pub(super) fn tick_rooms(
    log: &Logger,
    level: &mut Level,
    entities: &mut Container,
    scripting: &script::Engine,
    players: &mut PlayerInfoMap,
) {
    let asset_manager = level.asset_manager.clone();

    for room in level.room_ids() {
        let (needs_update, ty) = {
            let mut room = level.get_room_info_mut(room);
            let nu = room.needs_update;
            room.needs_update = false;
            if room.controller.is_invalid() {
                continue;
            }
            if !room.state.is_done() {
                entities.with(|
                    _em: EntityManager<'_>,
                    mut c: Write<Controlled>,
                    mut gr: Write<GotoRoom>,
                    mut props: Write<LuaRoomProperties>,
                    mut rc: Write<RoomController>,
                |{
                    let rc = assume!(log, rc.get_component_mut(room.controller));
                    for e in &rc.entities {
                        let c = assume!(log, c.get_component_mut(*e));
                        c.should_release = true;
                    }
                    for e in &rc.visitors {
                        let c = assume!(log, c.get_component_mut(*e));
                        c.should_release = true;
                    }
                    for e in rc.waiting_list.drain(..) {
                        gr.remove_component(e);
                    }
                    if rc.entities.is_empty() && rc.visitors.is_empty() {
                        props.remove_component(room.controller);
                    }
                });
            }
            (nu, assume!(log, asset_manager.loader_open::<room::Loader>(room.key.borrow())))
        };
        if let Some(controller) = ty.controller.as_ref() {
            let lua_room = LuaRoom::from_room(log, &*level.rooms.borrow(), entities, room, scripting);
            if needs_update {
                if let Err(err) = scripting.with_borrows()
                    .borrow_mut(entities)
                    .borrow_mut(players)
                    .invoke_function::<_, ()>("invoke_module_method", (
                        Ref::new_string(scripting, controller.module()),
                        Ref::new_string(scripting, controller.resource()),
                        Ref::new_string(scripting, "update"),
                        lua_room.clone()
                )) {
                    error!(log, "Failed to update room: {}", err; "room" => ?room, "type" => &ty.name);
                }
            }
            if let Err(err) = scripting.with_borrows()
                .borrow_mut(entities)
                .borrow_mut(players)
                .invoke_function::<_, ()>("invoke_module_method", (
                    Ref::new_string(scripting, controller.module()),
                    Ref::new_string(scripting, controller.resource()),
                    Ref::new_string(scripting, "server"),
                    lua_room
            )) {
                error!(log, "Failed to tick room"; "room" => ?room, "type" => &ty.name, "error" => %err);
            }
        }
    }
}

pub enum Types {}

impl script::ScriptTypes for Types {
    type EntityRef = LuaEntityRef;
    type Entity = LuaEntity;
    type RoomRef = LuaRoomProperties;
    type Room = LuaRoom;

    fn from_entity(
        lua: &Lua,
        props: &mut ecs::Write<Self::EntityRef>,
        living: &ecs::Read<Living>,
        object: &ecs::Read<Object>,
        e: Entity,
        controller: Option<Controller>
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


/// Fake player used by the server
pub(crate) struct ServerPlayer {
    pub state: player::State,
    pub config: player::PlayerConfig,
}

impl player::Player for ServerPlayer {
    type EntityCreator = ServerEntityCreator;
    type EntityInfo = ServerComponent;

    fn get_uid(&self) -> PlayerId {
        PlayerId(0)
    }
    fn set_state(&mut self, state: player::State) {
        self.state = state;
    }
    fn get_state(&self) -> player::State {
        self.state.clone()
    }
    fn can_charge(&self) -> bool {
        false
    }
    fn get_money(&self) -> UniDollar {
        UniDollar(99_999_999)
    }
    fn change_money(&mut self, _val: UniDollar) {}

    fn get_rating(&self) -> i16 {
        0x1FFF
    }
    fn set_rating(&mut self, _val: i16) {}
    fn get_config(&self) -> player::PlayerConfig {
        self.config.clone()
    }
    fn set_config(&mut self, cfg: player::PlayerConfig) {
        self.config = cfg;
    }
}

pub(crate) struct Handler<'a> {
    pub(crate) running_choices: &'a RunningChoices,
    pub(crate) choices: &'a choice::Choices,
}
impl <'a> CommandHandler for Handler<'a> {
    type Player = ServerPlayer;

    fn execute_exec_idle<E>(&mut self, cmd: &mut ExecIdle, _player: &mut ServerPlayer, params: &mut CommandParams<'_, E>) -> UResult<()>
        where E: Invokable,
    {
        let rc = if let Some(rc) = self.running_choices.choices.get(&(cmd.player, cmd.idx as usize)) {
            rc
        } else {
            bail!("Missing idle task")
        };
        let script = assume!(params.log, self.choices.student_idle.get_choice_by_index(cmd.idx as usize));
        if let Err(err) = params.engine.with_borrows()
            .borrow_mut(params.entities)
            .invoke_function::<_, ()>("invoke_module_method", (
                Ref::new_string(params.engine, script.script.module()),
                Ref::new_string(params.engine, script.script.resource()),
                Ref::new_string(params.engine, "on_exec"),
                rc.handle.clone(),
                Ref::new_string(params.engine, cmd.method.as_str()),
                Ref::new(params.engine, Arc::clone(&cmd.data.0))
        )) {
            error!(params.log, "Failed to on_exec idle script"; "error" => %err);
        }
        Ok(())
    }
}

pub(crate) enum ExtraCommands {}

impl lua::LuaUsable for ExtraCommands {}
impl script::LuaTracked for ExtraCommands {
    const KEY: script::NulledString = nul_str!("extra_commands");
    type Storage = Rc<RefCell<Vec<command::Command>>>;
    type Output = Rc<RefCell<Vec<command::Command>>>;
    fn try_convert(s: &Self::Storage) -> Option<Self::Output> {
        Some(s.clone())
    }
}