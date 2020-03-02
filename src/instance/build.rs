use std::sync::mpsc;

use super::*;
use crate::entity;
use crate::keybinds;
use crate::render;
use crate::server::assets;
use crate::server::command;
use crate::server::event;
use crate::server::level::object::{self, PlacementStyle};
use crate::server::level::tile;
use crate::server::player::State;
use crate::state;
use crate::ui;
use crate::util::*;

use cgmath::Vector3;

const PAN_SPEED: f32 = 0.04;
const EDGE_DISTANCE: i32 = 50;

/// An area currently being selected by the player.
#[derive(Clone)]
struct Selection {
    /// The start location of the selection
    pub start: Location,
    /// The end location of the selection
    pub end: Location,
    tooltip: ui::Node,
}

bitflags! {
    struct PanEdge: u8 {
        const LEFT  = 0b0001;
        const RIGHT = 0b0010;
        const UP    = 0b0100;
        const DOWN  = 0b1000;
    }
}

/// The state where the player is selecting an area for the
/// room.
///
/// This state ends when the player selects a valid area.
pub struct BuildState {
    room: assets::ResourceKey<'static>,
    selection: Option<Selection>,
    ui: Option<ui::Node>,
    pan_edge: PanEdge,
    last_mouse: (i32, i32),
}

impl BuildState {
    pub fn new(room: assets::ResourceKey<'_>) -> BuildState {
        BuildState {
            room: room.into_owned(),
            selection: None,
            ui: None,
            pan_edge: PanEdge::empty(),
            last_mouse: (0, 0),
        }
    }
}

/// Attempts to preload all textures the room could use
/// excluding models.
///
/// This does not cover the case where rooms use scripts
/// to style the floor.
fn preload_room_textures(
    renderer: &mut render::Renderer,
    level: &mut Level,
    room: assets::ResourceKey<'_>,
) {
    use std::iter;
    let room = assume!(
        renderer.log,
        level.asset_manager.loader_open::<room::Loader>(room)
    );

    // The floor tile and border if the room
    // has one.
    let tiles = iter::once(room.tile.borrow()).chain(room.border_tile.as_ref().map(|v| v.borrow()));

    for tile in tiles {
        let tile = assume!(
            renderer.log,
            level
                .asset_manager
                .loader_open::<tile::Loader>(tile.borrow())
        );
        for tex in tile.get_possible_textures() {
            renderer.preload_texture(tex.borrow());
        }
    }

    if let Some(wall) = room.wall.as_ref() {
        renderer.preload_texture(wall.texture.borrow());
        wall.texture_top
            .as_ref()
            .map(|v| renderer.preload_texture(v.borrow()));
    }
}

impl state::State for BuildState {
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(BuildState {
            room: self.room.clone(),
            selection: self.selection.clone(),
            ui: self.ui.clone(),
            pan_edge: self.pan_edge,
            last_mouse: self.last_mouse,
        })
    }

    fn active(
        &mut self,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
    ) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());
        // Bind placement controls
        state
            .keybinds
            .add_collection(keybinds::KeyCollection::RoomPlacement);
        state.renderer.cursor_visible = true;

        // Spawn the room placement ui so that
        // placement can be canceled.
        let ui = state
            .ui_manager
            .create_node(assets::ResourceKey::new("base", "room/select_room"));
        self.ui = Some(ui.clone());

        // Update the room name text on the ui
        let room = assume!(
            state.global_logger,
            instance
                .level
                .asset_manager
                .loader_open::<room::Loader>(self.room.borrow())
        );
        if let Some(txt) = query!(ui, room_name > @text).next() {
            txt.set_text(room.name.as_str());
        }
        if let Some(txt) = query!(ui, requirements > @text).next() {
            txt.set_text(format!(
                "Requires at least {x} by {y} tiles of space",
                x = room.min_size.0,
                y = room.min_size.1
            ));
        }
        if let Some(price_tag) = query!(ui, price_tag).next() {
            let cost = room.base_cost.unwrap_or(UniDollar(0));
            price_tag.set_property("can_afford", instance.player.get_money() >= cost);
            if let Some(txt) = query!(price_tag, @text).next() {
                txt.set_text(format!("Cost: {}", cost));
            }
        }

        // Help prevent flickering when the room is placed
        // by loading textures beforehand
        preload_room_textures(&mut state.renderer, &mut instance.level, self.room.borrow());

        state
            .renderer
            .set_lowered_region(instance.level.level_bounds);
        instance.level.tiles.borrow_mut().flag_all_dirty();

        state::Action::Nothing
    }

    fn tick(
        &mut self,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
    ) -> state::Action {
        let mut x = 0.0;
        let mut y = 0.0;
        if self.pan_edge.contains(PanEdge::LEFT) {
            x = state.delta as f32 * PAN_SPEED;
        } else if self.pan_edge.contains(PanEdge::RIGHT) {
            x = -state.delta as f32 * PAN_SPEED;
        }
        if self.pan_edge.contains(PanEdge::UP) {
            y = state.delta as f32 * PAN_SPEED;
        } else if self.pan_edge.contains(PanEdge::DOWN) {
            y = -state.delta as f32 * PAN_SPEED;
        }
        state.renderer.move_camera(x, y);
        if !self.pan_edge.is_empty() {
            let mpos = self.last_mouse;
            self.mouse_move(instance, state, mpos);
        }

        let instance = assume!(state.global_logger, instance.as_mut());

        if !instance.player.state.is_none() {
            // Can't place a room whilst one is already in progress
            state::Action::Pop
        } else {
            state::Action::Nothing
        }
    }

    fn inactive(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) {
        let instance = assume!(state.global_logger, instance.as_mut());
        // Close the placement ui
        if let Some(ui) = self.ui.take() {
            state.ui_manager.remove_node(ui);
        }

        // Revert the cursor and keybinds back to normal
        state.renderer.cursor_visible = false;
        state
            .renderer
            .set_mouse_sprite(ResourceKey::new("base", "ui/cursor/normal"));
        state
            .keybinds
            .remove_collection(keybinds::KeyCollection::RoomPlacement);
        state.renderer.clear_lowered_region();
        instance.level.tiles.borrow_mut().flag_all_dirty();

        // End any active selections
        if let Some(sel) = self.selection.take() {
            state
                .renderer
                .stop_selection(&mut instance.level, sel.end.x, sel.end.y);
        }
    }

    fn ui_event(
        &mut self,
        _instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
        evt: &mut event::EventHandler,
    ) -> state::Action {
        let mut action = state::Action::Nothing;

        let ui = assume!(state.global_logger, self.ui.clone());
        // Cancel button
        evt.handle_event_if::<super::CancelEvent, _, _>(
            |evt| evt.0.is_same(&ui),
            |_| {
                action = state::Action::Pop;
            },
        );

        action
    }

    fn mouse_move(
        &mut self,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
        mouse_pos: (i32, i32),
    ) -> state::Action {
        self.last_mouse = mouse_pos;
        let instance = assume!(state.global_logger, instance.as_mut());
        let (lx, ly) = state.renderer.mouse_to_level(mouse_pos.0, mouse_pos.1);

        // If we are currently selecting update the position using
        // the mouse's current position. (Snapped to tiles)
        let mouse_loc = Location::new(lx.floor() as i32, ly.floor() as i32);
        self.pan_edge = PanEdge::empty();
        if let Some(sel) = self.selection.as_mut() {
            sel.end = mouse_loc;
            state.renderer.move_selection(
                instance.player.id,
                &mut instance.level,
                self.room.borrow(),
                sel.end.x,
                sel.end.y,
            );
            state
                .ui_manager
                .move_tooltip("current_size", mouse_pos.0, mouse_pos.1);

            // Pan the camera for the player if they are near to the edge of the screen
            if mouse_pos.0 <= EDGE_DISTANCE {
                self.pan_edge |= PanEdge::LEFT;
            } else if mouse_pos.0 >= state.renderer.width as i32 - EDGE_DISTANCE {
                self.pan_edge |= PanEdge::RIGHT;
            }
            if mouse_pos.1 <= EDGE_DISTANCE {
                self.pan_edge |= PanEdge::UP;
            } else if mouse_pos.1 >= state.renderer.height as i32 - EDGE_DISTANCE {
                self.pan_edge |= PanEdge::DOWN;
            }

            let area = Bound::new(sel.start, sel.end);
            sel.tooltip
                .set_text(format!("{}x{}", area.width(), area.height()));

            let room = assume!(
                state.global_logger,
                instance
                    .level
                    .asset_manager
                    .loader_open::<room::Loader>(self.room.borrow())
            );
            if let Some(price_tag) =
                query!(assume!(state.global_logger, self.ui.as_ref()), price_tag).next()
            {
                let cost = room.cost_for_area(area);
                price_tag.set_property("can_afford", instance.player.get_money() >= cost);
                if let Some(txt) = query!(price_tag, @text).next() {
                    txt.set_text(format!("Cost: {}", cost));
                }
            }
        }

        state::Action::Nothing
    }

    fn key_action_req(
        &mut self,
        req: &mut state::CaptureRequester,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
        action: keybinds::KeyAction,
        mouse_pos: (i32, i32),
    ) -> state::Action {
        use crate::keybinds::KeyAction::*;

        let instance = assume!(state.global_logger, instance.as_mut());

        match action {
            RoomStartAreaSelect => {
                // If we aren't currently selecting an area
                // begin a selection.
                if self.selection.is_none() {
                    let (lx, ly) = state.renderer.mouse_to_level(mouse_pos.0, mouse_pos.1);
                    let (lx, ly) = (lx.floor() as i32, ly.floor() as i32);

                    let content = node! {
                        content {
                            @text("1x1")
                        }
                    };
                    let text = assume!(state.global_logger, query!(content, @text).next());
                    state.ui_manager.show_tooltip(
                        "current_size",
                        content,
                        mouse_pos.0,
                        mouse_pos.1,
                    );
                    let sel = Selection {
                        start: Location::new(lx, ly),
                        end: Location::new(lx, ly),
                        tooltip: text,
                    };
                    // Being rendering the selection grid and set the
                    // cursor to notify the player
                    state.renderer.start_selection(
                        instance.player.id,
                        &mut instance.level,
                        self.room.borrow(),
                        sel.start.x,
                        sel.start.y,
                    );
                    state
                        .renderer
                        .set_mouse_sprite(ResourceKey::new("base", "ui/cursor/selection"));
                    self.selection = Some(sel);
                }
            }

            RoomFinishAreaSelect => {
                // If there is a selection in progress end it
                // and take the selected area.
                if let Some(sel) = self.selection.take() {
                    state.ui_manager.hide_tooltip("current_size");
                    let (lx, ly) = state.renderer.mouse_to_level(mouse_pos.0, mouse_pos.1);
                    let (lx, ly) = (lx.floor() as i32, ly.floor() as i32);

                    // Stop rendering the current selection now as
                    // it needs to stop whether the selection is
                    // successful or not.
                    state
                        .renderer
                        .stop_selection(&mut instance.level, sel.end.x, sel.end.y);

                    // Attempt to place a room with the given bounds
                    let mut cmd: command::Command = command::PlaceSelection::new(
                        self.room.borrow(),
                        sel.start,
                        Location::new(lx, ly),
                    )
                    .into();
                    let mut proxy = super::GameProxy::proxy(state);

                    try_cmd!(
                        instance.log,
                        cmd.execute(
                            &mut proxy,
                            &mut instance.player,
                            command::CommandParams {
                                log: &instance.log,
                                level: &mut instance.level,
                                engine: &instance.scripting,
                                entities: &mut instance.entities,
                                snapshots: &instance.snapshots,
                                mission_handler: instance
                                    .mission_handler
                                    .as_ref()
                                    .map(|v| v.borrow()),
                            }
                        ),
                        {
                            // Success, preform the same command on the
                            // the server and move to the next screen
                            instance.push_command(cmd, req);
                            return state::Action::Switch(Box::new(FinalizePlacement::new()));
                        }
                    );
                }
            }
            _ => {}
        }
        state::Action::Nothing
    }
}

