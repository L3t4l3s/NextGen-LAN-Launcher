//! Starting games.
//!
//! * Windows: run the ETI batch scripts exactly like the original launcher
//!   (`game_setup.cmd "<path>" <id>` once, then
//!   `game_start.cmd "<path>" <id> <lang> "<player>"`). This keeps all ~190
//!   packages working unchanged.
//! * macOS/Linux: use the manifest (curated, organiser overlay or derived from
//!   the script) and a runner (CrossOver, Wine, Proton or native).

pub mod elevate;
pub mod fnr;
pub mod proton;
pub mod setup_script;
pub mod unix;
pub mod windows;
pub mod winetricks;

use crate::error::{Error, Result};
use crate::install::Receipt;
use crate::manifest::{Manifest, Runner};
use crate::paths::GamePaths;
use crate::settings::Settings;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A fully resolved command line, ready to spawn. Serialisable so the UI can
/// show "what will run" before the user confirms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchPlan {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    /// Human-readable description of the runner (`Wine 9.0`, `game_start.cmd`).
    pub runner: String,
    /// Windows scripts need elevation for `netsh`/`reg`.
    pub needs_elevation: bool,
    /// Windows only: the complete command line after `program`, passed
    /// verbatim (`raw_arg`) because `cmd.exe /C` has its own quoting rules
    /// that std's argument escaping would break. When set, `args` is only
    /// informational.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_command_line: Option<String>,
    /// Programs the start runs through, in front of `program`: `gamemoderun`,
    /// `mangohud`, `gamescope -w 1280 -h 800 --`. Kept apart rather than
    /// folded into `program`, because what the plan *runs* — the Proton or
    /// Wine that the prefix record and `cxbottle` look at — stays `program`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wrapper: Vec<String>,
}

impl LaunchPlan {
    /// What is started, for logs and the diagnostics page: the wrapper, if
    /// any, in front of the program.
    pub fn command_display(&self) -> String {
        if self.wrapper.is_empty() {
            self.program.display().to_string()
        } else {
            format!("{} {}", self.wrapper.join(" "), self.program.display())
        }
    }
}

/// Inputs shared by the platform launchers.
pub struct LaunchContext<'a> {
    pub paths: &'a GamePaths,
    pub game_id: &'a str,
    pub settings: &'a Settings,
    pub manifest: Option<&'a Manifest>,
    pub receipt: Option<&'a Receipt>,
    /// Index into `manifest.launch.alternatives` (None = primary).
    pub alternative: Option<usize>,
}

/// Optional per-game tools ETI packages may ship next to the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Extra {
    /// `keygen.exe` in the game folder.
    Keygen,
    /// `server_start.cmd`, a dedicated server.
    Server,
}

/// Plan for a per-game extra. Both are Windows programs; on other platforms
/// the UI does not offer them and this returns an error code.
pub fn extra_plan(extra: Extra, ctx: &LaunchContext<'_>) -> Result<LaunchPlan> {
    if !cfg!(target_os = "windows") {
        return Err(Error::Code("err.windows_only".into()));
    }
    match extra {
        Extra::Keygen => {
            let exe = ctx
                .paths
                .keygen()
                .ok_or_else(|| Error::Code("err.extra_missing".into()))?;
            let cwd = exe
                .parent()
                .map(PathBuf::from)
                .unwrap_or_else(|| ctx.paths.share_dir.clone());
            Ok(LaunchPlan {
                program: exe,
                args: Vec::new(),
                cwd,
                env: BTreeMap::new(),
                runner: "keygen.exe".into(),
                needs_elevation: false,
                raw_command_line: None,
                wrapper: Vec::new(),
            })
        }
        Extra::Server => windows::server_plan(
            ctx.paths,
            ctx.game_id,
            &ctx.settings.game_language,
            &ctx.settings.safe_player_name(),
        )
        .ok_or_else(|| Error::Code("err.extra_missing".into())),
    }
}

/// ETI's runtime package installer under a library root
/// (`eti_launcher/bin/preqsetup.exe`), only when it is complete: Resilio
/// downloads into `<name>.!sync` and renames at the end, a plain copy (folder
/// mode) has no such marker, so the file must also have been untouched for a
/// minute. `None` off Windows.
pub fn prereq_installer(library_root: &std::path::Path) -> Option<PathBuf> {
    if !cfg!(target_os = "windows") {
        return None;
    }
    prereq_installer_at(library_root, std::time::SystemTime::now())
}

