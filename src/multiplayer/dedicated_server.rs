use super::*;

pub(crate) struct MenuState {
    ui: Option<ui::Node>,
    error: Option<String>,
}

impl MenuState {
    pub(crate) fn new<S: Into<Option<String>>>(error: S) -> MenuState {
        MenuState {
            ui: None,
            error: error.into(),
        }
    }
}

impl MultiplayerReturn for MenuState {
    fn return_ok() -> Self {
        MenuState::new(None)
    }
    fn return_error<S: Into<String>>(err: S) -> Self {
        MenuState::new(Some(err.into()))
    }
}

impl state::State for MenuState {
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(MenuState {
            ui: self.ui.clone(),
            error: self.error.clone(),
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
            .create_node(ResourceKey::new("base", "menus/multiplayer_server"));
        if let Some(error) = self.error.as_ref() {
            if let Some(error_box) = query!(node, server_connect_error).next() {
                error_box.set_property("show", true);
                let txt = assume!(state.global_logger, query!(error_box, @text).next());
                txt.set_text(error.as_str());
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

    fn ui_event(
        &mut self,
        _instance: &mut Option<GameInstance>,
        state: &mut GameState,
        evt: &mut server::event::EventHandler,
    ) -> state::Action {
        let mut action = state::Action::Nothing;
        evt.handle_event::<MultiPlayer, _>(|MultiPlayer(addr)| {
            use std::net::ToSocketAddrs;
            fn parse_addr(addr: &str) -> errors::Result<net::SocketAddr> {
                if addr.contains(':') {
                    addr.to_socket_addrs()?
                        .next()
                        .ok_or_else(|| errors::ErrorKind::AddressResolveError)
                        .map_err(Into::into)
                } else {
                    (addr, 23_347)
                        .to_socket_addrs()?
                        .next()
                        .ok_or_else(|| errors::ErrorKind::AddressResolveError)
                        .map_err(Into::into)
                }
            }
            match parse_addr(&addr) {
                Ok(addr) => {
                    action = state::Action::Switch(Box::new(ConnectingState::<
                        MenuState,
                        network::UdpClientSocket,
                        _,
                    >::new(
                        move |_state| network::UdpClientSocket::connect(addr),
                    )))
                }
                Err(err) => {
                    let ui = assume!(state.global_logger, self.ui.clone());
                    if let Some(error_box) = query!(ui, server_connect_error).next() {
                        error_box.set_property("show", true);
                        let txt = assume!(state.global_logger, query!(error_box, @text).next());
                        txt.set_text(format!("{}", err));
                    }
                }
            };
        });
        action
    }
}
