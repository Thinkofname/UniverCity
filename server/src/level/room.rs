//! Manages tiles and any information attached to them

use super::*;

use serde_json;

use crate::util::{BitSet, FNVMap};
use std::collections::hash_map::Entry;
use std::sync::Arc;
use crate::assets;
use crate::level::Check;
use crate::player;
use crate::prelude::*;
use crate::script;

/// Loads room descriptions from an asset manager.
pub enum Loader {}

impl <'a> assets::AssetLoader<'a> for Loader {
    type LoaderData = LoaderData;
    type Return = Arc<Room>;
    type Key = assets::ResourceKey<'a>;

    fn init(_assets: &assets::Store) -> Self::LoaderData {
        LoaderData {
            by_name: Default::default(),
            object_groups: Default::default(),
        }
    }

    fn load(data: &mut Self::LoaderData, assets: &assets::AssetManager, resource: Self::Key) -> UResult<Self::Return> {
        if let Some(room) = data.by_name.get(&resource) {
            return Ok(room.clone());
        }

        let file = assets.open_from_pack(resource.module_key(), &format!("rooms/{}.json", resource.resource()))?;
        let info: RoomInfo = serde_json::from_reader(file)?;

        let mut checks = vec![];
        for m in info.placement_match {
            checks.push(Check::new(resource.module_key(), 0, 0, &m));
        }
        if checks.is_empty() {
            checks.push(Check::Always);
        }

        let mut valid_objects = Vec::new();
        for v in info.valid_objects {
            if v.starts_with('@') {
                let group = assets::LazyResourceKey::parse(&v[1..])
                    .or_module(resource.module_key())
                    .into_owned();
                let objects = match data.object_groups.entry(group) {
                    Entry::Occupied(val) => val.into_mut(),
                    Entry::Vacant(val) => {
                        let file = assets.open_from_pack(val.key().module_key(), &format!("objects/groups/{}.json", val.key().resource()))?;
                        let info: Vec<String> = serde_json::from_reader(file)?;

                        val.insert(info.into_iter()
                            .map(|v|
                                LazyResourceKey::parse(&v)
                                    .or_module(resource.module_key())
                                    .into_owned()
                            )
                            .collect())
                    }
                };
                valid_objects.extend(objects.iter().cloned());
            } else {
                valid_objects.push(assets::LazyResourceKey::parse(&v)
                    .or_module(resource.module_key())
                    .into_owned());
            }
        }

        let room = Arc::new(Room {
            name: info.name,
            min_size: (info.min_size.x, info.min_size.y),
            tile: assets::LazyResourceKey::parse(&info.tile).or_module(resource.module_key()).into_owned(),
            border_tile: info.border_tile.map(|v| assets::LazyResourceKey::parse(&v).or_module(resource.module_key()).into_owned()),
            tile_placer: info.tile_placer.map(|v| {
                let (sub, method) = if let Some(pos) = v.char_indices().find(|v| v.1 == '#') {
                    v.split_at(pos.0)
                } else {
                    panic!("Invalid placer")
                };
                (
                    assets::LazyResourceKey::parse(sub)
                        .or_module(resource.module_key())
                        .into_owned(),
                    method[1..].into()
                )
            }),
            tile_updater: info.tile_updater.map(|v| {
                assets::LazyResourceKey::parse(&v)
                    .or_module(resource.module_key())
                    .into_owned()
            }),
            wall: info.wall.map(|v| RoomWalls {
                texture: assets::LazyResourceKey::parse(&v.texture)
                    .or_module(resource.module_key())
                    .into_owned(),
                texture_top: v.texture_top.map(|v| assets::LazyResourceKey::parse(&v)
                    .or_module(resource.module_key())
                    .into_owned()),
                priority: v.priority,
            }),
            placement_checks: checks,
            only_within: info.only_within.map(|v| assets::LazyResourceKey::parse(&v)
                .or_module(resource.module_key())
                .into_owned()),
            valid_objects,
            required_objects: info.required_objects
                .into_iter()
                .map(|(k, v)| (
                    assets::LazyResourceKey::parse(&k).or_module(resource.module_key()).into_owned(),
                    v
                ))
                .collect(),
            required_entities: info.required_entities
                .into_iter()
                .map(|(k, v)| (
                    assets::LazyResourceKey::parse(&k).or_module(resource.module_key()).into_owned(),
                    v
                ))
                .collect(),
            build_anywhere: info.build_anywhere,
            requirements: info.requirements.into_iter()
                .map(|v| match v {
                    RequirementInfo::Room{key, count} => Requirement::Room {
                        key: LazyResourceKey::parse(&key)
                            .or_module(resource.module_key())
                            .into_owned(),
                        count,
                    }
                })
                .collect(),
            controller: info.controller.map(|v| {
                assets::LazyResourceKey::parse(&v)
                    .or_module(resource.module_key())
                    .into_owned()
            }),
            allow_edit: info.allow_edit,
            allow_limited_edit: info.allow_limited_edit,
            base_cost: info.base_cost,
            cost_per_tile: info.cost_per_tile,
            can_idle: info.can_idle,
            used_for_teaching: info.used_for_teaching,
        });

        data.by_name.insert(resource.into_owned(), room.clone());

        Ok(room)
    }
}

