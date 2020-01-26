use super::*;
use crate::prelude::*;
use crate::ui;
use crate::entity;
use crate::server::event;

use std::sync::mpsc;

pub fn create_level(log: &Logger, assets: &AssetManager, ui: &mut ui::Manager, entities: &mut Container) -> server::errors::Result<Level> {
    let level = Level::new::<entity::ClientEntityCreator, _>(
        log.new(o!("type" => "level")),
        ui.get_script_engine(),
        assets,
        entities,
        &[PlayerId(1)],
        100,
    );
    let mut level = assume!(log, level);

    let player = PlayerId(1);
    let tmp_room_id = RoomId(-1);

    let door = ResourceKey::new("base", "doors/basic");
    let desk = ResourceKey::new("base", "desk_two_chair");
    let window = ResourceKey::new("base", "window");
    let wait = ResourceKey::new("base", "other/waiting_area");
    let projector = ResourceKey::new("base", "teach_objs/projector");
    let chair = ResourceKey::new("base", "chairs/single_chair");

    let engine = &*ui.get_script_engine();
    let base_loc = Location::new(32 + 16, 32 + 16 + 50 - 13);

    type EC = entity::ClientEntityCreator;

    // Registration office
    {
        let bloc = base_loc + (1, 15);
        let id = assume!(log, level.place_room_id::<entity::ClientEntityCreator, _>(
            engine, entities,
            tmp_room_id, player,
            ResourceKey::new("base", "registration_office"),
            Bound::new(
                bloc,
                bloc + (4, 4)
            )
        ));
        let id = level.finalize_placement(id);
        place_objects! {
            init(level, engine, entities)
            room(id at bloc) {
                place door at (2.5, 0.0)
                place window at (1.5, 0.0)
                place window at (3.5, 0.0)
                place desk at (2.5, 3.5)
                place wait at (3.5, 0.5)
                place wait at (3.5, 1.5)
                place wait at (4.5, 0.5)
                place wait at (4.5, 1.5)
            }
        }
        level.finalize_room::<entity::ClientEntityCreator, _>(engine, entities, id)?;
    }
    // Lecture room
    {
        let bloc = base_loc + (1, 7);
        let id = assume!(log, level.place_room_id::<entity::ClientEntityCreator, _>(
            engine, entities,
            tmp_room_id, player,
            ResourceKey::new("base", "lecture_room"),
            Bound::new(
                bloc,
                bloc + (4, 4)
            )
        ));
        let id = level.finalize_placement(id);
        place_objects! {
            init(level, engine, entities)
            room(id at bloc) {
                place door at (2.5, 4.5)
                place window at (1.5, 4.5)
                place window at (3.5, 4.5)
                place projector at (4.5, 2.5)
            }
        }

        for y in 0 .. 3 {
            for x in 0 .. 3 {
                place_objects! {
                    init(level, engine, entities)
                    room(id at bloc) {
                        place chair at (0.75 + x as f32 * 0.75, 1.25 + y as f32) rotated(3)
                    }
                }
            }
        }
        level.finalize_room::<entity::ClientEntityCreator, _>(engine, entities, id)?;
    }

    Ok(level)
}


pub struct MainMenuState {
    ui: Option<ui::Node>,
}

pub struct DummyInstance {
    pub entities: ecs::Container,
    pub level: Level,
}

impl MainMenuState {
    pub fn new() -> MainMenuState {
        MainMenuState {
            ui: None,
        }
    }
}

impl state::State for MainMenuState {
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(MainMenuState {
            ui: self.ui.clone(),
        })
    }

    fn takes_focus(&self) -> bool { true }

    fn active(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState) -> state::Action {
        let node = state.ui_manager.create_node(ResourceKey::new("base", "menus/main_menu"));
        self.ui = Some(node.clone());
        state.renderer.set_mouse_sprite(ResourceKey::new("base", "ui/cursor/normal"));

        state.audio.set_playlist("menu");

        // If the assets folder contains more than just the default, allow for publishing
        let assets_folders = assume!(state.global_logger, std::fs::read_dir("./assets"))
            .flat_map(|v| v.ok())
            .filter(|v| v.file_type().ok().map_or(false, |v| v.is_dir()))
            .filter(|v| v.file_name() != "base" && v.file_name() != "packed")
            .count();
        if assets_folders > 0 {
            if let Some(buttons) = query!(node, menu_buttons).next() {
                buttons.set_property("buttons", 7);
                buttons.add_child_first(node! {
                    button(on_click="init#ui.emit_event('switch_menu', 'modding')".to_owned()) {
                        content {
                            @text("Modding".to_owned())
                        }
                    }
                });
            }
        }

        state::Action::Nothing
    }

    fn inactive(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState) {
        if let Some(node) = self.ui.take() {
            state.ui_manager.remove_node(node);
        }
        state.audio.update_settings(&state.config);
    }
}

