//! Contains everything to do with levels in the game.

pub mod tile;
pub mod object;
pub mod room;

mod script_helper;
pub use self::script_helper::init_levellib;
pub use self::script_helper::get_rooms_for_player;
pub use self::object::{
    ObjectPlacement,
    ReverseObjectPlacement,
    ObjectPlacementAction,
    Loc2D,
    Loc3D,
    Size2D,
};
#[macro_use]
mod macros;
mod virt;
pub use self::virt::RoomVirtualLevel;
pub use self::room::{RoomPlacement, RoomState};
mod room_placement;
mod placement;
use self::placement::*;

use crate::prelude::*;

use std::cell::{RefCell, Ref, RefMut};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use delta_encode::bitio;
use crate::network::packet;
use lua;

/// Size of a section in a level. Sections are areas of a level split
/// up for performance (culling, rendering etc).
pub const SECTION_SIZE: usize = 16;

/// Level contains all information of the level currently being played.
/// e.g. layout of the level.
pub struct Level {
    /// The level's logger
    pub log: Logger,
    /// Width of the level in tiles
    pub width: u32,
    /// Height of the level in tiles
    pub height: u32,
    /// Used for testing whether a location is
    /// within the level easily
    pub level_bounds: Bound,
    /// The asset manager in use by this level.
    pub asset_manager: AssetManager,
    /// Sharable storage for the level's tiles
    pub tiles: Rc<RefCell<LevelTiles>>,
    /// Sharable storage for the level's rooms
    pub rooms: Rc<RefCell<LevelRooms>>,
    /// Whether to compute path data
    ///
    /// Useful as an optimization when loading
    pub compute_path_data: bool,
}

struct PathSection {
    info: [u64; 15 * 4],
    movement_cost: i32,
}

bitflags! {
    /// A set of flags that can be set on a tile
    #[derive(Serialize, Deserialize, DeltaEncode)]
    pub struct TileFlag: u8 {
        /// Marks the tile as being built on
        const BUILDING = 0b0000_0001;
        /// Marks the tile as having no walls
        ///
        /// This disables the normal wall placement checks
        /// from this tile but not to this tile
        const NO_WALLS = 0b0000_0010;
    }
}

/// Information about a single tile in the level.
#[derive(Clone, Copy, Debug)]
pub struct TileData {
    /// The id of the tile at this location
    id: tile::TileId,
    /// The id room that owns this tile
    owner: Option<RoomId>,
    /// Flags set on the tile
    flags: TileFlag,
}

/// Stores information about the wall west and south of the tile location
#[derive(Clone, Copy, Debug)]
struct WallData {
    data: [Option<WallInfo>; 2],
}

/// Information about a wall
#[derive(Clone, Copy, Debug)]
pub struct WallInfo {
    /// Flag set to modify the look of the wall and
    /// the way the wall is interacted with.
    pub flag: TileWallFlag,
}

/// Used to mark a wall on a tile with a special property
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TileWallFlag {
    /// The tile wall is default
    None,
    /// The tile wall has a window
    Window(u8),
    /// The tile wall is a door and should be hidden
    Door,
}

/// A view on to a level.
///
/// The level does not have to be the real level and may just
/// a copy or a possible future view of the level.
///
/// The may also return dummy data for areas outside its bounds.
pub trait LevelView {
    /// Gets the tile at the passed location. If the location is out of bounds then
    /// a default tile is returned.
    fn get_tile(&self, loc: Location) -> Arc<tile::Type>;
    /// Gets the raw tile at the passed location.
    ///
    /// Returns None if out of bounds
    fn get_tile_raw(&self, loc: Location) -> Option<TileData>;
    /// Returns the id of the owner of the tile. None is returned if
    /// no room owns the tile.
    fn get_room_owner(&self, loc: Location) -> Option<RoomId>;
    /// Returns the flags set on the tile
    fn get_tile_flags(&self, loc: Location) -> TileFlag;
    /// Returns the information about a wall at a location + direction if it exists
    /// otherwise it returns none.
    fn get_wall_info(&self, loc: Location, dir: Direction) -> Option<WallInfo>;

    /// Returns a reference to the asset manager currently in
    /// use by this level.
    fn get_asset_manager(&self) -> AssetManager;

    /// Returns the type of window as given by the id
    fn get_window(&self, id: u8) -> ResourceKey<'static>;
}

impl <'a, T> LevelView for Ref<'a, T> where T: LevelView {
    fn get_tile(&self, loc: Location) -> Arc<tile::Type> {
        T::get_tile(self, loc)
    }
    fn get_tile_raw(&self, loc: Location) -> Option<TileData> {
        T::get_tile_raw(self, loc)
    }
    fn get_room_owner(&self, loc: Location) -> Option<RoomId> {
        T::get_room_owner(self, loc)
    }
    fn get_tile_flags(&self, loc: Location) -> TileFlag {
        T::get_tile_flags(self, loc)
    }
    fn get_wall_info(&self, loc: Location, dir: Direction) -> Option<WallInfo> {
        T::get_wall_info(self, loc, dir)
    }
    fn get_asset_manager(&self) -> AssetManager {
        T::get_asset_manager(self)
    }
    fn get_window(&self, id: u8) -> ResourceKey<'static> {
        T::get_window(self, id)
    }
}

impl <T> LevelView for Box<T> where T: LevelView {
    fn get_tile(&self, loc: Location) -> Arc<tile::Type> {
        T::get_tile(self, loc)
    }
    fn get_tile_raw(&self, loc: Location) -> Option<TileData> {
        T::get_tile_raw(self, loc)
    }
    fn get_room_owner(&self, loc: Location) -> Option<RoomId> {
        T::get_room_owner(self, loc)
    }
    fn get_tile_flags(&self, loc: Location) -> TileFlag {
        T::get_tile_flags(self, loc)
    }
    fn get_wall_info(&self, loc: Location, dir: Direction) -> Option<WallInfo> {
        T::get_wall_info(self, loc, dir)
    }
    fn get_asset_manager(&self) -> AssetManager {
        T::get_asset_manager(self)
    }
    fn get_window(&self, id: u8) -> ResourceKey<'static> {
        T::get_window(self, id)
    }
}

/// Provides write access to a level.
///
/// The level does not have to be the real level and may just
/// a copy or a possible future view of the level.
///
/// The may also ignore writes to areas outside its bounds
pub trait LevelAccess: LevelView {
    /// Sets the tile at the passed location to the provided `TileData`. If
    /// the location is out of bounds then it is ignored.
    fn set_tile(&mut self, loc: Location, tile: ResourceKey<'_>);
    /// Sets the raw tile at the passed location.
    fn set_tile_raw(&mut self, loc: Location, tile: TileData);
    /// Sets the flags set on the tile
    fn set_tile_flags(&mut self, loc: Location, flags: TileFlag);
    /// Sets the information about a wall at a location + direction.
    fn set_wall_info(&mut self, loc: Location, dir: Direction, info: Option<WallInfo>);

    /// Returns the id for the window type, creating if needed
    fn get_or_create_window(&mut self, key: ResourceKey<'_>) -> u8;
}

impl <T> LevelAccess for Box<T> where T: LevelAccess {
    fn set_tile(&mut self, loc: Location, tile: ResourceKey<'_>) {
        T::set_tile(self, loc, tile)
    }
    fn set_tile_raw(&mut self, loc: Location, tile: TileData) {
        T::set_tile_raw(self, loc, tile)
    }
    fn set_tile_flags(&mut self, loc: Location, flags: TileFlag) {
        T::set_tile_flags(self, loc, flags)
    }
    fn set_wall_info(&mut self, loc: Location, dir: Direction, info: Option<WallInfo>) {
        T::set_wall_info(self, loc, dir, info)
    }
    fn get_or_create_window(&mut self, key: ResourceKey<'_>) -> u8 {
        T::get_or_create_window(self, key)
    }
}

impl LevelView for Level {
    fn get_tile(&self, loc: Location) -> Arc<tile::Type> {
        self.tiles.borrow().get_tile(loc)
    }
    fn get_tile_raw(&self, loc: Location) -> Option<TileData> {
        self.tiles.borrow().get_tile_raw(loc)
    }
    fn get_room_owner(&self, loc: Location) -> Option<RoomId> {
        self.tiles.borrow().get_room_owner(loc)
    }
    fn get_tile_flags(&self, loc: Location) -> TileFlag {
        self.tiles.borrow().get_tile_flags(loc)
    }
    fn get_wall_info(&self, loc: Location, dir: Direction) -> Option<WallInfo> {
        self.tiles.borrow().get_wall_info(loc, dir)
    }

    fn get_asset_manager(&self) -> AssetManager {
        self.asset_manager.clone()
    }

    fn get_window(&self, id: u8) -> ResourceKey<'static> {
        let tiles = self.tiles.borrow();
        tiles.get_window(id).clone()
    }
}

impl LevelAccess for Level {
    fn set_tile(&mut self, loc: Location, tile: ResourceKey<'_>) {
        self.tiles.borrow_mut().set_tile(loc, tile)
    }

    fn set_wall_info(&mut self, loc: Location, dir: Direction, info: Option<WallInfo>) {
        self.tiles.borrow_mut().set_wall_info(loc, dir, info)
    }

