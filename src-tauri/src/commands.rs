//! Tauri commands exposed to the frontend.

use crate::state::AppState;
use lanlauncher_core::catalog::Game;
use lanlauncher_core::diagnostics::{self, Report};
use lanlauncher_core::install::{GameStatus, Receipt};
use lanlauncher_core::lanpage::EventBundle;
use lanlauncher_core::launch::{self, LaunchContext, LaunchPlan};
use lanlauncher_core::manifest::{Manifest, ManifestOrigin};
use lanlauncher_core::problem::FixAction;
use lanlauncher_core::settings::{GameRunner, Settings, TransportMode};
use lanlauncher_core::transport::TransportHealth;
use serde::Serialize;
use std::path::PathBuf;
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
    /// The tester's own launch configuration is laid over the profile.
    pub own_config: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerOption {
    pub program: String,
    pub label: String,
    pub kind: lanlauncher_core::manifest::Runner,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerChoices {
    pub selected: Option<String>,
    pub selected_kind: Option<lanlauncher_core::manifest::Runner>,
    pub options: Vec<RunnerOption>,
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
        verified_for_revision: m.verified_for(revision, Manifest::current_platform()),
        own_config: m.user_config,
    }
}

/// The game's manifest, for paths the caller took from its own settings
/// copy. Not read here with `try_read`: that finds nothing while someone
/// writes the settings, and the manifest would silently be missing.
fn resolve_manifest(
    state: &AppState,
    game: &Game,
    paths: Option<&lanlauncher_core::paths::GamePaths>,
) -> Option<Manifest> {
    state.manifests.resolve_for(&game.id, paths?)
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
        let manifest = resolve_manifest(&state, g, paths.as_ref());
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
                // A tester's configuration speaks for the package that is
                // installed, which is not the catalog's while an update waits.
                .map(|m| {
                    let installed = statuses
                        .get(&g.id)
                        .and_then(|s| s.installed_revision.as_deref())
                        .unwrap_or(&g.revision);
                    manifest_info(m, installed, &lang)
                }),
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

/// Does this folder hold nothing but the engine's own `.sync` bookkeeping?
/// That is what the library counts as empty, and the only case in which a
/// recursive delete is anything but dangerous.
fn only_sync(dir: &std::path::Path) -> bool {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries.flatten().all(|e| {
                e.file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(".sync")
            })
        })
        .unwrap_or(false)
}

async fn prepare_install_root(
    state: &AppState,
    manager: &lanlauncher_core::install::InstallManager,
    game_id: &str,
) -> Cmd<Option<std::path::PathBuf>> {
    // Which root a new game goes to is decided here, once, by free space:
    // creating the folder is what every later lookup follows. Asking per
    // lookup would enumerate the volumes hundreds of times per round.
    let mut created: Option<std::path::PathBuf> = None;
    if let Some(game) = state.catalog().await.game(game_id) {
        // The same snapshot the manager resolves paths from, so the folder is
        // created where the download will look for it.
        let library = state.library.read().ok().map(|l| l.clone());
        let needed = game.size_bytes.saturating_mul(2);
        if let Some((paths, leftovers, relocate)) = library.and_then(|l| {
            let paths = l.game_paths_for(&game.id, needed)?;
            let leftovers = l.empty_leftovers(&game.id, &paths.share_dir);
            let relocate = l.game_paths(&game.id).is_some_and(|old| {
                old.share_dir != paths.share_dir
                    && old.share_dir.exists()
                    && !leftovers.contains(&old.share_dir)
            });
            Some((paths, leftovers, relocate))
        }) {
            if relocate {
                manager
                    .relocate_download(&game.id, paths.clone())
                    .await
                    .map_err(err)?;
            }
            // An empty folder from a cancelled attempt in another root would
            // send every later lookup to the wrong disk — and the engine may
            // still have a share registered on it, which is what leaves a
            // download wedged at "folder not found".
            let transport = state.transport.read().await.clone();
            for dir in leftovers {
                if let Some(t) = &transport {
                    if let Err(e) = t.remove_share(&dir).await {
                        log::debug!("no share to remove for {}: {e}", dir.display());
                    }
                    // Windows keeps the engine's handles on `.sync` for a
                    // moment after it lets the folder go.
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                }
                // Only the engine's own `.sync` may still be in there — the
                // library counts that as empty, so a plain `remove_dir` would
                // fail on it.
                let removed = std::fs::remove_dir(&dir).or_else(|first| {
                    if only_sync(&dir) {
                        std::fs::remove_dir_all(&dir)
                    } else {
                        Err(first)
                    }
                });
                // One retry: the handles are usually gone a moment later. The
                // guard is checked again — whatever the engine may have
                // written back meanwhile is not ours to delete.
                let removed = match removed {
                    Err(first) => {
                        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
                        if only_sync(&dir) {
                            std::fs::remove_dir_all(&dir)
                        } else {
                            std::fs::remove_dir(&dir).map_err(|_| first)
                        }
                    }
                    ok => ok,
                };
                match removed {
                    Ok(()) => log::info!("removed the empty leftover folder {}", dir.display()),
                    Err(e) => {
                        log::warn!("leftover folder {} stays: {e}", dir.display());
                    }
                }
            }
            let free = lanlauncher_core::library::disk_space(&paths.share_dir).map(|(f, _)| f);
            log::info!(
                "installing {} into {} (needs {} bytes, {} free)",
                game.id,
                paths.share_dir.display(),
                needed,
                free.map(|f| f.to_string()).unwrap_or_else(|| "?".into())
            );
            let existed = paths.share_dir.exists();
            std::fs::create_dir_all(&paths.share_dir)
                .map_err(|e| format!("err.create_folder|{}: {e}", paths.share_dir.display()))?;
            // A marker makes the selected root discoverable even while the
            // engine has not written a file yet. An old locked `.sync` folder
            // must never redirect this download back to the full volume.
            let marker = paths.share_dir.join(".nll-download");
            if !marker.exists() {
                std::fs::File::create_new(&marker)
                    .map_err(|e| format!("err.create_folder|{}: {e}", marker.display()))?;
            }
            if !existed {
                created = Some(paths.share_dir.clone());
            }
        }
    }
    Ok(created)
}

#[tauri::command]
pub async fn install_game(state: State<'_, Arc<AppState>>, game_id: String) -> Cmd<()> {
    // An update replaces the files the prefix lives among.
    let _use = GameUse::claim(&state.prefix_use, &game_id)?;
    let manager = manager(&state).await?;
    let created = prepare_install_root(&state, &manager, &game_id).await?;
    match manager.install(&game_id).await {
        Ok(()) => Ok(()),
        Err(e) => {
            // An install that never started must not leave the game pinned to
            // this root: the empty folder is what every later lookup follows.
            // `remove_dir` only removes it while it is still empty.
            if let Some(dir) = created {
                let _ = std::fs::remove_file(dir.join(".nll-download"));
                if let Err(e) = std::fs::remove_dir(&dir) {
                    log::warn!(
                        "install of {game_id} failed and {} could not be removed: {e}",
                        dir.display()
                    );
                }
            }
            Err(err(e))
        }
    }
}

#[tauri::command]
pub async fn repair_game(state: State<'_, Arc<AppState>>, game_id: String) -> Cmd<()> {
    repair_game_inner(&state, &game_id).await
}

pub(crate) async fn repair_game_inner(state: &AppState, game_id: &str) -> Cmd<()> {
    // Only for the request: the work after it shows in the game's phase,
    // which a component installation checks.
    let _use = GameUse::claim(&state.prefix_use, game_id)?;
    // A repair usually follows something the user just did to the folder;
    // the engine's state has to be read fresh, not from the last snapshot.
    if let Some(t) = state.transport.read().await.as_ref() {
        t.invalidate();
    }
    let manager = manager(state).await?;
    prepare_install_root(state, &manager, game_id).await?;
    manager.repair(game_id).await.map_err(err)
}