/// Testable core of [`prereq_installer`]; `now` is the reference for the
/// one-minute settle time.
pub fn prereq_installer_at(
    library_root: &std::path::Path,
    now: std::time::SystemTime,
) -> Option<PathBuf> {
    let exe = library_root.join(crate::paths::PREREQ_INSTALLER_RELATIVE);
    let meta = std::fs::metadata(&exe).ok().filter(|m| m.is_file())?;
    let mut partial = exe.clone().into_os_string();
    partial.push(".!sync");
    if PathBuf::from(partial).exists() {
        return None;
    }
    let settled = meta
        .modified()
        .ok()
        .and_then(|m| now.duration_since(m).ok())
        .is_some_and(|age| age >= std::time::Duration::from_secs(60));
    settled.then_some(exe)
}

/// Plan for the runtime package installer: the file as is, its folder as
/// working directory; it brings its own UI.
pub fn prereq_plan(installer: &std::path::Path) -> LaunchPlan {
    LaunchPlan {
        program: installer.to_path_buf(),
        args: Vec::new(),
        cwd: installer
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".")),
        env: BTreeMap::new(),
        runner: "preqsetup.exe".into(),
        needs_elevation: true,
        raw_command_line: None,
        wrapper: Vec::new(),
    }
}

/// Build the launch plan for the current platform.
pub fn plan(ctx: &LaunchContext<'_>) -> Result<LaunchPlan> {
    if cfg!(target_os = "windows") {
        windows::plan(ctx)
    } else {
        unix::plan(ctx)
    }
}

/// Resolve the executable relative to `local/`. A user choice stored in the
/// receipt wins over the manifest (the manifest may target another package
/// revision); an explicit alternative always comes from the manifest.
pub fn resolve_exe(ctx: &LaunchContext<'_>) -> Result<(PathBuf, Vec<String>, PathBuf, Runner)> {
    let platform = Manifest::current_platform();
    let spec = ctx.manifest.map(|m| m.launch_for(platform));
    if let (None, Some(exe)) = (
        ctx.alternative,
        ctx.receipt.and_then(|r| r.exe_override.as_ref()),
    ) {
        if !crate::manifest::is_safe_relative(exe) {
            return Err(Error::Launch("stored executable path is invalid".into()));
        }
        let exe_path = ctx.paths.local_dir.join(exe.replace('\\', "/"));
        let cwd = exe_path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| ctx.paths.local_dir.clone());
        let runner = spec.as_ref().map(|s| s.runner).unwrap_or(Runner::Auto);
        return Ok((exe_path, Vec::new(), cwd, runner));
    }
    if let Some(spec) = spec {
        let (exe, args, workdir) = match ctx.alternative {
            Some(i) => {
                let alt = spec
                    .alternatives
                    .get(i)
                    .ok_or_else(|| Error::Launch(format!("alternative {i} does not exist")))?;
                (alt.exe.clone(), alt.args.clone(), alt.workdir.clone())
            }
            None => (spec.exe.clone(), spec.args.clone(), spec.workdir.clone()),
        };
        if !exe.is_empty() {
            let exe_path = ctx.paths.local_dir.join(exe.replace('\\', "/"));
            let cwd = workdir
                .map(|w| ctx.paths.local_dir.join(w.replace('\\', "/")))
                .or_else(|| exe_path.parent().map(PathBuf::from))
                .unwrap_or_else(|| ctx.paths.local_dir.clone());
            return Ok((exe_path, args, cwd, spec.runner));
        }
    }
    Err(Error::Code("err.no_executable".into()))
}

/// Substitute ETI script variables the manifests may reference.
pub fn expand_args(args: &[String], ctx: &LaunchContext<'_>) -> Vec<String> {
    args.iter()
        .map(|a| {
            a.replace("%player%", &ctx.settings.safe_player_name())
                .replace("%game_lang%", &ctx.settings.game_language)
                .replace("%game_id%", ctx.game_id)
                .replace("%game_path%", &ctx.paths.share_dir.to_string_lossy())
        })
        .collect()
}

/// Receives the exit code of a started process once it ends (`None` when the
/// system did not report one). The diagnostics page uses it to tell "the game
/// is running" apart from "the window closed straight away".
pub type ExitWatch = tokio::sync::oneshot::Receiver<Option<i32>>;

/// Detach a child: reap it in the background and report its exit code.
fn watch(child: tokio::process::Child) -> (u32, ExitWatch) {
    let pid = child.id().unwrap_or(0);
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let mut child = child;
        let code = child.wait().await.ok().and_then(|s| s.code());
        let _ = tx.send(code);
    });
    (pid, rx)
}

