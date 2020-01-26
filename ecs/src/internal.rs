use std::any::{Any, TypeId};
use std::sync::mpsc;
use std::sync::{Mutex, RwLock};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::cell::UnsafeCell;
use crate::util;

use rayon::{ThreadPool, ThreadPoolBuilder};
use super::{Entity, EntityManager, Component, ComponentStorage, SyncComponentSystem};

pub struct SystemParam<'a> {
    pub entities: &'a RwLock<EntityAllocator>,
    pub components: &'a mut ComponentStore,
    pub kill_chan: Mutex<mpsc::Sender<Entity>>,
}
unsafe impl<'a> Send for SystemParam<'a> {}
unsafe impl<'a> Sync for SystemParam<'a> {}

#[allow(clippy::type_complexity)]
pub struct Scheduler {
    funcs: Vec<(AtomicUsize, Vec<CType>, Box<dyn Fn(&SystemParam) + Sync + Send>)>,
    locked: fnv::FnvHashMap<TypeId, ScheduleLock>,
    pool: ThreadPool,
    cycle: usize,
    num_threads: usize,
}

impl Scheduler {
    pub fn new(num_threads: usize) -> Scheduler {
        Scheduler {
            funcs: Vec::new(),
            locked: fnv::FnvHashMap::default(),
            pool: ThreadPoolBuilder::new()
                .thread_name(|id| format!("ECS Scheduler Task Thread: {}", id))
                .num_threads(num_threads)
                .build()
                .unwrap(),
            cycle: 0,
            num_threads,
        }
    }

    #[inline]
    pub fn add<S>(&mut self, system: S)
        where S: for<'a> SyncComponentSystem<'a> + Sync + Send + 'static,
    {
        let mut types = Vec::new();
        S::Param::collect_ctypes(&mut types);
        self.funcs.push((AtomicUsize::new(self.cycle), types, Box::new(move |sysparam| {
            let entities = EntityManager {
                kill_chan: &sysparam.kill_chan,
                entities: &sysparam.entities,
            };
            let param: S::Param = S::Param::create(&sysparam.components);
            system.run(entities, param);
        })));
    }

    pub fn run(&mut self, param: &SystemParam) {
        use std::sync::mpsc::RecvTimeoutError;
        use std::panic::{catch_unwind, AssertUnwindSafe, resume_unwind};
        self.cycle = self.cycle.wrapping_add(1);
        let cur_cycle = self.cycle;
        let max_tasks = self.num_threads;
        let mut free_tasks = max_tasks;
        let mut to_process = self.funcs.len();

        let funcs = &self.funcs;
        let locked = &mut self.locked;
        self.pool.scope(|scope| {
            let (done_send, done_recv) = mpsc::channel();
            let (panic_send, panic_recv) = mpsc::channel();
            while to_process > 0 || free_tasks != max_tasks {
                if let Ok(val) = panic_recv.try_recv() {
                    resume_unwind(val);
                }
                // If we have inactive thread and something left to process
                // attempt to process it.
                'consume_func:
                while free_tasks > 0 && to_process > 0 {
                    // Search for a function which can be executed in the current state
                    'funcs:
                    for (id, &(ref cycle, ref types, ref f)) in funcs.iter()
                            .enumerate()
                            .filter(|&(_, ref v)| v.0.load(Ordering::Relaxed) != cur_cycle) {
                        for ty in types {
                            match *ty {
                                CType::Read(id) => {
                                    // If there isn't an entry or if something is already reading it
                                    // its fine to read again
                                    if !locked.get(&id).map_or(true, ScheduleLock::is_read) {
                                        continue 'funcs;
                                    }
                                },
                                CType::Write(id) => {
                                    // Only a single writer is a allowed so if something is reading
                                    // or writing this function cannot be executed.
                                    if locked.contains_key(&id) {
                                        continue 'funcs;
                                    }
                                },
                            }
                        }
                        // If we make it here then the function is safe to execute. Update the locked
                        // map to take control of the data it requested.
                        for ty in types {
                            match *ty {
                                CType::Read(id) => {
                                    // Add another reader to the entry
                                    if let ScheduleLock::Read(ref mut count) = *locked.entry(id).or_insert(ScheduleLock::Read(0)) {
                                        *count += 1;
                                    } else {
                                        // At this point the lock should never be write
                                        unreachable!();
                                    }
                                },
                                CType::Write(id) => {
                                    // Mark the entry as being write locked
                                    locked.insert(id, ScheduleLock::Write);
                                },
                            }
                        }
                        // Mark the function as executed this cycle so that its not checked again
                        cycle.store(cur_cycle, Ordering::Relaxed);
                        free_tasks -= 1; // Take a thread
                        to_process -= 1;
                        {
                            let done_send = done_send.clone();
                            let panic_send = panic_send.clone();
                            scope.spawn(move |_| {
                                if let Err(err) = catch_unwind(AssertUnwindSafe(|| {
                                    (f)(param);
                                    done_send.send(id).unwrap();
                                })) {
                                    panic_send.send(err).unwrap();
                                }
                            });
                        }
                        continue 'consume_func;
                    }
                    break;
                }
                {
                    // Timeout to catch panicing threads.
                    // No system should take this long anyway so hitting it wont be common
                    let id = done_recv.recv_timeout(::std::time::Duration::new(1, 0));
                    let id = match id {
                        Ok(id) => id,
                        Err(RecvTimeoutError::Timeout) => continue,
                        Err(RecvTimeoutError::Disconnected) => break,
                    };
                    let &(_, ref types, _) = funcs.get(id).unwrap();
                    for ty in types {
                        match *ty {
                            CType::Read(id) => {
                                // Release the function's read lock on the type
                                if let ScheduleLock::Read(mut count) = *locked.get(&id).unwrap() {
                                    count -= 1;
                                    if count == 0 {
                                        // Everyone has finished with it, allow writing
                                        locked.remove(&id);
                                    } else {
                                        locked.insert(id, ScheduleLock::Read(count));
                                    }
                                } else {
                                    unreachable!();
                                }
                            },
                            CType::Write(id) => {
                                // Release the function's write lock on the type
                                locked.remove(&id);
                            },
                        }
                    }
                    // Allow the thread to be used again
                    free_tasks += 1;
                }
            }
            if let Ok(val) = panic_recv.try_recv() {
                resume_unwind(val);
            }
        });
    }

}

