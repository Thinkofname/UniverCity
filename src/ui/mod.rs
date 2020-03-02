//! UI system for the game.

mod layout;
pub mod prompt;

use crate::prelude::*;
use crate::render;
use crate::script;
use crate::server::assets;
use crate::server::event;
use crate::server::lua::{self, Function, Ref, Scope, Table};

use sdl2::keyboard::Keycode;
use std::cell::{RefCell, RefMut as CellRefMut};
use std::rc::{Rc, Weak};

use fungui;
use fungui::StaticKey;

use crate::render::ui::color::Color;

/// The fungui node type used
pub type Node = fungui::Node<UniverCityUI>;
/// The weak version of the fungui node type used
pub type WeakNode = fungui::WeakNode<UniverCityUI>;
/// The fungui value type used
pub type Value = fungui::Value<UniverCityUI>;

/// An UnivercityUI specific flag for marking the font as changed
pub static FONT_FLAG: fungui::DirtyFlags = fungui::DirtyFlags::EXT_1;

enum SManager {}
impl script::LuaTracked for SManager {
    const KEY: script::NulledString = nul_str!("ui_manager_ref");
    type Storage = Weak<RefCell<fungui::Manager<UniverCityUI>>>;
    type Output = Rc<RefCell<fungui::Manager<UniverCityUI>>>;

    fn try_convert(s: &Self::Storage) -> Option<Self::Output> {
        s.upgrade()
    }
}

enum Events {}
impl script::LuaTracked for Events {
    const KEY: script::NulledString = nul_str!("ui_events_ref");
    type Storage = Weak<RefCell<event::Container>>;
    type Output = Rc<RefCell<event::Container>>;

    fn try_convert(s: &Self::Storage) -> Option<Self::Output> {
        s.upgrade()
    }
}

struct Tooltip {
    key: String,
    holder: Node,
}

impl script::LuaTracked for Tooltip {
    const KEY: script::NulledString = nul_str!("ui_tooltip_ref");
    type Storage = Weak<RefCell<Option<Tooltip>>>;
    type Output = Rc<RefCell<Option<Tooltip>>>;

    fn try_convert(s: &Self::Storage) -> Option<Self::Output> {
        s.upgrade()
    }
}

/// The UniverCity specific fungui extension
pub enum UniverCityUI {}

macro_rules! define_ui_events {
    ($($key:ident = $ety:ty;)*) => (

/// Collection of possible events that a node can
/// have
#[derive(Default)]
pub struct NodeEvents {
    $(
        $key: Box<[MethodDesc<$ety>]>,
    )*
}

// Keys
mod event_keys {
$(
    #[allow(non_upper_case_globals)]
    pub static $key: fungui::StaticKey = fungui::StaticKey(stringify!($key));
)*
}

macro_rules! register_ui_props {
    ($prop:ident) => (
        $(
            $prop(event_keys::$key);
        )*
    )
}

macro_rules! update_ui_props {
    ($styles:ident, $nc:ident, $rule:ident, $data:ident) => (
        $(
            eval!($styles, $nc, $rule.event_keys::$key => val => {
                if let Some(new) = MethodDesc::<$ety>::from_value(val) {
                    $data.events.borrow_mut().$key = new.into_boxed_slice();
                } else {
                    println!("Failed to convert into an event {}", stringify!($key));
                }
            });
        )*
    )
}
macro_rules! reset_ui_props {
    ($used:ident, $events:ident) => (
        $(
            if !$used.contains(&event_keys::$key) {
                $events.$key = Default::default();
            }
        )*
    )
}

    )
}

define_ui_events! {
    on_init = InitEvent;
    on_deinit = InitEvent;
    on_update = UpdateEvent;
    on_focus = FocusEvent;
    on_unfocus = FocusEvent;

    on_char_input = CharInputEvent;
    on_key_down = KeyDownEvent;
    on_key_up = KeyUpEvent;

    on_mouse_down = MouseDownEvent;
    on_mouse_up = MouseUpEvent;
    on_mouse_scroll = MouseScrollEvent;
    on_mouse_move_out = MouseMoveEvent;
    on_mouse_move_over = MouseMoveEvent;
    on_mouse_move = MouseMoveEvent;
}

/// UniverCity specific fungui node data
pub struct NodeData {
    // Would be nice to not require this but due to the
    // fact events would want to modify the node this
    // becomes required
    events: Rc<RefCell<NodeEvents>>,

    can_hover: bool,
    can_focus: bool,

    // Rendering
    pub(crate) render_loc: (f32, f32, f32, f32),

    pub(crate) image: Option<String>,
    pub(crate) image_render: Option<render::ui::ImageRender>,
    pub(crate) tint: Color,
    pub(crate) background_color: Option<Color>,
    pub(crate) box_render: Option<render::ui::BoxRender>,

    pub(crate) border_render: Option<render::ui::BorderRender>,
    pub(crate) border: Option<render::ui::border::Border>,
    pub(crate) border_width: Option<render::ui::border::BorderWidthInfo>,

    pub(crate) shadow_render: Vec<render::ui::ShadowRender>,
    pub(crate) shadows: Vec<render::ui::shadow::Shadow>,

    pub(crate) text_render: Option<render::ui::TextRender>,
    /// Optional list of text slices and draw positions to
    /// text split over mulitple locations
    pub(crate) text_splits: Vec<(usize, usize, fungui::Rect)>,
    pub(crate) font: Option<String>,
    pub(crate) font_color: Color,
    pub(crate) font_size: f32,
    pub(crate) text_shadow: Option<render::ui::text_shadow::TShadow>,
}

static IMAGE: StaticKey = StaticKey("image");
static BACKGROUND_COLOR: StaticKey = StaticKey("background_color");
static TINT: StaticKey = StaticKey("tint");
static FONT: StaticKey = StaticKey("font");
static FONT_SIZE: StaticKey = StaticKey("font_size");
static FONT_COLOR: StaticKey = StaticKey("font_color");
static TEXT_SHADOW: StaticKey = StaticKey("text_shadow");
static SHADOW: StaticKey = StaticKey("shadow");
pub(crate) static BORDER: StaticKey = StaticKey("border");
static BORDER_WIDTH: StaticKey = StaticKey("border_width");

static CAN_HOVER: StaticKey = StaticKey("can_hover");
static CAN_FOCUS: StaticKey = StaticKey("can_focus");

impl fungui::Extension for UniverCityUI {
    type NodeData = NodeData;
    type Value = UValue;

    fn new_data() -> Self::NodeData {
        NodeData {
            events: Default::default(),

            can_hover: false,
            can_focus: false,

            render_loc: Default::default(),

            image: None,
            image_render: None,
            tint: Default::default(),
            background_color: None,
            box_render: None,

            border_render: None,
            border: None,
            border_width: None,

            shadow_render: Vec::new(),
            shadows: Vec::new(),

            text_render: None,
            text_splits: Vec::new(),
            font: None,
            font_color: Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
            font_size: 16.0,
            text_shadow: None,
        }
    }

    fn style_properties<'a, F>(mut prop: F)
    where
        F: FnMut(fungui::StaticKey) + 'a,
    {
        // Events
        register_ui_props!(prop);

        prop(CAN_HOVER);
        prop(CAN_FOCUS);

        // Rendering
        prop(FONT);
        prop(FONT_SIZE);
        prop(FONT_COLOR);
        prop(TEXT_SHADOW);
        prop(TINT);
        prop(BACKGROUND_COLOR);
        prop(SHADOW);
        prop(IMAGE);
        prop(BORDER);
        prop(BORDER_WIDTH);
    }

