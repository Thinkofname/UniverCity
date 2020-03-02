//! System used to evaluate a list of choices and select one.
//!
//! Can use random number generation to select choices as well
//! as rules.

use crate::prelude::*;
use rand::Rng;
use std::fmt::{self, Debug};

use serde::de::DeserializeOwned;

mod parse;
pub use self::parse::Type;
mod instruction;
use self::instruction::*;
mod component;
pub use self::component::*;

mod rule;
pub use self::rule::*;

/// Collection of precompiled choice rules
pub struct Choices {
    /// Global variable storage
    pub global: GlobalMemory,
    /// Rules for students
    pub student_idle: ChoiceSelector<ScriptChoice>,
}

/// The attributes for script based choices
#[derive(Debug, Clone)]
pub struct ScriptChoice {
    /// The name of the script that handles this choice
    pub script: ResourceKey<'static>,
}

/// The attributes for script based choices
#[derive(Debug, Deserialize)]
pub struct ScriptChoiceRaw {
    /// The name of the script that handles this choice
    pub script: String,
}

/// Converts from the raw format
pub fn convert_script_choice(module: ModuleKey<'_>, raw: ScriptChoiceRaw) -> ScriptChoice {
    ScriptChoice {
        script: LazyResourceKey::parse(&raw.script)
            .or_module(module)
            .into_owned(),
    }
}

/// A helper that can be used to store global variables
/// for rules
pub struct GlobalMemory {
    alloc: BasicAlloc<()>,
    memory: Vec<u32>,
}

impl GlobalMemory {
    /// Creates storage for global variables based on the
    /// passed allocator.
    #[inline]
    pub fn new(alloc: BasicAlloc<()>) -> GlobalMemory {
        GlobalMemory {
            memory: vec![0; alloc.storage_ty.len()],
            alloc,
        }
    }
}

impl GlobalMemory {
    /// Sets the value of the global variable
    #[inline]
    pub fn set_int(&mut self, name: &str, val: i32) {
        if let Some((_, idx)) = self.alloc.storage_ty.get(name) {
            self.memory[*idx as usize] = val as u32;
        } else {
            panic!("Missing global: {:?}", name)
        }
    }
    /// Sets the value of the global variable
    #[inline]
    pub fn set_float(&mut self, name: &str, val: f32) {
        self.set_int(name, val.to_bits() as i32);
    }
    /// Sets the value of the global variable
    #[inline]
    pub fn set_bool(&mut self, name: &str, val: bool) {
        self.set_int(name, val as i32);
    }

    /// Gets the value of the global variable
    #[inline]
    pub fn get_int(&mut self, name: &str) -> i32 {
        if let Some((_, idx)) = self.alloc.storage_ty.get(name) {
            self.memory[*idx as usize] as i32
        } else {
            panic!("Missing global: {:?}", name)
        }
    }
    /// Gets the value of the global variable
    #[inline]
    pub fn get_float(&mut self, name: &str) -> f32 {
        f32::from_bits(self.get_int(name) as u32)
    }
    /// Gets the value of the global variable
    #[inline]
    pub fn get_bool(&mut self, name: &str) -> bool {
        self.get_int(name) != 0
    }

    /// Wraps the global memory and entity memory in a way
    /// that can be used by rules
    pub fn wrap<'a, T>(&'a self, vars: &'a EntityVars<T>) -> VMem<'a> {
        VMem {
            memory: vars.slice(),
            global_memory: &self.memory,
        }
    }
}

/// A wrapper of entity memory and global memory for
/// use with rules
#[derive(Debug)]
pub struct VMem<'a> {
    memory: &'a [u32],
    global_memory: &'a [u32],
}

impl<'a> VariableAccess for VMem<'a> {
    #[inline]
    unsafe fn get(&self, idx: u16) -> u32 {
        *self.memory.get_unchecked(idx as usize)
    }
    #[inline]
    unsafe fn get_global(&self, idx: u16) -> u32 {
        *self.global_memory.get_unchecked(idx as usize)
    }
}

/// Chooses from a collection of choices based on variables
/// and an optional random chance
pub struct ChoiceSelector<E> {
    choices: Vec<Choice<E>>,
    by_name: FNVMap<ResourceKey<'static>, usize>,
}

