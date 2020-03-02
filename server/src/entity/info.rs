//! Manages any information needed to create entities

use rand::seq::SliceRandom;
use serde::de::DeserializeOwned;
use serde::de::{Deserialize, Deserializer, Visitor};
use serde_json;

use crate::assets;
use crate::ecs;
use crate::prelude::*;
use crate::util::FNVMap;
use std::marker::PhantomData;
use std::sync::Arc;

use crate::common::{AnimationInfo, AnimationSet};

/// Loads entity descriptions
pub struct Loader<CC> {
    _cc: PhantomData<CC>,
}

impl<'a, CC> assets::AssetLoader<'a> for Loader<CC>
where
    CC: ComponentCreator,
{
    type LoaderData = LoaderData<CC>;
    type Return = Arc<Type<CC>>;
    type Key = assets::ResourceKey<'a>;

    fn init(assets: &assets::Store) -> Self::LoaderData {
        LoaderData {
            log: assets.log.new(o!("type"=>"component loader")),
            entities: Default::default(),
            names: Default::default(),
            _component_creator: PhantomData,
        }
    }

    fn load(
        data: &mut Self::LoaderData,
        assets: &assets::AssetManager,
        resource: Self::Key,
    ) -> UResult<Self::Return> {
        if !data.entities.contains_key(&resource) {
            let file = assets.open_from_pack(
                resource.module_key(),
                &format!("entity/{}.json", resource.resource()),
            )?;
            let info: TypeInfo<CC::Raw> = serde_json::from_reader(file)?;

            let dnames = &mut data.names;
            let names = &info.names;
            let model = &info.model;
            let animations = &info.animations;
            let log = &data.log;
            let icon = &info.icon;

            data.entities.insert(
                resource.borrow().into_owned(),
                Arc::new(Type {
                    key: resource.borrow().into_owned(),
                    display_name: info.name,
                    components: info
                        .components
                        .into_iter()
                        .map(|v| CC::from_raw(log, resource.module_key(), v))
                        .collect(),
                    generator: info.generator.map(|v| {
                        assets::LazyResourceKey::parse(&v)
                            .or_module(resource.module_key())
                            .into_owned()
                    }),
                    highlight: info.highlight.map(|v| Highlight {
                        color: v.color,
                        label: v.label,
                    }),

                    variants: info
                        .variants
                        .into_iter()
                        .map(|v| {
                            let names =
                                assets::LazyResourceKey::parse(v.names.as_ref().unwrap_or(names))
                                    .or_module(resource.module_key());

                            let name_list = dnames
                                .entry(names.borrow().into_owned())
                                .or_insert_with(|| {
                                    let file = assume!(
                                        log,
                                        assets.open_from_pack(
                                            names.module_key(),
                                            &format!("entity/names/{}.json", names.resource())
                                        )
                                    );
                                    let names: NameListInfo =
                                        assume!(log, serde_json::from_reader(file));
                                    Arc::new(NameList {
                                        first: names.first.into_iter().map(Into::into).collect(),
                                        second: names.second.into_iter().map(Into::into).collect(),
                                    })
                                });
                            let mut ani = animations.clone();
                            for (k, v) in v.animations {
                                ani.insert(k, v);
                            }
                            SubType {
                                name_list: name_list.clone(),
                                model: assets::LazyResourceKey::parse(
                                    v.model.as_ref().unwrap_or(model),
                                )
                                .or_module(resource.module_key())
                                .into_owned(),
                                icon: assets::LazyResourceKey::parse(
                                    v.icon.as_ref().unwrap_or(icon),
                                )
                                .or_module(resource.module_key())
                                .into_owned(),
                                animations: AnimationInfo::map_to_animation_set(
                                    resource.module_key(),
                                    ani,
                                ),
                            }
                        })
                        .collect(),
                }),
            );
        }
        data.entities
            .get(&resource)
            .cloned()
            .ok_or_else(|| ErrorKind::NoSuchAsset.into())
    }
}

/// A collection of tiles that can be used in a level
pub struct LoaderData<CC> {
    log: Logger,
    entities: FNVMap<assets::ResourceKey<'static>, Arc<Type<CC>>>,
    names: FNVMap<assets::ResourceKey<'static>, Arc<NameList>>,
    _component_creator: PhantomData<CC>,
}