#[tauri::command]
pub async fn pause_game(state: State<'_, Arc<AppState>>, game_id: String, paused: bool) -> Cmd<()> {
    let manager = manager(&state).await?;
    if !paused {
        prepare_install_root(&state, &manager, &game_id).await?;
        // Re-register if preparing the root relocated an unfinished share.
        manager.install(&game_id).await.map_err(err)?;
    }
    manager.set_paused(&game_id, paused).await.map_err(err)
}

/// Stop a download and delete its data. Returns `true` when an installed
/// version (and its savegames) was kept because only an update was cancelled.
#[tauri::command]
pub async fn cancel_download(state: State<'_, Arc<AppState>>, game_id: String) -> Cmd<bool> {
    let _use = GameUse::claim(&state.prefix_use, &game_id)?;
    manager(&state)
        .await?
        .cancel_download(&game_id)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn uninstall_game(state: State<'_, Arc<AppState>>, game_id: String) -> Cmd<()> {
    let _use = GameUse::claim(&state.prefix_use, &game_id)?;
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
    Ok(plan_with_profile(state, game_id, alternative).await?.0)
}

/// The plan and the profile it was made from, from one resolution: what a
/// caller reads beside the plan must belong to the same configuration.
async fn plan_with_profile(
    state: &AppState,
    game_id: &str,
    alternative: Option<usize>,
) -> Cmd<(LaunchPlan, Option<Manifest>)> {
    with_launch_context(state, game_id, alternative, None, launch::plan).await
}

/// How the game's setup script runs on macOS and Linux: Wine's `cmd.exe`
/// with the game's runner and prefix (`None` for a native game). The same
/// resolution as the game's own start, so both end up in the same prefix —
/// in the folder the setup files were written to, `paths`.
pub(crate) async fn setup_script_plan(
    state: &AppState,
    game_id: &str,
    paths: &lanlauncher_core::paths::GamePaths,
) -> Cmd<Option<LaunchPlan>> {
    with_launch_context(
        state,
        game_id,
        None,
        Some(paths.clone()),
        launch::unix::setup_script_plan,
    )
    .await
    .map(|(plan, _)| plan)
}

/// What a start needs planned: the game's own plan and, on macOS/Linux, the
/// start script's preparation before it ([`launch::unix::preparation_plan`])
/// with the executable the start runs — the script's line that starts it
/// ends the preparation — and the run that sets the profile's registry
/// values in the prefix (`player::apply`); the profile beside them.
pub(crate) struct StartPlans {
    pub plan: LaunchPlan,
    pub preparation: Option<(LaunchPlan, Option<String>)>,
    pub registry: Option<LaunchPlan>,
    pub manifest: Option<Manifest>,
}

/// [`StartPlans`] from one context (profile, receipt, settings); the
/// runner is still looked up for each plan.
async fn build_start(
    state: &AppState,
    game_id: &str,
    alternative: Option<usize>,
) -> Cmd<StartPlans> {
    let (made, manifest) = with_launch_context(state, game_id, alternative, None, |ctx| {
        let plan = launch::plan(ctx)?;
        if cfg!(target_os = "windows") {
            return Ok((plan, None, None));
        }
        let exe = launch::resolve_exe(ctx)
            .ok()
            .and_then(|(exe, ..)| exe.file_name().map(|n| n.to_string_lossy().to_string()));
        let preparation = launch::unix::preparation_plan(ctx)?.map(|prep| (prep, exe));
        let registry = if ctx
            .manifest
            .is_some_and(|m| m.sets_registry() || m.uses_windows_profile())
        {
            launch::unix::script_plan(ctx, launch::setup_script::Script::Settings)?
        } else {
            None
        };
        Ok((plan, preparation, registry))
    })
    .await?;
    let (plan, preparation, registry) = made;
    Ok(StartPlans {
        plan,
        preparation,
        registry,
        manifest,
    })
}

/// `make` with the context a game's start is planned from — its paths
/// (`paths`, or where the library finds the game), settings, profile and
/// receipt — and the profile beside its result.
async fn with_launch_context<T: Send + 'static>(
    state: &AppState,
    game_id: &str,
    alternative: Option<usize>,
    paths: Option<lanlauncher_core::paths::GamePaths>,
    make: impl FnOnce(&LaunchContext<'_>) -> lanlauncher_core::Result<T> + Send + 'static,
) -> Cmd<(T, Option<Manifest>)> {
    let catalog = state.catalog().await;
    let game = catalog.game(game_id).ok_or("err.unknown_game")?;
    let settings = state.settings.read().await.clone();
    let paths = match paths {
        Some(paths) => paths,
        None => settings
            .library
            .game_paths(game_id)
            .ok_or("err.no_library")?,
    };
    let manifest = resolve_manifest(state, game, Some(&paths));
    drop(catalog);
    let game_id = game_id.to_string();
    // On a blocking thread: on macOS and Linux the plan searches for its
    // runner — each Steam library, `PATH` — every time the details are shown.
    tauri::async_runtime::spawn_blocking(move || {
        let receipt = Receipt::load(&paths.receipt);
        let ctx = LaunchContext {
            paths: &paths,
            game_id: &game_id,
            settings: &settings,
            manifest: manifest.as_ref(),
            receipt: receipt.as_ref(),
            alternative,
        };
        let made = make(&ctx).map_err(err)?;
        Ok((made, manifest))
    })
    .await
    .map_err(|e| format!("err.plan_task|{e}"))?
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
pub async fn get_runner_options(
    state: State<'_, Arc<AppState>>,
    game_id: String,
) -> Cmd<RunnerChoices> {
    if state.catalog().await.game(&game_id).is_none() {
        return Err("err.unknown_game".into());
    }
    // A copy, so the lock is gone before the scan: walking every Steam
    // library and `PATH` for each game opened must not hold up anyone who
    // wants to write the settings meanwhile.
    let settings = state.settings.read().await.clone();
    let stored = settings.game_runner(&game_id).cloned();
    let mut runners = scan_runners(settings).await?;
    if let Some(stored) = &stored {
        let already_present = runners.iter().any(|runner| {
            runner.runner == stored.runner
                && launch::unix::same_program(&runner.program, &stored.program)
        });
        if let Some(runner) = launch::unix::persisted_runner(stored).filter(|_| !already_present) {
            runners.push(runner);
        }
    }
    // Return the spelling used by the matching option. The persisted path may
    // reach the same Proton through a Steam symlink; the browser cannot
    // canonicalise native paths and would otherwise show it as unavailable.
    let selected = stored.as_ref().map(|stored| {
        runners
            .iter()
            .find(|runner| {
                runner.runner == stored.runner
                    && launch::unix::same_program(&runner.program, &stored.program)
            })
            .map(|runner| runner.program.as_path())
            .unwrap_or(&stored.program)
            .to_string_lossy()
            .to_string()
    });
    let options = runners
        .into_iter()
        .map(|runner| RunnerOption {
            program: runner.program.to_string_lossy().to_string(),
            label: runner.label,
            kind: runner.runner,
        })
        .collect();
    let selected_kind = stored.map(|runner| runner.runner);
    Ok(RunnerChoices {
        selected,
        selected_kind,
        options,
    })
}

#[tauri::command]
pub async fn set_game_runner(
    state: State<'_, Arc<AppState>>,
    game_id: String,
    program: Option<String>,
    kind: Option<lanlauncher_core::manifest::Runner>,
) -> Cmd<()> {
    // Which prefix the game uses may change with the tool.
    let _use = GameUse::claim(&state.prefix_use, &game_id)?;
    // One choice at a time, and taken before anything else here awaits, so
    // picks queue up in the order their commands started. The scan below
    // runs without the settings lock; without this, a quick second pick
    // whose scan ends first would be overwritten by the first one, and the
    // game would start with a tool the dropdown no longer shows.
    let _one_at_a_time = state.runner_choice.lock().await;
    if state.catalog().await.game(&game_id).is_none() {
        return Err("err.unknown_game".into());
    }
    let selected = match program.map(PathBuf::from) {
        None => None,
        Some(program) => {
            let kind = kind.ok_or("err.runner_missing")?;
            let snapshot = state.settings.read().await.clone();
            let game = game_id.clone();
            // Everything that touches the disk — the scan, the record of
            // what ran in the game's prefix, resolving the path — on a
            // blocking thread, not on a worker other commands wait for.
            let chosen = tauri::async_runtime::spawn_blocking(move || {
                pick_runner(&snapshot, &game, &program, kind)
            })
            .await
            .map_err(|e| format!("err.runner_scan|{e}"))??;
            Some(chosen)
        }
    };
    // The change goes onto the settings as they are now, not onto the copy
    // the scan ran against: something else may have saved in the meantime.
    let mut settings = state.settings.write().await;
    let mut updated = settings.clone();
    updated.set_game_runner(&game_id, selected);
    updated.save(&state.settings_path()).map_err(err)?;
    *settings = updated;
    Ok(())
}

/// The pin `set_game_runner` stores for `program`, found among what this
/// machine has — or kept from the stored pin when that tool is gone.
fn pick_runner(
    settings: &lanlauncher_core::settings::Settings,
    game_id: &str,
    program: &std::path::Path,
    kind: lanlauncher_core::manifest::Runner,
) -> Cmd<GameRunner> {
    let stored = settings.game_runner(game_id);
    let found = launch::unix::detect_runners(settings);
    let runner =
        launch::unix::find_runner(&found, kind, program, stored).ok_or("err.runner_missing")?;
    // Whether this pin keeps the game's own prefix is decided once, now.
    // Choosing again what is already chosen keeps what was decided then.
    let shares_default_prefix = match stored {
        Some(stored)
            if stored.runner == runner.runner
                && launch::unix::same_program(&stored.program, &runner.program) =>
        {
            stored.shares_default_prefix
        }
        _ => settings.library.game_paths(game_id).is_some_and(|paths| {
            launch::unix::pinning_keeps_default_prefix(&paths, game_id, &runner)
        }),
    };
    Ok(GameRunner {
        // Persist the resolved path once. Prefix hashing then remains stable
        // even when the originally selected symlink is retargeted, while
        // aliases of one installation still share a prefix.
        program: std::fs::canonicalize(&runner.program).unwrap_or(runner.program),
        runner: runner.runner,
        label: runner.label,
        steam_root: runner.steam_root,
        shares_default_prefix,
    })
}

/// Every runner on this machine, found off the async runtime. The search
/// reads the filesystem — each Steam library, `PATH` — and that belongs on a
/// blocking thread, not on a worker other commands are waiting for.
async fn scan_runners(
    settings: lanlauncher_core::settings::Settings,
) -> Cmd<Vec<launch::unix::DetectedRunner>> {
    tauri::async_runtime::spawn_blocking(move || launch::unix::detect_runners(&settings))
        .await
        .map_err(|e| format!("err.runner_scan|{e}"))
}

#[tauri::command]
pub async fn play_game(
    state: State<'_, Arc<AppState>>,
    game_id: String,
    alternative: Option<usize>,
    // `capture` writes the program's output to a file instead of its console.
    // Off by default: an ETI start script may ask a question (Doom's package
    // offers Heretic, Hexen or the GZDoom launcher), and a script whose
    // output goes into a file asks it into a window that shows nothing.
    capture: Option<bool>,
    // Start without the profile's Windows components, after the user was
    // told they are missing (`err.components_needed`).
    skip_components: Option<bool>,
) -> Cmd<u32> {
    if state.demo {
        return Err("err.demo_no_play".into());
    }
    // Held until the game has been spawned; from then on a component
    // installation finds it running in its prefix.
    let _starting = GameUse::claim(&state.prefix_use, &game_id)?;
    let StartPlans {
        mut plan,
        preparation,
        registry,
        manifest,
    } = build_start(&state, &game_id, alternative).await?;
    if prefix_being_filled(&state, &plan) {
        return Err("err.components_busy_game".into());
    }
    // Components the profile names and this prefix lacks: the frontend
    // offers to install them (minutes, the internet once) or to start
    // without. FlatOut 2 without d3dx9_30 only shows an error.
    if !cfg!(target_os = "windows") && !skip_components.unwrap_or(false) {
        let verbs = manifest
            .as_ref()
            .map(|m| m.launch_for(Manifest::current_platform()).winetricks)
            .unwrap_or_default();
        if !verbs.is_empty() {
            let probe = plan.clone();
            let missing = tauri::async_runtime::spawn_blocking(move || {
                launch::winetricks::missing(&probe, &verbs)
            })
            .await
            .map_err(err)?;
            if !missing.is_empty() {
                return Err(format!("err.components_needed|{}", missing.join(" ")));
            }
        }
    }
    // Read once: a library move published in between must not send the
    // setup, the preparation and the start to different folders.
    let (allow, lang, player, paths) = {
        let s = state.settings.read().await;
        (
            s.allow_elevation,
            s.game_language.clone(),
            s.safe_player_name(),
            s.library.game_paths(&game_id),
        )
    };
    // Asked before the setup below or the start creates it: a prefix that
    // was already there may hold saves of versions the launcher never
    // recorded.
    let existed_before = cfg!(not(windows))
        && paths.as_ref().is_some_and(|paths| {
            launch::unix::own_prefix_exists_before_start(&plan, paths, &game_id)
        });
    // Into the prefix the start then uses; see `wine_setup::catch_up` for
    // what holds the start back and what does not.
    if !cfg!(target_os = "windows") {
        crate::wine_setup::catch_up(state.inner(), &game_id, &plan).await?;
        // What the game's start script does, in the prefix: all of it, or
        // what it prepares before a profile's executable starts.
        if let Some(paths) = &paths {
            if launch::unix::is_script_start(&plan) {
                let extra = manifest
                    .as_ref()
                    .map(|m| m.script_args.clone())
                    .unwrap_or_default();
                crate::wine_setup::prepare_start(state.inner(), &game_id, paths, &mut plan, extra)
                    .await?;
            } else if let Some(preparation) = preparation {
                crate::wine_setup::run_preparation(state.inner(), &game_id, paths, preparation)
                    .await?;
            }
        }
    }
    if let Some(paths) = &paths {
        // The player's name and language, on top of what the script does.
        crate::player::apply(state.inner(), &game_id, paths, manifest.as_ref(), registry).await?;
        crate::fixes::ensure_firewall_rules(&state, paths, &game_id, &lang, &player).await;
    }
    let title = state
        .catalog()
        .await
        .game(&game_id)
        .map(|g| g.title.clone())
        .unwrap_or_else(|| game_id.clone());
    let mut attempt = crate::state::LaunchAttempt::new(&game_id, &title, "play", &plan);
    attempt.alternative = alternative;
    // The prefix belongs in this line: a separately pinned version ignores
    // a manifest's own, and this is where that shows once per start.
    let prefix = ["WINEPREFIX", "STEAM_COMPAT_DATA_PATH"]
        .iter()
        .filter_map(|name| plan.env.get(*name).map(|dir| format!(" ({name}={dir})")))
        .collect::<String>();
    log::info!(
        "starting {game_id} via {}{prefix}: {} {}",
        plan.runner,
        plan.command_display(),
        attempt.command_line
    );
    // With capture the script's console stays empty and everything it prints
    // lands in the file — for a start that failed, that is the whole point;
    // for the normal start it would hide a question the script is asking.
    let log = capture.unwrap_or(false).then(|| {
        let path = state.launch_log(&game_id, "play");
        // An elevated start runs through a batch file and writes nothing
        // here; the previous run's output must not be shown as this one's.
        let _ = std::fs::remove_file(&path);
        path
    });
    let outcome =
        launch::spawn_for_user_watched(&plan, &state.run_dir(), allow, log.as_deref()).await;
    let pid = state
        .inner()
        .record_launch(attempt, log, outcome)
        .await
        .map_err(err)?;
    // Only a start that happened counts: this is what a later pin of the
    // same tool is matched against to keep the game's savegames.
    if let Some(paths) = paths.filter(|_| cfg!(not(windows))) {
        // Awaited: a pin chosen right after this start must find its record.
        crate::wine_setup::remember_prefix(plan.clone(), paths, game_id.clone(), existed_before)
            .await;
    }
    {
        let mut running = state.running.write().await;
        running.retain(|(g, _)| g != &game_id);
        running.insert(0, (game_id, pid));
    }
    crate::chat::publish_playing(&state).await;
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
    let attempt =
        crate::state::LaunchAttempt::new(&game_id, &game_id, &format!("{extra:?}"), &plan);
    log::info!(
        "starting extra {extra:?} for {game_id}: {} {}",
        plan.program.display(),
        attempt.command_line
    );
    let log = state.launch_log(&game_id, &format!("{extra:?}"));
    let _ = std::fs::remove_file(&log);
    let outcome = launch::spawn_for_user_watched(
        &plan,
        &state.run_dir(),
        settings.allow_elevation,
        Some(&log),
    )
    .await;
    state
        .inner()
        .record_launch(attempt, Some(log), outcome)
        .await
        .map_err(err)
}

/// Run a game's `game_setup.cmd` again, with its output captured.
///
/// The setup runs once per install and keeps its own console so it can ask
/// questions; when it fails, "the setup script reported an error" is all the
/// user has. Repeated from here everything it prints goes into the transcript
/// the diagnostics page shows — including the question it may be waiting for,
/// which is why this does not wait for the script to finish.
#[tauri::command]
pub async fn rerun_setup(state: State<'_, Arc<AppState>>, game_id: String) -> Cmd<()> {
    if !cfg!(target_os = "windows") {
        // In the prefix the output always goes into the transcript. What
        // keeps it from starting is the answer to the click; the run itself
        // is not waited for, a helper may wait for a click of its own.
        let paths = state
            .settings
            .read()
            .await
            .library
            .game_paths(&game_id)
            .ok_or("err.no_library")?;
        if !paths.setup_script.is_file() {
            return Err("err.extra_missing".into());
        }
        let started = crate::wine_setup::start(
            state.inner(),
            &game_id,
            &paths,
            crate::wine_setup::Record::Always,
        )
        .await?;
        let Some(started) = started else {
            return Err("err.setup_native".into());
        };
        tauri::async_runtime::spawn(started.wait(crate::wine_setup::LIMIT));
        return Ok(());
    }
    let settings = state.settings.read().await.clone();
    let paths = settings
        .library
        .game_paths(&game_id)
        .ok_or("err.no_library")?;
    let plan = lanlauncher_core::launch::windows::setup_plan(
        &paths,
        &game_id,
        &settings.game_language,
        &settings.safe_player_name(),
    )
    .ok_or("err.extra_missing")?;
    let stem = format!("{game_id}-setup-log");
    let log = lanlauncher_core::launch::elevate::batch_log_path(&state.run_dir(), &stem);
    let mut attempt = crate::state::LaunchAttempt::new(&game_id, &game_id, "setup", &plan);
    attempt.elevated = true;
    attempt.captured = true;
    attempt.log = Some(log.clone());
    let stamp = attempt.at;
    *state.last_launch.write().await = Some(attempt);
    // Not `console`: this run exists to be read. It is also not waited for —
    // a script asking a question would never return.
    let line = lanlauncher_core::launch::elevate::batch_line(&plan).into();
    let st = state.inner().clone();
    let cwd = paths.share_dir.clone();
    tauri::async_runtime::spawn(async move {
        let run = crate::fixes::run_admin_lines_at(&st, &stem, vec![line], &cwd).await;
        let output = st.launch_output_since(&log, stamp);
        let mut slot = st.last_launch.write().await;
        if let Some(rec) = slot.as_mut().filter(|r| r.at == stamp) {
            rec.ended = true;
            rec.output = output;
            if let Err(e) = run {
                rec.error = Some(e);
            }
        }
    });
    Ok(())
}

/// The interface reporting that it is running. Called once, on mount.
///
/// Without it the launcher cannot tell a webview that came up from one that
/// opened a window and never ran anything — the case that leaves a user in
/// front of a blank rectangle with nothing in the log.
#[tauri::command]
pub fn frontend_ready() {
    crate::FRONTEND_READY.store(true, std::sync::atomic::Ordering::Relaxed);
    log::info!("interface ready");
}

/// What the launcher started last and how it went — the diagnostics page's
/// answer to "a window opened and nothing happened".
#[tauri::command]
pub async fn get_last_launch(
    state: State<'_, Arc<AppState>>,
) -> Cmd<Option<crate::state::LaunchAttempt>> {
    let mut last = state.last_launch.read().await.clone();
    // While the game is still running the file grows; read it on every ask
    // rather than only when the process ends.
    if let Some(rec) = last.as_mut().filter(|r| !r.ended) {
        if let Some(path) = rec.log.clone() {
            rec.output = state.launch_output(&path);
        }
    }
    Ok(last)
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
    let paths = state
        .settings
        .read()
        .await
        .library
        .game_paths(&game_id)
        .ok_or("err.no_library")?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || -> Cmd<()> {
        let _one_at_a_time = state.config_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut receipt = Receipt::load(&paths.receipt).ok_or("err.not_installed")?;
        // With a launch configuration of the tester's on this platform, the
        // choice goes into it — and starts the way a choice in the receipt
        // does, bare: the configuration's arguments and folder were for
        // another executable. In the receipt it would win at start and
        // ignore the configuration altogether.
        let platform = Manifest::current_platform();
        let config = match cfg!(windows) {
            true => None,
            // An unreadable configuration is not in force: the receipt, as
            // before there were configurations.
            false => state
                .manifests
                .load_config_for_update(&game_id)
                .ok()
                .flatten()
                .filter(|c| c.platform.contains_key(platform)),
        };
        if let Some(mut config) = config {
            let path = state
                .manifests
                .config_path(&game_id)
                .ok_or("err.unknown_game")?;
            let mut block = config.platform.remove(platform).unwrap_or_default();
            block.exe = Some(exe);
            block.args = Some(Vec::new());
            block.workdir = Some(String::new());
            let text = lanlauncher_core::game_config::updated_overlay(
                Some(config),
                &game_id,
                platform,
                Some(block),
                &receipt.revision,
            )
            .map_err(|e| e.0)?;
            write_config(&path, text)?;
            return clear_exe_override(&paths);
        }
        receipt.exe_override = Some(exe);
        receipt.save(&paths.receipt).map_err(err)
    })
    .await
    .map_err(err)?
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
    let _lifecycle = state.transport_lifecycle.lock().await;
    let mut current = state.settings.write().await;
    let mut new = settings;
    if state.demo {
        new.library = current.library.clone();
        new.transport = TransportMode::Demo;
        new.setup_complete = true;
    }
    // Neither of these has a field in the settings dialog. The frontend posts
    // the snapshot it fetched at start-up, so changes made in game details or
    // diagnostics in the meantime must not be dropped here.
    new.ignored_problems = current.ignored_problems.clone();
    new.game_runners = current.game_runners.clone();
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
    let library_changed = current.library != new.library;
    let catalog_changed = current.catalog_key != new.catalog_key || old_root != new_root;
    // Binary and API key both go into the engine config: restart on change.
    let binary_changed = current.resilio_binary != new.resilio_binary
        || current.resilio_api_key != new.resilio_api_key
        || current.player_name != new.player_name;
    if current.language != new.language {
        crate::tray::relabel(&app, &new.language);
    }
    *current = new.clone();
    drop(current);
    // Publish library changes before returning from Save: an immediate
    // install must see all configured roots, not last polling round's list.
    if let Ok(mut library) = state.library.write() {
        let old = std::mem::replace(&mut *library, new.library.clone());
        crate::update_media_scope(&app, &old.roots, &new.library);
    }
    let restarting = binary_changed && new.transport == TransportMode::Managed && !state.demo;
    // Disconnect the previous catalog before a restart registers its
    // successor; never remove a just-registered share on the new engine.
    if catalog_changed && !state.demo {
        if let Some(old) = &old_root {
            if let Some(t) = state.transport.read().await.clone() {
                let old_dir = old.join(lanlauncher_core::paths::LAUNCHER_SHARE_ID);
                if let Err(e) = t.remove_share(&old_dir).await {
                    log::warn!("cannot remove old catalog share {}: {e}", old_dir.display());
                }
            }
        }
    }
    if restarting {
        // A different engine binary only takes effect with a fresh transport;
        // the error, if any, is shown by the next diagnostics run.
        let _ = restart_transport_locked(&state).await;
    }
    if catalog_changed && !restarting && !state.demo {
        // The wizard saves a root without restarting the transport, so the
        // catalog share is (re-)registered right here. The old registration
        // is dropped first: a moved root must not sync the catalog twice, and
        // Resilio ignores re-adding a known folder with a different key.
        crate::register_catalog_share(&state).await;
    }
    // Switched on or off, or a new name to announce.
    crate::chat::apply_settings(&app, state.inner()).await;
    // Newly added secondary roots may already contain a catalog. Load in
    // the background without waiting for the periodic file watcher.
    if library_changed && !state.demo {
        let st = state.inner().clone();
        tauri::async_runtime::spawn(async move {
            if let Some(n) = crate::reload_catalog(&st, true, true).await {
                use tauri::Emitter;
                log::info!("catalog loaded after settings change: {n} games");
                let _ = app.emit(crate::CATALOG_EVENT, n);
            }
        });
    }
    Ok(new)
}

/// The zones of a running firewalld, or `None` where there is none (SteamOS)
/// or it cannot be asked. Listing zones needs no root: firewalld answers
/// read-only queries for every user.
async fn firewalld_zones() -> Option<Vec<diagnostics::FirewalldZone>> {
    // One call: without a running firewalld it fails too (exit 252), so a
    // separate `--state` would only cost a second interpreter start.
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        launch::host_command("firewall-cmd")
            .arg("--list-all-zones")
            // A hung D-Bus must not leave one `firewall-cmd` behind per run.
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    output
        .status
        .success()
        .then(|| diagnostics::parse_firewalld_zones(&String::from_utf8_lossy(&output.stdout)))
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

    // Asking Windows for the network profiles costs a PowerShell start and a
    // module load; it runs while the engine is queried instead of after it.
    let profiles = cfg!(target_os = "windows").then(|| {
        checks.push("network_profile".into());
        tauri::async_runtime::spawn(crate::fixes::network_profiles())
    });
    // Only the managed engine is ours to worry about, as with the Windows
    // firewall: folder mode or a demo has no sync port to open.
    let transport = state.transport.read().await.clone();
    let managed = transport
        .as_ref()
        .is_some_and(|t| t.kind() == lanlauncher_core::transport::TransportKind::Resilio);
    let chat = state.chat.read().await.clone();
    let chat_on = chat.is_some();
    // Beacons of other launchers arrive: whatever the rules say, the chat is
    // let in. A missing rule only matters while nothing has come through.
    let chat_heard = chat.as_ref().is_some_and(|c| c.heard_from_others());
    // A Python start and a D-Bus round trip; alongside the engine query too.
    let firewalld = (cfg!(target_os = "linux") && (managed || chat_on)).then(|| {
        checks.push("firewalld".into());
        tauri::async_runtime::spawn(firewalld_zones())
    });

    checks.push("transport".into());
    let mut catalog_connected = false;
    if let Some(t) = &transport {
        let health = t.health().await;
        catalog_connected = health.running
            && health.api_reachable
            && (health.server_found == Some(true) || health.activity.is_some());
        problems.extend(diagnostics::check_transport(&health));
        // The bundled (or downloaded) engine runs in place and no installer
        // added firewall rules for it; a system Resilio or ETI's btsync.exe
        // brought their own rules and are left alone.
        if health.kind == lanlauncher_core::transport::TransportKind::Resilio {
            let (override_path, resource_dir, data_dir) = (
                settings.resilio_binary.clone(),
                state.resource_dir.clone(),
                state.dirs.data.clone(),
            );
            let ours = tauri::async_runtime::spawn_blocking(move || {
                let found = lanlauncher_core::transport::resilio::locate_binary_detailed(
                    override_path.as_deref(),
                    resource_dir.as_deref(),
                    &data_dir,
                )
                .found?;
                let in_ours = resource_dir
                    .as_deref()
                    .is_some_and(|r| found.starts_with(r))
                    || found.starts_with(&data_dir);
                in_ours.then_some(found)
            })
            .await
            .ok()
            .flatten();
            if let Some(program) = ours {
                if crate::fixes::firewall_rule_present(diagnostics::FIREWALL_RULE_IN).await
                    == Some(false)
                {
                    problems.push(diagnostics::firewall_missing_problem(&program));
                }
            }
        }
    }
    if let Some(handle) = profiles {
        let found = handle.await.unwrap_or_default();
        problems.extend(diagnostics::check_network_profiles(&found));
    }
    if chat_on && !chat_heard && cfg!(target_os = "windows") {
        checks.push("chat_firewall".into());
        if crate::fixes::firewall_rule_present(diagnostics::FIREWALL_RULE_CHAT).await == Some(false)
        {
            problems.push(diagnostics::chat_firewall_missing_problem());
        }
    }
    if let Some(handle) = firewalld {
        let zones = handle.await.ok().flatten();
        if let (true, Some(zones)) = (chat_on && !chat_heard, &zones) {
            problems.extend(diagnostics::check_firewalld_chat(zones));
        }
        if let (true, Some(zones)) = (managed, zones) {
            // A port of 0 in the settings lets the engine pick; which one it
            // took is in its listening sockets.
            let random = settings.sync_port == 0;
            let pid = transport.as_ref().and_then(|t| t.process_id());
            let port = match (settings.sync_port, pid) {
                (0, Some(pid)) => tauri::async_runtime::spawn_blocking(move || {
                    diagnostics::lan_listening_ports(pid).first().copied()
                })
                .await
                .ok()
                .flatten()
                .unwrap_or(0),
                (port, _) => port,
            };
            problems.extend(diagnostics::check_firewalld(&zones, port, random));
        }
    }
    if cfg!(target_os = "linux") {
        checks.push("appimage".into());
        problems.extend(diagnostics::check_appimage_unpacked(
            std::env::var("APPIMAGE").ok().as_deref(),
            std::env::var("APPDIR").ok().as_deref(),
            std::env::var("APPIMAGE_EXTRACT_AND_RUN").ok().as_deref(),
        ));
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
            .step("transport.start_failed.step.api_key")
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
            let share = if let (Some(t), Some(root)) = (&transport, settings.library.default_root())
            {
                t.share_status(&root.path.join(lanlauncher_core::paths::LAUNCHER_SHARE_ID))
                    .await
                    .ok()
                    .flatten()
            } else {
                None
            };
            problems.push(diagnostics::catalog_pending_problem(
                catalog_connected,
                share.as_ref(),
            ));
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

    Ok(Report::new(problems, checks).hide_ignored(|k| settings.problem_ignored(k)))
}

/// Hide a diagnostics warning for good, or show it again. Only problems that
/// carry a dismiss key can be hidden; the UI offers the button for those.
#[tauri::command]
pub async fn set_problem_ignored(
    state: State<'_, Arc<AppState>>,
    key: String,
    ignored: bool,
) -> Cmd<()> {
    let mut settings = state.settings.write().await;
    if settings.set_problem_ignored(&key, ignored) {
        settings.save(&state.settings_path()).map_err(err)?;
        log::info!(
            "diagnostics warning {key}: {}",
            if ignored { "hidden" } else { "shown again" }
        );
    }
    Ok(())
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
    let allow = state.settings.read().await.allow_elevation;
    launch::spawn_for_user(&launch::prereq_plan(&exe), &state.run_dir(), allow)
        .await
        .map_err(err)
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
        || url.starts_with("adcs://")
        // The "send by mail" button of the launch configuration.
        || url.starts_with("mailto:"))
    {
        return Err("err.unsupported_link".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(err)
}

/// The Resilio read-only key of a game, for folder mode where the user adds
/// the share manually. Not available in managed mode to keep keys private.
#[tauri::command]
pub async fn get_share_key(state: State<'_, Arc<AppState>>, game_id: String) -> Cmd<String> {
    // The *running* transport decides, not the setting: without a working
    // engine the launcher falls back to folder mode by itself, and that is
    // exactly when someone needs the key to paste into their own Resilio.
    let transport = state.transport.read().await.clone();
    let folder_mode = match transport {
        Some(t) => t.kind() == lanlauncher_core::transport::TransportKind::Folder,
        // Still starting: `build_transport` falls back to folder mode by
        // itself, so "no transport yet" says nothing about the mode.
        None => false,
    };
    if !folder_mode {
        return Err("err.key_folder_mode_only".into());
    }
    let catalog = state.catalog().await;
    let game = catalog.game(&game_id).ok_or("err.unknown_game")?;
    Ok(game.key.expose().to_string())
}

/// Peers of a game's share with their current rates, for the expanded
/// download panel. Empty when the engine cannot tell (folder mode, no API
/// key); queried only while the panel is open.
#[tauri::command]
pub async fn get_share_peers(
    state: State<'_, Arc<AppState>>,
    game_id: String,
) -> Cmd<Vec<lanlauncher_core::transport::SharePeer>> {
    let paths = state
        .settings
        .read()
        .await
        .library
        .game_paths(&game_id)
        .ok_or("err.no_library")?;
    let transport = state
        .transport
        .read()
        .await
        .clone()
        .ok_or("err.not_ready")?;
    transport.share_peers(&paths.share_dir).await.map_err(err)
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
    let _lifecycle = state.transport_lifecycle.lock().await;
    restart_transport_locked(state).await
}

/// Caller holds `transport_lifecycle` for the entire replacement.
async fn restart_transport_locked(state: &Arc<AppState>) -> Cmd<()> {
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

/// What the launch-configuration editor in the game details starts from.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameConfigView {
    pub config: lanlauncher_core::game_config::GameConfig,
    /// The tester's own configuration is in force on this platform.
    pub own: bool,
    /// The saved configuration exists but does not read; only a reset
    /// clears it.
    pub config_error: Option<String>,
    /// Executables inside `local/` to choose from.
    pub executables: Vec<String>,
    pub platform: &'static str,
    /// The address the "send by mail" button writes to.
    pub report_email: &'static str,
}

/// The game and its paths, from one settings copy.
async fn game_and_paths(
    state: &AppState,
    game_id: &str,
) -> Cmd<(Game, lanlauncher_core::paths::GamePaths)> {
    let game = state
        .catalog()
        .await
        .game(game_id)
        .cloned()
        .ok_or("err.unknown_game")?;
    let paths = state
        .settings
        .read()
        .await
        .library
        .game_paths(game_id)
        .ok_or("err.no_library")?;
    Ok((game, paths))
}

/// The package a tester actually ran: the installed one, which is not the
/// catalog's while an update is pending.
fn installed_revision(game: &Game, paths: &lanlauncher_core::paths::GamePaths) -> String {
    Receipt::load(&paths.receipt)
        .map(|r| r.revision)
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| game.revision.clone())
}

/// The game, its paths and the profile in force (with the tester's
/// configuration laid over it), from one settings copy.
async fn config_basis(
    state: &AppState,
    game_id: &str,
) -> Cmd<(Game, lanlauncher_core::paths::GamePaths, Option<Manifest>)> {
    let (game, paths) = game_and_paths(state, game_id).await?;
    let manifest = resolve_manifest(state, &game, Some(&paths));
    Ok((game, paths, manifest))
}

/// What starts now, as the editor's configuration: the receipt's executable
/// choice included, since it wins at start.
fn config_in_force(
    manifest: Option<&Manifest>,
    paths: &lanlauncher_core::paths::GamePaths,
) -> lanlauncher_core::game_config::GameConfig {
    let receipt = Receipt::load(&paths.receipt);
    lanlauncher_core::game_config::GameConfig::from_manifest(
        manifest,
        Manifest::current_platform(),
        receipt.as_ref().and_then(|r| r.exe_override.as_deref()),
    )
}

#[tauri::command]
pub async fn get_game_config(
    state: State<'_, Arc<AppState>>,
    game_id: String,
) -> Cmd<GameConfigView> {
    let (_, paths, manifest) = config_basis(&state, &game_id).await?;
    let state_manifests = state.manifests.clone();
    tauri::async_runtime::spawn_blocking(move || GameConfigView {
        config: config_in_force(manifest.as_ref(), &paths),
        own: manifest.as_ref().is_some_and(|m| m.user_config),
        config_error: state_manifests
            .load_config_for_update(&game_id)
            .err()
            .map(|e| e.to_string()),
        executables: launch::list_executables(&paths, 200),
        platform: Manifest::current_platform(),
        report_email: lanlauncher_core::game_config::REPORT_EMAIL,
    })
    .await
    .map_err(err)
}

/// Write a game's configuration file, or remove it when `text` is `None`.
fn write_config(path: &std::path::Path, text: Option<String>) -> Cmd<()> {
    let failed = |e: std::io::Error| format!("err.config_write|{e}");
    match text {
        Some(text) => {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(failed)?;
            }
            let staging = path.with_extension("toml.tmp");
            std::fs::write(&staging, text).map_err(failed)?;
            std::fs::rename(&staging, path).map_err(failed)
        }
        None => match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(failed(e)),
            _ => Ok(()),
        },
    }
}