    fn update_data(
        styles: &fungui::Styles<Self>,
        nc: &fungui::NodeChain<'_, Self>,
        rule: &fungui::Rule<Self>,
        data: &mut Self::NodeData,
    ) -> fungui::DirtyFlags {
        let mut flags = fungui::DirtyFlags::empty();
        update_ui_props!(styles, nc, rule, data);

        eval!(styles, nc, rule.CAN_HOVER => val => {
            if let Some(new) = val.convert::<bool>() {
                data.can_hover = new;
            }
        });
        eval!(styles, nc, rule.CAN_FOCUS => val => {
            if let Some(new) = val.convert::<bool>() {
                data.can_focus = new;
            }
        });

        eval!(styles, nc, rule.IMAGE => val => {
            if let Some(new) = val.convert::<String>() {
                if data.image.as_ref().map_or(true, |v| *v != new) {
                    data.image = Some(new);
                    data.image_render = None;
                }
            }
        });
        eval!(styles, nc, rule.TINT => val => {
            if let Some(new) = Color::from_val(val) {
                if data.tint != new {
                    data.tint = new;
                    data.image_render = None;
                    data.border_render = None;
                }
            }
        });
        eval!(styles, nc, rule.BACKGROUND_COLOR => val => {
            if let Some(new) = Color::from_val(val) {
                if data.background_color.as_ref().map_or(true, |v| *v != new) {
                    data.background_color = Some(new);
                    data.box_render = None;
                }
            }
        });

        eval!(styles, nc, rule.BORDER => val => {
            let new = val.convert::<render::ui::border::Border>();
            if data.border != new {
                data.border = new;
                data.border_render = None;
            }
        });
        eval!(styles, nc, rule.BORDER_WIDTH => val => {
            let new = val.convert::<render::ui::border::BorderWidthInfo>();
            if data.border_width != new {
                data.border_width = new;
                data.border_render = None;
            }
        });

        eval!(styles, nc, rule.SHADOW => val => {
            if let Some(new) = val.convert::<Vec<render::ui::shadow::Shadow>>() {
                if data.shadows != new {
                    data.shadows = new;
                    data.shadow_render.clear();
                }
            }
        });

        eval!(styles, nc, rule.FONT => val => {
            if let Some(new) = val.convert::<String>() {
                if data.font.as_ref().map_or(true, |v| *v != new) {
                    data.font = Some(new);
                    data.text_render = None;
                    flags |= FONT_FLAG;
                }
            }
        });
        eval!(styles, nc, rule.FONT_COLOR => val => {
            if let Some(new) = Color::from_val(val) {
                if data.font_color != new {
                    data.font_color = new;
                    data.text_render = None;
                }
            }
        });
        eval!(styles, nc, rule.FONT_SIZE => val => {
            if let Some(new) = val.convert::<f32>() {
                if data.font_size != new {
                    data.font_size = new;
                    data.text_render = None;
                    flags |= FONT_FLAG;
                }
            }
        });
        eval!(styles, nc, rule.TEXT_SHADOW => val => {
            if let Some(new) = val.convert::<render::ui::text_shadow::TShadow>() {
                let new = Some(new);
                if data.text_shadow != new {
                    data.text_shadow = new;
                    data.text_render = None;
                }
            }
        });

        flags
    }

    fn reset_unset_data(
        used_keys: &fungui::FnvHashSet<fungui::StaticKey>,
        data: &mut Self::NodeData,
    ) -> fungui::DirtyFlags {
        let mut flags = fungui::DirtyFlags::empty();
        let mut events = data.events.borrow_mut();
        reset_ui_props!(used_keys, events);

        if !used_keys.contains(&CAN_HOVER) {
            data.can_hover = false;
        }
        if !used_keys.contains(&CAN_FOCUS) {
            data.can_focus = false;
        }

        if !used_keys.contains(&IMAGE) {
            data.image = None;
            data.image_render = None;
        }
        if !used_keys.contains(&TINT) {
            let new = Default::default();
            if data.tint != new {
                data.tint = new;
                data.image_render = None;
                data.border_render = None;
            }
        }
        if !used_keys.contains(&BACKGROUND_COLOR) {
            data.background_color = None;
            data.box_render = None;
        }
        if !used_keys.contains(&BORDER) || !used_keys.contains(&BORDER_WIDTH) {
            data.border = None;
            data.border_render = None;
        }

        if !used_keys.contains(&FONT) {
            data.font = None;
            data.text_render = None;
            flags |= FONT_FLAG;
        }
        if !used_keys.contains(&FONT_SIZE) && data.font_size != 16.0 {
            data.font_size = 16.0;
            data.text_render = None;
            flags |= fungui::DirtyFlags::SIZE;
            flags |= FONT_FLAG;
        }
        if !used_keys.contains(&FONT_COLOR) {
            const DEF_COLOR: Color = Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            };
            if data.font_color != DEF_COLOR {
                data.font_color = DEF_COLOR;
                data.text_render = None;
            }
        }
        if data.text_shadow.is_some() && !used_keys.contains(&TEXT_SHADOW) {
            data.text_shadow = None;
            data.text_render = None;
        }
        if !used_keys.contains(&SHADOW) {
            data.shadows.clear();
            data.shadow_render.clear();
        }

        flags
    }

    fn check_flags(data: &mut Self::NodeData, flags: fungui::DirtyFlags) {
        // TODO: Handle this without recreating?
        if flags.contains(fungui::DirtyFlags::POSITION) || flags.contains(fungui::DirtyFlags::SIZE)
        {
            data.image_render = None;
            data.box_render = None;
            data.text_render = None;
            data.border_render = None;
            data.shadow_render.clear();
            data.text_splits.clear();
        }
        if flags.contains(fungui::DirtyFlags::TEXT) {
            data.text_splits.clear();
        }
    }
}

/// Manages all UI elements.
pub struct Manager {
    log: Logger,
    ui_scale: f32,
    /// The fungui ui manager
    pub manager: Rc<RefCell<fungui::Manager<UniverCityUI>>>,
    /// Contains events emitted by ui elements
    pub events: Rc<RefCell<event::Container>>,

    tooltip: Rc<RefCell<Option<Tooltip>>>,

    current_focus: Option<WeakNode>,
    last_hover: Option<WeakNode>,
    assets: AssetManager,
    /// The scripting engine used by the UI system.
    ///
    /// Should match the one used by the game if one is running.
    pub scripting: script::Engine,

    style_groups: FNVMap<ResourceKey<'static>, Vec<String>>,

    // Used for init/deinit checking
    cycle: bool,
    nodes: Vec<Node>,
}

fn list<'a>(
    args: &mut (dyn Iterator<Item = fungui::FResult<'a, Value>> + 'a),
) -> fungui::FResult<'a, Value> {
    Ok(fungui::Value::ExtValue(UValue::UntypedList(
        args.collect::<fungui::FResult<'_, _>>()?,
    )))
}

impl Manager {
    /// Creates a new ui manager that loads the ui descriptions from
    /// the passed asset maanger.
    pub fn new(log: Logger, asset_manager: AssetManager) -> Manager {
        Manager {
            scripting: script::Engine::empty(&log),
            log,
            assets: asset_manager.clone(),
            ui_scale: 1.0,
            manager: Rc::new(RefCell::new({
                let mut manager = fungui::Manager::new();
                manager.add_layout_engine(layout::Center::default);
                manager.add_layout_engine(layout::Padded::default);
                manager.add_layout_engine(layout::Tooltip::default);
                manager.add_layout_engine(layout::Rows::default);
                manager.add_layout_engine(layout::RowsInv::default);
                manager.add_layout_engine(layout::Clipped::default);
                manager.add_layout_engine(layout::Grid::default);

                manager.add_func_raw("list", list);

                manager
            })),
            events: Rc::new(RefCell::new(event::Container::new())),

            tooltip: Rc::new(RefCell::new(None)),

            current_focus: None,
            last_hover: None,

            style_groups: FNVMap::default(),

            cycle: false,
            nodes: Vec::new(),
        }
    }

    /// Sets the current UI scale
    pub fn set_ui_scale(&mut self, scale: f32) {
        self.ui_scale = scale;
    }

