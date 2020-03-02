//! State management system

use crate::instance::GameInstance;
use crate::keybinds;
use crate::server::event;
use crate::util::FNVMap;
use std::any;
use std::mem;

/// Manages a list of states the game is currently in
/// or able to go back to.
pub struct StateManager {
    // (init, active, state)
    states: Vec<(bool, bool, Box<dyn State>)>,
    // (was_init, was_active, state)
    removed_states: Vec<(bool, bool, Box<dyn State>)>,

    next_cap_id: u32,
    captures: FNVMap<u32, CapturedState>,
}

/// Stores a snapshot of the current state
/// to be restored later.
pub struct CapturedState {
    states: Vec<(bool, bool, Box<dyn State>)>,
}

/// Allows for requesting a capture to be made
pub struct CaptureRequester {
    next_cap_id: u32,
}

impl Capturable for CaptureRequester {
    fn request_capture(&mut self) -> PossibleCapture {
        let id = self.next_cap_id;
        self.next_cap_id = self.next_cap_id.wrapping_add(1);
        PossibleCapture::Uncollected(id)
    }
}

/// Either contains a capture or an id
/// of a capture to be collected
pub enum PossibleCapture {
    /// A collected capture state
    Captured(CapturedState),
    /// A uncollected capture state
    Uncollected(u32),
}

/// Requests a capture of the current state of the system
pub trait Capturable {
    /// Requests a capture of the current state of the system
    fn request_capture(&mut self) -> PossibleCapture;
}

macro_rules! handle_state_func {
    ($me:ident, $func:expr) => {
        match { $func } {
            Some((idx, Action::Switch(new))) => {
                let cur_ty = $me.states[idx].2.state_type_id();
                if cur_ty != new.state_type_id()
                    && StateManager::is_state_active_of(&$me.states, &*new)
                {
                    // Can't have two of the same state active at once (currently)
                    $me.removed_states.push($me.states.remove(idx));
                } else {
                    let old = mem::replace(&mut $me.states[idx], (false, false, new));
                    $me.removed_states.push(old);
                }
            }
            Some((idx, Action::Push(new))) => {
                if new.can_have_duplicates()
                    || !StateManager::is_state_active_of(&$me.states, &*new)
                {
                    $me.states.insert(idx + 1, (false, false, new));
                }
            }
            Some((idx, Action::Toggle(new))) => {
                if let Some(pos) = StateManager::is_state_active_of_at(&$me.states, &*new) {
                    $me.removed_states.push($me.states.remove(pos));
                } else {
                    $me.states.insert(idx + 1, (false, false, new));
                }
            }
            Some((idx, Action::Pop)) => {
                $me.removed_states.push($me.states.remove(idx));
            }
            _ => {}
        }
    };
}

impl Capturable for StateManager {
    fn request_capture(&mut self) -> PossibleCapture {
        PossibleCapture::Captured(self.capture())
    }
}