/// A collection of rooms that can be used in a level
pub struct LoaderData {
    by_name: FNVMap<ResourceKey<'static>, Arc<Room>>,
    object_groups: FNVMap<ResourceKey<'static>, Vec<ResourceKey<'static>>>,
}

/// Information about the room including placement rules and
/// details about tiles needed to render it.
#[derive(Debug)]
pub struct Room {
    /// The display name of the room
    pub name: String,
    /// The smallest allowed size of the room (width, height)
    pub min_size: (i32, i32),
    /// The tile to be placed in the room's area
    pub tile: assets::ResourceKey<'static>,
    /// The tile to be placed at the edge of the room
    /// (within the bounds). If `None` then `tile` is used
    pub border_tile: Option<assets::ResourceKey<'static>>,
    /// The textures to use for the walls
    pub wall: Option<RoomWalls>,
    /// The optional lua method that handles placement of the
    /// tiles in this room
    pub tile_placer: Option<(assets::ResourceKey<'static>, String)>,
    /// The lua method that will be run when something changes in a room
    /// to update some tiles
    pub tile_updater: Option<assets::ResourceKey<'static>>,
    /// List of placement checks to run per a tile
    pub placement_checks: Vec<Check>,
    /// Limits this room's placement to being within a room of
    /// the type
    pub only_within: Option<ResourceKey<'static>>,
    /// List of objects that can be placed in the room
    pub valid_objects: Vec<assets::ResourceKey<'static>>,
    /// Objects that are required to be in the room
    pub required_objects: FNVMap<assets::ResourceKey<'static>, i32>,
    /// Entities that are required for the room to
    /// be considered active.
    pub required_entities: FNVMap<assets::ResourceKey<'static>, i32>,
    /// Removes the requirement that the player must own the land to
    /// build on it.
    pub build_anywhere: bool,
    /// Requirements for the room to be buildable
    pub requirements: Vec<Requirement>,
    /// The lua method that will be run every tick to control the room
    pub controller: Option<assets::ResourceKey<'static>>,
    /// Whether the room can be editted by a player once placed
    pub allow_edit: bool,
    /// Allows the room to be editted in a limited mode which
    /// prevents resizing or removing the room.
    pub allow_limited_edit: bool,
    /// The flat cost of placing the room.
    /// If the room meets the minimum size requirements
    /// of the room this would be its cost.
    pub base_cost: Option<UniDollar>,
    /// The cost per an tile in the room over the
    /// minimum requirements.
    pub cost_per_tile: Option<UniDollar>,
    /// Whether a player can idle within this room
    pub can_idle: bool,
    /// Whether the room is used for teaching
    ///
    /// Used for inspectors and for student spawning calculations
    pub used_for_teaching: bool,
}

/// Information about walls within this room
#[derive(Debug)]
pub struct RoomWalls {
    /// Texture for the wall.
    pub texture: assets::ResourceKey<'static>,
    /// Optional texture for the top of the wall.
    ///
    /// Defaults to using the same texture as the rest of
    /// the wall.
    pub texture_top: Option<assets::ResourceKey<'static>>,
    /// Marks the wall texture as being overriding
    /// most other textures
    pub priority: bool,
}

/// A op to be performed against an entity's feelings
#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
#[serde(tag = "op", content = "value")]
pub enum BiasOp {
    /// Bias towards entity's with feelings less than the
    /// value.
    #[serde(rename = "less_than")]
    LessThan(f32),
    /// Bias towards entity's with feelings more than the
    /// value.
    #[serde(rename = "more_than")]
    MoreThan(f32),
}

/// A requirement for a room to be buildable
#[derive(Debug)]
pub enum Requirement {
    /// Requires a certain number of rooms to be built
    Room {
        /// The type of room
        key: ResourceKey<'static>,
        /// The number required
        count: usize,
    }
}

impl Requirement {