    fn set_tile_flags(&mut self, loc: Location, flags: TileFlag) {
        self.tiles.borrow_mut().set_tile_flags(loc, flags)
    }

    fn set_tile_raw(&mut self, loc: Location, tile: TileData) {
        self.tiles.borrow_mut().set_tile_raw(loc, tile)
    }

    fn get_or_create_window(&mut self, key: ResourceKey<'_>) -> u8 {
        self.tiles.borrow_mut().get_or_create_window(key)
    }
}

impl Level {
    /// Creates a new level.
    pub fn new<EC, E>(
            log: Logger,
            engine: &E,
            asset_manager: &AssetManager,
            entities: &mut Container,
            players: &[PlayerId], player_area: u32
    ) -> UResult<Level>
        where E: Invokable,
              EC: EntityCreator
    {
        let def_id = asset_manager.loader_open::<tile::Loader>(ResourceKey::new("base", "grass"))?.id;

        let width = (players.len() as f64).sqrt().ceil() as u32;
        let height = (players.len() as f64 / f64::from(width)).ceil() as u32;
        // 16 padding around the edges for each player
        // plus an extra 32 (64 for both sides) for camera clamping
        let size_width = 32 * 2 + 16 + (16 + player_area) * width;
        let size_height = 32 * 2 + 16 + (16 + player_area) * height;

        let mut level = Level::new_raw_internal(log.clone(), asset_manager, engine, size_width, size_height, def_id);

        // Initial roads
        let road = ResourceKey::new("base", "external/road");
        let server_player = PlayerId(-1337);
        let tmp_room_id = RoomId(-1);
        {
            // Corners
            for x in 0 ..= width {
                for y in 0 ..= height {
                    let loc = Location::new(
                        32 + (16 + player_area as i32) * x as i32,
                        32 + (16 + player_area as i32) * y as i32,
                    );
                    let loc_max = loc + (15, 15);
                    let bound = Bound::new(loc, loc_max);
                    let id = assume!(log, level.place_room_id::<EC, _>(engine, entities, tmp_room_id, server_player, road.borrow(), bound));
                    let id = level.finalize_placement(id);
                    level.finalize_room::<EC, _>(engine, entities, id)?;
                }
            }
            // Edges
            for x in 0 .. width {
                let loc = Location::new(
                    32 + 16 + (16 + player_area as i32) * x as i32,
                    32,
                );
                let bound = Bound::new(loc, loc + (player_area as i32 - 1, 15));
                let id = assume!(log, level.place_room_id::<EC, _>(engine, entities, tmp_room_id, server_player, road.borrow(), bound));
                let id = level.finalize_placement(id);
                level.finalize_room::<EC, _>(engine, entities, id)?;
            }
            for y in 0 .. height {
                let loc = Location::new(
                    32,
                    32 + 16 + (16 + player_area as i32) * y as i32,
                );
                let bound = Bound::new(loc, loc + (15, player_area as i32 - 1));
                let id = assume!(log, level.place_room_id::<EC, _>(engine, entities, tmp_room_id, server_player, road.borrow(), bound));
                let id = level.finalize_placement(id);
                level.finalize_room::<EC, _>(engine, entities, id)?;
            }
        }

        // Places joining roads
        for x in 0 .. width {
            for y in 0 .. height {
                let x = x as i32;
                let y = y as i32;
                let loc = Location::new(32 + 16 + x * (16 + player_area as i32), 32 + 16 + y * (16 + player_area as i32));
                let pos = loc + (player_area as i32, player_area as i32);
                // Edges
                let bound = Bound::new(pos - (player_area as i32, 0), pos + (0, 15));
                let id = assume!(log, level.place_room_id::<EC, _>(engine, entities, tmp_room_id, server_player, road.borrow(), bound));
                let id = level.finalize_placement(id);
                level.finalize_room::<EC, _>(engine, entities, id)?;
                let bound = Bound::new(pos - (0, player_area as i32), pos + (15, 0));
                let id = assume!(log, level.place_room_id::<EC, _>(engine, entities, tmp_room_id, server_player, road.borrow(), bound));
                let id = level.finalize_placement(id);
                level.finalize_room::<EC, _>(engine, entities, id)?;
            }
        }
        for x in 0 ..= width {
            let x = x as i32;

            let pos = Location::new(32 + x * (16 + player_area as i32), 0);
            let bound = Bound::new(
                pos,
                pos + (15, 31)
            );
            let id = assume!(log, level.place_room_id::<EC, _>(engine, entities, tmp_room_id, server_player, road.borrow(), bound));
            let id = level.finalize_placement(id);
            level.finalize_room::<EC, _>(engine, entities, id)?;


            let pos = Location::new(32 + x * (16 + player_area as i32), 32 + 16 + (16 + player_area as i32) * height as i32);
            let bound = Bound::new(
                pos,
                pos + (15, 31)
            );
            let id = assume!(log, level.place_room_id::<EC, _>(engine, entities, tmp_room_id, server_player, road.borrow(), bound));
            let id = level.finalize_placement(id);
            level.finalize_room::<EC, _>(engine, entities, id)?;
        }
        for y in 0 ..= height {
            let y = y as i32;

            let pos = Location::new(0, 32 + y * (16 + player_area as i32));
            let bound = Bound::new(
                pos,
                pos + (31, 15)
            );
            let id = assume!(log, level.place_room_id::<EC, _>(engine, entities, tmp_room_id, server_player, road.borrow(), bound));
            let id = level.finalize_placement(id);
            level.finalize_room::<EC, _>(engine, entities, id)?;


            let pos = Location::new(32 + 16 + (16 + player_area as i32) * width as i32, 32 + y * (16 + player_area as i32));
            let bound = Bound::new(
                pos,
                pos + (31, 15)
            );
            let id = assume!(log, level.place_room_id::<EC, _>(engine, entities, tmp_room_id, server_player, road.borrow(), bound));
            let id = level.finalize_placement(id);
            level.finalize_room::<EC, _>(engine, entities, id)?;
        }

        // Place zones
        let mut ox = 0;
        let mut oy = 0;
        let owned_area = ResourceKey::new("base", "owned_area");
        let building = ResourceKey::new("base", "building");
        let door = ResourceKey::new("base", "doors/basic");
        let tree_plant = ResourceKey::new("base", "plants/small_tree");
        let window = ResourceKey::new("base", "window");

        let building_size = ::std::cmp::min(26, player_area / 4) as i32;

        for player in players {
            // Placement area for the player to use
            let loc = Location::new(32 + 16 + ox * (16 + player_area as i32), 32 + 16 + oy * (16 + player_area as i32));
            let id = assume!(log, level.place_room_id::<EC, _>(
                engine, entities,
                tmp_room_id, *player,
                owned_area.borrow(),
                Bound::new(
                    loc,
                    loc + (player_area as i32 - 1, player_area as i32 - 1)
                )
            ));
            let id = level.finalize_placement(id);
            level.finalize_room::<EC, _>(engine, entities, id)?;

            // Initial building for them to work with
            let bloc = loc + (
                0,
                player_area as i32 / 2 - building_size / 2,
            );
            let id = assume!(log, level.place_room_id::<EC, _>(
                engine, entities,
                tmp_room_id, *player,
                building.borrow(),
                Bound::new(
                    bloc,
                    bloc + (building_size, building_size)
                )
            ));
            let id = level.finalize_placement(id);
            place_objects! {
                init(level, engine, entities)
                room(id at bloc) {
                    place door at (0.5, building_size as f32 / 2.0)
                    place window at (0.5, (building_size as f32 / 2.0) + 1.0)
                    place window at (0.5, (building_size as f32 / 2.0) - 1.0)
                    place tree_plant at (0.5, (building_size as f32 / 2.0) - 1.0)
                    place tree_plant at (0.5, (building_size as f32 / 2.0) + 1.0)
                }
            }
            level.finalize_room::<EC, _>(engine, entities, id)?;

            ox += 1;
            if ox >= width as i32 {
                ox = 0;
                oy += 1;
            }
        }

        Ok(level)
    }

    /// Creates a new empty level server. This is normally used by clients as
    /// the server normally loads the level for them.
    pub fn new_raw<E>(log: Logger, asset_manager: &AssetManager, scripting: &E, width: u32, height: u32) -> UResult<Level>
        where E: Invokable
    {
        let def_id = asset_manager.loader_open::<tile::Loader>(ResourceKey::new("base", "grass"))?.id;
        Ok(Level::new_raw_internal(log, asset_manager, scripting, width, height, def_id))
    }

