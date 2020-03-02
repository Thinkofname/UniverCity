use super::super::*;

impl Level {
    /// Places the room back into a planning state from building
    pub fn undo_placement<P: player::Player, EC: EntityCreator>(
        &mut self,
        p: &mut P,
        entities: &mut Container,
        room_id: room::Id,
    ) -> room::Id {
        use std::mem;
        self.cancel_object_placement::<EC>(room_id, entities);
        let mut rooms = self.rooms.borrow_mut();
        let mut room = assume!(self.log, rooms.rooms.remove(room_id));
        rooms.room_order.retain(|v| *v != room_id);
        let room_id = room::Id(-p.get_uid().0);
        room.id = room_id;
        room.state = RoomState::Planning;
        if !room.controller.is_invalid() {
            entities.remove_entity(room.controller);
            room.controller = Entity::INVALID;
        }
        {
            let virt = assume!(self.log, room.building_level.as_mut());
            virt.dirty = true;
            for obj in mem::replace(&mut virt.objects, vec![]).into_iter().rev() {
                if let Some(obj) = obj {
                    assume!(self.log, obj.1.apply::<_, EC>(&self.log, virt, entities));
                }
            }
            virt.placement_map.clear();
            for loc in room.area {
                let idx = RoomVirtualLevel::location_index(loc - virt.offset, virt.width);
                // Take ownership of the tile.
                let td = &mut virt.tiles[idx];
                td.owner = Some(room.id);
            }
        }
        let mut tiles = self.tiles.borrow_mut();
        for loc in room.area {
            let td = &mut tiles.tiles[LevelTiles::location_index(loc, self.width)];
            td.owner = Some(room.id);
        }
        rooms.room_order.push(room.id);
        rooms.rooms.insert(room.id, room);
        room_id
    }

    /// Marks the room as built and allows it to be used
    pub fn finalize_room<EC: EntityCreator, E: Invokable>(
        &mut self,
        engine: &E,
        entities: &mut Container,
        room_id: room::Id,
    ) -> UResult<()> {
        use std::mem;
        let log = self.log.clone();
        self.cancel_object_placement::<EC>(room_id, entities);
        let (area, mut virt, objs) = {
            let mut rooms = self.rooms.borrow_mut();
            let room = assume!(self.log, rooms.rooms.get_mut(room_id));
            room.state = RoomState::Done;
            let virt = assume!(self.log, room.building_level.take());
            let area = room.area;
            (area, *virt, mem::replace(&mut room.objects, vec![]))
        };
        // Remove existing objects
        for obj in objs.into_iter().rev() {
            if let Some(obj) = obj {
                obj.1.apply::<_, EC>(&log, self, entities)?;
            }
        }
        // Same for the virtual level
        let mut new_objs = Vec::with_capacity(virt.objects.len());
        for obj in mem::replace(&mut virt.objects, vec![])
            .into_iter()
            .rev()
            .filter_map(|v| v)
        {
            new_objs.push(obj.0);
            obj.1.apply::<_, EC>(&log, &mut virt, entities)?;
        }
        new_objs.reverse();

        {
            let mut tiles = self.tiles.borrow_mut();
            let keys = virt.tile_map.keys().collect::<Vec<_>>();
            for id in keys {
                tiles
                    .tile_map
                    .insert(id, assume!(self.log, virt.tile_map.remove(id)));
            }
            for loc in area {
                let idx = LevelTiles::location_index(loc, self.width);
                let mut td =
                    virt.tiles[RoomVirtualLevel::location_index(loc - virt.offset, virt.width)];
                // Update the owner
                td.owner = Some(room_id);
                td.flags.remove(TileFlag::BUILDING);
                tiles.tiles[idx] = td;
                for dir in &ALL_DIRECTIONS {
                    tiles.set_wall_info(loc, *dir, virt.get_wall_info(loc, *dir));
                }
            }
        }
        // Apply objects to the real level
        let (info, area) = {
            let mut applied_objs = Vec::with_capacity(new_objs.len());
            for obj in new_objs {
                let rev = assume!(
                    self.log,
                    obj.apply::<_, EC>(&log, self, entities, room_id, false)
                );
                applied_objs.push(Some((obj, rev)));
            }
            let mut rooms = self.rooms.borrow_mut();
            let room = assume!(self.log, rooms.rooms.get_mut(room_id));
            room.objects = applied_objs;
            // Reuse the allocation from the virtual room but
            // recompute to be safe.
            room.placement_map = virt.placement_map;
            room.rebuild_object_maps();

            // Create the room controller
            if room.controller.is_invalid() {
                let controller = entities.new_entity();
                entities.add_component(
                    controller,
                    RoomController {
                        room_id: room.id,
                        active: false,
                        waiting_list: vec![],
                        potential_list: Default::default(),
                        entities: vec![],
                        visitors: vec![],
                        timetabled_visitors: Default::default(),
                        active_staff: None,
                        capacity: 0,
                        script_requests: FNVMap::default(),
                        ticks_missing_staff: 0,
                        have_warned_missing: false,
                    },
                );
                entities.add_component(
                    controller,
                    Owned {
                        player_id: room.owner,
                    },
                );
                room.controller = controller;
            }
            room.needs_update = true;
            (
                assume!(
                    self.log,
                    self.asset_manager
                        .loader_open::<room::Loader>(room.key.borrow())
                ),
                room.area,
            )
        };

        self.do_update_room::<EC, _>(engine, entities, room_id);

        let cost = info.cost_for_room(self, room_id);
        {
            let mut rooms = self.rooms.borrow_mut();
            let room = assume!(self.log, rooms.rooms.get_mut(room_id));
            room.placement_cost = cost;
        }
        {
            let mut tiles = self.tiles.borrow_mut();
            tiles.update_walls(virt.bounds);
        }
        self.rebuild_path_sections(area);
        Ok(())
    }
}