/// Store the editor's settings as this platform's block of the game's
/// configuration (`<data>/game-configs/<id>.toml`), laid over the profile.
#[tauri::command]
pub async fn save_game_config(
    state: State<'_, Arc<AppState>>,
    game_id: String,
    config: lanlauncher_core::game_config::GameConfig,
) -> Cmd<bool> {
    // The tool, and with it the prefix, may change.
    let _use = GameUse::claim(&state.prefix_use, &game_id)?;
    let (game, paths) = game_and_paths(&state, &game_id).await?;
    let state = state.inner().clone();
    // Reading profiles, the receipt and the configuration: a blocking thread.
    tauri::async_runtime::spawn_blocking(move || -> Cmd<bool> {
        let _one_at_a_time = state.config_lock.lock().unwrap_or_else(|e| e.into_inner());
        let platform = Manifest::current_platform();
        // Compared against the profile alone: the block keeps only what the
        // tester changed, everything else stays the profile's.
        let profile = state.manifests.resolve_profile_for(&game_id, &paths);
        let block = config
            .to_block(profile.as_ref(), &game_id, platform)
            .map_err(|e| e.0)?;
        let path = state
            .manifests
            .config_path(&game_id)
            .ok_or("err.unknown_game")?;
        let current = state
            .manifests
            .load_config_for_update(&game_id)
            .map_err(|e| format!("err.config_unreadable|{e}"))?;
        // A block that changes nothing leaves no configuration behind on
        // this platform, whatever other platforms the file still holds.
        let own = !block.is_empty();
        let text = lanlauncher_core::game_config::updated_overlay(
            current,
            &game_id,
            platform,
            Some(block),
            &installed_revision(&game, &paths),
        )
        .map_err(|e| e.0)?;
        write_config(&path, text)?;
        // The editor showed the receipt's executable choice as its exe and
        // has now saved it into the configuration. Left in the receipt it
        // would win over whatever the tester picks next. Saving again
        // repeats both steps, so a failure here is fixed by the next save;
        // it is reported as what it is, not as "not saved".
        clear_exe_override(&paths)?;
        log::info!(
            "saved launch configuration for {game_id} to {}",
            path.display()
        );
        Ok(own)
    })
    .await
    .map_err(err)?
}

