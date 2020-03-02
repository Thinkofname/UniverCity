//! Handles transforming key/mouse events into
//! events.

use crate::prelude::*;
use crate::util::FNVMap;
use sdl2::event::Event as SDLEvent;
use sdl2::keyboard::Keycode;
use sdl2::mouse::MouseButton;
use serde_json;
use std::fs::File;

/// A collection of keybinds to be used for certain game states
#[derive(PartialEq, Eq, Hash, Debug, Clone, Copy)]
pub enum KeyCollection {
    /// The default collection, should always exist
    Root,
    /// The default collection to be used in-game
    Game,
    /// The collection used when placing a room
    RoomPlacement,
    /// The collection used when resizing a room
    RoomResize,
    /// The collection used when building a room
    BuildRoom,
    /// The collection used when placing an object
    PlaceObject,
    /// The collection used when placing an entity
    PlaceStaff,
    /// The collection used when selecting a room to
    /// edit
    EditRoom,
}

impl KeyCollection {
    pub(crate) fn as_str(self) -> &'static str {
        use self::KeyCollection::*;
        match self {
            Root => "Root",
            Game => "Game",
            RoomPlacement => "Room Placement",
            RoomResize => "Room Resize",
            BuildRoom => "Build Room",
            PlaceObject => "Place Object",
            PlaceStaff => "Place Staff",
            EditRoom => "Edit Room",
        }
    }

    #[allow(dead_code)]
    fn from_str(val: &str) -> Option<KeyCollection> {
        use self::KeyCollection::*;
        match val {
            "Root" => Some(Root),
            "Game" => Some(Game),
            "Room Placement" => Some(RoomPlacement),
            "Room Resize" => Some(RoomResize),
            "Build Room" => Some(BuildRoom),
            "Place Object" => Some(PlaceObject),
            "Place Staff" => Some(PlaceStaff),
            "Edit Room" => Some(EditRoom),
            _ => None,
        }
    }
}

/// An action triggered when pressing a key (either keyboard
/// or mouse)
#[derive(PartialEq, Eq, Hash, Debug, Clone, Copy, PartialOrd, Ord)]
pub enum KeyAction {
    /// Opens the pause menu
    SystemMenu,
    /// Begins a chat message
    BeginChat,

    // Render actions
    /// Requests that the renderer zooms in
    RenderZoomIn,
    /// Requests that the renderer zooms out
    RenderZoomOut,
    /// Requests that the renderer rotates left
    RenderRotateLeft,
    /// Requests that the renderer rotates right
    RenderRotateRight,

    /// Starts moving the camera left
    RenderCameraLeft,
    /// Stops moving the camera left
    RenderCameraLeftStop,
    /// Starts moving the camera right
    RenderCameraRight,
    /// Stops moving the camera right
    RenderCameraRightStop,
    /// Starts moving the camera up
    RenderCameraUp,
    /// Stops moving the camera up
    RenderCameraUpStop,
    /// Starts moving the camera down
    RenderCameraDown,
    /// Stops moving the camera up
    RenderCameraDownStop,

    // Room actions
    /// Starts selecting at the mouse's current position
    RoomStartAreaSelect,
    /// Stops moving the camera
    RoomStartAreaSelectStop,
    /// Stops selecting at the mouse's current position
    RoomFinishAreaSelect,
    /// Starts resizing the current room
    RoomStartRoomResize,
    /// Stops resizing the current room
    RoomFinishRoomResize,

    // Placement actions
    /// Places the active object/entity
    PlacementFinish,
    /// Starts placing a drag placable object
    PlacementDragStart,
    /// Tries to rotate the active object/entity
    PlacementRotate,
    /// Tries to move a placed object
    PlacementMove,
    /// Tries to remove a placed object
    PlacementRemove,

    /// Tries to edit the room at the mouse's
    /// location
    SelectEditRoom,

    /// Inspect's a student or staff member
    /// under the mouse
    InspectMember,
}

impl KeyAction {
    // Helpers for the keybinds UI

