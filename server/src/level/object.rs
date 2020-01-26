//! Manages objects and any information attached to them

use serde_json;

use crate::util::*;
use delta_encode::AlwaysVec;
use std::sync::Arc;
use crate::assets;
use super::*;

use crate::common::{AnimationInfo, AnimationSet};

/// Loads object descriptions from an asset manager.
pub enum Loader {}

impl <'a> assets::AssetLoader<'a> for Loader {
    type LoaderData = LoaderData;
    type Return = Arc<Type>;
    type Key = assets::ResourceKey<'a>;

    fn init(_assets: &assets::Store) -> Self::LoaderData {
        LoaderData {
            objects: Default::default(),
        }
    }

    fn load(data: &mut Self::LoaderData, assets: &assets::AssetManager, resource: Self::Key) -> UResult<Self::Return> {
        use std::collections::hash_map::Entry;
        Ok(match data.objects.entry(resource.into_owned()) {
            Entry::Occupied(val) => val.into_mut().clone(),
            Entry::Vacant(val) => {
                let file = assets.open_from_pack(val.key().module_key(), &format!("objects/{}.json", val.key().resource()))?;
                let info: TypeInfo = serde_json::from_reader(file)?;

                let (handle, params) = match info.placer {
                    Placer::NoParams(handle) => (handle, FNVMap::default()),
                    Placer::Params{handle, params} => (handle, params),
                };

                let (sub, method) = if let Some(pos) = handle.char_indices().find(|v| v.1 == '#') {
                    handle.split_at(pos.0)
                } else {
                    bail!("Invalid placer")
                };
                let obj = Arc::new(Type {
                    key: val.key().clone().into_owned(),
                    display_name: info.name,
                    description: info.description,
                    group: info.group,
                    placer: (
                        assets::LazyResourceKey::parse(sub)
                            .or_module(val.key().module_key())
                            .into_owned(),
                        method[1..].into()
                    ),
                    placer_parameters: params,
                    ty: info.ty,
                    animations: info.animations.map(|v| AnimationInfo::map_to_animation_set(val.key().module_key(), v)),
                    lower_walls_placement: info.lower_walls_placement,
                    cost: info.cost.unwrap_or(UniDollar(0)),
                    placement_style: info.placement_style,
                });
                val.insert(obj).clone()
            }
        })
    }
}

/// A collection of tiles that can be used in a level
pub struct LoaderData {
    objects: FNVMap<assets::ResourceKey<'static>, Arc<Type>>,
}

/// Represents a type of object that can be used in a room.
pub struct Type {
    /// The name of the object
    pub key: assets::ResourceKey<'static>,

    /// The display name of the object
    pub display_name: String,
    /// The description of the object
    pub description: String,
    /// The group/type of the object for displaying
    /// on the object list
    pub group: String,
    /// The lua method that handles placement of this
    /// object
    pub placer: (assets::ResourceKey<'static>, String),
    /// Optional parameters to the placer
    pub placer_parameters: FNVMap<String, String>,
    /// Optional type tag for the object
    pub ty: Option<String>,
    /// Optional animation information for animated models
    pub animations: Option<AnimationSet>,
    /// Whether walls should be lowered to ease placement
    /// of this object.
    pub lower_walls_placement: bool,
    /// Cost of this object
    pub cost: UniDollar,
    /// The style of placement to use for this object
    pub placement_style: Option<PlacementStyle>,
}

/// The style of placement to use
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Copy)]
pub enum PlacementStyle {
    /// Drag place per a tile
    TileRepeat,
}

/// A collection of actions that make up a single object placement
#[derive(Debug)]
pub struct ObjectPlacement {
    /// The key to the object that this placement is about
    pub key: assets::ResourceKey<'static>,
    /// The list of actions this placement causes
    pub actions: AlwaysVec<ObjectPlacementAction>,
    /// The position the object was placed at.
    ///
    /// This is the raw click position as passed to the script
    pub position: ObjectPlacePosition,
    /// The rotation value passed to the placement script
    ///
    /// Used when moving an object to keep its rotation the
    /// same
    pub rotation: i16,
    /// A script provided version number to use when replacing
    /// this object
    pub version: i32,
}

/// Represents the placement position of an object
#[derive(Debug, DeltaEncode, Clone, Copy)]
#[delta_always]
pub struct ObjectPlacePosition {
    /// The position on the x axis on the level
    pub x: f32,
    /// The position on the y axis on the level
    pub y: f32,
}

impl ObjectPlacement {
    // TODO: ?
    pub(super) fn empty(key: assets::ResourceKey<'_>, pos: (f32, f32), rotation: i16) -> ObjectPlacement {
        ObjectPlacement {
            key: key.into_owned(),
            actions: AlwaysVec(vec![]),
            position: ObjectPlacePosition {
                x: pos.0,
                y: pos.1,
            },
            rotation,
            version: 0,
        }
    }
}