/// Remove an executable choice from the receipt, where it would win over
/// the launch configuration at start.
fn clear_exe_override(paths: &lanlauncher_core::paths::GamePaths) -> Cmd<()> {
    if let Some(mut receipt) = Receipt::load(&paths.receipt) {
        if receipt.exe_override.take().is_some() {
            receipt
                .save(&paths.receipt)
                .map_err(|e| format!("err.config_receipt|{e}"))?;
        }
    }
    Ok(())
}

/// Drop this platform's configuration; the profile applies again.
#[tauri::command]
pub async fn reset_game_config(state: State<'_, Arc<AppState>>, game_id: String) -> Cmd<()> {
    let _use = GameUse::claim(&state.prefix_use, &game_id)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || -> Cmd<()> {
        let _one_at_a_time = state.config_lock.lock().unwrap_or_else(|e| e.into_inner());
        let path = state
            .manifests
            .config_path(&game_id)
            .ok_or("err.unknown_game")?;
        // A file that does not read is what "reset" is for: it goes.
        let Ok(current) = state.manifests.load_config_for_update(&game_id) else {
            log::warn!(
                "removing unreadable launch configuration {}",
                path.display()
            );
            return write_config(&path, None);
        };
        let text = lanlauncher_core::game_config::updated_overlay(
            current,
            &game_id,
            Manifest::current_platform(),
            None,
            "",
        )
        .map_err(|e| e.0)?;
        write_config(&path, text)
    })
    .await
    .map_err(err)?
}

