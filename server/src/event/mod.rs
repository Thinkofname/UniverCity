//! A generic event system that uses closures as callbacks

use std::any::Any;

/// Contains a list of a events that haven't been handled
pub struct Container {
    event_queue: Vec<Box<dyn Any>>,
}

impl Container {
    /// Creates a new container with no events
    pub fn new() -> Container {
        Container {
            event_queue: Vec::new(),
        }
    }

    /// Merges this container with another
    pub fn join(&mut self, mut other: Container) {
        self.event_queue.append(&mut other.event_queue);
    }

    /// Emits the passed event.
    ///
    /// The event will not be handled until `handle_events`
    /// is called.
    pub fn emit<T: Any>(&mut self, evt: T) {
        self.event_queue.push(Box::new(evt));
    }

    /// Creates a handler that can be used to register and execute
    /// event handlers.
    ///
    /// Events are not consumed until `exec` is called on the handler
    pub fn handle_events(&mut self) -> Vec<EventHandler> {
        self.event_queue
            .drain(..)
            .map(|v| EventHandler { event: Some(v) })
            .collect()
    }
}

/// Handles executing event handlers for the event within
#[must_use]
pub struct EventHandler {
    event: Option<Box<dyn Any>>,
}

impl EventHandler {
    /// Tries to the handle the event with the passed handler
    pub fn handle_event<'a, T: Any, F>(&'a mut self, efunc: F)
    where
        F: FnOnce(T) + 'a,
    {
        if let Some(evt) = self.event.take() {
            match evt.downcast::<T>() {
                Ok(val) => {
                    efunc(*val);
                }
                Err(val) => {
                    self.event = Some(val);
                }
            }
        }
    }

    /// Tries to the handle the event with the passed handler
    pub fn handle_event_if<'a, T: Any, IF, F>(&'a mut self, if_func: IF, efunc: F)
    where
        F: FnOnce(T) + 'a,
        IF: FnOnce(&T) -> bool + 'a,
    {
        if let Some(evt) = self.event.take() {
            match evt.downcast::<T>() {
                Ok(val) => {
                    if if_func(&*val) {
                        efunc(*val);
                    } else {
                        self.event = Some(val);
                    }
                }
                Err(val) => {
                    self.event = Some(val);
                }
            }
        }
    }
    /// Tries to the handle the event with the passed handler
    pub fn inspect_event<'a, T: Any, F>(&'a mut self, mut efunc: F)
    where
        F: FnMut(&T) + 'a,
    {
        if let Some(evt) = self.event.as_ref() {
            match evt.downcast_ref::<T>() {
                Some(val) => {
                    efunc(val);
                }
                None => {}
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    struct TestPrint;

    impl TestPrint {
        fn print(&mut self, val: &str) {
            println!("TestPrint: {:?}", val);
        }
    }

    struct TestEvent(i32);
    struct TestEvent2(String);

    #[test]
    fn test() {
        let mut con = Container::new();
        let mut test = TestPrint;
        test.print("Start");

        con.emit(TestEvent(1));
        con.emit(TestEvent2("testing".into()));
        con.emit(TestEvent(2));
        con.emit(TestEvent(3));
        con.emit(TestEvent2("a".into()));
        con.emit(TestEvent(6));
        con.emit(TestEvent2("b".into()));
        con.emit(TestEvent(10));

        let mut total = 0;
        let mut strings = vec!["testing", "a", "b"];

        for mut evt in con.handle_events() {
            evt.handle_event::<TestEvent, _>(|evt| {
                test.print(&format!("test event: {}", evt.0));
                total += evt.0;
            });
            evt.handle_event::<TestEvent2, _>(|evt| {
                let should = strings.remove(0);
                test.print(&format!("test 2: {:?}", evt.0));
                assert_eq!(should, evt.0);
            });
        }

        test.print("End");
        assert_eq!(total, 1 + 2 + 3 + 6 + 10);
        assert!(strings.is_empty());
    }
}