    /// Handles text boxes
    pub fn update(&mut self, renderer: &mut render::Renderer, delta: f64) {
        crate::server::script::handle_reloads(&self.log, &self.scripting, &self.assets);
        if let Some(node) = self
            .manager
            .borrow()
            .query()
            .property("focused", true)
            .next()
        {
            let n = node.borrow();
            if !n.ext.events.borrow().on_char_input.is_empty() {
                if let Some(rect) = node.render_position() {
                    renderer.mark_text_input(rect.x, rect.y, rect.width, rect.height);
                }
            }
        } else {
            self.current_focus = None;
        }

        self.cycle = !self.cycle;
        let scripting = self.scripting.clone();
        let mut events = event::Container::new();

        for node in self.manager.borrow().query().matches() {
            node.raw_set_property("$cycle", self.cycle);
            if node.has_layout() && node.get_property::<bool>("$init").is_none() {
                node.raw_set_property("$init", true);
                invoke_event(
                    &self.log,
                    &mut events,
                    &scripting,
                    &node,
                    |v| &mut v.on_init,
                    &(),
                );
                self.nodes.push(node.clone());
            }
            invoke_event(
                &self.log,
                &mut events,
                &scripting,
                &node,
                |v| &mut v.on_update,
                &delta,
            );
        }

        let cycle = self.cycle;
        let log = &self.log;
        self.nodes.retain(|v| {
            if v.get_property::<bool>("$cycle")
                .map_or(true, |c| c != cycle)
            {
                if v.get_property::<bool>("$init").is_some() {
                    invoke_event(log, &mut events, &scripting, v, |v| &mut v.on_deinit, &());
                }
                false
            } else {
                true
            }
        });
        self.events.borrow_mut().join(events);
    }

    /// Loads the named style rules
    pub fn load_styles(&mut self, key: ResourceKey<'_>) {
        use std::io::Read;

        // Save repeated derefs
        let manager: &mut fungui::Manager<_> = &mut *self.manager.borrow_mut();

        // Remove the old styles in this group if they exist
        for old in self
            .style_groups
            .remove(&key.borrow().into_owned())
            .into_iter()
            .flat_map(|v| v)
        {
            manager.remove_styles(&old);
        }

        let mut group = Vec::new();
        let mut styles = String::new();
        let mut res = if let Ok(res) = self
            .assets
            .open_from_pack(key.module_key(), &format!("ui/{}.list", key.resource()))
        {
            res
        } else {
            error!(self.log, "Missing style rule list {:?}", key);
            return;
        };
        assume!(self.log, res.read_to_string(&mut styles));
        for line in styles.lines() {
            let line = line.trim();
            // Skip empty lines/comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Support cross module loading
            let s_key = LazyResourceKey::parse(line).or_module(key.module_key());
            group.push(s_key.as_string());

            let mut style = String::new();
            let mut res = assume!(
                self.log,
                self.assets.open_from_pack(
                    s_key.module_key(),
                    &format!("ui/{}.style", s_key.resource())
                )
            );
            assume!(self.log, res.read_to_string(&mut style));
            // Instead of failing on error just report it in the console.
            // TODO: Maybe report on screen somewhere?
            if let Err(err) = manager.load_styles(&s_key.as_string(), &style) {
                let mut out: Vec<u8> = Vec::new();
                assume!(
                    self.log,
                    fungui::format_parse_error(&mut out, style.lines(), err,)
                );
                warn!(
                    self.log,
                    "Failed to parse {:?}\n{}",
                    s_key,
                    String::from_utf8_lossy(&out)
                );
            }
        }

        self.style_groups.insert(key.into_owned(), group);
    }

    /// Loads and adds the node as described by the resource.
    pub fn create_node(&self, key: ResourceKey<'_>) -> Node {
        let node =
            Self::create_node_impl(&self.log, &self.assets, key).expect("Failed to load node");
        self.manager.borrow_mut().add_node(node.clone());
        node
    }

    fn create_node_impl(
        log: &Logger,
        assets: &AssetManager,
        key: ResourceKey<'_>,
    ) -> UResult<Node> {
        use std::io::Read;
        let mut desc = String::new();
        let mut res =
            assets.open_from_pack(key.module_key(), &format!("ui/{}.desc", key.resource()))?;
        res.read_to_string(&mut desc)?;
        Node::from_str(&desc).map_err(|err| {
            let mut out: Vec<u8> = Vec::new();
            assume!(
                log,
                fungui::format_parse_error(&mut out, desc.lines(), err,)
            );
            ErrorKind::UINodeLoadError(key.into_owned(), String::from_utf8_lossy(&out).into_owned())
                .into()
        })
    }

    /// Adds the passed node to the root node
    pub fn add_node(&self, node: Node) {
        self.manager.borrow_mut().add_node(node);
    }

    /// Removes the passed node from the root node
    pub fn remove_node(&self, node: Node) {
        self.manager.borrow_mut().remove_node(node);
    }

    /// Handles events targetting the focused element
    pub fn focused_event<E>(&mut self, param: E::Param) -> bool
    where
        E: Event + 'static,
    {
        let mut events = event::Container::new();
        if let Some(node) = self.current_focus.as_ref().and_then(|v| v.upgrade()) {
            if invoke_event(
                &self.log,
                &mut events,
                &self.scripting,
                &node,
                E::event_funcs,
                &param,
            ) {
                self.events.borrow_mut().join(events);
                return true;
            }
        }
        self.events.borrow_mut().join(events);
        false
    }

    /// Handles mouse move events
    pub fn mouse_event<E>(&mut self, x: i32, y: i32, param: E::Param) -> bool
    where
        E: Event + 'static,
    {
        let x = ((x as f32) * self.ui_scale) as i32;
        let y = ((y as f32) * self.ui_scale) as i32;
        let matches = {
            let manager = self.manager.borrow();
            manager.query_at(x, y).matches()
        };
        let mut events = event::Container::new();
        for node in matches {
            if invoke_event(
                &self.log,
                &mut events,
                &self.scripting,
                &node,
                E::event_funcs,
                &param,
            ) {
                self.events.borrow_mut().join(events);
                return true;
            }
        }
        self.events.borrow_mut().join(events);
        false
    }

    /// Handles mouse move events
    pub fn mouse_move(&mut self, x: i32, y: i32) -> bool {
        let x = ((x as f32) * self.ui_scale) as i32;
        let y = ((y as f32) * self.ui_scale) as i32;
        let matches = {
            let manager = self.manager.borrow();
            manager.query_at(x, y).matches()
        };
        let mut events = event::Container::new();
        for node in matches {
            if node.borrow().ext.can_hover {
                if self
                    .last_hover
                    .as_ref()
                    .and_then(|v| v.upgrade())
                    .map_or(true, |v| !v.is_same(&node))
                {
                    if let Some(last_hover) = self.last_hover.take().and_then(|v| v.upgrade()) {
                        last_hover.set_property("hover", false);
                        invoke_event(
                            &self.log,
                            &mut events,
                            &self.scripting,
                            &last_hover,
                            |v| &mut v.on_mouse_move_out,
                            &MouseMove { x, y },
                        );
                    }
                    node.set_property("hover", true);
                    self.last_hover = Some(node.weak());
                    invoke_event(
                        &self.log,
                        &mut events,
                        &self.scripting,
                        &node,
                        |v| &mut v.on_mouse_move_over,
                        &MouseMove { x, y },
                    );
                }
                invoke_event(
                    &self.log,
                    &mut events,
                    &self.scripting,
                    &node,
                    |v| &mut v.on_mouse_move,
                    &MouseMove { x, y },
                );
                self.events.borrow_mut().join(events);
                return true;
            }
        }
        if let Some(last_hover) = self.last_hover.take().and_then(|v| v.upgrade()) {
            last_hover.set_property("hover", false);
            invoke_event(
                &self.log,
                &mut events,
                &self.scripting,
                &last_hover,
                |v| &mut v.on_mouse_move_out,
                &MouseMove { x, y },
            );
        }
        self.events.borrow_mut().join(events);
        false
    }

    // Proxy methods to Elements