/// Spawn the plan as a detached child process. Returns the PID.
/// Start a plan as the user, elevating only when the plan needs it and the
/// launcher is not already elevated (Windows; `allow` is the user's setting).
/// A program whose own manifest demands administrator rights (Windows error
/// 740) is retried the same way. `run_dir` receives the batch files.
pub async fn spawn_for_user(plan: &LaunchPlan, run_dir: &Path, allow: bool) -> Result<u32> {
    spawn_for_user_watched(plan, run_dir, allow, None)
        .await
        .map(|(pid, _)| pid)
}

/// Like [`spawn_for_user`], plus a handle that reports the exit code and, with
/// `log`, the program's output written to that file.
///
/// A game script that opens an empty console window and ends leaves nothing
/// behind to look at; with the file the diagnostics page can show what it
/// printed. The window itself stays empty either way — its output goes to the
/// file instead of the screen.
pub async fn spawn_for_user_watched(
    plan: &LaunchPlan,
    run_dir: &Path,
    allow: bool,
    log: Option<&Path>,
) -> Result<(u32, ExitWatch)> {
    let not_elevated = elevate::running_elevated() == Some(false);
    if plan.needs_elevation && not_elevated {
        if !allow {
            log::warn!(
                "{} needs administrator rights but elevation is disabled in the settings; running as user",
                plan.runner
            );
        } else {
            return spawn_elevated(plan, run_dir, log).await;
        }
    }
    match spawn(plan, log).await {
        Err(Error::Launch(msg)) if not_elevated && allow && msg.contains("os error 740") => {
            log::info!(
                "{} demands administrator rights itself; retrying elevated",
                plan.runner
            );
            spawn_elevated(plan, run_dir, log).await
        }
        other => other,
    }
}

/// Run a plan through a batch file as administrator (UAC prompt). The
/// batch name is fixed per runner (overwritten each time). PowerShell waits
/// for the batch, so its pid stays alive as long as the game runs; a prompt
/// the user declines ends PowerShell within moments with a non-zero code,
/// which is reported as `err.elevation_denied`.
pub async fn spawn_elevated(
    plan: &LaunchPlan,
    run_dir: &Path,
    log: Option<&Path>,
) -> Result<(u32, ExitWatch)> {
    let stem = plan.runner.trim_end_matches(".cmd").replace(' ', "-");
    // Without a log the script keeps its own console: the ETI scripts that
    // need administrator rights are the ones that print instructions and wait
    // for a key, and a redirect would leave the user in front of a blank
    // window. With one, the caller asked to read what it prints instead.
    let line = elevate::batch_line(plan);
    let batch = match log {
        None => elevate::write_batch(run_dir, &stem, &[elevate::BatchLine::console(line)])?,
        Some(path) => elevate::write_batch_logged_to(run_dir, &stem, &[line.into()], Some(path))?.0,
    };
    log::info!("starting {} elevated via {}", plan.runner, batch.display());
    let runas = elevate::runas_plan(&batch, &plan.cwd);
    let mut cmd = tokio::process::Command::new(&runas.program);
    elevate::hide_window(&mut cmd)
        .current_dir(&runas.cwd)
        .envs(&runas.env);
    apply_args(&mut cmd, &runas);
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut child = cmd
        .spawn()
        .map_err(|e| Error::Launch(format!("cannot start PowerShell for elevation: {e}")))?;
    let pid = child.id().unwrap_or(0);
    match tokio::time::timeout(std::time::Duration::from_secs(3), child.wait()).await {
        Ok(Ok(status)) if !status.success() => {
            log::warn!(
                "elevated start of {} ended with {status} right away (prompt declined?)",
                plan.runner
            );
            Err(Error::Code("err.elevation_denied".into()))
        }
        // Ended within the three seconds and succeeded: the code is already
        // known, so the watch is handed a closed channel's counterpart.
        Ok(Ok(status)) => {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let _ = tx.send(status.code());
            Ok((pid, rx))
        }
        Ok(Err(_)) => {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let _ = tx.send(None);
            Ok((pid, rx))
        }
        Err(_) => Ok(watch(child)),
    }
}

