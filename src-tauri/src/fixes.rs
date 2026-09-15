//! Executes [`FixAction`]s. Windows-only actions return a clear error on
//! other platforms instead of failing silently.

use crate::state::AppState;
use lanlauncher_core::diagnostics;
use lanlauncher_core::problem::FixAction;
use std::path::Path;
use std::sync::Arc;

#[cfg(target_os = "windows")]
async fn powershell(script: &str) -> Result<String, String> {
    let out = tokio::process::Command::new("powershell")
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
    let out = tokio::process::Command::new("netsh")
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
            powershell(&diagnostics::ps_set_private(interface_index)).await?;
            Ok("msg.profile_private".into())
        }
        FixAction::AddFirewallRules => {
            let (override_path, port) = {
                let s = state.settings.read().await;
                (s.resilio_binary.clone(), s.sync_port)
            };
            let located = lanlauncher_core::transport::resilio::locate_binary_detailed(
                override_path.as_deref(),
                state.resource_dir.as_deref(),
                &state.dirs.data,
            );
            let binary = located
                .found
                .ok_or_else(|| format!("err.resilio_not_found|{}", located.probed.len()))?;
            for rule in diagnostics::firewall_rules(&binary, port) {
                netsh(&rule).await?;
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
            powershell(&format!("Add-MpPreference -ExclusionPath '{escaped}'")).await?;
            Ok("msg.defender_exclusion_added".into())
        }
    }
}
