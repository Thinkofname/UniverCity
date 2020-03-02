//! A multi-threaded entity component system without locks on components
//!
//! # Features
//!
//! - Fast, aims to have low overhead
//! - Threaded, systems will automatically be threaded where possible
//! - Simple, nothing complex in terms of api usage
//!
//! # How it works
//!
//! Systems declare the components they wish to access by using parameters.
//! A `Read<T>` parameter declares that the system only wishes to access
//! the component as read-only, whilst `Write<T>` declares that the system
//! may mutate the component (add/remove/edit).
//!
//! Internally a scheduler will execute all systems using a set number of
//! threads only allowing systems that can run safely in parallel to
//! run at any given time. The rules for this system are simple:
//! A component may have any number of readers at a given time as long
//! as it has no writers. Only a single system may write to a component
//! and no readers are allow whilst a system holds write access to a component.
//!
//! # Usage
//!
//! Firstly all structs that you wish to use as components must implement
//! the `Component` trait. This can be done with the `component!` macro.
//! (See `component!`'s documentation for the different types).
//! The component must then be registered with the `Container`.
//!
//! ```
//! # #[macro_use] extern crate think_ecs;
//! # use think_ecs::*;
//! # fn main() {
//! struct Position {
//!     x: i32,
//!     y: i32,
//! }
//! component!(Position => Vec);
//! let mut c = Container::new();
//! c.register_component::<Position>();
//! # }
//! ```
//!
//! Now the container can be used to create entities, add components
//! and access them.
//!
//! ```
//! # #[macro_use] extern crate think_ecs;
//! # use think_ecs::*;
//! # fn main() {
//! # #[derive(Debug, PartialEq, Eq)]
//! # struct Position {
//! #     x: i32,
//! #     y: i32,
//! # }
//! # component!(Position => Vec);
//! # let mut c = Container::new();
//! # c.register_component::<Position>();
//! let entity = c.new_entity();
//! c.add_component(entity, Position { x: 5, y: 10});
//! // Mutable access to the component
//! {
//!     let pos = c.get_component_mut::<Position>(entity).unwrap();
//!     pos.x += 4;
//! }
//! // Immutable access to the component
//! assert_eq!(c.get_component::<Position>(entity), Some(&Position { x: 9, y: 10}));
//! # }
//! ```
//!
//! Entities are generally processed via systems. Systems are just functions.
//! You can register a list of functions to be run via the `Systems` type.
//! When run `Systems` will automatically decide when to run a system based on
//! its parameters and what other systems are currently running. The order
//! that systems are run in is not defined.
//!
//! The functions take at least one parameter, a `EntityManager` reference.
//! This provides a interface to create and iterate over all entities in the
//! system. Other parameters must either be a `Read<T>` or `Write<T>` reference
//! where T is a component type. `Read` provides immutable access to a component
//! and `Write` provides mutable access (adding/removing the component as well).
//! Both the `Read` and `Write` types provide a `mask` method which can be used
//! with `EntityManager`'s `iter_mask` method to iterator over a subset of entities.
//! Masks can be chained as followed to iterator over the intersection of multiple
//! types `pos.mask().and(vel)`.
//!
//! ```
//! # #[macro_use] extern crate think_ecs;
//! # use think_ecs::*;
//! # fn main() {
//! # #[derive(Debug, PartialEq, Eq)]
//! # struct Position {
//! #     x: i32,
//! #     y: i32,
//! # }
//! # component!(Position => Vec);
//! # let mut c = Container::new();
//! # c.register_component::<Position>();
//! # let entity = c.new_entity();
//! # c.add_component(entity, Position { x: 5, y: 10});
//! let mut sys = Systems::new();
//! closure_system!(fn example(em: EntityManager, mut pos: Write<Position>) {
//!     let mask = pos.mask();
//!     for e in em.iter_mask(&mask) {
//!         let pos = pos.get_component_mut(e).unwrap();
//!         pos.y -= 3;
//!     }
//! });
//! sys.add(example);
//! sys.run(&mut c);
//! # }
//! ```
//!
//! # Quirks/Issues
//!
//! - Removing an entity whilst in a system wont actually remove it until after
//!   all systems have finished executing.

#![warn(missing_docs)]
// Not a fan of this style
#![allow(clippy::new_without_default)]
// Not always bad, mainly style
#![allow(clippy::single_match)]
// Used to make sure debug is stripped out
#![allow(clippy::inline_always)]
// Clippy bug? https://github.com/Manishearth/rust-clippy/issues/1725
#![allow(clippy::get_unwrap)]
// Sometimes things get complex
#![allow(clippy::cognitive_complexity)]
// I generally use this correctly. Its mostly done for
// networking.
#![allow(clippy::float_cmp)]
// Not making a library
#![allow(clippy::should_implement_trait)]
#![allow(clippy::clone_on_ref_ptr)]

mod storage;
pub use crate::storage::*;
mod internal;
pub use crate::internal::BoxedStorage as InternalBoxedStorage;
mod par;
pub use crate::par::*;
mod group;
pub use crate::group::*;
mod util;

use std::any::{Any, TypeId};
use std::fmt::{self, Debug, Formatter};
use std::marker::PhantomData;
use std::ops::Deref;
use std::sync::mpsc;
use std::sync::{Mutex, RwLock};

use rayon::iter::plumbing::{bridge_unindexed, Folder, UnindexedConsumer, UnindexedProducer};
use rayon::iter::ParallelIterator;

/// Collection of entities and their components.
pub struct Container {
    entities: RwLock<internal::EntityAllocator>,
    components: internal::ComponentStore,
}

impl Container {
    /// Returns an entity that represents the world.
    ///
    /// This entity is generally used to pass state into systems
    pub const WORLD: Entity = Entity {
        id: 0,
        generation: 0,
    };

    /// Creates a new empty entity container.
    pub fn new() -> Container {
        Container {
            entities: RwLock::new(internal::EntityAllocator::new()),
            components: internal::ComponentStore::new(),
        }
    }

    /// Allocates a new entity which can have components attached to
    /// it.
    ///
    /// This may reuse entity ids from removed entities and not every
    /// method checks if the entity is valid before accessing it.
    /// (Namely within systems). If this is required then `is_valid`
    /// can be used to check manually.
    #[inline]
    pub fn new_entity(&mut self) -> Entity {
        self.entities
            .get_mut()
            .expect("Failed to lock entities")
            .alloc()
    }