/// Contains the information required to reverse a placement of an object
#[derive(Debug)]
pub struct ReverseObjectPlacement {
    actions: Vec<ReverseObjectPlacementAction>,
}

impl ReverseObjectPlacement {
    pub(super)fn apply<L, EC>(&self, log: &Logger, level: &mut L, entities: &mut Container) -> Result<(), String>
        where L: LevelAccess,
              EC: EntityCreator,
    {
        for action in self.actions.iter().rev() {
            action.execute_action::<L, EC>(log, level, entities)?;
        }
        Ok(())
    }

    /// Returns a list of entities that were created when then
    /// `ObjectPlacement` belonging to this was applied
    pub fn get_entities(&self) -> Vec<Entity> {
        let mut out = vec![];
        for action in &self.actions {
            action.get_entities(&mut out);
        }
        out
    }
}

impl ObjectPlacement {
    pub(crate) fn apply<L, EC>(
            &self,
            log: &Logger,
            level: &mut L, entities: &mut Container,
            room: room::Id, invalid: bool
    ) -> Result<ReverseObjectPlacement, String>
        where L: LevelAccess,
              EC: EntityCreator,
    {
        let mut reverse = vec![];
        for action in &self.actions.0 {
            match action.execute_action::<L, EC>(log, self.key.borrow(), level, entities, room, invalid) {
                Ok(val) => reverse.push(val),
                Err(err) => {
                    // Revert first
                    for rev in reverse.into_iter().rev() {
                        rev.execute_action::<L, EC>(log, level, entities).expect("Failed to roll back actions after failing")
                    }
                    return Err(err);
                }
            }
        }
        Ok(ReverseObjectPlacement { actions: reverse })
    }
}

/// A flag to set on a wall
#[derive(Debug, Serialize, Deserialize, Clone, DeltaEncode)]
pub enum WallPlacementFlag {
    /// Flags a wall as a normal wall
    None,
    /// Flags a wall as a window
    Window {
        /// The window model
        key: ResourceKey<'static>
    },
    /// Flags a wall as a door
    Door,
}

/// A single placement of an object in a room
#[derive(Debug, DeltaEncode)]
#[delta_always]
pub enum ObjectPlacementAction {
    /// An object that changes a flag of a wall
    WallFlag {
        /// The location of the wall
        location: Location,
        /// The direction of the wall from the location
        direction: Direction,
        /// The flag to set on the wall
        flag: WallPlacementFlag,
    },
    /// An object that changes the tile at a location
    Tile {
        /// The location of the tile
        location: Location,
        /// The key of the tile to set
        key: assets::ResourceKey<'static>,
        /// Marks the tile change as a floor replacement
        ///
        /// If another floor replacement is set at this location
        /// this object will be removed first
        floor_replacement: bool,
    },
    /// A object that changes a flag of a tile
    TileFlag {
        /// The location of the tile
        location: Location,
        /// The flag to set on the tile
        flag: TileFlag,
    },
    /// An object which is a static model
    StaticModel {
        /// Location of the object
        location: Loc3D,
        /// Rotation in radians of the object
        rotation: Angle,
        /// The object's model location
        object: assets::ResourceKey<'static>,
        /// The optional replacement texture
        texture: Option<assets::ResourceKey<'static>>,
    },
    /// An object which is a animated model
    AnimatedModel {
        /// Location of the object
        location: Loc3D,
        /// Rotation in radians of the object
        rotation: Angle,
        /// The object's model location
        object: assets::ResourceKey<'static>,
        /// The optional replacement texture
        texture: Option<assets::ResourceKey<'static>>,
        /// The initial animation of the model
        animation: String,
    },
    /// Marks a region as unpathable.
    ///
    /// Prevents entities from walking through that area.
    /// Can be stacked multiple times.
    CollisionBound {
        /// The location of the bound
        location: Loc2D,
        /// The size of the bound
        size: Size2D,
    },
    /// Marks a region as placeable.
    ///
    /// Prevents other objects from being placed in the area.
    /// Also used in the placement checks when placing this object
    PlacementBound {
        /// The location of the bound
        location: Loc2D,
        /// The size of the bound
        size: Size2D,
    },
    /// Bound used to allow the player to select the object
    SelectionBound(AABB),
    /// Marks the object as blocking the tile.
    ///
    /// Useful for things like update scripts that shouldn't
    /// touch tiles that objects depend on.
    BlocksTile(Location),
}

