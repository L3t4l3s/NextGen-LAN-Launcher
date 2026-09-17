//! Tauri shell around `lanlauncher-core`.

mod commands;
mod fixes;
mod state;

use lanlauncher_core::catalog::Catalog;
use lanlauncher_core::install::{InstallManager, ManifestSetupHook, SetupHook};
use lanlauncher_core::library::Library;
use lanlauncher_core::manifest::{Manifest, ManifestStore};
use lanlauncher_core::paths::{AppDirs, GamePaths};
use lanlauncher_core::settings::{Settings, TransportMode};
use lanlauncher_core::transport::demo::DemoTransport;
use lanlauncher_core::transport::folder::FolderTransport;
use lanlauncher_core::transport::resilio::{self, ResilioConfig, ResilioTransport};
use lanlauncher_core::transport::Transport;
use state::AppState;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, Manager};
use tokio::sync::RwLock;

pub const STATUS_EVENT: &str = "install-status";
pub const HEALTH_EVENT: &str = "transport-health";
/// Rates and peer count, once a second: the status bar should move while
/// something is moving, and the full health is too expensive for that.
pub const RATES_EVENT: &str = "transport-rates";
pub const EVENT_UPDATED: &str = "event-updated";
/// Emitted with the new game count after `game.db` changed on disk.
pub const CATALOG_EVENT: &str = "catalog-updated";
/// How often the catalog watcher looks at `game.db`/`assets.eti`.
const CATALOG_POLL: u64 = 10;
/// Reload every five minutes even when the files look unchanged. The launcher
/// has no "refresh" button, so this loop is the only path to a fresh list.
const CATALOG_BLIND_RELOAD_TICKS: u32 = (5 * 60) / CATALOG_POLL as u32;

/// `<semver> (<commit>)`, shown in the status bar and the first log line so
/// installers of the same version can be told apart.
pub(crate) fn app_version() -> String {
    format!("{} ({})", env!("CARGO_PKG_VERSION"), env!("NLL_BUILD_ID"))
}

fn is_demo() -> bool {
    std::env::args().any(|a| a == "--demo")
        || std::env::var("LANLAUNCHER_DEMO")
            .map(|v| v == "1")
            .unwrap_or(false)
}

/// Platform post-extraction hook: Windows runs `game_setup.cmd`, everyone
/// applies manifest steps. Holds the state weakly because the state owns the
/// manager that owns this hook.
struct AppSetupHook {
    state: std::sync::Weak<AppState>,
}

#[async_trait::async_trait]
impl SetupHook for AppSetupHook {
    async fn run_setup(
        &self,
        paths: &GamePaths,
        manifest: Option<&Manifest>,
    ) -> lanlauncher_core::Result<()> {
        ManifestSetupHook.run_setup(paths, manifest).await?;
        if !cfg!(target_os = "windows") {
            return Ok(());
        }
        let Some(state) = self.state.upgrade() else {
            return Ok(());
        };
        let (lang, player) = {
            let s = state.settings.read().await;
            (s.game_language.clone(), s.safe_player_name())
        };
        let game_id = paths
            .share_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        use lanlauncher_core::launch::{self, elevate};
        // One elevated run per install: the firewall rules the start script
        // would add every time (so it can run as a normal user later) plus
        // the optional game_setup.cmd. Adopted or older installs get their
        // rules at the first start instead (fixes::ensure_firewall_rules).
        let rules = std::fs::read_to_string(&paths.start_script)
            .map(|t| elevate::firewall_add_rules(&t, &paths.share_dir, game_id, &lang, &player))
            .unwrap_or_default();
        let setup = launch::windows::setup_plan(paths, game_id, &lang, &player);
        let mut lines: Vec<elevate::BatchLine> = rules.iter().cloned().map(Into::into).collect();
        if let Some(plan) = &setup {
            // The game's own script keeps the console: it prints instructions
            // and may wait for a key press. The rules above are logged, so a
            // rule a policy refuses still names itself in the error.
            lines.push(elevate::BatchLine::console(elevate::batch_line(plan)));
        }
        if lines.is_empty() {
            return Ok(());
        }
        // The setup script is the last line of the batch; the transcript
        // numbers its sections the same way, so a failure can be tied to it.
        let setup_line = lines.len();
        let run = crate::fixes::run_admin_lines_at(
            &state,
            &format!("{game_id}-setup"),
            lines,
            &paths.share_dir,
        )
        .await;
        // What ran, and what the transcript holds: "the setup script reported
        // an error" without either is a dead end for the user.
        // Only a failure: a setup that worked has nothing to explain, and the
        // panel belongs to whatever the user started last.
        let mut transcript = String::new();
        if let (Some(plan), Err(e)) = (&setup, &run) {
            let log = elevate::batch_log_path(&state.run_dir(), &format!("{game_id}-setup"));
            let mut attempt = crate::state::LaunchAttempt::new(game_id, game_id, "setup", plan);
            attempt.elevated = true;
            attempt.ended = true;
            attempt.error = Some(e.clone());
            attempt.output = state.launch_output_since(&log, attempt.at);
            attempt.captured = !attempt.output.is_empty();
            transcript = attempt.output.clone();
            *state.last_launch.write().await = Some(attempt);
        }
        let run = run.map(|_| ());
        match run {
            Ok(()) => {
                if !rules.is_empty() {
                    let marker = elevate::firewall_marker(&state.dirs.data, game_id);
                    if let Some(dir) = marker.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    let _ = std::fs::write(marker, "");
                }
                Ok(())
            }
            // Rules only and no rights: not a failure of the game's setup;
            // the first start asks again.
            Err(e) if setup.is_none() => {
                log::warn!("firewall rules for {game_id} not registered at setup: {e}");
                Ok(())
            }
            Err(e) => Err(lanlauncher_core::Error::Code(explain_setup_failure(
                paths,
                &transcript,
                setup_line,
                e,
            ))),
        }
    }
}

