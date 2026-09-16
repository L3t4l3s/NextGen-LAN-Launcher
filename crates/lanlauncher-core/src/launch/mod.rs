//! Starting games.
//!
//! * Windows: run the ETI batch scripts exactly like the original launcher
//!   (`game_setup.cmd "<path>" <id>` once, then
//!   `game_start.cmd "<path>" <id> <lang> "<player>"`). This keeps all ~190
//!   packages working unchanged.
//! * macOS/Linux: use the manifest (curated, organiser overlay or derived from
//!   the script) and a runner (CrossOver, Wine, Proton or native).

pub mod elevate;
pub mod unix;
pub mod windows;

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
    spawn_for_user_watched(plan, run_dir, allow)
        .await
        .map(|(pid, _)| pid)
}

/// Like [`spawn_for_user`], plus a handle that reports the exit code.
pub async fn spawn_for_user_watched(
    plan: &LaunchPlan,
    run_dir: &Path,
    allow: bool,
) -> Result<(u32, ExitWatch)> {
    let not_elevated = elevate::running_elevated() == Some(false);
    if plan.needs_elevation && not_elevated {
        if !allow {
            log::warn!(
                "{} needs administrator rights but elevation is disabled in the settings; running as user",
                plan.runner
            );
        } else {
            return spawn_elevated(plan, run_dir).await;
        }
    }
    match spawn(plan).await {
        Err(Error::Launch(msg)) if not_elevated && allow && msg.contains("os error 740") => {
            log::info!(
                "{} demands administrator rights itself; retrying elevated",
                plan.runner
            );
            spawn_elevated(plan, run_dir).await
        }
        other => other,
    }
}

/// Run a plan through a batch file as administrator (UAC prompt). The
/// batch name is fixed per runner (overwritten each time). PowerShell waits
/// for the batch, so its pid stays alive as long as the game runs; a prompt
/// the user declines ends PowerShell within moments with a non-zero code,
/// which is reported as `err.elevation_denied`.
pub async fn spawn_elevated(plan: &LaunchPlan, run_dir: &Path) -> Result<(u32, ExitWatch)> {
    let stem = plan.runner.trim_end_matches(".cmd").replace(' ', "-");
    let batch = elevate::write_batch(run_dir, &stem, &[elevate::batch_line(plan).into()])?;
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

pub async fn spawn(plan: &LaunchPlan) -> Result<(u32, ExitWatch)> {
    let mut cmd = tokio::process::Command::new(&plan.program);
    cmd.current_dir(&plan.cwd).envs(&plan.env);
    apply_args(&mut cmd, plan);
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let child = cmd
        .spawn()
        .map_err(|e| Error::Launch(format!("cannot start {}: {e}", plan.program.display())))?;
    Ok(watch(child))
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