    fn new_raw_internal<E>(
        log: Logger,
        asset_manager: &AssetManager,
        scripting: &E,
        width: u32, height: u32,
        def_id: tile::TileId) -> Level
    where E: Invokable
    {
        let sw = (width as usize + (SECTION_SIZE - 1)) / SECTION_SIZE;
        let sh = (height as usize + (SECTION_SIZE - 1)) / SECTION_SIZE;
        let mut tile_map = IntMap::new();
        tile_map.insert(def_id, assume!(log, asset_manager.loader_open::<tile::ById>(def_id)));
        let level_bounds = Bound::new(
            Location::zero(),
            Location::new(width as i32 - 1, height as i32 - 1)
        );
        let lvl = Level {
            log: log.clone(),
            width,
            height,
            level_bounds,
            compute_path_data: true,
            tiles: Rc::new(RefCell::new(LevelTiles {
                log: log.clone(),
                width,
                height,
                level_bounds,
                asset_manager: asset_manager.clone(),
                tiles: vec![TileData{
                    id: def_id,
                    owner: None,
                    flags: TileFlag::empty(),
                }; (width * height) as usize],
                tile_map,
                walls: vec![WallData{
                    data: [None; 2],
                }; ((width + 1) * (height + 1)) as usize],
                dirty_sections: vec![true; sw * sh],
                window_type: vec![],
                window_type_map: FNVMap::default(),
                pathmap: {
                    let size = (((width + 3) / 4) * ((height + 3) / 4)) as usize;
                    let mut pathmap = Vec::with_capacity(size);
                    for _ in 0 .. size {
                        pathmap.push(PathSection {
                            info: [0xFFFF_FFFF_FFFF_FFFF; 15 * 4],
                            movement_cost: 0,
                        });
                    }
                    pathmap
                },
            })),
            rooms: Rc::new(RefCell::new(LevelRooms {
                log: log.clone(),
                width,
                height,
                level_bounds,
                rooms: IntMap::new(),
                room_order: vec![],
            })),
            asset_manager: asset_manager.clone(),
        };

        scripting.store_tracked::<LevelRooms>(Rc::downgrade(&lvl.rooms));
        scripting.store_tracked::<LevelTiles>(Rc::downgrade(&lvl.tiles));

        lvl
    }

    fn recompute_path_section(&mut self, sx: usize, sy: usize) {
        let mut visited = BitSet::new(16 * 16);
        let mut tiles = self.tiles.borrow_mut();
        let rooms = self.rooms.borrow();
        let idx = sx + sy * ((self.width as usize + 3) / 4);
        {
            let section = &mut tiles.pathmap[idx];
            // Clear existing data
            for val in section.info.iter_mut() {
                *val = 0;
            }
            section.movement_cost = 0;
        }

        let min_x = sx * 16;
        let min_y = sy * 16;

        {
            let mut cost = 0;
            for y in sy * 4 .. (sy * 4 + 4) {
                for x in sx * 4 .. (sx * 4 + 4) {
                    cost += tiles.get_tile(Location::new(x as i32, y as i32)).movement_cost;
                }
            }
            let section = &mut tiles.pathmap[idx];
            section.movement_cost = cost;
        }

        for y in min_y .. min_y + 16 {
            for x in min_x .. min_x + 16 {
                let cx = x & 0xF;
                let cy = y & 0xF;
                if visited.get(cx + cy * 16) || !self.level_bounds.in_bounds(Location::new(x as i32 / 4, y as i32 / 4)) {
                    continue;
                }

                if !can_visit(&*tiles, &*rooms, x, y) {
                    visited.set(cx + cy * 16, true);
                    continue;
                }

                let touched = flood_fill(&mut visited, &*tiles, &*rooms, x, y, sx, sy);
                let section = &mut tiles.pathmap[idx];
                for e1 in 0 ..= 15 * 4 {
                    if touched & (1 << e1) != 0 {
                        let edge = &mut section.info[e1];
                        for e2 in 0 ..= 15 * 4 {
                            if touched & (1 << e2) != 0 {
                                *edge |= 1 << e2;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Returns a collection of room ids
    pub fn room_ids(&self) -> Vec<RoomId> {
        let rooms = self.rooms.borrow();
        rooms.room_ids().collect()
    }

    /// Sets the tile at the passed location to the provided `TileData`. If
    /// the location is out of bounds then it is ignored.
    pub fn set_tile(&mut self, loc: Location, tile: ResourceKey<'_>) {
        self.tiles.borrow_mut().set_tile(loc, tile)
    }

    /// Flags the section at the location as dirty
    pub fn flag_dirty(&mut self, x: i32, y: i32) {
        self.tiles.borrow_mut()
            .flag_dirty(x, y)
    }

    /// Gets the tile at the passed location. If the location is out of bounds then
    /// a default tile is returned.
    pub fn get_tile(&self, loc: Location) -> Arc<tile::Type> {
        self.tiles.borrow().get_tile(loc)
    }

    /// Returns the id of the owner of the tile. None is returned if
    /// no room owns the tile.
    pub fn get_room_owner(&self, loc: Location) -> Option<RoomId> {
        self.tiles.borrow().get_room_owner(loc)
    }

    /// Returns the flags set on the tile
    pub fn get_tile_flags(&self, loc: Location) -> TileFlag {
        self.tiles.borrow().get_tile_flags(loc)
    }

    /// Sets the flags set on the tile
    pub fn set_tile_flags(&mut self, loc: Location, flags: TileFlag) {
        self.tiles.borrow_mut().set_tile_flags(loc, flags)
    }

    /// Returns the information about a wall at a location + direction if it exists
    /// otherwise it returns none.
    pub fn get_wall_info(&self, loc: Location, dir: Direction) -> Option<WallInfo> {
        self.tiles.borrow().get_wall_info(loc, dir)
    }

    /// Sets the information about a wall at a location + direction.
    pub fn set_wall_info(&mut self, loc: Location, dir: Direction, info: Option<WallInfo>) {
        self.tiles.borrow_mut().set_wall_info(loc, dir, info)
    }

    /// Returns whether a room with the given id exists
    pub fn room_exists(&self, id: RoomId) -> bool {
        self.rooms.borrow().room_exists(id)
    }

    /// Returns the placement information of the room with the given
    /// id.
    pub fn try_room_info(&self, id: RoomId) -> Option<Ref<'_, RoomPlacement>> {
        let rooms = self.rooms.borrow();
        ref_filter_map(rooms, |v| v.try_room_info(id))

    }
    /// Returns the placement information of the room with the given
    /// id.
    pub fn try_room_info_mut(&mut self, id: RoomId) -> Option<RefMut<'_, RoomPlacement>> {
        let rooms = self.rooms.borrow_mut();
        ref_mut_filter_map(rooms, |v| v.try_room_info_mut(id))
    }

    /// Returns the placement information of the room with the given
    /// id.
    pub fn get_room_info(&self, id: RoomId) -> Ref<'_, RoomPlacement> {
        Ref::map(self.rooms.borrow(), |v| v.get_room_info(id))
    }

    /// Returns the placement information of the room with the given
    /// id.
    pub fn get_room_info_mut(&mut self, id: RoomId) -> RefMut<'_, RoomPlacement> {
        RefMut::map(self.rooms.borrow_mut(), |v| v.get_room_info_mut(id))
    }

    /// Returns the room placement information for the room at the location
    /// if any, otherwise this returns `None`.
    pub fn get_room_at<'a>(tiles: &LevelTiles, rooms: &'a LevelRooms, loc: Location) -> Option<&'a RoomPlacement> {
        tiles.get_tile_raw(loc)
            .and_then(|v| v.owner)
            .map(|v| rooms.get_room_info(v))
    }

    /// Returns whether the section at the location is dirty and clears the flag
    /// if it is. Used by the renderer and shouldn't be used by anything else.
    pub fn get_and_clear_dirty_section(&mut self, x: usize, y: usize) -> bool {
        self.tiles.borrow_mut().get_and_clear_dirty_section(x, y)
    }

    fn virt_placer(log: &Logger, rooms: &Rc<RefCell<LevelRooms>>, id: RoomId) -> virt::VirtualPlacer {
        virt::VirtualPlacer {
            rooms: rooms.clone(),
            log: log.clone(),
            id,
        }
    }

    /// Starts trying to place the named object in the given room
    pub fn begin_object_placement<'a, E, EC>(
            &mut self, room: RoomId,
            engine: &E,
            entities: &mut Container,
            obj: ResourceKey<'a>,
            version: Option<i32>,
    ) -> UResult<()>
        where E: Invokable,
              EC: EntityCreator,
    {
        let limited = {
            let (building, limited) = {
                let rooms = self.rooms.borrow();
                let room = rooms.get_room_info(room);
                (room.building_level.is_some(), room.limited_editing)
            };
            if building {
                return Level::virt_placer(&self.log, &self.rooms, room).begin_object_placement::<E, EC>(
                    &self.log,
                    &self.asset_manager,
                    engine,
                    entities, obj,
                    version
                )
            }
            limited
        };
        // If it has a version then its an existing object
        if limited || version.is_some() {
            let assets = self.asset_manager.clone();
            let log = self.log.clone();
            room::LimitedRoom {
                room_id: room,
                level: self,
            }.begin_object_placement::<E, EC>(
                &log,
                &assets,
                engine,
                entities, obj,
                version
            )
        } else {
            Err(ErrorKind::InvalidRoomState.into())
        }
    }

    /// Moves the active object to new location. If the location is not
    /// valid this returns an error but the object will still be moved
    /// it'll just be unplaceable
    pub fn move_active_object<E, EC>(
            &mut self, room: RoomId,
            engine: &E,
            entities: &mut Container,
            pos: (f32, f32),
            version: Option<i32>,
            rotation: i16,
    ) -> UResult<()>
        where E: Invokable,
              EC: EntityCreator,
    {
        let limited = {
            let (building, limited) = {
                let rooms = self.rooms.borrow();
                let room = rooms.get_room_info(room);
                (room.building_level.is_some(), room.limited_editing)
            };
            if building {
                return Level::virt_placer(&self.log, &self.rooms, room).move_active_object::<E, EC>(
                    &self.log,
                    &self.asset_manager,
                    engine,
                    entities, pos,
                    version, rotation
                )
            }
            limited
        };
        if limited || version.is_some() {
            let assets = self.asset_manager.clone();
            let log = self.log.clone();
            room::LimitedRoom {
                room_id: room,
                level: self,
            }.move_active_object::<E, EC>(
                &log,
                &assets,
                engine,
                entities, pos,
                version, rotation
            )
        } else {
            Err(ErrorKind::InvalidRoomState.into())
        }
    }