/// What a failed setup was looking for, where that can be said for certain.
///
/// ETI's scripts call helpers from the original launcher's installation
/// (`%programfiles%\eti\lan launcher\unrar.exe`, `fnr.exe`). On a machine
/// that never had it cmd answers "The system cannot find the path specified"
/// and names nothing, and that is what the user was shown: an error nobody
/// can act on. The script says which programs it wanted, so the missing ones
/// are named instead.
///
/// Only when the game's own script is what failed: the same batch registers
/// the firewall rules first, and a `netsh` that a policy refused must not be
/// reported as a missing helper. Where the transcript does not say which line
/// it was, the original code stands.
fn explain_setup_failure(
    paths: &GamePaths,
    transcript: &str,
    setup_line: usize,
    code: String,
) -> String {
    if lanlauncher_core::launch::elevate::failed_line(transcript) != Some(setup_line) {
        return code;
    }
    let Ok(script) = std::fs::read_to_string(&paths.setup_script) else {
        return code;
    };
    let missing = lanlauncher_core::launch::windows::missing_tools(
        &script,
        &|name| std::env::var(name).ok(),
        &|path| path.exists(),
    );
    if missing.is_empty() {
        return code;
    }
    let list = missing
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    format!("msg.setup_tools_missing|{list}")
}

/// Only the preview-video folders of the library are exposed to the WebView
/// through the asset protocol; `game.db` (share keys) and game files stay out
/// of reach. Folders of roots that were removed are revoked again.
pub(crate) fn update_media_scope(
    app: &tauri::AppHandle,
    old_roots: &[lanlauncher_core::library::LibraryRoot],
    library: &Library,
) {
    let scope = app.asset_protocol_scope();
    for root in old_roots {
        if !library.roots.iter().any(|r| r.path == root.path) {
            for dir in lanlauncher_core::catalog::video_dirs(&root.path) {
                let _ = scope.forbid_directory(&dir, false);
            }
        }
    }
    for root in &library.roots {
        for dir in lanlauncher_core::catalog::video_dirs(&root.path) {
            let _ = scope.allow_directory(&dir, false);
        }
    }
}

/// A folder shipped in `bundle.resources`. `tauri dev` runs from `src-tauri`
/// without a bundle, so the repository folder is the fallback.
fn bundled_dir(resource_dir: Option<&Path>, name: &str, dev_relative: &str) -> Option<PathBuf> {
    resource_dir
        .map(|r| r.join(name))
        .filter(|p| p.is_dir())
        .or_else(|| {
            let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(dev_relative);
            dev.is_dir().then_some(dev)
        })
}

pub(crate) fn demo_catalog() -> Catalog {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(include_str!("../demo/demo_catalog.sql"))
        .expect("demo catalog sql");
    Catalog::from_connection(&conn).expect("demo catalog")
}

/// Load the catalog from the default library root, if present.
/// `extract_covers` re-unpacks `assets.eti` into the cover cache; skip it when
/// only `game.db` changed.
pub(crate) fn load_catalog_from_library(
    library: &Library,
    dirs: &AppDirs,
    extract_covers: bool,
) -> Option<Catalog> {
    let root = library.default_root()?;
    let db = root.path.join(lanlauncher_core::paths::CATALOG_RELATIVE);
    let catalog = Catalog::load(&db).ok()?;
    // Both lists exist only in the LAN's catalog; log them so a report from a
    // test PC shows what the share offers (the UI for tools is not built yet).
    if !catalog.tools.is_empty() {
        let tools: Vec<String> = catalog
            .tools
            .iter()
            .map(|t| {
                format!(
                    "{} ({}{}{})",
                    t.name,
                    t.id,
                    t.size
                        .as_deref()
                        .map(|s| format!(", {s}"))
                        .unwrap_or_default(),
                    if t.disabled { ", disabled" } else { "" }
                )
            })
            .collect();
        log::info!("catalog tools: {}", tools.join(" | "));
    }
    let video_dirs = lanlauncher_core::catalog::video_dirs(&root.path);
    let videos: usize = video_dirs
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .map(|rd| rd.flatten().count())
        .sum();
    log::info!(
        "videos: {videos} files under {}",
        video_dirs
            .iter()
            .map(|d| d.display().to_string())
            .collect::<Vec<_>>()
            .join(" | ")
    );
    let assets = root.path.join(lanlauncher_core::paths::ASSETS_RELATIVE);
    if extract_covers && assets.is_file() {
        match lanlauncher_core::catalog::extract_covers(&assets, &dirs.covers_dir()) {
            Ok(n) => log::info!("covers: {n} extracted from {}", assets.display()),
            Err(e) => log::warn!("covers: cannot extract {}: {e}", assets.display()),
        }
    } else if extract_covers {
        log::info!("covers: {} not present yet", assets.display());
    }
    Some(catalog)
}

