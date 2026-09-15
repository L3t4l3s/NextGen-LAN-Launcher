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
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, Manager};
use tokio::sync::RwLock;

pub const STATUS_EVENT: &str = "install-status";
pub const HEALTH_EVENT: &str = "transport-health";
pub const EVENT_UPDATED: &str = "event-updated";
/// Emitted with the new game count after `game.db` changed on disk.
pub const CATALOG_EVENT: &str = "catalog-updated";

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
        let (lang, player) = match self.state.upgrade() {
            Some(state) => {
                let s = state.settings.read().await;
                (s.game_language.clone(), s.safe_player_name())
            }
            None => ("de".to_string(), String::new()),
        };
        let game_id = paths
            .share_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        let Some(plan) =
            lanlauncher_core::launch::windows::setup_plan(paths, game_id, &lang, &player)
        else {
            return Ok(());
        };
        // The script runs through cmd.exe with its own quoting rules, so the
        // command line is passed verbatim (see LaunchPlan::raw_command_line).
        let mut cmd = tokio::process::Command::new(&plan.program);
        lanlauncher_core::launch::apply_args(&mut cmd, &plan);
        let output = cmd
            .current_dir(&plan.cwd)
            .output()
            .await
            .map_err(|e| lanlauncher_core::Error::Launch(format!("game_setup.cmd: {e}")))?;
        if !output.status.success() {
            let tail = |b: &[u8]| {
                let s = String::from_utf8_lossy(b);
                s.chars()
                    .rev()
                    .take(4096)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<String>()
            };
            log::warn!(
                "game_setup.cmd for {game_id} exited with {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
                output.status,
                tail(&output.stdout),
                tail(&output.stderr)
            );
            return Err(lanlauncher_core::Error::Launch(format!(
                "game_setup.cmd exited with {}",
                output.status
            )));
        }
        Ok(())
    }
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

/// Re-read the catalog from the default library root and hand it to the
/// install manager. `None` when no readable `game.db` exists (yet).
pub(crate) async fn reload_catalog(state: &AppState, extract_covers: bool) -> Option<usize> {
    let library = state.settings.read().await.library.clone();
    let dirs = state.dirs.clone();
    // SQLite open + tar extraction are blocking work; keep them off the
    // async runtime threads.
    let catalog = tauri::async_runtime::spawn_blocking(move || {
        load_catalog_from_library(&library, &dirs, extract_covers)
    })
    .await
    .ok()??;
    let n = catalog.games.len();
    if let Some(m) = state.manager.read().await.as_ref() {
        m.set_catalog(catalog).await;
        m.adopt_existing().await;
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
            let (lan_only, port, override_path) = {
                let settings = state.settings.read().await;
                (
                    settings.lan_mode,
                    settings.sync_port,
                    settings.resilio_binary.clone(),
                )
            };
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
            let mut binary = located.found.clone();
            if binary.is_none() {
                match resilio::install_bundled_windows(
                    state.resource_dir.as_deref(),
                    &state.dirs.data,
                )
                .await
                {
                    Ok(found) => binary = found,
                    Err(e) => log::warn!("bundled Resilio installer failed: {e}"),
                }
            }
            match binary {
                Some(binary) => {
                    let mut cfg = ResilioConfig::new(binary, state.dirs.transport_dir());
                    cfg.lan_only = lan_only;
                    cfg.listening_port = port;
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
            lib_for_paths
                .read()
                .ok()
                .and_then(|l| l.game_paths(&game.id))
        },
        move |game, paths| match manifests.resolve(&game.id, Some(&paths.share_dir)) {
            Ok(Some(m)) => Some(m),
            _ => std::fs::read_to_string(&paths.start_script)
                .ok()
                .and_then(|s| {
                    lanlauncher_core::script_probe::ScriptProbe::analyse(&s).to_manifest(&game.id)
                }),
        },
        Arc::new(AppSetupHook {
            state: Arc::downgrade(state),
        }),
    );
    manager.lan_only = lan_only;
    Arc::new(manager)
}

pub fn run() {
    tauri::Builder::default()
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
            for d in [&dirs.config, &dirs.data, &dirs.cache] {
                let _ = std::fs::create_dir_all(d);
            }
            let resource_dir = app.path().resource_dir().ok();
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
            let bundled_manifests = resource_dir
                .as_ref()
                .map(|r| r.join("manifests"))
                .filter(|p| p.is_dir())
                .or_else(|| {
                    // `tauri dev` runs from src-tauri; fall back to the repository folder.
                    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../manifests");
                    dev.is_dir().then_some(dev)
                });
            let manifests =
                ManifestStore::new(Some(dirs.data.join("manifests")), bundled_manifests);
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
                running: RwLock::new(Vec::new()),
                transport_error: RwLock::new(None),
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
            commands::uninstall_game,
            commands::play_game,
            commands::get_launch_plan,
            commands::list_executables,
            commands::set_exe_override,
            commands::get_settings,
            commands::save_settings,
            commands::refresh_catalog,
            commands::run_diagnostics,
            commands::apply_fix,
            commands::get_transport_health,
            commands::refresh_event,
            commands::open_path,
            commands::open_url,
            commands::get_share_key,
            commands::get_library_space,
            commands::restart_transport,
        ])
        .run(tauri::generate_context!())
        .expect("error while running NextGen LAN Launcher");
}

