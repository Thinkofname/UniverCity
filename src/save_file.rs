
use crate::server;

use crate::prelude::*;
use super::{GameState, GameInstance};
use crate::state;
use crate::ui;
use crate::instance;
use chrono::prelude::*;
use crate::server::saving::SaveType;
use crate::server::saving::filesystem::FileSystem;

use std::time::SystemTime;
use std::rc::Rc;
use std::path::PathBuf;

pub(crate) struct MenuState<F> {
    ui: Option<ui::Node>,
    selected_item: usize,
    save_type: SaveType,
    start_func: Rc<F>,
}

impl <F> MenuState<F>
    where F: Fn(&mut crate::GameState, &str) -> Box<dyn state::State> + 'static
{
    pub(crate) fn new(save_type: SaveType, start_func: F) -> MenuState<F> {
        MenuState {
            ui: None,
            selected_item: 0,
            save_type,
            start_func: Rc::new(start_func),
        }
    }
}

impl <F> state::State for MenuState<F>
    where F: Fn(&mut crate::GameState, &str) -> Box<dyn state::State> + 'static
{
    fn copy(&self) -> Box<dyn state::State> {
        panic!("Save file menu isn't clonable (Shouldn't be used during networking")
    }

    fn takes_focus(&self) -> bool { true }

    fn active(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState) -> state::Action {
        // Look for save files, if none skip straight to the creation screen
        let mut save_files = state.filesystem
            .files()
            .into_iter()
            .filter(|v| !v.contains('/'))
            .map(|v| PathBuf::from(v))
            .filter(|v| v.extension().map_or(false, |v| v == "usav"))
            .filter_map(|file| {
                let name = assume!(state.global_logger, file.file_stem()).to_string_lossy();

                let valid = server::saving::can_load(&state.filesystem, &name, self.save_type);
                if valid.as_ref().map(|v| *v).unwrap_or(true) {
                    Some((file, valid))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        if save_files.is_empty() {
            return state::Action::Switch(Box::new(NewSaveState::new(self.save_type, self.start_func.clone())));
        }

        let node = state.ui_manager.create_node(ResourceKey::new("base", "menus/singleplayer"));
        if let Some(new_game) = query!(node, button(id="new_game")).next() {
            new_game.set_property("on_click", ui::MethodDesc::<ui::MouseUpEvent>::native(|evt, _, _| {
                evt.emit(NewGame);
                true
            }));
        }
        if let Some(load_game) = query!(node, button(id="load_game")).next() {
            load_game.set_property("on_click", ui::MethodDesc::<ui::MouseUpEvent>::native(|evt, _, _| {
                evt.emit(LoadGame);
                true
            }));
        }

        save_files.sort_by_key(|(path, _)| {
            let dt: DateTime<Local> = state.filesystem.timestamp(&*path.to_string_lossy())
                .unwrap_or_else(|_| SystemTime::now().into());
            dt
        });
        save_files.reverse();

        if let Some(content) = query!(node, save_list > scroll_panel > content).next() {
            for (idx, (path, valid)) in save_files.into_iter().enumerate() {
                let name = assume!(state.global_logger, path.file_stem()).to_string_lossy();

                let dt: DateTime<Local> = state.filesystem.timestamp(&*path.to_string_lossy())
                    .unwrap_or_else(|_| SystemTime::now().into());
                let is_valid = valid.is_ok();

                let entry = node! {
                    save_entry(selected=idx == self.selected_item, entry=idx as i32, name=name.clone().into_owned(), valid=is_valid) {
                        name {
                            @text(if let Err(err) = valid { format!("{} - {}", name, err) } else { name.into_owned() })
                        }
                        time {
                            @text(dt.format("%H:%M %d/%m/%Y").to_string())
                        }
                        delete_save {
                            @text("X")
                        }
                    }
                };
                if is_valid {
                    entry.set_property("on_click", ui::MethodDesc::<ui::MouseUpEvent>::native(move |evt, _, _| {
                        evt.emit(SelectEntry(idx));
                        true
                    }));
                }
                if let Some(delete) = query!(entry, delete_save).next() {
                    delete.set_property("on_click", ui::MethodDesc::<ui::MouseUpEvent>::native(move |evt, _, _| {
                        evt.emit(DeleteEntry(idx));
                        true
                    }));
                }
                content.add_child(entry);
            }
        }

        self.ui = Some(node);
        state.ui_manager.events.borrow_mut().emit(SelectEntry(0));

        state::Action::Nothing
    }

    fn inactive(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState) {
        if let Some(node) = self.ui.take() {
            state.ui_manager.remove_node(node);
        }
    }

    fn ui_event(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState, evt: &mut server::event::EventHandler) -> state::Action {
        use std::io;

        let mut action = state::Action::Nothing;
        let ui = assume!(state.global_logger, self.ui.clone());
        evt.handle_event::<NewGame, _>(|_| {
            action = state::Action::Switch(Box::new(NewSaveState::new(self.save_type, self.start_func.clone())));
        });
        evt.handle_event::<LoadGame, _>(|_| {
            if let Some(cur) = query!(ui, save_entry(entry=self.selected_item as i32)).next() {
                let name = assume!(state.global_logger, cur.get_property_ref::<String>("name"));
                if cur.get_property::<bool>("valid").unwrap_or(false) {
                    action = state::Action::Switch((self.start_func)(state, &*name));
                }
            }
        });
        evt.handle_event::<DeleteEntry, _>(|DeleteEntry(idx)| {
            if let Some(cur) = query!(ui, save_entry(entry=idx as i32)).next() {
                let fs = crate::make_filesystem(#[cfg(feature = "steam")] &state.steam);
                let name = assume!(state.global_logger, cur.get_property::<String>("name"));
                action = state::Action::Push(Box::new(ui::prompt::Confirm::new(
                    ui::prompt::ConfirmConfig {
                        description: format!("Are you sure you wish to delete \"{}\"?", name),
                        accept: "Delete".into(),
                        ..ui::prompt::ConfirmConfig::default()
                    },
                    move |rpl| {
                        if rpl == ui::prompt::ConfirmResponse::Accept {
                            let _ = server::saving::delete_save(&fs, &name);
                        }
                    }
                )));
            }
        });
        evt.handle_event::<SelectEntry, _>(|SelectEntry(idx)| {
            if let Some(old) = query!(ui, save_entry(entry=self.selected_item as i32)).next() {
                old.set_property("selected", false);
            }
            self.selected_item = idx;
            if let Some(new) = query!(ui, save_entry(entry=self.selected_item as i32)).next() {
                new.set_property("selected", true);
            }

            if let Some(cur) = query!(ui, save_entry(entry=self.selected_item as i32)).next() {
                let name = assume!(state.global_logger, cur.get_property::<String>("name"));

                let data = if let Some(icon) = server::saving::get_icon(&state.filesystem, &name) {
                    let dec = png::Decoder::new(io::Cursor::new(icon));
                    let (info, mut reader) = assume!(state.global_logger, dec.read_info());
                    let mut buf = vec![0; info.buffer_size()];
                    assume!(state.global_logger, reader.next_frame(&mut buf));
                    buf
                } else {
                    vec![0; 800 * 600 * 3]
                };

                let mut rgba = vec![0; 800 * 600 * 4];
                for (i, o) in data.chunks_exact(3).zip(rgba.chunks_exact_mut(4)) {
                    o[3] = 255;
                    o[0] = i[0];
                    o[1] = i[1];
                    o[2] = i[2];
                }

                state.renderer.update_image(ResourceKey::new("dynamic", "800@600@save_icon"), 800, 600, rgba);
            }
        });
        action
    }
}

struct NewGame;
struct LoadGame;
struct SelectEntry(usize);
struct DeleteEntry(usize);

struct NewSaveState<F> {
    ui: Option<ui::Node>,
    save_type: SaveType,
    start_func: Rc<F>,
}

impl <F> NewSaveState<F>
    where F: Fn(&mut crate::GameState, &str) -> Box<dyn state::State> + 'static,
{
    fn new(save_type: SaveType, start_func: Rc<F>) -> NewSaveState<F> {
        NewSaveState {
            ui: None,
            save_type,
            start_func,
        }
    }
}

impl <F> state::State for NewSaveState<F>
    where F: Fn(&mut crate::GameState, &str) -> Box<dyn state::State> + 'static,
{
    fn copy(&self) -> Box<dyn state::State> {
        panic!("Save file menu isn't clonable (Shouldn't be used during networking")
    }

    fn takes_focus(&self) -> bool { true }

    fn active(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState) -> state::Action {
        let node = state.ui_manager.create_node(ResourceKey::new("base", "menus/new_save"));
        self.ui = Some(node);

        state::Action::Nothing
    }

    fn inactive(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState) {
        if let Some(node) = self.ui.take() {
            state.ui_manager.remove_node(node);
        }
    }

    fn ui_event(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState, evt: &mut server::event::EventHandler) -> state::Action {
        let mut action = state::Action::Nothing;
        let ui = assume!(state.global_logger, self.ui.clone());
        evt.handle_event_if::<instance::AcceptEvent, _, _>(|evt| evt.0.is_same(&ui), |_| {
            let name = query!(ui, textbox(id="name") > content > @text).next();
            let name = name.as_ref()
                .and_then(|v| v.text());
            let name = name
                .as_ref()
                .map_or("", |v| &*v);
            let err_msg = if !name.is_empty() {
                fn validate_name(name: &str) -> Option<String> {
                    if name.len() > 200 {
                        return Some(format!("Name too long: {} > 200", name.len()));
                    }
                    for c in name.chars() {
                        match c {
                            // FAT*, NTFS limits
                            _c @ '\x00' ..= '\x1F' => return Some("Invalid characeter".to_owned()),
                            '"' | '*' | '/' | ':'
                              | '<' | '>' | '?' | '\\'
                              | '|' | '+' | ',' | '.'
                              | ';' | '=' | '[' | ']' => return Some(format!("Can't contain special character: {}", c)),
                            _ => {},
                        }
                    }
                    match name.to_lowercase().as_str() {
                        "$idle$"
                        | "aux"
                        | "con"
                        | "config$"
                        | "clock$"
                        | "keybd$"
                        | "lst"
                        | "nul"
                        | "prn"
                        | "screen$"
                        => return Some("Can't use reserved word".to_owned()),

                        name if (name.starts_with("com") ||
                            name.starts_with("lpt")) && name.len() == 4 => return Some("Can't use reserved word".to_owned()),
                        _ => {}
                    }
                    None
                }

                if let Some(err) = validate_name(&name) {
                    Some(err)
                } else {
                    let valid = server::saving::can_load(&state.filesystem, &name, self.save_type);
                    if let Err(server::errors::Error(server::errors::ErrorKind::NoSuchSave, _)) = valid {
                        action = state::Action::Switch((self.start_func)(state, name));
                        None
                    } else {
                        Some("A save with that name already exists".to_owned())
                    }
                }
            } else {
                Some("File name cannot be empty".to_owned())
            };
            if let Some(err_msg) = err_msg {
                if let Some(error_box) = query!(ui, new_save_error).next() {
                    error_box.set_property("show", true);
                    let txt = assume!(state.global_logger, query!(error_box, @text).next());
                    txt.set_text(err_msg);
                }
            }
        });
        action
    }
}