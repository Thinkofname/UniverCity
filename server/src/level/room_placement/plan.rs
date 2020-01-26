use super::super::*;

use crate::util::BitSet;

impl Level {

    /// Builds the requested room at the target location if possible
    pub fn place_room<'a, EC, E, P>(
            &mut self,
            engine: &E, entities: &mut Container,
            p: &P, room_key: ResourceKey<'a>, bound: Bound
    ) -> Option<room::Id>
        where E: Invokable,
              P: player::Player,
              EC: EntityCreator,
    {
        self.place_room_id::<EC, _>(engine, entities, room::Id(-p.get_uid().0), p.get_uid(), room_key, bound)
    }

    /// Builds the requested room at the target location if possible
    /// with the given id
    pub fn place_room_id<EC, E>(
            &mut self,
            engine: &E, entities: &mut Container,
            id: room::Id, owner_id: player::Id,
            room_key: ResourceKey<'_>, bound: Bound
    ) -> Option<room::Id>
        where E: Invokable,
              EC: EntityCreator,
    {
        if !self.can_place_room(owner_id, room_key.borrow(), bound) {
            return None;
        }
        let mut virt = self.capture_area(id, bound);
        if self.build_room(engine, &mut virt, id, room_key.borrow(), bound).is_err() {
            return None;
        }
        let original_tiles = self.clear_room_area(bound, id);
        // Add our room
        {
            let mut rooms = self.rooms.borrow_mut();
            rooms.room_order.push(id);
            rooms.rooms.insert(id, RoomPlacement {
                id,
                owner: owner_id,
                area: bound,
                key: room_key.into_owned(),
                state: RoomState::Planning,
                building_level: Some(Box::new(virt)),
                objects: vec![],
                controller: Entity::INVALID,
                placement_map: BitSet::new(0),
                collision_map: BitSet::new(0),
                blocked_map: BitSet::new(0),
                needs_update: true,
                original_tiles,
                temp_placement: None,
                limited_editing: false,
                lower_walls: false,
                placement_cost: UniDollar(0),
                tile_update_state: None,
                required_tiles: vec![],
            });
        }

        self.do_update_room::<EC, _>(engine, entities, id);
        Some(id)
    }

    /// Attempts to resize the room with the given id with the new size
    pub fn resize_room_id<EC, E>(
            &mut self,
            engine: &E, entities: &mut Container,
            owner: player::Id, room_id: room::Id, bound: Bound
    ) -> bool
        where E: Invokable,
              EC: EntityCreator,
    {
        use std::mem::replace;
        let (mut original_tiles, orig_bound, key) = {
            let mut rooms = self.rooms.borrow_mut();
            let room = assume!(self.log, rooms.rooms.get_mut(room_id));
            (replace(&mut room.original_tiles, vec![]), room.area, room.key.clone())
        };
        {
            let mut tiles = self.tiles.borrow_mut();
            for loc in orig_bound {
                tiles.tiles[LevelTiles::location_index(loc, self.width)] = original_tiles.remove(0);
                tiles.flag_dirty(loc.x, loc.y);
            }
        }
        let selected_bound = if self.can_place_room(owner, key.borrow(), bound) {
            bound
        } else {
            orig_bound
        };

        {
            let mut virt = self.capture_area(room_id, selected_bound);
            assume!(self.log, self.build_room(engine, &mut virt, room_id, key, selected_bound));
            let original_tiles = self.clear_room_area(selected_bound, room_id);
            let mut rooms = self.rooms.borrow_mut();
            let room = assume!(self.log, rooms.rooms.get_mut(room_id));
            room.area = selected_bound;
            room.state = RoomState::Planning;
            room.original_tiles = original_tiles;
            room.building_level = Some(Box::new(virt));
            room.tile_update_state = None;
        }

        self.do_update_room_area::<EC, _>(engine, entities, Some(room_id), orig_bound);
        self.do_update_room_area::<EC, _>(engine, entities, Some(room_id), bound);

        selected_bound == bound
    }

    /// Cancels the placement of the room returning the area back to its
    /// original state
    pub fn cancel_placement<EC, E>(&mut self, engine: &E, entities: &mut Container, room_id: room::Id) -> RoomPlacement
        where EC: EntityCreator,
              E: Invokable,
    {
        let room = {
            let mut rooms = self.rooms.borrow_mut();
            let mut tiles = self.tiles.borrow_mut();
            let mut room = assume!(self.log, rooms.rooms.remove(room_id));
            rooms.room_order.retain(|v| *v != room_id);
            for loc in room.area {
                tiles.tiles[LevelTiles::location_index(loc, self.width)] = room.original_tiles.remove(0);
                tiles.flag_dirty(loc.x, loc.y);
            }
            room
        };
        self.do_update_room_area::<EC, _>(engine, entities, None, room.area);
        room
    }

    /// Finalizes the placement of the room allowing it to be modified with
    /// doors and windows and other features.
    pub fn finalize_placement(&mut self, room_id: room::Id) -> room::Id {
        let mut rooms = self.rooms.borrow_mut();
        // Find a free room id.
        let mut id = room::Id(0);
        if room_id.0 < 0 {
            while rooms.rooms.get(id).is_some() {
                id.0 += 1;
            }
        } else {
            id = room_id;
        }
        rooms.room_order.retain(|v| *v != room_id);

        // Move the room to its new id
        let mut room = assume!(self.log, rooms.rooms.remove(room_id));
        room.id = id;
        // Swap the state to building
        room.state = RoomState::Building;
        {
            let lvl = assume!(self.log, room.building_level.as_mut());
            lvl.dirty = true; // Force a redraw
            lvl.id = id; // Update the room id
            // Update the ownership to the new id
            for loc in room.area {
                let idx = RoomVirtualLevel::location_index(loc - lvl.offset, lvl.width);
                // Take ownership of the tile.
                let td = &mut lvl.tiles[idx];
                td.owner = Some(id);
            }
            let mut tiles = self.tiles.borrow_mut();
            for loc in room.area {
                let idx = LevelTiles::location_index(loc, self.width);
                let td = &mut tiles.tiles[idx];
                td.owner = Some(id);
            }
        }
        // Insert with the new id
        rooms.room_order.push(id);
        rooms.rooms.insert(id, room);
        id
    }

    // Clears the area for building
    fn clear_room_area(&mut self, bound: Bound, room_id: room::Id) -> Vec<TileData> {
        let mut original_tiles = vec![];
        let mut tiles = self.tiles.borrow_mut();
        for loc in bound {
            tiles.flag_dirty(loc.x, loc.y);
            let idx = LevelTiles::location_index(loc, self.width);
            original_tiles.push(tiles.tiles[idx]);
            // Take ownership of the tile.
            let td = &mut tiles.tiles[idx];
            td.flags = TileFlag::BUILDING;
            td.owner = Some(room_id);
        }
        original_tiles
    }

    fn build_room<E>(
            &mut self,
            engine: &E,
            lvl: &mut RoomVirtualLevel, id: room::Id,
            room_key: ResourceKey<'_>, bound: Bound,
    ) -> UResult<()>
        where E: Invokable,
    {
        use lua::Ref;
        // Build the room

        let room: &room::Room = &*self.asset_manager.loader_open::<room::Loader>(room_key.borrow())?;
        let border = room.border_tile.as_ref().map_or(room.tile.borrow(), |v| v.borrow());
        lvl.has_border = room.border_tile.is_some();
        for loc in bound {
            let idx = RoomVirtualLevel::location_index(loc - lvl.offset, lvl.width);
            let tile = {
                let orig = lvl.get_tile(loc);
                let orig = &orig.key;
                // If the tile was our original floor tile (not border)
                // then we shouldn't replace it as it would introduce an
                // a break in the room.
                // This case will only happen when a room is extendable.
                if (loc.x == bound.min.x || loc.y == bound.min.y || loc.x == bound.max.x || loc.y == bound.max.y) && (*orig != room.tile){
                    border.borrow()
                } else {
                    room.tile.borrow()
                }
            };
            lvl.set_tile(loc, tile);
            // Take ownership of the tile.
            let td = &mut lvl.tiles[idx];
            td.owner = Some(id);
        }
        // Support lua scripts for complex placements
        if let Some(placer_script) = room.tile_placer.as_ref() {
            struct Placer {
                bound: Bound,
                tiles: Vec<(Location, Ref<String>)>,
            }

            #[allow(clippy::needless_pass_by_value)] // Lua requirements
            fn set_tile(_: &lua::Lua, p: Ref<RefCell<Placer>>, x: i32, y: i32, tile: Ref<String>) -> UResult<()> {
                let loc = Location::new(x, y);
                let mut p = p.borrow_mut();
                if !p.bound.in_bounds(loc) {
                    bail!("location not within room bounds");
                }
                p.tiles.push((loc, tile));
                Ok(())
            }

            impl lua::LuaUsable for Placer {
                fn fields(t: &lua::TypeBuilder) {
                    t.field("set_tile", lua::closure4(set_tile));
                }
            }

            let placer = lua::Ref::new(engine, RefCell::new(Placer {
                bound: Bound::new(
                    Location::new(0, 0),
                    Location::new(bound.width(), bound.height()),
                ),
                tiles: vec![],
            }));

            engine.invoke_function::<(Ref<String>, Ref<String>, Ref<String>, lua::Ref<_>, i32, i32), ()>("invoke_module_method", (
                Ref::new_string(engine, placer_script.0.module()),
                Ref::new_string(engine, placer_script.0.resource()),
                Ref::new_string(engine, &*placer_script.1),
                placer.clone(),
                bound.width(), bound.height(),
            ))?;

            let p = placer.borrow();
            for tile_info in &p.tiles {
                let tile = LazyResourceKey::parse(&*tile_info.1)
                    .or_module(room_key.module_key());
                let loc = tile_info.0.offset(bound.min.x, bound.min.y);
                lvl.set_tile(loc, tile);
                // Take ownership of the tile.
                let idx = RoomVirtualLevel::location_index(loc - lvl.offset, lvl.width);
                let td = &mut lvl.tiles[idx];
                td.owner = Some(id);
            }
        }
        // Compute walls
        for loc in lvl.bounds {
            for dir in &ALL_DIRECTIONS {
                let was_wall = lvl.get_wall_info(loc, *dir).is_some();
                let is_wall = {
                    lvl.get_tile(loc).should_place_wall(lvl, loc, *dir)
                        || lvl.get_tile(loc.shift(*dir)).should_place_wall(lvl, loc.shift(*dir), dir.reverse())
                };
                if was_wall != is_wall {
                    if is_wall {
                        lvl.set_wall_info(loc, *dir, Some(WallInfo {
                            flag: TileWallFlag::None,
                        }));
                    } else {
                        lvl.set_wall_info(loc, *dir, None);
                    }
                }
            }
        }
        Ok(())
    }
}