impl StateManager {
    /// Creates a state manager which has the initial
    /// passed state
    pub fn new<S: State + 'static>(state: S) -> StateManager {
        StateManager {
            states: vec![(false, false, Box::new(state))],
            removed_states: vec![],
            next_cap_id: 0,
            captures: FNVMap::default(),
        }
    }

    fn update_states(
        &mut self,
        instance: &mut Option<GameInstance>,
        game_state: &mut crate::GameState,
    ) {
        let mut changed = true;
        while changed {
            let mut req = CaptureRequester {
                next_cap_id: self.next_cap_id,
            };
            for mut state in self.removed_states.drain(..) {
                if state.1 {
                    state.2.inactive_req(&mut req, instance, game_state);
                }
                if state.0 {
                    state.2.removed_req(&mut req, instance, game_state);
                }
            }
            // Mark previous states as inactive
            for state in self
                .states
                .iter_mut()
                .rev()
                .skip_while(|v| !v.2.takes_focus())
                .skip(1)
            {
                if state.1 {
                    state.2.inactive(instance, game_state);
                    state.1 = false;
                }
            }
            // Mark the current states as active
            let mut action: Option<(usize, Action)> = None;
            let first_active = self
                .states
                .iter()
                .enumerate()
                .rev()
                .find(|v| (v.1).2.takes_focus())
                .map_or(0, |v| v.0);
            for (idx, state) in self.states.iter_mut().enumerate().skip(first_active) {
                if !state.0 {
                    state.0 = true;
                    let act = state.2.added_req(&mut req, instance, game_state);
                    match act {
                        Action::Nothing => {}
                        act => action = Some((idx, act)),
                    }
                }
                if !state.1 && action.is_none() {
                    state.1 = true;
                    let act = state.2.active_req(&mut req, instance, game_state);
                    match act {
                        Action::Nothing => {}
                        act => action = Some((idx, act)),
                    }
                }
            }
            while self.next_cap_id != req.next_cap_id {
                let cap = self.capture();
                self.captures.insert(self.next_cap_id, cap);
                self.next_cap_id = self.next_cap_id.wrapping_add(1);
            }
            changed = action.is_some();
            handle_state_func!(self, action);
        }
    }

    /// If the capture is uncollected this collects the capture otherwise it returns itself
    pub fn collect_capture(&mut self, possible: &mut PossibleCapture) {
        match *possible {
            PossibleCapture::Uncollected(id) => {
                *possible = PossibleCapture::Captured(
                    self.captures
                        .remove(&id)
                        .expect("Repeated capture collection"),
                )
            }
            PossibleCapture::Captured(_) => {}
        }
    }

    /// Drops a capture if it hasn't been colllected
    pub fn drop_capture(&mut self, possible: PossibleCapture) {
        match possible {
            PossibleCapture::Uncollected(id) => {
                self.captures.remove(&id);
            }
            PossibleCapture::Captured(_) => {}
        }
    }

    /// Returns a snapshot of the current state
    pub fn capture(&self) -> CapturedState {
        CapturedState {
            states: self
                .states
                .iter()
                .skip(1)
                .map(|&(add, _, ref state)| (add, false, state.copy()))
                .collect(),
        }
    }

    /// Restores the state from a captured snapshot
    pub fn restore(&mut self, snap: CapturedState) {
        for state in self.states.drain(1..) {
            self.removed_states.push(state);
        }
        for mut state in snap.states {
            state.0 = false;
            state.1 = false;
            self.states.push(state);
        }
    }

    /// Ticks the currently active state
    pub fn tick(&mut self, instance: &mut Option<GameInstance>, game_state: &mut crate::GameState) {
        self.update_states(instance, game_state);

        let mut req = CaptureRequester {
            next_cap_id: self.next_cap_id,
        };

        let mut action: Option<(usize, Action)> = None;
        let first_active = self
            .states
            .iter()
            .enumerate()
            .rev()
            .find(|v| (v.1).2.takes_focus())
            .map_or(0, |v| v.0);
        for (idx, state) in self.states.iter_mut().enumerate().skip(first_active).rev() {
            match state.2.tick_req(&mut req, instance, game_state) {
                Action::Nothing => continue,
                act => {
                    action = Some((idx, act));
                    break;
                }
            }
        }

        while self.next_cap_id != req.next_cap_id {
            let cap = self.capture();
            self.captures.insert(self.next_cap_id, cap);
            self.next_cap_id = self.next_cap_id.wrapping_add(1);
        }
        handle_state_func!(self, action);
    }

    /// Calls the mouse move event on the active state
    ///
    /// Special due to the event spam it would cause
    pub fn mouse_move(
        &mut self,
        instance: &mut Option<GameInstance>,
        game_state: &mut crate::GameState,
        mouse_pos: (i32, i32),
    ) {
        self.update_states(instance, game_state);

        let mut req = CaptureRequester {
            next_cap_id: self.next_cap_id,
        };

        let mut action: Option<(usize, Action)> = None;
        let first_active = self
            .states
            .iter()
            .enumerate()
            .rev()
            .find(|v| (v.1).2.takes_focus())
            .map_or(0, |v| v.0);
        for (idx, state) in self.states.iter_mut().enumerate().skip(first_active).rev() {
            match state
                .2
                .mouse_move_req(&mut req, instance, game_state, mouse_pos)
            {
                Action::Nothing => continue,
                act => {
                    action = Some((idx, act));
                    break;
                }
            }
        }

        while self.next_cap_id != req.next_cap_id {
            let cap = self.capture();
            self.captures.insert(self.next_cap_id, cap);
            self.next_cap_id = self.next_cap_id.wrapping_add(1);
        }
        handle_state_func!(self, action);
    }

    /// Calls the mouse move ui event on the active state.
    ///
    /// This is called when the UI takes control of the mouse to
    /// allow states to release control where possible
    ///
    /// Special due to the event spam it would cause
    pub fn mouse_move_ui(
        &mut self,
        instance: &mut Option<GameInstance>,
        game_state: &mut crate::GameState,
        mouse_pos: (i32, i32),
    ) {
        self.update_states(instance, game_state);

        let mut req = CaptureRequester {
            next_cap_id: self.next_cap_id,
        };

        let mut action: Option<(usize, Action)> = None;
        let first_active = self
            .states
            .iter()
            .enumerate()
            .rev()
            .find(|v| (v.1).2.takes_focus())
            .map_or(0, |v| v.0);
        for (idx, state) in self.states.iter_mut().enumerate().skip(first_active).rev() {
            match state
                .2
                .mouse_move_ui_req(&mut req, instance, game_state, mouse_pos)
            {
                Action::Nothing => continue,
                act => {
                    action = Some((idx, act));
                    break;
                }
            }
        }

        while self.next_cap_id != req.next_cap_id {
            let cap = self.capture();
            self.captures.insert(self.next_cap_id, cap);
            self.next_cap_id = self.next_cap_id.wrapping_add(1);
        }
        handle_state_func!(self, action);
    }

    /// Calls the key action event on the active state
    pub fn key_action(
        &mut self,
        instance: &mut Option<GameInstance>,
        game_state: &mut crate::GameState,
        key_action: keybinds::KeyAction,
        mouse_pos: (i32, i32),
    ) {
        self.update_states(instance, game_state);

        let mut req = CaptureRequester {
            next_cap_id: self.next_cap_id,
        };

        let mut action: Option<(usize, Action)> = None;
        let first_active = self
            .states
            .iter()
            .enumerate()
            .rev()
            .find(|v| (v.1).2.takes_focus())
            .map_or(0, |v| v.0);
        for (idx, state) in self.states.iter_mut().enumerate().skip(first_active).rev() {
            match state
                .2
                .key_action_req(&mut req, instance, game_state, key_action, mouse_pos)
            {
                Action::Nothing => continue,
                act => {
                    action = Some((idx, act));
                    break;
                }
            }
        }

        while self.next_cap_id != req.next_cap_id {
            let cap = self.capture();
            self.captures.insert(self.next_cap_id, cap);
            self.next_cap_id = self.next_cap_id.wrapping_add(1);
        }
        handle_state_func!(self, action);
    }

    /// Calls the ui event on the active state
    pub fn ui_event(
        &mut self,
        instance: &mut Option<GameInstance>,
        game_state: &mut crate::GameState,
        evt: &mut event::EventHandler,
    ) {
        self.update_states(instance, game_state);

        let mut req = CaptureRequester {
            next_cap_id: self.next_cap_id,
        };

        let mut action: Option<(usize, Action)> = None;
        let first_active = self
            .states
            .iter()
            .enumerate()
            .rev()
            .find(|v| (v.1).2.takes_focus())
            .map_or(0, |v| v.0);
        for (idx, state) in self.states.iter_mut().enumerate().skip(first_active).rev() {
            match state.2.ui_event_req(&mut req, instance, game_state, evt) {
                Action::Nothing => continue,
                act => {
                    action = Some((idx, act));
                    break;
                }
            }
        }
        while self.next_cap_id != req.next_cap_id {
            let cap = self.capture();
            self.captures.insert(self.next_cap_id, cap);
            self.next_cap_id = self.next_cap_id.wrapping_add(1);
        }
        handle_state_func!(self, action);
    }

    /// Adds the state to the front of the state list
    pub fn add_state<S: State + 'static>(&mut self, state: S) {
        if state.can_have_duplicates() || !self.is_state_active::<S>() {
            self.states.push((false, false, Box::new(state)));
        }
    }

    /// Removes the state at the top of the state list
    pub fn pop_state(&mut self) {
        if let Some(state) = self.states.pop() {
            self.removed_states.push(state);
        }
    }

    /// Removes all states from the state list
    pub fn pop_all(&mut self) {
        while let Some(state) = self.states.pop() {
            self.removed_states.push(state);
        }
    }

    /// Returns whether a state of the given type is currently
    /// active
    pub fn is_state_active<S: State + 'static>(&self) -> bool {
        let ty = any::TypeId::of::<S>();
        self.states
            .iter()
            .filter(|v| v.1)
            .any(|v| v.2.state_type_id() == ty)
    }

    fn is_state_active_of(states: &[(bool, bool, Box<dyn State>)], s: &dyn State) -> bool {
        let ty = s.state_type_id();
        states
            .iter()
            .filter(|v| v.1)
            .any(|v| v.2.state_type_id() == ty)
    }

    fn is_state_active_of_at(
        states: &[(bool, bool, Box<dyn State>)],
        s: &dyn State,
    ) -> Option<usize> {
        let ty = s.state_type_id();
        states
            .iter()
            .filter(|v| v.1)
            .position(|v| v.2.state_type_id() == ty)
    }

    /// Returns whether the state list is empty or not
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }
}