    /// Returns whether the player meets the requirement
    pub fn check_requirement(&self, level: &Level, player: PlayerId) -> bool {
        use self::Requirement::*;
        match *self {
            Room{ref key, count} => {
                let ids = level.room_ids();
                let c = ids.into_iter()
                    .map(|v| level.get_room_info(v))
                    .filter(|v| v.owner == player)
                    .filter(|v| v.key == *key)
                    .count();
                if c < count {
                    return false;
                }
            },
        }
        true
    }
    /// Prints a formatted version of this requirement
    pub fn print_tooltip<W>(&self, assets: &AssetManager, w: &mut W) -> ::std::fmt::Result
        where W: ::std::fmt::Write
    {
        use self::Requirement::*;
        match *self {
            Room{ref key, count} => {
                let ty;
                let s;
                let name = if let Ok(t) = assets.loader_open::<Loader>(key.borrow()) {
                    ty = t;
                    &ty.name
                } else {
                    s = key.as_string();
                    &s
                };
                if count == 1 {
                    write!(w, "Requires a *{}*", name)?;
                } else {
                    write!(w, "Requires at least #{}# *{}*", count, name)?;
                }
            }
        }
        Ok(())
    }
}

impl Room {
    /// Calculates the cost to build the passed room
    pub fn cost_for_room(&self, level: &Level, room_id: Id) -> UniDollar {
        let info = level.get_room_info(room_id);
        let mut cost = self.cost_for_area(info.area);
        for obj in level.get_room_objects(room_id).iter().filter_map(|v| v.as_ref()) {
            let obj = assume!(level.log, level.asset_manager.loader_open::<object::Loader>(obj.0.key.borrow()));
            cost += obj.cost;
        }
        cost
    }
    /// Calculates the cost to build a room of this
    /// size. If the room is too small it returns
    /// the base cost
    pub fn cost_for_area(&self, area: Bound) -> UniDollar {
        let mut cost = self.base_cost.unwrap_or(UniDollar(0));
        let min_size = self.min_size.0 * self.min_size.1;
        let size = area.width() * area.height();
        // If the room is too small its invalid so just show the base
        // cost.
        // If its more than the base size then start charging per
        // a tile extra
        if size > min_size {
            cost += (size - min_size) * self.cost_per_tile.unwrap_or(UniDollar(0));
        }
        cost
    }

    /// Returns whether the player is able to build a room of this type
    pub fn check_requirements(&self, level: &Level, player: PlayerId) -> bool {
        for req in &self.requirements {
            if !req.check_requirement(level, player) {
                return false;
            }
        }
        true
    }

    /// Returns whether the room placement is valid.
    ///
    /// Currently this only checks object requirements.
    pub fn is_valid_placement(&self, level: &Level, room_id: Id) -> bool {
        self.check_valid_placement(level, room_id, |_, _| {})
    }

    /// Returns whether the room placement is valid.
    ///
    /// The callback is called for matching objects to allow
    /// for the display of the remaining requirements.
    /// Currently this only checks object requirements.
    pub fn check_valid_placement<F>(&self, level: &Level, room_id: Id, mut cb: F) -> bool
        where F: FnMut(usize, i32)
    {

        let required = &self.required_objects;
        // Check each requirement against the currently
        // placed objects.
        let mut valid = true;
        for (k, v) in required {
            let mut count = *v;
            // Weak match against the key's the objects
            // to work out how many are placed.
            //
            // Weak matching allows for things like
            // doors to be grouped.
            for obj in level.get_room_objects(room_id).iter() {
                if let Some(obj) = obj.as_ref() {
                    if k.weak_match(&obj.0.key) {
                        count -= 1;
                    }
                }
            }

            // Still need to place more, mark the
            // room as invalid
            if count > 0 {
                valid = false;
            }
            // Find the ui elements of matching objects
            // and update their requirements text.
            for id in self.valid_objects.iter()
                    .enumerate()
                    .filter(|&(_, o)| k.weak_match(o))
                    .map(|(id, _)| id)
            {
                cb(id, count);
            }
        }
        valid
    }
}

/// Represents a room id
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, DeltaEncode)]
pub struct Id(pub i16);

/// Marks a room placed in the level
pub struct RoomPlacement {
    /// The id of the room
    pub id: room::Id,
    /// The owner of the room
    pub owner: player::Id,
    /// The bounding area of the room
    pub area: Bound,
    /// The resource that the room was loaded from
    pub key: assets::ResourceKey<'static>,
    /// The state of the room (e.g. planning etc)
    pub state: RoomState,
    // TODO: This should really be a apart of the room state instead of
    // seperate.
    /// If the room is currently being worked on then
    /// this will contain the room
    pub building_level: Option<Box<RoomVirtualLevel>>,
    /// List of objects currently placed in the room
    pub objects: Vec<Option<(ObjectPlacement, ReverseObjectPlacement)>>,
    /// The entity that controls this room or `Entity::invalid` if
    /// building.
    pub controller: Entity,
    /// Placement bit set used for quicking testing an area to see if
    /// an object can be placed.
    ///
    /// Its size is 4 times larger than the room.
    pub placement_map: BitSet,
    /// Collision bit set used for quicking testing an area to see if
    /// it is pathable.
    ///
    /// Its size is 4 times larger than the room.
    pub collision_map: BitSet,
    /// Bit set used to mark tiles as blocked by an object
    pub blocked_map: BitSet,
    /// Whether this room has changed and needs the script's
    /// update function run on it.
    ///
    /// Only happens on the server
    pub needs_update: bool,
    /// A list of the original tiles in the area the room
    /// takes up. Stored in the iteration order of the room
    /// bound.
    pub original_tiles: Vec<TileData>,

    pub(super) temp_placement: Option<super::placement::TempObjectPlacement>,
    /// Whether the room is being editted in limited mode
    pub limited_editing: bool,
    /// Whether this room has its walls lowered.
    ///
    /// Only valid when editing in limited mode
    pub lower_walls: bool,
    /// The amount of money put into placing this room.
    /// This is used as a buffer when editting a room
    pub placement_cost: UniDollar,
    /// The stored state from the tile updater script
    pub tile_update_state: Option<Vec<u8>>,
    /// Tiles this room requires to be not blocked to be able to fully edit
    pub required_tiles: Vec<Location>,
}