/// List of names that an entity can use
pub struct NameList {
    /// List of first names
    pub first: Vec<Arc<str>>,
    /// List of second names
    pub second: Vec<Arc<str>>,
}

/// Represents a type of object that can be used in a room.
pub struct Type<CC> {
    /// The name of the object
    pub key: assets::ResourceKey<'static>,
    /// The variants of the entity
    pub variants: Vec<SubType>,
    /// The display name of the object
    pub display_name: String,
    /// The components to add/modify on all spawned entities
    /// of this type.
    pub components: Vec<CC>,
    /// The name of the script that should be used to generate new
    /// instances of this entity
    pub generator: Option<ResourceKey<'static>>,
    /// Contains information on what to do when the entity is
    /// highlighted if anything
    pub highlight: Option<Highlight>,
}

/// Information on what to do when highlighted
pub struct Highlight {
    /// The highlight color
    pub color: (u8, u8, u8),
    /// The highlight tooltip text
    pub label: Option<String>,
}

/// Variant specific information
pub struct SubType {
    /// The key of the list of names to use for this entity
    pub name_list: Arc<NameList>,
    /// The icon to display in the gui representing this
    /// object
    pub icon: assets::ResourceKey<'static>,
    /// The model that the entity will be rendered with
    pub model: assets::ResourceKey<'static>,
    /// Animation information for animated models
    pub animations: AnimationSet,
}

impl<CC: ComponentCreator> Type<CC> {
    /// Creates a new entity based on this type.
    pub fn create_entity(
        &self,
        em: &mut ecs::Container,
        variant_id: usize,
        name: Option<(Arc<str>, Arc<str>)>,
    ) -> ecs::Entity {
        use rand::thread_rng;
        let variant = &self.variants[variant_id];
        let e = <CC::Creator as super::EntityCreator>::animated_model(
            em,
            variant.model.borrow(),
            None,
            variant.animations.clone(),
            "idle",
        );
        let mut rng = thread_rng();
        em.add_component(
            e,
            super::Living {
                key: self.key.clone(),
                variant: variant_id,
                name: name.unwrap_or_else(|| {
                    (
                        variant
                            .name_list
                            .first
                            .choose(&mut rng)
                            .cloned()
                            .unwrap_or_else(|| "Missing".into()),
                        variant
                            .name_list
                            .second
                            .choose(&mut rng)
                            .cloned()
                            .unwrap_or_else(|| "Name".into()),
                    )
                }),
            },
        );
        em.add_component(e, Controlled::new());
        for c in &self.components {
            c.apply(em, e);
        }
        e
    }
}

/// Used to create components from json descriptions
pub trait ComponentCreator: Send + Sync + 'static {
    /// The type that should be used to initially create the entity
    type Creator: super::EntityCreator;

    /// The json form of this component
    type Raw: DeserializeOwned + ::std::fmt::Debug;

    /// Converts a json component description into a type that can be
    /// later applied to an entity.
    fn from_raw(log: &Logger, module: assets::ModuleKey<'_>, val: Self::Raw) -> Self;

    /// Applies the components described by this creator.
    fn apply(&self, em: &mut ecs::Container, e: ecs::Entity);
}

/// Server component types
pub enum ServerComponent {
    /// Client only
    Unknown,
    /// A size component
    Size {
        /// Width of the entity
        width: f32,
        /// Height of the entity
        height: f32,
        /// Depth of the entity
        depth: f32,
    },
    /// A movement speed component
    Speed {
        /// The movement speed of the entity
        speed: f32,
    },
    /// Entity requires payment
    Paid {
        /// The cost per a term for the entity
        cost: UniDollar,
    },
    /// Marks the entity has a student
    Student {},
    /// Tints part of the entity's model
    Tint {
        /// List of tints that can be used
        tints: Vec<Vec<TintColor>>,
    },
    /// An entity that works indepentantly of a room
    FreeRoam {
        /// The name of the script for this entity
        script: ResourceKey<'static>,
    },
    /// Vars that scripts can access/modify
    Vars {
        /// The collection of vars to use
        vars: String,
    },
}

