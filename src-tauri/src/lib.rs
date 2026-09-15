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

fn is_demo() -> bool {
    std::env::args().any(|a| a == "--demo")
        || std::env::var("LANLAUNCHER_DEMO")
            .map(|v| v == "1")
            .unwrap_or(false)
}

/// Platform post-extraction hook: Windows runs `game_setup.cmd`, everyone
/// applies manifest steps.
struct AppSetupHook;

#[async_trait::async_trait]
impl SetupHook for AppSetupHook {
    async fn run_setup(
        &self,
        paths: &GamePaths,
        manifest: Option<&Manifest>,
    ) -> lanlauncher_core::Result<()> {
        ManifestSetupHook.run_setup(paths, manifest).await?;
        if cfg!(target_os = "windows") {
            if let Some(plan) = lanlauncher_core::launch::windows::setup_plan(
                paths,
                paths
                    .share_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(""),
            ) {
                let status = tokio::process::Command::new(&plan.program)
                    .args(&plan.args)
                    .current_dir(&plan.cwd)
                    .status()
                    .await
                    .map_err(|e| lanlauncher_core::Error::Launch(format!("game_setup.cmd: {e}")))?;
                if !status.success() {
                    return Err(lanlauncher_core::Error::Launch(format!(
                        "game_setup.cmd exited with {status}"
                    )));
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn demo_catalog() -> Catalog {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(include_str!("../demo/demo_catalog.sql"))
        .expect("demo catalog sql");
    Catalog::from_connection(&conn).expect("demo catalog")
}

/// Load the catalog from the default library root, if present.
pub(crate) fn load_catalog_from_library(library: &Library, dirs: &AppDirs) -> Option<Catalog> {
    let root = library.default_root()?;
    let db = root.path.join(lanlauncher_core::paths::CATALOG_RELATIVE);
    let catalog = Catalog::load(&db).ok()?;
    let assets = root.path.join(lanlauncher_core::paths::ASSETS_RELATIVE);
    if assets.is_file() {
        let _ = lanlauncher_core::catalog::extract_covers(&assets, &dirs.covers_dir());
    }
    Some(catalog)
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
            let settings = state.settings.read().await;
            match resilio::locate_binary(state.resource_dir.as_deref(), &state.dirs.data) {
                Some(binary) => {
                    let mut cfg = ResilioConfig::new(binary, state.dirs.transport_dir());
                    cfg.lan_only = settings.lan_mode;
                    cfg.listening_port = settings.sync_port;
                    let t = ResilioTransport::new(cfg);
                    match t.start().await {
                        Ok(()) => (Arc::new(t), None),
                        Err(e) => (Arc::new(FolderTransport::new()), Some(format!("Resilio Sync konnte nicht gestartet werden: {e}"))),
                    }
                }
                None => (
                    Arc::new(FolderTransport::new()),
                    Some("Resilio Sync wurde nicht gefunden (weder mitgeliefert noch installiert). Ordner-Modus aktiv.".into()),
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
    state: &AppState,
    transport: Arc<dyn Transport>,
    catalog: Catalog,
    library: Arc<std::sync::RwLock<Library>>,
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
        Arc::new(AppSetupHook),
    );
    manager.lan_only = true;
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
            };
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
            let state = Arc::new(AppState {
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
    let library = Arc::new(std::sync::RwLock::new(
        state.settings.read().await.library.clone(),
    ));
    let (transport, error) = build_transport(&state).await;
    *state.transport_error.write().await = error;
    *state.transport.write().await = Some(transport.clone());

    let catalog = if state.demo {
        demo_catalog()
    } else {
        let lib = library.read().map(|l| l.clone()).unwrap_or_default();
        load_catalog_from_library(&lib, &state.dirs).unwrap_or_default()
    };
    let manager = build_manager(&state, transport.clone(), catalog, library.clone());
    manager.adopt_existing().await;
    *state.manager.write().await = Some(manager.clone());

    // Keep the shared library in sync with settings changes.
    let lib_sync = library.clone();
    let st = state.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            {
                let s = st.settings.read().await;
                if let Ok(mut l) = lib_sync.write() {
                    if *l != s.library {
                        *l = s.library.clone();
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