/// (size, mtime) of a file, `None` when absent.
pub(crate) type Stamp = Option<(u64, Option<std::time::SystemTime>)>;
/// Stamps of `game.db` and `assets.eti` under the default root.
pub(crate) type CatalogSig = (Stamp, Stamp);

pub(crate) fn catalog_signature(root: Option<&std::path::Path>) -> CatalogSig {
    let stamp = |p: PathBuf| -> Stamp {
        let meta = std::fs::metadata(p).ok()?;
        Some((meta.len(), meta.modified().ok()))
    };
    match root {
        Some(r) => (
            stamp(r.join(lanlauncher_core::paths::CATALOG_RELATIVE)),
            stamp(r.join(lanlauncher_core::paths::ASSETS_RELATIVE)),
        ),
        None => (None, None),
    }
}

/// Re-read the catalog from the default library root and hand it to the
/// install manager. `None` when no readable `game.db` exists (yet). On
/// success the file signature is recorded so the watcher stays quiet.
/// `adopt` runs the install manager's `adopt_existing()` afterwards; the
/// periodic reload passes `false`, because re-adopting without a changed
/// catalog can resurrect a download the user just cancelled (the tracker is
/// gone, a leftover archive is not).
pub(crate) async fn reload_catalog(
    state: &AppState,
    extract_covers: bool,
    adopt: bool,
) -> Option<usize> {
    // One reload at a time; a caller that queued behind another one still
    // runs (an explicit refresh must not be swallowed).
    let _serial = state.catalog_reload.lock().await;
    let library = state.settings.read().await.library.clone();
    let sig = catalog_signature(library.default_root().map(|r| r.path.as_path()));
    // Claim the signature before the (slow) load so the watcher does not
    // start a second extraction of the same assets.eti meanwhile; on failure
    // the previous signature is restored. Only this function writes the
    // signature while the reload lock is held, so the claim is ours.
    let previous = state
        .catalog_sig
        .lock()
        .map(|mut s| std::mem::replace(&mut *s, sig))
        .ok();
    let dirs = state.dirs.clone();
    // SQLite open + archive extraction are blocking work; keep them off the
    // async runtime threads.
    let catalog = tauri::async_runtime::spawn_blocking(move || {
        load_catalog_from_library(&library, &dirs, extract_covers)
    })
    .await
    .ok()
    .flatten();
    let Some(catalog) = catalog else {
        if let (Some(prev), Ok(mut s)) = (previous, state.catalog_sig.lock()) {
            if *s == sig {
                *s = prev;
            }
        }
        return None;
    };
    let n = catalog.games.len();
    if let Some(m) = state.manager.read().await.as_ref() {
        m.set_catalog(catalog).await;
        if adopt {
            m.adopt_existing().await;
        }
    }
    Some(n)
}

/// Managed mode only: make Resilio sync the catalog share
/// `<default root>/eti_launcher`, which carries `game.db`, covers and videos
/// and whose peers tell whether a sync server is present. Idempotent (the
/// engine ignores re-adding a known folder) and non-fatal: failures are
/// logged and the health then reports `server_found: None`.
pub(crate) async fn register_catalog_share(state: &AppState) {
    let Some(transport) = state.transport.read().await.clone() else {
        return;
    };
    if transport.kind() != lanlauncher_core::transport::TransportKind::Resilio {
        return;
    }
    // Key, LAN mode and root come from one settings snapshot so a concurrent
    // save cannot pair a new key with an old directory.
    let (key, lan_only, dir) = {
        let s = state.settings.read().await;
        (
            lanlauncher_core::catalog::catalog_share_key(s.catalog_key.as_deref()),
            s.lan_mode,
            s.library
                .default_root()
                .map(|r| r.path.join(lanlauncher_core::paths::LAUNCHER_SHARE_ID)),
        )
    };
    let Some(key) = key else {
        log::warn!("catalog share key not configured; eti_launcher is not synced");
        return;
    };
    let Some(dir) = dir else {
        log::warn!("no library root configured; catalog share not registered");
        return;
    };
    let opts = lanlauncher_core::transport::ShareOptions {
        lan_only,
        paused: false,
    };
    match transport.add_share(&key, &dir, &opts).await {
        Ok(()) => log::info!("catalog share registered at {}", dir.display()),
        Err(e) => log::warn!("cannot register catalog share at {}: {e}", dir.display()),
    }
}

