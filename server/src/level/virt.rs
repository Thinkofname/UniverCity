use super::*;
use crate::util::*;

/// A virtual level used for building rooms before placing them
pub struct RoomVirtualLevel {
    log: Logger,
    pub(super) id: room::Id,
    pub(super) offset: (i32, i32),
    pub(super) width: u32,
    pub(super) _height: u32,
    /// The bounds the virtual level covers
    pub bounds: Bound,
    /// The room's bounds
    pub room_bounds: Bound,
    pub(super) tiles: Vec<TileData>,
    pub(super) tile_map: IntMap<tile::TileId, Arc<tile::Type>>,
    walls: Vec<WallData>,
    /// Set to flag that the virtual level was modified
    pub dirty: bool,
    asset_manager: AssetManager,

    /// Objects in the room
    pub objects: Vec<Option<(ObjectPlacement, ReverseObjectPlacement)>>,
    pub(super) object_placement: Option<TempObjectPlacement>,

    /// Map of valid placement areas
    pub placement_map: BitSet,

    /// Whether this room has a border tile
    pub has_border: bool,
    /// Whether some of the walls of the room should be lowered
    /// to help with placement
    pub should_lower_walls: bool,

    window_type: Vec<ResourceKey<'static>>,
    window_type_map: FNVMap<ResourceKey<'static>, u8>,
}

impl RoomVirtualLevel {
    pub(super) fn new(
        log: &Logger,
        id: room::Id,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        room_bounds: Bound,
        asset_manager: AssetManager,
    ) -> RoomVirtualLevel {
        RoomVirtualLevel {
            log: log.new(o!("source" => "virtual_level")),
            id,
            offset: (x, y),
            width: w,
            _height: h,
            bounds: Bound::new(
                Location::new(x, y),
                Location::new(x + w as i32 - 1, y + h as i32 - 1),
            ),
            room_bounds,
            tiles: vec![
                TileData {
                    id: 0,
                    owner: None,
                    flags: TileFlag::empty(),
                };
                (w * h) as usize
            ],
            tile_map: IntMap::new(),
            walls: vec![WallData { data: [None; 2] }; ((w + 1) * (h + 1)) as usize],
            dirty: true,
            asset_manager,
            objects: vec![],
            object_placement: None,

            placement_map: BitSet::new(
                (room_bounds.width() * 4 * room_bounds.height() * 4) as usize,
            ),
            has_border: false,
            should_lower_walls: false,
            window_type: vec![],
            window_type_map: FNVMap::default(),
        }
    }

    pub(super) fn location_index(loc: Location, width: u32) -> usize {
        (loc.x + loc.y * (width as i32)) as usize
    }

    /// Sets the tile at the passed location to the provided `TileData`. If
    /// the location is out of bounds then it is ignored.
    ///
    /// Shares a tile map with the passed level
    pub fn set_tile(&mut self, loc: Location, tile: ResourceKey<'_>) {
        if self.bounds.in_bounds(loc) {
            let t = if let Ok(t) = self
                .asset_manager
                .loader_open::<tile::Loader>(tile.borrow())
            {
                t
            } else {
                panic!("Tried to set an invalid tile: {:?}", tile);
            };
            let mut td = assume!(self.log, self.get_tile_raw(loc));
            td.id = t.id;
            td.flags = TileFlag::empty();
            self.tile_map.insert(t.id, t);
            self.set_tile_raw(loc, td);
            self.dirty = true;
        }
    }

    /// Gets the tile at the passed location. If the location is out of bounds then
    /// a default tile is returned.
    ///
    /// Shares a tile map with the passed level
    pub fn get_tile(&self, loc: Location) -> Arc<tile::Type> {
        let id = if self.bounds.in_bounds(loc) {
            self.tiles[Self::location_index(loc - self.offset, self.width)].id
        } else {
            0
        };
        assume!(self.log, self.tile_map.get(id)).clone()
    }

    /// Returns the flags set on the tile
    pub fn get_tile_flags(&self, loc: Location) -> TileFlag {
        if self.bounds.in_bounds(loc) {
            self.tiles[Self::location_index(loc - self.offset, self.width)].flags
        } else {
            TileFlag::empty()
        }
    }

    /// Sets the flags set on the tile
    pub fn set_tile_flags(&mut self, loc: Location, flags: TileFlag) {
        if self.bounds.in_bounds(loc) {
            self.tiles[Self::location_index(loc - self.offset, self.width)].flags = flags;
            self.dirty = true;
        }
    }