    /// Returns the event container
    pub fn events(&self) -> CellRefMut<'_, event::Container> {
        self.events.borrow_mut()
    }

    /// Changes the scripting engine to the passed engine.
    ///
    /// This clears all active ui elements.
    pub fn set_script_engine(&mut self, audio: &AudioManager, engine: &script::Engine) {
        engine.store_tracked::<SManager>(Rc::downgrade(&self.manager));
        engine.store_tracked::<Events>(Rc::downgrade(&self.events));
        engine.store_tracked::<Tooltip>(Rc::downgrade(&self.tooltip));
        engine.store_tracked::<AudioController>(Rc::downgrade(&audio.controller));
        self.scripting = engine.clone();

        let mut manager = self.manager.borrow_mut();
        if let Some(root) = manager.query().name("root").matches().next() {
            for c in root.children() {
                manager.remove_node(c);
            }
        }
        let ver = node!(game_version);
        ver.add_child(node!(@text("UniverCity: ")));
        ver.add_child({
            let txt = Node::new_text(env!("CARGO_PKG_VERSION"));
            txt.set_property("version", true);
            txt
        });
        ver.add_child(node!(@text("-")));
        ver.add_child({
            let txt = Node::new_text(server::GAME_HASH);
            txt.set_property("hash", true);
            txt
        });
        manager.add_node(ver);
    }

    /// Changes the scripting engine to the default engine.
    ///
    /// This clears all active ui elements.
    pub fn clear_script_engine(&mut self, audio: &AudioManager) {
        let scripting = script::Engine::new(&self.log, self.assets.clone());
        self.set_script_engine(audio, &scripting);
        for pack in self.assets.get_packs() {
            scripting.init_pack(pack.module());
        }
    }

    /// Returns the current scripting engine
    pub fn get_script_engine(&self) -> &script::Engine {
        &self.scripting
    }

    /// Focuses the passed node
    pub fn focus_node(&mut self, node: Node) {
        if let Some(current) = self.current_focus.as_ref().and_then(|v| v.upgrade()) {
            current.set_property("focused", false);
            invoke_event(
                &self.log,
                &mut *self.events.borrow_mut(),
                &self.scripting,
                &current,
                |v| &mut v.on_unfocus,
                &(),
            );
        }
        self.current_focus = Some(node.weak());
        node.set_property("focused", true);
        invoke_event(
            &self.log,
            &mut *self.events.borrow_mut(),
            &self.scripting,
            &node,
            |v| &mut v.on_focus,
            &(),
        );
    }

    /// Cycles the focus to the next element that can take input
    /// if one exists
    pub fn cycle_focus(&mut self) {
        let manager = self.manager.borrow();
        let mut events = event::Container::new();
        let mut current = self.current_focus.as_ref().and_then(|v| v.upgrade());

        let matches = manager.query().matches().collect::<Vec<_>>();
        let mut can_loop = true;
        while can_loop {
            can_loop = false;
            for node in matches.iter().rev() {
                if current.as_ref().map_or(false, |v| v.is_same(node)) {
                    node.set_property("focused", false);
                    invoke_event(
                        &self.log,
                        &mut events,
                        &self.scripting,
                        node,
                        |v| &mut v.on_unfocus,
                        &(),
                    );
                    current = None;
                    can_loop = true;
                } else if current.is_none() && node.borrow().ext.can_focus {
                    node.set_property("focused", true);
                    invoke_event(
                        &self.log,
                        &mut events,
                        &self.scripting,
                        node,
                        |v| &mut v.on_focus,
                        &(),
                    );
                    self.current_focus = Some(node.weak());
                    can_loop = false;
                    break;
                }
            }
            if current.is_some() {
                current = None;
                can_loop = true;
            }
        }
        self.events.borrow_mut().join(events);
    }

    /// Returns the key of the current tooltip
    pub fn current_tooltip(&mut self) -> Option<String> {
        let tooltip = self.tooltip.borrow();
        if let Some(tp) = tooltip.as_ref() {
            Some(tp.key.clone())
        } else {
            None
        }
    }

    /// Shows the named tooltip at the given location if its not shown already
    pub fn show_tooltip(&mut self, key: &str, content: Node, x: i32, y: i32) {
        let mut tooltip = self.tooltip.borrow_mut();
        let mut manager = self.manager.borrow_mut();
        let x = ((x as f32) * self.ui_scale) as i32;
        let y = ((y as f32) * self.ui_scale) as i32;
        Self::show_tooltip_impl(&mut *manager, &mut *tooltip, key, content, x, y);
    }

    fn show_tooltip_impl(
        manager: &mut fungui::Manager<UniverCityUI>,
        tooltip: &mut Option<Tooltip>,
        key: &str,
        content: Node,
        x: i32,
        y: i32,
    ) {
        if let Some(old) = tooltip.take() {
            if old.key == *key {
                *tooltip = Some(old);
                return;
            } else {
                manager.remove_node(old.holder);
            }
        }

        let node = node! {
            tooltip_holder {
                tooltip(x=x + 8, y=y + 8) {

                }
            }
        };
        if let Some(tt) = query!(node, tooltip_holder > tooltip).next() {
            tt.add_child(content);
        }

        manager.add_node(node.clone());
        *tooltip = Some(Tooltip {
            key: key.to_string(),
            holder: node,
        });
    }

    /// Moves the tooltip with the given key if active
    pub fn move_tooltip(&mut self, key: &str, x: i32, y: i32) {
        let tooltip = self.tooltip.borrow();
        let x = ((x as f32) * self.ui_scale) as i32;
        let y = ((y as f32) * self.ui_scale) as i32;
        Self::move_tooltip_impl(&*tooltip, key, x, y);
    }

    fn move_tooltip_impl(tooltip: &Option<Tooltip>, key: &str, x: i32, y: i32) {
        if let Some(tt) = tooltip.as_ref() {
            if tt.key == *key {
                if let Some(t) = query!(tt.holder, tooltip_holder > tooltip).next() {
                    t.set_property("x", x + 8);
                    t.set_property("y", y + 8);
                }
            }
        }
    }

    /// Hides the tooltip with the given key
    pub fn hide_tooltip(&mut self, key: &str) {
        let mut tooltip = self.tooltip.borrow_mut();
        let mut manager = self.manager.borrow_mut();
        Self::hide_tooltip_impl(&mut *manager, &mut *tooltip, key);
    }

    fn hide_tooltip_impl(
        manager: &mut fungui::Manager<UniverCityUI>,
        tooltip: &mut Option<Tooltip>,
        key: &str,
    ) {
        if let Some(old) = tooltip.take() {
            if old.key == *key {
                manager.remove_node(old.holder);
            } else {
                *tooltip = Some(old);
            }
        }
    }
}

pub(super) struct FocusNode(pub Node);

pub(crate) struct NodeRef(pub(crate) Node);
impl lua::LuaUsable for NodeRef {
    fn metatable(t: &lua::TypeBuilder) {
        crate::server::script::support_getters_setters(t);
    }