impl<E> ChoiceSelector<E> {
    /// Creates a new `ChoiceSelector` from the named
    /// lists of rules
    pub fn new<F, R: DeserializeOwned>(
        log: &Logger,
        assets: &AssetManager,
        valloc: &mut impl VariableAllocator,
        resource: &str,
        convert: F,
    ) -> ChoiceSelector<E>
    where
        F: for<'a> Fn(ModuleKey<'a>, R) -> E,
    {
        let mut choices = Vec::new();
        for pack in assets.get_packs() {
            let f = if let Ok(f) =
                assets.open_from_pack(pack.borrow(), &format!("choice/{}.json", resource))
            {
                f
            } else {
                continue;
            };
            let c: ChoiceFile<R> = assume!(log, ::serde_json::from_reader(f));
            for (k, t) in c.vars {
                assume!(log, valloc.storage_loc(t, &k));
            }
            c.choices
                .into_iter()
                .map(|v| Choice {
                    name: LazyResourceKey::parse(&v.name)
                        .or_module(pack.borrow())
                        .into_owned(),
                    chance: v.chance,
                    when: assume!(log, Rules::parse(valloc, &v.when)),
                    order: v.order,
                    execute: convert(pack.borrow(), v.execute),
                })
                .for_each(|v| choices.push(v));
        }
        choices.sort_by_key(|v| v.order);
        let by_name = choices
            .iter()
            .enumerate()
            .map(|(idx, c)| (c.name.clone(), idx))
            .collect();
        ChoiceSelector { choices, by_name }
    }

    /// Selects a choice based on the passed list of variables
    #[inline]
    pub fn choose(&self, rng: &mut impl Rng, vars: &impl VariableAccess) -> Option<(usize, &E)> {
        for (idx, choice) in self.choices.iter().enumerate() {
            if choice.chance < 1.0 && rng.gen::<f32>() >= choice.chance {
                continue;
            }
            if choice.when.execute(vars) {
                return Some((idx, &choice.execute));
            }
        }
        None
    }

    /// Returns all matching choices based on the passed list of variables
    #[inline]
    pub fn matching<'a>(
        &'a self,
        vars: &'a impl VariableAccess,
    ) -> impl Iterator<Item = (usize, &'a E)> + 'a {
        self.choices
            .iter()
            .enumerate()
            .filter(move |(_, c)| c.when.execute(vars))
            .map(|(idx, c)| (idx, &c.execute))
    }

    /// Selects a choice at the given index
    #[inline]
    pub fn get_choice_by_index(&self, idx: usize) -> Option<&E> {
        self.choices.get(idx).map(|v| &v.execute)
    }

    /// Returns the choice index of the choice with the given
    /// name (if any).
    #[inline]
    pub fn get_choice_index_by_name(&self, name: ResourceKey<'_>) -> Option<usize> {
        self.by_name.get(&name).cloned()
    }
    /// Returns the choice name of the choice with the given
    /// index (if any).
    #[inline]
    pub fn get_choice_name_by_index(&self, idx: usize) -> Option<ResourceKey<'_>> {
        self.choices.get(idx).map(|v| v.name.borrow())
    }

    /// Returns the number of stored choices
    #[inline]
    pub fn num_choices(&self) -> usize {
        self.choices.len()
    }

    /// Iterates over all choices
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = (usize, &E)> {
        self.choices.iter().map(|v| &v.execute).enumerate()
    }
}

/// The component for student variables
pub struct StudentVars;
/// The component for Professor variables
pub struct ProfessorVars;
/// The component for Office workers variables
pub struct OfficeWorkerVars;
/// The component for Janitors variables
pub struct JanitorVars;

impl Component for StudentVars {
    type Storage = EntityVarStorage<StudentVars>;
}
impl Component for ProfessorVars {
    type Storage = EntityVarStorage<ProfessorVars>;
}
impl Component for OfficeWorkerVars {
    type Storage = EntityVarStorage<OfficeWorkerVars>;
}
impl Component for JanitorVars {
    type Storage = EntityVarStorage<JanitorVars>;
}

/// Helper for accessing vars from any type of entity
pub fn get_vars(entities: &mut Container, e: Entity) -> Option<choice::EntityVars<()>> {
    entities
        .get_custom::<StudentVars>(e)
        .map(|v| v.remove_type())
        .or_else(|| {
            entities
                .get_custom::<ProfessorVars>(e)
                .map(|v| v.remove_type())
        })
        .or_else(|| {
            entities
                .get_custom::<OfficeWorkerVars>(e)
                .map(|v| v.remove_type())
        })
        .or_else(|| {
            entities
                .get_custom::<JanitorVars>(e)
                .map(|v| v.remove_type())
        })
}

struct Choice<E> {
    name: ResourceKey<'static>,
    chance: f32,
    when: Rules,
    order: i32,
    execute: E,
}

#[derive(Deserialize, Debug)]
struct ChoiceFile<E> {
    #[serde(default)]
    vars: FNVMap<String, Type>,
    choices: Vec<ChoiceRaw<E>>,
}

#[derive(Deserialize, Debug)]
struct ChoiceRaw<E> {
    name: String,
    chance: f32,
    when: Vec<String>,
    #[serde(default)]
    order: i32,
    #[serde(flatten)]
    execute: E,
}