    /// The standard 'direction' for the action.
    ///
    /// The lack of a direction means that its considered
    /// normal to allow both `up`(true) and `down`(false)
    /// directions for this action.
    ///
    /// This doesn't prevent these rules from being broken
    /// by the config, only the UI.
    pub(crate) fn standard_direction(self) -> Option<bool> {
        use self::KeyAction::*;
        match self {
            RenderZoomIn | RenderZoomOut | RenderCameraLeft | RenderCameraRight
            | RenderCameraUp | RenderCameraDown | RoomStartAreaSelect | RoomStartRoomResize
            | PlacementDragStart => Some(false),
            SystemMenu | BeginChat | RenderRotateLeft | RenderRotateRight | SelectEditRoom
            | PlacementMove | PlacementRemove | PlacementFinish | PlacementRotate
            | InspectMember => Some(true),
            _ => None,
        }
    }

    /// Linked actions automatically set the other one when set
    /// via the ui but inversed.
    pub(crate) fn linked_action(self) -> Option<KeyAction> {
        use self::KeyAction::*;
        match self {
            RenderCameraLeft => Some(RenderCameraLeftStop),
            RenderCameraRight => Some(RenderCameraRightStop),
            RenderCameraUp => Some(RenderCameraUpStop),
            RenderCameraDown => Some(RenderCameraDownStop),
            RoomStartAreaSelect => Some(RoomFinishAreaSelect),
            RoomStartRoomResize => Some(RoomFinishRoomResize),
            PlacementFinish => Some(PlacementDragStart),
            _ => None,
        }
    }

    /// Hidden actions don't show on the UI
    pub(crate) fn hidden(self) -> bool {
        use self::KeyAction::*;
        match self {
            RenderCameraLeftStop
            | RenderCameraRightStop
            | RenderCameraUpStop
            | RenderCameraDownStop
            | RoomFinishAreaSelect
            | RoomFinishRoomResize
            | PlacementDragStart => true,
            _ => false,
        }
    }

    pub(crate) fn as_tooltip(self) -> &'static str {
        use self::KeyAction::*;
        match self {
            SystemMenu => {
                "Opens the system menu allowing you to save/exit a game. Pauses in single player"
            }
            BeginChat => "Begins a chat message",
            RenderZoomIn => "Causes the #camera# to zoom in",
            RenderZoomOut => "Causes the #camera# to zoom out",
            RenderRotateLeft => "Rotates the #camera# to the left",
            RenderRotateRight => "Rotates the #camera# to the right",
            RenderCameraLeft => "Causes the #camera# to start moving to the left",
            RenderCameraLeftStop => {
                "Stops the #camera# from moving to the left if it was currently moving"
            }
            RenderCameraRight => "Causes the #camera# to start moving to the right",
            RenderCameraRightStop => {
                "Stops the #camera# from moving to the right if it was currently moving"
            }
            RenderCameraUp => "Causes the #camera# to start moving up",
            RenderCameraUpStop => "Stops the #camera# from moving up if it was currently moving",
            RenderCameraDown => "Causes the #camera# to start moving down",
            RenderCameraDownStop => {
                "Stops the #camera# from moving down if it was currently moving"
            }
            RoomStartAreaSelect => "Starts selecting an area for a *building* or *room*",
            RoomStartAreaSelectStop => "Stops selecting an area for a *building* or *room*",
            RoomFinishAreaSelect => "Finishes selecting an area for a *building* or *room*",
            RoomStartRoomResize => "Starts resizing a *building* or *room*",
            RoomFinishRoomResize => "Stops resizing a *building* or *room*",
            PlacementFinish => "Finishes placing an *object* or *staff* member",
            PlacementDragStart => "Starts placing an *object* if it can be drag placed",
            PlacementRotate => "Rotates an *object*",
            PlacementMove => "Starts moving a placed *object*",
            PlacementRemove => "Removes a placed *object*",
            SelectEditRoom => "Begins editting a placed *building* or *room*",
            InspectMember => "Inspects the *student* or *staff* member under the mouse",
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        use self::KeyAction::*;
        match self {
            SystemMenu => "System Menu",
            BeginChat => "Begin Chat",
            RenderZoomIn => "Zoom In",
            RenderZoomOut => "Zoom Out",
            RenderRotateLeft => "Rotate Left",
            RenderRotateRight => "Rotate Right",
            RenderCameraLeft => "Camera Left",
            RenderCameraLeftStop => "Camera Left Stop",
            RenderCameraRight => "Camera Right",
            RenderCameraRightStop => "Camera Right Stop",
            RenderCameraUp => "Camera Up",
            RenderCameraUpStop => "Camera Up Stop",
            RenderCameraDown => "Camera Down",
            RenderCameraDownStop => "Camera Down Stop",
            RoomStartAreaSelect => "Start Area Select",
            RoomStartAreaSelectStop => "Stop Area Select",
            RoomFinishAreaSelect => "Finish Area Select",
            RoomStartRoomResize => "Start Room Resize",
            RoomFinishRoomResize => "Finish Room Resize",
            PlacementDragStart => "Placement Drag Start",
            PlacementFinish => "Placement Finish",
            PlacementRotate => "Placement Rotate",
            PlacementMove => "Placement Move",
            PlacementRemove => "Placement Remove",
            SelectEditRoom => "Select Edit Room",
            InspectMember => "Inspect Member",
        }
    }