/// The report for sending what starts now: the profile with this
/// platform's settings in it, the test context and a tester's note.
#[tauri::command]
pub async fn share_game_config(
    state: State<'_, Arc<AppState>>,
    game_id: String,
    comment: String,
) -> Cmd<lanlauncher_core::game_config::Report> {
    use lanlauncher_core::game_config;
    let (game, paths, manifest) = config_basis(&state, &game_id).await?;
    let platform = Manifest::current_platform();
    // Built from what starts — the receipt's executable choice included,
    // and an exe the start script chose for a guidance-only profile: that is
    // what ran, and the maintainer learns it worked. The block goes onto the
    // profile as shipped, so `[launch]` stays free of those guesses.
    let in_force = config_in_force(manifest.as_ref(), &paths);
    // A shipped profile that does not load is the tester's to hear about:
    // a profile built from nothing would go out as if it were the whole one.
    let shipped = state
        .manifests
        .resolve(&game_id, Some(&paths.share_dir))
        .map_err(|e| format!("err.config_profile|{e}"))?;
    let shared = match in_force.to_block(shipped.as_ref(), &game_id, platform) {
        Ok(block) => {
            game_config::with_block(shipped.as_ref(), &game_id, &game.title, platform, &block)
        }
        // Nothing that starts (a guidance-only profile with no executable):
        // the profile as shipped, never one with this machine's guesses.
        Err(e) if e.0 == "err.config_exe_missing" => shipped.unwrap_or_else(|| Manifest {
            id: game_id.clone(),
            title: Some(game.title.clone()),
            ..Default::default()
        }),
        // Anything else is a configuration that does not load: the tester
        // has to hear about it, not get the shipped profile sent instead.
        Err(e) => return Err(e.0),
    };
    let revision = installed_revision(&game, &paths);
    // The links carry the platform's block as it belongs in the profile.
    let block = shared
        .platform
        .get(platform)
        .and_then(|b| game_config::block_toml(&game_id, platform, b).ok());
    let toml = game_config::to_toml(&shared).map_err(|e| e.0)?;
    // The tool the game ran with, from its last start; only without one is
    // a plan built for it — that searches every Steam library.
    let last = state
        .last_launch
        .read()
        .await
        .as_ref()
        .filter(|attempt| attempt.game_id == game_id && attempt.what == "play")
        .map(|attempt| attempt.runner.clone());
    let runner = match last {
        Some(runner) => runner,
        None => build_plan(&state, &game_id, None)
            .await
            .map(|plan| plan.runner)
            .unwrap_or_default(),
    };
    let version = crate::app_version();
    tauri::async_runtime::spawn_blocking(move || {
        let context = game_config::this_machine(&version, &runner);
        game_config::report(
            &game_id,
            &game.title,
            &revision,
            &toml,
            block.as_deref(),
            &context,
            &comment,
        )
    })
    .await
    .map_err(err)
}