    /// Cancels placement of the active object
    pub fn cancel_object_placement<EC>(&mut self, room: RoomId, entities: &mut Container)
        where EC: EntityCreator,
    {
        {
            let building = {
                let rooms = self.rooms.borrow();
                let room = rooms.get_room_info(room);
                room.building_level.is_some()
            };
            if building {
                return Level::virt_placer(&self.log, &self.rooms, room).cancel_object_placement::<EC>(&self.log, entities)
            }
        };
        let log = self.log.clone();
        room::LimitedRoom {
            room_id: room,
            level: self,
        }.cancel_object_placement::<EC>(&log, entities);
    }

    /// Places the active object if its in a valid location otherwise
    /// returns an error and does nothing
    pub fn finalize_object_placement<E, EC>(
            &mut self, room: RoomId,
            engine: &E,
            entities: &mut Container,
            version: Option<i32>,
            rotation: i16,
    ) -> UResult<usize>
        where E: Invokable,
              EC: EntityCreator,
    {
        let limited = {
            let (building, limited) = {
                let rooms = self.rooms.borrow();
                let room = rooms.get_room_info(room);
                (room.building_level.is_some(), room.limited_editing)
            };
            if building {
                return Level::virt_placer(&self.log, &self.rooms, room).finalize_object_placement::<E, EC>(
                    &self.log,
                    &self.asset_manager,
                    engine,
                    entities,
                    version,
                    rotation
                )
            }
            limited
        };
        if limited || version.is_some() {
            let assets = self.asset_manager.clone();
            let log = self.log.clone();
            room::LimitedRoom {
                room_id: room,
                level: self,
            }.finalize_object_placement::<E, EC>(
                &log,
                &assets,
                engine,
                entities,
                version,
                rotation
            )
        } else {
            Err(ErrorKind::InvalidRoomState.into())
        }
    }

    fn try_remove_object<L, EC>(
        log: &Logger,
        lvl: &mut L,
        objects: &mut [Option<(ObjectPlacement, ReverseObjectPlacement)>],
        entities: &mut Container, room_id: RoomId,
        object_id: usize,
    ) -> UResult<ObjectPlacement>
        where EC: EntityCreator,
              L: placement::ObjectPlaceable
    {
        if object_id >= objects.len() || objects[object_id].is_none() {
            bail!("Invalid object ID");
        }
        // Remove the objects after this one as they
        // may depend on it
        let mut replacement_list = Vec::with_capacity(objects.len() - (object_id + 1));
        for obj in objects[object_id + 1 ..].iter_mut().rev() {
            let obj = obj.take();
            if let Some(obj) = obj {
                replacement_list.push(Some(obj.0));
                obj.1.apply::<_, EC>(log, lvl, entities)
                    // Removing these objects shouldn't fail
                    .expect("Failed to remove object");
            } else {
                replacement_list.push(None);
            }
        }
        replacement_list.reverse();

        // Remove the object that was requested
        let obj = objects[object_id]
            .take()
            .expect("Object missing");
        obj.1.apply::<_, EC>(log, lvl, entities)
            .expect("Failed to remove object");
        lvl.rebuild_placement_map();

        // Try and place everything back
        let mut failed_at = None;
        let mut unapplied = Vec::new();
        for (idx, obj) in replacement_list.into_iter().enumerate() {
            if let Some(obj) = obj {
                let rev = if let Ok(rev) = obj.apply::<_, EC>(log, lvl, entities, room_id, false) {
                    rev
                } else {
                    failed_at = Some(idx);
                    unapplied.push(Some(obj));
                    continue;
                };
                objects[object_id + idx] = Some((obj, rev));
            } else if failed_at.is_some() {
                unapplied.push(None);
            }
        }

        // If failed remove everything and put it back to normal
        if let Some(failed_at) = failed_at {
            unapplied.reverse();
            for obj in objects[object_id..object_id + failed_at].iter_mut().rev() {
                let obj = obj.take();
                if let Some(obj) = obj {
                    unapplied.push(Some(obj.0));
                    obj.1.apply::<_, EC>(log, lvl, entities)
                        // Removing these objects shouldn't fail
                        .expect("Failed to remove object");
                } else {
                    unapplied.push(None);
                }
            }
            unapplied.push(Some(obj.0));
            unapplied.reverse();
            for (idx, obj) in unapplied.into_iter().enumerate() {
                if let Some(obj) = obj {
                    let rev = obj.apply::<_, EC>(log, lvl, entities, room_id, false)
                        .expect("Failed to replace object");
                    objects[object_id + idx] = Some((obj, rev));
                }
            }
            bail!("Object is blocked");
        } else {
            return Ok(obj.0);
        }
    }

    /// Removes the object with the given id from the room
    pub fn remove_object<EC: EntityCreator>(&mut self, entities: &mut Container, room_id: RoomId, object_id: usize) -> UResult<ObjectPlacement> {
        use std::mem;
        let log = self.log.clone();
        let (mut objects, area) = {
            let (building, area, mut objects) = {
                let mut rooms = self.rooms.borrow_mut();
                let room = rooms.get_room_info_mut(room_id);
                let objects = room.building_level.as_mut()
                    .map(|v| mem::replace(&mut v.objects, Vec::new()))
                    .unwrap_or_else(|| mem::replace(&mut room.objects, Vec::new()));
                (room.building_level.is_some(), room.area, objects)
            };
            if building {
                let ret = Self::try_remove_object::<_, EC>(
                    &log,
                    &mut Level::virt_placer(&log, &self.rooms, room_id), &mut objects,
                    entities, room_id,
                    object_id
                );
                let mut rooms = self.rooms.borrow_mut();
                let room = rooms.get_room_info_mut(room_id);
                let lvl = assume!(log, room.building_level.as_mut());
                lvl.objects = objects;
                lvl.rebuild_placement_map();
                lvl.dirty = true;
                return ret;
            }
            (objects, area)
        };

        let ret = {
            let mut lvl = room::LimitedRoom {
                level: self,
                room_id,
            };
            Self::try_remove_object::<_, EC>(
                &log,
                &mut lvl, &mut objects,
                entities, room_id,
                object_id
            )
        };
        {
            let mut info = self.get_room_info_mut(room_id);
            info.objects = objects;
            info.rebuild_object_maps();
        }
        self.rebuild_path_sections(area);

        for loc in area {
            self.flag_dirty(loc.x, loc.y);
        }

        ret
    }

    /// Replaces the object with the given id into the room
    pub fn replace_object<EC: EntityCreator>(
            &mut self, entities: &mut Container, room_id: RoomId,
            object_id: usize, obj: ObjectPlacement
    ) {
        use std::mem;
        let (mut objects, area) = {
            let mut info = self.get_room_info_mut(room_id);
            if let Some(lvl) = info.building_level.as_mut() {
                lvl.replace_object::<EC>(entities, object_id, obj);
                return;
            };
            let objects = mem::replace(&mut info.objects, Vec::new());
            (objects, info.area)
        };

        {
            let log = self.log.clone();
            let mut lvl = room::LimitedRoom {
                level: self,
                room_id,
            };
            let rev = assume!(lvl.level.log, obj.apply::<_, EC>(&log, &mut lvl, entities, room_id, false));
            objects[object_id] = Some((obj, rev));
        }

        {
            let mut info = self.get_room_info_mut(room_id);
            info.objects = objects;
            info.rebuild_object_maps();
        }
        self.rebuild_path_sections(area);

        for loc in area {
            self.flag_dirty(loc.x, loc.y);
        }
    }

    /// Returns the objects in the room
    pub fn get_room_objects(&self, room_id: RoomId) -> Ref<'_, [Option<(ObjectPlacement, ReverseObjectPlacement)>]> {
        let info = self.get_room_info(room_id);
        Ref::map(info, |info| if let Some(lvl) = info.building_level.as_ref() {
            lvl.objects.as_slice()
        } else {
            info.objects.as_slice()
        })
    }

    /// Returns whether the requested room can be placed at the passed location.
    fn can_place_room(&self, owner: PlayerId, room_key: ResourceKey<'_>, bound: Bound) -> bool {
        // Is the room in bounds?
        if !(bound.min.x >= 0 && bound.min.y >= 0 && bound.max.x < self.width as i32 && bound.max.y < self.height as i32) {
            debug!(self.log, "Room out of bounds"; "room" => ?room_key, "owner" => ?owner, "bounds" => ?bound, "level_size" => ?(self.width, self.height));
            return false;
        }
        if !self.check_room_size(room_key.borrow(), bound) {
            debug!(self.log, "Room too small"; "room" => ?room_key, "owner" => ?owner, "bounds" => ?bound);
            return false;
        }
        // Is there something blocking the way?
        for loc in bound {
            if !self.check_tile_placeable(owner, room_key.borrow(), loc) {
                debug!(self.log, "Tile not placable"; "room" => ?room_key, "owner" => ?owner, "location" => ?loc);
                return false;
            }
        }
        true
    }