impl RoomPlacement {
    /// Optionally runs the rooms update script
    pub fn do_update<E, T: script::ScriptTypes>(
        log: &Logger, assets: &AssetManager, engine: &E,
        entities: &mut Container,
        rooms: &RefCell<LevelRooms>, room_id: RoomId,
    )
        where E: script::Invokable
    {
        use lua::*;
        let (key, is_virtual) = {
            let rooms = rooms.borrow();
            let room = rooms.get_room_info(room_id);
            (room.key.clone(), room.building_level.is_some())
        };
        let room = assume!(log, assets.loader_open::<Loader>(key.borrow()));
        if let Some(tile_updater) = room.tile_updater.as_ref() {
            if is_virtual {
                engine.set::<Option<i32>>(Scope::Registry, "level_virtual_mode", Some(i32::from(room_id.0)));
            }
            let lua_room = entities.with(|
                _em: EntityManager<'_>,
                mut props: ecs::Write<T::RoomRef>,
                rc: ecs::Read<RoomController>,
                mut entity_ref: ecs::Write<T::EntityRef>,
                living: ecs::Read<Living>,
                object: ecs::Read<Object>,
            | {
                T::from_room(
                    log,
                    engine,
                    &*rooms.borrow(),
                    &mut props,
                    &rc,
                    &mut entity_ref,
                    &living,
                    &object,
                    room_id,
                )
            });
            let prev = {
                let rooms = rooms.borrow();
                let room = rooms.get_room_info(room_id);
                room.tile_update_state.as_ref().map(|v| {
                    let mut de = serde_cbor::de::Deserializer::from_slice(v);
                    assume!(log, with_table_serializer(engine, |se| serde_transcode::transcode(&mut de, se)))
                })
            };
            let res = engine.with_borrows()
                .borrow_mut(entities)
                .invoke_function::<_, Ref<Table>>("invoke_module_method", (
                    Ref::new_string(engine, tile_updater.module()),
                    Ref::new_string(engine, tile_updater.resource()),
                    Ref::new_string(engine, "update"),
                    lua_room,
                    prev,
                ));
            if is_virtual {
                engine.set::<Option<i32>>(Scope::Registry, "level_virtual_mode", None);
            }
            let res = match res {
                Ok(val) => val,
                Err(err) => {
                    error!(log, "room tile update failed"; "error" => %err);
                    return;
                }
            };
            let mut se = serde_cbor::ser::Serializer::new(vec![]);
            assume!(log, with_table_deserializer(&res, |de| {
                serde_transcode::transcode(de, &mut se)
            }));
            let data = se.into_inner();
            {
                let mut rooms = rooms.borrow_mut();
                let room = rooms.get_room_info_mut(room_id);
                room.tile_update_state = Some(data);
            }
        }
    }
    /// Optionally runs the rooms update apply script
    pub fn do_update_apply<E, T: script::ScriptTypes>(
        log: &Logger, assets: &AssetManager, engine: &E,
        entities: &mut Container,
        rooms: &RefCell<LevelRooms>, room_id: RoomId,
    )
        where E: script::Invokable
    {
        use lua::*;
        let (key, is_virtual) = {
            let rooms = rooms.borrow();
            let room = rooms.get_room_info(room_id);
            (room.key.clone(), room.building_level.is_some())
        };
        let room = assume!(log, assets.loader_open::<Loader>(key.borrow()));
        if let Some(tile_updater) = room.tile_updater.as_ref() {
            if is_virtual {
                engine.set::<Option<i32>>(Scope::Registry, "level_virtual_mode", Some(i32::from(room_id.0)));
            }
            let lua_room = entities.with(|
                _em: EntityManager<'_>,
                mut props: ecs::Write<T::RoomRef>,
                rc: ecs::Read<RoomController>,
                mut entity_ref: ecs::Write<T::EntityRef>,
                living: ecs::Read<Living>,
                object: ecs::Read<Object>,
            | {
                T::from_room(
                    log,
                    engine,
                    &*rooms.borrow(),
                    &mut props,
                    &rc,
                    &mut entity_ref,
                    &living,
                    &object,
                    room_id,
                )
            });
            let (prev, module) = {
                let mut rooms = rooms.borrow_mut();
                let room = rooms.get_room_info_mut(room_id);
                room.required_tiles.clear();
                (
                    room.tile_update_state.as_ref().map(|v| {
                        let mut de = serde_cbor::de::Deserializer::from_slice(v);
                        assume!(log, with_table_serializer(engine, |se| serde_transcode::transcode(&mut de, se)))
                    }),
                    room.key.module_key().into_owned()
                )
            };

            struct Placer {
                room_id: room::Id,
                module: ModuleKey<'static>,
            }

            fn set_tile(lua: &lua::Lua, placer: Ref<Placer>, x: i32, y: i32, tile: Ref<String>) -> UResult<()> {
                let tile = assets::LazyResourceKey::parse(&tile)
                    .or_module(placer.module.borrow());

                let rooms = lua.get_tracked::<LevelRooms>()
                    .ok_or_else(|| ErrorKind::InvalidState)?;
                let tiles = lua.get_tracked::<LevelTiles>()
                    .ok_or_else(|| ErrorKind::InvalidState)?;

                let mut tiles = tiles.borrow_mut();
                let mut rooms = rooms.borrow_mut();
                let room = rooms.try_room_info_mut(placer.room_id)
                    .ok_or_else(|| ErrorKind::StaleScriptReference)?;

                let location = Location::new(x, y);
                if let Some(lvl) = room.building_level.as_mut() {
                    lvl.set_tile(location, tile);
                } else {
                    tiles.set_tile(location, tile);
                }

                Ok(())
            }

            fn depends_on(lua: &lua::Lua, placer: Ref<Placer>, x: i32, y: i32) -> UResult<()> {
                let rooms = lua.get_tracked::<LevelRooms>()
                    .ok_or_else(|| ErrorKind::InvalidState)?;
                let mut rooms = rooms.borrow_mut();
                let room = rooms.try_room_info_mut(placer.room_id)
                    .ok_or_else(|| ErrorKind::StaleScriptReference)?;

                room.required_tiles.push(Location::new(x, y));

                Ok(())
            }

            impl lua::LuaUsable for Placer {
                fn metatable(t: &lua::TypeBuilder) {
                    script::support_getters_setters(t);
                }
                fn fields(t: &lua::TypeBuilder) {
                    t.field("set_tile", lua::closure4(set_tile));
                    t.field("depends_on", lua::closure3(depends_on));
                }
            }

            let placer = Ref::new(engine, Placer {
                room_id,
                module,
            });

            if let Err(err) = engine.with_borrows()
                .borrow_mut(entities)
                .invoke_function::<_, ()>("invoke_module_method", (
                    Ref::new_string(engine, tile_updater.module()),
                    Ref::new_string(engine, tile_updater.resource()),
                    Ref::new_string(engine, "apply"),
                    lua_room,
                    placer,
                    prev,
                )) {
                error!(log, "room tile apply failed"; "error" => %err);
            }
            if is_virtual {
                engine.set::<Option<i32>>(Scope::Registry, "level_virtual_mode", None);
            }
        }
    }