    /// Removes the entity and all allocated components attached to it.
    #[inline]
    pub fn remove_entity(&mut self, e: Entity) {
        if !self
            .entities
            .get_mut()
            .expect("Failed to lock entities")
            .free(e)
        {
            return;
        }
        self.components.free_all_components(e.id);
    }

    /// Returns whether the entity is still valid.
    ///
    /// Entity ids can be reused but the generation will be changed allowing
    /// for you to detect whether this entity is still relevant.
    #[inline]
    pub fn is_valid(&self, e: Entity) -> bool {
        self.entities
            .read()
            .expect("Failed to lock entities")
            .is_valid(e)
    }

    /// Registers the component allowing it to be attached to entities.
    ///
    /// This must be done before a component is used.
    #[inline]
    pub fn register_component<T: Component>(&mut self) {
        self.components.register_component::<T>();
    }

    /// Registers the component allowing it to be attached to entities.
    ///
    /// This must be done before a component is used.
    #[inline]
    pub fn register_component_self<T: Component>(&mut self, storage: T::Storage) {
        self.components.register_component_self::<T>(storage);
    }

    /// Returns an iterator that iterates over every active entity.
    #[inline]
    pub fn iter_all(
        &self,
    ) -> MaskedEntityIter<
        impl Deref<Target = internal::EntityAllocator> + '_,
        impl for<'b> Fn(&'b internal::EntityAllocator, usize) -> bool + '_,
    > {
        let ea = self.entities.read().expect("Failed to lock entities");
        MaskedEntityIter {
            id: 0,
            max: ea.max_entities,
            entities: ea,
            test_mask: |e: &internal::EntityAllocator, i| e.entities.get(i),
        }
    }

    /// Returns an iterator that iterates over every active entity
    /// which contains the components represented by the passed mask.
    #[inline]
    pub fn iter_mask<'a>(
        &'a self,
        mask: &'a EntityMask,
    ) -> MaskedEntityIter<
        impl Deref<Target = internal::EntityAllocator> + 'a,
        impl for<'b> Fn(&'b internal::EntityAllocator, usize) -> bool + 'a,
    > {
        let ea = self.entities.read().expect("Failed to lock entities");
        MaskedEntityIter {
            entities: ea,
            id: 0,
            max: mask.max,
            test_mask: move |_: &internal::EntityAllocator, i| mask.mask.get(i),
        }
    }

    /// Returns a mask which contains every entity with this component
    #[inline]
    pub fn mask_for<T: Component>(&self) -> EntityMask {
        let wrap = unsafe {
            &mut *self
                .components
                .components
                .get(&TypeId::of::<T>())
                .expect("Component not registered")
                .get()
        };
        EntityMask {
            mask: wrap.mask.clone(),
            max: wrap.max as u32,
        }
    }

    /// Returns a `Read` component accessor which can be used to quickly
    /// access components.
    pub fn component_read<'a, T: Component>(&'a self) -> Read<'a, T> {
        use self::internal::Accessor;
        Read::new(&self.components)
    }

    /// Returns a `Write` component accessor which can be used to quickly
    /// access components.
    pub fn component_write<'a, T: Component>(&'a mut self) -> Write<'a, T> {
        use self::internal::Accessor;
        Write::new(&self.components)
    }

    /// Adds a component to an entity.
    #[inline]
    pub fn add_component<T: Component>(&mut self, e: Entity, val: T) {
        if !self
            .entities
            .get_mut()
            .expect("Failed to lock entities")
            .is_valid(e)
        {
            return;
        }
        self.components.add_component(e.id, val)
    }

    /// Removes a component from an entity.
    #[inline]
    pub fn remove_component<T: Component>(&mut self, e: Entity) -> Option<T> {
        if !self
            .entities
            .get_mut()
            .expect("Failed to lock entities")
            .is_valid(e)
        {
            return None;
        }
        self.components.remove_component(e.id)
    }

    /// Gets an immutable reference to a component from an entity.
    #[inline]
    pub fn get_component<T: Component>(&self, e: Entity) -> Option<&T> {
        if !self
            .entities
            .read()
            .expect("Failed to lock entities")
            .is_valid(e)
        {
            return None;
        }
        self.components.get_component(e.id)
    }

    /// Gets an mutable reference to a component from an entity.
    #[inline]
    pub fn get_component_mut<T: Component>(&mut self, e: Entity) -> Option<&mut T> {
        if !self
            .entities
            .get_mut()
            .expect("Failed to lock entities")
            .is_valid(e)
        {
            return None;
        }
        self.components.get_component_mut(e.id)
    }

    /// Gets a custom value from an entity
    #[inline]
    pub fn get_custom<T>(&mut self, e: Entity) -> Option<<T::Storage as StorageCustom>::Value>
    where
        T: Component,
        T::Storage: StorageCustom,
    {
        self.component_write::<T>().get_custom(e)
    }

    /// Runs the passed function like a system
    pub fn with<'a, F, D>(&mut self, f: F) -> F::Return
    where
        F: IntoWithSystem<'a, D> + 'a,
    {
        let (send, recv) = mpsc::channel();
        let ret = {
            let param = internal::SystemParam {
                entities: &self.entities,
                components: &mut self.components,
                kill_chan: Mutex::new(send),
            };
            f.run_system(&param)
        };
        let entities = self.entities.get_mut().expect("Failed to lock entities");
        for e in recv {
            if !entities.free(e) {
                continue;
            }
            self.components.free_all_components(e.id);
        }
        ret
    }
}

/// Represents an entity that exists or did exist within a
/// `Container`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Entity {
    id: u32,
    generation: u32,
}

impl Default for Entity {
    #[inline]
    fn default() -> Entity {
        Entity::INVALID
    }
}

impl Debug for Entity {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> fmt::Result {
        write!(fmt, "Entity({}@{})", self.id, self.generation)
    }
}

impl Entity {
    /// An invalid entity reference
    pub const INVALID: Entity = Entity {
        id: 0,
        generation: 0xFFFF_FFFF,
    };

