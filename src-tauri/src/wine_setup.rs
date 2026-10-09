//! macOS/Linux: a game's `game_setup.cmd` in the game's prefix.
//!
//! What runs, and what is left out, is `lanlauncher_core::launch::setup_script`'s
//! business; this is the part that needs the launcher's state: the runner the
//! game starts with, the prefix claims, the diagnostics page and the receipt.

use crate::state::{AppState, LaunchAttempt};
use lanlauncher_core::install::Receipt;
use lanlauncher_core::launch::setup_script::{self, Script, Skipped};
use lanlauncher_core::launch::{self, ExitWatch, LaunchPlan};
use lanlauncher_core::paths::GamePaths;
use std::sync::Arc;
use std::time::Duration;

/// A helper that waits for a click nobody sees must not keep the game in
/// "setting up" for ever. The script goes on running and its claim stays
/// ([`SetupClaim`]); only the waiting stops, and the game is not marked.
pub(crate) const LIMIT: Duration = Duration::from_secs(30 * 60);

/// The same before a start: the player is waiting in front of the button,
/// and a helper that asks something shows its window meanwhile.
const CATCH_UP_LIMIT: Duration = Duration::from_secs(10 * 60);

/// What a start script prepares — a name into a config file, a value into
/// the registry — takes seconds; after this the game starts all the same.
const PREPARATION_LIMIT: Duration = Duration::from_secs(2 * 60);

/// Whether the diagnostics page shows the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Record {
    /// Asked for ("Repeat the setup"): from the start on.
    Always,
    /// After an install: only when it could not start or did not end in
    /// time. The page belongs to whatever the user started last; an exit
    /// code means little under Wine (`cmd /c` returns that of the script's
    /// last command) and goes to the log.
    OnFailure,
    /// Before a start, which already holds the game: as `OnFailure`.
    BeforeStart,
}

/// A running setup holds its game like a start does (`prefix_use.busy`, so
/// no component installation begins beside it) and is listed in
/// `prefix_use.setups`, so that neither a second setup rewrites the files the
/// first is reading nor the game starts into the prefix it is writing.
pub(crate) struct SetupClaim {
    state: Arc<AppState>,
    game_id: String,
}

impl SetupClaim {
    pub(crate) fn claim(
        state: &Arc<AppState>,
        game_id: &str,
        held_by_caller: bool,
    ) -> Result<Self, String> {
        state
            .prefix_use
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .begin_setup(game_id, held_by_caller)?;
        Ok(Self {
            state: state.clone(),
            game_id: game_id.to_string(),
        })
    }
}

impl Drop for SetupClaim {
    fn drop(&mut self) {
        self.state
            .prefix_use
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .end_setup(&self.game_id);
    }
}

/// A setup script that was started, to be waited for ([`Started::wait`]).
///
/// Dropped before it ended — the wait timed out, or the install job that
/// ran it was cancelled — it hands the process to a task of its own that
/// keeps the claim and, once the process is gone, records the prefix and the
/// finished setup: Wine does not stop because nobody waits for it, and a
/// helper answered after the limit has still done its work.
pub(crate) struct Started {
    paths: GamePaths,
    plan: LaunchPlan,
    /// [`launch::unix::prefix_id`] of `plan`: what the receipt records.
    prefix: Option<String>,
    watch: Option<ExitWatch>,
    attempt: LaunchAttempt,
    record: Record,
    existed_before: bool,
    claim: Option<SetupClaim>,
}

impl Drop for Started {
    fn drop(&mut self) {
        let (Some(watch), Some(claim)) = (self.watch.take(), self.claim.take()) else {
            return;
        };
        let (plan, paths, prefix) = (self.plan.clone(), self.paths.clone(), self.prefix.take());
        let (game_id, existed_before) = (claim.game_id.clone(), self.existed_before);
        let mut attempt = self.attempt.clone();
        tauri::async_runtime::spawn(async move {
            let ended = watch.await;
            remember_prefix(plan, paths.clone(), game_id.clone(), existed_before).await;
            if let Ok(code) = ended {
                log::info!(
                    "setup {game_id}: game_setup.cmd ended after it stopped being waited for (exit code {code:?})"
                );
                mark_done(&paths, prefix.as_deref()).await;
                // The page may still show it as running or timed out.
                attempt.exit_code = code;
                attempt.ended = true;
                attempt.error = None;
                attempt.output = output(&claim.state, &attempt);
                store_if_current(&claim.state, attempt).await;
            }
            drop(claim);
        });
    }
}

