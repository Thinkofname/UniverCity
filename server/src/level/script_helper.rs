use super::*;
use crate::ecs;
use crate::prelude::*;
use crate::script;

/// Returns a list of rooms owned by the player
pub fn get_rooms_for_player<T: script::ScriptTypes>(
    lua: &lua::Lua,
    player: PlayerId,
) -> UResult<lua::Ref<lua::Table>> {
    use lua::Ref;
    let log = lua
        .get_tracked::<Logger>()
        .ok_or_else(|| ErrorKind::InvalidState)?;
    let rooms = lua
        .get_tracked::<LevelRooms>()
        .ok_or_else(|| ErrorKind::InvalidState)?;
    let rooms = rooms.borrow();
    let mut entities = lua.write_borrow::<Container>();

    let out = Ref::new_table(lua);

    entities.with(
        |_em: EntityManager<'_>,
         rc: ecs::Read<RoomController>,
         mut entity_ref: ecs::Write<T::EntityRef>,
         mut room_ref: ecs::Write<T::RoomRef>,
         living: ecs::Read<Living>,
         object: ecs::Read<Object>| {
            for (i, room) in rooms
                .room_ids()
                .map(|v| rooms.get_room_info(v))
                .filter(|v| v.state.is_done())
                .filter(|v| v.owner == player)
                .filter(|v| v.key != ResourceKey::new("base", "owned_area"))
                .enumerate()
            {
                let r = T::from_room(
                    &log,
                    lua,
                    &rooms,
                    &mut room_ref,
                    &rc,
                    &mut entity_ref,
                    &living,
                    &object,
                    room.id,
                );
                out.insert((i + 1) as i32, r);
            }
        },
    );
    Ok(out)
}