    /// Returns whether this entity is invalid or not.
    ///
    /// An entity not being invalid does not mean that it
    /// is usable, just that it is not equal to the invalid
    /// entity created with `invalid()`
    #[inline]
    pub fn is_invalid(self) -> bool {
        self == Self::INVALID
    }
}

/// A component that can be attached to an entity.
pub trait Component: Any + Sized {
    /// The type of storage to be used to store the component
    /// when attached to an entity.
    type Storage: ComponentStorage<Self>;
}

impl<K, V, S> Component for ::std::collections::HashMap<K, V, S>
where
    K: Sync + Send + 'static,
    V: Sync + Send + 'static,
    S: Sync + Send + 'static,
{
    type Storage = MutWorldStore<Self>;
}

/// Mark a type as a component and specify what type of storage it should use.
///
/// Three types of storage are available, `Map`, `Vec` and `Marker` each with
/// their own pros and cons to using them.
///
/// # Map
///
/// ```
/// # #[macro_use] extern crate think_ecs;
/// # use think_ecs::*;
/// # fn main() {
/// # struct Position;
/// component!(Position => Map);
/// # }
/// ```
///
/// `Map` based storage is backed by a `HashMap`. This doesn't waste memory when
/// an entity doesn't use the component.
///
/// ## Pros
///
/// - No wasted memory when an entity doesn't use the component
///
/// ## Cons
///
/// - Slower lookup times compared to other options
/// - Isn't cache friendly
/// - If used on a large number entities this uses more memory than
///   other types
///
/// # Vec
///
/// ```
/// # #[macro_use] extern crate think_ecs;
/// # use think_ecs::*;
/// # fn main() {
/// # struct Position;
/// component!(Position => Vec);
/// # }
/// ```
///
/// `Vec` based storage is backed by a `Vec`. This allocates space for a component
/// whether an entity uses it or not but provides fast lookup times.
///
/// ## Pros
///
/// - Fast lookup times
/// - Cache friendly
///
/// ## Cons
///
/// - Wastes memory when an entity doesn't use the component
///
/// # Marker
///
/// ```
/// # #[macro_use] extern crate think_ecs;
/// # use think_ecs::*;
/// # fn main() {
/// # #[derive(Default)] struct Position;
/// component!(Position => Marker);
/// # }
/// ```
///
/// `Marker` isn't backed by anything. This simply returns the `Default::default()`
/// value for every `get_*` call if the entity has the component.
///
/// ## Pros
///
/// - Fast
/// - Memory usage is the size of one component
/// - Useful for marker components
///
/// ## Cons
///
/// - Can't be used for anything more than saying whether an entity has the component
///   as the component value is shared between all entities.
#[macro_export]
macro_rules! component {
    ($ty:ty => Map) => {
        impl $crate::Component for $ty {
            type Storage = $crate::MapStorage<$ty>;
        }
    };
    ($ty:ty => Vec) => {
        impl $crate::Component for $ty {
            type Storage = $crate::VecStorage<$ty>;
        }
    };
    ($ty:ty => Marker) => {
        impl $crate::Component for $ty {
            type Storage = $crate::DefaultStorage<$ty>;
        }
    };
    ($ty:ty => const World) => {
        impl $crate::Component for $ty {
            type Storage = $crate::ConstWorldStore<$ty>;
        }
    };
    ($ty:ty => mut World) => {
        impl $crate::Component for $ty {
            type Storage = $crate::MutWorldStore<$ty>;
        }
    };
}

#[cfg(test)]
mod tests;

/// A collection of systems to be run on a `Container`.
///
/// Systems are just functions. When run this will automatically decide when
/// to run a system based on its parameters and what other systems are
/// currently running. The order that systems are run in is not defined.
pub struct Systems {
    scheduler: internal::Scheduler,
}

impl Systems {
    /// Allocates a system with 4 threads for executing systems
    pub fn new() -> Systems {
        Systems {
            scheduler: internal::Scheduler::new(4),
        }
    }

    /// Adds the passed function to the system collection.
    ///
    /// The functions take at least one parameter, a `EntityManager` reference.
    /// This provides a interface to create and iterate over all entities in the
    /// system. Other parameters must either be a `Read<T>` or `Write<T>` reference
    /// where T is a component type. `Read` provides immutable access to a component
    /// and `Write` provides mutable access (adding/removing the component as well).
    /// Both the `Read` and `Write` types provide a `mask` method which can be used
    /// with `EntityManager`'s `iter_mask` method to iterator over a subset of entities.
    /// Masks can be chained as followed to iterator over the intersection of multiple
    /// types `pos.mask().and(vel)`.
    pub fn add<S, D>(&mut self, system: S)
    where
        S: IntoSyncSystem<D>,
    {
        system.into_system(&mut self.scheduler);
    }
    //     where S: for<'a> SyncComponentSystem<'a> + Send + Sync + 'static,
    // {
    //     self.scheduler.add(system);
    // }

    /// Runs all systems on the passed container.
    ///
    /// # Panics
    ///
    /// Panics if one of the systems panics (not with the same panic the system threw
    /// however).
    #[inline]
    pub fn run(&mut self, container: &mut Container) {
        use std::panic;
        if let Err(err) = self.run_internal(container) {
            panic::resume_unwind(err);
        }
    }

    fn run_internal(&mut self, container: &mut Container) -> Result<(), Box<dyn Any + Send>> {
        let (send, recv) = mpsc::channel();
        {
            let param = internal::SystemParam {
                entities: &container.entities,
                components: &mut container.components,
                kill_chan: Mutex::new(send),
            };
            self.scheduler.run(&param)
        }
        for e in recv {
            container.remove_entity(e);
        }
        Ok(())
    }

    /// Returns a builder which allows attaching temporary values to the world
    /// before running and releasing them afterwords
    #[inline]
    pub fn run_with_borrows<'a>(&'a mut self, container: &'a mut Container) -> BorrowBuilder<'a> {
        BorrowBuilder {
            sys: self,
            container,
            to_remove: vec![],
        }
    }
}

/// Used for building up temporary borrows to be attached to the world
pub struct BorrowBuilder<'a> {
    sys: &'a mut Systems,
    container: &'a mut Container,
    to_remove: Vec<TypeId>,
}