pub async fn spawn(plan: &LaunchPlan, log: Option<&Path>) -> Result<(u32, ExitWatch)> {
    ensure_proton_prefix(plan)?;
    let bottle_note = ensure_crossover_bottle(plan).await;
    let mut cmd = match plan.wrapper.split_first() {
        Some((first, rest)) => {
            let mut cmd = tokio::process::Command::new(first);
            cmd.args(rest).arg(&plan.program);
            cmd
        }
        None => tokio::process::Command::new(&plan.program),
    };
    cmd.current_dir(&plan.cwd);
    // Before the plan's own environment: a manifest may set one of these
    // deliberately for a game, and that value is the one that counts.
    give_the_host_environment_back(&mut cmd);
    cmd.envs(&plan.env);
    apply_args(&mut cmd, plan);
    // Without a log the child keeps the handles it would have had: a console
    // program started from a windowed one gets its own console, and that is
    // where an ETI script prints its menu and reads the answer. Redirecting
    // to null is what turned Doom's start into an empty window that waits.
    // Nothing to set without a log: inherited handles are that console.
    if let Some((out, err)) = log.and_then(|path| {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut file = std::fs::File::create(path)
            .inspect_err(|e| log::warn!("cannot write {}: {e}", path.display()))
            .ok()?;
        // First in the transcript, above whatever `wine --bottle` answers:
        // when the start fails because the bottle is missing after all, the
        // reason is right there, on the page that shows this file.
        if let Some(note) = &bottle_note {
            use std::io::Write;
            let _ = writeln!(file, "{note}");
        }
        let err = file
            .try_clone()
            .inspect_err(|e| log::warn!("cannot write {}: {e}", path.display()))
            .ok()?;
        Some((file, err))
    }) {
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::from(out))
            .stderr(std::process::Stdio::from(err));
    }
    let child = cmd.spawn().map_err(|e| match plan.wrapper.first() {
        // The wrapper is what failed to start, not the Proton behind it.
        Some(wrapper) => Error::Launch(format!(
            "cannot start {wrapper} (in front of {}): {e}",
            plan.program.display()
        )),
        None => Error::Launch(format!("cannot start {}: {e}", plan.program.display())),
    })?;
    Ok(watch(child))
}

/// Proton locks the compatibility-data directory before it creates `pfx/`.
/// Steam normally creates that outer directory for it; a standalone launcher
/// has to do the same or Proton exits immediately while opening `pfx.lock`.
fn ensure_proton_prefix(plan: &LaunchPlan) -> Result<()> {
    let Some(value) = plan.env.get("STEAM_COMPAT_DATA_PATH") else {
        return Ok(());
    };
    if value.is_empty() {
        return Err(Error::Launch(
            "STEAM_COMPAT_DATA_PATH must not be empty".into(),
        ));
    }
    let path = PathBuf::from(value);
    let path = if path.is_absolute() {
        path
    } else {
        plan.cwd.join(path)
    };
    std::fs::create_dir_all(&path).map_err(|e| Error::io(path, e))
}

/// CrossOver's `wine --bottle` only opens an existing bottle. The GUI creates
/// one before its Run Command action; a standalone launcher must do that
/// preparation itself when a per-runner bottle is used for the first time.
/// Returns what went wrong, if creating it failed, for the start's transcript.
async fn ensure_crossover_bottle(plan: &LaunchPlan) -> Option<String> {
    let manager = plan.program.parent().map(|dir| dir.join("cxbottle"))?;
    if !manager.is_file() {
        return None;
    }
    let bottle = plan
        .args
        .windows(2)
        .find(|args| args[0] == "--bottle")
        .map(|args| args[1].as_str())?;
    if crossover_bottle_exists(bottle, &plan.env) {
        return None;
    }

    let mut command = tokio::process::Command::new(&manager);
    // The environment the game gets, not the launcher's own: `cxbottle` is a
    // CrossOver program too, and the AppImage's libraries or a renderer
    // setting forced for the launcher's window are no more its business.
    give_the_host_environment_back(&mut command);
    command.envs(&plan.env).args([
        "--create",
        "--scope",
        "private",
        "--bottle",
        bottle,
        "--install",
        "--template",
        "win10_64",
    ]);
    // Preparation, not a gate. The probe above can miss a bottle that exists
    // — relocated, in a scope other than `private`, `HOME` unset — and then
    // `--create` fails because it is already there. Treating that as fatal
    // made such a game unstartable on every later attempt, although the
    // bottle it needed was right where CrossOver looks. So the launch goes
    // ahead either way: if the bottle really is missing, `wine --bottle` says
    // so itself, and what `cxbottle` answered stands above it in the start's
    // transcript and in the log.
    let note = match command.output().await {
        Ok(output) if output.status.success() => return None,
        Ok(output) => {
            let said = [&output.stderr[..], &output.stdout[..]]
                .iter()
                .map(|bytes| String::from_utf8_lossy(bytes).trim().to_string())
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join(" | ");
            format!(
                "cxbottle --create {bottle} exited with {}{}",
                output.status,
                if said.is_empty() {
                    String::new()
                } else {
                    format!(": {said}")
                }
            )
        }
        Err(e) => format!(
            "cannot run {} to create bottle {bottle}: {e}",
            manager.display()
        ),
    };
    log::warn!("{note}; starting anyway");
    Some(note)
}