    /// Returns whether the area is large enough for the requested room
    pub fn check_room_size(&self, room_key: ResourceKey<'_>, bound: Bound) -> bool {
        let room = assume!(self.log, self.asset_manager.loader_open::<room::Loader>(room_key));
        // Is the room big enough?
        bound.width() >= room.min_size.0 && bound.height() >= room.min_size.1
    }

    /// Returns whether the tile is suitable for the room type
    pub fn check_tile_placeable(&self, owner_id: PlayerId, room_key: ResourceKey<'_>, loc: Location) -> bool {
        let room = assume!(self.log, self.asset_manager.loader_open::<room::Loader>(room_key.borrow()));
        // Is the room in bounds?
        if !self.level_bounds.in_bounds(loc) {
            debug!(self.log, "Tile out of bounds"; "room" => ?room_key, "owner" => ?owner_id, "location" => ?loc);
            return false;
        }
        let rooms = self.rooms.borrow();

        let tile = assume!(self.log, self.get_tile_raw(loc));
        // Can't build on areas already being worked on
        if tile.flags.contains(TileFlag::BUILDING) {
            debug!(self.log, "Tile being built"; "room" => ?room_key, "owner" => ?owner_id, "location" => ?loc);
            return false;
        }
        if !room.build_anywhere {
            if let Some(owner) = tile.owner {
                let owner = rooms.get_room_info(owner);
                if owner.owner != owner_id {
                debug!(self.log, "Tile not owned"; "room" => ?room_key, "owner" => ?owner_id, "location" => ?loc);
                    return false;
                }
                for dir in &ALL_DIRECTIONS {
                    if let Some(flag) = self.get_wall_info(loc, *dir) {
                        if flag.flag == TileWallFlag::Door {
                            debug!(self.log, "Tile blocked by door"; "room" => ?room_key, "owner" => ?owner_id, "location" => ?loc);
                            return false;
                        }
                    }
                }
                for yy in 0 .. 4 {
                    for xx in 0 .. 4 {
                        if owner.is_placeable_scaled(loc.x * 4 + xx, loc.y * 4 + yy) {
                            debug!(self.log, "Tile has placement blocked"; "room" => ?room_key, "owner" => ?owner_id, "location" => ?loc);
                            return false;
                        }
                    }
                }
            } else {
                debug!(self.log, "Tile has no owner"; "room" => ?room_key, "owner" => ?owner_id, "location" => ?loc);
                return false;
            }
        }
        // Does the tile meet the placement requirements
        if let Some(only_within) = room.only_within.as_ref() {
            if let Some(owner) = tile.owner {
                let owner = rooms.get_room_info(owner);
                if owner.key != *only_within {
                    debug!(self.log, "Tile isn't within {:?}", only_within; "room" => ?room_key, "owner" => ?owner_id, "location" => ?loc);
                    return false;
                }
            } else {
                debug!(self.log, "Tile not owned"; "room" => ?room_key, "owner" => ?owner_id, "location" => ?loc);
                return false;
            }
        }
        for check in &room.placement_checks {
            if !check.test(self, loc, 0, 0) {
                debug!(self.log, "Tile failed placement check {:?}", check; "room" => ?room_key, "owner" => ?owner_id, "location" => ?loc);
                return false;
            }
        }
        true
    }

    /// Places the room back into the building state
    pub fn undo_room_build<EC: EntityCreator, E: Invokable>(
            &mut self,
            engine: &E,
            entities: &mut Container,
            room_id: RoomId
    ) {
        use std::mem;

        let log = self.log.clone();

        let mut tiles;
        let objects = {
            let mut rooms = self.rooms.borrow_mut();
            let room = rooms.get_room_info_mut(room_id);
            room.state = RoomState::Building;
            tiles = room.original_tiles.clone();
            // Remove collision information
            room.collision_map.clear();
            mem::replace(&mut room.objects, vec![])
        };
        // Remove objects from the level and then apply them to the virtual level
        for obj in objects.iter().rev() {
            if let Some(obj) = obj.as_ref() {
                assume!(log, obj.1.apply::<_, EC>(&log, self, entities));
            }
        }

        self.do_update_room::<EC, _>(engine, entities, room_id);
        let (mut virt, area) = {
            let rooms = self.rooms.borrow_mut();
            let area = rooms.get_room_info(room_id).area;
            (self.capture_area(room_id, area), area)
        };
        for obj in objects {
            if let Some(obj) = obj {
                let rev = assume!(self.log, obj.0.apply::<_, EC>(&log, &mut virt, entities, room_id, false));
                virt.objects.push(Some((obj.0, rev)));
            } else {
                virt.objects.push(None);
            }
        }
        virt.rebuild_placement_map();
        {
            let mut rooms = self.rooms.borrow_mut();
            let room = rooms.get_room_info_mut(room_id);
            room.building_level = Some(Box::new(virt));
        }
        {
            let mut tiles_info = self.tiles.borrow_mut();
            for loc in area {
                {
                    let td = &mut tiles_info.tiles[LevelTiles::location_index(loc, self.width)];
                    *td = tiles.remove(0);
                    td.flags |= TileFlag::BUILDING;
                    td.owner = Some(room_id);
                }
                tiles_info.flag_dirty(loc.x, loc.y);
            }
            tiles_info.update_walls(area);
        }

        self.do_update_room::<EC, _>(engine, entities, room_id);
        self.rebuild_path_sections(area);
    }

    fn capture_area(&self, id: RoomId, bound: Bound) -> RoomVirtualLevel {
        use std::cmp::{max, min};
        let mut fbound = bound;
        fbound.min -= (4, 4);
        fbound.max += (4, 4);
        fbound.min.x = max(fbound.min.x, 0);
        fbound.min.y = max(fbound.min.y, 0);
        fbound.max.x = min(fbound.max.x, self.width as i32 - 1);
        fbound.max.y = min(fbound.max.y, self.height as i32 - 1);
        let mut virt = RoomVirtualLevel::new(
            &self.log,
            id,
            fbound.min.x, fbound.min.y,
            fbound.width() as u32,
            fbound.height() as u32,
            bound,
            self.asset_manager.clone(),
        );
        let tiles_info = self.tiles.borrow();
        virt.tile_map = tiles_info.tile_map.clone();
        for loc in virt.bounds {
            let td = tiles_info.tiles[LevelTiles::location_index(loc, self.width)];
            virt.tiles[RoomVirtualLevel::location_index(loc - virt.offset, virt.width)] = td;
            for dir in &ALL_DIRECTIONS {
                virt.set_wall_info(loc, *dir, tiles_info.get_wall_info(loc, *dir));
            }
        }
        virt
    }

    pub(crate) fn do_update_room<EC: EntityCreator, E: Invokable>(&self, engine: &E, entities: &mut Container, room_id: RoomId) {
        let area = {
            self.get_room_info(room_id).area
        };
        self.do_update_room_area::<EC, E>(engine, entities, Some(room_id), area);
    }
    pub(crate) fn do_update_room_area<EC: EntityCreator, E: Invokable>(&self, engine: &E, entities: &mut Container, room_id: Option<RoomId>, area: Bound) {
        // Run possible update scripts
        let mut near_rooms = FNVSet::default();
        room_id.map(|room_id| near_rooms.insert(room_id));
        {
            let tiles = self.tiles.borrow();
            for x in -1 ..= area.width() {
                tiles.get_room_owner(area.min + (x, -1))
                    .map(|v| near_rooms.insert(v));
                tiles.get_room_owner(area.min + (x, area.height()))
                    .map(|v| near_rooms.insert(v));
            }
            for y in -1 ..= area.height() {
                tiles.get_room_owner(area.min + (-1, y))
                    .map(|v| near_rooms.insert(v));
                tiles.get_room_owner(area.min + (area.width(), y))
                    .map(|v| near_rooms.insert(v));
            }
        }

        for room_id in near_rooms {
            {
                let mut rooms = self.rooms.borrow_mut();
                let room = rooms.get_room_info_mut(room_id);
                room.needs_update = true;
            }
            RoomPlacement::do_update::<_, EC::ScriptTypes>(
                &self.log, &self.asset_manager, engine,
                entities,
                &self.rooms, room_id,
            );
            RoomPlacement::do_update_apply::<_, EC::ScriptTypes>(
                &self.log, &self.asset_manager, engine,
                entities,
                &self.rooms, room_id,
            );
        }
    }