/// A 3d position in the level
#[derive(Clone, Copy, PartialEq, DeltaEncode, Debug)]
#[delta_always]
pub struct Loc3D(pub f32, pub f32, pub f32);
/// A 2d position in the level
#[derive(Clone, Copy, PartialEq, DeltaEncode, Debug)]
#[delta_always]
pub struct Loc2D(pub f32, pub f32);
#[derive(Clone, Copy, PartialEq, DeltaEncode, Debug)]
/// A 2d size
#[delta_always]
pub struct Size2D(pub f32, pub f32);

impl ObjectPlacementAction {
    fn execute_action<L, EC>(
            &self,
            log: &Logger,
            key: assets::ResourceKey<'_>,
            level: &mut L, entities: &mut Container,
            room: room::Id, invalid: bool
    ) -> Result<ReverseObjectPlacementAction, String>
        where L: LevelAccess,
              EC: EntityCreator,
    {
        match *self {
            ObjectPlacementAction::WallFlag{location, direction, ref flag} => {
                let mut info = level.get_wall_info(location, direction).ok_or_else(|| "No wall at location".to_owned())?;
                let orig = info.flag;
                info.flag = match *flag {
                    WallPlacementFlag::None => TileWallFlag::None,
                    WallPlacementFlag::Window{ref key} => TileWallFlag::Window(level.get_or_create_window(key.borrow())),
                    WallPlacementFlag::Door => TileWallFlag::Door,
                };
                level.set_wall_info(location, direction, Some(info));
                Ok(ReverseObjectPlacementAction::WallFlag { location, direction, flag: orig})
            },
            ObjectPlacementAction::Tile{location, ref key, ..} => {
                if level.get_room_owner(location) != Some(room) {
                    return Ok(ReverseObjectPlacementAction::None);
                }
                let old_walls = [
                    level.get_wall_info(location, Direction::North),
                    level.get_wall_info(location, Direction::South),
                    level.get_wall_info(location, Direction::East),
                    level.get_wall_info(location, Direction::West),
                ];
                let old = level.get_tile_raw(location);
                level.set_tile(location, key.borrow());
                {
                    if let Some(mut new) = level.get_tile_raw(location) {
                        new.owner = old.as_ref().and_then(|v| v.owner);
                        level.set_tile_raw(location, new);
                    }
                }
                Ok(ReverseObjectPlacementAction::Tile {
                    location,
                    old,
                    old_walls,
                })
            },
            ObjectPlacementAction::TileFlag{location, flag} => {
                let flags = level.get_tile_flags(location);
                level.set_tile_flags(location, flags | flag);
                Ok(ReverseObjectPlacementAction::TileFlag {
                    location,
                    flags,
                })
            }
            ObjectPlacementAction::StaticModel{location, rotation, ref object, ref texture} => {
                let e = EC::static_model(entities, object.borrow(), texture.as_ref().map(|v| v.borrow()));
                {
                    let pos = assume!(log, entities.get_component_mut::<Position>(e));
                    pos.x = location.0;
                    pos.y = location.1;
                    pos.z = location.2;
                }
                assume!(log, entities.get_component_mut::<Rotation>(e)).rotation = rotation;
                if invalid {
                    entities.add_component(e, InvalidPlacement);
                }
                entities.add_component(e, RoomOwned::new(room));
                tag_object_entity(log, &level.get_asset_manager(), key.borrow(), entities, e);
                Ok(ReverseObjectPlacementAction::StaticModel{ entity: e })
            },
            ObjectPlacementAction::AnimatedModel{location, rotation, ref object, ref texture, ref animation} => {
                let animations = {
                    let assets = level.get_asset_manager();
                    assume!(log,
                        assume!(log, assets.loader_open::<object::Loader>(key.borrow())).animations.clone()
                    )
                };
                let e = EC::animated_model(entities, object.borrow(), texture.as_ref().map(|v| v.borrow()), animations, animation);
                {
                    let pos = assume!(log, entities.get_component_mut::<Position>(e));
                    pos.x = location.0;
                    pos.y = location.1;
                    pos.z = location.2;
                }
                assume!(log, entities.get_component_mut::<Rotation>(e)).rotation = rotation;
                if invalid {
                    entities.add_component(e, InvalidPlacement);
                }
                entities.add_component(e, RoomOwned::new(room));
                tag_object_entity(log, &level.get_asset_manager(), key.borrow(), entities, e);
                Ok(ReverseObjectPlacementAction::AnimatedModel{ entity: e })
            },
            ObjectPlacementAction::CollisionBound{..}
            | ObjectPlacementAction::PlacementBound{..}
            | ObjectPlacementAction::SelectionBound{..}
            | ObjectPlacementAction::BlocksTile(..) => Ok(ReverseObjectPlacementAction::None),
        }
    }
}

