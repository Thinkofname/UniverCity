//! Manages tiles and any information attached to them

use serde_json::{self, Value};

use crate::util::{Direction, FNVMap, FNVSet, Location};
use std::sync::Arc;
use crate::level::{self, Check};
use crate::assets;
use crate::prelude::*;

/// Loads tile descriptions from an asset manager.
pub enum Loader {}

impl <'a> assets::AssetLoader<'a> for Loader {
    type LoaderData = LoaderData;
    type Return = Arc<Type>;
    type Key = assets::ResourceKey<'a>;

    fn init(_assets: &assets::Store) -> Self::LoaderData {
        LoaderData {
            tiles: Vec::new(),
            by_name: Default::default(),
        }
    }

    fn load(data: &mut Self::LoaderData, assets: &assets::AssetManager, resource: Self::Key) -> UResult<Self::Return> {
        let id = data.by_name.get(&resource).cloned();
        let id = match id {
            Some(id) => id,
            None => {
                let file = assets.open_from_pack(resource.module_key(), &format!("tiles/{}.json", resource.resource()))?;
                let info: TypeInfo = serde_json::from_reader(file)?;
                let id = data.tiles.len();
                let mut rules = vec![];
                for tex in info.texture {
                    let mut checks = vec![];
                    for m in tex.matches {
                        let x = m[0].as_i64().ok_or_else(|| ErrorKind::Msg("Expected integer for `x` texture check".into()))? as i32;
                        let y = m[1].as_i64().ok_or_else(|| ErrorKind::Msg("Expected integer for `y` texture check".into()))? as i32;
                        let val = m[2].as_str().ok_or_else(|| ErrorKind::Msg("Expected string for `val` texture check".into()))?;
                        checks.push(Check::new(resource.module_key(), x, y, val));
                    }
                    if checks.is_empty() {
                        checks.push(Check::Always);
                    }
                    rules.push(TextureRule {
                        texture: assets::LazyResourceKey::parse(&tex.texture)
                            .or_module(resource.module_key())
                            .into_owned(),
                        checks,
                    });
                }
                let wall_rule = if let Some(wall_info) = info.wall {
                    let mut checks = vec![];
                    for m in wall_info.matches {
                        checks.push(Check::new(resource.module_key(), 0, 0, &m));
                    }
                    if checks.is_empty() {
                        checks.push(Check::Always);
                    }
                    Some(WallRule {
                        checks,
                    })
                } else {
                    None
                };
                let mut properties = FNVSet::default();
                for prop in info.properties {
                    properties.insert(prop);
                }
                data.tiles.push(Arc::new(Type {
                    id: id as TileId,
                    key: resource.clone().into_owned(),
                    texture_rules: rules,
                    wall_rule,
                    properties,
                    movement_cost: info.movement_cost,
                    movement_edge_cost: info.movement_edge_cost.unwrap_or(info.movement_cost),
                }));
                data.by_name.insert(resource.into_owned(), id as TileId);
                id as TileId
            },
        };
        data.tiles.get(id as usize)
            .cloned()
            .ok_or_else(|| ErrorKind::NoSuchAsset.into())
    }
}

/// Gets a tile by id. This doesn't load tiles and will panic
/// if it isn't already loaded
pub enum ById {}

impl <'a> assets::AssetLoader<'a> for ById {
    type LoaderData = LoaderData;
    type Return = Arc<Type>;
    type Key = TileId;

    fn init(_assets: &assets::Store) -> Self::LoaderData {
        LoaderData {
            tiles: Vec::new(),
            by_name: Default::default(),
        }
    }

    fn load(data: &mut Self::LoaderData, _assets: &assets::AssetManager, id: Self::Key) -> UResult<Self::Return> {
        data.tiles.get(id as usize)
            .cloned()
            .ok_or_else(|| ErrorKind::NoSuchAsset.into())
    }
}

/// A collection of tiles that can be used in a level
pub struct LoaderData {
    tiles: Vec<Arc<Type>>,
    by_name: FNVMap<assets::ResourceKey<'static>, TileId>,
}