#[cfg(feature = "steam")]
pub struct ModMenuState {
    ui: Option<ui::Node>,
    start_watcher: Option<mpsc::Receiver<steamworks::UpdateWatchHandle<steamworks::ClientManager>>>,
}

#[cfg(feature = "steam")]
impl ModMenuState {
    pub fn new() -> ModMenuState {
        ModMenuState {
            ui: None,
            start_watcher: None,
        }
    }
}

#[cfg(feature = "steam")]
impl state::State for ModMenuState {
    fn copy(&self) -> Box<dyn state::State> {
        unimplemented!()
    }

    fn takes_focus(&self) -> bool { true }

    fn active(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState) -> state::Action {
        let ui = state.ui_manager.create_node(ResourceKey::new("base", "menus/modding"));


        let assets_folders = assume!(state.global_logger, std::fs::read_dir("./assets"))
            .flat_map(|v| v.ok())
            .filter(|v| v.file_type().ok().map_or(false, |v| v.is_dir()))
            .filter(|v| v.file_name() != "base" && v.file_name() != "packed");

        if let Some(mod_list) = query!(ui, mod_list > scroll_panel > content).next() {
            for folder in assets_folders {
                let name = if let Ok(name) = folder.file_name().into_string() {
                    name
                } else {
                    continue
                };
                let name_copy = name.clone();
                let evt = ui::MethodDesc::<ui::MouseUpEvent>::native(move |evt, _node, _| {
                    evt.emit(UploadMod(name.clone()));
                    true
                });
                let node = node! {
                    mod_entry {
                        name {
                            @text(name_copy)
                        }
                        button(id="upload".to_owned(), on_click=evt) {
                            content {
                                @text("Upload")
                            }
                        }
                    }
                };
                mod_list.add_child(node);
            }
        }

        self.ui = Some(ui);
        state::Action::Nothing
    }

    fn inactive(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState) {
        if let Some(node) = self.ui.take() {
            state.ui_manager.remove_node(node);
        }
    }

    fn tick(&mut self, _instance: &mut Option<GameInstance>, _state: &mut GameState) -> state::Action {
        if let Some(watcher) = self.start_watcher.as_mut().and_then(|v| v.try_recv().ok()) {
            return state::Action::Push(Box::new(UploadWait {
                ui: None,
                watcher,
            }));
        }
        state::Action::Nothing
    }

    fn ui_event(&mut self, _instance: &mut Option<GameInstance>, state: &mut crate::GameState, evt: &mut event::EventHandler) -> state::Action {
        let mut action = state::Action::Nothing;
        let ui = assume!(state.global_logger, self.ui.as_ref());
        evt.handle_event_if::<super::AcceptEvent, _, _>(|evt| evt.0.is_same(&ui), |_| {
            action = state::Action::Pop;
        });
        evt.handle_event::<UploadMod, _>(|evt| {
            let (tx, rx) = mpsc::channel();
            self.start_watcher = Some(rx);
            do_upload(&state.global_logger, tx, state.steam.clone(), evt.0);
        });
        action
    }
}