fn tag_object_entity(
    log: &Logger,
    assets: &assets::AssetManager, key: assets::ResourceKey<'_>,
    entities: &mut Container, e: Entity
) {
    let obj = assume!(log, assets.loader_open::<object::Loader>(key.borrow()));

    match obj.ty.as_ref().map(|v| v.as_ref()) {
        Some("door") => {
            entities.add_component(e, Door::new(false));
        },
        _ => {},
    }

    entities.add_component(e, Object {
        key: key.into_owned(),
        ty: obj.ty.clone(),
    });
}

#[derive(Debug)]
enum ReverseObjectPlacementAction {
    WallFlag {
        location: Location,
        direction: Direction,
        flag: TileWallFlag,
    },
    Tile {
        location: Location,
        old: Option<TileData>,
        old_walls: [Option<WallInfo>; 4],
    },
    TileFlag {
        location: Location,
        flags: TileFlag,
    },
    StaticModel {
        entity: Entity,
    },
    AnimatedModel {
        entity: Entity,
    },
    None,
}

impl ReverseObjectPlacementAction {
    fn execute_action<L, EC>(&self, log: &Logger, level: &mut L, entities: &mut Container) -> Result<(), String>
        where L: LevelAccess,
              EC: EntityCreator,
    {
        match *self {
            ReverseObjectPlacementAction::WallFlag{location, direction, flag} => {
                let mut info = level.get_wall_info(location, direction)
                    .ok_or_else(|| "No wall at location".to_owned())?;
                info.flag = flag;
                level.set_wall_info(location, direction, Some(info));
                Ok(())
            }
            ReverseObjectPlacementAction::StaticModel{entity}
            | ReverseObjectPlacementAction::AnimatedModel{entity} => {
                entities.remove_entity(entity);
                Ok(())
            }
            ReverseObjectPlacementAction::Tile{location, old, old_walls} => {
                if let Some(old) = old {
                    if old.owner == level.get_tile_raw(location).and_then(|v| v.owner) {
                        level.set_tile_raw(location, old);
                    } else {
                        return Ok(());
                    }
                }
                for (dir, old) in old_walls.iter().cloned().enumerate() {
                    if let Some(old) = old {
                        if old.flag != TileWallFlag::None {
                            let dir = assume!(log, Direction::from_usize(dir));
                            level.set_wall_info(location, dir, Some(old));
                        }
                    }
                }
                Ok(())
            }
            ReverseObjectPlacementAction::TileFlag{location, flags} => {
                level.set_tile_flags(location, flags);
                Ok(())
            }
            ReverseObjectPlacementAction::None => Ok(()),
        }
    }

    fn get_entities(&self, out: &mut Vec<Entity>) {
        match *self {
            ReverseObjectPlacementAction::None
            | ReverseObjectPlacementAction::WallFlag{..}
            | ReverseObjectPlacementAction::Tile{..}
            | ReverseObjectPlacementAction::TileFlag{..} => {}

            ReverseObjectPlacementAction::StaticModel{entity}
            | ReverseObjectPlacementAction::AnimatedModel{entity} => {
                out.push(entity);
            }
        }
    }
}

// Raw json structs

#[derive(Debug, Serialize, Deserialize)]
struct TypeInfo {
    name: String,
    description: String,
    group: String,
    placer: Placer,
    #[serde(rename="type")]
    ty: Option<String>,
    animations: Option<FNVMap<String, AnimationInfo>>,
    #[serde(default = "return_true")]
    lower_walls_placement: bool,
    cost: Option<UniDollar>,
    #[serde(default)]
    placement_style: Option<PlacementStyle>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum Placer {
    NoParams(String),
    Params {
        handle: String,
        params: FNVMap<String, String>,
    }
}

fn return_true() -> bool { true }

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::env;
    use crate::assets::*;

    #[test]
    fn try_load_objects() {
        let exe = env::current_exe().unwrap();
        let parent = exe.parent().unwrap();
        env::set_current_dir(parent.join("../../../")).unwrap();
        let log = ::slog::Logger::root(::slog::Discard, o!());
        let assets = AssetManager::with_packs(&log, &["base".to_owned()])
            .register::<super::Loader>()
            .build();
        load_dir(&assets, Path::new("./assets/base/base/objects"));
    }

    fn load_dir(assets: &AssetManager, dir: &Path) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                load_dir(assets, &path);
            } else {
                let path_str = path.to_string_lossy();
                let path_str = &path_str["./assets/base/base/objects/".len()..];
                let path_str = &path_str[..path_str.len() - 5];
                if path_str.starts_with("groups/") {
                    continue;
                }
                println!("Trying to load: {:?}", path_str);
                assets.loader_open::<super::Loader>(ResourceKey::new("base", path_str)).unwrap();
            }
        }
    }
}