/// The type required to store a tile id. Useful encase this changes later.
pub type TileId = u16;

/// Represents a type of tile that can be used in a level.
pub struct Type {
    /// The id of this tile type.
    pub id: TileId,
    /// The name of the tile
    pub key: assets::ResourceKey<'static>,
    texture_rules: Vec<TextureRule>,
    wall_rule: Option<WallRule>,
    /// A set of properties that the tile has.
    pub properties: FNVSet<String>,
    /// The cost of moving on this tile
    pub movement_cost: i32,
    /// The cost of moving on this tile's edge
    pub movement_edge_cost: i32,
}

struct WallRule {
    checks: Vec<Check>,
}

struct TextureRule {
    texture: assets::ResourceKey<'static>,
    checks: Vec<Check>,
}

impl Type {

    /// Returns all possible textures this tile could use.
    ///
    /// Used for preloading
    pub fn get_possible_textures<'a>(&'a self) -> impl Iterator<Item=assets::ResourceKey<'a>> + 'a {
        Box::new(self.texture_rules.iter()
            .map(|v| &v.texture)
            .map(|v| v.borrow()))
    }

    /// Returns the texture for the given location by matching the rules
    /// defined for this type.
    pub fn get_texture_for<L: level::LevelView>(&self, level: &L, loc: Location) -> assets::ResourceKey<'_> {
        'check:
        for rule in &self.texture_rules {
            for check in &rule.checks {
                if !check.test(level, loc, 0, 0) {
                    continue 'check;
                }
            }
            return rule.texture.borrow();
        }
        panic!("Missing texture for {:?}", self.key);
    }

    /// Returns the texture to be used for a wall if one should be placed in the direction.
    pub fn should_place_wall<L: level::LevelView>(&self, level: &L, loc: Location, dir: Direction) -> bool {
        if let Some(wall) = self.wall_rule.as_ref() {
            if level.get_tile_flags(loc).contains(TileFlag::NO_WALLS) {
                return false;
            }
            let (dx, dy) = dir.offset();
            for check in &wall.checks {
                if !check.test(level, loc, dx, dy) {
                    return false;
                }
            }
            true
        } else {
            false
        }
    }
}
// Raw json structs

#[derive(Debug, Serialize, Deserialize)]
struct TypeInfo {
    texture: Vec<TextureRuleInfo>,
    #[serde(default)]
    properties: Vec<String>,
    #[serde(default)]
    wall: Option<WallInfo>,
    #[serde(default="move_cost")]
    movement_cost: i32,
    #[serde(default)]
    movement_edge_cost: Option<i32>,
}

fn move_cost() -> i32 { 10 }

#[derive(Debug, Serialize, Deserialize)]
struct TextureRuleInfo {
    texture: String,
    #[serde(default, rename="match")]
    matches: Vec<Vec<Value>>,
}
#[derive(Debug, Serialize, Deserialize)]
struct WallInfo {
    #[serde(default, rename="match")]
    matches: Vec<String>,
}


#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::env;
    use crate::assets::*;

    #[test]
    fn try_load_tiles() {
        let exe = env::current_exe().unwrap();
        let parent = exe.parent().unwrap();
        env::set_current_dir(parent.join("../../../")).unwrap();
        let log = ::slog::Logger::root(::slog::Discard, o!());
        let assets = AssetManager::with_packs(&log, &["base".to_owned()])
            .register::<super::Loader>()
            .build();
        load_dir(&assets, Path::new("./assets/base/base/tiles"));
    }

    fn load_dir(assets: &AssetManager, dir: &Path) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                load_dir(assets, &path);
            } else {
                let path_str = path.to_string_lossy();
                let path_str = &path_str["./assets/base/base/tiles/".len()..];
                let path_str = &path_str[..path_str.len() - 5];

                println!("Trying to load: {:?}", path_str);
                assets.loader_open::<super::Loader>(ResourceKey::new("base", path_str)).unwrap();
            }
        }
    }
}