    fn from_str(val: &str) -> Option<KeyAction> {
        use self::KeyAction::*;
        match val {
            "System Menu" => Some(SystemMenu),
            "Begin Chat" => Some(BeginChat),
            "Zoom In" => Some(RenderZoomIn),
            "Zoom Out" => Some(RenderZoomOut),
            "Rotate Left" => Some(RenderRotateLeft),
            "Rotate Right" => Some(RenderRotateRight),
            "Camera Left" => Some(RenderCameraLeft),
            "Camera Left Stop" => Some(RenderCameraLeftStop),
            "Camera Right" => Some(RenderCameraRight),
            "Camera Right Stop" => Some(RenderCameraRightStop),
            "Camera Up" => Some(RenderCameraUp),
            "Camera Up Stop" => Some(RenderCameraUpStop),
            "Camera Down" => Some(RenderCameraDown),
            "Camera Down Stop" => Some(RenderCameraDownStop),
            "Start Area Select" => Some(RoomStartAreaSelect),
            "Stop Area Select" => Some(RoomStartAreaSelectStop),
            "Finish Area Select" => Some(RoomFinishAreaSelect),
            "Start Room Resize" => Some(RoomStartRoomResize),
            "Finish Room Resize" => Some(RoomFinishRoomResize),
            "Placement Drag Start" => Some(PlacementDragStart),
            "Placement Finish" => Some(PlacementFinish),
            "Placement Rotate" => Some(PlacementRotate),
            "Placement Move" => Some(PlacementMove),
            "Placement Remove" => Some(PlacementRemove),
            "Select Edit Room" => Some(SelectEditRoom),
            "Inspect Member" => Some(InspectMember),
            _ => None,
        }
    }
}

/// Transforms binds to events
pub struct BindTransformer {
    /// Whether this transform should capture keys instead
    pub(crate) capture: bool,
    /// The last captured key if any
    pub(crate) captured_bind: Option<BindType>,
    current_collections: Vec<KeyCollection>,
    pub(crate) collections: FNVMap<KeyCollection, BindCollection>,
}

pub(crate) type BindStore = FNVMap<BindType, (Option<KeyAction>, Option<KeyAction>)>;

#[derive(Default)]
pub(crate) struct BindCollection {
    pub(crate) binds: BindStore,
}

impl BindCollection {
    fn set_bind(&mut self, bind: BindType, down: Option<KeyAction>, up: Option<KeyAction>) {
        self.binds.insert(bind, (down, up));
    }