/// The record a start keeps of the default prefix; after a setup as well,
/// which is usually what made it.
pub(crate) async fn remember_prefix(
    plan: LaunchPlan,
    paths: GamePaths,
    game_id: String,
    existed_before: bool,
) {
    let _ = tauri::async_runtime::spawn_blocking(move || {
        launch::unix::remember_default_prefix_user(&plan, &paths, &game_id, existed_before)
    })
    .await;
}

/// Start the setup script of `game_id`. `Ok(None)` when nothing runs: no
/// script, or a native game whose Windows script sets nothing up — which
/// writes nothing into its folder either.
///
/// `Err` when it could not start — no Wine or Proton, the files could not be
/// written, components are being installed into the prefix, it is running
/// already — as a code the interface translates.
pub(crate) async fn start(
    state: &Arc<AppState>,
    game_id: &str,
    paths: &GamePaths,
    record: Record,
) -> Result<Option<Started>, String> {
    // Before anything else: no script, nothing to plan — a machine without
    // Wine yet has no runner to find either.
    if !has_script(paths).await {
        return Ok(None);
    }
    // Before the files are written: a run still going reads them.
    let claim = SetupClaim::claim(state, game_id, record == Record::BeforeStart)?;
    let Some(mut plan) = crate::commands::setup_script_plan(state, game_id, paths).await? else {
        log::info!("setup {game_id}: the game runs natively; its Windows setup script is not run");
        return Ok(None);
    };
    // Another game's components may be going into this prefix (profiles can
    // share one); the claim only knows this game.
    if crate::commands::prefix_being_filled(state, &plan) {
        return Err("err.components_busy_game".into());
    }
    let lang = state.settings.read().await.game_language.clone();
    let prepared = {
        let (paths, game_id) = (paths.clone(), game_id.to_string());
        tauri::async_runtime::spawn_blocking(move || {
            setup_script::prepare(&paths, Script::Setup, &game_id, &lang, &[], &[])
                .map_err(|e| format!("err.setup_write|{}: {e}", paths.share_dir.display()))
        })
        .await
        .map_err(|e| format!("err.setup_write|{e}"))??
    };
    let Some(skipped) = prepared else {
        return Ok(None);
    };
    log_skipped(game_id, "setup", &skipped);
    let title = state
        .catalog()
        .await
        .game(game_id)
        .map(|g| g.title.clone())
        .unwrap_or_else(|| game_id.to_string());
    // Not into a game that is running: the setup rewrites its registry.
    if let Ok(target) = launch::winetricks::target(&plan) {
        let in_use = tauri::async_runtime::spawn_blocking(move || {
            launch::winetricks::prefix_in_use(&target)
        })
        .await
        .unwrap_or(false);
        if in_use {
            return Err("err.setup_game_running".into());
        }
    }
    let log = state.launch_log(game_id, "setup");
    // The last run's transcript must not pass for this one's, should this
    // one get none.
    {
        let log = log.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || std::fs::remove_file(log)).await;
    }
    let mut attempt = LaunchAttempt::new(game_id, &title, "setup", &plan);
    attempt.log = Some(log.clone());
    log::info!(
        "setup {game_id}: running game_setup.cmd via {}: {} {}",
        plan.runner,
        plan.command_display(),
        attempt.command_line
    );
    // Asked before the run creates it, as a start does: the game's first
    // start must not take the prefix for one the launcher never recorded.
    let existed_before = {
        let (plan, paths, game_id) = (plan.clone(), paths.clone(), game_id.to_string());
        tauri::async_runtime::spawn_blocking(move || {
            launch::unix::own_prefix_exists_before_start(&plan, &paths, &game_id)
        })
        .await
        .unwrap_or(true)
    };
    with_fnr(state, &mut plan).await;
    let watch = match launch::spawn(&plan, Some(&log)).await {
        Ok((pid, watch)) => {
            attempt.pid = Some(pid);
            watch
        }
        Err(e) => {
            attempt.error = Some(e.to_string());
            attempt.ended = true;
            store(state, attempt).await;
            return Err(e.to_string());
        }
    };
    // `spawn` starts without a transcript when the file cannot be written.
    attempt.captured = {
        let log = log.clone();
        tauri::async_runtime::spawn_blocking(move || log.is_file())
            .await
            .unwrap_or(false)
    };
    if record == Record::Always {
        store(state, attempt.clone()).await;
    }
    Ok(Some(Started {
        paths: paths.clone(),
        prefix: launch::unix::prefix_id(&plan),
        plan,
        watch: Some(watch),
        attempt,
        record,
        existed_before,
        claim: Some(claim),
    }))
}