/// Resilio API key in order of precedence: the setting, an installed ETI
/// client's config on this PC, `resilio_api_key` from the LANPage's
/// launcher.ini. `None` means the web-UI endpoints are used instead.
async fn resolve_api_key(state: &AppState, setting: Option<String>) -> Option<String> {
    if let Some(k) = setting {
        log::info!("Resilio API key: from settings");
        return Some(k);
    }
    let from_eti = tauri::async_runtime::spawn_blocking(|| {
        resilio::eti_client_api_key(&resilio::WinEnv::program_files_from_env())
    })
    .await
    .ok()
    .flatten();
    if let Some((k, file)) = from_eti {
        log::info!("Resilio API key: from ETI client config {}", file.display());
        return Some(k);
    }
    let from_page = state
        .event
        .read()
        .await
        .config
        .as_ref()
        .and_then(|c| c.extra.get("resilio_api_key").cloned())
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty());
    match &from_page {
        Some(_) => log::info!("Resilio API key: from launcher.ini"),
        None => log::info!("Resilio API key: none; using the web-UI endpoints"),
    }
    from_page
}

pub(crate) async fn build_transport(state: &AppState) -> (Arc<dyn Transport>, Option<String>) {
    match state.effective_transport_mode().await {
        TransportMode::Demo => {
            let mut demo = DemoTransport::new();
            demo.duration = Duration::from_secs(25);
            demo.stuck_at_99 = true;
            demo.archive_source = Some(demo_archive_path(state));
            (Arc::new(demo), None)
        }
        TransportMode::Folder => (Arc::new(FolderTransport::new()), None),
        TransportMode::Managed => {
            let (lan_only, port, override_path, key_setting) = {
                let settings = state.settings.read().await;
                (
                    settings.lan_mode,
                    settings.sync_port,
                    settings.resilio_binary.clone(),
                    settings.resilio_api_key.clone(),
                )
            };
            let api_key = resolve_api_key(state, key_setting).await;
            // The search may run `reg query`; keep it off the async threads.
            let resource_dir = state.resource_dir.clone();
            let data_dir = state.dirs.data.clone();
            let located = tauri::async_runtime::spawn_blocking(move || {
                resilio::locate_binary_detailed(
                    override_path.as_deref(),
                    resource_dir.as_deref(),
                    &data_dir,
                )
            })
            .await
            .unwrap_or_default();
            match &located.found {
                Some(b) => log::info!("Resilio binary: {}", b.display()),
                None => log::warn!(
                    "Resilio binary not found; probed {} paths: {}",
                    located.probed.len(),
                    located
                        .probed
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(" | ")
                ),
            }
            match located.found.clone() {
                Some(binary) => {
                    let mut cfg = ResilioConfig::new(binary, state.dirs.transport_dir());
                    cfg.lan_only = lan_only;
                    cfg.listening_port = port;
                    cfg.api_key = api_key;
                    let t = ResilioTransport::new(cfg);
                    match t.start().await {
                        Ok(()) => (Arc::new(t), None),
                        Err(e) => (
                            Arc::new(FolderTransport::new()),
                            Some(format!("err.resilio_start_failed|{e}")),
                        ),
                    }
                }
                None => (
                    Arc::new(FolderTransport::new()),
                    Some(format!("err.resilio_not_found|{}", located.probed.len())),
                ),
            }
        }
    }
}

/// Demo mode ships a tiny real RAR so verify/extract run end to end.
fn demo_archive_path(state: &AppState) -> PathBuf {
    let p = state.dirs.data.join("demo").join("demo_amongus.rar");
    if !p.exists() {
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let _ = std::fs::write(
            &p,
            include_bytes!("../../crates/lanlauncher-core/tests/fixtures/demo_amongus.rar"),
        );
    }
    p
}

pub(crate) fn build_manager(
    state: &Arc<AppState>,
    transport: Arc<dyn Transport>,
    catalog: Catalog,
    library: Arc<std::sync::RwLock<Library>>,
    lan_only: bool,
) -> Arc<InstallManager> {
    let lib_for_paths = library.clone();
    let manifests = state.manifests.clone();
    let mut manager = InstallManager::new(
        transport,
        catalog,
        move |game| {
            // Cheap on purpose: this runs for every tracked game on every
            // tick, and asking the volumes for their free space per game per
            // tick is what `game_paths_for` is for — the install command asks
            // once and creates the folder, and that is what is found here.
            lib_for_paths
                .read()
                .ok()
                .and_then(|l| l.game_paths(&game.id))
        },
        move |game, paths| manifests.resolve_for(&game.id, paths),
        Arc::new(AppSetupHook {
            state: Arc::downgrade(state),
        }),
    );
    manager.lan_only = lan_only;
    Arc::new(manager)
}