bitflags! {
    struct ResizeEdges: u8 {
        const MIN_X = 0b0001;
        const MIN_Y = 0b0010;
        const MAX_X = 0b0100;
        const MAX_Y = 0b1000;
    }
}

/// Allows for resizing the room before
/// placing it
pub struct FinalizePlacement {
    resize_edges: Option<ResizeEdges>,
    ui: Option<ui::Node>,
    pan_edge: PanEdge,
    last_mouse: (i32, i32),
    last_bounds_try: Bound,
}

impl FinalizePlacement {
    pub fn new() -> FinalizePlacement {
        FinalizePlacement {
            resize_edges: None,
            ui: None,
            pan_edge: PanEdge::empty(),
            last_mouse: (0, 0),
            last_bounds_try: Bound::new(Location::zero(), Location::zero()),
        }
    }
}

impl state::State for FinalizePlacement {
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(FinalizePlacement {
            resize_edges: self.resize_edges,
            ui: self.ui.clone(),
            pan_edge: self.pan_edge,
            last_mouse: self.last_mouse,
            last_bounds_try: self.last_bounds_try,
        })
    }

    fn active(
        &mut self,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
    ) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());

        // Ensure the player has an active room.
        // It shouldn't be possible to be in this state
        // without one.
        let room_id = match instance.player.state {
            State::BuildRoom { active_room } => active_room,
            _ => panic!("Player is in the incorrect state"),
        };

        // Update the cursor and spawn the planning ui
        state
            .renderer
            .set_mouse_sprite(ResourceKey::new("base", "ui/cursor/question"));

        let ui = state
            .ui_manager
            .create_node(assets::ResourceKey::new("base", "room/plan_room"));
        self.ui = Some(ui.clone());

        // Update the room name text on the ui
        if let Some(txt) = query!(ui, room_name > @text).next() {
            let key = {
                let info = instance.level.get_room_info(room_id);
                info.key.clone()
            };
            let room = assume!(
                state.global_logger,
                instance
                    .level
                    .asset_manager
                    .loader_open::<room::Loader>(key)
            );
            txt.set_text(room.name.clone());
        }

        let room = instance.level.get_room_info(room_id);
        let room_info = assume!(
            state.global_logger,
            instance
                .asset_manager
                .loader_open::<room::Loader>(room.key.borrow())
        );

        if let Some(price_tag) = query!(ui, price_tag).next() {
            let cost = room_info.cost_for_area(room.area) - room.placement_cost;
            let cost = if cost < UniDollar(0) {
                UniDollar(0)
            } else {
                cost
            };
            price_tag.set_property("can_afford", instance.player.get_money() >= cost);
            if let Some(txt) = query!(price_tag, @text).next() {
                txt.set_text(format!("Cost: {}", cost));
            }
        }
        state
            .renderer
            .set_lowered_region(instance.level.level_bounds);
        instance.level.tiles.borrow_mut().flag_all_dirty();

        state::Action::Nothing
    }

    fn inactive(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) {
        let instance = assume!(state.global_logger, instance.as_mut());
        // Close the active ui window and reset the cursor
        if let Some(cid) = self.ui.take() {
            state.ui_manager.remove_node(cid);
        }
        state
            .renderer
            .set_mouse_sprite(ResourceKey::new("base", "ui/cursor/normal"));
        state.renderer.clear_lowered_region();
        instance.level.tiles.borrow_mut().flag_all_dirty();
        // If the resize keybinds are bound remove
        // them.
        state
            .keybinds
            .remove_collection(keybinds::KeyCollection::RoomResize);
    }

    fn tick_req(
        &mut self,
        req: &mut state::CaptureRequester,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
    ) -> state::Action {
        let mut x = 0.0;
        let mut y = 0.0;
        if self.pan_edge.contains(PanEdge::LEFT) {
            x = state.delta as f32 * PAN_SPEED;
        } else if self.pan_edge.contains(PanEdge::RIGHT) {
            x = -state.delta as f32 * PAN_SPEED;
        }
        if self.pan_edge.contains(PanEdge::UP) {
            y = state.delta as f32 * PAN_SPEED;
        } else if self.pan_edge.contains(PanEdge::DOWN) {
            y = -state.delta as f32 * PAN_SPEED;
        }
        state.renderer.move_camera(x, y);
        if !self.pan_edge.is_empty() {
            let mpos = self.last_mouse;
            self.mouse_move_req(req, instance, state, mpos);
        }

        state::Action::Nothing
    }

    fn mouse_move_req(
        &mut self,
        req: &mut state::CaptureRequester,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
        mouse_pos: (i32, i32),
    ) -> state::Action {
        self.last_mouse = mouse_pos;
        let instance = assume!(state.global_logger, instance.as_mut());

        let room = match instance.player.state {
            State::BuildRoom { active_room } => active_room,
            _ => panic!("Player is in the incorrect state"),
        };

        let (lx, ly) = state.renderer.mouse_to_level(mouse_pos.0, mouse_pos.1);
        let mut mouse_loc = Location::new(lx.floor() as i32, ly.floor() as i32);

        let bounds = instance.level.get_room_info(room).area;
        self.pan_edge = PanEdge::empty();
        if let Some(edges) = self.resize_edges {
            // Pan the camera for the player if they are near to the edge of the screen
            if mouse_pos.0 <= EDGE_DISTANCE {
                self.pan_edge |= PanEdge::LEFT;
            } else if mouse_pos.0 >= state.renderer.width as i32 - EDGE_DISTANCE {
                self.pan_edge |= PanEdge::RIGHT;
            }
            if mouse_pos.1 <= EDGE_DISTANCE {
                self.pan_edge |= PanEdge::UP;
            } else if mouse_pos.1 >= state.renderer.height as i32 - EDGE_DISTANCE {
                self.pan_edge |= PanEdge::DOWN;
            }

            // Update the bounds based on the currnetyly selected
            // edges
            let mut new_bounds = bounds;
            if edges.contains(ResizeEdges::MIN_X) {
                new_bounds.min.x = mouse_loc.x;
            } else if edges.contains(ResizeEdges::MAX_X) {
                new_bounds.max.x = mouse_loc.x;
            }
            if edges.contains(ResizeEdges::MIN_Y) {
                new_bounds.min.y = mouse_loc.y;
            } else if edges.contains(ResizeEdges::MAX_Y) {
                new_bounds.max.y = mouse_loc.y;
            }

            // Save bandwidth by only updating the server
            // when the size changes
            if bounds != new_bounds && new_bounds != self.last_bounds_try {
                self.last_bounds_try = new_bounds;
                let mut cmd: command::Command = command::ResizeRoom::new(new_bounds).into();
                let mut proxy = super::GameProxy::proxy(state);
                try_cmd!(
                    instance.log,
                    cmd.execute(
                        &mut proxy,
                        &mut instance.player,
                        command::CommandParams {
                            log: &instance.log,
                            level: &mut instance.level,
                            engine: &instance.scripting,
                            entities: &mut instance.entities,
                            snapshots: &instance.snapshots,
                            mission_handler: instance.mission_handler.as_ref().map(|v| v.borrow()),
                        }
                    ),
                    {
                        instance.push_command(cmd, req);

                        let room = instance.level.get_room_info(room);
                        let room_info = assume!(
                            proxy.state.global_logger,
                            instance
                                .asset_manager
                                .loader_open::<room::Loader>(room.key.borrow())
                        );
                        if let Some(price_tag) = query!(
                            assume!(proxy.state.global_logger, self.ui.as_ref()),
                            price_tag
                        )
                        .next()
                        {
                            let cost = room_info.cost_for_area(room.area) - room.placement_cost;
                            let cost = if cost < UniDollar(0) {
                                UniDollar(0)
                            } else {
                                cost
                            };
                            price_tag
                                .set_property("can_afford", instance.player.get_money() >= cost);
                            if let Some(txt) = query!(price_tag, @text).next() {
                                txt.set_text(format!("Cost: {}", cost));
                            }
                        }
                    }
                );
            }
        } else {
            let room = instance.level.get_room_info(room);
            let ray = state.renderer.get_mouse_ray(mouse_pos.0, mouse_pos.1);

            if let Some((loc, dir)) = room
                .building_level
                .as_ref()
                .and_then(|v| intersect_wall(v, ray, (lx, ly)))
            {
                if bounds.in_bounds(loc) {
                    mouse_loc = loc;
                }
                let shifted = loc.shift(dir);
                if bounds.in_bounds(shifted) {
                    mouse_loc = shifted;
                }
            }
            // Change the cursor on grabbable edges
            let inner_bounds = bounds.inset(1);
            state
                .keybinds
                .remove_collection(keybinds::KeyCollection::RoomResize);
            if bounds.in_bounds(mouse_loc) && !inner_bounds.in_bounds(mouse_loc) {
                state
                    .renderer
                    .set_mouse_sprite(ResourceKey::new("base", "ui/cursor/resize"));
                state
                    .keybinds
                    .add_collection(keybinds::KeyCollection::RoomResize);
            } else {
                state
                    .renderer
                    .set_mouse_sprite(ResourceKey::new("base", "ui/cursor/question"));
            }
        }

        state::Action::Nothing
    }

    fn key_action(
        &mut self,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
        action: keybinds::KeyAction,
        mouse_pos: (i32, i32),
    ) -> state::Action {
        use crate::keybinds::KeyAction::*;
        let instance = assume!(state.global_logger, instance.as_mut());
        match action {
            RoomStartRoomResize => {
                // Skip if already resizing
                if self.resize_edges.is_some() {
                    return state::Action::Nothing;
                }
                state
                    .renderer
                    .set_mouse_sprite(ResourceKey::new("base", "ui/cursor/resize_active"));

                let room = if let State::BuildRoom { active_room } = instance.player.state {
                    active_room
                } else {
                    return state::Action::Nothing;
                };

                let (lx, ly) = state.renderer.mouse_to_level(mouse_pos.0, mouse_pos.1);
                let ray = state.renderer.get_mouse_ray(mouse_pos.0, mouse_pos.1);
                let mut mouse_loc = Location::new(lx.floor() as i32, ly.floor() as i32);
                let room = instance.level.get_room_info(room);
                let bounds = room.area;

                if let Some((loc, dir)) = room
                    .building_level
                    .as_ref()
                    .and_then(|v| intersect_wall(v, ray, (lx, ly)))
                {
                    if bounds.in_bounds(loc) {
                        mouse_loc = loc;
                    }
                    let shifted = loc.shift(dir);
                    if bounds.in_bounds(shifted) {
                        mouse_loc = shifted;
                    }
                }

                // Work out the edges being clicked on
                let mut edges = ResizeEdges::empty();
                if mouse_loc.x == bounds.min.x {
                    edges.insert(ResizeEdges::MIN_X);
                }
                if mouse_loc.y == bounds.min.y {
                    edges.insert(ResizeEdges::MIN_Y);
                }
                if mouse_loc.x == bounds.max.x {
                    edges.insert(ResizeEdges::MAX_X);
                }
                if mouse_loc.y == bounds.max.y {
                    edges.insert(ResizeEdges::MAX_Y);
                }

                if !edges.is_empty() {
                    self.resize_edges = Some(edges);
                }
            }

            RoomFinishRoomResize => {
                state
                    .renderer
                    .set_mouse_sprite(ResourceKey::new("base", "ui/cursor/resize"));
                self.resize_edges = None;
            }
            _ => {}
        }
        state::Action::Nothing
    }

    fn ui_event_req(
        &mut self,
        req: &mut state::CaptureRequester,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
        evt: &mut event::EventHandler,
    ) -> state::Action {
        let mut action = state::Action::Nothing;
        let instance = assume!(state.global_logger, instance.as_mut());
        let ui = assume!(state.global_logger, self.ui.clone());

        // Cancel placement
        evt.handle_event_if::<super::CancelEvent, _, _>(
            |evt| evt.0.is_same(&ui),
            |_| {
                let mut cmd: command::Command = command::CancelRoomPlacement::default().into();
                let mut proxy = super::GameProxy::proxy(state);
                try_cmd!(
                    instance.log,
                    cmd.execute(
                        &mut proxy,
                        &mut instance.player,
                        command::CommandParams {
                            log: &instance.log,
                            level: &mut instance.level,
                            engine: &instance.scripting,
                            entities: &mut instance.entities,
                            snapshots: &instance.snapshots,
                            mission_handler: instance.mission_handler.as_ref().map(|v| v.borrow()),
                        }
                    ),
                    {
                        instance.push_command(cmd, req);
                        action = state::Action::Pop;
                    }
                );
            },
        );

        // Attempt placement and continue to the next state
        evt.handle_event_if::<super::AcceptEvent, _, _>(
            |evt| evt.0.is_same(&ui),
            |_| {
                let mut cmd: command::Command = command::FinalizeRoomPlacement::default().into();
                let mut proxy = super::GameProxy::proxy(state);
                try_cmd!(
                    instance.log,
                    cmd.execute(
                        &mut proxy,
                        &mut instance.player,
                        command::CommandParams {
                            log: &instance.log,
                            level: &mut instance.level,
                            engine: &instance.scripting,
                            entities: &mut instance.entities,
                            snapshots: &instance.snapshots,
                            mission_handler: instance.mission_handler.as_ref().map(|v| v.borrow()),
                        }
                    ),
                    {
                        instance.push_command(cmd, req);
                        action = state::Action::Switch(Box::new(BuildRoom::new(false)));
                    }
                );
            },
        );
        action
    }
}