/// Whether CrossOver has a bottle `name`, looked for where a child with
/// environment `env` would find it.
pub(crate) fn crossover_bottle_exists(name: &str, env: &BTreeMap<String, String>) -> bool {
    crossover_bottle_dirs(env)
        .iter()
        .any(|root| root.join(name).join("cxbottle.conf").is_file())
}

fn crossover_bottle_dirs(env: &BTreeMap<String, String>) -> Vec<PathBuf> {
    if let Some(paths) = env
        .get("CX_BOTTLE_PATH")
        .map(std::ffi::OsString::from)
        .or_else(|| std::env::var_os("CX_BOTTLE_PATH"))
    {
        return std::env::split_paths(&paths).collect();
    }
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    vec![
        home.join("Library/Application Support/CrossOver/Bottles"),
        home.join(".cxoffice"),
    ]
}

/// The environment variable in which the launcher records what it forced on
/// itself, as JSON: every name it changed with the value that was there
/// before (`null` where there was none).
pub const FORCED_ENV: &str = "NLL_FORCED_ENV";

/// A command for one of the machine's own programs (`firewall-cmd`, …),
/// started with the environment it would have had without the launcher:
/// what the launcher forced on itself restored, the AppImage's paths gone.
/// A host tool written in Python would otherwise load the image's libraries.
pub fn host_command(program: impl AsRef<std::ffi::OsStr>) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(program);
    give_the_host_environment_back(&mut cmd);
    cmd
}

/// The value of `name` a child started with [`host_command`] sees: what
/// the recipe below sets or removes, else the launcher's own. Read back from
/// the recipe itself, so the two cannot drift apart; it only ever sets and
/// removes single variables, never clears the environment.
pub fn host_value(name: &str) -> Option<String> {
    let cmd = host_command("true");
    let set = cmd
        .as_std()
        .get_envs()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.map(|v| v.to_string_lossy().into_owned()));
    match set {
        Some(value) => value,
        None => std::env::var(name).ok(),
    }
}

/// The one recipe for a child of the machine rather than of the launcher,
/// shared by games, `cxbottle` and host tools.
fn give_the_host_environment_back(cmd: &mut tokio::process::Command) {
    restore_what_the_launcher_forced(cmd);
    leave_the_appimage_behind(cmd);
}

/// Apply [`without_appimage_paths`] to a child's environment.
///
/// Only variables that are text: one that is not stays as it is, because
/// nothing in an AppImage's paths needs to be compared against bytes that are
/// not a path.
fn leave_the_appimage_behind(cmd: &mut tokio::process::Command) {
    let Some(appdir) = std::env::var_os("APPDIR") else {
        return;
    };
    // A name the launcher forced was already answered for by the restore,
    // with the value that was there before it: that decision stands.
    let forced: Vec<String> = forced_env().into_iter().map(|(name, _)| name).collect();
    let vars = std::env::vars_os()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)));
    for (name, value) in without_appimage_paths(vars, &appdir.to_string_lossy()) {
        if !is_appimage_marker(&name) && forced.contains(&name) {
            continue;
        }
        match value {
            Some(value) => cmd.env(&name, value),
            None => cmd.env_remove(&name),
        };
    }
}

/// Give a child back the environment the launcher changed for itself.
///
/// To draw at all on some Linux machines the launcher forces software
/// rendering, an X11 backend and an EGL platform on itself. A game must not
/// inherit that — `LIBGL_ALWAYS_SOFTWARE=1` would run it on the CPU — and
/// must not lose a setting of the user's either, so each of those variables
/// goes back to the value it had before the launcher touched it.
fn restore_what_the_launcher_forced(cmd: &mut tokio::process::Command) {
    for (name, before) in forced_env() {
        match before {
            Some(value) => cmd.env(&name, value),
            None => cmd.env_remove(&name),
        };
    }
    cmd.env_remove(FORCED_ENV);
}

/// Take the AppImage's own environment out of a child's.
///
/// An AppImage starts the launcher with `LD_LIBRARY_PATH`, `PATH`,
/// `XDG_DATA_DIRS` and a row of GTK variables pointing into its mount. A game
/// that inherits them loads the image's libraries — Ubuntu's — instead of the
/// machine's, which on a Steam Deck is a different distribution altogether.
/// Every path entry under `appdir` is dropped, a variable that consists of
/// nothing else goes away, and the image's own markers go with them.
///
/// Returns only what changes: the new value, or `None` to remove it.
/// Names an AppImage sets for itself whatever their value is: its own
/// markers, and the two settings its GTK hook makes that are not paths
/// (`GDK_BACKEND=x11` unconditionally, `GTK_THEME` from the host). They exist
/// for the launcher's window, so they never belong to a game — not even when
/// the launcher recorded one of them as something it forced, because what it
/// found there was the image's value too.
pub fn is_appimage_marker(name: &str) -> bool {
    matches!(
        name,
        "APPDIR" | "APPIMAGE" | "ARGV0" | "OWD" | "GDK_BACKEND" | "GTK_THEME"
    )
}