impl<'a> BorrowBuilder<'a> {
    /// Borrows a immutable reference and attaches it to the world.
    pub fn borrow<T>(mut self, val: &'a T) -> BorrowBuilder<'a>
    where
        T: Component<Storage = ConstWorldStore<T>> + Send + Sync,
    {
        let id = TypeId::of::<T>();
        {
            let store = unsafe {
                &mut *self
                    .container
                    .components
                    .components
                    .get_mut(&id)
                    .unwrap()
                    .get()
            };
            store.mask.set(0, true);
            let s = store
                .store
                .as_mut_any()
                .downcast_mut::<ConstWorldStore<T>>()
                .unwrap();
            s.val = val;
        }
        self.to_remove.push(id);
        self
    }

    /// Borrows a mutable reference and attaches it to the world.
    pub fn borrow_mut<T>(mut self, val: &'a mut T) -> BorrowBuilder<'a>
    where
        T: Component<Storage = MutWorldStore<T>> + Send + Sync,
    {
        let id = TypeId::of::<T>();
        {
            let store = unsafe {
                &mut *self
                    .container
                    .components
                    .components
                    .get_mut(&id)
                    .unwrap()
                    .get()
            };
            store.mask.set(0, true);
            let s = store
                .store
                .as_mut_any()
                .downcast_mut::<MutWorldStore<T>>()
                .unwrap();
            s.val = val;
        }
        self.to_remove.push(id);
        self
    }

    /// Runs all systems on the passed container. Releases all borrows once complete.
    ///
    /// # Panics
    ///
    /// Panics if one of the systems panics (not with the same panic the system threw
    /// however).
    pub fn run(self) {
        use std::panic;
        let err = self.sys.run_internal(self.container);
        for id in self.to_remove {
            let store = unsafe {
                &mut *self
                    .container
                    .components
                    .components
                    .get_mut(&id)
                    .unwrap()
                    .get()
            };
            store.mask.set(0, false);
            store.store.free_id(0);
        }
        if let Err(err) = err {
            panic::resume_unwind(err);
        }
    }
}

/// Access to entities whilst in a system
pub struct EntityManager<'a> {
    entities: &'a RwLock<internal::EntityAllocator>,
    kill_chan: &'a Mutex<mpsc::Sender<Entity>>,
}

impl<'a> EntityManager<'a> {
    /// Returns whether the entity is still valid.
    ///
    /// Entity ids can be reused but the generation will be changed allowing
    /// for you to detect whether this entity is still relevant.
    #[inline]
    pub fn is_valid(&self, e: Entity) -> bool {
        let ea = self.entities.read().unwrap();
        ea.is_valid(e)
    }

    /// Allocates a new entity which can have components attached to
    /// it.
    ///
    /// This may reuse entity ids from removed entities and not every
    /// method checks if the entity is valid before accessing it.
    /// (Namely within systems). If this is required then `is_valid`
    /// can be used to check manually.
    #[inline]
    pub fn new_entity(&self) -> Entity {
        let mut ea = self.entities.write().unwrap();
        ea.alloc()
    }

    /// Removes the entity and all allocated components attached to it.
    #[inline]
    pub fn remove_entity(&self, e: Entity) {
        let k = self.kill_chan.lock().unwrap();
        k.send(e).unwrap();
    }

    /// Returns an iterator that iterates over every active entity.
    #[inline]
    pub fn iter_all(
        &'a self,
    ) -> MaskedEntityIter<
        impl Deref<Target = internal::EntityAllocator> + '_,
        impl for<'b> Fn(&'b internal::EntityAllocator, usize) -> bool + '_,
    > {
        let ea = self.entities.read().unwrap();
        MaskedEntityIter {
            id: 0,
            max: ea.max_entities,
            test_mask: |e: &internal::EntityAllocator, i| e.entities.get(i),
            entities: ea,
        }
    }

    /// Returns an iterator that iterates over every active entity
    /// which contains the components represented by the passed mask.
    #[inline]
    pub fn iter_mask(
        &'a self,
        mask: &'a EntityMask,
    ) -> MaskedEntityIter<
        impl Deref<Target = internal::EntityAllocator> + '_,
        impl for<'b> Fn(&'b internal::EntityAllocator, usize) -> bool + '_,
    > {
        let ea = self.entities.read().unwrap();
        MaskedEntityIter {
            entities: ea,
            id: 0,
            max: mask.max,
            test_mask: move |_: &internal::EntityAllocator, i| mask.mask.get(i),
        }
    }

    /// Returns an iterator that iterates over every active entity
    /// which contains the components passed in.
    #[inline]
    pub fn par_group<F>(&'a self, components: F) -> GroupPar<'a, F>
    where
        F: FetchableComponent<'a>,
        F::Component: Send + Sync,
    {
        let mask = components.mask();
        let mut est_size = mask.mask.data.len() * 64;
        est_size -= mask
            .mask
            .data
            .iter()
            .rev()
            .position(|v| *v != 0)
            .unwrap_or(est_size);
        GroupPar {
            entities: self.entities,
            components,
            est_size,
            mask,
        }
    }

    /// Returns an iterator that iterates over every active entity
    /// which contains the components passed in.
    #[inline]
    pub fn par_group_mask<'b, F, MaskOp>(&'a self, components: F, op: MaskOp) -> GroupPar<'a, F>
    where
        F: FetchableComponent<'a>,
        F::Component: Send + Sync,
        MaskOp: FnOnce(EntityMask) -> EntityMask + 'b,
    {
        let mask = components.mask();
        let mask = op(mask);
        let mut est_size = mask.mask.data.len() * 64;
        est_size -= mask
            .mask
            .data
            .iter()
            .rev()
            .position(|v| *v != 0)
            .unwrap_or(est_size);
        GroupPar {
            entities: self.entities,
            components,
            est_size,
            mask,
        }
    }

    /// Returns an iterator that iterates over every active entity
    /// which contains the components passed in.
    #[inline]
    pub fn group<F>(&'a self, components: F) -> Group<'a, F>
    where
        F: FetchableComponent<'a>,
    {
        let mask = components.mask();
        let mut est_size = mask.mask.data.len() * 64;
        est_size -= mask
            .mask
            .data
            .iter()
            .rev()
            .position(|v| *v != 0)
            .unwrap_or(est_size);
        Group {
            entities: self.entities,
            components,
            est_size,
            mask,
            offset: 0,
        }
    }

    /// Returns an iterator that iterates over every active entity
    /// which contains the components passed in.
    #[inline]
    pub fn group_mask<'b, F, MaskOp>(&'a self, components: F, op: MaskOp) -> Group<'a, F>
    where
        F: FetchableComponent<'a>,
        MaskOp: FnOnce(EntityMask) -> EntityMask + 'b,
    {
        let mask = components.mask();
        let mask = op(mask);
        let mut est_size = mask.mask.data.len() * 64;
        est_size -= mask
            .mask
            .data
            .iter()
            .rev()
            .position(|v| *v != 0)
            .unwrap_or(est_size);
        Group {
            entities: self.entities,
            components,
            est_size,
            mask,
            offset: 0,
        }
    }
}