    fn transform(&self, event: &SDLEvent) -> Option<Vec<KeyAction>> {
        match *event {
            SDLEvent::KeyDown { keycode, .. } => {
                return keycode
                    .and_then(|v| self.binds.get(&BindType::Key(v)))
                    .and_then(|v| v.0)
                    .map(|v| vec![v]);
            }
            SDLEvent::KeyUp { keycode, .. } => {
                return keycode
                    .and_then(|v| self.binds.get(&BindType::Key(v)))
                    .and_then(|v| v.1)
                    .map(|v| vec![v]);
            }
            SDLEvent::MouseWheel { y, .. } => {
                if let Some(&(ref up, ref down)) = self.binds.get(&BindType::MouseWheel(y > 0)) {
                    let mut out = vec![];
                    if let Some(up) = up.as_ref() {
                        out.push(*up);
                    }
                    if let Some(down) = down.as_ref() {
                        out.push(*down);
                    }
                    return Some(out);
                }
            }
            SDLEvent::MouseButtonDown { mouse_btn, .. } => {
                return self
                    .binds
                    .get(&BindType::Mouse(mouse_btn))
                    .and_then(|v| v.0)
                    .map(|v| vec![v]);
            }
            SDLEvent::MouseButtonUp { mouse_btn, .. } => {
                return self
                    .binds
                    .get(&BindType::Mouse(mouse_btn))
                    .and_then(|v| v.1)
                    .map(|v| vec![v]);
            }
            _ => {}
        }
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum BindType {
    Key(Keycode),
    Mouse(MouseButton),
    MouseWheel(bool),
}

impl BindType {
    pub(crate) fn as_string(self) -> String {
        use self::BindType::*;
        match self {
            Key(code) => code.name(),
            Mouse(MouseButton::Left) => "Mouse Left".to_owned(),
            Mouse(MouseButton::Middle) => "Mouse Middle".to_owned(),
            Mouse(MouseButton::Right) => "Mouse Right".to_owned(),
            Mouse(MouseButton::X1) => "Mouse X1".to_owned(),
            Mouse(MouseButton::X2) => "Mouse X2".to_owned(),
            Mouse(MouseButton::Unknown) => "Mouse Unknown".to_owned(),
            MouseWheel(true) => "M. Wheel Up".to_owned(),
            MouseWheel(false) => "M. Wheel Down".to_owned(),
        }
    }

    fn from_str(val: &str) -> Option<BindType> {
        use self::BindType::*;
        match val {
            "Mouse Left" => Some(Mouse(MouseButton::Left)),
            "Mouse Middle" => Some(Mouse(MouseButton::Middle)),
            "Mouse Right" => Some(Mouse(MouseButton::Right)),
            "Mouse X1" => Some(Mouse(MouseButton::X1)),
            "Mouse X2" => Some(Mouse(MouseButton::X2)),
            "Mouse Unknown" => Some(Mouse(MouseButton::Unknown)),
            "M. Wheel Up" => Some(MouseWheel(true)),
            "M. Wheel Down" => Some(MouseWheel(false)),
            val => Keycode::from_name(val).map(Key),
        }
    }
}

type ConfigMap = FNVMap<String, FNVMap<String, ConfigKeys>>;
#[derive(Serialize, Deserialize)]
struct ConfigKeys {
    #[serde(skip_serializing_if = "Option::is_none")]
    up: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    down: Option<String>,
}

const CONFIG_LOCATION: &str = "./keybinds.json";

impl BindTransformer {
    /// Creates a transformer that will load its settings
    /// from a file.
    pub fn new() -> BindTransformer {
        let mut binds = BindTransformer {
            capture: false,
            captured_bind: None,
            current_collections: vec![KeyCollection::Root],
            collections: Default::default(),
        };
        let config: ConfigMap = File::open(CONFIG_LOCATION)
            .ok()
            .and_then(|f| serde_json::from_reader(f).ok())
            .unwrap_or_default();
        binds.def_root(&config);
        binds.def_game(&config);
        binds.def_room_placement(&config);
        binds.def_room_resize(&config);
        binds.def_object_placement(&config);
        binds.def_staff_placement(&config);
        binds.def_room_build(&config);
        binds.def_edit_room(&config);
        binds
    }

    /// Returns the matching keys for a given action
    pub(crate) fn keys_for_action(
        &'_ self,
        action: KeyAction,
    ) -> impl Iterator<Item = (bool, BindType)> + '_ {
        use std::iter::once;
        self.current_collections
            .iter()
            .filter_map(move |v| self.collections.get(v))
            .flat_map(|v| &v.binds)
            .flat_map(|(btn, (down, up))| once((btn, true, down)).chain(once((btn, false, up))))
            .filter_map(move |(btn, dir, act)| {
                if *act == Some(action) {
                    Some((dir, *btn))
                } else {
                    None
                }
            })
    }

    /// Causes the keybinds to read keybinds from the passed collection.
    ///
    /// If the collection does not contain a key then the previous
    /// collections will be checked.
    pub fn add_collection(&mut self, name: KeyCollection) {
        self.current_collections.push(name);
    }

    /// Removes the last occurance of the named collection
    pub fn remove_collection(&mut self, name: KeyCollection) {
        if let Some(pos) = self.current_collections.iter().position(|v| *v == name) {
            self.current_collections.remove(pos);
        }
    }

    fn sdl_to_bind_type(event: &SDLEvent) -> Option<BindType> {
        match *event {
            SDLEvent::KeyUp { keycode, .. } => keycode.map(BindType::Key),
            SDLEvent::MouseWheel { y, .. } => Some(BindType::MouseWheel(y > 0)),
            SDLEvent::MouseButtonUp { mouse_btn, .. } => Some(BindType::Mouse(mouse_btn)),
            _ => None,
        }
    }

    /// Transforms the passed event into a normal event if a match is found
    pub fn transform(&mut self, event: &SDLEvent) -> Option<Vec<KeyAction>> {
        if self.capture {
            if let Some(bind) = Self::sdl_to_bind_type(event) {
                self.captured_bind = Some(bind);
            }
            return None;
        }
        for col in self.current_collections.iter().rev() {
            let col = &self.collections[col];
            if let Some(evts) = col.transform(event) {
                return Some(evts);
            }
        }
        None
    }

    /// Saves the current keybind settings to a file
    pub fn save(&self) -> UResult<()> {
        let mut config = ConfigMap::default();

        for (col, binds) in &self.collections {
            let mut keys: FNVMap<String, ConfigKeys> = FNVMap::default();
            for (ty, &(down, up)) in &binds.binds {
                keys.insert(
                    ty.as_string().to_owned(),
                    ConfigKeys {
                        up: up.map(KeyAction::as_str).map(|v| v.to_owned()),
                        down: down.map(KeyAction::as_str).map(|v| v.to_owned()),
                    },
                );
            }
            config.insert(col.as_str().to_owned(), keys);
        }
        serde_json::to_writer_pretty(File::create(CONFIG_LOCATION)?, &config)?;
        Ok(())
    }

    fn load_collection(
        &mut self,
        config: &ConfigMap,
        name: KeyCollection,
        mut def: BindCollection,
    ) {
        if let Some(col) = config.get(name.as_str()) {
            let orig = def;
            def = BindCollection::default();
            let mut set = FNVSet::default();
            for (k, v) in col {
                if let Some(key) = BindType::from_str(k) {
                    let down = v
                        .down
                        .as_ref()
                        .map(String::as_str)
                        .and_then(KeyAction::from_str);
                    let up =
                        v.up.as_ref()
                            .map(String::as_str)
                            .and_then(KeyAction::from_str);
                    down.map(|v| set.insert(v));
                    up.map(|v| set.insert(v));
                    def.set_bind(key, down, up);
                }
            }
            for (k, (down, up)) in orig.binds {
                let k = def.binds.entry(k).or_insert((None, None));
                if let Some(down) = down {
                    if !set.contains(&down) && k.0.is_none() {
                        k.0 = Some(down);
                    }
                }
                if let Some(up) = up {
                    if !set.contains(&up) && k.1.is_none() {
                        k.1 = Some(up);
                    }
                }
            }
        }
        self.collections.insert(name, def);
    }

    // Defaults

    // Keybinds for the default state
    fn def_root(&mut self, config: &ConfigMap) {
        let binds = BindCollection::default();
        self.load_collection(config, KeyCollection::Root, binds);
    }

    // Keybinds for the default in-game state
    fn def_game(&mut self, config: &ConfigMap) {
        let mut binds = BindCollection::default();

        binds.set_bind(
            BindType::Key(Keycode::Escape),
            None,
            Some(KeyAction::SystemMenu),
        );
        binds.set_bind(
            BindType::Key(Keycode::Return),
            None,
            Some(KeyAction::BeginChat),
        );

        binds.set_bind(
            BindType::MouseWheel(true),
            Some(KeyAction::RenderZoomIn),
            None,
        );
        binds.set_bind(
            BindType::MouseWheel(false),
            Some(KeyAction::RenderZoomOut),
            None,
        );
        binds.set_bind(
            BindType::Key(Keycode::PageDown),
            None,
            Some(KeyAction::RenderRotateLeft),
        );
        binds.set_bind(
            BindType::Key(Keycode::PageUp),
            None,
            Some(KeyAction::RenderRotateRight),
        );
        binds.set_bind(
            BindType::Mouse(MouseButton::Left),
            None,
            Some(KeyAction::InspectMember),
        );

        // Camera controls
        for &(a, b, action, stop) in &[
            (
                Keycode::Left,
                Keycode::A,
                KeyAction::RenderCameraLeft,
                KeyAction::RenderCameraLeftStop,
            ),
            (
                Keycode::Right,
                Keycode::D,
                KeyAction::RenderCameraRight,
                KeyAction::RenderCameraRightStop,
            ),
            (
                Keycode::Up,
                Keycode::W,
                KeyAction::RenderCameraUp,
                KeyAction::RenderCameraUpStop,
            ),
            (
                Keycode::Down,
                Keycode::S,
                KeyAction::RenderCameraDown,
                KeyAction::RenderCameraDownStop,
            ),
        ] {
            binds.set_bind(BindType::Key(a), Some(action), Some(stop));
            binds.set_bind(BindType::Key(b), Some(action), Some(stop));
        }

        self.load_collection(config, KeyCollection::Game, binds);
    }

    // Keybinds for placement
    fn def_room_placement(&mut self, config: &ConfigMap) {
        let mut binds = BindCollection::default();
        binds.set_bind(
            BindType::Mouse(MouseButton::Left),
            Some(KeyAction::RoomStartAreaSelect),
            Some(KeyAction::RoomFinishAreaSelect),
        );
        self.load_collection(config, KeyCollection::RoomPlacement, binds);
    }

    // Keybinds for resizing
    fn def_room_resize(&mut self, config: &ConfigMap) {
        let mut binds = BindCollection::default();
        binds.set_bind(
            BindType::Mouse(MouseButton::Left),
            Some(KeyAction::RoomStartRoomResize),
            Some(KeyAction::RoomFinishRoomResize),
        );
        self.load_collection(config, KeyCollection::RoomResize, binds);
    }

    // Keybinds for building a room
    fn def_room_build(&mut self, config: &ConfigMap) {
        let mut binds = BindCollection::default();
        binds.set_bind(
            BindType::Mouse(MouseButton::Left),
            None,
            Some(KeyAction::PlacementMove),
        );
        binds.set_bind(
            BindType::Mouse(MouseButton::Right),
            None,
            Some(KeyAction::PlacementRemove),
        );
        self.load_collection(config, KeyCollection::BuildRoom, binds);
    }

    // Keybinds for resizing
    fn def_object_placement(&mut self, config: &ConfigMap) {
        let mut binds = BindCollection::default();
        binds.set_bind(
            BindType::Mouse(MouseButton::Left),
            Some(KeyAction::PlacementDragStart),
            Some(KeyAction::PlacementFinish),
        );
        binds.set_bind(
            BindType::Mouse(MouseButton::Right),
            None,
            Some(KeyAction::PlacementRotate),
        );
        self.load_collection(config, KeyCollection::PlaceObject, binds);
    }

    // Keybinds for resizing
    fn def_staff_placement(&mut self, config: &ConfigMap) {
        let mut binds = BindCollection::default();
        binds.set_bind(
            BindType::Mouse(MouseButton::Left),
            None,
            Some(KeyAction::PlacementFinish),
        );
        self.load_collection(config, KeyCollection::PlaceStaff, binds);
    }

    // Keybinds for selecting a room to edit
    fn def_edit_room(&mut self, config: &ConfigMap) {
        let mut binds = BindCollection::default();

        binds.set_bind(
            BindType::Mouse(MouseButton::Left),
            None,
            Some(KeyAction::SelectEditRoom),
        );

        self.load_collection(config, KeyCollection::EditRoom, binds);
    }
}