/// Object placement state
pub struct BuildRoom {
    placement_obj: Option<(assets::ResourceKey<'static>, i16)>,
    ui: Option<ui::Node>,
    place_ui: Option<ui::Node>,
    close_after_place: bool,
    waiting_for_empty: bool,
    limited_mode: bool,
    highlighted_entities: Option<Vec<Entity>>,
    highlighted_object: Option<usize>,
    required_only: bool,
    cancel_event: Option<mpsc::Receiver<ui::prompt::ConfirmResponse>>,
    placement_style: Option<PlacementStyleData>,

    last_error: Option<String>,
}

#[derive(Clone, Copy)]
enum PlacementStyleData {
    TileRepeat { last: Location },
}

struct SetObject(assets::ResourceKey<'static>, bool, i16);
struct ToggleWallsEvent;
struct ShowRequiredEvent;

impl BuildRoom {
    pub fn new(limited_mode: bool) -> BuildRoom {
        BuildRoom {
            placement_obj: None,
            ui: None,
            place_ui: None,
            close_after_place: false,
            waiting_for_empty: false,
            limited_mode,
            highlighted_entities: None,
            highlighted_object: None,
            required_only: false,
            cancel_event: None,
            placement_style: None,
            last_error: None,
        }
    }

    /// Cancels the current object placement and updates
    /// the ui.
    fn cancel_object_placement(
        &mut self,
        instance: &mut GameInstance,
        state: &mut crate::GameState,
    ) {
        let room_id = match instance.player.state {
            State::EditRoom { active_room } => active_room,
            _ => panic!("Player is in the incorrect state"),
        };

        instance
            .level
            .cancel_object_placement::<entity::ClientEntityCreator>(
                room_id,
                &mut instance.entities,
            );

        // Remove the object placement controls
        state
            .keybinds
            .remove_collection(keybinds::KeyCollection::PlaceObject);
        self.placement_obj = None;

        // Re-enable the buttons (if the room if valid)
        let ui = assume!(state.global_logger, self.ui.as_ref());
        let valid = self.is_room_valid(instance, &mut state.ui_manager);
        if let Some(btn) = query!(ui, button(id = "accept")).next() {
            btn.set_property("disabled", !valid);
        }
        if let Some(btn) = query!(ui, button(id = "cancel")).next() {
            btn.set_property(
                "disabled",
                self.is_room_waiting(instance) || self.limited_mode,
            );
        }
        // Close the placement UI window
        if let Some(place) = self.place_ui.take() {
            state.ui_manager.remove_node(place);
        }
    }

    fn is_room_waiting(&self, instance: &mut GameInstance) -> bool {
        if self.limited_mode {
            return false;
        }
        let room_id = match instance.player.state {
            State::EditRoom { active_room } => active_room,
            _ => panic!("Player is in the incorrect state"),
        };
        instance.entities.with(
            |em: EntityManager<'_>, living: ecs::Read<Living>, ro: ecs::Read<RoomOwned>| {
                em.group_mask(&ro, |m| m.and(&living))
                    .any(|(_e, ro)| ro.room_id == room_id)
            },
        )
    }

    /// Returns whether the active room can be finalized in
    /// its current state. Also updates the object requirements
    /// text on the ui
    fn is_room_valid(&self, instance: &mut GameInstance, _ui_manager: &mut ui::Manager) -> bool {
        if self.is_room_waiting(instance) {
            return false;
        }
        let room_id = match instance.player.state {
            State::EditRoom { active_room } => active_room,
            _ => panic!("Player is in the incorrect state"),
        };

        let room = {
            let info = instance.level.get_room_info(room_id);
            assume!(
                instance.log,
                instance
                    .asset_manager
                    .loader_open::<room::Loader>(info.key.borrow())
            )
        };

        let ui = assume!(instance.log, self.ui.as_ref());

        if let Some(btn) = query!(ui, button(id="show_required") > content > @text).next() {
            btn.set_text(if self.required_only {
                "Show all".to_owned()
            } else {
                let room_id = match instance.player.state {
                    State::EditRoom { active_room } => active_room,
                    _ => panic!("Player is in the incorrect state"),
                };

                let room = {
                    let info = instance.level.get_room_info(room_id);
                    assume!(
                        instance.log,
                        instance
                            .asset_manager
                            .loader_open::<room::Loader>(info.key.borrow())
                    )
                };
                let mut total_required = 0;
                room.check_valid_placement(&instance.level, room_id, |_id, count| {
                    total_required += ::std::cmp::max(0, count);
                });
                format!("Show required ({})", total_required)
            });
        }

        let mut valid = room.check_valid_placement(&instance.level, room_id, |id, count| {
            if let Some(req) =
                query!(ui, object_entry(id=id as i32) > name > @text(requirement=true)).next()
            {
                req.set_text(format!(" (Required {})", ::std::cmp::max(0, count)));
            }
        });

        let cost = room.cost_for_room(&instance.level, room_id) - {
            let info = instance.level.get_room_info(room_id);
            info.placement_cost
        };
        let cost = if cost < UniDollar(0) {
            UniDollar(0)
        } else {
            cost
        };

        if let Some(price_tag) = query!(ui, window > price_tag).next() {
            price_tag.set_property(
                "can_afford",
                instance.player.get_money() >= cost || cost == UniDollar(0),
            );
            if let Some(txt) = query!(price_tag, @text).next() {
                txt.set_text(format!("Cost: {}", cost));
            }
        }

        if instance.player.get_money() < cost && cost != UniDollar(0) {
            valid = false;
        }

        valid
    }

    fn build_object_list(
        instance: &crate::GameInstance,
        ui: &ui::Node,
        room: &room::Room,
        required_only: bool,
    ) {
        use std::hash::{Hash, Hasher};
        // Create the icons for objects that can be placed
        // in the room
        if let Some(content) = query!(ui, scroll_panel > content).next() {
            for c in content.children() {
                content.remove_child(c);
            }

            let mut groups = FNVMap::default();
            for (id, object) in room.valid_objects.iter().enumerate() {
                let obj = assume!(
                    instance.log,
                    instance
                        .asset_manager
                        .loader_open::<object::Loader>(object.borrow())
                );
                if required_only
                    && !room
                        .required_objects
                        .iter()
                        .any(|(k, _)| k.weak_match(&obj.key))
                {
                    continue;
                }
                let group = groups.entry(obj.group.clone()).or_insert_with(Vec::new);
                group.push((id, obj));
            }
            let mut groups = groups.into_iter().collect::<Vec<_>>();
            groups.sort_unstable_by(|a, b| a.0.cmp(&b.0));

            for (group, mut objs) in groups {
                objs.sort_unstable_by(|a, b| a.1.display_name.cmp(&b.1.display_name));
                let mut hasher = FNVHash::default();
                group.hash(&mut hasher);
                let hue = (hasher.finish() as f64) / (u64::max_value() as f64);
                let (r, g, b) = hsl_to_rgb(hue, 1.0, 0.8);
                let group_node = node! {
                    object_group(entries=objs.len() as i32, r=f64::from(r)/255.0, g=f64::from(g)/255.0, b=f64::from(b)/255.0, open=true) {
                        background
                        title {
                            @text(group)
                        }
                        objects {
                        }
                    }
                };
                if let Some(objects) = query!(group_node, objects).next() {
                    for (id, obj) in objs {
                        let required = room
                            .required_objects
                            .iter()
                            .any(|(k, _)| k.weak_match(&obj.key));
                        let node = node! {
                            object_entry(id = id as i32) {
                                name {
                                    @text(obj.display_name.clone())
                                }
                                price(can_afford=true) {
                                    @text(obj.cost.to_string())
                                }
                            }
                        };
                        if required {
                            if let Some(name) = query!(node, name).next() {
                                let required = ui::Node::new_text(" Missing req");
                                required.set_property("requirement", true);
                                name.add_child(required);
                            }
                        }
                        let object = obj.key.clone();
                        node.set_property(
                            "on_click",
                            ui::MethodDesc::<ui::MouseUpEvent>::native(move |evt, _, _| {
                                evt.emit(SetObject(object.clone(), false, 0));
                                true
                            }),
                        );

                        objects.add_child(node);
                    }
                }
                content.add_child(group_node);
            }
        }
    }
}

fn display_error<T>(
    ui_manager: &mut ui::Manager,
    err: server::errors::Result<T>,
    last_error: &mut Option<String>,
    mouse_pos: (i32, i32),
) -> Option<T> {
    match err {
        Ok(val) => {
            if let Some(last) = last_error.take() {
                let key = format!("{}", last);
                ui_manager.hide_tooltip(&key);
            }
            Some(val)
        }
        Err(err) => {
            let key = format!("{}", err);
            if last_error.as_ref().map_or(true, |o| *o != key) {
                let content = node!(content);
                let txt = ui::Node::new_text(&*key);
                txt.set_property("error", true);
                content.add_child(txt);
                ui_manager.show_tooltip(&key, content, mouse_pos.0, mouse_pos.1);
                *last_error = Some(key);
            } else {
                ui_manager.move_tooltip(&key, mouse_pos.0, mouse_pos.1);
            }
            None
        }
    }
}

impl state::State for BuildRoom {
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(BuildRoom {
            placement_obj: self.placement_obj.clone(),
            ui: self.ui.clone(),
            place_ui: self.place_ui.clone(),
            close_after_place: self.close_after_place,
            waiting_for_empty: self.waiting_for_empty,
            limited_mode: self.limited_mode,
            highlighted_entities: self.highlighted_entities.clone(),
            highlighted_object: self.highlighted_object,
            required_only: self.required_only,
            cancel_event: None,
            placement_style: self.placement_style,
            last_error: None,
        })
    }