/// A mask which represents a sub-set of entities
///
/// Created by `Read::mask` and `Write::mask`
#[derive(Clone)]
pub struct EntityMask {
    mask: util::BitSet,
    max: u32,
}

impl EntityMask {
    /// Returns a mask which is the intersection of this mask and the
    /// entities with the component (represented by the accessor).
    #[inline]
    pub fn and<T: ComponentAccessor>(self, other: &T) -> EntityMask {
        other.and_mask(self)
    }

    /// Returns a mask which is the intersection of this mask and the
    /// entities without the component (represented by the accessor).
    #[inline]
    pub fn and_not<T: ComponentAccessor>(self, other: &T) -> EntityMask {
        other.and_not_mask(self)
    }

    /// Returns a mask which is the intersection of this mask and the
    /// entities with the component.
    #[inline]
    pub fn and_component<T: Component>(mut self, c: &Container) -> EntityMask {
        use std::cmp::min;
        let wrap = unsafe {
            &*c.components
                .components
                .get(&TypeId::of::<T>())
                .expect("Component not registered")
                .get()
        };
        self.max = min(self.max, wrap.max as u32);
        self.mask.and(&wrap.mask);
        self
    }

    /// Returns a mask which is the intersection of this mask and the
    /// entities without the component.
    #[inline]
    pub fn and_not_component<T: Component>(mut self, c: &Container) -> EntityMask {
        use std::cmp::max;
        let wrap = unsafe {
            &*c.components
                .components
                .get(&TypeId::of::<T>())
                .expect("Component not registered")
                .get()
        };
        self.max = max(self.max, wrap.max as u32);
        self.mask.and_not(&wrap.mask);
        self
    }

    fn and_mask(mut self, other: EntityMask) -> EntityMask {
        use std::cmp::min;
        self.max = min(self.max, other.max);
        self.mask.and(&other.mask);
        self
    }
}

/// Iterates over a sub-set of entities
pub struct MaskedEntityIter<T, F> {
    entities: T,
    id: u32,
    max: u32,
    test_mask: F,
}

impl<T, F> Iterator for MaskedEntityIter<T, F>
where
    T: Deref<Target = internal::EntityAllocator>,
    F: for<'a> Fn(&'a internal::EntityAllocator, usize) -> bool,
{
    type Item = Entity;

    #[inline]
    fn next(&mut self) -> Option<Entity> {
        while self.id < self.max {
            let id = self.id;
            self.id += 1;
            if (self.test_mask)(&self.entities, id as usize) {
                return Some(Entity {
                    id,
                    generation: self.entities.generations[id as usize],
                });
            }
        }
        None
    }
}

/// A function which is a system.
pub trait SystemFunction: internal::InternalFunction {}

/// A type that can be used to provide access to components
/// whilst executing a system.
pub trait ComponentAccessor: internal::Accessor {}

/// Marks the accessor as being safe to use via threads
pub unsafe trait SyncComponentAccessor {}

/// Marks the system's components as being safe to use via threads
pub unsafe trait SyncComponentSystem<'a>: System<'a> {}

unsafe impl<'a, T> SyncComponentSystem<'a> for T
where
    T: System<'a>,
    T::Param: SyncComponentAccessor,
{
}

/// Allows storages to return a custom value
pub trait StorageCustom {
    /// The return value
    type Value;

    /// Returns a custom value for the entity with the
    /// given id
    fn get(&mut self, id: u32) -> Self::Value;
}

/// Provides mutable access to a component whilst executing a system.
pub struct Write<'a, T: Component> {
    _t: PhantomData<&'a T>,
    wrap: *mut internal::StoreWrap,
    storage: *mut T::Storage,
}

impl<'a, T> Write<'a, T>
where
    T: Component,
    T::Storage: StorageCustom,
{
    /// Gets a custom value from an entity
    #[inline]
    pub fn get_custom(&mut self, e: Entity) -> Option<<T::Storage as StorageCustom>::Value> {
        let storage = unsafe { &mut *self.storage };
        if !T::Storage::self_bookkeeps() && !unsafe { &*self.wrap }.mask.get(e.id as usize) {
            return None;
        }
        Some(storage.get(e.id))
    }
}