    fn fields(t: &lua::TypeBuilder) {
        t.field(
            "get_text",
            lua::closure1(|lua, this: Ref<NodeRef>| -> Option<Ref<String>> {
                this.0.text().map(|v| Ref::new_string(lua, &*v))
            }),
        );
        t.field(
            "set_text",
            lua::closure2(|_lua, this: Ref<NodeRef>, txt: Ref<String>| {
                this.0.set_text(&*txt);
            }),
        );
        t.field(
            "query",
            lua::closure1(|lua, this: Ref<NodeRef>| {
                Ref::new(
                    lua,
                    QueryRef(RefCell::new(Some(this.0.query().into_owned()))),
                )
            }),
        );
        t.field(
            "get_parent",
            lua::closure1(|lua, this: Ref<NodeRef>| -> Option<Ref<NodeRef>> {
                this.0.parent().map(|v| Ref::new(lua, NodeRef(v)))
            }),
        );
        t.field(
            "render_position",
            lua::closure1(|_lua, this: Ref<NodeRef>| -> Option<(i32, i32, i32, i32)> {
                this.0
                    .render_position()
                    .map(|v| (v.x, v.y, v.width, v.height))
            }),
        );
        t.field(
            "raw_position",
            lua::closure1(|_lua, this: Ref<NodeRef>| -> (i32, i32, i32, i32) {
                let v = this.0.raw_position();
                (v.x, v.y, v.width, v.height)
            }),
        );
        t.field(
            "focus",
            lua::closure1(|lua, this: Ref<NodeRef>| -> UResult<()> {
                let events = lua
                    .get_tracked::<Events>()
                    .ok_or_else(|| ErrorKind::UINotBound)?;
                let mut events = events.borrow_mut();
                events.emit(FocusNode(this.0.clone()));
                Ok(())
            }),
        );
        t.field(
            "add_child",
            lua::closure2(|_lua, this: Ref<NodeRef>, other: Ref<NodeRef>| {
                this.0.add_child(other.0.clone())
            }),
        );
        t.field(
            "remove_child",
            lua::closure2(|_lua, this: Ref<NodeRef>, other: Ref<NodeRef>| {
                this.0.remove_child(other.0.clone())
            }),
        );

        #[allow(clippy::map_clone)]
        t.field(
            "get_property_table",
            lua::closure2(
                |_lua, this: Ref<NodeRef>, key: Ref<String>| -> Option<Ref<Table>> {
                    this.0.get_property_ref::<LuaTable>(&key).map(|v| v.clone())
                },
            ),
        );
        t.field(
            "set_property_table",
            lua::closure3(
                |_lua, this: Ref<NodeRef>, key: Ref<String>, val: Ref<Table>| {
                    if key.starts_with('$') {
                        this.0.raw_set_property(&key, LuaTable(val));
                    } else {
                        this.0.set_property(&key, LuaTable(val));
                    }
                },
            ),
        );

        t.field(
            "get_property_int",
            lua::closure2(
                |_lua, this: Ref<NodeRef>, key: Ref<String>| -> Option<i32> {
                    this.0.get_property::<i32>(&key)
                },
            ),
        );
        t.field(
            "set_property_int",
            lua::closure3(|_lua, this: Ref<NodeRef>, key: Ref<String>, val: i32| {
                if key.starts_with('$') {
                    this.0.raw_set_property(&key, val);
                } else {
                    this.0.set_property(&key, val);
                }
            }),
        );
        t.field(
            "get_property_float",
            lua::closure2(
                |_lua, this: Ref<NodeRef>, key: Ref<String>| -> Option<f64> {
                    this.0.get_property::<f64>(&key)
                },
            ),
        );
        t.field(
            "set_property_float",
            lua::closure3(|_lua, this: Ref<NodeRef>, key: Ref<String>, val: f64| {
                if key.starts_with('$') {
                    this.0.raw_set_property(&key, val);
                } else {
                    this.0.set_property(&key, val);
                }
            }),
        );
        t.field(
            "get_property_bool",
            lua::closure2(
                |_lua, this: Ref<NodeRef>, key: Ref<String>| -> Option<bool> {
                    this.0.get_property::<bool>(&key)
                },
            ),
        );
        t.field(
            "set_property_bool",
            lua::closure3(|_lua, this: Ref<NodeRef>, key: Ref<String>, val: bool| {
                if key.starts_with('$') {
                    this.0.raw_set_property(&key, val);
                } else {
                    this.0.set_property(&key, val);
                }
            }),
        );
        t.field(
            "get_property_string",
            lua::closure2(
                |lua, this: Ref<NodeRef>, key: Ref<String>| -> Option<Ref<String>> {
                    this.0
                        .get_property::<String>(&key)
                        .map(|v| Ref::new_string(lua, v))
                },
            ),
        );
        t.field(
            "set_property_string",
            lua::closure3(
                |_lua, this: Ref<NodeRef>, key: Ref<String>, val: Ref<String>| {
                    if key.starts_with('$') {
                        this.0.raw_set_property(&key, val.to_string());
                    } else {
                        this.0.set_property(&key, val.to_string());
                    }
                },
            ),
        );
    }
}

/// A wrapper around lua's table type to allow it to be used
/// as property.
#[derive(Clone)]
pub struct LuaTable(pub Ref<Table>);

struct QueryRef(RefCell<Option<fungui::Query<'static, UniverCityUI>>>);
impl lua::LuaUsable for QueryRef {
    fn fields(t: &lua::TypeBuilder) {
        t.field(
            "text",
            lua::closure1(|_lua, this: Ref<QueryRef>| -> UResult<Ref<QueryRef>> {
                {
                    let mut query = this.0.borrow_mut();
                    if let Some(q) = query.take() {
                        *query = Some(q.text());
                    } else {
                        bail!("Query already used")
                    }
                }
                Ok(this)
            }),
        );
        t.field(
            "name",
            lua::closure2(
                |_lua, this: Ref<QueryRef>, name: Ref<String>| -> UResult<Ref<QueryRef>> {
                    {
                        let mut query = this.0.borrow_mut();
                        if let Some(q) = query.take() {
                            *query = Some(q.name(name.to_string()));
                        } else {
                            bail!("Query already used")
                        }
                    }
                    Ok(this)
                },
            ),
        );
        t.field(
            "child",
            lua::closure1(|_lua, this: Ref<QueryRef>| -> UResult<Ref<QueryRef>> {
                {
                    let mut query = this.0.borrow_mut();
                    if let Some(q) = query.take() {
                        *query = Some(q.child());
                    } else {
                        bail!("Query already used")
                    }
                }
                Ok(this)
            }),
        );
        t.field(
            "property_bool",
            lua::closure3(
                |_lua, this: Ref<QueryRef>, name: Ref<String>, v: bool| -> UResult<Ref<QueryRef>> {
                    {
                        let mut query = this.0.borrow_mut();
                        if let Some(q) = query.take() {
                            *query = Some(q.property(name.to_string(), v));
                        } else {
                            bail!("Query already used")
                        }
                    }
                    Ok(this)
                },
            ),
        );
        t.field(
            "property_int",
            lua::closure3(
                |_lua, this: Ref<QueryRef>, name: Ref<String>, v: i32| -> UResult<Ref<QueryRef>> {
                    {
                        let mut query = this.0.borrow_mut();
                        if let Some(q) = query.take() {
                            *query = Some(q.property(name.to_string(), v));
                        } else {
                            bail!("Query already used")
                        }
                    }
                    Ok(this)
                },
            ),
        );
        t.field(
            "property_float",
            lua::closure3(
                |_lua, this: Ref<QueryRef>, name: Ref<String>, v: f64| -> UResult<Ref<QueryRef>> {
                    {
                        let mut query = this.0.borrow_mut();
                        if let Some(q) = query.take() {
                            *query = Some(q.property(name.to_string(), v));
                        } else {
                            bail!("Query already used")
                        }
                    }
                    Ok(this)
                },
            ),
        );
        t.field(
            "property_string",
            lua::closure3(
                |_lua,
                 this: Ref<QueryRef>,
                 name: Ref<String>,
                 v: Ref<String>|
                 -> UResult<Ref<QueryRef>> {
                    {
                        let mut query = this.0.borrow_mut();
                        if let Some(q) = query.take() {
                            *query = Some(q.property(name.to_string(), v.to_string()));
                        } else {
                            bail!("Query already used")
                        }
                    }
                    Ok(this)
                },
            ),
        );

        t.field(
            "matches",
            lua::closure1(|_lua, this: Ref<QueryRef>| -> UResult<_> {
                let mut query = this.0.borrow_mut();
                if let Some(q) = query.take() {
                    let mut iter = q.matches();
                    Ok(lua::closure2(move |lua, _: (), _: ()| {
                        iter.next().map(|v| Ref::new(lua, NodeRef(v)))
                    }))
                } else {
                    bail!("Query already used")
                }
            }),
        )
    }
}

// Events

/// References a button on the mouse
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    /// Left mouse button
    Left,
    /// Right mouse button
    Right,
    /// Middle mouse button
    Middle,
    /// Unknown mouse button
    Unknown,
}

impl From<::sdl2::mouse::MouseButton> for MouseButton {
    fn from(m: ::sdl2::mouse::MouseButton) -> MouseButton {
        use ::sdl2::mouse::MouseButton::*;
        match m {
            Left => MouseButton::Left,
            Right => MouseButton::Right,
            Middle => MouseButton::Middle,
            _ => MouseButton::Unknown,
        }
    }
}

impl MouseButton {
    /// Returns a lower case string form of the button
    pub fn as_string(self) -> &'static str {
        match self {
            MouseButton::Left => "left",
            MouseButton::Right => "right",
            MouseButton::Middle => "middle",
            MouseButton::Unknown => "unknown",
        }
    }
}

