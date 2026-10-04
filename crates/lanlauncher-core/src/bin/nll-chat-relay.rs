//! A memory for the LAN chat: runs on a machine that is on all weekend (the
//! sync server, say), takes part in the chat as a relay and hands every
//! launcher that starts later what was said before. Private messages are
//! kept sealed — the relay cannot read them — and delivered when their
//! recipient comes online, even if the sender has gone by then.
//!
//! ```text
//! nll-chat-relay [--data <dir>] [--name <name>]
//! ```
//!
//! Needs UDP and TCP port 41950 open, like every launcher.

use lanlauncher_core::chat::{Chat, ChatConfig, CHAT_PORT};
use std::path::PathBuf;

/// Log lines to standard error, `NLL_LOG=debug` for more.
struct Stderr(log::LevelFilter);

impl log::Log for Stderr {
    fn enabled(&self, meta: &log::Metadata) -> bool {
        meta.level() <= self.0
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!(
                "{} {:<5} {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

const USAGE: &str = "nll-chat-relay [--data <dir>] [--name <name>]

Keeps the NextGen LAN Launcher chat for everyone who starts later.
  --data <dir>   where history and identity live (default: ./nll-chat-relay)
  --name <name>  name shown in the launchers (default: Chat-Archiv)";

fn main() {
    let level = match std::env::var("NLL_LOG").as_deref() {
        Ok("debug") => log::LevelFilter::Debug,
        _ => log::LevelFilter::Info,
    };
    let logger: &'static Stderr = Box::leak(Box::new(Stderr(level)));
    let _ = log::set_logger(logger).map(|()| log::set_max_level(level));

    let mut data = PathBuf::from("nll-chat-relay");
    let mut name = "Chat-Archiv".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match (arg.as_str(), args.next()) {
            ("--data", Some(v)) => data = PathBuf::from(v),
            ("--name", Some(v)) => name = v,
            _ => {
                eprintln!("{USAGE}");
                std::process::exit(if arg == "--help" || arg == "-h" { 0 } else { 2 });
            }
        }
    }

    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    runtime.block_on(async move {
        let mut config = ChatConfig::new(data.clone(), &name);
        config.relay = true;
        let chat = match Chat::start(config).await {
            Ok(chat) => chat,
            Err(e) => {
                log::error!("cannot start: {e}");
                std::process::exit(1);
            }
        };
        let snapshot = chat.snapshot();
        if let Some(problem) = &snapshot.problem {
            log::error!("{problem}: UDP port {CHAT_PORT} is taken, launchers will not find this relay");
        }
        log::info!(
            "relay \"{}\" running, data in {}; Ctrl+C stops it",
            snapshot.nick,
            data.display()
        );
        // Public messages need nothing; private ones only go to a relay the
        // LANPage vouches for, since any machine could call itself one.
        // launcher.ini is a block format; a `key = value` line would make
        // the launchers drop the whole file.
        log::info!(
            "to keep private messages for absent players too, add this block to the LANPage's launcher.ini:\nchat_relay ### NextGen chat relay {{\n{}\n}}",
            snapshot.me
        );
        let _ = tokio::signal::ctrl_c().await;
        log::info!("stopping");
        chat.stop().await;
    });
}
