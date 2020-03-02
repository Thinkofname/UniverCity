use super::object::{
    self, ObjectPlacement, ObjectPlacementAction, ReverseObjectPlacement, WallPlacementFlag,
};
use super::room;
use crate::assets;
use crate::ecs;
use crate::entity;
use crate::errors;
use crate::prelude::*;
use crate::script;
use crate::util::*;
use lua;

use std::cell::RefCell;

pub(super) struct TempObjectPlacement {
    pub(super) key: assets::ResourceKey<'static>,
    pub(super) position: (f32, f32),
    pub(super) placement: Option<ObjectPlacement>,
    pub(super) placement_rev: Option<ReverseObjectPlacement>,
    pub(super) valid: bool,
}

impl TempObjectPlacement {
    pub(super) fn replaces_floor(&self) -> Option<Location> {
        self.placement
            .iter()
            .flat_map(|v| v.actions.0.iter())
            .filter_map(|v| {
                if let ObjectPlacementAction::Tile {
                    location,
                    floor_replacement,
                    ..
                } = *v
                {
                    if floor_replacement {
                        Some(location)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .next()
    }
}

pub(super) trait ObjectPlaceable: super::LevelAccess + Sized {
    fn id(&self) -> room::Id;
    fn bounds(&self) -> Bound;
    fn should_lower_walls(&mut self, flag: bool);
    fn set_placement(&mut self, placement: TempObjectPlacement);
    fn get_placement_position(&self) -> Option<(f32, f32)>;
    fn take_placement(&mut self) -> Option<TempObjectPlacement>;
    fn can_place_object(&self, obj: &ObjectPlacement) -> bool;
    fn flag_dirty(&mut self);
    fn place_object(&mut self, placement: TempObjectPlacement) -> errors::Result<usize>;
    fn is_virtual() -> bool;
    fn rebuild_placement_map(&mut self);

    fn get_remove_target(&self, loc: Location) -> Option<usize>;
    fn remove_object<EC: EntityCreator>(
        &mut self,
        entities: &mut Container,
        object_id: usize,
    ) -> UResult<ObjectPlacement>;
    fn replace_object<EC: EntityCreator>(
        &mut self,
        entities: &mut Container,
        object_id: usize,
        obj: ObjectPlacement,
    );

    /// Starts trying to place the named object
    fn begin_object_placement<'b, E, EntityCreator>(
        &mut self,
        log: &Logger,
        asset_manager: &assets::AssetManager,
        engine: &E,
        entities: &mut ecs::Container,
        obj: assets::ResourceKey<'b>,
        version: Option<i32>,
    ) -> UResult<()>
    where
        E: script::Invokable,
        EntityCreator: entity::EntityCreator,
    {
        self.cancel_object_placement::<EntityCreator>(log, entities);
        {
            let obj = asset_manager.loader_open::<object::Loader>(obj.borrow())?;
            self.should_lower_walls(obj.lower_walls_placement);
        }
        let bound = self.bounds();
        let pos = (
            bound.min.x as f32 + (bound.width() as f32 / 2.0),
            bound.min.y as f32 + (bound.height() as f32 / 2.0),
        );
        self.set_placement(TempObjectPlacement {
            key: obj.borrow().into_owned(),
            position: pos,
            placement: None,
            placement_rev: None,
            valid: false,
        });
        let _ = self.move_active_object::<_, EntityCreator>(
            log,
            asset_manager,
            engine,
            entities,
            pos,
            version,
            0,
        );
        Ok(())
    }

    /// Moves the active object to new location. If the location is not
    /// valid this returns an error but the object will still be moved
    /// it'll just be unplaceable
    #[allow(clippy::needless_pass_by_value)] // Lua bindings
    fn move_active_object<E, EntityCreator>(
        &mut self,
        log: &Logger,
        asset_manager: &assets::AssetManager,
        engine: &E,
        entities: &mut ecs::Container,
        pos: (f32, f32),
        version: Option<i32>,
        rotation: i16,
    ) -> errors::Result<()>
    where
        E: script::Invokable,
        EntityCreator: entity::EntityCreator,
    {
        use lua::{Ref, Scope};
        if let Some(mut placement) = self.take_placement() {
            placement.position = pos;
            let obj = asset_manager.loader_open::<object::Loader>(placement.key.borrow())?;

            // Prevent it from seeing its past self
            if let Some(old) = placement.placement_rev.take() {
                old.apply::<_, EntityCreator>(log, self, entities)
                    .expect("Failed to reverse previous placement, this shouldn't happen");
            }

            struct Placer {
                room_id: room::Id,
                obj: assets::ResourceKey<'static>,
                failed: Option<String>,
                placement: Option<ObjectPlacement>,
                bounds: Bound,
                parameters: Ref<lua::Table>,
                remove_on_error: bool,
            }

            fn fail(_: &lua::Lua, p: Ref<RefCell<Placer>>, res: Ref<String>) {
                let mut placer = p.borrow_mut();
                placer.failed = Some(res.to_string());
            }

            #[allow(clippy::unit_arg)]
            fn place_window(
                _: &lua::Lua,
                p: Ref<RefCell<Placer>>,
                x: i32,
                y: i32,
                dir: Ref<String>,
                kind: Ref<String>,
            ) -> errors::Result<()> {
                let mut placer = p.borrow_mut();
                let kind = assets::LazyResourceKey::parse(&kind)
                    .or_module(placer.obj.module_key())
                    .into_owned();
                placer
                    .placement
                    .as_mut()
                    .ok_or(errors::ErrorKind::InvalidState)
                    .and_then_into(|v| {
                        Ok(v.actions.0.push(ObjectPlacementAction::WallFlag {
                            location: Location::new(x, y),
                            direction: Direction::from_str(&dir)?,
                            flag: WallPlacementFlag::Window { key: kind },
                        }))
                    })
            }

            fn absolute_name(
                lua: &lua::Lua,
                p: Ref<RefCell<Placer>>,
                res: Ref<String>,
            ) -> Ref<String> {
                let placer = p.borrow();
                let res = assets::LazyResourceKey::parse(&res).or_module(placer.obj.module_key());
                Ref::new_string(lua, res.as_string())
            }

            #[allow(clippy::unit_arg)]
            fn set_floor(
                _: &lua::Lua,
                p: Ref<RefCell<Placer>>,
                x: i32,
                y: i32,
                tile: Ref<String>,
            ) -> errors::Result<()> {
                let mut placer = p.borrow_mut();
                let tile = assets::LazyResourceKey::parse(&tile)
                    .or_module(placer.obj.module_key())
                    .into_owned();
                placer
                    .placement
                    .as_mut()
                    .ok_or(errors::ErrorKind::InvalidState)
                    .and_then_into(|v| {
                        Ok(v.actions.0.push(ObjectPlacementAction::Tile {
                            location: Location::new(x, y),
                            key: tile,
                            floor_replacement: true,
                        }))
                    })
            }

            #[allow(clippy::unit_arg)]
            fn set_tile(
                _: &lua::Lua,
                p: Ref<RefCell<Placer>>,
                x: i32,
                y: i32,
                tile: Ref<String>,
            ) -> errors::Result<()> {
                let mut placer = p.borrow_mut();
                let tile = assets::LazyResourceKey::parse(&tile)
                    .or_module(placer.obj.module_key())
                    .into_owned();
                placer
                    .placement
                    .as_mut()
                    .ok_or(errors::ErrorKind::InvalidState)
                    .and_then_into(|v| {
                        Ok(v.actions.0.push(ObjectPlacementAction::Tile {
                            location: Location::new(x, y),
                            key: tile,
                            floor_replacement: false,
                        }))
                    })
            }

            #[allow(clippy::unit_arg)]
            fn set_no_walls(
                _: &lua::Lua,
                p: Ref<RefCell<Placer>>,
                x: i32,
                y: i32,
            ) -> errors::Result<()> {
                let mut placer = p.borrow_mut();
                placer
                    .placement
                    .as_mut()
                    .ok_or(errors::ErrorKind::InvalidState)
                    .and_then_into(|v| {
                        Ok(v.actions.0.push(ObjectPlacementAction::TileFlag {
                            location: Location::new(x, y),
                            flag: TileFlag::NO_WALLS,
                        }))
                    })
            }

            #[allow(clippy::unit_arg)]
            fn mark_door(
                _: &lua::Lua,
                p: Ref<RefCell<Placer>>,
                x: i32,
                y: i32,
                dir: Ref<String>,
            ) -> errors::Result<()> {
                let mut placer = p.borrow_mut();
                placer
                    .placement
                    .as_mut()
                    .ok_or(errors::ErrorKind::InvalidState)
                    .and_then_into(|v| {
                        Ok(v.actions.0.push(ObjectPlacementAction::WallFlag {
                            location: Location::new(x, y),
                            direction: Direction::from_str(&dir)?,
                            flag: WallPlacementFlag::Door,
                        }))
                    })
            }

            #[allow(clippy::unit_arg)]
            fn static_model(
                _: &lua::Lua,
                p: Ref<RefCell<Placer>>,
                name: Ref<String>,
                x: f64,
                y: f64,
                z: f64,
                dir: f64,
            ) -> errors::Result<()> {
                let mut placer = p.borrow_mut();
                let obj = assets::LazyResourceKey::parse(&name)
                    .or_module(placer.obj.module_key())
                    .into_owned();
                placer
                    .placement
                    .as_mut()
                    .ok_or(errors::ErrorKind::InvalidState)
                    .and_then_into(|v| {
                        Ok(v.actions.0.push(ObjectPlacementAction::StaticModel {
                            location: object::Loc3D(x as f32, y as f32, z as f32),
                            rotation: Angle::new(dir as f32),
                            object: obj,
                            texture: None,
                        }))
                    })
            }

            #[allow(clippy::unit_arg)]
            fn static_model_tex(
                _: &lua::Lua,
                p: Ref<RefCell<Placer>>,
                name: Ref<String>,
                texture: Ref<String>,
                x: f64,
                y: f64,
                z: f64,
                dir: f64,
            ) -> errors::Result<()> {
                let mut placer = p.borrow_mut();
                let obj = assets::LazyResourceKey::parse(&name)
                    .or_module(placer.obj.module_key())
                    .into_owned();
                let texture = assets::LazyResourceKey::parse(&texture)
                    .or_module(placer.obj.module_key())
                    .into_owned();
                placer
                    .placement
                    .as_mut()
                    .ok_or(errors::ErrorKind::InvalidState)
                    .and_then_into(|v| {
                        Ok(v.actions.0.push(ObjectPlacementAction::StaticModel {
                            location: object::Loc3D(x as f32, y as f32, z as f32),
                            rotation: Angle::new(dir as f32),
                            object: obj,
                            texture: Some(texture),
                        }))
                    })
            }

            #[allow(clippy::unit_arg)]
            fn animated_model(
                _: &lua::Lua,
                p: Ref<RefCell<Placer>>,
                name: Ref<String>,
                animation: Ref<String>,
                x: f64,
                y: f64,
                z: f64,
                dir: f64,
            ) -> errors::Result<()> {
                let mut placer = p.borrow_mut();
                let obj = assets::LazyResourceKey::parse(&name)
                    .or_module(placer.obj.module_key())
                    .into_owned();
                placer
                    .placement
                    .as_mut()
                    .ok_or(errors::ErrorKind::InvalidState)
                    .and_then_into(|v| {
                        Ok(v.actions.0.push(ObjectPlacementAction::AnimatedModel {
                            location: object::Loc3D(x as f32, y as f32, z as f32),
                            rotation: Angle::new(dir as f32),
                            object: obj,
                            texture: None,
                            animation: animation.to_string(),
                        }))
                    })
            }

            #[allow(clippy::unit_arg)]
            fn animated_model_tex(
                _: &lua::Lua,
                p: Ref<RefCell<Placer>>,
                name: Ref<String>,
                texture: Ref<String>,
                animation: Ref<String>,
                x: f64,
                y: f64,
                z: f64,
                dir: f64,
            ) -> errors::Result<()> {
                let mut placer = p.borrow_mut();
                let obj = assets::LazyResourceKey::parse(&name)
                    .or_module(placer.obj.module_key())
                    .into_owned();
                let texture = assets::LazyResourceKey::parse(&texture)
                    .or_module(placer.obj.module_key())
                    .into_owned();
                placer
                    .placement
                    .as_mut()
                    .ok_or(errors::ErrorKind::InvalidState)
                    .and_then_into(|v| {
                        Ok(v.actions.0.push(ObjectPlacementAction::AnimatedModel {
                            location: object::Loc3D(x as f32, y as f32, z as f32),
                            rotation: Angle::new(dir as f32),
                            object: obj,
                            texture: Some(texture),
                            animation: animation.to_string(),
                        }))
                    })
            }

            fn owns_tile(lua: &lua::Lua, p: Ref<RefCell<Placer>>, x: i32, y: i32) -> UResult<bool> {
                let loc = Location::new(x, y);
                let placer = p.borrow();
                let tiles = lua
                    .get_tracked::<LevelTiles>()
                    .ok_or_else(|| ErrorKind::InvalidState)?;
                let tiles = tiles.borrow();
                Ok(tiles.get_room_owner(loc) == Some(placer.room_id))
            }

            fn in_bounds(_: &lua::Lua, p: Ref<RefCell<Placer>>, x: i32, y: i32) -> bool {
                let loc = Location::new(x, y);
                let placer = p.borrow();
                placer.bounds.in_bounds(loc)
            }

            fn contains_bound(
                _: &lua::Lua,
                p: Ref<RefCell<Placer>>,
                x: f64,
                y: f64,
                width: f64,
                height: f64,
            ) -> bool {
                let placer = p.borrow();
                let bound = placer.bounds;
                x >= f64::from(bound.min.x)
                    && x + width <= f64::from(bound.max.x) + 1.0
                    && y >= f64::from(bound.min.y)
                    && y + height <= f64::from(bound.max.y) + 1.0
            }

            #[allow(clippy::unit_arg)]
            fn placement_bound(
                _: &lua::Lua,
                p: Ref<RefCell<Placer>>,
                x: f64,
                y: f64,
                width: f64,
                height: f64,
            ) -> errors::Result<()> {
                let mut placer = p.borrow_mut();
                placer
                    .placement
                    .as_mut()
                    .ok_or(errors::ErrorKind::InvalidState)
                    .and_then_into(|v| {
                        Ok(v.actions.0.push(ObjectPlacementAction::PlacementBound {
                            location: object::Loc2D(x as f32, y as f32),
                            size: object::Size2D(width as f32, height as f32),
                        }))
                    })
            }

            #[allow(clippy::unit_arg)]
            fn collision_bound(
                _: &lua::Lua,
                p: Ref<RefCell<Placer>>,
                x: f64,
                y: f64,
                width: f64,
                height: f64,
            ) -> errors::Result<()> {
                let mut placer = p.borrow_mut();
                placer
                    .placement
                    .as_mut()
                    .ok_or(errors::ErrorKind::InvalidState)
                    .and_then_into(|v| {
                        Ok(v.actions.0.push(ObjectPlacementAction::CollisionBound {
                            location: object::Loc2D(x as f32, y as f32),
                            size: object::Size2D(width as f32, height as f32),
                        }))
                    })
            }

            #[allow(clippy::unit_arg)]
            fn selection_bound(
                _: &lua::Lua,
                p: Ref<RefCell<Placer>>,
                x: f64,
                y: f64,
                z: f64,
                width: f64,
                height: f64,
                depth: f64,
            ) -> errors::Result<()> {
                let mut placer = p.borrow_mut();
                placer
                    .placement
                    .as_mut()
                    .ok_or(errors::ErrorKind::InvalidState)
                    .and_then_into(|v| {
                        Ok(v.actions
                            .0
                            .push(ObjectPlacementAction::SelectionBound(AABB {
                                min: cgmath::Vector3::new(x as f32, y as f32, z as f32),
                                max: cgmath::Vector3::new(
                                    (x + width) as f32,
                                    (y + height) as f32,
                                    (z + depth) as f32,
                                ),
                            })))
                    })
            }

            #[allow(clippy::unit_arg)]
            fn blocks_tile(
                _: &lua::Lua,
                p: Ref<RefCell<Placer>>,
                x: i32,
                y: i32,
            ) -> errors::Result<()> {
                let mut placer = p.borrow_mut();
                placer
                    .placement
                    .as_mut()
                    .ok_or(errors::ErrorKind::InvalidState)
                    .and_then_into(|v| {
                        Ok(v.actions
                            .0
                            .push(ObjectPlacementAction::BlocksTile(Location::new(x, y))))
                    })
            }

            fn get_parameters(_: &lua::Lua, p: Ref<RefCell<Placer>>) -> Ref<lua::Table> {
                p.borrow().parameters.clone()
            }

            fn remove_on_error(_lua: &lua::Lua, p: Ref<RefCell<Placer>>) {
                p.borrow_mut().remove_on_error = true;
            }

            fn get_room_key(lua: &lua::Lua, p: Ref<RefCell<Placer>>) -> UResult<Ref<String>> {
                let rooms = lua
                    .get_tracked::<LevelRooms>()
                    .ok_or_else(|| ErrorKind::InvalidState)?;
                let rooms = rooms.borrow();

                let room = rooms.get_room_info(p.borrow().room_id);
                Ok(Ref::new_string(lua, room.key.as_string()))
            }

            impl lua::LuaUsable for Placer {
                fn metatable(t: &lua::TypeBuilder) {
                    script::support_getters_setters(t);
                }
                fn fields(t: &lua::TypeBuilder) {
                    t.field("fail", lua::closure2(fail));
                    t.field("place_window", lua::closure5(place_window));
                    t.field("set_tile", lua::closure4(set_tile));
                    t.field("set_floor", lua::closure4(set_floor));
                    t.field("set_no_walls", lua::closure3(set_no_walls));
                    t.field("mark_door", lua::closure4(mark_door));
                    t.field("static_model", lua::closure6(static_model));
                    t.field("static_model_tex", lua::closure7(static_model_tex));
                    t.field("animated_model", lua::closure7(animated_model));
                    t.field("animated_model_tex", lua::closure8(animated_model_tex));
                    t.field("in_bounds", lua::closure3(in_bounds));
                    t.field("contains_bound", lua::closure5(contains_bound));
                    t.field("placement_bound", lua::closure5(placement_bound));
                    t.field("collision_bound", lua::closure5(collision_bound));
                    t.field("selection_bound", lua::closure7(selection_bound));
                    t.field("owns_tile", lua::closure3(owns_tile));
                    t.field("get_parameters", lua::closure1(get_parameters));
                    t.field("absolute_name", lua::closure2(absolute_name));
                    t.field("blocks_tile", lua::closure3(blocks_tile));
                    t.field("remove_on_error", lua::closure1(remove_on_error));
                    t.field("get_room_key", lua::closure1(get_room_key));
                }
            }

            let parameters =
                obj.placer_parameters
                    .iter()
                    .fold(Ref::new_table(engine), |tbl, pair| {
                        tbl.insert(
                            Ref::new_string(engine, pair.0.as_str()),
                            Ref::new_string(engine, pair.1.as_str()),
                        );
                        tbl
                    });

            let id = self.id();
            let placer = lua::Ref::new(
                engine,
                RefCell::new(Placer {
                    room_id: id,
                    obj: placement.key.clone(),
                    failed: None,
                    placement: Some(ObjectPlacement::empty(
                        placement.key.borrow(),
                        pos,
                        rotation,
                    )),
                    bounds: self.bounds(),
                    parameters,
                    remove_on_error: false,
                }),
            );

            if Self::is_virtual() {
                engine.set::<Option<i32>>(
                    Scope::Registry,
                    "level_virtual_mode",
                    Some(i32::from(self.id().0)),
                );
            }
            let res = engine.invoke_function::<_, i32>(
                "invoke_module_method",
                (
                    Ref::new_string(engine, obj.placer.0.module()),
                    Ref::new_string(engine, obj.placer.0.resource()),
                    Ref::new_string(engine, &*obj.placer.1),
                    f64::from(pos.0),
                    f64::from(pos.1),
                    placer.clone(),
                    version,
                    i32::from(rotation),
                ),
            );
            if Self::is_virtual() {
                engine.set::<Option<i32>>(Scope::Registry, "level_virtual_mode", None);
            }

            let mut placer = placer.borrow_mut();
            placement.valid = !(placer.failed.is_some() || res.is_err());
            let mut pm = assume!(log, placer.placement.take());

            if placement.valid && !self.can_place_object(&pm) {
                placement.valid = false;
                placer.failed = Some("Area not placeable".into());
            }

            match pm.apply::<_, EntityCreator>(log, self, entities, id, !placement.valid) {
                Ok(rev) => placement.placement_rev = Some(rev),
                Err(err) => {
                    placement.placement_rev = None;
                    return Err(errors::ErrorKind::InvalidPlacement(err).into());
                }
            }
            let res = match res {
                Ok(version) => {
                    pm.version = version;
                    Ok(())
                }
                Err(err) => Err(err),
            };
            placement.placement = Some(pm);

            self.flag_dirty();
            self.set_placement(placement);
            if let Some(res) = placer.failed.take() {
                if placer.remove_on_error {
                    return Err(errors::ErrorKind::RemoveInvalidPlacement(res).into());
                }
                return Err(errors::ErrorKind::InvalidPlacement(res).into());
            }
            res?
        }
        Ok(())
    }

    /// Cancels placement of the active object
    fn cancel_object_placement<EntityCreator>(
        &mut self,
        log: &Logger,
        entities: &mut ecs::Container,
    ) where
        EntityCreator: entity::EntityCreator,
    {
        if let Some(mut placement) = self.take_placement() {
            if let Some(old) = placement.placement_rev.take() {
                old.apply::<_, EntityCreator>(log, self, entities)
                    .expect("Failed to reverse placement whilst cancelling");
                self.flag_dirty();
            }
        }
        self.should_lower_walls(false);
    }

    /// Places the active object if its in a valid location otherwise
    /// returns an error and does nothing
    fn finalize_object_placement<E, EntityCreator>(
        &mut self,
        log: &Logger,
        asset_manager: &assets::AssetManager,
        engine: &E,
        entities: &mut ecs::Container,
        version: Option<i32>,
        rotation: i16,
    ) -> errors::Result<usize>
    where
        E: script::Invokable,
        EntityCreator: entity::EntityCreator,
    {
        self.should_lower_walls(false);
        let pos = match self.get_placement_position() {
            Some(val) => val,
            None => return Err("Nothing to place".into()),
        };
        self.move_active_object::<_, EntityCreator>(
            log,
            asset_manager,
            engine,
            entities,
            pos,
            version,
            rotation,
        )?;

        if let Some(mut placement) = self.take_placement() {
            // Check if this object wants to replace existing floor objects
            if let Some(loc) = placement.replaces_floor() {
                // Clear this object first before removing the floor below
                if let Some(old) = placement.placement_rev.take() {
                    old.apply::<_, EntityCreator>(log, self, entities)
                        .expect("Failed to reverse previous placement, this shouldn't happen");
                }

                let remove_target = self.get_remove_target(loc);

                let prev = if let Some(obj_id) = remove_target {
                    Some((
                        obj_id,
                        self.remove_object::<EntityCreator>(entities, obj_id)?,
                    ))
                } else {
                    None
                };

                // Try and put our object back
                let id = self.id();
                if let Some(pm) = placement.placement.take() {
                    match pm.apply::<_, EntityCreator>(log, self, entities, id, !placement.valid) {
                        Ok(rev) => placement.placement_rev = Some(rev),
                        Err(err) => {
                            placement.placement_rev = None;
                            if let Some((obj_id, prev)) = prev {
                                self.replace_object::<EntityCreator>(entities, obj_id, prev);
                            }
                            return Err(errors::ErrorKind::InvalidPlacement(err).into());
                        }
                    }
                    placement.placement = Some(pm);
                }
            }

            if placement.valid {
                self.place_object(placement)
            } else {
                Err("Try to finalize invalid placement".into())
            }
        } else {
            Err("Nothing to place".into())
        }
    }
}

pub(super) fn find_remove_target(
    objects: &[Option<(ObjectPlacement, ReverseObjectPlacement)>],
    loc: Location,
) -> Option<usize> {
    objects
        .iter()
        .enumerate()
        .filter_map(|(idx, obj)| obj.as_ref().map(|obj| (idx, obj.0.actions.0.iter())))
        .flat_map(|(idx, actions)| actions.map(move |action| (idx, action)))
        .filter_map(|(idx, action)| {
            use crate::ObjectPlacementAction::*;
            match *action {
                Tile {
                    location,
                    floor_replacement,
                    ..
                } if location == loc && floor_replacement => Some(idx),
                _ => None,
            }
        })
        .next()
}