/// The parameter to mouse click events
pub struct MouseClick {
    /// The mouse button pressed if any
    pub button: MouseButton,
    /// The x position of the mouse
    pub x: i32,
    /// The y position of the mouse
    pub y: i32,
}

impl EventParam for MouseClick {
    fn as_lua_table(&self, lua: &lua::Lua) -> Ref<lua::Table> {
        let table = Ref::new_table(lua);
        table.insert(
            Ref::new_string(lua, "button"),
            Ref::new_string(lua, self.button.as_string()),
        );
        table.insert(Ref::new_string(lua, "x"), self.x);
        table.insert(Ref::new_string(lua, "y"), self.y);
        table
    }
}

/// The parameter to mouse move events
pub struct MouseMove {
    /// The x position of the mouse
    pub x: i32,
    /// The y position of the mouse
    pub y: i32,
}

impl EventParam for MouseMove {
    fn as_lua_table(&self, lua: &lua::Lua) -> Ref<lua::Table> {
        let table = Ref::new_table(lua);
        table.insert(Ref::new_string(lua, "x"), self.x);
        table.insert(Ref::new_string(lua, "y"), self.y);
        table
    }
}

/// The parameter to mouse scroll events
pub struct MouseScroll {
    /// The x position of the mouse
    pub x: i32,
    /// The y position of the mouse
    pub y: i32,
    /// The amount the mouse wheel was scrolled by
    pub scroll_amount: i32,
}

impl EventParam for MouseScroll {
    fn as_lua_table(&self, lua: &lua::Lua) -> Ref<lua::Table> {
        let table = Ref::new_table(lua);
        table.insert(Ref::new_string(lua, "x"), self.x);
        table.insert(Ref::new_string(lua, "y"), self.y);
        table.insert(Ref::new_string(lua, "scroll_amount"), self.scroll_amount);
        table
    }
}

/// Event that is fired when the mouse moves
pub enum MouseMoveEvent {}

impl Event for MouseMoveEvent {
    type Param = MouseMove;

    fn event_funcs(data: &mut NodeEvents) -> &mut [MethodDesc<Self>] {
        &mut data.on_mouse_move
    }
}

/// Event that is fired when a mouse button is pressed
pub enum MouseDownEvent {}

impl Event for MouseDownEvent {
    type Param = MouseClick;

    fn event_funcs(data: &mut NodeEvents) -> &mut [MethodDesc<Self>] {
        &mut data.on_mouse_down
    }
}

/// Event that is fired when a mouse button is released
pub enum MouseUpEvent {}

impl Event for MouseUpEvent {
    type Param = MouseClick;

    fn event_funcs(data: &mut NodeEvents) -> &mut [MethodDesc<Self>] {
        &mut data.on_mouse_up
    }
}

/// Event that is fired when the mouse wheel is scrolled
pub enum MouseScrollEvent {}

impl Event for MouseScrollEvent {
    type Param = MouseScroll;

    fn event_funcs(data: &mut NodeEvents) -> &mut [MethodDesc<Self>] {
        &mut data.on_mouse_scroll
    }
}

/// Parameter to events that invoke a character being
/// input.
pub struct CharInput {
    /// The input character
    pub input: char,
}

impl EventParam for CharInput {
    fn as_lua_table(&self, lua: &lua::Lua) -> Ref<Table> {
        let tbl = Ref::new_table(lua);
        tbl.insert(Ref::new_string(lua, "input"), self.input as i32);
        tbl
    }
}

/// Event that is fired when character is input
pub enum CharInputEvent {}

impl Event for CharInputEvent {
    type Param = CharInput;

    fn event_funcs(data: &mut NodeEvents) -> &mut [MethodDesc<Self>] {
        &mut data.on_char_input
    }
}

/// Parameter to events that invoke a key being
/// pressed.
pub struct KeyInput {
    /// The input key
    pub input: Keycode,
}

impl EventParam for KeyInput {
    fn as_lua_table(&self, lua: &lua::Lua) -> Ref<Table> {
        let tbl = Ref::new_table(lua);
        tbl.insert(
            Ref::new_string(lua, "input"),
            Ref::new_string(lua, self.input.name()),
        );
        tbl
    }
}

/// Event that is fired when a key is pressed
pub enum KeyDownEvent {}

impl Event for KeyDownEvent {
    type Param = KeyInput;

    fn event_funcs(data: &mut NodeEvents) -> &mut [MethodDesc<Self>] {
        &mut data.on_key_down
    }
}

/// Event that is fired when a key is released
pub enum KeyUpEvent {}

impl Event for KeyUpEvent {
    type Param = KeyInput;

    fn event_funcs(data: &mut NodeEvents) -> &mut [MethodDesc<Self>] {
        &mut data.on_key_up
    }
}

/// Event that is fired when an element is focused/unfocused
pub enum FocusEvent {}

impl Event for FocusEvent {
    type Param = ();

    fn event_funcs(data: &mut NodeEvents) -> &mut [MethodDesc<Self>] {
        &mut data.on_focus
    }
}

/// Event that is fired when an element is created/removed
pub enum InitEvent {}

impl Event for InitEvent {
    type Param = ();

    fn event_funcs(data: &mut NodeEvents) -> &mut [MethodDesc<Self>] {
        &mut data.on_init
    }
}

/// Event that is fired every frame per an element
pub enum UpdateEvent {}

impl Event for UpdateEvent {
    type Param = f64;

    fn event_funcs(data: &mut NodeEvents) -> &mut [MethodDesc<Self>] {
        &mut data.on_update
    }
}

impl EventParam for () {
    fn as_lua_table(&self, lua: &lua::Lua) -> Ref<Table> {
        Ref::new_table(lua)
    }
}

impl EventParam for f64 {
    fn as_lua_table(&self, lua: &lua::Lua) -> Ref<Table> {
        let tbl = Ref::new_table(lua);
        tbl.insert(Ref::new_string(lua, "delta"), *self);
        tbl
    }
}

/// A parameter that can be passed to an event
pub trait EventParam {
    /// Turns this parameter into a table in lua
    fn as_lua_table(&self, lua: &lua::Lua) -> Ref<Table>;
}

/// An event that can be handled by an element
pub trait Event: Sized {
    /// The parameter to pass to the handler
    type Param: EventParam;

    /// Maps the node events structure to the specific event
    /// methods for this event
    fn event_funcs(data: &mut NodeEvents) -> &mut [MethodDesc<Self>];
}

/// A method call used by events
pub enum MethodDesc<E: Event> {
    /// Executes the given lua code
    Lua {
        /// The module + sub module to invoke the string in
        resource: assets::ResourceKey<'static>,
        /// The lua string to execute
        exec: String,
    },
    /// Executes the given lua code (precompiled)
    LuaCompiled {
        /// The precompiled lua function for this event
        func: Ref<Function>,
    },
    /// Native function call
    Native(Rc<dyn Fn(&mut event::Container, Node, &E::Param) -> bool>),
}

impl<E: Event> Clone for MethodDesc<E> {
    fn clone(&self) -> Self {
        match *self {
            MethodDesc::Lua {
                ref resource,
                ref exec,
            } => MethodDesc::Lua {
                resource: resource.clone(),
                exec: exec.clone(),
            },
            MethodDesc::LuaCompiled { ref func } => MethodDesc::LuaCompiled { func: func.clone() },
            MethodDesc::Native(ref func) => MethodDesc::Native(func.clone()),
        }
    }
}

impl<E: Event + 'static> MethodDesc<E> {
    fn from_value(val: fungui::Value<UniverCityUI>) -> Option<Vec<MethodDesc<E>>>
    where
        Vec<Self>: fungui::ConvertValue<UniverCityUI>,
    {
        if let Some(m) = val.convert_ref::<String>() {
            // TODO: Ideally this wouldn't default to base
            return Some(vec![Self::from_format(ModuleKey::new("base"), m)]);
        }
        if val.convert_ref::<Vec<Self>>().is_some() {
            return val.convert::<Vec<Self>>();
        }
        if val.convert_ref::<Vec<Value>>().is_some() {
            let vals = val
                .convert::<Vec<Value>>()
                .expect("Failed to convert untyped?");
            return Some(
                vals.into_iter()
                    .filter_map(Self::from_value)
                    .flat_map(|v| v)
                    .collect(),
            );
        }
        None
    }

    /// Wraps the passed function as a `MethodDesc`
    pub fn native<F>(f: F) -> MethodDesc<E>
    where
        E: Event,
        F: Fn(&mut event::Container, Node, &E::Param) -> bool + 'static,
    {
        MethodDesc::Native(Rc::new(f))
    }

    fn from_format(module: assets::ModuleKey<'_>, desc: &str) -> MethodDesc<E> {
        let func_start = desc.find('#').expect("Invalid method description");
        MethodDesc::Lua {
            resource: assets::LazyResourceKey::parse(&desc[..func_start])
                .or_module(module)
                .into_owned(),
            exec: desc[func_start + 1..].to_owned(),
        }
    }
}