/// Used to allow states to change between states
/// or add new ones.
pub enum Action {
    /// Do nothing, no state change
    Nothing,
    /// Replace the current state and
    /// add the contained state to
    /// the list
    Switch(Box<dyn State>),
    /// Adds the contained state to the
    /// state list without removing
    /// the current one
    Push(Box<dyn State>),
    /// Adds the contained state to the
    /// state list without removing
    /// the current one if a state of
    /// this type doesn't already exist.
    /// If it does it is removed instead.
    Toggle(Box<dyn State>),
    /// Removes the current state without
    /// adding another in its place.
    Pop,
}

/// A game state handler which will either
/// handle events or transition into another
/// state.
#[allow(missing_docs)]
pub trait State: any::Any {
    /// Crates a copy of the state in its current form
    fn copy(&self) -> Box<dyn State>;
    /// Returns the state as an `Any` type
    fn state_type_id(&self) -> any::TypeId {
        any::TypeId::of::<Self>()
    }
    /// Returns whether this state takes full focus from
    /// states below it or not.
    fn takes_focus(&self) -> bool {
        false
    }
    /// Whether the state can handle be open multiple times
    fn can_have_duplicates(&self) -> bool {
        false
    }

    /// Called once when initially added.
    ///
    /// Called at most once.
    fn added(
        &mut self,
        _instance: &mut Option<GameInstance>,
        _state: &mut crate::GameState,
    ) -> Action {
        Action::Nothing
    }
    fn added_req(
        &mut self,
        _req: &mut CaptureRequester,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
    ) -> Action {
        self.added(instance, state)
    }