pub fn without_appimage_paths(
    vars: impl Iterator<Item = (String, String)>,
    appdir: &str,
) -> Vec<(String, Option<String>)> {
    let prefix = appdir.trim_end_matches('/');
    // `/` as the mount would make every absolute path an entry to strip, and
    // the child would start with no PATH at all.
    if prefix.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (name, value) in vars {
        if is_appimage_marker(&name) {
            out.push((name, None));
            continue;
        }
        if !value.contains(prefix) {
            continue;
        }
        let kept: Vec<&str> = value
            .split(':')
            .filter(|part| {
                let part = part.trim_end_matches('/');
                // An empty entry is nothing to keep — `"$APPDIR/usr/lib:"`
                // with an unset base would leave the variable set to "".
                !part.is_empty() && part != prefix && !part.starts_with(&format!("{prefix}/"))
            })
            .collect();
        if kept.len() == value.split(':').count() {
            continue;
        }
        out.push((name, (!kept.is_empty()).then(|| kept.join(":"))));
    }
    out
}

/// The contents of [`FORCED_ENV`]: names with the value they had before.
pub fn forced_env() -> Vec<(String, Option<String>)> {
    forced_env_in(&std::env::var(FORCED_ENV).unwrap_or_default())
}

fn forced_env_in(json: &str) -> Vec<(String, Option<String>)> {
    serde_json::from_str::<BTreeMap<String, Option<String>>>(json)
        .unwrap_or_default()
        .into_iter()
        .filter(|(name, _)| !name.is_empty())
        .collect()
}

/// Apply a plan's arguments: on Windows the verbatim command line when set
/// (cmd.exe quoting rules), otherwise the argument vector.
#[cfg(windows)]
pub fn apply_args(cmd: &mut tokio::process::Command, plan: &LaunchPlan) {
    match &plan.raw_command_line {
        Some(raw) => {
            cmd.raw_arg(raw);
        }
        None => {
            cmd.args(&plan.args);
        }
    }
}

/// Apply a plan's arguments: on Windows the verbatim command line when set
/// (cmd.exe quoting rules), otherwise the argument vector.
#[cfg(not(windows))]
pub fn apply_args(cmd: &mut tokio::process::Command, plan: &LaunchPlan) {
    cmd.args(&plan.args);
}

/// Candidate executables inside `local/` for the "choose executable" dialog.
pub fn list_executables(paths: &GamePaths, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(&paths.local_dir)
        .max_depth(4)
        .into_iter()
        .flatten()
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        let is_exe = name.ends_with(".exe")
            || name.ends_with(".app")
            || name.ends_with(".sh")
            || (cfg!(unix)
                && entry
                    .metadata()
                    .map(|m| is_unix_executable(&m))
                    .unwrap_or(false));
        if is_exe
            && !name.contains("unins")
            && !name.contains("redist")
            && !name.contains("vcredist")
            && !name.contains("dxsetup")
        {
            if let Ok(rel) = entry.path().strip_prefix(&paths.local_dir) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
        if out.len() >= limit {
            break;
        }
    }
    out.sort_by_key(|p| (p.matches('/').count(), p.to_ascii_lowercase()));
    out
}

/// Whether a game still runs: any process whose program or working folder
/// lies inside `dir`. Many games start through a script or starter that ends
/// at once and leaves the game running; under Wine the program is Wine, but
/// it works in the game's folder. The pid of the start is no help: it is
/// usually that starter, and Windows hands its number out again soon after.
/// The launcher's own process never counts.
pub fn runs_from(dir: &Path) -> bool {
    let dir = crate::paths::strip_verbatim(
        std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf()),
    );
    let sys = crate::transport::resilio::scan_processes_with(
        sysinfo::ProcessRefreshKind::nothing()
            .with_exe(sysinfo::UpdateKind::Always)
            .with_cwd(sysinfo::UpdateKind::Always),
    );
    let own = std::process::id();
    let running = crate::transport::resilio::real_processes(&sys)
        .filter(|(id, p)| id.as_u32() != own && p.status() != sysinfo::ProcessStatus::Zombie)
        .any(|(_, p)| {
            p.exe().is_some_and(|e| is_inside(e, &dir))
                || p.cwd().is_some_and(|c| is_inside(c, &dir))
        });
    running
}