impl<'a, T: Component> Write<'a, T> {
    /// Returns a read accessor of this component type
    pub fn read(&self) -> Read<'a, T> {
        Read {
            _t: PhantomData,
            wrap: self.wrap,
            storage: self.storage,
        }
    }
    /// Gets an immutable reference to a component from an entity.
    #[inline]
    #[allow(clippy::ref_in_deref)]
    pub fn get_component(&self, e: Entity) -> Option<&T> {
        let storage = unsafe { &*self.storage };
        if !T::Storage::self_bookkeeps() {
            unsafe {
                if (&*self.wrap).mask.get(e.id as usize) {
                    Some(storage.get_unchecked_component(e.id))
                } else {
                    None
                }
            }
        } else {
            storage.get_component(e.id)
        }
    }

    /// Gets an mutable reference to a component from an entity.
    #[inline]
    #[allow(clippy::ref_in_deref)]
    pub fn get_component_mut(&mut self, e: Entity) -> Option<&mut T> {
        let storage = unsafe { &mut *self.storage };
        if !T::Storage::self_bookkeeps() {
            unsafe {
                if (&*self.wrap).mask.get(e.id as usize) {
                    Some(storage.get_unchecked_component_mut(e.id))
                } else {
                    None
                }
            }
        } else {
            storage.get_component_mut(e.id)
        }
    }

    /// Gets an mutable reference to a component from an entity.
    ///
    /// Creates the component with the given function if it doesn't
    /// exist
    #[inline]
    #[allow(clippy::ref_in_deref)]
    pub fn get_component_or_insert<F>(&mut self, e: Entity, f: F) -> &mut T
    where
        F: FnOnce() -> T,
    {
        use std::cmp;
        let storage = unsafe { &mut *self.storage };
        let wrap = unsafe { &mut *self.wrap };
        if wrap.max <= e.id as usize {
            wrap.max = cmp::max(wrap.max * 2, e.id as usize + 1);
            wrap.mask.resize(wrap.max);
        }
        if !T::Storage::self_bookkeeps() {
            unsafe {
                if (&*self.wrap).mask.get(e.id as usize) {
                    storage.get_unchecked_component_mut(e.id)
                } else {
                    wrap.mask.set(e.id as usize, true);
                    storage.add_and_get_component(e.id, f())
                }
            }
        } else {
            wrap.mask.set(e.id as usize, true);
            storage.get_component_or_insert(e.id, f)
        }
    }

    /// Adds a component to an entity.
    #[inline]
    pub fn add_component(&mut self, e: Entity, val: T) {
        use std::cmp;
        let storage = unsafe { &mut *self.storage };
        let wrap = unsafe { &mut *self.wrap };
        if wrap.max <= e.id as usize {
            wrap.max = cmp::max(wrap.max * 2, e.id as usize + 1);
            wrap.mask.resize(wrap.max);
        }
        if !T::Storage::self_bookkeeps() && wrap.mask.get(e.id as usize) {
            storage.remove_component(e.id);
        }
        wrap.mask.set(e.id as usize, true);
        storage.add_component(e.id, val)
    }

    /// Removes a component from an entity.
    #[inline]
    pub fn remove_component(&mut self, e: Entity) -> Option<T> {
        let storage = unsafe { &mut *self.storage };
        let wrap = unsafe { &mut *self.wrap };
        if !T::Storage::self_bookkeeps() && !wrap.mask.get(e.id as usize) {
            return None;
        }
        wrap.mask.set(e.id as usize, false);
        storage.remove_component(e.id)
    }

    /// Returns a mask which contains every entity with this component
    #[inline]
    pub fn mask(&self) -> EntityMask {
        let wrap = unsafe { &*self.wrap };
        EntityMask {
            mask: wrap.mask.clone(),
            max: wrap.max as u32,
        }
    }

    /// Returns a parallel iterator over the components
    #[inline]
    pub fn par_iter<'b>(&'b mut self, mask: &'b EntityMask) -> WriteParIter<'b, T>
    where
        T: Sync + Send,
    {
        let mut est_size = mask.mask.data.len() * 64;
        est_size -= mask
            .mask
            .data
            .iter()
            .rev()
            .position(|v| *v != 0)
            .unwrap_or(est_size);

        WriteParIter {
            inner: self,
            est_size,
            mask,
        }
    }
}
impl<'a, T: Component> ComponentAccessor for Write<'a, T> {}
unsafe impl<'a, T> SyncComponentAccessor for Write<'a, T> where T: Component + Sync + Send {}
impl<'a, T: Component> internal::Accessor for Write<'a, T> {
    type Component = T;

    #[inline]
    fn ctype() -> internal::CType {
        internal::CType::Write(TypeId::of::<T>())
    }

    fn new(store: &internal::ComponentStore) -> Self {
        let back_store = unsafe { &mut *store.components.get(&TypeId::of::<T>()).unwrap().get() };
        Write {
            _t: PhantomData,
            storage: back_store.store.as_mut_any().downcast_mut().unwrap(),
            wrap: back_store,
        }
    }

    #[inline]
    fn and_mask(&self, mut mask: EntityMask) -> EntityMask {
        use std::cmp::min;
        let wrap = unsafe { &*self.wrap };
        mask.max = min(mask.max, wrap.max as u32);
        mask.mask.and(&wrap.mask);
        mask
    }

    #[inline]
    fn and_not_mask(&self, mut mask: EntityMask) -> EntityMask {
        use std::cmp::max;
        let wrap = unsafe { &*self.wrap };
        mask.max = max(mask.max, wrap.max as u32);
        mask.mask.and_not(&wrap.mask);
        mask
    }
}

/// Provides immutable access to a component whilst executing a system.
pub struct Read<'a, T: Component> {
    _t: PhantomData<&'a T>,
    wrap: *const internal::StoreWrap,
    storage: *const T::Storage,
}

// Only immutable access so its safe to access from multiple threads
unsafe impl<'a, T: Component + Send> Send for Read<'a, T> {}
unsafe impl<'a, T: Component + Sync> Sync for Read<'a, T> {}

impl<'a, T: Component> Read<'a, T> {
    /// Gets an immutable reference to a component from an entity.
    #[inline]
    #[allow(clippy::ref_in_deref)]
    pub fn get_component(&self, e: Entity) -> Option<&T> {
        let storage = unsafe { &*self.storage };
        if !T::Storage::self_bookkeeps() {
            unsafe {
                if (&*self.wrap).mask.get(e.id as usize) {
                    Some(storage.get_unchecked_component(e.id))
                } else {
                    None
                }
            }
        } else {
            storage.get_component(e.id)
        }
    }

    /// Returns a mask which contains every entity with this component
    #[inline]
    pub fn mask(&self) -> EntityMask {
        let wrap = unsafe { &*self.wrap };
        EntityMask {
            mask: wrap.mask.clone(),
            max: wrap.max as u32,
        }
    }

    /// Returns a parallel iterator over the components
    #[inline]
    pub fn par_iter<'b>(&'b self, mask: &'b EntityMask) -> ReadParIter<'b, T>
    where
        T: Sync + Send,
    {
        let mut est_size = mask.mask.data.len() * 64;
        est_size -= mask
            .mask
            .data
            .iter()
            .rev()
            .position(|v| *v != 0)
            .unwrap_or(est_size);

        ReadParIter {
            inner: self,
            est_size,
            mask,
        }
    }
}
impl<'a, T: Component> ComponentAccessor for Read<'a, T> {}
unsafe impl<'a, T> SyncComponentAccessor for Read<'a, T> where T: Component + Sync + Send {}
impl<'a, T: Component> internal::Accessor for Read<'a, T> {
    type Component = T;