    pub(crate) fn rebuild_placement_map(&mut self) {
        Self::build_placement_map(&mut self.placement_map, self.area, &self.objects);
    }

    pub(super) fn build_placement_map(
            placement_map: &mut BitSet,
            area: Bound,
            objects: &[Option<(ObjectPlacement, ReverseObjectPlacement)>]
    ) {
        placement_map.clear();
        for obj in objects.iter().filter_map(|v| v.as_ref()) {
            for action in &obj.0.actions.0 {
                if let ObjectPlacementAction::PlacementBound{location, size} = *action {
                    let min_x = (location.0.max(area.min.x as f32) * 4.0).floor() as usize;
                    let min_y = (location.1.max(area.min.y as f32) * 4.0).floor() as usize;
                    let max_x = ((location.0 + size.0).min(area.max.x as f32 + 1.0) * 4.0).ceil() as usize;
                    let max_y = ((location.1 + size.1).min(area.max.y as f32 + 1.0) * 4.0).ceil() as usize;
                    for y in min_y .. max_y {
                        for x in min_x .. max_x {
                            let lx = x - (area.min.x * 4) as usize;
                            let ly = y - (area.min.y * 4) as usize;
                            placement_map.set(lx + ly * (area.width() as usize * 4), true);
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn rebuild_object_maps(&mut self) {
        self.rebuild_placement_map();
        self.collision_map.clear();
        self.blocked_map.clear();
        let required_space = (self.area.width() * 4 * self.area.height() * 4) as usize;
        if self.collision_map.capacity() < required_space {
            self.collision_map.resize(required_space);
        }
        let required_space = (self.area.width() * self.area.height()) as usize;
        if self.blocked_map.capacity() < required_space {
            self.blocked_map.resize(required_space);
        }
        for obj in self.objects.iter().filter_map(|v| v.as_ref()) {
            for action in &obj.0.actions.0 {
                match *action {
                    ObjectPlacementAction::CollisionBound{location, size} => {
                        let min_x = (location.0.max(self.area.min.x as f32) * 4.0).floor() as usize;
                        let min_y = (location.1.max(self.area.min.y as f32) * 4.0).floor() as usize;
                        let max_x = ((location.0 + size.0).min(self.area.max.x as f32 + 1.0) * 4.0).ceil() as usize;
                        let max_y = ((location.1 + size.1).min(self.area.max.y as f32 + 1.0) * 4.0).ceil() as usize;
                        for y in min_y .. max_y {
                            for x in min_x .. max_x {
                                let lx = x - (self.area.min.x * 4) as usize;
                                let ly = y - (self.area.min.y * 4) as usize;
                                self.collision_map.set(lx + ly * (self.area.width() as usize * 4), true);
                            }
                        }
                    },
                    ObjectPlacementAction::PlacementBound{location, size} => {
                        let min_x = location.0.max(self.area.min.x as f32).floor() as usize;
                        let min_y = location.1.max(self.area.min.y as f32).floor() as usize;
                        let max_x = (location.0 + size.0).min(self.area.max.x as f32 + 1.0).ceil() as usize;
                        let max_y = (location.1 + size.1).min(self.area.max.y as f32 + 1.0).ceil() as usize;
                        for y in min_y .. max_y {
                            for x in min_x .. max_x {
                                let lx = x - self.area.min.x as usize;
                                let ly = y - self.area.min.y as usize;
                                self.blocked_map.set(lx + ly * (self.area.width() as usize), true);
                            }
                        }
                    },
                    ObjectPlacementAction::BlocksTile(location) => {
                        let loc = location - (self.area.min.x, self.area.min.y);
                        self.blocked_map.set((loc.x + loc.y * self.area.width()) as usize, true);
                    },
                    _ => {},
                }
            }
        }
    }

    /// Finds a free location within a room
    pub fn find_free_point(&self, tiles: &LevelTiles, rooms: &LevelRooms, target: (f32, f32)) -> Option<(f32, f32)> {
        let tx = (target.0 * 4.0) as i32;
        let ty = (target.1 * 4.0) as i32;

        if can_visit(tiles, rooms, tx as usize, ty as usize) {
            return Some(target);
        }

        let mut queue = Vec::with_capacity(32 * 4);
        queue.push((tx as i32, ty as i32));

        let mut visited = BitSet::new((self.area.width() * 4 * self.area.height() * 4) as usize);

        while let Some((x, y)) = queue.pop() {
            let idx = (x - self.area.min.x * 4 + (y - self.area.min.y * 4) * self.area.width() * 4) as usize;
            if !self.area.in_bounds(Location::new(x / 4, y / 4))
                || visited.get(idx)
            {
                continue;
            }

            visited.set(idx, true);

            if can_visit(tiles, rooms, x as usize, y as usize) {
                return Some((
                    ((x as f32 + 0.5) / 4.0),
                    ((y as f32 + 0.5) / 4.0),
                ));
            }

            for d in &ALL_DIRECTIONS {
                let (ox, oy) = d.offset();
                let idx = ((x + ox - self.area.min.x * 4) + (y + oy - self.area.min.y * 4) * self.area.width() * 4) as usize;
                if !visited.get(idx) {
                    queue.push((x + ox, y + oy));
                }
            }
        }
        None
    }

    /// Returns whether the scaled (* 4) coordinates collide
    /// with an object
    pub fn collides_at_scaled(&self, x: i32, y: i32) -> bool {
        let lx = x as usize - (self.area.min.x * 4) as usize;
        let ly = y as usize - (self.area.min.y * 4) as usize;
        self.collision_map.get(lx + ly * (self.area.width() as usize * 4))
    }

    /// Returns whether the location is blocked by an object
    pub fn is_blocked(&self, loc: Location) -> bool {
        let lx = loc.x as usize - (self.area.min.x) as usize;
        let ly = loc.y as usize - (self.area.min.y) as usize;
        self.blocked_map.get(lx + ly * self.area.width() as usize)
    }

    /// Returns whether the scaled (* 4) coordinates collide
    /// with an object's placement bound
    pub fn is_placeable_scaled(&self, x: i32, y: i32) -> bool {
        let lx = x as usize - (self.area.min.x * 4) as usize;
        let ly = y as usize - (self.area.min.y * 4) as usize;
        self.placement_map.get(lx + ly * (self.area.width() as usize * 4))
    }

    /// Returns whether the scaled (* 4) coordinates collide
    /// with an object's placement bound checking the virtual
    /// room if any
    pub fn is_virt_placeable_scaled(&self, x: i32, y: i32) -> bool {
        let lx = x as usize - (self.area.min.x * 4) as usize;
        let ly = y as usize - (self.area.min.y * 4) as usize;
        let idx = lx + ly * (self.area.width() as usize * 4);
        if let Some(lvl) = self.building_level.as_ref() {
            lvl.placement_map.get(idx)
        } else {
            self.placement_map.get(idx)
        }
    }

    /// Returns the bounds of the object being placed
    /// or none if no object is being placed
    pub fn get_placement_bounds(&self) -> Option<(f32, f32, f32, f32)> {
        use std::f32;

        let obj_placement = self.building_level.as_ref()
            .and_then(|v| v.object_placement.as_ref())
            .or_else(|| self.temp_placement.as_ref());

        if let Some(obj) = obj_placement {
            let mut has_bound = false;
            let mut min_x: f32 = f32::INFINITY;
            let mut min_y: f32 = f32::INFINITY;
            let mut max_x: f32 = f32::NEG_INFINITY;
            let mut max_y: f32 = f32::NEG_INFINITY;

            if let Some(placement) = obj.placement.as_ref() {
                for action in &placement.actions.0 {
                    if let ObjectPlacementAction::PlacementBound{location, size} = *action {
                        min_x = location.0.min(min_x);
                        min_y = location.1.min(min_y);
                        max_x = (location.0 + size.0).max(max_x);
                        max_y = (location.1 + size.1).max(max_y);
                        has_bound = true;
                    }
                }
            }

            if has_bound {
                Some((min_x, min_y, max_x, max_y))
            } else {
                Some((0.0, 0.0, 0.0, 0.0))
            }
        } else {
            None
        }
    }
}

/// Marks the state of a room.
#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum RoomState {
    /// The room is being planned and may be removed
    Planning,
    /// The room is being built and may be removed
    Building,
    /// The room is done and active
    Done,
}

impl RoomState {
    /// Returns whether the room has been fully built
    pub fn is_done(self) -> bool {
        if let RoomState::Done = self {
            true
        } else {
            false
        }
    }

    /// Returns whether the room is being planned and may be resized
    pub fn is_planning(self) -> bool {
        if let RoomState::Planning{..} = self {
            true
        } else {
            false
        }
    }

    /// Returns whether the room is being built and cannot be resized
    pub fn is_building(self) -> bool {
        if let RoomState::Building{..} = self {
            true
        } else {
            false
        }
    }
}

pub(super) struct LimitedRoom<'a> {
    pub(super) room_id: room::Id,
    pub(super) level: &'a mut super::Level,
}

impl <'a> LevelView for LimitedRoom<'a> {
    fn get_tile(&self, loc: Location) -> Arc<tile::Type> {
        self.level.get_tile(loc)
    }
    fn get_tile_raw(&self, loc: Location) -> Option<TileData> {
        self.level.get_tile_raw(loc)
    }
    fn get_room_owner(&self, loc: Location) -> Option<room::Id> {
        self.level.get_room_owner(loc)
    }
    fn get_tile_flags(&self, loc: Location) -> TileFlag {
        self.level.get_tile_flags(loc)
    }
    fn get_wall_info(&self, loc: Location, dir: Direction) -> Option<WallInfo> {
        self.level.get_wall_info(loc, dir)
    }

    fn get_asset_manager(&self) -> assets::AssetManager {
        self.level.asset_manager.clone()
    }
    fn get_window(&self, id: u8) -> ResourceKey<'static> {
        self.level.get_window(id)
    }
}

impl <'a> LevelAccess for LimitedRoom<'a> {
    fn set_tile(&mut self, loc: Location, tile: assets::ResourceKey<'_>) {
        self.level.set_tile(loc, tile);
    }
    fn set_wall_info(&mut self, loc: Location, dir: Direction, info: Option<WallInfo>) {
        self.level.set_wall_info(loc, dir, info)
    }

    fn set_tile_flags(&mut self, loc: Location, flags: TileFlag) {
        Level::set_tile_flags(self.level, loc, flags);
    }

    fn set_tile_raw(&mut self, loc: Location, tile: TileData) {
        self.level.set_tile_raw(loc, tile);
    }
    fn get_or_create_window(&mut self, key: ResourceKey<'_>) -> u8 {
        self.level.get_or_create_window(key)
    }
}

impl <'a> placement::ObjectPlaceable for LimitedRoom<'a> {
    fn id(&self) -> room::Id {
        self.room_id
    }

    fn bounds(&self) -> Bound {
        let room = self.level.get_room_info(self.room_id);
        room.area
    }

    fn should_lower_walls(&mut self, flag: bool) {
        let mut room = self.level.get_room_info_mut(self.room_id);
        room.lower_walls = flag;
    }

    fn set_placement(&mut self, placement: TempObjectPlacement) {
        let mut room = self.level.get_room_info_mut(self.room_id);
        room.temp_placement = Some(placement);
    }

    fn get_placement_position(&self) -> Option<(f32, f32)> {
        let room = self.level.get_room_info(self.room_id);
        room.temp_placement.as_ref().map(|v| v.position)
    }

    fn take_placement(&mut self) -> Option<TempObjectPlacement> {
        let mut room = self.level.get_room_info_mut(self.room_id);
        room.temp_placement.take()
    }

    fn can_place_object(&self, obj: &ObjectPlacement) -> bool {
        let room = self.level.get_room_info(self.room_id);
        for action in &obj.actions.0 {
            if let ObjectPlacementAction::PlacementBound{location, size} = *action {
                let min_x = (location.0.max(room.area.min.x as f32) * 4.0).floor() as usize;
                let min_y = (location.1.max(room.area.min.y as f32) * 4.0).floor() as usize;
                let max_x = ((location.0 + size.0).min(room.area.max.x as f32 + 1.0) * 4.0).ceil() as usize;
                let max_y = ((location.1 + size.1).min(room.area.max.y as f32 + 1.0) * 4.0).ceil() as usize;
                for y in min_y .. max_y {
                    for x in min_x .. max_x {
                        let lx = x - (room.area.min.x * 4) as usize;
                        let ly = y - (room.area.min.y * 4) as usize;
                        if room.placement_map.get(lx + ly * (room.area.width() as usize * 4)) {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    fn flag_dirty(&mut self) {
        let area = {
            let room = self.level.get_room_info(self.room_id);
            room.area
        };
        for loc in area {
            self.level.flag_dirty(loc.x, loc.y);
        }
    }

    fn rebuild_placement_map(&mut self) {
        let mut room = self.level.get_room_info_mut(self.room_id);
        room.rebuild_placement_map();
    }

    fn place_object(&mut self, placement: TempObjectPlacement) -> UResult<usize> {
        let ret;
        let area = {
            let log = self.level.log.clone();
            let mut room = self.level.get_room_info_mut(self.room_id);
            let id = room.objects.iter().position(|v| v.is_none());
            let rev = placement.placement_rev.expect("Missing placement");
            let pm = Some((placement.placement.expect("Missing placement"), rev));
            if let Some(id) = id {
                *assume!(log, room.objects.get_mut(id)) = pm;
                ret = Ok(id)
            } else {
                let id = room.objects.len();
                room.objects.push(pm);
                ret = Ok(id)
            }
            room.rebuild_object_maps();
            room.area
        };
        self.level.rebuild_path_sections(area);
        ret
    }

    fn is_virtual() -> bool { false }

    fn remove_object<EC: EntityCreator>(&mut self, entities: &mut Container, object_id: usize) -> UResult<ObjectPlacement> {
        self.level.remove_object::<EC>(entities, self.room_id, object_id)
    }
    fn replace_object<EC: EntityCreator>(&mut self, entities: &mut Container, object_id: usize, obj: ObjectPlacement) {
        self.level.replace_object::<EC>(entities, self.room_id, object_id, obj)
    }
    fn get_remove_target(&self, loc: Location) -> Option<usize> {
        let room = self.level.get_room_info(self.room_id);
        placement::find_remove_target(&room.objects, loc)
    }
}

// Raw json structs

fn returns_one() -> usize { 1 }

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum RequirementInfo {
    #[serde(rename = "room")]
    Room {
        key: String,
        #[serde(default = "returns_one")]
        count: usize,
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct RoomInfo {
    name: String,
    min_size: Size,
    tile: String,
    border_tile: Option<String>,
    tile_placer: Option<String>,
    tile_updater: Option<String>,
    #[serde(default)]
    wall: Option<RoomWallsInfo>,
    #[serde(default)]
    placement_match: Vec<String>,
    #[serde(default)]
    only_within: Option<String>,
    #[serde(default)]
    valid_objects: Vec<String>,
    #[serde(default)]
    required_objects: FNVMap<String, i32>,
    #[serde(default)]
    required_entities: FNVMap<String, i32>,
    #[serde(default)]
    build_anywhere: bool,
    #[serde(default)]
    requirements: Vec<RequirementInfo>,
    controller: Option<String>,
    #[serde(default = "return_true")]
    allow_edit: bool,
    #[serde(default = "return_true")]
    allow_limited_edit: bool,
    base_cost: Option<UniDollar>,
    cost_per_tile: Option<UniDollar>,
    #[serde(default)]
    can_idle: bool,
    #[serde(default)]
    used_for_teaching: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct RoomWallsInfo {
    texture: String,
    texture_top: Option<String>,
    #[serde(default)]
    priority: bool,
}

fn return_true() -> bool { true }

#[derive(Debug, Serialize, Deserialize)]
struct Size {
    x: i32,
    y: i32,
}

impl IntKey for Id {
    #[inline]
    fn should_resize(offset: isize, len: usize, new: Self) -> Option<(isize, usize, bool)> {
        let new = new.0 as isize;
        if new + offset >= 0 && new + offset < len as isize {
            None
        } else if new + offset < 0 {
            let diff = (new + offset).abs();
            Some((new.abs(), len + diff as usize, true))
        } else {
            Some((offset, (new + offset + 1) as usize, false))
        }
    }

    #[inline]
    fn index(offset: isize, key: Self) -> usize {
        (key.0 as isize + offset) as usize
    }

    #[inline]
    fn to_key(offset: isize, index: usize) -> Self {
        Id((index as isize - offset) as i16)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::env;
    use crate::assets::*;

    #[test]
    fn try_load_rooms() {
        let exe = env::current_exe().unwrap();
        let parent = exe.parent().unwrap();
        env::set_current_dir(parent.join("../../../")).unwrap();
        let log = ::slog::Logger::root(::slog::Discard, o!());
        let assets = AssetManager::with_packs(&log, &["base".to_owned()])
            .register::<super::Loader>()
            .build();
        load_dir(&assets, Path::new("./assets/base/base/rooms"));
    }

    fn load_dir(assets: &AssetManager, dir: &Path) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                load_dir(assets, &path);
            } else {
                let path_str = path.to_string_lossy();
                let path_str = &path_str["./assets/base/base/rooms/".len()..];
                let path_str = &path_str[..path_str.len() - 5];

                // Not a room file, just a list of them
                if path_str == "rooms" {
                    continue;
                }
                println!("Trying to load: {:?}", path_str);
                assets.loader_open::<super::Loader>(ResourceKey::new("base", path_str)).unwrap();
            }
        }
    }
}