#![windows_subsystem = "console"]

extern crate error_chain;
extern crate univercity_server as server;
#[macro_use]
extern crate slog;
extern crate slog_async;
extern crate slog_json;
extern crate slog_term;
#[macro_use]
extern crate univercity_util;
use server::steamworks;

use server::assets;
use server::network::UdpSocketListener;
use server::saving::filesystem::*;
use server::{Server, ServerConfig};
use slog::Drain;
use std::env;
use std::net::{self, SocketAddr};
use std::path::Path;
use std::sync::mpsc;
use std::thread;

fn main() -> server::errors::Result<()> {
    // This forces the dedicated server to use the game's appid even when launched
    // through the steam client.
    // Whilst I don't expect many to launch this that way apparently they test it
    // that way during the review.
    env::set_var("SteamAppId", "808160");

    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let json_drain = slog_json::Json::default(::std::fs::File::create("log.json").unwrap()).fuse();
    let (drain, _guard) =
        slog_async::Async::new(slog::Duplicate::new(drain, json_drain).fuse()).build_with_guard();
    let drain = drain.fuse();
    // TODO: Filter debug logging at some point
    let log = slog::Logger::root(
        drain,
        o!(
            "dedicated_server" => "true"
        ),
    );
    univercity_util::log_panics(&log, server::GAME_HASH, false);

    let asset_manager =
        server::register_loaders(assets::AssetManager::with_packs(&log, &["base".to_owned()]))
            .build();
    let addr: SocketAddr = assume!(log, "0.0.0.0:23347".parse());

    let ip = if let net::IpAddr::V4(ip) = addr.ip() {
        ip
    } else {
        panic!("IPv6 not supported with steamworks currently")
    };
    let port = addr.port();
    let (steam, _single) = assume!(
        log,
        steamworks::Server::init(
            ip,
            port + 1,
            port,
            port + 2,
            steamworks::ServerMode::Authentication,
            server::GAME_HASH
        )
    );

    steam.set_product("UniverCity");
    steam.set_game_description("default");
    steam.set_dedicated_server(true);
    steam.log_on_anonymous();

    let (cmd_send, cmd_recv) = mpsc::channel();
    thread::spawn(move || {
        use std::io::{stdin, BufRead};
        let stdin = stdin();
        let mut stdin = stdin.lock();
        let mut line = String::new();
        loop {
            line.clear();
            if stdin.read_line(&mut line).is_err() {
                let _ = cmd_send.send("quit".into());
                return;
            }
            let l = line.trim();
            if !l.is_empty() {
                if cmd_send.send(l.to_owned()).is_err() {
                    return;
                }
            }
        }
    });

    let fs = NativeFileSystem::new(Path::new("./saves/")).into_boxed();

    let (mut server, _) = Server::<UdpSocketListener, _>::new(
        log,
        asset_manager,
        steam,
        fs,
        addr,
        ServerConfig {
            save_type: server::saving::SaveType::ServerFreePlay,
            save_name: "dedicated".into(),
            min_players: 1,
            max_players: 0xFFFF,
            autostart: false,
            player_area_size: 100,
            locked_players: false,
            mission: None,
            tick_rate: std::cell::Cell::new(20),
        },
        None,
        Some(cmd_recv),
    )?;
    server.run();
    Ok(())
}