    /// Returns whether something is blocking a full edit on
    /// the named room
    pub fn is_blocked_edit(&self, room_id: RoomId) -> Result<(), &'static str> {
        let rooms = self.rooms.borrow();
        let tiles = self.tiles.borrow();
        let room = rooms.get_room_info(room_id);
        // Firstly check if the room owns all of it self
        // (e.g. it doesn't have an another room inside)
        for loc in room.area {
            if tiles.get_room_owner(loc) != Some(room_id) {
                return Err("Another room is built inside");
            }
        }
        for loc in &room.required_tiles {
            if let Some(other) = tiles.get_room_owner(*loc) {
                let other = rooms.get_room_info(other);
                if other.is_blocked(*loc) {
                    return Err("Another room depends on this one");
                }
            }
        }
        Ok(())
    }

    // Network serializing

    /// Creates a packet containing the initial state of the level
    pub fn create_initial_state(&self) -> (Vec<String>, packet::Raw) {
        use delta_encode::bitio::{Writer, write_len_bits};
        use delta_encode::DeltaEncodable;
        use flate2::write::DeflateEncoder;
        use flate2::Compression;

        let mut strings = FNVMap::default();

        let rooms = self.rooms.borrow();

        let mut state = Writer::new(DeflateEncoder::new(Vec::new(), Compression::best()));
        let _ = write_len_bits(&mut state, rooms.room_order.len());
        for room in &rooms.room_order {
            let room = assume!(self.log, rooms.rooms.get(*room));
            let _ = state.write_signed(i64::from(room.id.0), 16);
            let _ = state.write_signed(i64::from(room.owner.0), 16);
            let _ = room.area.encode(None, &mut state);

            let _ = state.write_bool(room.tile_update_state.is_some());
            if let Some(data) = room.tile_update_state.as_ref() {
                let _ = write_len_bits(&mut state, data.len());
                for v in data {
                    let _ = state.write_unsigned(u64::from(*v), 8);
                }
            }

            write_string_packed(&mut state, &mut strings, room.key.as_string());

            let _ = state.write_unsigned(match room.state {
                RoomState::Planning => 0,
                RoomState::Building => 1,
                RoomState::Done => 2,
            }, 2);
            let objects = if let Some(lvl) = room.building_level.as_ref() {
                &lvl.objects
            } else {
                &room.objects
            };
            let _ = write_len_bits(&mut state, objects.len());
            for obj in objects {
                let _ = state.write_bool(obj.is_some());
                if let Some(obj) = obj.as_ref() {
                    let _ = ObjectNetworkInfo {
                        // TODO: Remove clone once delta-encode handles lifetimes
                        key: obj.0.key.clone(),
                        position: obj.0.position,
                        rotation: obj.0.rotation,
                        version: obj.0.version,
                    }.encode(None, &mut state);
                }
            }
        }

        let mut string_data = vec![String::new(); strings.len()];
        for (s, i) in strings {
            string_data[i] = s;
        }

        let state = assume!(self.log, state.finish()
            .and_then(|v| v.finish())
        );
        (string_data, packet::Raw(state))
    }

    /// Loads the initial state of the level from the packet
    pub fn load_initial_state<EC, E>(
            &mut self,
            engine: &E,
            entities: &mut Container,
            strings: Vec<String>,
            state: packet::Raw,
    ) -> UResult<()>
        where E: Invokable,
              EC: EntityCreator
    {
        use std::io::Cursor;
        use delta_encode::bitio::{Reader, read_len_bits};
        use delta_encode::DeltaEncodable;
        use flate2::read::DeflateDecoder;

        let strings = strings;
        let mut state = Reader::new(DeflateDecoder::new(Cursor::new(state.0)));
        let len = read_len_bits(&mut state)?;

        let mut objects = Vec::with_capacity(len);

        for _ in 0 .. len {
            let id = RoomId(state.read_signed(16)? as i16);
            let owner = PlayerId(state.read_signed(16)? as i16);
            let area = Bound::decode(None, &mut state)?;
            let tile_update_state = if state.read_bool()? {
                let len = read_len_bits(&mut state)?;
                let mut data = Vec::with_capacity(len);
                for _ in 0 .. len {
                    data.push(state.read_unsigned(8)? as u8);
                }
                Some(data)
            } else {
                None
            };
            let res = ResourceKey::parse(read_string_packed(&mut state, &strings)?)
                .ok_or_else(|| ErrorKind::InvalidResourceKey)?;
            let rstate = state.read_unsigned(2)?;

            self.place_room_id::<EC, _>(engine, entities, id, owner, res.borrow(), area)
                .ok_or_else(|| ErrorKind::FailedLevelRecreation)?;

            {
                let mut rm = self.get_room_info_mut(id);
                rm.tile_update_state = tile_update_state;
            }

            match rstate {
                // Planning
                0 => {
                    // Place starts in this state
                    // Do nothing
                }
                // Building
                1 => {
                    self.finalize_placement(id);
                }
                // Done
                2 => {
                    self.finalize_placement(id);
                    self.finalize_room::<EC, _>(engine, entities, id)?;
                }
                _ => return Err(ErrorKind::InvalidRoomState.into()),
            };
            let mut to_load = Vec::with_capacity(len);
            let len = read_len_bits(&mut state)?;
            for _ in 0 .. len {
                if state.read_bool()? {
                    let object: ObjectNetworkInfo = ObjectNetworkInfo::decode(None, &mut state)?;
                    to_load.push(Some(object));
                } else {
                    to_load.push(None);
                }
            }
            objects.push((id, to_load));
        }
        for (id, objects) in objects {
            let mut needs_gaps = vec![];
            self.compute_path_data = false;
            for (obj_id, obj) in objects.into_iter().enumerate() {
                if let Some(object) = obj {
                    self.begin_object_placement::<_, EC>(id, engine, entities, object.key, Some(object.version))?;
                    self.move_active_object::<_, EC>(
                        id, engine, entities,
                        (object.position.x, object.position.y),
                        Some(object.version), object.rotation,
                    )?;
                    self.finalize_object_placement::<_, EC>(id, engine, entities, Some(object.version), object.rotation)?;
                } else {
                    needs_gaps.push(obj_id);
                }
            }
            self.compute_path_data = true;
            let area = {
                let room: &mut RoomPlacement = &mut *self.get_room_info_mut(id);
                for gap in needs_gaps {
                    if let Some(lvl) = room.building_level.as_mut() {
                        lvl.objects.insert(gap, None)
                    } else {
                        room.objects.insert(gap, None)
                    }
                }
                room.rebuild_object_maps();
                room.area
            };
            {
                let room_info = {
                    self.asset_manager.loader_open::<room::Loader>(self.get_room_info(id).key.borrow())?
                };
                let cost = room_info.cost_for_room(self, id);
                let room: &mut RoomPlacement = &mut *self.get_room_info_mut(id);
                room.placement_cost = cost;
            }
            self.rebuild_path_sections(area);
        }
        Ok(())
    }

    pub(crate) fn rebuild_path_sections(&mut self, area: Bound) {
        if !self.compute_path_data {
            return;
        }
        let min_x = (area.min.x / 4) - 1;
        let min_y = (area.min.y / 4) - 1;
        let max_x = ((area.max.x + 3) / 4) + 1;
        let max_y = ((area.max.y + 3) / 4) + 1;
        let w = (self.width as i32 + 3) / 4;
        let h = (self.height as i32 + 3) / 4;
        for y in min_y .. max_y {
            for x in min_x .. max_x {
                if x >= 0 && y >= 0 && x < w && y < h {
                    self.recompute_path_section(x as usize, y as usize);
                }
            }
        }
    }
}

#[derive(Debug, DeltaEncode)]
#[delta_always]
struct ObjectNetworkInfo {
    /// The key to the object that this placement is about
    pub key: ResourceKey<'static>,
    /// The position the object was placed at.
    ///
    /// This is the raw click position as passed to the script
    pub position: object::ObjectPlacePosition,
    /// The rotation value passed to the placement script
    ///
    /// Used when moving an object to keep its rotation the
    /// same
    pub rotation: i16,
    /// A script provided version number to use when replacing
    /// this object
    pub version: i32,
}

/// The storage for the level's tiles
pub struct LevelTiles {
    log: Logger,
    /// The width of the level
    pub width: u32,
    /// The height of the level
    pub height: u32,
    asset_manager: AssetManager,
    /// The bounds of the level
    pub level_bounds: Bound,
    /// Raw tile data, `x + y * width`
    tiles: Vec<TileData>,
    tile_map: IntMap<tile::TileId, Arc<tile::Type>>,
    /// Raw wall data, `(x + 1) + (y + 1) * (width + 1)`
    ///
    /// Each location stores the south and west wall for that
    /// tile
    walls: Vec<WallData>,

    window_type: Vec<ResourceKey<'static>>,
    window_type_map: FNVMap<ResourceKey<'static>, u8>,

    /// Dirty flags per a section (defined by `SECTION_SIZE`)
    dirty_sections: Vec<bool>,

    pathmap: Vec<PathSection>,
}

impl lua::LuaUsable for LevelTiles {}
impl script::LuaTracked for LevelTiles {
    const KEY: script::NulledString = nul_str!("level_tiles");
    type Storage = Weak<RefCell<LevelTiles>>;
    type Output = Rc<RefCell<LevelTiles>>;
    fn try_convert(s: &Self::Storage) -> Option<Self::Output> {
        s.upgrade()
    }
}

impl LevelView for LevelTiles {
    fn get_tile(&self, loc: Location) -> Arc<tile::Type> {
        LevelTiles::get_tile(self, loc)
    }
    fn get_tile_raw(&self, loc: Location) -> Option<TileData> {
        LevelTiles::get_tile_raw(self, loc)
    }
    fn get_room_owner(&self, loc: Location) -> Option<RoomId> {
        LevelTiles::get_room_owner(self, loc)
    }
    fn get_tile_flags(&self, loc: Location) -> TileFlag {
        LevelTiles::get_tile_flags(self, loc)
    }
    fn get_wall_info(&self, loc: Location, dir: Direction) -> Option<WallInfo> {
        LevelTiles::get_wall_info(self, loc, dir)
    }