    /// Returns the infomation about a wall at a location + direction if it exists
    /// otherwise it returns none.
    pub fn get_wall_info(&self, loc: Location, dir: Direction) -> Option<WallInfo> {
        let (loc, dir) = match dir {
            Direction::North | Direction::East => (loc.shift(dir), dir.reverse()),
            _ => (loc, dir),
        };
        // The wall bounds is slightly bigger due to the way that only
        // the south and west walls are stored
        let mut special_bounds = self.bounds;
        special_bounds.min -= (1, 1);
        if special_bounds.in_bounds(loc) {
            let loc = loc - self.offset;
            let idx = (loc.x + 1) + (loc.y + 1) * (self.width + 1) as i32;
            self.walls
                .get(idx as usize)
                .and_then(|v| v.data[dir.as_usize() >> 1])
        } else {
            None
        }
    }

    /// Sets the infomation about a wall at a location + direction.
    pub fn set_wall_info(&mut self, loc: Location, dir: Direction, info: Option<WallInfo>) {
        let (loc, dir) = match dir {
            Direction::North | Direction::East => (loc.shift(dir), dir.reverse()),
            _ => (loc, dir),
        };
        // The wall bounds is slightly bigger due to the way that only
        // the south and west walls are stored
        let mut special_bounds = self.bounds;
        special_bounds.min -= (1, 1);
        if special_bounds.in_bounds(loc) {
            let loc = loc - self.offset;
            let idx = (loc.x + 1) + (loc.y + 1) * (self.width + 1) as i32;
            if let Some(wall) = self.walls.get_mut(idx as usize) {
                wall.data[dir.as_usize() >> 1] = info;
            }
        }
    }

    /// Returns the id of the owner of the tile. None is returned if
    /// no room owns the tile.
    pub fn get_room_owner(&self, loc: Location) -> Option<room::Id> {
        if self.bounds.in_bounds(loc) {
            self.tiles[Self::location_index(loc - self.offset, self.width)].owner
        } else {
            None
        }
    }

    pub(super) fn rebuild_placement_map(&mut self) {
        RoomPlacement::build_placement_map(
            &mut self.placement_map,
            self.room_bounds,
            &self.objects,
        );
    }