impl Started {
    /// Wait for the script, at most `limit`. `Ok` once it ended, whatever
    /// its exit code: under Wine a line that does nothing useful (a Windows
    /// tool the prefix lacks) fails just as harmlessly, and the transcript on
    /// the diagnostics page says what happened. The receipt then records the
    /// prefix, before the claim goes: whatever comes next finds it recorded.
    /// `Err("err.setup_timeout")` when it is still running; see [`Started`]
    /// for what happens then.
    pub(crate) async fn wait(mut self, limit: Duration) -> Result<(), String> {
        let Some(claim) = self.claim.as_ref() else {
            return Ok(());
        };
        let (state, game_id) = (claim.state.clone(), claim.game_id.clone());
        let Some(watch) = self.watch.as_mut() else {
            return Ok(());
        };
        let code = match tokio::time::timeout(limit, watch).await {
            Ok(Ok(code)) => code,
            // The process went unreported: not known to have ended, not
            // marked; dropping `self` lets the claim go.
            Ok(Err(_)) => {
                log::warn!("setup {game_id}: the end of game_setup.cmd went unreported");
                return Ok(());
            }
            Err(_) => {
                log::warn!(
                    "setup {game_id}: still running after {} minutes; not marked as set up",
                    limit.as_secs() / 60
                );
                let mut attempt = self.attempt.clone();
                attempt.error = Some("err.setup_timeout".into());
                attempt.output = output(&state, &attempt);
                if self.record == Record::Always {
                    store_if_current(&state, attempt).await;
                } else {
                    store(&state, attempt).await;
                }
                return Err("err.setup_timeout".into());
            }
        };
        self.watch = None;
        remember_prefix(
            self.plan.clone(),
            self.paths.clone(),
            game_id.clone(),
            self.existed_before,
        )
        .await;
        mark_done(&self.paths, self.prefix.as_deref()).await;
        let mut attempt = self.attempt.clone();
        attempt.exit_code = code;
        attempt.ended = true;
        attempt.output = output(&state, &attempt);
        if self.record == Record::Always {
            store_if_current(&state, attempt).await;
        }
        self.claim = None;
        log::info!("setup {game_id}: game_setup.cmd ended (exit code {code:?})");
        Ok(())
    }
}

fn output(state: &AppState, attempt: &LaunchAttempt) -> String {
    attempt
        .log
        .as_deref()
        .map(|log| state.launch_output(log))
        .unwrap_or_default()
}

/// Put `attempt` on the diagnostics page, in place of whatever was there.
async fn store(state: &AppState, attempt: LaunchAttempt) {
    *state.last_launch.write().await = Some(attempt);
}

/// The end of a run the page already shows — unless the user started
/// something else meanwhile, which the page then belongs to.
async fn store_if_current(state: &AppState, attempt: LaunchAttempt) {
    let mut slot = state.last_launch.write().await;
    if slot.as_ref().is_some_and(|r| r.at == attempt.at) {
        *slot = Some(attempt);
    }
}

/// Start the setup and wait for it.
async fn run(
    state: &Arc<AppState>,
    game_id: &str,
    paths: &GamePaths,
    record: Record,
    limit: Duration,
) -> Result<(), String> {
    match start(state, game_id, paths, record).await? {
        Some(started) => started.wait(limit).await,
        None => Ok(()),
    }
}

/// After an install: the setup (the receipt records it, [`Started::wait`]). One that
/// could not run — no Wine or Proton yet, a folder that cannot be written,
/// components going into the prefix — is no failure of the game's setup:
/// nothing ran, and the next start catches up ([`catch_up`]). Only a script
/// still running at the time limit is reported.
pub(crate) async fn after_install(
    state: &Arc<AppState>,
    game_id: &str,
    paths: &GamePaths,
) -> Result<(), String> {
    match run(state, game_id, paths, Record::OnFailure, LIMIT).await {
        Ok(()) => Ok(()),
        Err(e) if e == "err.setup_timeout" => Err(e),
        Err(e) => {
            log::warn!(
                "setup {game_id}: could not run after the install ({e}); the next start tries again"
            );
            Ok(())
        }
    }
}