    #[inline]
    fn ctype() -> internal::CType {
        internal::CType::Read(TypeId::of::<T>())
    }

    fn new(store: &internal::ComponentStore) -> Self {
        let back_store = unsafe { &*store.components.get(&TypeId::of::<T>()).unwrap().get() };
        Read {
            _t: PhantomData,
            storage: back_store.store.as_any().downcast_ref().unwrap(),
            wrap: back_store,
        }
    }

    #[inline]
    fn and_mask(&self, mut mask: EntityMask) -> EntityMask {
        use std::cmp::min;
        let wrap = unsafe { &*self.wrap };
        mask.max = min(mask.max, wrap.max as u32);
        mask.mask.and(&wrap.mask);
        mask
    }

    #[inline]
    fn and_not_mask(&self, mut mask: EntityMask) -> EntityMask {
        use std::cmp::max;
        let wrap = unsafe { &*self.wrap };
        mask.max = max(mask.max, wrap.max as u32);
        mask.mask.and_not(&wrap.mask);
        mask
    }
}

/// A system is used to access components and perform actions on them
pub trait System<'a> {
    /// The parameters the system needs to access
    type Param: AccessorSet;

    /// Called every tick of the world
    fn run(&self, em: EntityManager, param: Self::Param);
}

/// A set of accessors
pub trait AccessorSet: internal::AccessorSet {}

impl<T> AccessorSet for T where T: internal::AccessorSet {}

/// Helper trait to convert types into systems
pub trait IntoSyncSystem<Dummy> {
    #[doc(hidden)]
    fn into_system(self, scheduler: &mut internal::Scheduler);
}

impl<S> IntoSyncSystem<S> for S
where
    S: for<'a> SyncComponentSystem<'a> + Send + Sync + 'static,
{
    fn into_system(self, scheduler: &mut internal::Scheduler) {
        scheduler.add(self);
    }
}
/// Helper trait to convert types into systems
pub trait IntoSystem<'a, Dummy> {
    #[doc(hidden)]
    fn run_system(self, sysparam: &internal::SystemParam);
}

/// Helper trait to convert types into systems
pub trait IntoWithSystem<'a, Dummy> {
    /// The return value of the system
    type Return;
    #[doc(hidden)]
    fn run_system(self, sysparam: &internal::SystemParam) -> Self::Return;
}

impl<'a, S> IntoSystem<'a, S> for S
where
    S: for<'inner> System<'inner> + 'a,
{
    fn run_system(self, sysparam: &internal::SystemParam) {
        use crate::internal::AccessorSet;
        let entities = EntityManager {
            kill_chan: &sysparam.kill_chan,
            entities: &sysparam.entities,
        };
        let param: S::Param = S::Param::create(&sysparam.components);
        self.run(entities, param);
    }
}
impl<'a, S> IntoWithSystem<'a, S> for S
where
    S: IntoSystem<'a, S>,
{
    type Return = ();
    fn run_system(self, sysparam: &internal::SystemParam) -> Self::Return {
        IntoSystem::run_system(self, sysparam)
    }
}