    fn active_req(
        &mut self,
        req: &mut state::CaptureRequester,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
    ) -> state::Action {
        // Setup keybinds
        state
            .keybinds
            .add_collection(keybinds::KeyCollection::BuildRoom);

        let instance = assume!(state.global_logger, instance.as_mut());

        // Handle the reply from a prompt if any
        if let Some(ui::prompt::ConfirmResponse::Accept) =
            self.cancel_event.take().and_then(|v| v.recv().ok())
        {
            // Inform the server and roll back to the previous screen
            let mut cmd: command::Command = command::CancelRoom::default().into();
            let mut proxy = super::GameProxy::proxy(state);
            try_cmd!(
                instance.log,
                cmd.execute(
                    &mut proxy,
                    &mut instance.player,
                    command::CommandParams {
                        log: &instance.log,
                        level: &mut instance.level,
                        engine: &instance.scripting,
                        entities: &mut instance.entities,
                        snapshots: &instance.snapshots,
                        mission_handler: instance.mission_handler.as_ref().map(|v| v.borrow()),
                    }
                ),
                {
                    instance.push_command(cmd, req);
                    return state::Action::Switch(Box::new(FinalizePlacement::new()));
                }
            );
        }
        let room_id = match instance.player.state {
            State::EditRoom { active_room } => active_room,
            _ => panic!("Player is in the incorrect state"),
        };

        // Spawn the building ui and enable the
        // toggle walls button
        let ui = state
            .ui_manager
            .create_node(assets::ResourceKey::new("base", "room/build_room"));
        self.ui = Some(ui.clone());
        if let Some(btn) = query!(ui, button(id = "toggle_walls")).next() {
            btn.set_property(
                "on_click",
                ui::MethodDesc::<ui::MouseUpEvent>::native(|evt, _, _| {
                    evt.emit(ToggleWallsEvent);
                    true
                }),
            );
        }
        if let Some(btn) = query!(ui, button(id = "show_required")).next() {
            btn.set_property(
                "on_click",
                ui::MethodDesc::<ui::MouseUpEvent>::native(|evt, _, _| {
                    evt.emit(ShowRequiredEvent);
                    true
                }),
            );
        }

        let key = {
            let info = instance.level.get_room_info(room_id);
            state.renderer.set_focused_region(info.area);
            info.key.clone()
        };
        let room = assume!(
            state.global_logger,
            instance
                .level
                .asset_manager
                .loader_open::<room::Loader>(key.borrow())
        );

        // Set the name of the room on the ui window
        if let Some(txt) = query!(ui, room_name > @text).next() {
            txt.set_text(room.name.clone());
        }
        Self::build_object_list(instance, &ui, &room, false);

        // Update the requirements of objects
        // and disable the done button if the room
        // needs objects first
        let valid = self.is_room_valid(instance, &mut state.ui_manager);
        self.waiting_for_empty = self.is_room_waiting(instance);
        if let Some(btn) = query!(ui, button(id = "accept")).next() {
            btn.set_property("disabled", !valid);
        }
        if let Some(btn) = query!(ui, button(id = "cancel")).next() {
            btn.set_property("disabled", self.waiting_for_empty || self.limited_mode);
            if self.limited_mode {
                let info = instance.level.get_room_info(room_id);
                let reason = if let Err(reason) = instance.level.is_blocked_edit(room_id) {
                    reason.to_owned()
                } else if !info.controller.is_invalid()
                    && instance
                        .entities
                        .get_component::<entity::ClientBooked>(info.controller)
                        .is_some()
                {
                    "Room is booked for courses".to_owned()
                } else {
                    "".to_owned()
                };
                btn.set_property("tooltip", reason);
            }
        }
        state::Action::Nothing
    }