/// `path` lies in `dir` (or is it); without regard to case on Windows,
/// whose file names ignore it.
fn is_inside(path: &Path, dir: &Path) -> bool {
    let path = crate::paths::strip_verbatim(path.to_path_buf());
    if cfg!(windows) {
        let (p, d) = (
            path.to_string_lossy().to_lowercase(),
            dir.to_string_lossy().to_lowercase(),
        );
        Path::new(&p).starts_with(Path::new(&d))
    } else {
        path.starts_with(dir)
    }
}

#[cfg(unix)]
fn is_unix_executable(m: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    m.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_unix_executable(_m: &std::fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_game_runs_while_something_works_in_its_folder() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        assert!(!runs_from(dir.path()));
        // A game that works in its own folder, as under Wine, after its
        // starter has ended.
        let mut game = std::process::Command::new("sleep")
            .arg("30")
            .current_dir(dir.path())
            .spawn()
            .unwrap();
        assert!(runs_from(dir.path()));
        assert!(!runs_from(other.path()), "another folder");
        game.kill().unwrap();
        game.wait().unwrap();
        assert!(!runs_from(dir.path()));
    }

    #[test]
    fn creates_the_outer_proton_prefix_before_launch() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().join("game/local");
        let prefix = cwd.join("relative-prefix");
        let plan = LaunchPlan {
            program: PathBuf::from("proton"),
            args: Vec::new(),
            cwd,
            env: BTreeMap::from([("STEAM_COMPAT_DATA_PATH".into(), "relative-prefix".into())]),
            runner: "Proton".into(),
            needs_elevation: false,
            raw_command_line: None,
            wrapper: Vec::new(),
        };

        ensure_proton_prefix(&plan).unwrap();

        assert!(prefix.is_dir());
    }

    /// A `cxbottle --create` that fails — here because the probe missed a
    /// bottle that is there, the case that made a game unstartable for good —
    /// must not stop the launch. It is preparation; `wine --bottle` has the
    /// last word on whether the bottle exists, and what `cxbottle` said goes
    /// into the start's transcript.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_failed_bottle_creation_does_not_stop_the_launch() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("CrossOver/bin");
        std::fs::create_dir_all(&bin).unwrap();
        // A shell that rejects `--create` stands in for a `cxbottle` that
        // fails. A link to an existing program, not a freshly written script:
        // executing a file just written can fail with "text file busy" when a
        // parallel test forks meanwhile.
        std::os::unix::fs::symlink("/bin/sh", bin.join("cxbottle")).unwrap();
        let plan = LaunchPlan {
            program: bin.join("wine"),
            args: vec!["--bottle".into(), "nll-g".into()],
            cwd: tmp.path().to_path_buf(),
            // Points the probe at an empty folder, so it misses the bottle.
            env: BTreeMap::from([(
                "CX_BOTTLE_PATH".into(),
                tmp.path().join("nowhere").to_string_lossy().to_string(),
            )]),
            runner: "CrossOver".into(),
            needs_elevation: false,
            raw_command_line: None,
            wrapper: Vec::new(),
        };

        // The note is what the failure left for the transcript; its wording
        // shows `cxbottle` ran and failed, rather than could not be started.
        let note = ensure_crossover_bottle(&plan).await.expect("cxbottle ran");
        assert!(
            note.starts_with("cxbottle --create nll-g exited with"),
            "{note}"
        );
    }

    /// The wrapper really runs in front of the program: `env` with a
    /// variable starts `sh`, and what `sh` prints lands in the log.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_wrapper_starts_the_program_it_is_put_in_front_of() {
        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("out.log");
        let plan = LaunchPlan {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".into(), "echo started with $NLL_WRAPPED".into()],
            cwd: tmp.path().to_path_buf(),
            env: BTreeMap::new(),
            runner: "native".into(),
            needs_elevation: false,
            raw_command_line: None,
            wrapper: vec!["env".into(), "NLL_WRAPPED=wrapper".into()],
        };
        let (_, exit) = spawn(&plan, Some(&log)).await.unwrap();
        assert_eq!(exit.await.unwrap(), Some(0));
        assert_eq!(
            std::fs::read_to_string(&log).unwrap().trim(),
            "started with wrapper"
        );
    }

    #[test]
    fn a_game_does_not_inherit_the_appimage() {
        // The image's own paths in front of the machine's: a game that keeps
        // them loads Ubuntu's libraries on an Arch system. What the user put
        // in those variables stays.
        let vars = [
            ("LD_LIBRARY_PATH", "/tmp/.mount_x/usr/lib:/home/deck/lib"),
            ("XDG_DATA_DIRS", "/tmp/.mount_x/usr/share"),
            ("GTK_PATH", "/tmp/.mount_x/usr/lib/gtk-3.0"),
            ("APPDIR", "/tmp/.mount_x"),
            ("PATH", "/usr/bin:/bin"),
            ("STEAM_COMPAT_DATA_PATH", "/home/deck/compat"),
        ];
        let changes = without_appimage_paths(
            vars.iter().map(|(k, v)| (k.to_string(), v.to_string())),
            "/tmp/.mount_x",
        );
        assert_eq!(
            changes,
            vec![
                (
                    "LD_LIBRARY_PATH".to_string(),
                    Some("/home/deck/lib".to_string())
                ),
                ("XDG_DATA_DIRS".to_string(), None),
                ("GTK_PATH".to_string(), None),
                ("APPDIR".to_string(), None),
            ],
            "the image's entries go, the user's stay, and untouched variables \
             are not reported at all"
        );
        // A path that only looks like the mount is not inside it.
        assert!(without_appimage_paths(
            std::iter::once((
                "LD_LIBRARY_PATH".to_string(),
                "/tmp/.mount_xyz/lib".to_string()
            )),
            "/tmp/.mount_x",
        )
        .is_empty());
        // The GTK hook's own choices are the launcher's, not the game's.
        assert_eq!(
            without_appimage_paths(
                std::iter::once(("GDK_BACKEND".to_string(), "x11".to_string())),
                "/tmp/.mount_x",
            ),
            vec![("GDK_BACKEND".to_string(), None)]
        );
        // A mount of "/" would take every absolute path with it, so nothing
        // is touched at all.
        assert!(without_appimage_paths(
            vars.iter().map(|(k, v)| (k.to_string(), v.to_string())),
            "/",
        )
        .is_empty());
    }

    #[test]
    fn a_game_gets_back_what_the_launcher_changed_for_itself() {
        // What the launcher set on itself to get its own window drawn: a game
        // inheriting `LIBGL_ALWAYS_SOFTWARE=1` would run on the CPU. Where
        // the user had a value of their own, that one goes back — removing it
        // would take their setting away with ours.
        let list = forced_env_in(
            r#"{"LIBGL_ALWAYS_SOFTWARE":null,"GDK_BACKEND":"wayland","EGL_PLATFORM":null}"#,
        );
        assert_eq!(
            list,
            vec![
                ("EGL_PLATFORM".to_string(), None),
                ("GDK_BACKEND".to_string(), Some("wayland".to_string())),
                ("LIBGL_ALWAYS_SOFTWARE".to_string(), None),
            ]
        );
        // Nothing forced, and nothing to go wrong over: no list, an empty
        // one, or something that is not JSON at all.
        assert!(forced_env_in("").is_empty());
        assert!(forced_env_in("{}").is_empty());
        assert!(forced_env_in("LIBGL_ALWAYS_SOFTWARE").is_empty());
    }

    #[test]
    fn prereq_installer_requires_a_complete_settled_file() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("eti_launcher").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join("preqsetup.exe");
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(120);
        assert!(prereq_installer_at(dir.path(), later).is_none(), "absent");
        std::fs::write(&exe, "MZ").unwrap();
        assert!(
            prereq_installer_at(dir.path(), std::time::SystemTime::now()).is_none(),
            "just written: may still be copied"
        );
        assert_eq!(prereq_installer_at(dir.path(), later), Some(exe.clone()));
        std::fs::write(bin.join("preqsetup.exe.!sync"), "").unwrap();
        assert!(
            prereq_installer_at(dir.path(), later).is_none(),
            "Resilio still writing"
        );
        let plan = prereq_plan(&exe);
        assert_eq!(plan.cwd, bin);
        assert!(plan.args.is_empty());
    }

    #[test]
    fn lists_executables_sorted_by_depth() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = GamePaths::new(tmp.path(), "g");
        std::fs::create_dir_all(paths.local_dir.join("bin/x64")).unwrap();
        std::fs::write(paths.local_dir.join("bin/x64/Game.exe"), "").unwrap();
        std::fs::write(paths.local_dir.join("Launcher.exe"), "").unwrap();
        std::fs::write(paths.local_dir.join("unins000.exe"), "").unwrap();
        std::fs::write(paths.local_dir.join("readme.txt"), "").unwrap();
        let list = list_executables(&paths, 50);
        assert_eq!(list, vec!["Launcher.exe", "bin/x64/Game.exe"]);
    }
}