/// Helper macro to create systems from functions
#[macro_export]
macro_rules! closure_system {
    (
$(#[$attr:meta])*
$v:vis fn $name:ident($em:ident : $emt:ty, $( $($pname:ident)+ : $pty:ident<$ity:ty> ),*) $body:block
    ) => {
#[allow(non_camel_case_types)]
$(#[$attr])*
$v struct $name;

impl <'a> System<'a> for $name {
    type Param = (
        $(
            $crate::$pty<'a, $ity>,
        )*
    );

    fn run(&self, $em: $emt, ( $( $($pname)* , )* ): Self::Param) {
        $body
    }
}
    };
}

#[doc(hidden)]
#[macro_export(local_inner_macros)]
macro_rules! closure_system_get_param {
    (mut $name:ident) => {
        mut $name
    };
    ($name:ident) => {
        $name
    };
}

macro_rules! impl_system_function {
    ($wrapname:ident => $($param:ident),*) => (

        impl <$($param,)*> internal::AccessorSet for ($($param,)*)
            where $($param: internal::AccessorSet,)*
        {
            fn collect_ctypes(__types: &mut Vec<internal::CType>) {
                $(
                    $param::collect_ctypes(__types);
                )*
            }

            fn create(__store: &internal::ComponentStore) -> Self {
                (
                    $(
                        $param::create(__store),
                    )*
                )
            }
        }


        /*
        FIXME: Unsound as the parameters allow for static lifetimes allowing them
            to escape the closure
        impl <$($param,)* Func> IntoSyncSystem<($($param,)*)> for Func
            where Func: Fn(EntityManager, $($param,)*) + Sync + Send + 'static,
                  ($($param,)*): SyncComponentAccessor + AccessorSet + 'static,
        {
            #[allow(non_snake_case, unused_variables)]
            fn into_system(self, scheduler: &mut internal::Scheduler) {
                struct WrappedSystem<Func, Params> {
                    func: Func,
                    _p: PhantomData<Params>,
                }
                impl <'a, Func, Params> System<'a> for WrappedSystem<Func, Params>
                    where Func: Fn(EntityManager, Params) + Sync + Send,
                    Params: SyncComponentAccessor + AccessorSet,
                {
                    type Param = Params;
                    fn run(&self, em: EntityManager, params: Self::Param) {
                        (self.func)(em, params)
                    }
                }
                unsafe impl <F, P> Sync for WrappedSystem<F, P> {}
                unsafe impl <F, P> Send for WrappedSystem<F, P> {}

                let f = self;
                scheduler.add(WrappedSystem::<_, ($($param,)*)> {
                    func: move |em: EntityManager, ($($param,)*): ($($param,)*)| {
                        f(em, $($param,)*)
                    },
                    _p: PhantomData,
                });
            }
        }
        */

        impl <'a, $($param,)* Func, Ret> IntoWithSystem<'a, ($($param,)*)> for Func
            where Func: FnOnce(EntityManager, $($param,)*) -> Ret + 'a,
                  ($($param,)*): AccessorSet + 'a,
        {
            type Return = Ret;
            #[allow(non_snake_case, unused_variables)]
            fn run_system(self, sysparam: &internal::SystemParam) -> Self::Return {
                let entities = EntityManager {
                    kill_chan: &sysparam.kill_chan,
                    entities: &sysparam.entities,
                };
                let ($($param,)*) = <($($param,)*) as internal::AccessorSet>::create(&sysparam.components);
                (self)(entities, $($param,)*)
            }
        }

        impl <'a, $($param,)* Func> IntoSystem<'a, ($($param,)*)> for Func
            where Func: Fn(EntityManager, $($param,)*) + 'a,
                  ($($param,)*): internal::AccessorSet + 'a,
        {
            #[allow(non_snake_case, unused_variables)]
            fn run_system(self, sysparam: &internal::SystemParam) {
                let entities = EntityManager {
                    kill_chan: &sysparam.kill_chan,
                    entities: &sysparam.entities,
                };
                let ($($param,)*) = <($($param,)*) as internal::AccessorSet>::create(&sysparam.components);
                (self)(entities, $($param,)*);
            }
        }

        unsafe impl <$($param,)*> SyncComponentAccessor for ($($param,)*)
            where $($param: SyncComponentAccessor,)* {}
    );
}

macro_rules! impl_system_functions {
    ($($wrapname:ident => $param:ident),*) => (
        impl_system_functions!(internal(closure0) () post ( $($wrapname => $param),* ));
    );
    (internal($name:ident) ($($preparam:ident),*) post ($nextname:ident => $next:ident, $($wrapname:ident => $param:ident),*) ) => (
        impl_system_function!($name => $($preparam),*);
        impl_system_functions!(internal($nextname) ($($preparam,)* $next) post ($($wrapname => $param),*));
    );
    (internal($name:ident) ($($preparam:ident),*) post ($nextname:ident => $next:ident)) => (
        impl_system_function!($name => $($preparam),*);
        impl_system_functions!(internal($nextname) ($($preparam,)* $next) post ());
    );
    (internal($name:ident) ($($preparam:ident),*) post ()) => (
        impl_system_function!($name => $($preparam),*);
    );
}

impl_system_functions!(
    closure1 => A,
    closure2 => B,
    closure3 => C,
    closure4 => D,
    closure5 => E,
    closure6 => F,
    closure7 => G,
    closure8 => H,
    closure9 => I,
    closure10 => J,
    closure11 => K,
    closure12 => L,
    closure13 => M,
    closure14 => N,
    closure15 => O,
    closure16 => P,
    closure17 => Q,
    closure18 => R,
    closure19 => S,
    closure20 => T,
    closure21 => U,
    closure22 => V,
    closure23 => W,
    closure24 => X,
    closure25 => Y,
    closure26 => Z
);

/// A component that can be accessed genericly
pub unsafe trait FetchableComponent<'a> {
    /// The component (or reference) that'll be returned
    type Component;

    /// Returns the component for the entity with the given
    /// id.
    ///
    /// The entity is assumed to be valid and checked before
    unsafe fn fetch_component(&self, id: u32) -> Self::Component;

    /// Returns a mask of entities with the component(s)
    fn mask(&self) -> EntityMask;
}

unsafe impl<'a, 'b, T> FetchableComponent<'a> for &'a mut Write<'b, T>
where
    T: Component,
{
    type Component = &'a mut T;

    #[inline]
    unsafe fn fetch_component(&self, id: u32) -> Self::Component {
        let storage = &mut *self.storage;
        storage.get_unchecked_component_mut(id)
    }

    #[inline]
    fn mask(&self) -> EntityMask {
        Write::mask(self)
    }
}

unsafe impl<'a, 'b, T> FetchableComponent<'a> for &'a Write<'b, T>
where
    T: Component,
{
    type Component = &'a T;

    #[inline]
    unsafe fn fetch_component(&self, id: u32) -> Self::Component {
        let storage = &mut *self.storage;
        storage.get_unchecked_component(id)
    }

    #[inline]
    fn mask(&self) -> EntityMask {
        Write::mask(self)
    }
}

unsafe impl<'a, 'b, T> FetchableComponent<'a> for &'a Read<'b, T>
where
    T: Component,
{
    type Component = &'a T;

    #[inline]
    unsafe fn fetch_component(&self, id: u32) -> Self::Component {
        let storage = &*self.storage;
        storage.get_unchecked_component(id)
    }

    #[inline]
    fn mask(&self) -> EntityMask {
        Read::mask(self)
    }
}

macro_rules! impl_fetch_tuple {
    (@current($($current:ident:$cidx:tt),*) @next()) => (
        impl_fetch_tuple!(@for $($current:$cidx),*);
    );
    (@current($($current:ident:$cidx:tt),*) @next($next:ident:$nidx:tt $(, $ty:ident:$tidx:tt)*)) => (
        impl_fetch_tuple!(@for $($current:$cidx),*);
        impl_fetch_tuple!(@current($($current:$cidx,)* $next:$nidx) @next($($ty:$tidx),*));
    );
    ($first:ident:$fidx:tt $(, $ty:ident:$tidx:tt)*) => (
        impl_fetch_tuple!(@current($first:$fidx) @next($($ty:$tidx),*));
    );
    (
        @for $first:ident:$fidx:tt $(, $ty:ident:$tidx:tt)*
    ) => (
unsafe impl <'a, $first $(,$ty)*> FetchableComponent<'a> for ($first, $($ty),*)
    where
        $first: FetchableComponent<'a>
        $(,
            $ty: FetchableComponent<'a>
        )*
{
    type Component = (<($first) as FetchableComponent<'a>>::Component, $(<($ty) as FetchableComponent<'a>>::Component),*);

    #[inline]
    unsafe fn fetch_component(&self, id: u32) -> Self::Component {
        (
            self.$fidx.fetch_component(id),
            $(
                self.$tidx.fetch_component(id),
            )*
        )
    }

    #[inline]
    fn mask(&self) -> EntityMask {
        self.$fidx.mask()
        $(
            .and_mask(self.$tidx.mask())
        )*
    }
}
    )
}

impl_fetch_tuple!(
    A:0,
    B:1,
    C:2,
    D:3,
    E:4,
    F:5,
    G:6,
    H:7,
    I:8,
    J:9,
    K:10,
    L:11,
    M:12,
    N:13,
    O:14,
    P:15,
    Q:16,
    R:17,
    S:18,
    T:19,
    U:20,
    V:21,
    W:22,
    X:23,
    Y:24,
    Z:25
);