    fn inactive(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) {
        // Disable keybinds for building
        state
            .keybinds
            .remove_collection(keybinds::KeyCollection::BuildRoom);
        state.renderer.clear_focused_region();

        let instance = assume!(state.global_logger, instance.as_mut());
        if let Some(entities) = self.highlighted_entities.take() {
            for entity in entities {
                instance
                    .entities
                    .remove_component::<entity::Highlighted>(entity);
            }
        }

        // Clear away ui elements
        {
            if let Some(ui) = self.ui.take() {
                state.ui_manager.remove_node(ui);
            }
            if let Some(place) = self.place_ui.take() {
                state.ui_manager.remove_node(place);
            }
        }
        state
            .renderer
            .set_mouse_sprite(ResourceKey::new("base", "ui/cursor/normal"));
    }

    fn tick(
        &mut self,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
    ) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());
        let ui = assume!(state.global_logger, self.ui.as_ref());

        if self.is_room_waiting(instance) {
            state
                .renderer
                .set_mouse_sprite(ResourceKey::new("base", "ui/cursor/question"));
        } else if self.waiting_for_empty {
            self.waiting_for_empty = false;
            state
                .renderer
                .set_mouse_sprite(ResourceKey::new("base", "ui/cursor/normal"));
            if let Some(btn) = query!(ui, button(id = "cancel")).next() {
                btn.set_property(
                    "disabled",
                    self.is_room_waiting(instance) || self.limited_mode,
                );
            }
        }
        let valid = self.is_room_valid(instance, &mut state.ui_manager);
        if let Some(btn) = query!(ui, button(id = "accept")).next() {
            if btn.get_property("disabled").unwrap_or(false) != !valid {
                btn.set_property("disabled", !valid);
            }
        }

        let room_id = match instance.player.state {
            State::EditRoom { active_room } => active_room,
            _ => panic!("Player is in the incorrect state"),
        };
        let area = {
            let info = instance.level.get_room_info_mut(room_id);
            if info.building_level.as_ref().is_some() {
                None
            } else {
                let area = info.area;
                if info.lower_walls != state.renderer.get_lowered_region().is_some() {
                    if state.renderer.get_lowered_region().is_none() {
                        state.renderer.set_lowered_region(area);
                    } else {
                        state.renderer.clear_lowered_region();
                    }
                    Some(area)
                } else {
                    None
                }
            }
        };
        if let Some(area) = area {
            for loc in area {
                instance.level.flag_dirty(loc.x, loc.y);
            }
        }

        state::Action::Nothing
    }

    fn mouse_move_req(
        &mut self,
        req: &mut state::CaptureRequester,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
        mouse_pos: (i32, i32),
    ) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());

        if self.is_room_waiting(instance) {
            return state::Action::Nothing;
        }

        let world_pos = state.renderer.mouse_to_level(mouse_pos.0, mouse_pos.1);

        let room_id = match instance.player.state {
            State::EditRoom { active_room } => active_room,
            _ => panic!("Player is in the incorrect state"),
        };

        // If placing an object attempt to move it to the current mouse
        // position and then update the error message if it failed
        if self.place_ui.is_some() {
            let place = assume!(state.global_logger, self.placement_obj.as_ref());
            let err = instance
                .level
                .move_active_object::<_, entity::ClientEntityCreator>(
                    room_id,
                    &instance.scripting,
                    &mut instance.entities,
                    world_pos,
                    None,
                    place.1,
                );
            display_error(&mut state.ui_manager, err, &mut self.last_error, mouse_pos);

            if let Some(style) = self.placement_style {
                let do_place = match style {
                    PlacementStyleData::TileRepeat { last } => {
                        let cur = Location::new(world_pos.0 as i32, world_pos.1 as i32);
                        if cur != last {
                            self.placement_style =
                                Some(PlacementStyleData::TileRepeat { last: cur });
                            true
                        } else {
                            false
                        }
                    }
                };
                if do_place {
                    // Attempt to place an object and sync that placement
                    // with the server
                    let mut cmd: command::Command =
                        command::PlaceObject::new(place.0.clone(), world_pos, place.1).into();
                    let mut proxy = super::GameProxy::proxy(state);
                    match cmd.execute(
                        &mut proxy,
                        &mut instance.player,
                        command::CommandParams {
                            log: &instance.log,
                            level: &mut instance.level,
                            engine: &instance.scripting,
                            entities: &mut instance.entities,
                            snapshots: &instance.snapshots,
                            mission_handler: instance.mission_handler.as_ref().map(|v| v.borrow()),
                        },
                    ) {
                        Ok(_) => {
                            instance.push_command(cmd, req);
                            // Update the requirements list
                            self.is_room_valid(instance, &mut proxy.state.ui_manager);

                            proxy
                                .state
                                .audio
                                .controller
                                .borrow_mut()
                                .play_sound(ResourceKey::new("base", "place"));

                            // Checked below
                            assume!(
                                proxy.state.global_logger,
                                instance
                                    .level
                                    .begin_object_placement::<_, entity::ClientEntityCreator>(
                                        room_id,
                                        &instance.scripting,
                                        &mut instance.entities,
                                        place.0.borrow(),
                                        None
                                    )
                            );
                            let err = instance
                                .level
                                .move_active_object::<_, entity::ClientEntityCreator>(
                                    room_id,
                                    &instance.scripting,
                                    &mut instance.entities,
                                    world_pos,
                                    None,
                                    place.1,
                                );
                            display_error(
                                &mut proxy.state.ui_manager,
                                err,
                                &mut self.last_error,
                                mouse_pos,
                            );
                        }
                        Err(err) => {
                            display_error::<()>(
                                &mut proxy.state.ui_manager,
                                Err(err),
                                &mut self.last_error,
                                mouse_pos,
                            );

                            proxy
                                .state
                                .audio
                                .controller
                                .borrow_mut()
                                .play_sound(ResourceKey::new("base", "place_fail"));
                        }
                    }
                }
            }
        } else if let Some(last) = self.last_error.take() {
            let key = format!("{}", last);
            state.ui_manager.hide_tooltip(&key);
        }

        if let Some(entities) = self.highlighted_entities.take() {
            for entity in entities {
                instance
                    .entities
                    .remove_component::<entity::Highlighted>(entity);
            }
        }

        if self.place_ui.is_none() {
            if let Some(obj) = find_object_at(&state.renderer, &instance.level, room_id, mouse_pos)
            {
                if let Some(entities) = instance
                    .level
                    .get_room_objects(room_id)
                    .iter()
                    .nth(obj)
                    .and_then(|v| v.as_ref())
                    .map(|v| v.1.get_entities())
                {
                    for e in &entities {
                        instance.entities.add_component(
                            *e,
                            entity::Highlighted {
                                color: (0, 255, 255),
                            },
                        );
                    }
                    self.highlighted_entities = Some(entities);
                }
                if self.highlighted_object != Some(obj) {
                    if let Some(obj) = self.highlighted_object.take() {
                        state.ui_manager.hide_tooltip(&format!("object_{}", obj));
                    }
                    let content = node!(content);
                    let buttons = state
                        .keybinds
                        .keys_for_action(keybinds::KeyAction::PlacementMove)
                        .map(|(_, btn)| format!("<{}>", btn.as_string()))
                        .collect::<Vec<_>>();
                    let btn = ui::Node::new_text(buttons.join(", "));
                    btn.set_property("key_btn", true);
                    content.add_child(btn);
                    content.add_child(ui::Node::new_text(" - Move"));
                    content.add_child(ui::Node::new_text("\n"));
                    let buttons = state
                        .keybinds
                        .keys_for_action(keybinds::KeyAction::PlacementRemove)
                        .map(|(_, btn)| format!("<{}>", btn.as_string()))
                        .collect::<Vec<_>>();
                    let btn = ui::Node::new_text(buttons.join(", "));
                    btn.set_property("key_btn", true);
                    content.add_child(btn);
                    content.add_child(ui::Node::new_text(" - Remove"));

                    state.ui_manager.show_tooltip(
                        &format!("object_{}", obj),
                        content,
                        mouse_pos.0,
                        mouse_pos.1,
                    );
                }
                state
                    .ui_manager
                    .move_tooltip(&format!("object_{}", obj), mouse_pos.0, mouse_pos.1);

                self.highlighted_object = Some(obj);
            } else {
                if let Some(obj) = self.highlighted_object.take() {
                    state.ui_manager.hide_tooltip(&format!("object_{}", obj));
                }
            }
        } else {
            if let Some(obj) = self.highlighted_object.take() {
                state.ui_manager.hide_tooltip(&format!("object_{}", obj));
            }
        }

        state::Action::Nothing
    }

    fn key_action_req(
        &mut self,
        req: &mut state::CaptureRequester,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
        action: keybinds::KeyAction,
        mouse_pos: (i32, i32),
    ) -> state::Action {
        use crate::keybinds::KeyAction::*;
        let instance = assume!(state.global_logger, instance.as_mut());

        if self.is_room_waiting(instance) {
            return state::Action::Nothing;
        }

        match action {
            PlacementRotate => {
                if let Some(obj) = self.placement_obj.as_mut() {
                    // Update the rotation counter
                    // Up to scripts to interpret this value
                    obj.1 += 1;

                    let world_pos = state.renderer.mouse_to_level(mouse_pos.0, mouse_pos.1);
                    let room_id = match instance.player.state {
                        State::EditRoom { active_room } => active_room,
                        _ => panic!("Player is in the incorrect state"),
                    };

                    // Attempt to rotate the object and update the error if it fails

                    let err = instance
                        .level
                        .move_active_object::<_, entity::ClientEntityCreator>(
                            room_id,
                            &instance.scripting,
                            &mut instance.entities,
                            world_pos,
                            None,
                            obj.1,
                        );
                    display_error(&mut state.ui_manager, err, &mut self.last_error, mouse_pos);
                }
            }
            ty @ PlacementDragStart | ty @ PlacementFinish => {
                if let Some(obj) = self.placement_obj.as_ref() {
                    let info = assume!(
                        state.global_logger,
                        instance
                            .asset_manager
                            .loader_open::<object::Loader>(obj.0.borrow())
                    );
                    if ty == PlacementDragStart && self.close_after_place {
                        return state::Action::Nothing;
                    }
                    // Don't place on drag for normal objects
                    if ty == PlacementDragStart && info.placement_style == None {
                        return state::Action::Nothing;
                    }
                    // End dragging if we were already
                    if ty == PlacementFinish {
                        self.placement_style = None;
                    }
                    let world_pos = state.renderer.mouse_to_level(mouse_pos.0, mouse_pos.1);
                    let room_id = match instance.player.state {
                        State::EditRoom { active_room } => active_room,
                        _ => panic!("Player is in the incorrect state"),
                    };

                    // Attempt to place an object and sync that placement
                    // with the server
                    let mut cmd: command::Command =
                        command::PlaceObject::new(obj.0.clone(), world_pos, obj.1).into();
                    let mut proxy = super::GameProxy::proxy(state);
                    match cmd.execute(
                        &mut proxy,
                        &mut instance.player,
                        command::CommandParams {
                            log: &instance.log,
                            level: &mut instance.level,
                            engine: &instance.scripting,
                            entities: &mut instance.entities,
                            snapshots: &instance.snapshots,
                            mission_handler: instance.mission_handler.as_ref().map(|v| v.borrow()),
                        },
                    ) {
                        Ok(_) => {
                            instance.push_command(cmd, req);
                            // Update the requirements list
                            self.is_room_valid(instance, &mut proxy.state.ui_manager);

                            proxy
                                .state
                                .audio
                                .controller
                                .borrow_mut()
                                .play_sound(ResourceKey::new("base", "place"));

                            // If this wasn't a object move restart the placement again
                            // with the same object type
                            if !self.close_after_place {
                                // Checked below
                                assume!(
                                    proxy.state.global_logger,
                                    instance
                                        .level
                                        .begin_object_placement::<_, entity::ClientEntityCreator>(
                                            room_id,
                                            &instance.scripting,
                                            &mut instance.entities,
                                            obj.0.borrow(),
                                            None
                                        )
                                );

                                let err = instance
                                    .level
                                    .move_active_object::<_, entity::ClientEntityCreator>(
                                        room_id,
                                        &instance.scripting,
                                        &mut instance.entities,
                                        world_pos,
                                        None,
                                        obj.1,
                                    );
                                display_error(
                                    &mut proxy.state.ui_manager,
                                    err,
                                    &mut self.last_error,
                                    mouse_pos,
                                );
                            }
                        }
                        Err(err) => {
                            display_error::<()>(
                                &mut proxy.state.ui_manager,
                                Err(err),
                                &mut self.last_error,
                                mouse_pos,
                            );

                            proxy
                                .state
                                .audio
                                .controller
                                .borrow_mut()
                                .play_sound(ResourceKey::new("base", "place_fail"));

                            // Skip the closing of the ui if it failed to place
                            return state::Action::Nothing;
                        }
                    }
                    // If this was an object move close the placement
                    // ui after a placement
                    if !self.close_after_place && ty == PlacementDragStart {
                        self.placement_style = match info.placement_style {
                            Some(PlacementStyle::TileRepeat) => {
                                Some(PlacementStyleData::TileRepeat {
                                    last: Location::new(world_pos.0 as i32, world_pos.1 as i32),
                                })
                            }
                            None => None,
                        };
                    }
                }

                // If this was an object move close the placement
                // ui after a placement
                if self.close_after_place {
                    self.close_after_place = false;
                    self.cancel_object_placement(instance, state);
                }
            }
            evt @ PlacementRemove | evt @ PlacementMove => {
                {
                    // Find the object under the mouse (if any)
                    let obj = {
                        let room_id = match instance.player.state {
                            State::EditRoom { active_room } => active_room,
                            _ => panic!("Player is in the incorrect state"),
                        };

                        if let Some(obj) =
                            find_object_at(&state.renderer, &instance.level, room_id, mouse_pos)
                        {
                            // If 'moving' the object start a placement again after
                            // removing it
                            if evt == PlacementMove {
                                let (obj, rot) = assume!(
                                    state.global_logger,
                                    instance
                                        .level
                                        .get_room_objects(room_id)
                                        .iter()
                                        .nth(obj)
                                        .and_then(|v| v.as_ref())
                                        .map(|v| (v.0.key.clone(), v.0.rotation))
                                );
                                state.ui_manager.events().emit(SetObject(obj, true, rot));
                            }
                            Some(obj)
                        } else {
                            None
                        }
                    };

                    if let Some(obj) = obj {
                        state
                            .audio
                            .controller
                            .borrow_mut()
                            .play_sound(ResourceKey::new("base", "whoosh"));

                        // Inform the server of its removal if it can be removed
                        let mut cmd: command::Command = command::RemoveObject::new(obj).into();
                        let mut proxy = super::GameProxy::proxy(state);
                        try_cmd!(
                            instance.log,
                            cmd.execute(
                                &mut proxy,
                                &mut instance.player,
                                command::CommandParams {
                                    log: &instance.log,
                                    level: &mut instance.level,
                                    engine: &instance.scripting,
                                    entities: &mut instance.entities,
                                    snapshots: &instance.snapshots,
                                    mission_handler: instance
                                        .mission_handler
                                        .as_ref()
                                        .map(|v| v.borrow()),
                                }
                            ),
                            {
                                instance.push_command(cmd, req);
                            }
                        );
                    }
                }
                // Update the requirements
                let valid = self.is_room_valid(instance, &mut state.ui_manager);
                if let Some(btn) = query!(
                    assume!(state.global_logger, self.ui.as_ref()),
                    button(id = "accept")
                )
                .next()
                {
                    btn.set_property("disabled", !valid);
                }
            }
            _ => {}
        }
        state::Action::Nothing
    }

    fn ui_event_req(
        &mut self,
        req: &mut state::CaptureRequester,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
        evt: &mut event::EventHandler,
    ) -> state::Action {
        let mut action = state::Action::Nothing;

        let instance = assume!(state.global_logger, instance.as_mut());

        if self.is_room_waiting(instance) {
            return state::Action::Nothing;
        }

        let (place_ui, ui) = (
            self.place_ui.clone(),
            assume!(state.global_logger, self.ui.clone()),
        );

        // Cancel button for both the placement ui and the object list
        evt.handle_event_if::<super::CancelEvent, _, _>(
            |evt| place_ui.map_or(false, |v| v.is_same(&evt.0)),
            |_| {
                self.cancel_object_placement(instance, state);
            },
        );
        evt.handle_event_if::<super::CancelEvent, _, _>(|evt| ui.is_same(&evt.0), |_| {
            if self.place_ui.is_none() && !self.is_room_waiting(instance) && !self.limited_mode {
                let room_id = match instance.player.state {
                    State::EditRoom{active_room} => active_room,
                    _ => panic!("Player is in the incorrect state"),
                };
                if instance.level.get_room_objects(room_id).iter().next().is_some() {
                    let (send, recv) = mpsc::channel();
                    self.cancel_event = Some(recv);
                    action = state::Action::Push(Box::new(ui::prompt::Confirm::new(
                        ui::prompt::ConfirmConfig {
                            title: "Remove Room".into(),
                            description: "This will return the room to the blueprint mode, removing all objects. Are you sure?".into(),
                            accept: "Remove All".into(),
                            ..ui::prompt::ConfirmConfig::default()
                        },
                        move |rpl| {
                            let _ = send.send(rpl);
                        }
                    )));
                } else {
                    // Inform the server and roll back to the previous screen
                    let mut cmd: command::Command = command::CancelRoom::default().into();
                    let mut proxy = super::GameProxy::proxy(state);
                    try_cmd!(instance.log, cmd.execute(&mut proxy, &mut instance.player, command::CommandParams {
                        log: &instance.log,
                        level: &mut instance.level,
                        engine: &instance.scripting,
                        entities: &mut instance.entities,
                        snapshots: &instance.snapshots,
                        mission_handler: instance.mission_handler.as_ref().map(|v| v.borrow()),
                    }), {
                        instance.push_command(cmd, req);
                        action = state::Action::Switch(Box::new(FinalizePlacement::new()));
                    });
                }
            }
        });

        // Done button, only enabled if the room is valid
        evt.handle_event_if::<super::AcceptEvent, _, _>(
            |evt| ui.is_same(&evt.0),
            |_| {
                if self.is_room_valid(instance, &mut state.ui_manager) {
                    self.cancel_object_placement(instance, state);
                    // Inform the server and close the ui
                    let mut proxy = super::GameProxy::proxy(state);
                    if self.limited_mode {
                        let area = {
                            let room_id = match instance.player.state {
                                State::EditRoom { active_room } => active_room,
                                _ => panic!("Player is in the incorrect state"),
                            };

                            let info = instance.level.get_room_info_mut(room_id);
                            if info.building_level.as_ref().is_some() {
                                None
                            } else {
                                let area = info.area;
                                Some(area)
                            }
                        };
                        let mut cmd: command::Command =
                            command::FinalizeLimitedEdit::default().into();
                        try_cmd!(
                            instance.log,
                            cmd.execute(
                                &mut proxy,
                                &mut instance.player,
                                command::CommandParams {
                                    log: &instance.log,
                                    level: &mut instance.level,
                                    engine: &instance.scripting,
                                    entities: &mut instance.entities,
                                    snapshots: &instance.snapshots,
                                    mission_handler: instance
                                        .mission_handler
                                        .as_ref()
                                        .map(|v| v.borrow()),
                                }
                            ),
                            {
                                if let Some(area) = area {
                                    proxy.state.renderer.clear_lowered_region();
                                    for loc in area {
                                        instance.level.flag_dirty(loc.x, loc.y);
                                    }
                                }
                                instance.push_command(cmd, req);
                                action = state::Action::Pop;
                            }
                        );
                    } else {
                        let mut cmd: command::Command = command::FinalizeRoom::default().into();
                        try_cmd!(
                            instance.log,
                            cmd.execute(
                                &mut proxy,
                                &mut instance.player,
                                command::CommandParams {
                                    log: &instance.log,
                                    level: &mut instance.level,
                                    engine: &instance.scripting,
                                    entities: &mut instance.entities,
                                    snapshots: &instance.snapshots,
                                    mission_handler: instance
                                        .mission_handler
                                        .as_ref()
                                        .map(|v| v.borrow()),
                                }
                            ),
                            {
                                instance.push_command(cmd, req);
                                action = state::Action::Pop;
                            }
                        );
                    }
                }
            },
        );

        // Clicking a object in the list or when moving an object
        evt.handle_event::<SetObject, _>(|SetObject(obj, close, rotation)| {
            // End the previous placement
            if self.placement_obj.is_some() {
                self.cancel_object_placement(instance, state);
            }

            let room_id = match instance.player.state {
                State::EditRoom { active_room } => active_room,
                _ => panic!("Player is in the incorrect state"),
            };

            // Start a new placement with the selected object
            {
                instance
                    .level
                    .cancel_object_placement::<entity::ClientEntityCreator>(
                        room_id,
                        &mut instance.entities,
                    );
                // Handled below
                assume!(
                    state.global_logger,
                    instance
                        .level
                        .begin_object_placement::<_, entity::ClientEntityCreator>(
                            room_id,
                            &instance.scripting,
                            &mut instance.entities,
                            obj.borrow(),
                            None
                        )
                );
            }

            self.close_after_place = close;

            if self.placement_obj.is_none() {
                // Setup the placement keybinds and ui
                state
                    .keybinds
                    .add_collection(keybinds::KeyCollection::PlaceObject);

                let ui = assume!(state.global_logger, self.ui.as_ref());
                if let Some(btn) = query!(ui, button(id = "accept")).next() {
                    btn.set_property("disabled", true);
                }
                if let Some(btn) = query!(ui, button(id = "cancel")).next() {
                    btn.set_property("disabled", true);
                }

                let place = state
                    .ui_manager
                    .create_node(assets::ResourceKey::new("base", "room/place_object"));
                self.place_ui = Some(place.clone());
                if let Some(title) = query!(place, title > @text).next() {
                    let info = assume!(
                        state.global_logger,
                        instance
                            .asset_manager
                            .loader_open::<object::Loader>(obj.borrow())
                    );
                    title.set_text(format!("Place {}", info.display_name));
                }

                // Mark as placing this object type
                self.placement_obj = Some((obj, rotation));

                // Attempt to move to the last known cursor location to prevent a
                // flicker when moving an object
                let place = assume!(state.global_logger, self.placement_obj.as_ref());
                let world_pos = (
                    instance.last_cursor_position.x,
                    instance.last_cursor_position.y,
                );
                let _err = instance
                    .level
                    .move_active_object::<_, entity::ClientEntityCreator>(
                        room_id,
                        &instance.scripting,
                        &mut instance.entities,
                        world_pos,
                        None,
                        place.1,
                    );
            }
        });

        // Lowers the walls to make placing/moving easier
        evt.handle_event::<ToggleWallsEvent, _>(|_| {
            let room_id = match instance.player.state {
                State::EditRoom { active_room } => active_room,
                _ => panic!("Player is in the incorrect state"),
            };
            let info: &mut RoomPlacement = &mut *instance.level.get_room_info_mut(room_id);
            if let Some(virt) = info.building_level.as_mut() {
                virt.should_lower_walls = !virt.should_lower_walls;
                virt.dirty = true;
            } else {
                info.lower_walls = !info.lower_walls;
            }
        });
        evt.handle_event::<ShowRequiredEvent, _>(|_| {
            self.required_only = !self.required_only;
            if let Some(btn) = query!(ui, button(id="show_required") > content > @text).next() {
                btn.set_text(if self.required_only {
                    "Show all".to_owned()
                } else {
                    let room_id = match instance.player.state {
                        State::EditRoom { active_room } => active_room,
                        _ => panic!("Player is in the incorrect state"),
                    };

                    let room = {
                        let info = instance.level.get_room_info(room_id);
                        assume!(
                            instance.log,
                            instance
                                .asset_manager
                                .loader_open::<room::Loader>(info.key.borrow())
                        )
                    };
                    let mut total_required = 0;
                    room.check_valid_placement(&instance.level, room_id, |_id, count| {
                        total_required += ::std::cmp::max(0, count);
                    });
                    format!("Show required ({})", total_required)
                });
                if let Some(content) = query!(ui, scroll_panel > content).next() {
                    content.set_property("scroll_y", 0);
                }
                if let Some(thumb) = query!(ui, scroll_panel > scroll_bar > scroll_thumb).next() {
                    thumb.set_property("offset", 0.0);
                }
            }
            let room_id = match instance.player.state {
                State::EditRoom { active_room } => active_room,
                _ => panic!("Player is in the incorrect state"),
            };

            let key = {
                let info = instance.level.get_room_info(room_id);
                info.key.clone()
            };
            let room = assume!(
                state.global_logger,
                instance
                    .level
                    .asset_manager
                    .loader_open::<room::Loader>(key.borrow())
            );
            Self::build_object_list(instance, &ui, &room, self.required_only);
        });
        action
    }
}