#[derive(Clone, Copy, Debug)]
pub enum ScheduleLock {
    Read(usize),
    Write,
}

impl ScheduleLock {
    #[inline]
    fn is_read(&self) -> bool {
        match *self {
            ScheduleLock::Read(..) => true,
            ScheduleLock::Write => false,
        }
    }
}

#[derive(Clone, Copy)]
pub enum CType {
    Read(TypeId),
    Write(TypeId),
}

pub trait AccessorSet: Sized {
    fn collect_ctypes(types: &mut Vec<CType>);

    fn create(store: &ComponentStore) -> Self;
}

impl <T> AccessorSet for T
    where T: Accessor
{
    fn collect_ctypes(types: &mut Vec<CType>) {
        types.push(Self::ctype());
    }
    fn create(store: &ComponentStore) -> Self {
        Self::new(store)
    }
}

pub trait InternalFunction {
    fn into_scheduler(self, sch: &mut Scheduler);
}

pub trait Accessor {
    type Component: super::Component;
    fn ctype() -> CType;
    fn new(store: &ComponentStore) -> Self;
    fn and_mask(&self, mask: super::EntityMask) -> super::EntityMask;
    fn and_not_mask(&self, mask: super::EntityMask) -> super::EntityMask;
}

pub struct EntityAllocator {
    pub max_entities: u32,
    pub entities: util::BitSet,
    pub generations: Vec<u32>,
    next_id: u32,
}

impl EntityAllocator {
    pub fn new() -> EntityAllocator {
        let mut entities = util::BitSet::new(512);
        entities.set(0, true); // Reserve the world entity
        EntityAllocator {
            max_entities: 512,
            entities,
            generations: vec![0; 512],
            next_id: 0,
        }
    }

    #[inline]
    pub fn is_valid(&self, e: Entity) -> bool {
        if !self.entities.get(e.id as usize) {
            return false;
        }
        self.generations.get(e.id as usize)
            .map_or(false, |v| *v == e.generation)
    }

    pub fn alloc(&mut self) -> Entity {
        while self.next_id < self.max_entities && self.entities.get(self.next_id as usize) {
            self.next_id = self.next_id.wrapping_add(1);
        }
        if self.next_id >= self.max_entities {
            self.max_entities <<= 2;
            self.entities.resize(self.max_entities as usize);
            self.generations.resize(self.max_entities as usize, 0);
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.entities.set(id as usize, true);
        let gen = self.generations.get_mut(id as usize).unwrap();
        *gen = (*gen).wrapping_add(1);
        Entity {
            id,
            generation: *gen,
        }
    }

    pub fn free(&mut self, e: Entity) -> bool {
        if self.generations.get(e.id as usize).map_or(true, |v| *v != e.generation) {
            return false;
        }
        self.entities.set(e.id as usize, false);
        if self.next_id > e.id {
            self.next_id = e.id;
        }
        true
    }
}

#[doc(hidden)]
pub trait BoxedStorage: Any {
    fn as_any(&self) -> &dyn Any;
    fn as_mut_any(&mut self) -> &mut dyn Any;