/// Before a start on macOS/Linux: the setup, run now if the receipt does not
/// record it for the prefix the start (`plan`) uses. That is every game
/// installed before the launcher ran setup scripts here, an adopted ETI
/// install, one whose setup could not run at install time because Wine or
/// Proton came later, and one pinned to a version with a prefix of its own.
/// A native start needs none; switched to Wine later, the game gets its
/// setup then.
///
/// Only a setup busy in the prefix holds the start back: the game would
/// start into a prefix the script is still writing. (A repeated setup of a
/// game set up long ago already stopped the start at its claim, see
/// `PrefixUse::mark_busy`.) One that could not run — no runner, a folder
/// that cannot be written — does not: the game started before this existed
/// and still does, and the next start tries again.
pub(crate) async fn catch_up(
    state: &Arc<AppState>,
    game_id: &str,
    plan: &LaunchPlan,
) -> Result<(), String> {
    let Some(prefix) = launch::unix::prefix_id(plan) else {
        return Ok(());
    };
    let Some(paths) = state.settings.read().await.library.game_paths(game_id) else {
        return Ok(());
    };
    let receipt = {
        let file = paths.receipt.clone();
        tauri::async_runtime::spawn_blocking(move || Receipt::load(&file))
            .await
            .ok()
            .flatten()
    };
    if receipt.is_none_or(|r| r.script_setup_prefixes.contains(&prefix)) {
        return Ok(());
    }
    if !has_script(&paths).await {
        mark_done(&paths, Some(&prefix)).await;
        return Ok(());
    }
    log::info!("setup {game_id}: not run in {prefix} yet; running it before the start");
    match run(state, game_id, &paths, Record::BeforeStart, CATCH_UP_LIMIT).await {
        Ok(()) => {}
        Err(e) if is_busy(&e) => return Err(e),
        Err(e) => log::warn!("setup {game_id}: could not run ({e}); starting without it"),
    }
    Ok(())
}

/// The answers that mean "something is busy in this prefix right now".
fn is_busy(code: &str) -> bool {
    matches!(
        code,
        "err.setup_running"
            | "err.setup_timeout"
            | "err.setup_game_running"
            | "err.components_busy_game"
    )
}

async fn has_script(paths: &GamePaths) -> bool {
    let script = paths.setup_script.clone();
    tauri::async_runtime::spawn_blocking(move || script.is_file())
        .await
        .unwrap_or(false)
}

/// Record a finished setup in the game's receipt, for `prefix`.
async fn mark_done(paths: &GamePaths, prefix: Option<&str>) {
    let file = paths.receipt.clone();
    let prefix = prefix.map(str::to_string);
    let _ = tauri::async_runtime::spawn_blocking(move || {
        if let Some(mut receipt) = Receipt::load(&file) {
            receipt.setup_done = true;
            if let Some(prefix) = prefix.filter(|p| !receipt.script_setup_prefixes.contains(p)) {
                receipt.script_setup_prefixes.push(prefix);
            }
            if let Err(e) = receipt.save(&file) {
                log::warn!("cannot record the setup in {}: {e}", file.display());
            }
        }
    })
    .await;
}

/// The lines of a script that did not run as written, for the log.
fn log_skipped(game_id: &str, what: &str, skipped: &[Skipped]) {
    for s in skipped {
        let done = match (&s.reason, s.inline) {
            (setup_script::Reason::FindAndReplace, _) => "kept",
            (setup_script::Reason::Arguments, _) => "arguments added",
            (setup_script::Reason::ArgumentsUnused, _) => "not applied",
            (setup_script::Reason::ScreenQuery, _) => {
                "screen size asked in a form Wine's wmic answers"
            }
            (_, true) => "command replaced by ver>nul",
            (_, false) => "skipped",
        };
        log::info!(
            "{what} {game_id}: line {}: {done} ({}): {}",
            s.line,
            s.reason,
            s.text.trim()
        );
    }
}

/// Before a start through the game's start script
/// (`launch::unix::is_script_start`): the script's files in the game's
/// folder, and the find and replace its `fnr.exe` calls run as.
pub(crate) async fn prepare_start(
    state: &Arc<AppState>,
    game_id: &str,
    paths: &GamePaths,
    plan: &mut LaunchPlan,
    extra: Vec<lanlauncher_core::manifest::ScriptArgs>,
) -> Result<(), String> {
    let lang = state.settings.read().await.game_language.clone();
    let prepared = {
        let (paths, game_id) = (paths.clone(), game_id.to_string());
        tauri::async_runtime::spawn_blocking(move || {
            setup_script::prepare(&paths, Script::Start, &game_id, &lang, &[], &extra)
                .map_err(|e| format!("err.start_write|{}: {e}", paths.share_dir.display()))
        })
        .await
        .map_err(|e| format!("err.start_write|{e}"))??
    };
    let skipped = prepared.ok_or("err.start_script_missing")?;
    log_skipped(game_id, "start", &skipped);
    with_fnr(state, plan).await;
    Ok(())
}