/// Save a report's profile where the tester picks in the system's save
/// dialog. The path never comes from the web view: it may only name the
/// file name to suggest, so no page can write a file of its choosing.
#[tauri::command]
pub async fn export_game_config(
    app: tauri::AppHandle,
    file_name: String,
    contents: String,
) -> Cmd<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let suggested = std::path::Path::new(&file_name)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "game-config.toml".into());
    tauri::async_runtime::spawn_blocking(move || -> Cmd<Option<String>> {
        let Some(chosen) = app
            .dialog()
            .file()
            .set_file_name(&suggested)
            .add_filter("TOML", &["toml"])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = chosen
            .into_path()
            .map_err(|e| format!("err.config_write|{e}"))?;
        std::fs::write(&path, contents).map_err(|e| format!("err.config_write|{e}"))?;
        Ok(Some(path.display().to_string()))
    })
    .await
    .map_err(err)?
}

/// What installing a game's Windows components came to.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentsReport {
    #[serde(flatten)]
    pub outcome: launch::winetricks::Outcome,
    /// winetricks' output, for a failure the message cannot explain.
    pub log: String,
}

/// A start, uninstall, repair or cancel of `game` in progress, see
/// [`crate::state::AppState::prefix_use`]. Refused while components are
/// being installed for the game: its prefix lives among its files.
struct GameUse<'a>(&'a std::sync::Mutex<crate::state::PrefixUse>, String);