/// WebKitGTK 2.42 and newer draw through DMA-BUF. Several Linux graphics
/// stacks answer that with nothing at all: the window appears, with the right
/// title, and stays white — seen on SteamOS on a Steam Deck, and reported for
/// the same combination by every other GTK webview app.
///
/// Turning the DMA-BUF renderer off is the fix, and it is applied everywhere
/// on Linux rather than guessed per machine: a white window makes the
/// launcher useless, while the cost is accelerated compositing — noticeable
/// at most on the preview video of a game's detail page. Whoever knows their
/// machine draws fine sets `WEBKIT_DISABLE_DMABUF_RENDERER=0` and keeps it.
///
/// Called first thing in [`run`], before the webview exists and while the
/// process is still single-threaded.
#[cfg(target_os = "linux")]
fn prefer_a_renderer_that_draws() {
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
    // The window opens, the interface runs, and the machine still paints
    // nothing: no program can see that from the inside, so this one is a
    // switch. `--safe-graphics` turns everything off that a graphics stack
    // can fail at and is remembered, `--no-safe-graphics` forgets it again.
    let asked = std::env::args().any(|a| a == "--safe-graphics");
    let cancelled = std::env::args().any(|a| a == "--no-safe-graphics");
    let marker = safe_graphics_marker();
    if let Some(marker) = &marker {
        if asked {
            if let Some(dir) = marker.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(marker, "Started with --safe-graphics.\n");
        } else if cancelled {
            let _ = std::fs::remove_file(marker);
        }
    }
    let remembered = marker.map(|m| m.is_file()).unwrap_or(false);
    if (asked || remembered) && !cancelled {
        for (key, value) in safe_rendering_env() {
            std::env::set_var(key, value);
        }
        log::info!("safe graphics: software rendering, no compositing");
    }
}

/// Where the `--safe-graphics` choice is remembered. Written before the Tauri
/// paths exist, so the XDG location is built by hand — the same directory
/// Tauri's `app_config_dir()` returns for this identifier.
#[cfg(target_os = "linux")]
fn safe_graphics_marker() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config"))
        })?;
    Some(base.join("xyz.nextgen-lan.launcher").join("safe-graphics"))
}

/// Set when the interface has reported for duty (`frontend_ready`). The
/// webview's own "page loaded" is no use for this: it fires for the error
/// page too, so a launcher whose interface never ran would look fine.
pub(crate) static FRONTEND_READY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Marks the run that already took the safe rendering path, so the restart
/// below happens once and never turns into a loop.
#[cfg(target_os = "linux")]
const FALLBACK_MARKER: &str = "NLL_WEBVIEW_FALLBACK";

/// Everything that makes WebKitGTK draw on a machine where it otherwise does
/// not — accelerated rendering off, software GL, X11 rather than Wayland.
/// Slow, and it does not matter: this is a page of text and boxes, and the
/// alternative is a white window.
#[cfg(target_os = "linux")]
fn safe_rendering_env() -> Vec<(&'static str, String)> {
    let mut env = vec![
        ("WEBKIT_DISABLE_DMABUF_RENDERER", "1".to_string()),
        ("WEBKIT_DISABLE_COMPOSITING_MODE", "1".to_string()),
        ("LIBGL_ALWAYS_SOFTWARE", "1".to_string()),
        (FALLBACK_MARKER, "1".to_string()),
    ];
    // Only with an X server to fall back to: forcing x11 in a pure Wayland
    // session without XWayland would not start at all.
    if std::env::var_os("DISPLAY").is_some() {
        env.push(("GDK_BACKEND", "x11".to_string()));
    }
    env
}

/// Start this launcher again with the safe rendering settings. `Some` once
/// the new process is on its way; the caller then ends this one.
#[cfg(target_os = "linux")]
fn restart_with_safe_rendering() -> Option<()> {
    let exe = std::env::current_exe().ok()?;
    // Inside an AppImage the extracted binary is gone once this process ends;
    // the AppImage itself is the thing to start again.
    let program = std::env::var_os("APPIMAGE")
        .map(std::path::PathBuf::from)
        .unwrap_or(exe);
    let mut cmd = std::process::Command::new(&program);
    cmd.args(std::env::args().skip(1));
    for (key, value) in safe_rendering_env() {
        cmd.env(key, value);
    }
    match cmd.spawn() {
        Ok(child) => {
            log::warn!(
                "restarted as {} (pid {}) with software rendering",
                program.display(),
                child.id()
            );
            Some(())
        }
        Err(e) => {
            log::error!("could not restart {}: {e}", program.display());
            None
        }
    }
}

/// Watch for a webview that never shows anything.
///
/// The window is there, the title is right, the page never arrives: on Linux
/// that is the webview's renderer or its web process, and the user sees white.
/// The launcher then starts itself once more with everything that makes
/// WebKitGTK draw, and only says so when that did not help either — a message
/// box comes from the window manager rather than from the webview, so it is
/// visible even then.
fn warn_about_a_blank_window(app: &tauri::AppHandle) {
    use std::sync::atomic::Ordering;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Long enough for a cold start on a slow disk and for the retries of
        // the report itself: anything shorter risks restarting a launcher
        // that was merely still starting.
        for _ in 0..30 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if FRONTEND_READY.load(Ordering::Relaxed) {
                return;
            }
        }
        log::error!("the interface did not report for duty within 30 s");
        #[cfg(target_os = "linux")]
        if std::env::var_os(FALLBACK_MARKER).is_none() {
            // The successor shares the sync engine's folder, pid file and
            // port, so this one lets go of the engine first — a successor
            // that finds it still running would kill it as an orphan instead
            // of letting it shut down through its API. The window is blank
            // either way, so nothing is lost if the restart then fails.
            if let Some(state) = app.try_state::<Arc<AppState>>() {
                if let Some(transport) = state.transport.write().await.take() {
                    let _ = transport.stop().await;
                }
            }
            if restart_with_safe_rendering().is_some() {
                app.exit(0);
                return;
            }
        }
        let hint = if cfg!(target_os = "linux") {
            "The window stayed empty: the webview (WebKitGTK) did not render, \
             not even with software rendering. Start the launcher from a \
             terminal to see its messages; docs/TROUBLESHOOTING.md lists what \
             else to try."
        } else {
            "The window stayed empty: the webview did not load the interface. \
             The log folder holds the details."
        };
        use tauri_plugin_dialog::DialogExt;
        app.dialog()
            .message(hint)
            .title("NextGen LAN Launcher")
            .blocking_show();
    });
}