/// `plan` with [`launch::fnr::VAR`] naming the launcher's find and replace,
/// which the scripts' `fnr.exe` calls became. Without it (the folder cannot
/// be written) those calls fail as they would have before, and the script
/// goes on.
async fn with_fnr(state: &AppState, plan: &mut LaunchPlan) {
    // Asked once, the way the script starts it: without the libraries an
    // AppImage or Proton put in front.
    static PERL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let dir = state.dirs.data.join("tools").join("fnr");
    let installed = tauri::async_runtime::spawn_blocking(move || {
        let script = launch::fnr::install(&dir)?;
        let perl = *PERL.get_or_init(|| {
            let found = std::process::Command::new("env")
                .args(["-u", "LD_LIBRARY_PATH", "-u", "LD_PRELOAD", "perl", "-e", "1"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if !found {
                log::warn!(
                    "no perl on this machine: the scripts' fnr.exe calls (player name, language in config files) do nothing"
                );
            }
            found
        });
        Ok::<_, std::io::Error>((script, perl))
    })
    .await
    .map_err(std::io::Error::other)
    .and_then(|r| r);
    match installed {
        Ok((script, _perl)) => {
            plan.env.insert(
                launch::fnr::VAR.to_string(),
                setup_script::wine_path(&script),
            );
        }
        Err(e) => log::warn!("cannot write the find and replace for fnr.exe: {e}"),
    }
}

/// Before a start of an executable: what the start script does before it
/// starts the game itself — the player's name into a config file, the
/// language into the registry — run in the prefix and waited for, the game
/// held like during a setup. What cannot run is no reason not to start (the
/// game started without it before); a preparation still running at its
/// limit is: the game would start beside a script still writing its
/// configuration. Its claim then stays until it ends.
pub(crate) async fn run_preparation(
    state: &Arc<AppState>,
    game_id: &str,
    paths: &GamePaths,
    (mut plan, exe): (LaunchPlan, Option<String>),
) -> Result<(), String> {
    // The start holds the game already; this takes the prefix's script slot.
    let claim = SetupClaim::claim(state, game_id, true)?;
    let lang = state.settings.read().await.game_language.clone();
    let prepared = {
        let (paths, game_id) = (paths.clone(), game_id.to_string());
        tauri::async_runtime::spawn_blocking(move || {
            let script = std::fs::read_to_string(&paths.start_script).unwrap_or_default();
            let names = setup_script::game_exes(&script, exe.as_deref());
            setup_script::prepare(&paths, Script::Preparation, &game_id, &lang, &names, &[])
        })
        .await
        .map_err(std::io::Error::other)
        .and_then(|r| r)
    };
    let skipped = match prepared {
        Ok(Some(skipped)) => skipped,
        Ok(None) => {
            log::info!("prepare {game_id}: nothing in the start script to run before the game");
            return Ok(());
        }
        Err(e) => {
            log::warn!("prepare {game_id}: cannot write the files ({e}); starting without it");
            return Ok(());
        }
    };
    log_skipped(game_id, "prepare", &skipped);
    // A second start beside a running game: its configuration is in use.
    if let Ok(target) = launch::winetricks::target(&plan) {
        let in_use = tauri::async_runtime::spawn_blocking(move || {
            launch::winetricks::prefix_in_use(&target)
        })
        .await
        .unwrap_or(false);
        if in_use {
            log::info!("prepare {game_id}: the game runs already; starting without it");
            return Ok(());
        }
    }
    with_fnr(state, &mut plan).await;
    let log = state.launch_log(game_id, "prepare");
    // The last run's transcript must not pass for this one's.
    {
        let log = log.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || std::fs::remove_file(log)).await;
    }
    log::info!(
        "prepare {game_id}: running what game_start.cmd does before the game via {}",
        plan.runner
    );
    let mut watch = match launch::spawn(&plan, Some(&log)).await {
        Ok((_, watch)) => watch,
        Err(e) => {
            log::warn!("prepare {game_id}: cannot start ({e}); starting without it");
            return Ok(());
        }
    };
    match tokio::time::timeout(PREPARATION_LIMIT, &mut watch).await {
        Ok(code) => {
            log::info!(
                "prepare {game_id}: ended (exit code {:?})",
                code.ok().flatten()
            );
            Ok(())
        }
        Err(_) => {
            log::warn!(
                "prepare {game_id}: still running after {} minutes; the game does not start beside it (transcript: {})",
                PREPARATION_LIMIT.as_secs() / 60,
                log.display()
            );
            tauri::async_runtime::spawn(async move {
                let _ = watch.await;
                drop(claim);
            });
            Err("err.prepare_timeout".into())
        }
    }
}