impl<'a> GameUse<'a> {
    fn claim(lock: &'a std::sync::Mutex<crate::state::PrefixUse>, game: &str) -> Cmd<Self> {
        lock.lock()
            .unwrap_or_else(|e| e.into_inner())
            .mark_busy(game)?;
        Ok(Self(lock, game.to_string()))
    }
}

impl Drop for GameUse<'_> {
    fn drop(&mut self) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .unmark_busy(&self.1);
    }
}

/// The component installation in progress, cleared when it ends.
struct ComponentsRun<'a>(&'a std::sync::Mutex<crate::state::PrefixUse>);

impl<'a> ComponentsRun<'a> {
    fn claim(
        lock: &'a std::sync::Mutex<crate::state::PrefixUse>,
        game: &str,
        prefix: &std::path::Path,
    ) -> Cmd<Self> {
        let mut used = lock.lock().unwrap_or_else(|e| e.into_inner());
        if used.installing.is_some() {
            return Err("err.components_busy".into());
        }
        // Any game, not only this one: profiles may share a prefix, and a
        // start, repair or uninstall takes seconds.
        if !used.busy.is_empty() {
            return Err("err.components_game_busy".into());
        }
        used.installing = Some((game.to_string(), prefix.to_path_buf()));
        Ok(Self(lock))
    }
}

impl Drop for ComponentsRun<'_> {
    fn drop(&mut self) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).installing = None;
    }
}