pub fn run() {
    #[cfg(target_os = "linux")]
    prefer_a_renderer_that_draws();
    tauri::Builder::default()
        .on_page_load(|window, payload| {
            // Which URL the webview actually loaded — a production build
            // serves `tauri://localhost`, a dev build the vite server; the
            // difference explains an empty window on its own.
            log::info!("webview loaded {} in {}", payload.url(), window.label());
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("launcher".into()),
                    }),
                ])
                .build(),
        )
        .setup(|app| {
            let demo = is_demo();
            let dirs = AppDirs {
                config: app.path().app_config_dir()?,
                data: app.path().app_data_dir()?,
                cache: app.path().app_cache_dir()?,
                // Same directory the log plugin's `LogDir` target uses.
                logs: app.path().app_log_dir()?,
            };
            log::info!(
                "NextGen LAN Launcher {} starting; log dir {}",
                app_version(),
                dirs.logs.display()
            );
            warn_about_a_blank_window(app.handle());
            // A white window on Linux is almost always the webview's DMA-BUF
            // renderer; the log should say which way this run went.
            #[cfg(target_os = "linux")]
            log::info!(
                "webview: WEBKIT_DISABLE_DMABUF_RENDERER={}, compositing={}, software GL={}, \
                 session {}, backend {}, safe-mode restart {}",
                std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").unwrap_or_else(|_| "unset".into()),
                std::env::var("WEBKIT_DISABLE_COMPOSITING_MODE").unwrap_or_else(|_| "unset".into()),
                std::env::var("LIBGL_ALWAYS_SOFTWARE").unwrap_or_else(|_| "unset".into()),
                std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".into()),
                std::env::var("GDK_BACKEND").unwrap_or_else(|_| "default".into()),
                std::env::var("NLL_WEBVIEW_FALLBACK").unwrap_or_else(|_| "no".into())
            );
            #[cfg(target_os = "linux")]
            log::info!(
                "webkit helpers: exec {}, bundle {}",
                std::env::var("WEBKIT_EXEC_PATH").unwrap_or_else(|_| "system".into()),
                std::env::var("WEBKIT_INJECTED_BUNDLE_PATH").unwrap_or_else(|_| "system".into())
            );
            for d in [&dirs.config, &dirs.data, &dirs.cache] {
                let _ = std::fs::create_dir_all(d);
            }
            // Tauri hands out a verbatim path (`\\?\C:\…`) on Windows. Windows
            // accepts it, `netsh` does not, so every path derived from it is
            // normalised here rather than at each use.
            let resource_dir = app
                .path()
                .resource_dir()
                .ok()
                .map(lanlauncher_core::paths::strip_verbatim);
            let mut settings = Settings::load(&dirs.settings_file()).unwrap_or_default();
            if demo {
                let demo_root = dirs.data.join("demo-library");
                let _ = std::fs::create_dir_all(&demo_root);
                settings.library = Library::default();
                settings
                    .library
                    .add_root(lanlauncher_core::library::LibraryRoot::new(demo_root));
                settings.setup_complete = true;
                if settings.player_name.is_empty() {
                    settings.player_name = "DemoPlayer".into();
                }
                settings.transport = TransportMode::Demo;
            }
            let bundled_manifests =
                bundled_dir(resource_dir.as_deref(), "manifests", "../manifests");
            let manifests =
                ManifestStore::new(Some(dirs.data.join("manifests")), bundled_manifests);
            let bundled_covers = bundled_dir(resource_dir.as_deref(), "covers", "../assets/covers");
            match &bundled_covers {
                Some(p) => {
                    // Cover images are served through the asset protocol,
                    // whose static scope only covers the app data dirs.
                    match app.asset_protocol_scope().allow_directory(p, false) {
                        Ok(()) => log::info!("bundled covers: {}", p.display()),
                        Err(e) => log::warn!(
                            "bundled covers at {} cannot be served (asset scope): {e}",
                            p.display()
                        ),
                    }
                }
                None => log::warn!("bundled covers not found next to the app"),
            }
            let library = Arc::new(std::sync::RwLock::new(settings.library.clone()));
            update_media_scope(app.handle(), &[], &settings.library);
            let state = Arc::new(AppState {
                library,
                dirs,
                demo,
                settings: RwLock::new(settings),
                transport: RwLock::new(None),
                manager: RwLock::new(None),
                event: RwLock::new(Default::default()),
                manifests,
                resource_dir,
                bundled_covers,
                running: RwLock::new(Vec::new()),
                last_launch: RwLock::new(None),
                transport_error: RwLock::new(None),
                catalog_sig: std::sync::Mutex::new((None, None)),
                catalog_reload: tokio::sync::Mutex::new(()),
                startup_catalog: RwLock::new(None),
            });
            app.manage(state.clone());
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                start_services(handle, state).await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_bootstrap,
            commands::get_games,
            commands::get_statuses,
            commands::install_game,
            commands::repair_game,
            commands::pause_game,
            commands::cancel_download,
            commands::uninstall_game,
            commands::play_game,
            commands::run_extra,
            commands::get_prereq_installer,
            commands::run_prereq_installer,
            commands::get_launch_plan,
            commands::list_executables,
            commands::set_exe_override,
            commands::get_settings,
            commands::save_settings,
            commands::frontend_ready,
            commands::get_last_launch,
            commands::rerun_setup,
            commands::run_diagnostics,
            commands::set_problem_ignored,
            commands::apply_fix,
            commands::get_transport_health,
            commands::open_path,
            commands::open_url,
            commands::get_share_key,
            commands::get_share_peers,
            commands::get_library_space,
            commands::restart_transport,
        ])
        .build(tauri::generate_context!())
        .expect("error while running NextGen LAN Launcher")
        .run(|handle, event| {
            // The sync engine is our own child process. Leaving it behind
            // would block the next start (Resilio allows one instance per
            // program file) and keep syncing unnoticed.
            if matches!(event, tauri::RunEvent::Exit) {
                let Some(state) = handle.try_state::<Arc<AppState>>() else {
                    return;
                };
                let state = state.inner().clone();
                tauri::async_runtime::block_on(async move {
                    let transport = state.transport.read().await.clone();
                    if let Some(t) = transport {
                        log::info!("shutting the sync engine down");
                        if tokio::time::timeout(Duration::from_secs(10), t.stop())
                            .await
                            .is_err()
                        {
                            log::warn!("sync engine did not stop within 10 s");
                        }
                    }
                });
            }
        });
}