fn invoke_event<E, F>(
    log: &Logger,
    events: &mut event::Container,
    engine: &script::Engine,
    node: &Node,
    get_actions: F,
    param: &E::Param,
) -> bool
where
    E: Event,
    F: FnOnce(&mut NodeEvents) -> &mut [MethodDesc<E>],
{
    let node_events = node.borrow().ext.events.clone();
    let mut node_events = node_events.borrow_mut();
    let actions = get_actions(&mut *node_events);

    if actions.is_empty() {
        return false;
    }
    let lua_elem = Ref::new(engine, NodeRef(node.clone()));
    let lua_param = param.as_lua_table(engine);
    let mut handled = false;

    for action in actions {
        let mut replace = None;
        handled |= match action {
            MethodDesc::Lua { resource, exec } => {
                let func = match engine.invoke_function::<(
                    // Resource
                    Ref<String>, Ref<String>,
                    // Exec
                    Ref<String>,
                ), Ref<Function>>(
                    "compile_ui_action",
                    (
                        Ref::new_string(engine, resource.module()),
                        Ref::new_string(engine, resource.resource()),
                        Ref::new_string(engine, exec.as_str()),
                    )
                ) {
                    Ok(ret) => ret,
                    Err(err) => {
                        error!(
                            log,
                            "Error compiling lua callback:\nReason: {}\nSource: \n{}", err, exec
                        );
                        return false;
                    }
                };
                let ret = match func.invoke((lua_elem.clone(), lua_param.clone())) {
                    Ok(ret) => ret,
                    Err(err) => {
                        error!(log, "Error invoking lua callback:\nReason: {}", err);
                        false
                    }
                };
                replace = Some(MethodDesc::LuaCompiled { func });
                ret
            }
            MethodDesc::LuaCompiled { func } => {
                match func.invoke((lua_elem.clone(), lua_param.clone())) {
                    Ok(ret) => ret,
                    Err(err) => {
                        error!(log, "Error invoking lua callback:\nReason: {}", err);
                        false
                    }
                }
            }
            MethodDesc::Native(func) => (func)(events, node.clone(), param),
        };
        if let Some(r) = replace {
            *action = r;
        }
    }
    handled
}

/// Sets up a interface for scripts to interface with the ui
pub fn init_uilib(state: &lua::Lua) {
    state.set(
        Scope::Global,
        "ui_root_query",
        lua::closure(|lua| -> UResult<Ref<QueryRef>> {
            let manager = lua
                .get_tracked::<SManager>()
                .ok_or_else(|| ErrorKind::UINotBound)?;
            let manager = manager.borrow();
            Ok(Ref::new(
                lua,
                QueryRef(RefCell::new(Some(manager.query().into_owned()))),
            ))
        }),
    );
    state.set(
        Scope::Global,
        "ui_add_node",
        lua::closure1(|lua, node: Ref<NodeRef>| -> UResult<()> {
            let manager = lua
                .get_tracked::<SManager>()
                .ok_or_else(|| ErrorKind::UINotBound)?;
            let mut manager = manager.borrow_mut();
            manager.add_node(node.0.clone());
            Ok(())
        }),
    );
    state.set(
        Scope::Global,
        "ui_remove_node",
        lua::closure1(|lua, node: Ref<NodeRef>| -> UResult<()> {
            let manager = lua
                .get_tracked::<SManager>()
                .ok_or_else(|| ErrorKind::UINotBound)?;
            let mut manager = manager.borrow_mut();
            manager.remove_node(node.0.clone());
            Ok(())
        }),
    );
    state.set(
        Scope::Global,
        "ui_load_node",
        lua::closure1(|lua, key: Ref<String>| -> UResult<_> {
            let assets = lua
                .get_tracked::<AssetManager>()
                .ok_or_else(|| ErrorKind::InvalidState)?;
            let log = lua
                .get_tracked::<Logger>()
                .ok_or_else(|| ErrorKind::InvalidState)?;

            let key = assets::LazyResourceKey::parse(&key).or_module(ModuleKey::new("base"));

            let node = Manager::create_node_impl(&log, &assets, key)?;
            Ok(Ref::new(lua, NodeRef(node)))
        }),
    );
    state.set(
        Scope::Global,
        "ui_new_node",
        lua::closure1(|lua, name: Ref<String>| -> UResult<Ref<NodeRef>> {
            Ok(Ref::new(lua, NodeRef(Node::new(&*name))))
        }),
    );
    state.set(
        Scope::Global,
        "ui_new_text_node",
        lua::closure1(|lua, value: Ref<String>| -> UResult<Ref<NodeRef>> {
            Ok(Ref::new(lua, NodeRef(Node::new_text(&*value))))
        }),
    );
    state.set(
        Scope::Global,
        "ui_new_node_str",
        lua::closure1(|lua, s: Ref<String>| -> UResult<Ref<NodeRef>> {
            Ok(Ref::new(
                lua,
                NodeRef(Node::from_str(&*s).map_err(|e| ErrorKind::Msg(e.to_string()))?),
            ))
        }),
    );

    state.set(
        Scope::Global,
        "ui_show_tooltip",
        lua::closure4(
            |lua, key: Ref<String>, content: Ref<NodeRef>, x: i32, y: i32| -> UResult<()> {
                let tooltip = lua
                    .get_tracked::<Tooltip>()
                    .ok_or_else(|| ErrorKind::UINotBound)?;
                let manager = lua
                    .get_tracked::<SManager>()
                    .ok_or_else(|| ErrorKind::UINotBound)?;

                let mut tooltip = tooltip.borrow_mut();
                let mut manager = manager.borrow_mut();
                Manager::show_tooltip_impl(
                    &mut *manager,
                    &mut *tooltip,
                    &*key,
                    content.0.clone(),
                    x,
                    y,
                );

                Ok(())
            },
        ),
    );
    state.set(
        Scope::Global,
        "ui_move_tooltip",
        lua::closure3(|lua, key: Ref<String>, x: i32, y: i32| -> UResult<()> {
            let tooltip = lua
                .get_tracked::<Tooltip>()
                .ok_or_else(|| ErrorKind::UINotBound)?;
            let tooltip = tooltip.borrow();
            Manager::move_tooltip_impl(&*tooltip, &*key, x, y);

            Ok(())
        }),
    );
    state.set(
        Scope::Global,
        "ui_hide_tooltip",
        lua::closure1(|lua, key: Ref<String>| -> UResult<()> {
            let tooltip = lua
                .get_tracked::<Tooltip>()
                .ok_or_else(|| ErrorKind::UINotBound)?;
            let manager = lua
                .get_tracked::<SManager>()
                .ok_or_else(|| ErrorKind::UINotBound)?;

            let mut tooltip = tooltip.borrow_mut();
            let mut manager = manager.borrow_mut();

            Manager::hide_tooltip_impl(&mut *manager, &mut *tooltip, &*key);
            Ok(())
        }),
    );

    // Event handling
    state.set(
        Scope::Global,
        "ui_emit_0",
        lua::closure1(|lua, evt: Ref<String>| -> UResult<()> {
            let events = lua
                .get_tracked::<Events>()
                .ok_or_else(|| ErrorKind::UINotBound)?;
            let mut events = events.borrow_mut();
            match &evt[..] {
                "exit_game" => events.emit(crate::ExitGame),
                _ => bail!("unknown event type {:?}", evt),
            }
            Ok(())
        }),
    );
    state.set(
        Scope::Global,
        "ui_emit_1",
        lua::closure2(|lua, evt: Ref<String>, p1: Ref<String>| -> UResult<()> {
            let events = lua
                .get_tracked::<Events>()
                .ok_or_else(|| ErrorKind::UINotBound)?;
            let mut events = events.borrow_mut();
            match &evt[..] {
                "switch_menu" => events.emit(crate::SwitchMenu(p1.to_string())),
                "set_cursor" => events.emit(crate::SetCursor(
                    LazyResourceKey::parse(&p1)
                        .or_module(ModuleKey::new("base"))
                        .into_owned(),
                )),
                "open" => match &p1[..] {
                    "room_buy" => events.emit(crate::instance::OpenBuyRoomMenu),
                    "staff_buy" => events.emit(crate::instance::OpenBuyStaffMenu),
                    "staff_list" => events.emit(crate::instance::OpenStaffListMenu),
                    "stats" => events.emit(crate::instance::OpenStatsMenu),
                    "settings" => events.emit(crate::instance::OpenSettingsMenu),
                    "courses" => events.emit(crate::instance::OpenCoursesMenu),
                    "edit_room" => events.emit(crate::instance::OpenEditRoom),
                    _ => bail!("unknown window type {:?}", evt),
                },
                "multi_player" => events.emit(crate::MultiPlayer(p1.to_string())),
                _ => bail!("unknown event type {:?}", evt),
            }
            Ok(())
        }),
    );

    state.set(
        Scope::Global,
        "ui_emit_accept",
        lua::closure1(|lua, node: Ref<NodeRef>| -> UResult<()> {
            let events = lua
                .get_tracked::<Events>()
                .ok_or_else(|| ErrorKind::UINotBound)?;
            let mut events = events.borrow_mut();
            let mut base = node.0.clone();
            loop {
                if let Some(parent) = base.parent() {
                    if parent.name().map_or(false, |v| v == "root") {
                        break;
                    }
                    base = parent;
                } else {
                    bail!("Missing node parent")
                }
            }
            events.emit(crate::instance::AcceptEvent(base));
            Ok(())
        }),
    );
    state.set(
        Scope::Global,
        "ui_emit_cancel",
        lua::closure1(|lua, node: Ref<NodeRef>| -> UResult<()> {
            let events = lua
                .get_tracked::<Events>()
                .ok_or_else(|| ErrorKind::UINotBound)?;
            let mut events = events.borrow_mut();
            let mut base = node.0.clone();
            loop {
                if let Some(parent) = base.parent() {
                    if parent.name().map_or(false, |v| v == "root") {
                        break;
                    }
                    base = parent;
                } else {
                    bail!("Missing node parent")
                }
            }
            events.emit(crate::instance::CancelEvent(base));
            Ok(())
        }),
    );
}