/// A prefix as [`AppState::prefix_use`] keeps it: resolved before the lock
/// is taken, so no file system call happens under it. Through the nearest
/// folder that exists — a prefix winetricks is about to create resolves to
/// the same key before and after (`/home` → `/var/home` on Bazzite).
fn prefix_key(prefix: &std::path::Path) -> std::path::PathBuf {
    let mut rest = Vec::new();
    let mut dir = prefix;
    loop {
        if let Ok(real) = std::fs::canonicalize(dir) {
            return rest.iter().rev().fold(real, |path, part| path.join(part));
        }
        match (dir.file_name(), dir.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name.to_os_string());
                dir = parent;
            }
            _ => return prefix.to_path_buf(),
        }
    }
}

/// Whether `plan` would start in the prefix components are being
/// installed into right now — another game's, when profiles share one.
pub(crate) fn prefix_being_filled(state: &AppState, plan: &LaunchPlan) -> bool {
    let running = state
        .prefix_use
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .installing
        .is_some();
    if !running {
        return false;
    }
    let Ok(target) = launch::winetricks::target(plan) else {
        return false;
    };
    let key = prefix_key(target.prefix());
    let used = state.prefix_use.lock().unwrap_or_else(|e| e.into_inner());
    used.installing
        .as_ref()
        .is_some_and(|(_, prefix)| *prefix == key)
}

/// Into an installed game only: one never installed or just removed has no
/// receipt (`installed`), and a repair or update being checked or extracted
/// shows in its phase. An update still downloading leaves the old version
/// in place, and that one starts.
async fn ready_for_components(state: &AppState, game_id: &str, installed: bool) -> Cmd<()> {
    use lanlauncher_core::install::Phase;
    let phase = manager(state).await?.tracker_phase(game_id).await;
    if !installed
        || matches!(
            phase,
            Some(Phase::Verifying | Phase::Extracting | Phase::Setup)
        )
    {
        return Err("err.components_not_ready".into());
    }
    Ok(())
}

/// The prefix of a Proton game that never started, for its components:
/// made by the profile's settings run. A run that could not make it is
/// `err.components_prefix` (its transcript is in the game's logs).
async fn make_proton_prefix(
    state: &Arc<AppState>,
    game_id: &str,
    alternative: Option<usize>,
    paths: lanlauncher_core::paths::GamePaths,
) -> Cmd<()> {
    let (plan, _) = with_launch_context(state, game_id, alternative, Some(paths.clone()), |ctx| {
        launch::unix::script_plan(ctx, launch::setup_script::Script::Settings)
    })
    .await?;
    let plan = plan.ok_or(launch::winetricks::Refusal::StartFirst.code())?;
    crate::player::make_prefix(state, game_id, &paths, plan)
        .await
        .map_err(|e| {
            log::warn!("components {game_id}: cannot make the prefix ({e})");
            "err.components_prefix".to_string()
        })
}

/// Install the Windows components the profile in force names (winetricks
/// verbs) into the prefix the game starts in (with `alternative`, as the
/// start that asked). Only on request: it takes minutes and usually needs
/// the internet once. One run at a time, and the game does not start into a
/// prefix that is being changed.
#[tauri::command]
pub async fn install_components(
    state: State<'_, Arc<AppState>>,
    game_id: String,
    force: bool,
    alternative: Option<usize>,
) -> Cmd<ComponentsReport> {
    if cfg!(windows) {
        return Err("err.components_native".into());
    }
    if state.demo {
        return Err("err.demo_no_play".into());
    }
    // The verbs are not part of the plan; both come from one resolution, so
    // a configuration saved meanwhile cannot mix into one of them.
    let (plan, manifest) = plan_with_profile(&state, &game_id, alternative).await?;
    let verbs = manifest
        .as_ref()
        .map(|m| m.launch_for(Manifest::current_platform()).winetricks)
        .unwrap_or_default();
    if verbs.is_empty() {
        return Err("err.components_none".into());
    }
    let paths = state.settings.read().await.library.game_paths(&game_id);
    // File system work on a blocking thread, not on a worker other commands
    // wait for (an SD card is slow). Asked before anything runs in the
    // prefix: winetricks, or the run below, may be what creates it, and the
    // record of who used it must not take that for a prefix from before.
    let (receipt, existed_before) = {
        let (paths, plan, game_id) = (paths.clone(), plan.clone(), game_id.clone());
        tauri::async_runtime::spawn_blocking(move || {
            let receipt = paths
                .as_ref()
                .is_some_and(|paths| Receipt::load(&paths.receipt).is_some());
            let existed_before = paths.as_ref().is_some_and(|paths| {
                launch::unix::own_prefix_exists_before_start(&plan, paths, &game_id)
            });
            (receipt, existed_before)
        })
        .await
        .map_err(err)?
    };
    let target = match launch::winetricks::target(&plan) {
        // Proton makes its prefix on a start; a run of nothing makes it here.
        Err(launch::winetricks::Refusal::StartFirst)
            if plan.env.contains_key("STEAM_COMPAT_DATA_PATH") =>
        {
            ready_for_components(&state, &game_id, receipt).await?;
            let paths = paths.clone().ok_or("err.components_not_ready")?;
            make_proton_prefix(&state, &game_id, alternative, paths).await?;
            launch::winetricks::target(&plan).map_err(|_| "err.components_prefix".to_string())?
        }
        found => found.map_err(|refusal| refusal.code())?,
    };
    let key = {
        let prefix = target.prefix().to_path_buf();
        tauri::async_runtime::spawn_blocking(move || prefix_key(&prefix))
            .await
            .map_err(err)?
    };
    let _run = ComponentsRun::claim(&state.prefix_use, &game_id, &key)?;
    // Asked again after the claim: a repair may have begun meanwhile.
    ready_for_components(&state, &game_id, receipt).await?;
    // After the claim: a start from now on is refused, and one before it
    // runs in the prefix by now. A game there would share its wineserver
    // with the installers, and the time limit's `wineboot -k` would end it.
    let probe = target.clone();
    let in_use =
        tauri::async_runtime::spawn_blocking(move || launch::winetricks::prefix_in_use(&probe))
            .await
            .map_err(err)?;
    if in_use {
        return Err("err.components_game_running".into());
    }
    let host = launch::winetricks::HostEnv::current();
    let (resource_dir, data_dir, path) = (
        state.resource_dir.clone(),
        state.dirs.data.clone(),
        host.path.clone(),
    );
    let proton = matches!(target, launch::winetricks::Target::Proton { .. });
    let tools = tauri::async_runtime::spawn_blocking(move || {
        launch::winetricks::tools(resource_dir.as_deref(), &data_dir, &path, proton)
    })
    .await
    .map_err(err)?
    .ok_or("err.components_no_winetricks")?;
    let job = launch::winetricks::job(&target, &verbs, force, &tools, &host);
    let log = state.launch_log(&game_id, "winetricks");
    // Long enough for the .NET runtimes; an installer waiting for a click
    // nobody sees ends here instead of holding the prefix for ever.
    let outcome = launch::winetricks::run(&job, &log, std::time::Duration::from_secs(45 * 60))
        .await
        .map_err(|e| format!("err.components_run|{e}"))?;
    // What a start asks for before it runs (`launch::winetricks::missing`).
    {
        let (target, installed, game_id) =
            (target.clone(), outcome.installed.clone(), game_id.clone());
        let _ = tauri::async_runtime::spawn_blocking(move || {
            if let Err(e) = launch::winetricks::remember(&target, &installed) {
                log::warn!("components {game_id}: cannot note what was installed ({e})");
            }
        })
        .await;
    }
    // The game's Wine or Proton ran in its prefix, as on a start.
    if let Some(paths) = paths {
        let game_id = game_id.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            launch::unix::remember_default_prefix_user(&plan, &paths, &game_id, existed_before)
        })
        .await;
    }
    Ok(ComponentsReport {
        outcome,
        log: log.display().to_string(),
    })
}
