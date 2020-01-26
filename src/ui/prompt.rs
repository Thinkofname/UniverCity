//! States for prompts

use std::rc::Rc;

use crate::server;

use crate::prelude::*;
use crate::{GameState, GameInstance};
use crate::state;
use crate::ui;
use crate::instance;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ConfirmResponse {
    Accept,
    Cancel,
}

pub(crate) struct Confirm<F> {
    ui: Option<ui::Node>,
    config: ConfirmConfig,
    reply: Rc<F>,
}

#[derive(Clone)]
pub(crate) struct ConfirmConfig {
    pub title: String,
    pub description: String,
    pub accept: String,
    pub cancel: String,
}

impl Default for ConfirmConfig {
    fn default() -> Self {
        ConfirmConfig {
            title: "Confirmation".into(),
            description: "".into(),
            accept: "Accept".into(),
            cancel: "Cancel".into(),
        }
    }
}

impl <F> Confirm<F>
    where F: Fn(ConfirmResponse) + 'static
{
    pub(crate) fn new(config: ConfirmConfig, reply: F) -> Confirm<F>
    {
        Confirm {
            ui: None,
            config,
            reply: Rc::new(reply),
        }
    }
}

impl <F> state::State for Confirm<F>
    where F: Fn(ConfirmResponse) + 'static
{
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(Confirm {
            ui: self.ui.clone(),
            config: self.config.clone(),
            reply: self.reply.clone(),
        })
    }

    fn takes_focus(&self) -> bool { true }

    fn active(&mut self, _instance: &mut Option<GameInstance>, state: &mut GameState) -> state::Action {
        let node = state.ui_manager.create_node(ResourceKey::new("base", "prompt/confirm"));

        if let Some(title) = query!(node, title > @text).next() {
            title.set_text(self.config.title.as_str());
        }
        if let Some(description) = query!(node, description > @text).next() {
            description.set_text(self.config.description.as_str());
        }
        if let Some(accept) = query!(node, button(id="accept") > content > @text).next() {
            accept.set_text(self.config.accept.as_str());
        }
        if let Some(cancel) = query!(node, button(id="cancel") > content > @text).next() {
            cancel.set_text(self.config.cancel.as_str());
        }

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
            action = state::Action::Pop;
            (self.reply)(ConfirmResponse::Accept);
        });
        evt.handle_event_if::<instance::CancelEvent, _, _>(|evt| evt.0.is_same(&ui), |_| {
            action = state::Action::Pop;
            (self.reply)(ConfirmResponse::Cancel);
        });
        action
    }
}