    /// Called once when removed and about to be
    /// dropped.
    ///
    /// Called at most once after `added` is called.
    fn removed(&mut self, _instance: &mut Option<GameInstance>, _state: &mut crate::GameState) {}
    fn removed_req(
        &mut self,
        _req: &mut CaptureRequester,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
    ) {
        self.removed(instance, state);
    }
    /// Called when the state becomes active.
    ///
    /// May be called multiple times but will always have
    /// `inactive` called once between calls (excluding the first
    /// time)
    fn active(
        &mut self,
        _instance: &mut Option<GameInstance>,
        _state: &mut crate::GameState,
    ) -> Action {
        Action::Nothing
    }
    fn active_req(
        &mut self,
        _req: &mut CaptureRequester,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
    ) -> Action {
        self.active(instance, state)
    }
    /// Called when the state stops being active.
    ///
    /// May be called multiple times but will always have
    /// `active` before once between calls.
    ///
    /// Will be called before remove if the state was active
    /// upon removal.
    fn inactive(&mut self, _instance: &mut Option<GameInstance>, _state: &mut crate::GameState) {}
    fn inactive_req(
        &mut self,
        _req: &mut CaptureRequester,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
    ) {
        self.inactive(instance, state);
    }

    /// Called once a frame whilst the state is active
    fn tick(
        &mut self,
        _instance: &mut Option<GameInstance>,
        _state: &mut crate::GameState,
    ) -> Action {
        Action::Nothing
    }
    fn tick_req(
        &mut self,
        _req: &mut CaptureRequester,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
    ) -> Action {
        self.tick(instance, state)
    }

    // Event things

    /// Called when the mouse moves whilst the state is active
    ///
    /// Special due to the event spam it would cause
    fn mouse_move(
        &mut self,
        _instance: &mut Option<GameInstance>,
        _state: &mut crate::GameState,
        _mouse_pos: (i32, i32),
    ) -> Action {
        Action::Nothing
    }
    fn mouse_move_req(
        &mut self,
        _req: &mut CaptureRequester,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
        mouse_pos: (i32, i32),
    ) -> Action {
        self.mouse_move(instance, state, mouse_pos)
    }

    /// Called when the mouse moves whilst the state is active
    ///
    /// This is called when the UI takes control of the mouse to
    /// allow states to release control where possible
    ///
    /// Special due to the event spam it would cause
    fn mouse_move_ui(
        &mut self,
        _instance: &mut Option<GameInstance>,
        _state: &mut crate::GameState,
        _mouse_pos: (i32, i32),
    ) -> Action {
        Action::Nothing
    }
    fn mouse_move_ui_req(
        &mut self,
        _req: &mut CaptureRequester,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
        mouse_pos: (i32, i32),
    ) -> Action {
        self.mouse_move_ui(instance, state, mouse_pos)
    }

    /// Called whenever a key action is fired whilst the state is active
    fn key_action(
        &mut self,
        _instance: &mut Option<GameInstance>,
        _state: &mut crate::GameState,
        _action: keybinds::KeyAction,
        _mouse_pos: (i32, i32),
    ) -> Action {
        Action::Nothing
    }
    fn key_action_req(
        &mut self,
        _req: &mut CaptureRequester,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
        action: keybinds::KeyAction,
        mouse_pos: (i32, i32),
    ) -> Action {
        self.key_action(instance, state, action, mouse_pos)
    }

    /// Called whenever a ui event is fired whilst the state is active
    fn ui_event(
        &mut self,
        _instance: &mut Option<GameInstance>,
        _state: &mut crate::GameState,
        _evt: &mut event::EventHandler,
    ) -> Action {
        Action::Nothing
    }
    fn ui_event_req(
        &mut self,
        _req: &mut CaptureRequester,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
        evt: &mut event::EventHandler,
    ) -> Action {
        self.ui_event(instance, state, evt)
    }
}