    fn get_asset_manager(&self) -> AssetManager {
        self.asset_manager.clone()
    }

    fn get_window(&self, id: u8) -> ResourceKey<'static> {
        self.window_type[id as usize].clone()
    }
}

impl LevelAccess for LevelTiles {
    fn set_tile(&mut self, loc: Location, tile: ResourceKey<'_>) {
        LevelTiles::set_tile(self, loc, tile);
    }

    fn set_wall_info(&mut self, loc: Location, dir: Direction, info: Option<WallInfo>) {
        LevelTiles::set_wall_info(self, loc, dir, info)
    }

    fn set_tile_flags(&mut self, loc: Location, flags: TileFlag) {
        LevelTiles::set_tile_flags(self, loc, flags);
    }

    fn set_tile_raw(&mut self, loc: Location, tile: TileData) {
        if self.level_bounds.in_bounds(loc) {
            self.tiles[Self::location_index(loc, self.width)] = tile;

            for xx in -1 .. 2 {
                for yy in -1 .. 2 {
                    self.flag_dirty(loc.x + xx, loc.y + yy);
                }
            }
            self.update_walls(Bound::new(loc, loc));
        }
    }

    fn get_or_create_window(&mut self, key: ResourceKey<'_>) -> u8 {
        if let Some(id) = self.window_type_map.get(&key).cloned() {
            return id;
        }
        let id = self.window_type.len();
        self.window_type_map.insert(key.borrow().into_owned(), id as u8);
        self.window_type.push(key.into_owned());
        id as u8
    }
}

impl LevelTiles {

    fn location_index(loc: Location, width: u32) -> usize {
        (loc.x + loc.y * (width as i32)) as usize
    }

    /// Sets the tile at the passed location to the provided `TileData`. If
    /// the location is out of bounds then it is ignored.
    pub fn set_tile(&mut self, loc: Location, tile: ResourceKey<'_>) {
        if self.level_bounds.in_bounds(loc) {
            let t = if let Ok(t) = self.asset_manager.loader_open::<tile::Loader>(tile.borrow()) {
                t
            } else {
                panic!("Tried to set an invalid tile: {:?}", tile);
            };
            {
                let td = &mut self.tiles[Self::location_index(loc, self.width)];
                td.id = t.id;
                td.flags = TileFlag::empty();
            }
            self.tile_map.insert(t.id, t);
            for xx in -1 .. 2 {
                for yy in -1 .. 2 {
                    self.flag_dirty(loc.x + xx, loc.y + yy);
                }
            }
            self.update_walls(Bound::new(loc, loc));
        }
    }

    /// Flags the section at the location as dirty
    pub fn flag_dirty(&mut self, x: i32, y: i32) {
        if self.level_bounds.in_bounds(Location::new(x, y)) {
            let sx = x as usize / SECTION_SIZE;
            let sy = y as usize / SECTION_SIZE;
            let sw = (self.width as usize + (SECTION_SIZE - 1)) / SECTION_SIZE;
            self.dirty_sections[sx + sy * sw] = true;
        }
    }

    /// Flags all sections as dirty
    pub fn flag_all_dirty(&mut self) {
        for v in &mut self.dirty_sections {
            *v = true;
        }
    }

    /// Gets the tile at the passed location. If the location is out of bounds then
    /// a default tile is returned.
    pub fn get_tile(&self, loc: Location) -> Arc<tile::Type> {
        let id = if self.level_bounds.in_bounds(loc) {
            self.tiles[Self::location_index(loc, self.width)].id
        } else {
            0
        };
        assume!(self.log, self.tile_map.get(id)).clone()
    }

    fn get_tile_raw(&self, loc: Location) -> Option<TileData> {
        if self.level_bounds.in_bounds(loc) {
            Some(self.tiles[Self::location_index(loc, self.width)])
        } else {
            None
        }
    }

    /// Returns the id of the owner of the tile. None is returned if
    /// no room owns the tile.
    pub fn get_room_owner(&self, loc: Location) -> Option<RoomId> {
        if self.level_bounds.in_bounds(loc) {
            self.tiles[Self::location_index(loc, self.width)].owner
        } else {
            None
        }
    }

    /// Returns the flags set on the tile
    pub fn get_tile_flags(&self, loc: Location) -> TileFlag {
        if self.level_bounds.in_bounds(loc) {
            self.tiles[Self::location_index(loc, self.width)].flags
        } else {
            TileFlag::empty()
        }
    }

    /// Sets the flags set on the tile
    pub fn set_tile_flags(&mut self, loc: Location, flags: TileFlag) {
        if self.level_bounds.in_bounds(loc) {
            self.tiles[Self::location_index(loc, self.width)].flags = flags;
            for xx in -1 .. 2 {
                for yy in -1 .. 2 {
                    self.flag_dirty(loc.x + xx, loc.y + yy);
                }
            }
            self.update_walls(Bound::new(loc, loc));
        }
    }

    /// Returns the information about a wall at a location + direction if it exists
    /// otherwise it returns none.
    pub fn get_wall_info(&self, loc: Location, dir: Direction) -> Option<WallInfo> {
        let (loc, dir) = match dir {
            Direction::North | Direction::East => (loc.shift(dir), dir.reverse()),
            _ => (loc, dir),
        };
        // The wall bounds is slightly bigger due to the way that only
        // the south and west walls are stored
        let mut special_bounds = self.level_bounds;
        special_bounds.min -= (1, 1);
        if special_bounds.in_bounds(loc) {
            let idx = (loc.x + 1) + (loc.y + 1) * (self.width + 1) as i32;
            self.walls.get(idx as usize).and_then(|v| v.data[dir.as_usize() >> 1])
        } else {
            None
        }
    }

    /// Sets the information about a wall at a location + direction.
    pub fn set_wall_info(&mut self, loc: Location, dir: Direction, info: Option<WallInfo>) {
        let (loc, dir) = match dir {
            Direction::North | Direction::East => (loc.shift(dir), dir.reverse()),
            _ => (loc, dir),
        };
        // The wall bounds is slightly bigger due to the way that only
        // the south and west walls are stored
        let mut special_bounds = self.level_bounds;
        special_bounds.min -= (1, 1);
        if special_bounds.in_bounds(loc) {
            let idx = (loc.x + 1) + (loc.y + 1) * (self.width + 1) as i32;
            if let Some(wall) = self.walls.get_mut(idx as usize) {
                wall.data[dir.as_usize() >> 1] = info;
            }
        }
    }

    /// Returns whether the section at the location is dirty and clears the flag
    /// if it is. Used by the renderer and shouldn't be used by anything else.
    pub fn get_and_clear_dirty_section(&mut self, x: usize, y: usize) -> bool {
        let sw = (self.width as usize + (SECTION_SIZE - 1)) / SECTION_SIZE;
        let dirty = self.dirty_sections[x + y * sw];
        self.dirty_sections[x + y * sw] = false;
        dirty
    }

    fn update_walls(&mut self, bound: Bound) {
        for loc in bound.extend(1) {
            // Recompute walls
            for dir in &ALL_DIRECTIONS {
                let was_wall = self.get_wall_info(loc, *dir).is_some();
                let is_wall = {
                    self.get_tile(loc).should_place_wall(self, loc, *dir)
                        || self.get_tile(loc.shift(*dir)).should_place_wall(self, loc.shift(*dir), dir.reverse())
                };
                if was_wall != is_wall {
                    if is_wall {
                        self.set_wall_info(loc, *dir, Some(WallInfo {
                            flag: TileWallFlag::None,
                        }));
                    } else {
                        self.set_wall_info(loc, *dir, None);
                    }
                }
            }
        }
        for loc in bound.extend(2) {
            self.flag_dirty(loc.x, loc.y);
        }
    }

    /// Returns a bitset containing edges that this edge is connected too
    pub fn get_pathable_edges(&self, sx: usize, sy: usize, edge: usize) -> u64 {
        let section = &self.pathmap[sx + sy * ((self.width as usize + 3) / 4)];
        section.info[edge]
    }

    /// Returns an estimated cost of moving through the area
    pub fn get_section_cost(&self, sx: usize, sy: usize) -> i32 {
        let section = &self.pathmap[sx + sy * ((self.width as usize + 3) / 4)];
        section.movement_cost
    }
}

/// The storage for the level's rooms
pub struct LevelRooms {
    log: Logger,
    /// The width of the level
    pub width: u32,
    /// The height of the level
    pub height: u32,
    /// The bounds of the level
    pub level_bounds: Bound,

    /// Loaded rooms by id
    rooms: IntMap<RoomId, RoomPlacement>,
    /// Order the rooms were added. Used to ease reproducing
    /// the level state for the client
    room_order: Vec<RoomId>,
}

impl lua::LuaUsable for LevelRooms {}
impl script::LuaTracked for LevelRooms {
    const KEY: script::NulledString = nul_str!("level_rooms");
    type Storage = Weak<RefCell<LevelRooms>>;
    type Output = Rc<RefCell<LevelRooms>>;
    fn try_convert(s: &Self::Storage) -> Option<Self::Output> {
        s.upgrade()
    }
}

