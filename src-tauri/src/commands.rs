//! Tauri commands exposed to the frontend.

use crate::state::AppState;
use lanlauncher_core::catalog::Game;
use lanlauncher_core::diagnostics::{self, Report};
use lanlauncher_core::install::{GameStatus, Receipt};
use lanlauncher_core::lanpage::EventBundle;
use lanlauncher_core::launch::{self, LaunchContext, LaunchPlan};
use lanlauncher_core::manifest::{Manifest, ManifestOrigin};
use lanlauncher_core::problem::FixAction;
use lanlauncher_core::settings::{Settings, TransportMode};
use lanlauncher_core::transport::TransportHealth;
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

type Cmd<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapInfo {
    pub settings: Settings,
    pub demo: bool,
    pub platform: &'static str,
    /// `<semver> (<build id>)`, e.g. `0.1.0 (dc6722c)`.
    pub version: String,
    pub event: EventBundle,
    pub transport_mode: TransportMode,
    pub transport_error: Option<String>,
    pub needs_setup: bool,
    /// The built-in key for the catalog share (`eti_launcher`) parses (a fork
    /// may ship without one). Together with a non-empty `settings.catalogKey`
    /// the frontend derives whether the launcher can detect the sync server.
    pub builtin_catalog_key: bool,
    pub dirs: DirsInfo,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirsInfo {
    pub config: String,
    pub data: String,
    pub cache: String,
    pub logs: String,
}

/// Public game description without the share key.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GameView {
    pub id: String,
    pub order: i64,
    pub title: String,
    pub revision: String,
    pub size_bytes: u64,
    pub release_year: Option<String>,
    pub publisher: Option<String>,
    pub max_players: Option<String>,
    pub needs_master_server: bool,
    pub genre: Option<String>,
    pub readme: Option<String>,
    pub cover: Option<String>,
    /// Preview video (`eti_launcher/video/<id>.mp4`) if the share provides one.
    pub video: Option<String>,
    pub status: Option<GameStatus>,
    pub manifest: Option<ManifestInfo>,
    pub disabled_by_event: bool,
    pub share_dir: Option<String>,
    /// `keygen.exe` present in the game folder (ETI packages with a key
    /// generator); the UI offers a button only then.
    pub has_keygen: bool,
    /// `server_start.cmd` present (dedicated server script).
    pub has_server_script: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ManifestInfo {
    pub origin: ManifestOrigin,
    pub exe: String,
    pub args: Vec<String>,
    pub runner: lanlauncher_core::manifest::Runner,
    pub alternatives: Vec<String>,
    pub notes: Option<String>,
    pub verified_for_revision: bool,
}

fn manifest_info(m: &Manifest, revision: &str, lang: &str) -> ManifestInfo {
    let spec = m.launch_for(Manifest::current_platform());
    ManifestInfo {
        origin: m.origin,
        exe: spec.exe.clone(),
        args: spec.args.clone(),
        runner: spec.runner,
        alternatives: spec.alternatives.iter().map(|a| a.name.clone()).collect(),
        notes: m
            .setup
            .notes
            .get(lang)
            .or_else(|| m.setup.notes.get("en"))
            .cloned(),
        verified_for_revision: m.matches_revision(revision),
    }
}

fn resolve_manifest(state: &AppState, game: &Game) -> Option<Manifest> {
    let paths = state_paths(state, game)?;
    match state.manifests.resolve(&game.id, Some(&paths.share_dir)) {
        Ok(Some(m)) => Some(m),
        _ => std::fs::read_to_string(&paths.start_script)
            .ok()
            .and_then(|s| {
                lanlauncher_core::script_probe::ScriptProbe::analyse(&s).to_manifest(&game.id)
            }),
    }
}

fn state_paths(state: &AppState, game: &Game) -> Option<lanlauncher_core::paths::GamePaths> {
    // Settings lock is async; commands call this from async context via blocking read.
    let settings = state.settings.try_read().ok()?;
    settings.library.game_paths(&game.id)
}

#[tauri::command]
pub async fn get_bootstrap(state: State<'_, Arc<AppState>>) -> Cmd<BootstrapInfo> {
    let settings = state.settings.read().await.clone();
    let event = state.event.read().await.clone();
    Ok(BootstrapInfo {
        needs_setup: !settings.setup_complete && !state.demo,
        builtin_catalog_key: lanlauncher_core::catalog::ShareKey::parse(
            lanlauncher_core::catalog::BUILTIN_CATALOG_KEY,
        )
        .is_some(),
        transport_mode: state.effective_transport_mode().await,
        transport_error: state.transport_error.read().await.clone(),
        demo: state.demo,
        platform: Manifest::current_platform(),
        version: crate::app_version(),
        event,
        dirs: DirsInfo {
            config: state.dirs.config.to_string_lossy().to_string(),
            data: state.dirs.data.to_string_lossy().to_string(),
            cache: state.dirs.cache.to_string_lossy().to_string(),
            logs: state.dirs.logs_dir().to_string_lossy().to_string(),
        },
        settings,
    })
}

#[tauri::command]
pub async fn get_games(state: State<'_, Arc<AppState>>) -> Cmd<Vec<GameView>> {
    let catalog = state.catalog().await;
    let statuses: std::collections::HashMap<String, GameStatus> =
        match state.manager.read().await.as_ref() {
            Some(m) => m
                .tick()
                .await
                .into_iter()
                .map(|s| (s.game_id.clone(), s))
                .collect(),
            None => Default::default(),
        };
    let settings = state.settings.read().await.clone();
    let event = state.event.read().await.clone();
    let lang = settings.language.clone();
    let covers = state.cover_dirs();
    let mut out = Vec::with_capacity(catalog.games.len());
    for g in &catalog.games {
        let paths = settings.library.game_paths(&g.id);
        let manifest = resolve_manifest(&state, g);
        out.push(GameView {
            id: g.id.clone(),
            order: g.order,
            title: g.title.clone(),
            revision: g.revision.clone(),
            size_bytes: g.size_bytes,
            release_year: g.release_year.clone(),
            publisher: g.publisher.clone(),
            max_players: g.max_players.clone(),
            needs_master_server: g.needs_master_server,
            genre: g.genre(&catalog).map(|ge| ge.name(&lang).to_string()),
            readme: g
                .readme
                .get(&lang)
                .or_else(|| g.readme.get("en"))
                .or_else(|| g.readme.values().next())
                .cloned(),
            cover: lanlauncher_core::catalog::find_cover_in(&covers, &g.id)
                .map(|p| p.to_string_lossy().to_string()),
            video: settings
                .library
                .default_root()
                .and_then(|r| lanlauncher_core::catalog::find_video(&r.path, &g.id))
                .map(|p| p.to_string_lossy().to_string()),
            status: statuses.get(&g.id).cloned(),
            manifest: manifest
                .as_ref()
                .map(|m| manifest_info(m, &g.revision, &lang)),
            disabled_by_event: event
                .config
                .as_ref()
                .map(|c| c.is_game_disabled(&g.id))
                .unwrap_or(false),
            share_dir: paths
                .as_ref()
                .map(|p| p.share_dir.to_string_lossy().to_string()),
            has_keygen: paths.as_ref().and_then(|p| p.keygen()).is_some(),
            has_server_script: paths.as_ref().is_some_and(|p| p.server_script().is_file()),
        });
    }
    Ok(out)
}

#[tauri::command]
pub async fn get_statuses(state: State<'_, Arc<AppState>>) -> Cmd<Vec<GameStatus>> {
    match state.manager.read().await.as_ref() {
        Some(m) => Ok(m.tick().await),
        None => Ok(Vec::new()),
    }
}

async fn manager(state: &AppState) -> Cmd<Arc<lanlauncher_core::install::InstallManager>> {
    state
        .manager
        .read()
        .await
        .clone()
        .ok_or_else(|| "err.not_ready".to_string())
}

#[tauri::command]
pub async fn install_game(state: State<'_, Arc<AppState>>, game_id: String) -> Cmd<()> {
    manager(&state).await?.install(&game_id).await.map_err(err)
}

#[tauri::command]
pub async fn repair_game(state: State<'_, Arc<AppState>>, game_id: String) -> Cmd<()> {
    manager(&state).await?.repair(&game_id).await.map_err(err)
}

#[tauri::command]
pub async fn pause_game(state: State<'_, Arc<AppState>>, game_id: String, paused: bool) -> Cmd<()> {
    manager(&state)
        .await?
        .set_paused(&game_id, paused)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn uninstall_game(state: State<'_, Arc<AppState>>, game_id: String) -> Cmd<()> {
    manager(&state)
        .await?
        .uninstall(&game_id)
        .await
        .map_err(err)
}

async fn build_plan(
    state: &AppState,
    game_id: &str,
    alternative: Option<usize>,
) -> Cmd<LaunchPlan> {
    let catalog = state.catalog().await;
    let game = catalog.game(game_id).ok_or("err.unknown_game")?;
    let settings = state.settings.read().await.clone();
    let paths = settings
        .library
        .game_paths(game_id)
        .ok_or("err.no_library")?;
    let manifest = resolve_manifest(state, game);
    let receipt = Receipt::load(&paths.receipt);
    let ctx = LaunchContext {
        paths: &paths,
        game_id,
        settings: &settings,
        manifest: manifest.as_ref(),
        receipt: receipt.as_ref(),
        alternative,
    };
    launch::plan(&ctx).map_err(err)
}

#[tauri::command]
pub async fn get_launch_plan(
    state: State<'_, Arc<AppState>>,
    game_id: String,
    alternative: Option<usize>,
) -> Cmd<LaunchPlan> {
    build_plan(&state, &game_id, alternative).await
}

#[tauri::command]
pub async fn play_game(
    state: State<'_, Arc<AppState>>,
    game_id: String,
    alternative: Option<usize>,
) -> Cmd<u32> {
    if state.demo {
        return Err("err.demo_no_play".into());
    }
    let plan = build_plan(&state, &game_id, alternative).await?;
    let pid = launch::spawn(&plan).await.map_err(err)?;
    let mut running = state.running.write().await;
    running.retain(|(g, _)| g != &game_id);
    running.insert(0, (game_id, pid));
    Ok(pid)
}

/// Start a per-game extra (`keygen.exe`, `server_start.cmd`) that an ETI
/// package ships next to the game. Offered by the UI only when the file
/// exists; Windows only like the scripts themselves.
#[tauri::command]
pub async fn run_extra(
    state: State<'_, Arc<AppState>>,
    game_id: String,
    extra: launch::Extra,
) -> Cmd<u32> {
    if state.demo {
        return Err("err.demo_no_play".into());
    }
    let settings = state.settings.read().await.clone();
    let paths = settings
        .library
        .game_paths(&game_id)
        .ok_or("err.no_library")?;
    let ctx = LaunchContext {
        paths: &paths,
        game_id: &game_id,
        settings: &settings,
        manifest: None,
        receipt: None,
        alternative: None,
    };
    let plan = launch::extra_plan(extra, &ctx).map_err(err)?;
    log::info!(
        "starting extra {extra:?} for {game_id}: {} {}",
        plan.program.display(),
        plan.raw_command_line.clone().unwrap_or_default()
    );
    launch::spawn(&plan).await.map_err(err)
}

#[tauri::command]
pub async fn list_executables(
    state: State<'_, Arc<AppState>>,
    game_id: String,
) -> Cmd<Vec<String>> {
    let settings = state.settings.read().await;
    let paths = settings
        .library
        .game_paths(&game_id)
        .ok_or("err.no_library")?;
    Ok(launch::list_executables(&paths, 200))
}

#[tauri::command]
pub async fn set_exe_override(
    state: State<'_, Arc<AppState>>,
    game_id: String,
    exe: String,
) -> Cmd<()> {
    if !lanlauncher_core::manifest::is_safe_relative(&exe) {
        return Err("err.invalid_path".into());
    }
    let settings = state.settings.read().await;
    let paths = settings
        .library
        .game_paths(&game_id)
        .ok_or("err.no_library")?;
    let mut receipt = Receipt::load(&paths.receipt).ok_or("err.not_installed")?;
    receipt.exe_override = Some(exe);
    receipt.save(&paths.receipt).map_err(err)
}

#[tauri::command]
pub async fn get_settings(state: State<'_, Arc<AppState>>) -> Cmd<Settings> {
    Ok(state.settings.read().await.clone())
}

#[tauri::command]
pub async fn save_settings(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
    settings: Settings,
) -> Cmd<Settings> {
    let mut current = state.settings.write().await;
    let mut new = settings;
    if state.demo {
        new.library = current.library.clone();
        new.transport = TransportMode::Demo;
        new.setup_complete = true;
    }
    new.normalise_catalog_key();
    new.normalise_resilio_binary();
    new.normalise_resilio_api_key();
    if let Some(k) = &new.catalog_key {
        if lanlauncher_core::catalog::ShareKey::parse(k).is_none() {
            return Err("err.invalid_catalog_key".into());
        }
    }
    for root in &new.library.roots {
        if !root.path.exists() {
            std::fs::create_dir_all(&root.path)
                .map_err(|e| format!("err.create_folder|{}: {e}", root.path.display()))?;
        }
    }
    new.save(&state.settings_path()).map_err(err)?;
    let old_root = current.library.default_root().map(|r| r.path.clone());
    let new_root = new.library.default_root().map(|r| r.path.clone());
    let catalog_changed = current.catalog_key != new.catalog_key || old_root != new_root;
    // Binary and API key both go into the engine config: restart on change.
    let binary_changed = current.resilio_binary != new.resilio_binary
        || current.resilio_api_key != new.resilio_api_key;
    *current = new.clone();
    drop(current);
    if binary_changed && new.transport == TransportMode::Managed && !state.demo {
        // A different engine binary only takes effect with a fresh transport;
        // the error, if any, is shown by the next diagnostics run.
        let _ = restart_transport_inner(&state).await;
    }
    if catalog_changed && !state.demo {
        // The wizard saves a root without restarting the transport, so the
        // catalog share is (re-)registered right here. The old registration
        // is dropped first: a moved root must not sync the catalog twice, and
        // Resilio ignores re-adding a known folder with a different key.
        if let Some(old) = &old_root {
            if let Some(t) = state.transport.read().await.clone() {
                let old_dir = old.join(lanlauncher_core::paths::LAUNCHER_SHARE_ID);
                if let Err(e) = t.remove_share(&old_dir).await {
                    log::warn!("cannot remove old catalog share {}: {e}", old_dir.display());
                }
            }
        }
        crate::register_catalog_share(&state).await;
        // The wizard runs its diagnostics right after saving the first root;
        // load the catalog now instead of waiting for the file watcher. Covers
        // are only re-extracted when the root moved; a key change touches no
        // files. Runs in the background so Save returns immediately.
        if old_root != new_root {
            let st = state.inner().clone();
            tauri::async_runtime::spawn(async move {
                if let Some(n) = crate::reload_catalog(&st, true).await {
                    use tauri::Emitter;
                    log::info!("catalog loaded after settings change: {n} games");
                    let _ = app.emit(crate::CATALOG_EVENT, n);
                }
            });
        }
    }
    Ok(new)
}

#[tauri::command]
pub async fn refresh_catalog(state: State<'_, Arc<AppState>>) -> Cmd<usize> {
    if state.demo {
        return Ok(state.catalog().await.games.len());
    }
    crate::reload_catalog(&state, true)
        .await
        .ok_or_else(|| "err.no_catalog".to_string())
}

#[tauri::command]
pub async fn run_diagnostics(state: State<'_, Arc<AppState>>) -> Cmd<Report> {
    let mut problems = Vec::new();
    let mut checks = Vec::new();
    let settings = state.settings.read().await.clone();

    checks.push("library".into());
    problems.extend(diagnostics::check_disk_space(
        &settings.library,
        20 * 1024 * 1024 * 1024,
    ));

    if cfg!(target_os = "windows") {
        checks.push("network_profile".into());
        let profiles = crate::fixes::network_profiles().await;
        problems.extend(diagnostics::check_network_profiles(&profiles));
    }

    checks.push("transport".into());
    let transport = state.transport.read().await.clone();
    if let Some(t) = &transport {
        let health = t.health().await;
        problems.extend(diagnostics::check_transport(&health));
    }
    checks.push("covers".into());
    if let Some(root) = state.default_root_path().await {
        let assets = root.join(lanlauncher_core::paths::ASSETS_RELATIVE);
        let covers = std::fs::read_dir(state.dirs.covers_dir())
            .map(|d| d.flatten().count())
            .unwrap_or(0);
        if assets.is_file() && covers == 0 {
            problems.push(
                lanlauncher_core::problem::Problem::new(
                    "catalog.covers_missing",
                    lanlauncher_core::problem::Severity::Info,
                )
                .param("path", assets.display().to_string())
                .step("catalog.covers_missing.step.refresh"),
            );
        }
    }

    // Only the managed Resilio registers the catalog share, so only there a
    // missing key matters (a fallback to folder mode is reported above).
    checks.push("catalog_key".into());
    let managed = transport
        .as_ref()
        .is_some_and(|t| t.kind() == lanlauncher_core::transport::TransportKind::Resilio);
    if managed
        && lanlauncher_core::catalog::catalog_share_key(settings.catalog_key.as_deref()).is_none()
    {
        problems.push(
            lanlauncher_core::problem::Problem::new(
                "catalog.key_missing",
                lanlauncher_core::problem::Severity::Warning,
            )
            .step("catalog.key_missing.step.settings"),
        );
    }
    if let Some(e) = state.transport_error.read().await.clone() {
        problems.push(
            lanlauncher_core::problem::Problem::new(
                "transport.start_failed",
                lanlauncher_core::problem::Severity::Error,
            )
            .param("detail", e)
            .step("transport.start_failed.step.install")
            .step("transport.start_failed.step.pick_binary")
            .step("transport.start_failed.step.folder_mode")
            .with_fix(FixAction::OpenUrl {
                url: lanlauncher_core::transport::resilio::official_download_url(),
            }),
        );
    }

    checks.push("orphans".into());
    let own_pid = match state.transport.read().await.as_ref() {
        Some(t) => t.process_id(),
        None => None,
    };
    problems.extend(diagnostics::check_orphans(own_pid));

    checks.push("lanpage".into());
    let event = state.event.read().await.clone();
    if !state.demo && event.config.is_none() {
        problems.push(
            lanlauncher_core::problem::Problem::new(
                "lanpage.unreachable",
                lanlauncher_core::problem::Severity::Info,
            )
            .param("host", &settings.lanpage_host)
            .param("detail", event.errors.join("; "))
            .step("lanpage.unreachable.step.ask_orga"),
        );
    }
    checks.push("clock".into());
    problems.extend(diagnostics::check_clock(event.server_time));

    if !settings.library.roots.is_empty() {
        checks.push("catalog".into());
        let catalog = state.catalog().await;
        if catalog.games.is_empty() && !state.demo {
            problems.push(
                lanlauncher_core::problem::Problem::new(
                    "catalog.missing",
                    lanlauncher_core::problem::Severity::Warning,
                )
                .step("catalog.missing.step.wait")
                .step("catalog.missing.step.peers"),
            );
        }
        if !catalog.skipped_rows.is_empty() {
            problems.push(
                lanlauncher_core::problem::Problem::new(
                    "catalog.skipped_rows",
                    lanlauncher_core::problem::Severity::Info,
                )
                .param("count", catalog.skipped_rows.len())
                .param("rows", catalog.skipped_rows.join(", ")),
            );
        }
    }

    Ok(Report::new(problems, checks))
}

#[tauri::command]
pub async fn apply_fix(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
    fix: FixAction,
) -> Cmd<String> {
    crate::fixes::apply(&app, &state, fix).await
}

#[tauri::command]
pub async fn get_transport_health(state: State<'_, Arc<AppState>>) -> Cmd<Option<TransportHealth>> {
    match state.transport.read().await.clone() {
        Some(t) => Ok(Some(t.health().await)),
        None => Ok(None),
    }
}

/// Path of ETI's runtime package installer (`eti_launcher/bin/preqsetup.exe`)
/// once the catalog share has delivered it completely; `None` elsewhere, in
/// demo mode and off Windows.
#[tauri::command]
pub async fn get_prereq_installer(state: State<'_, Arc<AppState>>) -> Cmd<Option<String>> {
    if state.demo {
        return Ok(None);
    }
    let Some(root) = state.default_root_path().await else {
        return Ok(None);
    };
    Ok(launch::prereq_installer(&root).map(|p| p.to_string_lossy().to_string()))
}

/// Start ETI's runtime package installer. It brings its own UI; the user
/// confirms in the frontend first, as the ETI client does.
#[tauri::command]
pub async fn run_prereq_installer(state: State<'_, Arc<AppState>>) -> Cmd<u32> {
    if state.demo {
        return Err("err.demo_no_play".into());
    }
    let root = state.default_root_path().await.ok_or("err.no_library")?;
    let exe = launch::prereq_installer(&root).ok_or("err.prereq_missing")?;
    log::info!("starting runtime package installer {}", exe.display());
    launch::spawn(&launch::prereq_plan(&exe)).await.map_err(err)
}

#[tauri::command]
pub async fn open_path(app: tauri::AppHandle, path: String) -> Cmd<()> {
    use tauri_plugin_opener::OpenerExt;
    let p = std::path::Path::new(&path);
    if !p.exists() {
        std::fs::create_dir_all(p).map_err(err)?;
    }
    app.opener().open_path(path, None::<&str>).map_err(err)
}

#[tauri::command]
pub async fn open_url(app: tauri::AppHandle, url: String) -> Cmd<()> {
    use tauri_plugin_opener::OpenerExt;
    if !(url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("ts3server://")
        || url.starts_with("dchub://")
        || url.starts_with("adc://")
        || url.starts_with("adcs://"))
    {
        return Err("err.unsupported_link".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(err)
}

/// The Resilio read-only key of a game, for folder mode where the user adds
/// the share manually. Not available in managed mode to keep keys private.
#[tauri::command]
pub async fn get_share_key(state: State<'_, Arc<AppState>>, game_id: String) -> Cmd<String> {
    if state.effective_transport_mode().await != TransportMode::Folder {
        return Err("err.key_folder_mode_only".into());
    }
    let catalog = state.catalog().await;
    let game = catalog.game(&game_id).ok_or("err.unknown_game")?;
    Ok(game.key.expose().to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySpace {
    pub path: String,
    pub label: String,
    pub is_default: bool,
    pub free_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub games: usize,
}

#[tauri::command]
pub async fn get_library_space(state: State<'_, Arc<AppState>>) -> Cmd<Vec<LibrarySpace>> {
    let settings = state.settings.read().await.clone();
    let catalog = state.catalog().await;
    Ok(settings
        .library
        .roots
        .iter()
        .map(|r| {
            let space = lanlauncher_core::library::disk_space(&r.path);
            LibrarySpace {
                path: r.path.to_string_lossy().to_string(),
                label: r.label.clone(),
                is_default: r.is_default,
                free_bytes: space.map(|s| s.0),
                total_bytes: space.map(|s| s.1),
                games: catalog
                    .games
                    .iter()
                    .filter(|g| r.path.join(&g.id).is_dir())
                    .count(),
            }
        })
        .collect())
}

pub async fn restart_transport_inner(state: &Arc<AppState>) -> Cmd<()> {
    // Jobs of the old manager must not race the successor's on the same
    // staging directories.
    if let Some(m) = state.manager.read().await.clone() {
        m.cancel_all().await;
    }
    if let Some(t) = state.transport.read().await.clone() {
        let _ = t.stop().await;
    }
    let (transport, error) = crate::build_transport(state).await;
    *state.transport_error.write().await = error.clone();
    *state.transport.write().await = Some(transport.clone());
    crate::register_catalog_share(state).await;
    let catalog = state.catalog().await;
    let lan_only = state.settings.read().await.lan_mode;
    let manager = crate::build_manager(state, transport, catalog, state.library.clone(), lan_only);
    manager.adopt_existing().await;
    *state.manager.write().await = Some(manager);
    match error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

#[tauri::command]
pub async fn restart_transport(state: State<'_, Arc<AppState>>) -> Cmd<()> {
    restart_transport_inner(&state).await
}