/// Returns the object id of the object at the cursor's location
/// if any. Walls are only checked if they aren't lowered
fn find_object_at(
    renderer: &render::Renderer,
    level: &Level,
    room_id: room::Id,
    pos: (i32, i32),
) -> Option<usize> {
    use crate::server::level::object::ObjectPlacementAction::{SelectionBound, WallFlag};

    let ray = renderer.get_mouse_ray(pos.0, pos.1);
    let world_pos = renderer.mouse_to_level(pos.0, pos.1);

    let lower_wall = if let Some(lvl) = level.get_room_info(room_id).building_level.as_ref() {
        if lvl.should_lower_walls {
            None
        } else {
            Some(())
        }
    } else {
        Some(())
    };
    // If walls aren't lowered
    lower_wall
        // and the cursor intersects with a wall
        .and_then(|_| {
            if let Some(lvl) = level.get_room_info(room_id).building_level.as_ref() {
                intersect_wall(lvl, ray, world_pos)
            } else {
                intersect_wall(level, ray, world_pos)
            }
        })
        // and the wall has an object that is a part of this
        // room
        .and_then(|(loc, dir)| {
            let info = if let Some(lvl) = level.get_room_info(room_id).building_level.as_ref() {
                lvl.get_wall_info(loc, dir)
            } else {
                level.get_wall_info(loc, dir)
            };
            info.map(|v| v.flag)
                .filter(|v| *v != TileWallFlag::None)
                .and_then(|iflag| {
                    // Find the object on the given wall (if any)
                    level
                        .get_room_objects(room_id)
                        .iter()
                        .enumerate()
                        .rev()
                        .find(|&(_, obj)| {
                            obj.iter().flat_map(|v| &v.0.actions.0).any(|action| {
                                // Have to check both ways as an object
                                // can be attached to either side of a wall
                                if let WallFlag {
                                    location,
                                    direction,
                                    ref flag,
                                } = *action
                                {
                                    let flags_same = match (iflag, flag) {
                                        (TileWallFlag::None, &object::WallPlacementFlag::None) => {
                                            true
                                        }
                                        (
                                            TileWallFlag::Window(_),
                                            &object::WallPlacementFlag::Window { .. },
                                        ) => true,
                                        (TileWallFlag::Door, &object::WallPlacementFlag::Door) => {
                                            true
                                        }
                                        _ => false,
                                    };
                                    return flags_same && (location == loc && direction == dir)
                                        || (location.shift(direction) == loc
                                            && direction.reverse() == dir);
                                }
                                false
                            })
                        })
                        .map(|v| v.0)
                })
        })
        // If we have already found an object return it
        // otherwise check placed objects in the room for a hit
        .or_else(|| {
            level
                .get_room_objects(room_id)
                .iter()
                .enumerate()
                .rev()
                .find(|&(_, obj)| {
                    obj.iter().flat_map(|v| &v.0.actions.0).any(|action| {
                        if let SelectionBound(bound) = *action {
                            return bound.intersects_ray(ray);
                        }
                        false
                    })
                })
                .map(|v| v.0)
        })
}