    // Frees the component for the given id if one exists
    fn free_id(&mut self, id: u32);
}

pub struct ComponentStore {
    pub components: fnv::FnvHashMap<TypeId, UnsafeCell<StoreWrap>>,
}

pub struct StoreWrap {
    pub mask: util::BitSet,
    pub max: usize,
    pub store: Box<dyn BoxedStorage>,
}

impl Drop for StoreWrap {
    fn drop(&mut self) {
        let mut current = 0;
        while current < self.max {
            let id = current;
            current += 1;
            if self.mask.get(id) {
                self.store.free_id(id as u32);
            }
        }
    }
}

impl ComponentStore {
    pub fn new() -> ComponentStore {
        ComponentStore {
            components: fnv::FnvHashMap::default(),
        }
    }

    #[inline]
    pub fn register_component<T: Component>(&mut self) {
        let store = T::Storage::new();
        self.register_component_self::<T>(store)
    }

    pub fn register_component_self<T: Component>(&mut self, store: T::Storage) {
        let tid = TypeId::of::<T>();
        if self.components.contains_key(&tid) {
            return;
        }
        self.components.insert(tid, UnsafeCell::new(StoreWrap {
            mask: util::BitSet::new(256),
            max: 256,
            store: Box::new(store),
        }));
    }

    pub fn add_component<T: Component>(&mut self, id: u32, val: T) {
        use std::cmp;
        let back_store = unsafe { &mut *self.components.get_mut(&TypeId::of::<T>())
            .expect("Component type not registered")
            .get()};
        let store: &mut T::Storage = back_store.store.as_mut_any().downcast_mut().unwrap();
        if back_store.max <= id as usize {
            back_store.max = cmp::max(back_store.max * 2, id as usize + 1);
            back_store.mask.resize(back_store.max);
        }
        if !T::Storage::self_bookkeeps() && back_store.mask.get(id as usize) {
            store.remove_component(id);
        }
        store.add_component(id, val);
        back_store.mask.set(id as usize, true);
    }

    pub fn remove_component<T: Component>(&mut self, id: u32) -> Option<T> {
        let back_store = unsafe { &mut *self.components.get_mut(&TypeId::of::<T>())
            .expect("Component type not registered")
            .get() };
        if !T::Storage::self_bookkeeps() && !back_store.mask.get(id as usize) {
            return None;
        }
        let store: &mut T::Storage = back_store.store.as_mut_any().downcast_mut().unwrap();
        back_store.mask.set(id as usize, false);
        store.remove_component(id)
    }

    pub fn free_all_components(&mut self, id: u32) {
        for store in self.components.values_mut() {
            let store = unsafe { &mut *store.get() };
            if store.mask.get(id as usize) {
                store.store.free_id(id);
                store.mask.set(id as usize, false);
            }
        }
    }

    pub fn get_component<T: Component>(&self, id: u32) -> Option<&T> {
        let back_store = unsafe { &*self.components.get(&TypeId::of::<T>())
            .expect("Component type not registered")
            .get() };
        if !T::Storage::self_bookkeeps() {
            if back_store.mask.get(id as usize)  {
                let store: &T::Storage = back_store.store.as_any().downcast_ref().unwrap();
                Some(unsafe { store.get_unchecked_component(id) })
            } else {
                None
            }
        } else {
            let store: &T::Storage = back_store.store.as_any().downcast_ref().unwrap();
            store.get_component(id)
        }
    }

    pub fn get_component_mut<T: Component>(&mut self, id: u32) -> Option<&mut T> {
        let back_store = unsafe { &mut *self.components
            .get_mut(&TypeId::of::<T>())
            .expect("Component type not registered")
            .get() };
        if !T::Storage::self_bookkeeps() {
            if back_store.mask.get(id as usize)  {
                let store: &mut T::Storage = back_store.store.as_mut_any().downcast_mut().unwrap();
                Some(unsafe { store.get_unchecked_component_mut(id) })
            } else {
                None
            }
        } else {
            let store: &mut T::Storage = back_store.store.as_mut_any().downcast_mut().unwrap();
            store.get_component_mut(id)
        }
    }
}