#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct TintColor(u8, u8, u8, u8);

impl ComponentCreator for ServerComponent {
    type Creator = super::ServerEntityCreator;
    type Raw = ServerComponentInfo;

    fn from_raw(_log: &Logger, module: assets::ModuleKey<'_>, val: ServerComponentInfo) -> Self {
        match val {
            ServerComponentInfo::Size {
                width,
                height,
                depth,
            } => ServerComponent::Size {
                width,
                height,
                depth,
            },
            ServerComponentInfo::Speed { speed } => ServerComponent::Speed { speed },
            ServerComponentInfo::Paid { cost } => ServerComponent::Paid { cost },
            ServerComponentInfo::Student {} => ServerComponent::Student {},
            ServerComponentInfo::Tint { tints } => ServerComponent::Tint { tints },
            ServerComponentInfo::FreeRoam { script } => ServerComponent::FreeRoam {
                script: LazyResourceKey::parse(&script)
                    .or_module(module)
                    .into_owned(),
            },
            ServerComponentInfo::Vars { vars } => ServerComponent::Vars { vars },
            _ => ServerComponent::Unknown,
        }
    }

    /// Applies the components described by this creator.
    fn apply(&self, em: &mut ecs::Container, e: ecs::Entity) {
        use self::ServerComponent::*;
        use rand::thread_rng;
        match *self {
            Unknown => {}
            Size {
                width,
                height,
                depth,
            } => {
                em.add_component(
                    e,
                    super::Size {
                        width,
                        height,
                        depth,
                    },
                );
            }
            Speed { speed } => {
                em.add_component(
                    e,
                    super::MovementSpeed {
                        speed,
                        base_speed: speed,
                    },
                );
            }
            Paid { cost } => em.add_component(
                e,
                super::Paid {
                    cost,
                    wanted_cost: cost,
                    last_payment: None,
                },
            ),
            Student {} => {
                em.add_component(e, super::StudentController::new());
                em.add_component(e, super::Grades::new());
            }
            Tint { ref tints } => {
                let mut rng = thread_rng();
                em.add_component(
                    e,
                    super::Tints {
                        tints: tints
                            .iter()
                            .map(|v| {
                                v.choose(&mut rng)
                                    .map_or(TintColor(255, 255, 255, 255), |v| *v)
                            })
                            .map(|v| (v.0, v.1, v.2, v.3))
                            .collect(),
                    },
                );
            }
            FreeRoam { ref script } => {
                em.add_component(
                    e,
                    super::free_roam::FreeRoam {
                        script: script.clone(),
                    },
                );
            }
            Vars { ref vars } => match vars.as_str() {
                "student" => em.add_component(e, choice::StudentVars),
                "professor" => {
                    em.add_component(e, AutoRest);
                    em.add_component(e, choice::ProfessorVars);
                }
                "office_worker" => em.add_component(e, choice::OfficeWorkerVars),
                "janitor" => em.add_component(e, choice::JanitorVars),
                _ => {}
            },
        }
    }
}

/// Information about a type of staff member
#[derive(Clone, Debug)]
pub struct StaffInfo {
    /// The entity for this staff type
    pub entity: assets::ResourceKey<'static>,
}

/// Returns the stat variant for the given entity
pub fn entity_variant(ty: &Type<ServerComponent>) -> StatVariant {
    for c in &ty.components {
        if let ServerComponent::Vars { ref vars } = *c {
            match vars.as_str() {
                "student" => return Stats::STUDENT,
                "professor" => return Stats::PROFESSOR,
                "office_worker" => return Stats::OFFICE_WORKER,
                "janitor" => return Stats::JANITOR,
                _ => {}
            }
        }
    }
    Stats::STUDENT
}