#[cfg(feature = "steam")]
fn do_upload(log: &Logger, sender: mpsc::Sender<steamworks::UpdateWatchHandle<steamworks::ClientManager>>, steam: steamworks::Client, name: String) {
    use std::fs;
    let ugc = steam.ugc();

    // Look for existing mod info to work with
    let meta_path = Path::new("assets").join(&name).join("meta.json")
        .to_owned();
    let info: server::ModMeta = if let Ok(meta) = fs::File::open(&meta_path)
        .map_err(errors::Error::from)
        .and_then(|v| serde_json::from_reader(v).map_err(errors::Error::from))
    {
        info!(log, "Using existing meta for {}", name);
        meta
    } else {
        let log = log.clone();
        info!(log, "Creating new item for {}", name);
        ugc.create_item(crate::STEAM_APP_ID, steamworks::FileType::Community, move |res| {
            match res {
                Ok((id, _needs_sign)) => {
                    let meta = assume!(log, fs::File::create(meta_path.clone()));
                    assume!(log, serde_json::to_writer_pretty(meta, &server::ModMeta {
                        main: name.clone(),
                        workshop_id: id,
                    }));
                    let sender = sender.clone();
                    do_upload(&log, sender, steam.clone(), name.clone());
                },
                Err(err) => error!(log, "Failed to create mod on steam: {:?}", err),
            }
        });
        return;
    };

    info!(log, "{:#?}", info);
    let log = log.clone();
    let watcher = ugc.start_item_update(crate::STEAM_APP_ID, info.workshop_id)
        .title(&name)
        .content_path(&Path::new("assets").join(&name))
        .submit(None, move |res| {
            info!(log, "{:?}", res);
            steam.friends().activate_game_overlay_to_web_page(&format!("steam://url/CommunityFilePage/{}", info.workshop_id.0));
        });
    let _ = sender.send(watcher);
}

#[cfg(feature = "steam")]
struct UploadMod(String);

#[cfg(feature = "steam")]
pub(crate) struct UploadWait {
    ui: Option<ui::Node>,
    watcher: steamworks::UpdateWatchHandle<steamworks::ClientManager>,
}

#[cfg(feature = "steam")]
impl state::State for UploadWait {
    fn copy(&self) -> Box<dyn state::State> {
        unimplemented!()
    }

    fn takes_focus(&self) -> bool { true }

    fn active(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState) -> state::Action {
        let node = state.ui_manager.create_node(ResourceKey::new("base", "prompt/info"));

        if let Some(title) = query!(node, title > @text).next() {
            title.set_text("Uploading");
        }
        if let Some(description) = query!(node, description > @text).next() {
            description.set_text("Uploading...");
        }

        self.ui = Some(node);

        state::Action::Nothing
    }

    fn inactive(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState) {
        if let Some(node) = self.ui.take() {
            state.ui_manager.remove_node(node);
        }
    }

    fn tick(&mut self, _instance: &mut Option<GameInstance>, _state: &mut GameState) -> state::Action {
        let node = self.ui.as_ref().unwrap();
        let (status, progress, total) = self.watcher.progress();
        if status == steamworks::UpdateStatus::Invalid {
            return state::Action::Pop;
        }
        if let Some(description) = query!(node, description > @text).next() {
            description.set_text(format!("{:?} {}/{}", status, progress, total));
        }
        state::Action::Nothing
    }
}

#[cfg(feature = "steam")]
pub(crate) struct ModDownloadWait {
    pub(crate) ui: Option<ui::Node>,
}

#[cfg(feature = "steam")]
impl state::State for ModDownloadWait {
    fn copy(&self) -> Box<dyn state::State> {
        unimplemented!()
    }

    fn takes_focus(&self) -> bool { true }

    fn active(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState) -> state::Action {
        let node = state.ui_manager.create_node(ResourceKey::new("base", "prompt/info"));

        if let Some(title) = query!(node, title > @text).next() {
            title.set_text("Waiting for mods to download");
        }
        if let Some(description) = query!(node, description > @text).next() {
            description.set_text("Waiting...");
        }

        self.ui = Some(node);

        state::Action::Nothing
    }

    fn inactive(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState) {
        if let Some(node) = self.ui.take() {
            state.ui_manager.remove_node(node);
        }
    }

    fn tick(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState) -> state::Action {
        let mut waiting_for_workshop = false;
        let ugc = state.steam.ugc();
        for item in ugc.subscribed_items() {
            let item_state = ugc.item_state(item);
            if item_state.contains(steamworks::ItemState::NEEDS_UPDATE) {
                waiting_for_workshop = true;
                // No need to check the rest
                break;
            }
        }
        info!(state.global_logger, "Wait: {}", waiting_for_workshop);
        if !waiting_for_workshop {
            state.should_restart = true;
        }
        state::Action::Nothing
    }
}