/// Fetch launcher.ini, launcher.css, logo.png (and theme.json) from the
/// LANPage host into the shared state and tell the UI.
async fn refresh_event(state: &AppState, app: &tauri::AppHandle) {
    if state.demo {
        return;
    }
    let host = state.settings.read().await.lanpage_host.clone();
    let bundle = lanlauncher_core::lanpage::fetch_event(&host).await;
    log::info!(
        "lanpage {host}: fetched [{}]{}",
        bundle.fetched.join(", "),
        if bundle.errors.is_empty() {
            String::new()
        } else {
            format!("; errors: {}", bundle.errors.join("; "))
        }
    );
    *state.event.write().await = bundle.clone();
    let _ = app.emit(EVENT_UPDATED, &bundle);
}

async fn start_services(app: tauri::AppHandle, state: Arc<AppState>) {
    let library = state.library.clone();
    // The catalog is loaded while the sync engine starts (which may take up
    // to its API timeout), so the library appears as early as possible.
    let catalog_task = {
        let st = state.clone();
        let library = library.clone();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let st2 = st.clone();
            let catalog = tauri::async_runtime::spawn_blocking(move || {
                if st2.demo {
                    return demo_catalog();
                }
                let lib = library.read().map(|l| l.clone()).unwrap_or_default();
                // Signature before the load: a game.db swapped in while the
                // load runs must show up as changed to the watcher.
                let sig = catalog_signature(lib.default_root().map(|r| r.path.as_path()));
                let loaded = load_catalog_from_library(&lib, &st2.dirs, true);
                if loaded.is_some() {
                    if let Ok(mut s) = st2.catalog_sig.lock() {
                        *s = sig;
                    }
                }
                loaded.unwrap_or_default()
            })
            .await
            .unwrap_or_default();
            // Visible before the manager exists: `AppState::catalog()` falls
            // back to this copy, and the UI is told to re-read the games.
            *st.startup_catalog.write().await = Some(catalog.clone());
            log::info!("catalog ready at start: {} games", catalog.games.len());
            let _ = app.emit(CATALOG_EVENT, catalog.games.len());
            catalog
        })
    };
    // launcher.ini may carry the Resilio API key (`resilio_api_key`), so the
    // LANPage is asked before the engine starts. The wait is bounded: a slow
    // or absent page must not hold the engine back, the 5-minute loop
    // fetches it later.
    if tokio::time::timeout(Duration::from_secs(5), refresh_event(&state, &app))
        .await
        .is_err()
    {
        log::info!("lanpage not answered within 5 s; starting the sync engine without it");
    }
    let (transport, error) = build_transport(&state).await;
    *state.transport_error.write().await = error;
    *state.transport.write().await = Some(transport.clone());
    register_catalog_share(&state).await;

    let catalog = catalog_task.await.unwrap_or_default();
    let lan_only = state.settings.read().await.lan_mode;
    let manager = build_manager(
        &state,
        transport.clone(),
        catalog,
        library.clone(),
        lan_only,
    );
    manager.adopt_existing().await;
    *state.manager.write().await = Some(manager.clone());
    // Statuses exist only now; the UI re-reads the games once more.
    let _ = app.emit(CATALOG_EVENT, 0usize);

    // Keep the shared library in sync with settings changes.
    let lib_sync = library.clone();
    let app_lib = app.clone();
    let st = state.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            {
                let s = st.settings.read().await;
                if let Ok(mut l) = lib_sync.write() {
                    if *l != s.library {
                        let old = std::mem::replace(&mut *l, s.library.clone());
                        update_media_scope(&app_lib, &old.roots, &s.library);
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });

    // Event configuration from the LANPage host, refreshed every 5 minutes
    // (the first fetch happened before the transport start, see above).
    let st = state.clone();
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(300)).await;
            refresh_event(&st, &app2).await;
        }
    });

    // Install ticks.
    let st = state.clone();
    let app3 = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            if let Some(m) = st.manager.read().await.clone() {
                let statuses = m.tick().await;
                log::debug!("tick: {} statuses", statuses.len());
                let _ = app3.emit(STATUS_EVENT, &statuses);
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });

    // Rates for the status bar, once a second: one `get_folders` against the
    // engine on localhost, without the peer count that costs a request per
    // share. The peer number comes from the health poll below.
    let st = state.clone();
    let app_rates = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let Some(t) = st.transport.read().await.clone() else {
                continue;
            };
            if let Ok(rates) = t.rates().await {
                let _ = app_rates.emit(RATES_EVENT, rates);
            }
        }
    });

    // Transport health.
    let st = state.clone();
    let app4 = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            if let Some(t) = st.transport.read().await.clone() {
                let health = t.health().await;
                let _ = app4.emit(HEALTH_EVENT, &health);
            }
            tokio::time::sleep(Duration::from_secs(15)).await;
        }
    });

    // Catalog reload: `game.db` and `assets.eti` arrive through the catalog
    // share minutes after start at a LAN; a size/mtime change of either
    // triggers a reload (covers are re-extracted when assets.eti changed).
    // There is no button for this: the loop is the only way the list is
    // refreshed, so it also reloads every few minutes without a visible
    // change — a rewrite within the same second and mtimes a file system
    // rounds off are otherwise invisible.
    let st = state.clone();
    let app5 = app.clone();
    tauri::async_runtime::spawn(async move {
        if st.demo {
            return;
        }
        // Signature of the last attempt made by this loop; a changed but
        // unloadable catalog is tried once per change, not every 10 s.
        let mut tried: CatalogSig = (None, None);
        // Ticks since the last load of any kind, for the blind reload.
        let mut idle_ticks = 0u32;
        loop {
            tokio::time::sleep(Duration::from_secs(CATALOG_POLL)).await;
            let root = st.default_root_path().await;
            let now = catalog_signature(root.as_deref());
            // `last` is the signature of the last successful load, wherever it
            // happened (startup, settings change, this loop). While nothing was
            // ever loaded, an unchanged game.db is retried: the file may have
            // been locked or half-synced at start. A vanished game.db keeps the
            // old catalog.
            let last = st.catalog_sig.lock().map(|s| *s).unwrap_or((None, None));
            let ever_loaded = last.0.is_some();
            let changed = now != last && (now != tried || !ever_loaded);
            idle_ticks += 1;
            let due = idle_ticks >= CATALOG_BLIND_RELOAD_TICKS;
            if now.0.is_some() && (changed || due || !ever_loaded) {
                idle_ticks = 0;
                tried = now;
                let assets_changed = now.1 != last.1 || !ever_loaded;
                // A reload nobody asked for is not worth a log line every five
                // minutes; only a real change is.
                let blind = !changed && ever_loaded;
                match reload_catalog(&st, assets_changed, !blind).await {
                    Some(n) => {
                        if blind {
                            log::debug!("catalog reloaded on schedule: {n} games");
                        } else {
                            log::info!("catalog reloaded: {n} games");
                        }
                        let _ = app5.emit(CATALOG_EVENT, n);
                    }
                    None if ever_loaded => {
                        log::warn!("catalog changed on disk but could not be loaded yet")
                    }
                    None => log::debug!("catalog still not loadable; retrying"),
                }
            }
        }
    });

    // Stats beacon (ETI LANPage compatible), every 3 minutes.
    let st = state.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(20)).await;
            let (enabled, player) = {
                let s = st.settings.read().await;
                (!st.demo, s.safe_player_name())
            };
            let url = st
                .event
                .read()
                .await
                .config
                .as_ref()
                .and_then(|c| c.stats_url.clone());
            if let (true, Some(url)) = (enabled, url) {
                let current = st.running.read().await.first().map(|(g, _)| g.clone());
                let report =
                    lanlauncher_core::lanpage::StatsReport::collect(&player, current.as_deref());
                if let Err(e) = report.send(&url).await {
                    log::warn!("stats beacon failed: {e}");
                }
            }
            tokio::time::sleep(Duration::from_secs(160)).await;
        }
    });
}