/// Returns the location and direction of the wall that intersects
/// with the passed ray if one is there.
fn intersect_wall<L: LevelView>(
    lvl: &L,
    ray: Ray,
    pos: (f32, f32),
) -> Option<(Location, Direction)> {
    let loc = Location::new(pos.0 as i32, pos.1 as i32);
    // Check a few walls
    for y in -2..2 {
        for x in -2..2 {
            for dir in &[Direction::South, Direction::West] {
                let loc = loc + (x, y);
                if let Some(_info) = lvl.get_wall_info(loc, *dir) {
                    let (ox, oy) = dir.offset();
                    // Build a bound of the wall to intersect with
                    let pos = Vector3::new(
                        loc.x as f32 + 0.5 + (ox as f32) * 0.5,
                        0.0,
                        loc.y as f32 + 0.5 + (oy as f32) * 0.5,
                    );

                    let fx = (ox as f32).abs();
                    let fy = (oy as f32).abs();
                    let bound = AABB {
                        min: pos
                            - Vector3::new(
                                fx * (2.0 / 16.0) + (1.0 - fx) * 0.5,
                                0.0,
                                (1.0 - fy) * 0.5 + fy * (2.0 / 16.0),
                            ),
                        max: pos
                            + Vector3::new(
                                fx * (2.0 / 16.0) + (1.0 - fx) * 0.5,
                                1.0,
                                (1.0 - fy) * 0.5 + fy * (2.0 / 16.0),
                            ),
                    };

                    if bound.intersects_ray(ray) {
                        return Some((loc, *dir));
                    }
                }
            }
        }
    }
    None
}
