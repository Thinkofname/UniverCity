use super::{GameInstance, GameState};
use crate::prelude::*;
use crate::state;
use crate::ui;
use std::collections::HashSet;

use serde_json;
use std::fs;

pub(crate) struct MenuState {
    ui: Option<ui::Node>,
}

impl MenuState {
    pub(crate) fn new() -> MenuState {
        MenuState { ui: None }
    }
}

impl state::State for MenuState {
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(MenuState {
            ui: self.ui.clone(),
        })
    }

    fn takes_focus(&self) -> bool {
        true
    }

    fn active(
        &mut self,
        _instance: &mut Option<GameInstance>,
        state: &mut GameState,
    ) -> state::Action {
        let node = state
            .ui_manager
            .create_node(ResourceKey::new("base", "menus/credits"));

        let licenses =
            fs::File::open("licenses/licenses.json").expect("Missing license information");

        let mut deps: Vec<DepInfo> =
            serde_json::from_reader(licenses).expect("Failed to parse license information");

        // Special ones not picked up by the script
        deps.push(DepInfo {
            name: "Rust's stdlib".into(),
            version: "-".into(),
            license: "MIT".into(),
            from: Some("https://www.rust-lang.org/".into()),
        });
        deps.push(DepInfo {
            name: "LuaJIT".into(),
            version: "2".into(),
            license: "MIT".into(),
            from: Some("http://luajit.org".into()),
        });
        deps.push(DepInfo {
            name: "SDL2".into(),
            version: "2.0.5".into(),
            license: "ZLIB".into(),
            from: Some("https://www.libsdl.org/".into()),
        });
        deps.push(DepInfo {
            name: "fungui".into(),
            version: "latest".into(),
            license: "MIT".into(),
            from: Some("https://github.com/thinkofname/fungui".into()),
        });
        deps.push(DepInfo {
            name: "delta_encode".into(),
            version: "latest".into(),
            license: "MIT".into(),
            from: Some("https://github.com/thinklibs/delta_encode".into()),
        });
        deps.push(DepInfo {
            name: "think_bitio".into(),
            version: "latest".into(),
            license: "MIT".into(),
            from: Some("https://github.com/thinklibs/think_bitio".into()),
        });

        if let Some(content) = query!(node, scroll_panel > content).next() {
            for dep in deps {
                let licenses = dep
                    .license
                    .split('/')
                    .flat_map(|v| v.split("OR"))
                    .map(|v| v.trim())
                    .collect::<HashSet<_>>();

                // In order of preferance
                let license = if licenses.contains("MIT") {
                    "MIT License"
                } else if licenses.contains("Apache-2.0") {
                    "Apache License 2.0"
                } else if licenses.contains("BSD-3-Clause") {
                    "BSD 3-Clause License"
                } else if licenses.contains("BSD-2-Clause") {
                    "BSD 2-Clause License"
                } else if licenses.contains("FTL") {
                    "Freetype Project License"
                } else if licenses.contains("MPL-2.0") {
                    "Mozilla Public License 2.0"
                } else if licenses.contains("ZLIB") || licenses.contains("Zlib") {
                    "ZLib License"
                } else if licenses.contains("BSD-3-Clause AND Zlib") {
                    "BSD 3-Clause License and the ZLib License"
                } else if licenses.contains("ISC") {
                    "ISC License"
                } else if licenses.contains("CC0-1.0") {
                    continue;
                } else {
                    panic!("Unhandled licenses: {:?}", licenses)
                };

                let name = dep.name;
                let version = dep.version;
                let from = dep
                    .from
                    .unwrap_or_else(|| format!("https://crates.io/crates/{}", name));

                let node = node! {
                    credit_library {
                        content {
                            @text(" version: ")
                            @text(version)
                        }
                    }
                };
                let name = ui::Node::new_text(name);
                name.set_property("library", true);
                assume!(
                    state.global_logger,
                    query!(node, credit_library > content).next()
                )
                .add_child_first(name);
                content.add_child(node);

                let node = node! {
                    credit_library_license {
                        content {
                            @text("Under the ")
                        }
                    }
                };
                let license = ui::Node::new_text(license);
                license.set_property("license", true);
                assume!(
                    state.global_logger,
                    query!(node, credit_library_license > content).next()
                )
                .add_child(license);
                content.add_child(node);

                content.add_child(node! {
                    credit_library_url {
                        content {
                            @text(from)
                        }
                    }
                });

                content.add_child(node!(credit_spacer_small));
            }
        }

        self.ui = Some(node);

        state::Action::Nothing
    }

    fn inactive(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState) {
        if let Some(node) = self.ui.take() {
            state.ui_manager.remove_node(node);
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct DepInfo {
    name: String,
    version: String,
    license: String,
    #[serde(default)]
    from: Option<String>,
}