    fn can_place_object(&self, obj: &ObjectPlacement) -> bool {
        for action in &obj.actions.0 {
            if let ObjectPlacementAction::PlacementBound { location, size } = *action {
                let min_x = (location.0.max(self.room_bounds.min.x as f32) * 4.0).floor() as usize;
                let min_y = (location.1.max(self.room_bounds.min.y as f32) * 4.0).floor() as usize;
                let max_x = ((location.0 + size.0).min(self.room_bounds.max.x as f32 + 1.0) * 4.0)
                    .ceil() as usize;
                let max_y = ((location.1 + size.1).min(self.room_bounds.max.y as f32 + 1.0) * 4.0)
                    .ceil() as usize;
                for y in min_y..max_y {
                    for x in min_x..max_x {
                        let lx = x - (self.room_bounds.min.x * 4) as usize;
                        let ly = y - (self.room_bounds.min.y * 4) as usize;
                        if self
                            .placement_map
                            .get(lx + ly * (self.room_bounds.width() as usize * 4))
                        {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    /// Replaces the object with the given id into the room
    pub fn replace_object<EC: EntityCreator>(
        &mut self,
        entities: &mut Container,
        object_id: usize,
        obj: ObjectPlacement,
    ) {
        let id = self.id;
        let rev = assume!(
            self.log,
            obj.apply::<_, EC>(&self.log.clone(), self, entities, id, false)
        );
        self.objects[object_id] = Some((obj, rev));
        self.rebuild_placement_map();
        self.dirty = true;
    }
}

impl LevelView for RoomVirtualLevel {
    fn get_tile(&self, loc: Location) -> Arc<tile::Type> {
        RoomVirtualLevel::get_tile(self, loc)
    }

    fn get_tile_raw(&self, loc: Location) -> Option<TileData> {
        if self.bounds.in_bounds(loc) {
            Some(self.tiles[Self::location_index(loc - self.offset, self.width)])
        } else {
            None
        }
    }

    fn get_room_owner(&self, loc: Location) -> Option<room::Id> {
        RoomVirtualLevel::get_room_owner(self, loc)
    }
    fn get_tile_flags(&self, loc: Location) -> TileFlag {
        RoomVirtualLevel::get_tile_flags(self, loc)
    }
    fn get_wall_info(&self, loc: Location, dir: Direction) -> Option<WallInfo> {
        RoomVirtualLevel::get_wall_info(self, loc, dir)
    }

    fn get_asset_manager(&self) -> AssetManager {
        self.asset_manager.clone()
    }

    fn get_window(&self, id: u8) -> ResourceKey<'static> {
        self.window_type[id as usize].clone()
    }
}

impl LevelAccess for RoomVirtualLevel {
    fn set_tile(&mut self, loc: Location, tile: ResourceKey<'_>) {
        RoomVirtualLevel::set_tile(self, loc, tile);
    }
    fn set_wall_info(&mut self, loc: Location, dir: Direction, info: Option<WallInfo>) {
        RoomVirtualLevel::set_wall_info(self, loc, dir, info)
    }

    fn set_tile_raw(&mut self, loc: Location, tile: TileData) {
        if self.bounds.in_bounds(loc) {
            self.tiles[Self::location_index(loc - self.offset, self.width)] = tile;
            // Recompute walls
            for dir in &ALL_DIRECTIONS {
                let was_wall = self.get_wall_info(loc, *dir).is_some();
                let is_wall = {
                    self.get_tile(loc).should_place_wall(self, loc, *dir)
                        || self.get_tile(loc.shift(*dir)).should_place_wall(
                            self,
                            loc.shift(*dir),
                            dir.reverse(),
                        )
                };
                if was_wall != is_wall {
                    if is_wall {
                        self.set_wall_info(
                            loc,
                            *dir,
                            Some(WallInfo {
                                flag: TileWallFlag::None,
                            }),
                        );
                    } else {
                        self.set_wall_info(loc, *dir, None);
                    }
                }
            }
            self.dirty = true;
        }
    }

    fn set_tile_flags(&mut self, loc: Location, flags: TileFlag) {
        RoomVirtualLevel::set_tile_flags(self, loc, flags);
    }

    fn get_or_create_window(&mut self, key: ResourceKey<'_>) -> u8 {
        if let Some(id) = self.window_type_map.get(&key).cloned() {
            return id;
        }
        let id = self.window_type.len();
        self.window_type_map
            .insert(key.borrow().into_owned(), id as u8);
        self.window_type.push(key.into_owned());
        id as u8
    }
}

pub(super) struct VirtualPlacer {
    pub log: Logger,
    pub rooms: Rc<RefCell<super::LevelRooms>>,
    pub id: room::Id,
}

macro_rules! virt_placer_ref {
    ($self:ident) => {{
        let rooms = $self.rooms.borrow();
        let room = ::std::cell::Ref::map(rooms, |rooms| rooms.get_room_info($self.id));
        ::std::cell::Ref::map(room, |room| {
            assume!($self.log, room.building_level.as_ref())
        })
    }};
    (mut $self:ident) => {{
        let rooms = $self.rooms.borrow_mut();
        let room = ::std::cell::RefMut::map(rooms, |rooms| rooms.get_room_info_mut($self.id));
        ::std::cell::RefMut::map(room, |room| {
            assume!($self.log, room.building_level.as_mut())
        })
    }};
}

impl ObjectPlaceable for VirtualPlacer {
    fn id(&self) -> room::Id {
        self.id
    }

    fn bounds(&self) -> Bound {
        let room = virt_placer_ref!(self);
        room.room_bounds
    }

    fn should_lower_walls(&mut self, flag: bool) {
        let mut room = virt_placer_ref!(mut self);
        room.should_lower_walls = flag;
    }

    fn set_placement(&mut self, placement: TempObjectPlacement) {
        let mut room = virt_placer_ref!(mut self);
        room.object_placement = Some(placement);
    }

    fn get_placement_position(&self) -> Option<(f32, f32)> {
        let room = virt_placer_ref!(self);
        room.object_placement.as_ref().map(|v| v.position)
    }

    fn take_placement(&mut self) -> Option<TempObjectPlacement> {
        let mut room = virt_placer_ref!(mut self);
        room.object_placement.take()
    }

    fn can_place_object(&self, obj: &ObjectPlacement) -> bool {
        let room = virt_placer_ref!(self);
        RoomVirtualLevel::can_place_object(&*room, obj)
    }

    fn flag_dirty(&mut self) {
        let mut room = virt_placer_ref!(mut self);
        room.dirty = true;
    }

    fn rebuild_placement_map(&mut self) {
        let mut room = virt_placer_ref!(mut self);
        RoomVirtualLevel::rebuild_placement_map(&mut *room);
    }

    fn place_object(&mut self, placement: TempObjectPlacement) -> UResult<usize> {
        let mut room = virt_placer_ref!(mut self);
        let id = room.objects.iter().position(|v| v.is_none());
        let rev = placement.placement_rev.expect("Missing placement");
        let pm = Some((placement.placement.expect("Missing placement"), rev));
        if let Some(id) = id {
            *room.objects.get_mut(id).unwrap() = pm;
            room.rebuild_placement_map();
            Ok(id)
        } else {
            let id = room.objects.len();
            room.objects.push(pm);
            room.rebuild_placement_map();
            Ok(id)
        }
    }

    fn is_virtual() -> bool {
        true
    }

    fn remove_object<EC: EntityCreator>(
        &mut self,
        entities: &mut Container,
        object_id: usize,
    ) -> UResult<ObjectPlacement> {
        use std::mem;
        let mut objects = {
            let mut room = virt_placer_ref!(mut self);
            mem::replace(&mut room.objects, Vec::new())
        };
        let ret = Level::try_remove_object::<_, EC>(
            &self.log,
            &mut Level::virt_placer(&self.log, &self.rooms, self.id),
            &mut objects,
            entities,
            self.id,
            object_id,
        );
        let mut room = virt_placer_ref!(mut self);
        room.objects = objects;
        room.rebuild_placement_map();
        room.dirty = true;
        ret
    }
    fn replace_object<EC: EntityCreator>(
        &mut self,
        entities: &mut Container,
        object_id: usize,
        obj: ObjectPlacement,
    ) {
        let mut room = virt_placer_ref!(mut self);
        RoomVirtualLevel::replace_object::<EC>(&mut *room, entities, object_id, obj)
    }
    fn get_remove_target(&self, loc: Location) -> Option<usize> {
        let room = virt_placer_ref!(self);
        placement::find_remove_target(&room.objects, loc)
    }
}

impl LevelView for VirtualPlacer {
    fn get_tile(&self, loc: Location) -> Arc<tile::Type> {
        let room = virt_placer_ref!(self);
        room.get_tile(loc)
    }

    fn get_tile_raw(&self, loc: Location) -> Option<TileData> {
        let room = virt_placer_ref!(self);
        room.get_tile_raw(loc)
    }

    fn get_room_owner(&self, loc: Location) -> Option<room::Id> {
        let room = virt_placer_ref!(self);
        room.get_room_owner(loc)
    }
    fn get_tile_flags(&self, loc: Location) -> TileFlag {
        let room = virt_placer_ref!(self);
        room.get_tile_flags(loc)
    }
    fn get_wall_info(&self, loc: Location, dir: Direction) -> Option<WallInfo> {
        let room = virt_placer_ref!(self);
        room.get_wall_info(loc, dir)
    }

    fn get_asset_manager(&self) -> AssetManager {
        let room = virt_placer_ref!(self);
        room.asset_manager.clone()
    }

    fn get_window(&self, id: u8) -> ResourceKey<'static> {
        let room = virt_placer_ref!(self);
        room.get_window(id)
    }
}

impl LevelAccess for VirtualPlacer {
    fn set_tile(&mut self, loc: Location, tile: ResourceKey<'_>) {
        let mut room = virt_placer_ref!(mut self);
        room.set_tile(loc, tile)
    }
    fn set_wall_info(&mut self, loc: Location, dir: Direction, info: Option<WallInfo>) {
        let mut room = virt_placer_ref!(mut self);
        room.set_wall_info(loc, dir, info)
    }

    fn set_tile_raw(&mut self, loc: Location, tile: TileData) {
        let mut room = virt_placer_ref!(mut self);
        room.set_tile_raw(loc, tile)
    }

    fn set_tile_flags(&mut self, loc: Location, flags: TileFlag) {
        let mut room = virt_placer_ref!(mut self);
        room.set_tile_flags(loc, flags)
    }

    fn get_or_create_window(&mut self, key: ResourceKey<'_>) -> u8 {
        let mut room = virt_placer_ref!(mut self);
        room.get_or_create_window(key)
    }
}