/// Loads the list of possible staff members that can be hired
pub fn load_staff_list(log: &Logger, assets: &AssetManager) -> Vec<StaffInfo> {
    let mut staff_list = vec![];
    for module in assets.get_packs() {
        let entity_file = match assets.open_from_pack(module.borrow(), "entity/entities.json") {
            Ok(val) => val,
            Err(_) => continue,
        };
        let entity_raw: Vec<StaffInfoJson> = match serde_json::from_reader(entity_file) {
            Ok(val) => val,
            Err(err) => {
                error!(
                    log,
                    "Failed to parse entities.json for pack {:?}: {}", module, err
                );
                continue;
            }
        };
        staff_list.extend(entity_raw.into_iter().map(move |v| {
            StaffInfo {
                entity: assets::LazyResourceKey::parse(&v.entity)
                    .or_module(module.borrow())
                    .into_owned(),
            }
        }));
    }
    assert!(!staff_list.is_empty()); // Can't work with no entities
    staff_list
}

// Raw json structs

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[doc(hidden)]
pub enum ServerComponentInfo {
    AnimateMovement {},
    Size { width: f32, height: f32, depth: f32 },
    Speed { speed: f32 },
    Paid { cost: UniDollar },
    Student {},
    Tint { tints: Vec<Vec<TintColor>> },
    FreeRoam { script: String },
    Vars { vars: String },
}

#[derive(Debug, Deserialize)]
struct StaffInfoJson {
    entity: String,
}

#[derive(Deserialize)]
struct NameListInfo {
    pub first: Vec<String>,
    pub second: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct TypeInfo<C> {
    name: String,
    names: String,
    icon: String,
    model: String,
    animations: FNVMap<String, AnimationInfo>,
    components: Vec<C>,
    #[serde(default)]
    generator: Option<String>,

    variants: Vec<SubTypeInfoOpt>,
    #[serde(default)]
    highlight: Option<HighlightInfo>,
}

#[derive(Debug, Deserialize)]
struct HighlightInfo {
    color: (u8, u8, u8),
    #[serde(default)]
    label: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SubTypeInfoOpt {
    #[serde(default)]
    names: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    animations: FNVMap<String, AnimationInfo>,
}

impl<'de> Deserialize<'de> for TintColor {
    fn deserialize<D>(deserializer: D) -> Result<TintColor, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(TintColorVisitor)
    }
}

struct TintColorVisitor;

impl<'de> Visitor<'de> for TintColorVisitor {
    type Value = TintColor;

    fn expecting(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        formatter.write_str("a color")
    }

    fn visit_str<E>(self, v: &str) -> Result<TintColor, E>
    where
        E: ::serde::de::Error,
    {
        if v.starts_with('#') {
            let col = &v[1..];
            if col.len() == 6 || col.len() == 8 {
                Ok(TintColor(
                    u8::from_str_radix(&col[..2], 16).map_err(E::custom)?,
                    u8::from_str_radix(&col[2..4], 16).map_err(E::custom)?,
                    u8::from_str_radix(&col[4..6], 16).map_err(E::custom)?,
                    if col.len() == 8 {
                        u8::from_str_radix(&col[6..8], 16).map_err(E::custom)?
                    } else {
                        255
                    },
                ))
            } else {
                Err(E::custom("Incorrect length for color"))
            }
        } else {
            Err(E::custom("Not a color"))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::assets::*;
    use crate::entity::ServerComponent;
    use std::env;
    use std::fs;
    use std::path::Path;

    #[test]
    fn try_load_entities() {
        let exe = env::current_exe().unwrap();
        let parent = exe.parent().unwrap();
        env::set_current_dir(parent.join("../../../")).unwrap();
        let log = ::slog::Logger::root(::slog::Discard, o!());
        let assets = AssetManager::with_packs(&log, &["base".to_owned()])
            .register::<super::Loader<ServerComponent>>()
            .build();
        load_dir(&assets, Path::new("./assets/base/base/entity"));
    }

    fn load_dir(assets: &AssetManager, dir: &Path) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                load_dir(assets, &path);
            } else {
                let path_str = path.to_string_lossy();
                let path_str = &path_str["./assets/base/base/entity/".len()..];
                let path_str = &path_str[..path_str.len() - 5];

                // Not a room file, just a list of them
                if path_str == "entities" || path_str.starts_with("names/") {
                    continue;
                }
                println!("Trying to load: {:?}", path_str);
                assets
                    .loader_open::<super::Loader<ServerComponent>>(ResourceKey::new(
                        "base", path_str,
                    ))
                    .unwrap();
            }
        }
    }
}