impl LevelRooms {
    /// Iterates over rooms in placement order
    pub fn iter_rooms<'a>(&'a self) -> impl Iterator<Item=(RoomId, &RoomPlacement)> + 'a {
        self.room_order.iter()
            .cloned()
            .flat_map(move |v| self.rooms.get(v).map(|r| (v, r)))
    }

    /// Returns a collection of room ids
    pub fn room_ids(&self) -> impl Iterator<Item=RoomId> + '_ {
        self.rooms.keys()
    }

    /// Returns whether a room with the given id exists
    pub fn room_exists(&self, id: RoomId) -> bool {
        self.rooms.get(id).is_some()
    }

    /// Returns the placement information of the room with the given
    /// id.
    pub fn try_room_info(&self, id: RoomId) -> Option<&RoomPlacement> {
        self.rooms.get(id)
    }
    /// Returns the placement information of the room with the given
    /// id.
    pub fn try_room_info_mut(&mut self, id: RoomId) -> Option<&mut RoomPlacement> {
        self.rooms.get_mut(id)
    }

    /// Returns the placement information of the room with the given
    /// id.
    pub fn get_room_info(&self, id: RoomId) -> &RoomPlacement {
        assume!(self.log, self.rooms.get(id))
    }

    /// Returns the placement information of the room with the given
    /// id.
    pub fn get_room_info_mut(&mut self, id: RoomId) -> &mut RoomPlacement {
        assume!(self.log, self.rooms.get_mut(id))
    }
}

fn write_string_packed<W, S>(state: &mut bitio::Writer<W>, strings: &mut FNVMap<String, usize>, string: S)
    where W: ::std::io::Write,
          S: Into<String>,
{
    let len = strings.len();
    let id = *strings.entry(string.into()).or_insert(len);
    let _ = state.write_unsigned(id as u64, 16);
}

fn read_string_packed<'a, R>(state: &mut bitio::Reader<R>, strings: &'a [String]) -> ::std::io::Result<&'a str>
    where R: ::std::io::Read
{
    use std::io;
    let id = state.read_unsigned(16)? as usize;
    strings.get(id)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid string"))
        .map(|v| v.as_str())
}

/// A single check rule.
#[derive(Clone, Debug)]
pub enum Check {
    /// A check rule that matches a tile name
    Name(i32, i32, ResourceKey<'static>),
    /// A check rule that matches a tile property
    Property(i32, i32, String),
    /// A check that matches a tile owner
    Owner(i32, i32, OwnerType),
    /// A check rule that inverts the inner rule
    Not(Box<Check>),
    /// A check rule that always matches
    Always,
}

/// A type of owner for a tile
#[derive(Debug, Clone, Copy)]
pub enum OwnerType {
    /// A room owner
    Room,
}

impl Check {
    /// Creates a new check from the given check string
    ///
    /// * `!` inverts the following check string
    /// * `.` checks a property on the tile
    /// * `@` currently can only be `room` to check if the
    ///        tiles room owner matches this tile.
    /// * Anything else matches the tile name.
    pub fn new(module: ModuleKey<'_>, x: i32, y: i32, mut val: &str) -> Check {
        let inv = val.starts_with('!');
        if inv {
            val = &val[1..];
        }
        let check = match val.chars().next() {
            Some('.') => Check::Property(x, y, val[1..].into()),
            Some('@') => Check::Owner(x, y, match &val[1..] {
                "room" => OwnerType::Room,
                _ => panic!("Invalid owner type"),
            }),
            _ => Check::Name(x, y, LazyResourceKey::parse(val)
                    .or_module(module)
                    .into_owned()),
        };
        if inv {
            Check::Not(Box::new(check))
        } else {
            check
        }
    }

    /// Tests a tile using the check rule and returns whether it matches.
    ///
    /// `dx` and `dy` will be added to the offsets used by the checks.
    pub fn test<L: LevelView>(&self, level: &L, loc: Location, dx: i32, dy: i32) -> bool {
        match *self {
            Check::Always => true,
            Check::Name(ox, oy, ref name) => {
                let o = level.get_tile(loc.offset(ox + dx, oy + dy));
                o.key == *name
            }
            Check::Property(ox, oy, ref prop) => {
                let o = level.get_tile(loc.offset(ox + dx, oy + dy));
                o.properties.contains(prop)
            }
            Check::Not(ref sub) => {
                !sub.test(level, loc, dx, dy)
            }
            Check::Owner(ox, oy, owner_type) => {
                match owner_type {
                    OwnerType::Room => {
                        let self_owner = level.get_room_owner(loc);
                        let other_owner = level.get_room_owner(loc.offset(ox + dx, oy + dy));
                        self_owner == other_owner
                    }
                }
            }
        }
    }
}


pub(crate) fn edge_to_id(x: usize, y: usize) -> Option<usize> {
    match (x, y) {
        (0, y @ 0 ..= 14) => Some(y),
        (x @ 0 ..= 14, 15) => Some(15 + x),
        (15, y @ 0 ..= 15) => Some(45 - y),
        (x @ 0 ..= 15, 0) => Some(60 - x),
        _ => None
    }
}
pub(crate) fn id_to_edge(id: usize) -> (usize, usize) {
    match id {
        id @ 0 ..= 14 => (0, id),
        id @ 15 ..= 29 => (id - 15, 15),
        id @ 30 ..= 45 => (15, 15 - (id - 30)),
        id @ 46 ..= 60 => (15 - (id - 45), 0),
        _ => unreachable!(),
    }
}

pub(crate) fn flood_fill(visited: &mut BitSet, tiles: &LevelTiles, rooms: &LevelRooms, x: usize, y: usize, sx: usize, sy: usize) -> u64 {
    use std::cmp::min as cmin;

    let mut queue = Vec::with_capacity(32 * 4);
    queue.push((x as i32, y as i32));

    let mut touched = 0;

    let min = (sx as i32 * 16, sy as i32 * 16);
    let max = (
        cmin(min.0 + 15, tiles.width as i32 * 4),
        cmin(min.1 + 15, tiles.height as i32 * 4)
    );

    while let Some((x, y)) = queue.pop() {
        let cx = x & 0xF;
        let cy = y & 0xF;
        let idx = (cx + cy * 16) as usize;
        if x < min.0 || x > max.0
            || y < min.1 || y > max.1
            || visited.get(idx)
        {
            continue;
        }

        visited.set(idx, true);

        if !can_visit(tiles, rooms, x as usize, y as usize) {
            continue;
        }

        if let Some(edge) = edge_to_id((x & 0xF) as usize, (y & 0xF) as usize) {
            touched |= 1 << edge;
        }

        for d in &ALL_DIRECTIONS {
            let (ox, oy) = d.offset();
            let idx = (
                ((x + ox)&0xF)
                + ((y + oy)&0xF) * 16
            ) as usize;
            if x + ox >= min.0 && x + ox <= max.0
                && y + oy >= min.1 && y + oy <= max.1
                && !visited.get(idx)
            {
                queue.push((x + ox, y + oy));
            }
        }
    }
    touched
}

/// Helper to see whether an entity would be able to
/// visit the location
pub fn can_visit(tiles: &LevelTiles, rooms: &LevelRooms, x: usize, y: usize) -> bool {
    let cx = x as i32;
    let cy = y as i32;
    let loc = Location::new((cx / 4) as i32, (cy / 4) as i32);
    if !tiles.level_bounds.in_bounds(loc) || cx < 0 || cy < 0 {
        return false;
    }
    let owner = tiles.get_room_owner(loc);
    if let Some(owner) = owner {
        let room = rooms.get_room_info(owner);
        if room.collides_at_scaled(cx, cy) {
            return false;
        }
    }

    fn map(v: Option<WallInfo>) -> Option<TileWallFlag> {
        v.map(|v| v.flag).and_then(|v| if let TileWallFlag::Door = v { None } else { Some(v) })
    }

    let flag = match cx & 0b11 {
        0 => map(tiles.get_wall_info(loc, Direction::East)),
        3 => map(tiles.get_wall_info(loc, Direction::West)),
        _ => None,
    }.or_else(|| {
        match cy & 0b11 {
            0 => map(tiles.get_wall_info(loc, Direction::North)),
            3 => map(tiles.get_wall_info(loc, Direction::South)),
            _ => None,
        }
    }).or_else(|| {
        match (cx & 0b11, cy & 0b11) {
            (0, 0) => map(tiles.get_wall_info(loc.shift(Direction::North), Direction::East))
                .or_else(|| map(tiles.get_wall_info(loc.shift(Direction::East), Direction::North))),
            (3, 0) => map(tiles.get_wall_info(loc.shift(Direction::North), Direction::West))
                .or_else(|| map(tiles.get_wall_info(loc.shift(Direction::West), Direction::North))),
            (0, 3) => map(tiles.get_wall_info(loc.shift(Direction::South), Direction::East))
                .or_else(|| map(tiles.get_wall_info(loc.shift(Direction::East), Direction::South))),
            (3, 3) => map(tiles.get_wall_info(loc.shift(Direction::South), Direction::West))
                .or_else(|| map(tiles.get_wall_info(loc.shift(Direction::West), Direction::South))),
            _ => None,
        }
    });
    flag.is_none()
}