impl fungui::ConvertValue<UniverCityUI> for UValue {
    type RefType = UValue;

    fn from_value(v: Value) -> Option<Self> {
        match v {
            fungui::Value::ExtValue(e) => Some(e),
            _ => None,
        }
    }
    fn from_value_ref(v: &Value) -> Option<&Self::RefType> {
        match v {
            fungui::Value::ExtValue(e) => Some(e),
            _ => None,
        }
    }
    fn to_value(v: Self) -> Value {
        fungui::Value::ExtValue(v)
    }
}

/// The UniverCity fungui value extension
#[derive(Clone)]
pub enum UValue {
    /// Event handlers
    MethodCharInput(Vec<MethodDesc<CharInputEvent>>),
    /// Event handlers
    MethodKeyUp(Vec<MethodDesc<KeyUpEvent>>),
    /// Event handlers
    MethodKeyDown(Vec<MethodDesc<KeyDownEvent>>),
    /// Event handlers
    MethodMouseDown(Vec<MethodDesc<MouseDownEvent>>),
    /// Event handlers
    MethodMouseUp(Vec<MethodDesc<MouseUpEvent>>),
    /// Event handlers
    MethodMouseMove(Vec<MethodDesc<MouseMoveEvent>>),
    /// Event handlers
    MethodMouseScroll(Vec<MethodDesc<MouseScrollEvent>>),
    /// Event handlers
    MethodFocus(Vec<MethodDesc<FocusEvent>>),
    /// Event handlers
    MethodInit(Vec<MethodDesc<InitEvent>>),
    /// Event handlers
    MethodUpdate(Vec<MethodDesc<UpdateEvent>>),

    /// An uptyped list of univercity values
    UntypedList(Vec<Value>),

    /// A lua table
    LuaTable(LuaTable),
    /// An rgba color
    Color(Color),
    /// A text shadow
    TextShadow(render::ui::text_shadow::TShadow),
    /// A box shadow
    Shadow(Vec<render::ui::shadow::Shadow>),
    /// A border or border image
    Border(render::ui::border::Border),
    /// The widths of any border
    BorderWidth(render::ui::border::BorderWidthInfo),
    /// A side of a border
    BorderSide(render::ui::border::BorderSide),
}

impl PartialEq for UValue {
    fn eq(&self, _other: &UValue) -> bool {
        false
    }
}

impl fungui::ConvertValue<UniverCityUI> for LuaTable {
    type RefType = Ref<lua::Table>;

    fn from_value(v: Value) -> Option<Self> {
        if let fungui::Value::ExtValue(UValue::LuaTable(v)) = v {
            Some(v)
        } else {
            None
        }
    }
    fn from_value_ref(v: &Value) -> Option<&Self::RefType> {
        if let fungui::Value::ExtValue(UValue::LuaTable(v)) = v {
            Some(&v.0)
        } else {
            None
        }
    }
    fn to_value(v: Self) -> Value {
        fungui::Value::ExtValue(UValue::LuaTable(v))
    }
}

macro_rules! value_method {
    ($evt:ty, $name:ident) => {
        impl fungui::ConvertValue<UniverCityUI> for MethodDesc<$evt> {
            type RefType = [MethodDesc<$evt>];

            fn from_value(_v: Value) -> Option<Self> {
                None
            }
            fn from_value_ref(_v: &Value) -> Option<&Self::RefType> {
                None
            }
            fn to_value(v: Self) -> Value {
                fungui::Value::ExtValue(UValue::$name(vec![v]))
            }
        }

        impl fungui::ConvertValue<UniverCityUI> for Vec<MethodDesc<$evt>> {
            type RefType = [MethodDesc<$evt>];

            fn from_value(v: Value) -> Option<Self> {
                if let fungui::Value::ExtValue(UValue::$name(v)) = v {
                    Some(v)
                } else {
                    None
                }
            }
            fn from_value_ref(v: &Value) -> Option<&Self::RefType> {
                if let fungui::Value::ExtValue(UValue::$name(v)) = v {
                    Some(v)
                } else {
                    None
                }
            }
            fn to_value(v: Self) -> Value {
                fungui::Value::ExtValue(UValue::$name(v))
            }
        }
    };
}

impl fungui::ConvertValue<UniverCityUI> for Vec<Value> {
    type RefType = [Value];

    fn from_value(v: Value) -> Option<Self> {
        if let fungui::Value::ExtValue(UValue::UntypedList(v)) = v {
            Some(v)
        } else {
            None
        }
    }
    fn from_value_ref(v: &Value) -> Option<&Self::RefType> {
        if let fungui::Value::ExtValue(UValue::UntypedList(v)) = v {
            Some(v)
        } else {
            None
        }
    }
    fn to_value(v: Self) -> Value {
        fungui::Value::ExtValue(UValue::UntypedList(v))
    }
}

value_method!(CharInputEvent, MethodCharInput);
value_method!(KeyUpEvent, MethodKeyUp);
value_method!(KeyDownEvent, MethodKeyDown);
value_method!(MouseDownEvent, MethodMouseDown);
value_method!(MouseUpEvent, MethodMouseUp);
value_method!(MouseMoveEvent, MethodMouseMove);
value_method!(MouseScrollEvent, MethodMouseScroll);
value_method!(FocusEvent, MethodFocus);
value_method!(InitEvent, MethodInit);
value_method!(UpdateEvent, MethodUpdate);
