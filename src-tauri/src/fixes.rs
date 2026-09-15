//! Executes [`FixAction`]s. Windows-only actions return a clear error on
//! other platforms instead of failing silently.

use crate::state::AppState;
use lanlauncher_core::diagnostics;
use lanlauncher_core::problem::FixAction;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::sync::Arc;

/// Run batch lines that need administrator rights and wait for them:
/// directly through cmd.exe when the launcher is already elevated, else
/// through PowerShell `Start-Process -Verb RunAs` (UAC prompt). Errors are
/// `err.<code>` strings: `elevation_disabled` (setting), `elevation_denied`
/// (prompt declined), `elevation_failed|<exit code>`.
pub(crate) async fn run_admin_lines(
    state: &AppState,
    stem: &str,
    lines: Vec<String>,
    cwd: &Path,
) -> Result<(), String> {
    use lanlauncher_core::launch::{apply_args, elevate, LaunchPlan};
    let elevated = elevate::running_elevated() != Some(false);
    if !elevated && !state.settings.read().await.allow_elevation {
        return Err("err.elevation_disabled".into());
    }
    let batch = elevate::write_batch(&state.run_dir(), stem, &lines)
        .map_err(|e| format!("err.elevation_failed|{e}"))?;
    let plan = if elevated {
        LaunchPlan {
            program: std::path::PathBuf::from(
                std::env::var("ComSpec").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".into()),
            ),
            args: vec![],
            cwd: cwd.to_path_buf(),
            env: Default::default(),
            runner: stem.into(),
            needs_elevation: false,
            raw_command_line: Some(format!("/S /C \"\"{}\"\"", batch.display())),
        }
    } else {
        log::info!("{stem}: asking for administrator rights (UAC)");
        elevate::runas_plan(&batch, cwd)
    };
    let mut cmd = tokio::process::Command::new(&plan.program);
    apply_args(&mut cmd, &plan);
    let out = hidden(&mut cmd)
        .current_dir(&plan.cwd)
        .output()
        .await
        .map_err(|e| format!("err.elevation_failed|{e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let tail = |b: &[u8]| {
        String::from_utf8_lossy(b)
            .chars()
            .rev()
            .take(2000)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>()
    };
    log::warn!(
        "{stem} exited with {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        out.status,
        tail(&out.stdout),
        tail(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr).to_ascii_lowercase();
    if !elevated
        && (stderr.contains("cancel") || stderr.contains("abgebrochen") || stderr.contains("1223"))
    {
        Err("err.elevation_denied".into())
    } else {
        Err(format!(
            "err.elevation_failed|{}",
            out.status.code().unwrap_or(-1)
        ))
    }
}

/// Firewall rules of a game's start script, registered once per game
/// (marker in the data dir) so the script can run as a normal user. Errors
/// are logged only: a missing rule must not stop the game from starting.
pub(crate) async fn ensure_firewall_rules(
    state: &AppState,
    paths: &lanlauncher_core::paths::GamePaths,
    game_id: &str,
    lang: &str,
    player: &str,
) {
    use lanlauncher_core::launch::elevate;
    if !cfg!(target_os = "windows") {
        return;
    }
    let marker = elevate::firewall_marker(&state.dirs.data, game_id);
    if marker.exists() {
        return;
    }
    let rules = std::fs::read_to_string(&paths.start_script)
        .map(|t| elevate::firewall_add_rules(&t, &paths.share_dir, game_id, lang, player))
        .unwrap_or_default();
    if rules.is_empty() {
        return;
    }
    match run_admin_lines(
        state,
        &format!("{game_id}-firewall"),
        rules,
        &paths.share_dir,
    )
    .await
    {
        Ok(()) => {
            if let Some(dir) = marker.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&marker, "");
            log::info!("firewall rules for {game_id} registered");
        }
        Err(e) => log::warn!("firewall rules for {game_id} not registered: {e}"),
    }
}

/// Windows: does the inbound firewall rule for the sync engine exist?
/// `Get-NetFirewallRule` needs no elevation and its output is a plain count,
/// unlike netsh whose messages are localised. `None` off Windows, when
/// PowerShell fails or the firewall service is not running.
pub async fn firewall_rule_present() -> Option<bool> {
    if !cfg!(target_os = "windows") {
        return None;
    }
    let name = diagnostics::FIREWALL_RULE_IN.replace('\'', "''");
    let script = format!(
        "$ErrorActionPreference = 'Stop'; @(Get-NetFirewallRule -DisplayName '{name}' -ErrorAction SilentlyContinue).Count"
    );
    let out = powershell(&script).await.ok()?;
    out.trim().parse::<u32>().ok().map(|n| n > 0)
}

/// Whether administrative commands must go through `run_admin_lines`.
fn needs_runas() -> bool {
    lanlauncher_core::launch::elevate::running_elevated() == Some(false)
}

/// Batch line running a PowerShell snippet (encoded, so no quoting issues).
fn powershell_line(script: &str) -> String {
    format!(
        "powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand {}",
        lanlauncher_core::launch::elevate::powershell_encoded(script)
    )
}

/// Batch line for a netsh call; values with spaces are quoted the way
/// cmd.exe expects (`name="NextGen LAN Launcher Sync (in)"`).
fn netsh_line(args: &[String]) -> String {
    let mut line = String::from("netsh");
    for a in args {
        line.push(' ');
        match a.split_once('=') {
            Some((k, v)) if v.contains(' ') => line.push_str(&format!("{k}=\"{v}\"")),
            _ if a.contains(' ') => line.push_str(&format!("\"{a}\"")),
            _ => line.push_str(a),
        }
    }
    line
}

/// CREATE_NO_WINDOW: helper processes must not flash a console window from
/// this GUI process.
#[cfg(target_os = "windows")]
fn hidden(cmd: &mut tokio::process::Command) -> &mut tokio::process::Command {
    cmd.creation_flags(0x0800_0000)
}

#[cfg(not(target_os = "windows"))]
fn hidden(cmd: &mut tokio::process::Command) -> &mut tokio::process::Command {
    cmd
}

#[cfg(target_os = "windows")]
async fn powershell(script: &str) -> Result<String, String> {
    let mut cmd = tokio::process::Command::new("powershell");
    let out = hidden(&mut cmd)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output()
        .await
        .map_err(|e| format!("err.powershell|{e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).to_string())
    }
}

#[cfg(not(target_os = "windows"))]
async fn powershell(_script: &str) -> Result<String, String> {
    Err("err.windows_only".into())
}

/// Current Windows network profiles (empty on other platforms).
pub async fn network_profiles() -> Vec<diagnostics::NetworkProfile> {
    match powershell(diagnostics::PS_GET_PROFILES).await {
        Ok(json) => diagnostics::parse_net_profiles(&json),
        Err(_) => Vec::new(),
    }
}

#[cfg(target_os = "windows")]
async fn netsh(args: &[String]) -> Result<(), String> {
    let mut cmd = tokio::process::Command::new("netsh");
    let out = hidden(&mut cmd)
        .args(args)
        .output()
        .await
        .map_err(|e| format!("netsh: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stdout).to_string())
    }
}

#[cfg(not(target_os = "windows"))]
async fn netsh(_args: &[String]) -> Result<(), String> {
    Err("err.windows_only".into())
}

pub async fn apply(
    app: &tauri::AppHandle,
    state: &Arc<AppState>,
    fix: FixAction,
) -> Result<String, String> {
    use tauri_plugin_opener::OpenerExt;
    match fix {
        FixAction::SetNetworkProfilePrivate { interface_index } => {
            let script = diagnostics::ps_set_private(interface_index);
            if needs_runas() {
                run_admin_lines(
                    state,
                    "network-profile",
                    vec![powershell_line(&script)],
                    &state.dirs.data,
                )
                .await?;
            } else {
                powershell(&script).await?;
            }
            Ok("msg.profile_private".into())
        }
        FixAction::AddFirewallRules => {
            let (override_path, port) = {
                let s = state.settings.read().await;
                (s.resilio_binary.clone(), s.sync_port)
            };
            let (resource_dir, data_dir) = (state.resource_dir.clone(), state.dirs.data.clone());
            let located = tauri::async_runtime::spawn_blocking(move || {
                lanlauncher_core::transport::resilio::locate_binary_detailed(
                    override_path.as_deref(),
                    resource_dir.as_deref(),
                    &data_dir,
                )
            })
            .await
            .map_err(|e| format!("err.resilio_not_found|{e}"))?;
            let binary = located
                .found
                .ok_or_else(|| format!("err.resilio_not_found|{}", located.probed.len()))?;
            let rules = diagnostics::firewall_rules(&binary, port);
            if needs_runas() {
                let lines = rules.iter().map(|r| netsh_line(r)).collect();
                run_admin_lines(state, "firewall-rules", lines, &state.dirs.data).await?;
            } else {
                for rule in &rules {
                    netsh(rule).await?;
                }
            }
            Ok("msg.firewall_added".into())
        }
        FixAction::RestartTransport => {
            crate::commands::restart_transport_inner(state).await?;
            Ok("msg.transport_restarted".into())
        }
        FixAction::RepairGame { game_id } => {
            let m = state.manager.read().await.clone().ok_or("err.not_ready")?;
            m.repair(&game_id).await.map_err(|e| e.to_string())?;
            Ok("msg.repair_started".into())
        }
        FixAction::OpenFolder { path } => {
            let p = Path::new(&path);
            if !p.exists() {
                let _ = std::fs::create_dir_all(p);
            }
            app.opener()
                .open_path(path, None::<&str>)
                .map_err(|e| e.to_string())?;
            Ok(String::new())
        }
        FixAction::OpenUrl { url } => {
            app.opener()
                .open_url(url, None::<&str>)
                .map_err(|e| e.to_string())?;
            Ok(String::new())
        }
        FixAction::AddDefenderExclusion { path } => {
            let escaped = path.replace('\'', "''");
            let script = format!("Add-MpPreference -ExclusionPath '{escaped}'");
            if needs_runas() {
                run_admin_lines(
                    state,
                    "defender-exclusion",
                    vec![powershell_line(&script)],
                    &state.dirs.data,
                )
                .await?;
            } else {
                powershell(&script).await?;
            }
            Ok("msg.defender_exclusion_added".into())
        }
    }
}
