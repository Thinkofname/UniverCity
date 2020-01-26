use super::*;

#[test]
fn test_scheduler() {
    use std::sync::{RwLock, Arc};
    let mut c = Container::new();
    component!(u8 => Vec);
    component!(u16 => Vec);
    component!(u32 => Vec);
    c.register_component::<u8>();
    c.register_component::<u16>();
    c.register_component::<u32>();
    let mut sys = Systems::new();

    let u8lock = Arc::new(RwLock::new(true));
    let u16lock = Arc::new(RwLock::new(true));
    let u32lock = Arc::new(RwLock::new(true));

    for _ in 0 .. 20 {
        let u8lock = u8lock.clone();
        let u16lock = u16lock.clone();
        let u32lock = u32lock.clone();

        struct Test1 {
            u8lock: Arc<RwLock<bool>>,
            u16lock: Arc<RwLock<bool>>,
            u32lock: Arc<RwLock<bool>>,
        }
        impl <'a> System<'a> for Test1 {
            type Param = (Read<'a, u8>, Write<'a, u16>, Read<'a, u32>);
            fn run(&self, _em: EntityManager, _: Self::Param) {
                assert!(self.u8lock.try_read().is_ok());
                assert!(self.u16lock.try_write().is_ok());
                assert!(self.u32lock.try_read().is_ok());
            }
        }
        sys.add(Test1 {
            u8lock,
            u16lock,
            u32lock,
        });
    }

    for _ in 0 .. 20 {
        let u8lock = u8lock.clone();
        let u16lock = u16lock.clone();
        let u32lock = u32lock.clone();
        struct Test2 {
            u8lock: Arc<RwLock<bool>>,
            u16lock: Arc<RwLock<bool>>,
            u32lock: Arc<RwLock<bool>>,
        }
        impl <'a> System<'a> for Test2 {
            type Param = (Write<'a, u8>, Write<'a, u16>, Read<'a, u32>);
            fn run(&self, _em: EntityManager, _: Self::Param) {
                assert!(self.u8lock.try_write().is_ok());
                assert!(self.u16lock.try_write().is_ok());
                assert!(self.u32lock.try_read().is_ok());
            }
        }
        sys.add(Test2 {
            u8lock,
            u16lock,
            u32lock,
        });
    }

    {
        let u8lock = u8lock.clone();
        let u16lock = u16lock.clone();
        let u32lock = u32lock.clone();

        struct Test3 {
            u8lock: Arc<RwLock<bool>>,
            u16lock: Arc<RwLock<bool>>,
            u32lock: Arc<RwLock<bool>>,
        }
        impl <'a> System<'a> for Test3 {
            type Param = (Read<'a, u8>, Read<'a, u16>, Read<'a, u32>);
            fn run(&self, _em: EntityManager, _: Self::Param) {
                assert!(self.u8lock.try_read().is_ok());
                assert!(self.u16lock.try_read().is_ok());
                assert!(self.u32lock.try_read().is_ok());
            }
        }
        sys.add(Test3 {
            u8lock,
            u16lock,
            u32lock,
        });
    }

    for _ in 0 .. 20 {
        let u8lock = u8lock.clone();
        let u32lock = u32lock.clone();

        struct Test4 {
            u8lock: Arc<RwLock<bool>>,
            u32lock: Arc<RwLock<bool>>,
        }
        impl <'a> System<'a> for Test4 {
            type Param = (Read<'a, u8>, Read<'a, u32>);
            fn run(&self, _em: EntityManager, _: Self::Param) {
                assert!(self.u8lock.try_read().is_ok());
                assert!(self.u32lock.try_read().is_ok());
            }
        }
        sys.add(Test4 {
            u8lock,
            u32lock,
        });
    }
    sys.run(&mut c);
}

#[derive(Debug, PartialEq, Eq)]
struct Position {
    x: i32,
    y: i32
}
component!(Position => Vec);

#[derive(Debug, PartialEq, Eq)]
struct Name {
    name: String,
}
component!(Name => Vec);

#[derive(Default)]
struct IsMagic;
component!(IsMagic => Marker);

#[test]
fn test() {
    let mut c = Container::new();
    c.register_component::<Position>();
    c.register_component::<IsMagic>();
    for _ in 0 .. 5000 {
        let e = c.new_entity();
        c.add_component(e, Position { x: 0, y: 0 });
    }

    let test_entity = c.new_entity();
    c.add_component(test_entity, Position { x: 55, y: 64 });
    c.add_component(test_entity, IsMagic);

    let mut sys = Systems::new();

    struct TestSystem {
        test_entity: Entity,
    }
    impl <'a> System<'a> for TestSystem {
        type Param = (Write<'a, Position>, Read<'a, IsMagic>);
        fn run(&self, em: EntityManager, (mut pos, magic): Self::Param) {
            let new = em.new_entity();
            pos.add_component(new, Position {x: 2, y: 3});
            let mask = pos.mask();
            for e in em.iter_mask(&mask) {
                if magic.get_component(e).is_some() {
                    assert_eq!(e, self.test_entity);
                }
                if e == new {
                    assert_eq!(pos.get_component(e), Some(&Position {x: 2, y: 3}));
                }
            }

        }
    }
    sys.add(TestSystem {
        test_entity,
    });

    sys.run(&mut c);
}

#[test]
fn test_borrow() {
    struct Counter(i32);
    component!(Counter => mut World);
    struct DoCounter(bool);
    component!(DoCounter => const World);

    let mut c = Container::new();
    c.register_component::<Position>();
    c.register_component::<Counter>();
    c.register_component::<DoCounter>();
    for _ in 0 .. 5000 {
        let e = c.new_entity();
        c.add_component(e, Position { x: 0, y: 0 });
    }
    let mut sys = Systems::new();

    closure_system!(fn test(em: EntityManager<'_>, mut counter: Write<Counter>, do_counter: Read<DoCounter>, pos: Write<Position>) {
        let world = Container::WORLD;
        let counter = counter.get_component_mut(world).unwrap();
        let do_counter = do_counter.get_component(world).unwrap().0;
        let mask = pos.mask();
        for _ in em.iter_mask(&mask) {
            if do_counter {
                counter.0 += 1;
            }
        }
    });
    sys.add(test);

    let mut counter = Counter(0);
    let mut do_count = DoCounter(true);

    for i in 0 .. 600 {
        do_count.0 = i % 2 == 0;
        sys.run_with_borrows(&mut c)
            .borrow_mut(&mut counter)
            .borrow(&do_count)
            .run();
    }
    assert_eq!(counter.0, 5000 * 300);
}

#[test]
#[should_panic(expected = "Test panic")]
fn test_panic() {
    let mut c = Container::new();
    c.register_component::<Position>();
    for _ in 0 .. 5000 {
        let e = c.new_entity();
        c.add_component(e, Position { x: 0, y: 0 });
    }

    closure_system!(fn test1(em: EntityManager<'_>, pos: Write<Position>) {
        let mask = pos.mask();
        for e in em.iter_mask(&mask) {
            let _ = e;
        }
    });
    closure_system!(fn test2(_em: EntityManager<'_>, _pos: Read<Position>) {
        panic!("Test panic")
    });
    closure_system!(fn test3(em: EntityManager<'_>, pos: Write<Position>) {
        let mask = pos.mask();
        for e in em.iter_mask(&mask) {
            let _ = e;
        }
    });

    let mut sys = Systems::new();
    sys.add(test1);
    sys.add(test2);
    sys.add(test3);
    sys.run(&mut c);
}

#[test]
fn test_remove() {
    let mut c = Container::new();
    c.register_component::<Position>();
    for _ in 0 .. 1000 {
        let e = c.new_entity();
        c.add_component(e, Position { x: 0, y: 0 });
    }
    let to_remove = c.new_entity();
    c.add_component(to_remove, Position { x: 0, y: 0 });

    for _ in 0 .. 1000 {
        let e = c.new_entity();
        c.add_component(e, Position { x: 0, y: 0 });
    }

    struct ToRemove(Entity);
    component!(ToRemove => Vec);
    c.register_component::<ToRemove>();
    c.add_component(Container::WORLD, ToRemove(to_remove));

    let mut sys = Systems::new();

    closure_system!(fn test_remove(em: EntityManager<'_>, to_remove: Read<ToRemove>) {
        let to_remove = to_remove.get_component(Container::WORLD).unwrap().0;
        em.remove_entity(to_remove);
    });
    closure_system!(fn test1(em: EntityManager<'_>, pos: Write<Position>) {
        let mask = pos.mask();
        assert_eq!(em.iter_mask(&mask).count(), 2000);
        assert_eq!(em.iter_all().count(), 2001);
    });
    closure_system!(fn test2(em: EntityManager<'_>, pos: Write<Position>, to_remove: Read<ToRemove>) {
        let to_remove = to_remove.get_component(Container::WORLD).unwrap().0;
        let mask = pos.mask();
        for e in em.iter_mask(&mask) {
            assert!(e != to_remove);
        }
    });

    sys.add(test_remove);
    sys.run(&mut c);
    sys.add(test1);
    sys.add(test2);
    sys.run(&mut c);
}

#[test]
fn test_rayon() {
    use rayon::prelude::*;
    let mut c = Container::new();
    c.register_component::<Position>();
    for _ in 0 .. 1000 {
        let e = c.new_entity();
        c.add_component(e, Position { x: 0, y: 0 });
    }

    c.with(|
        _em: EntityManager<'_>,
        mut pos: Write<Position>,
    | {
        let mask = pos.mask();
        let count = pos.par_iter(&mask)
            .count();
        assert_eq!(count, 1000);
    });
}

#[test]
fn test_rayon_group() {
    use rayon::prelude::*;
    let mut c = Container::new();
    c.register_component::<Position>();
    c.register_component::<Name>();
    for _ in 0 .. 1123 {
        let e = c.new_entity();
        c.add_component(e, Position { x: 0, y: 0 });
        c.add_component(e, Name {
            name: format!("{:?}", e),
        });
    }

    c.with(|
        em: EntityManager<'_>,
        mut pos: Write<'_, Position>,
        name: Read<'_, Name>,
    | {
        let count = em.par_group((&name, &mut pos))
            .par_iter()
            .count();
        assert_eq!(count, 1123);
    });
}