//! Starting games.
//!
//! * Windows: run the ETI batch scripts exactly like the original launcher
//!   (`game_setup.cmd "<path>" <id>` once, then
//!   `game_start.cmd "<path>" <id> <lang> "<player>"`). This keeps all ~190
//!   packages working unchanged.
//! * macOS/Linux: use the manifest (curated, organiser overlay or derived from
//!   the script) and a runner (CrossOver, Wine, Proton or native).

pub mod unix;
pub mod windows;

use crate::error::{Error, Result};
use crate::install::Receipt;
use crate::manifest::{Manifest, Runner};
use crate::paths::GamePaths;
use crate::settings::Settings;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

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

/// Spawn the plan as a detached child process. Returns the PID.
pub async fn spawn(plan: &LaunchPlan) -> Result<u32> {
    let mut cmd = tokio::process::Command::new(&plan.program);
    cmd.current_dir(&plan.cwd).envs(&plan.env);
    apply_args(&mut cmd, plan);
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let child = cmd
        .spawn()
        .map_err(|e| Error::Launch(format!("cannot start {}: {e}", plan.program.display())))?;
    let pid = child.id().unwrap_or(0);
    tokio::spawn(async move {
        let mut child = child;
        let _ = child.wait().await;
    });
    Ok(pid)
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