async fn start_services(app: tauri::AppHandle, state: Arc<AppState>) {
    let library = state.library.clone();
    let (transport, error) = build_transport(&state).await;
    *state.transport_error.write().await = error;
    *state.transport.write().await = Some(transport.clone());
    register_catalog_share(&state).await;

    let catalog = if state.demo {
        demo_catalog()
    } else {
        let lib = library.read().map(|l| l.clone()).unwrap_or_default();
        load_catalog_from_library(&lib, &state.dirs, true).unwrap_or_default()
    };
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

    // Event configuration from the LANPage host.
    let st = state.clone();
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let host = st.settings.read().await.lanpage_host.clone();
            if !st.demo {
                let bundle = lanlauncher_core::lanpage::fetch_event(&host).await;
                *st.event.write().await = bundle.clone();
                let _ = app2.emit(EVENT_UPDATED, &bundle);
            }
            tokio::time::sleep(Duration::from_secs(300)).await;
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
    // triggers a reload (covers are re-extracted with it).
    let st = state.clone();
    let app5 = app.clone();
    tauri::async_runtime::spawn(async move {
        type Stamp = Option<(u64, Option<std::time::SystemTime>)>;
        let stamp = |p: std::path::PathBuf| -> Stamp {
            let meta = std::fs::metadata(p).ok()?;
            Some((meta.len(), meta.modified().ok()))
        };
        let signature = |root: Option<std::path::PathBuf>| -> (Stamp, Stamp) {
            match root {
                Some(r) => (
                    stamp(r.join(lanlauncher_core::paths::CATALOG_RELATIVE)),
                    stamp(r.join(lanlauncher_core::paths::ASSETS_RELATIVE)),
                ),
                None => (None, None),
            }
        };
        if st.demo {
            return;
        }
        let mut last = signature(st.default_root_path().await);
        // Whether any load has succeeded yet (the startup load counts when it
        // produced games). Until then an unchanged game.db is retried, because
        // the file may have been locked or half-synced at start.
        let mut ever_loaded = match st.manager.read().await.as_ref() {
            Some(m) => m.catalog_len().await > 0,
            None => false,
        };
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            let now = signature(st.default_root_path().await);
            // A vanished game.db keeps the old catalog. A changed one is
            // loaded once; if it is half-written and fails quick_check, the
            // next size/mtime change retries. Covers are re-extracted when
            // assets.eti changed or nothing was ever loaded.
            let changed = now != last;
            if now.0.is_some() && (changed || !ever_loaded) {
                let assets_changed = now.1 != last.1 || !ever_loaded;
                last = now;
                match reload_catalog(&st, assets_changed).await {
                    Some(n) => {
                        ever_loaded = true;
                        log::info!("catalog reloaded: {n} games");
                        let _ = app5.emit(CATALOG_EVENT, n);
                    }
                    None if changed => {
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
                (s.send_stats && !st.demo, s.safe_player_name())
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
