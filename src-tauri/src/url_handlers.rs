//! The links installed games registered in their prefixes (`bf3lan://`,
//! `lanlauncher_core::launch::url_protocol`), offered to the desktop with
//! this launcher as their handler, and looked up when one arrives.
//!
//! Linux only: the desktop asks `mimeapps.list`, which can change at any
//! time. On macOS the schemes a program takes are fixed in its bundle; on
//! Windows the game's own registration does the job.

use crate::state::AppState;
use lanlauncher_core::launch::url_protocol::{self, Handler};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

/// Per game: when its registries last changed, and what they register.
type Known = BTreeMap<String, (Option<SystemTime>, Vec<Handler>)>;

fn known() -> &'static Mutex<Known> {
    static KNOWN: OnceLock<Mutex<Known>> = OnceLock::new();
    KNOWN.get_or_init(Default::default)
}

/// A link the desktop handed over when no launcher was running: this start
/// is the launcher, and opens it once the games are known.
pub static PENDING: OnceLock<String> = OnceLock::new();

/// The event that carries the error code of a link that did not open.
pub const LINK_ERROR_EVENT: &str = "link-error";

/// Open `url` (`--open-url`) as the command line would, from a thread of
/// its own: `control::run` blocks on the async runtime.
fn open(app: &tauri::AppHandle, url: String) {
    let app = app.clone();
    std::thread::spawn(move || {
        let _ = crate::control::run(&app, lanlauncher_core::control::Command::OpenUrl(url));
    });
}

/// [`PENDING`], once the library knows its games.
pub async fn open_pending(app: &tauri::AppHandle, state: &Arc<AppState>) {
    let Some(url) = PENDING.get() else {
        return;
    };
    for _ in 0..60 {
        if !state.catalog().await.games.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    open(app, url.clone());
}

/// Read again what changed since the last look, for every installed game:
/// a setup that just ran, a game removed. Only the registries' times are
/// looked at for the rest. `None` while there is no catalog to name the
/// games (still loading, no sync server): that is no reason to take the
/// links back.
async fn refresh(state: &Arc<AppState>) -> Option<Known> {
    let catalog = state.catalog().await;
    if catalog.games.is_empty() {
        return None;
    }
    let library = state.settings.read().await.library.clone();
    let games: Vec<(String, lanlauncher_core::paths::GamePaths)> = catalog
        .games
        .iter()
        .filter_map(|g| library.game_paths(&g.id).map(|p| (g.id.clone(), p)))
        .collect();
    tauri::async_runtime::spawn_blocking(move || {
        let mut known = known().lock().unwrap_or_else(|e| e.into_inner());
        let mut now = Known::new();
        for (id, paths) in games {
            if !paths.receipt.is_file() {
                continue;
            }
            let changed = url_protocol::registries_changed(&paths.share_dir);
            if changed.is_none() {
                continue;
            }
            let entry = match known.remove(&id) {
                Some((at, handlers)) if at == changed => (at, handlers),
                _ => (changed, url_protocol::handlers_of(&paths.share_dir)),
            };
            now.insert(id, entry);
        }
        *known = now.clone();
        now
    })
    .await
    .ok()
}

/// The game that registered `scheme`, and what it registered. Two games
/// with the same scheme: the first by id, as the desktop can name only one
/// handler and the game's prefix decides what runs.
pub async fn find(state: &Arc<AppState>, scheme: &str) -> Option<(String, Handler)> {
    refresh(state)
        .await?
        .into_iter()
        .find_map(|(id, (_, handlers))| {
            handlers
                .into_iter()
                .find(|h| h.scheme == scheme)
                .map(|h| (id, h))
        })
}

/// Where the desktop starts this launcher: the AppImage, not the program
/// inside its mount, which is gone once it ends.
fn exec() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| std::env::current_exe().ok())
}

fn xdg(name: &str, fallback: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(fallback)))
}

/// Keep the desktop's handlers in step with the installed games, once a
/// minute: the setup that registers a scheme runs after an install, the
/// prefix's registry file is written by Wine when it gets to it, and a new
/// version of the launcher lies somewhere else.
pub fn keep_offered(state: Arc<AppState>) {
    if !cfg!(target_os = "linux") || state.demo {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let mut offered: Option<Vec<String>> = None;
        loop {
            tokio::time::sleep(Duration::from_secs(if offered.is_none() { 20 } else { 60 })).await;
            let Some(known) = refresh(&state).await else {
                continue;
            };
            let mut schemes: Vec<String> = known
                .into_values()
                .flat_map(|(_, handlers)| handlers.into_iter().map(|h| h.scheme))
                .collect();
            schemes.sort();
            schemes.dedup();
            let (Some(exec), Some(data), Some(config)) = (
                exec(),
                xdg("XDG_DATA_HOME", ".local/share"),
                xdg("XDG_CONFIG_HOME", ".config"),
            ) else {
                continue;
            };
            let result = tauri::async_runtime::spawn_blocking({
                let schemes = schemes.clone();
                move || url_protocol::register(&schemes, &exec, &data, &config)
            })
            .await;
            match result {
                Ok(Ok(true)) => log::info!("links offered to the desktop: {schemes:?}"),
                Ok(Ok(false)) => {}
                Ok(Err(e)) if offered.as_ref() != Some(&schemes) => {
                    log::warn!("links not offered to the desktop: {e}")
                }
                _ => {}
            }
            offered = Some(schemes);
        }
    });
}