/// Sets up a interface for scripts to interface with the level
pub fn init_levellib<T: script::ScriptTypes>(lua: &lua::Lua) {
    use lua::{Ref, Scope, Table};

    lua.set(
        Scope::Global,
        "get_entity_by_id",
        lua::closure1(move |lua, id: i32| -> UResult<_> {
            let entity_map = lua
                .get_tracked::<snapshot::EntityMap>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let entity_map = entity_map.borrow();
            let entity = entity_map[id as usize];

            let mut entities = lua.write_borrow::<Container>();
            entities.with(
                |_em: EntityManager<'_>,
                 mut entity_ref: ecs::Write<T::EntityRef>,
                 controlled: ecs::Read<Controlled>,
                 living: ecs::Read<Living>,
                 object: ecs::Read<Object>| {
                    Ok(entity.map(|e| {
                        let c = controlled.get_component(e).and_then(|v| v.by);
                        T::from_entity(lua, &mut entity_ref, &living, &object, e, c)
                    }))
                },
            )
        }),
    );

    lua.set::<Option<i32>>(Scope::Registry, "level_virtual_mode", None);
    fn level<'a>(
        lua: &lua::Lua,
        tiles: &'a LevelTiles,
        rooms: &'a LevelRooms,
    ) -> UResult<&'a dyn LevelView> {
        if let Some(id) = lua
            .get::<Option<i32>>(Scope::Registry, "level_virtual_mode")
            .expect("Level virtual mode incorrect")
        {
            let log = lua
                .get_tracked::<Logger>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            Ok({
                let room = rooms.get_room_info(room::Id(id as i16));
                assume!(log, room.building_level.as_ref())
            })
        } else {
            Ok(tiles)
        }
    };

    lua.set(
        Scope::Global,
        "level_get_player_rooms",
        lua::closure1(|lua, player: i32| -> UResult<Ref<Table>> {
            let player = PlayerId(player as i16);
            get_rooms_for_player::<T>(lua, player)
        }),
    );

    lua.set(
        Scope::Global,
        "level_get_tile",
        lua::closure2(move |lua, x: i32, y: i32| -> UResult<_> {
            let tiles = lua
                .get_tracked::<LevelTiles>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let tiles = tiles.borrow();
            let rooms = lua
                .get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let level = level(lua, &tiles, &rooms)?;
            Ok(i32::from(level.get_tile(Location::new(x, y)).id))
        }),
    );
    lua.set(
        Scope::Global,
        "level_tile_name",
        lua::closure1(move |lua, id: i32| -> UResult<_> {
            let assets = lua
                .get_tracked::<AssetManager>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let tile = assets.loader_open::<tile::ById>(id as tile::TileId)?;
            Ok(Ref::new_string(lua, tile.key.as_string()))
        }),
    );
    lua.set(
        Scope::Global,
        "level_tile_prop",
        lua::closure2(move |lua, id: i32, prop: Ref<String>| -> UResult<_> {
            let assets = lua
                .get_tracked::<AssetManager>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let tile = assets.loader_open::<tile::ById>(id as tile::TileId)?;
            Ok(tile.properties.contains(&*prop))
        }),
    );
    lua.set(
        Scope::Global,
        "level_get_wall",
        lua::closure3(
            move |lua, x: i32, y: i32, dir: Ref<String>| -> UResult<Ref<String>> {
                let tiles = lua
                    .get_tracked::<LevelTiles>()
                    .ok_or_else(|| ErrorKind::InvalidState)?;
                let tiles = tiles.borrow();
                let rooms = lua
                    .get_tracked::<LevelRooms>()
                    .ok_or_else(|| ErrorKind::InvalidState)?;
                let rooms = rooms.borrow();
                let level = level(lua, &tiles, &rooms)?;
                let direction = Direction::from_str(&dir)?;
                let info = level.get_wall_info(Location::new(x, y), direction);
                Ok(Ref::new_string(
                    lua,
                    match info {
                        None => "none",
                        Some(WallInfo {
                            flag: TileWallFlag::None,
                        }) => "wall",
                        Some(WallInfo {
                            flag: TileWallFlag::Window(_),
                        }) => "window",
                        Some(WallInfo {
                            flag: TileWallFlag::Door,
                        }) => "door",
                    },
                ))
            },
        ),
    );
    lua.set(
        Scope::Global,
        "level_get_room_type_at",
        lua::closure2(move |lua, x: i32, y: i32| -> UResult<_> {
            let tiles = lua
                .get_tracked::<LevelTiles>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let tiles = tiles.borrow();
            let rooms = lua
                .get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let level = level(lua, &tiles, &rooms)?;
            let loc = Location::new(x, y);
            Ok(level
                .get_room_owner(loc)
                .map(|v| rooms.get_room_info(v))
                .map(|v| Ref::new_string(lua, v.key.as_string())))
        }),
    );
    lua.set(
        Scope::Global,
        "level_is_room_type_at",
        lua::closure3(move |lua, x: i32, y: i32, ty: Ref<String>| -> UResult<_> {
            let rooms = lua
                .get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let loc = Location::new(x, y);

            let ty = ResourceKey::parse(&*ty)
                .ok_or_else(|| ErrorKind::Msg("Invalid resource key".into()))?;

            let res = rooms
                .room_ids()
                .map(|v| rooms.get_room_info(v))
                .filter(|v| v.state.is_done())
                .filter(|v| v.key == ty)
                .any(|v| v.area.in_bounds(loc));
            Ok(res)
        }),
    );
    lua.set(
        Scope::Global,
        "level_get_room_display_name",
        lua::closure1(move |lua, id: i32| -> UResult<_> {
            let rooms = lua
                .get_tracked::<LevelRooms>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let rooms = rooms.borrow();
            let assets = lua
                .get_tracked::<AssetManager>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            Ok(rooms
                .try_room_info(RoomId(id as i16))
                .and_then(|v| assets.loader_open::<room::Loader>(v.key.borrow()).ok())
                .map(|v| Ref::new_string(lua, v.name.as_str())))
        }),
    );
}
