//! Executes [`FixAction`]s. Windows-only actions return a clear error on
//! other platforms instead of failing silently.

use crate::state::AppState;
use lanlauncher_core::diagnostics;
use lanlauncher_core::problem::FixAction;
use std::path::Path;
use std::sync::Arc;

/// Run batch lines that need administrator rights and wait for them:
/// directly through cmd.exe when the launcher is already elevated, else
/// through PowerShell `Start-Process -Verb RunAs` (UAC prompt). Errors are
/// `err.<code>` strings: `elevation_disabled` (setting), `elevation_denied`
/// (prompt declined), `elevation_failed|<exit code>` (plus the message of the
/// command that failed, for lines whose output is captured).
pub(crate) async fn run_admin_lines(
    state: &AppState,
    stem: &str,
    lines: Vec<lanlauncher_core::launch::elevate::BatchLine>,
    cwd: &Path,
) -> Result<(), String> {
    run_admin_lines_at(state, stem, lines, cwd)
        .await
        .map(|_| ())
}

/// As [`run_admin_lines`], returning the transcript file the batch wrote so a
/// caller can show it. The file exists even when the run succeeded.
pub(crate) async fn run_admin_lines_at(
    state: &AppState,
    stem: &str,
    lines: Vec<lanlauncher_core::launch::elevate::BatchLine>,
    cwd: &Path,
) -> Result<std::path::PathBuf, String> {
    use lanlauncher_core::launch::{apply_args, elevate, LaunchPlan};
    let elevated = elevate::running_elevated() != Some(false);
    if !elevated && !state.settings.read().await.allow_elevation {
        return Err("err.elevation_disabled".into());
    }
    let (batch, log) = elevate::write_batch_logged(&state.run_dir(), stem, &lines)
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
    let out = lanlauncher_core::launch::elevate::hide_window(&mut cmd)
        .current_dir(&plan.cwd)
        .output()
        .await
        .map_err(|e| format!("err.elevation_failed|{e}"))?;
    if out.status.success() {
        return Ok(log);
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
    // The elevated process writes to its own console, so what actually went
    // wrong is only in the log the batch appends to. Windows console tools
    // still write OEM-encoded text; the lossy conversion keeps the message
    // readable even when the umlauts do not survive it.
    let transcript = std::fs::read(&log).map(|b| tail(&b)).unwrap_or_default();
    log::warn!(
        "{stem} exited with {}\n--- transcript ---\n{}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        out.status,
        transcript,
        tail(&out.stdout),
        tail(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr).to_ascii_lowercase();
    if !elevated
        && (stderr.contains("cancel") || stderr.contains("abgebrochen") || stderr.contains("1223"))
    {
        return Err("err.elevation_denied".into());
    }
    let code = out.status.code().unwrap_or(-1);
    // The batch returns the exit code of the *first* command that failed and
    // marks that command's section in the transcript, so this is the output
    // that explains the code — a later command that still succeeded would
    // name the wrong culprit.
    let reason = lanlauncher_core::launch::elevate::last_section(&transcript);
    if reason.is_empty() {
        Err(format!("err.elevation_failed|{code}"))
    } else {
        // Reads as "(Code 1 - Der Parameter ist ungueltig)" in the UI.
        Err(format!("err.elevation_failed|{code} \u{2013} {reason}"))
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
        rules.into_iter().map(Into::into).collect(),
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
///
/// `netsh` answers in milliseconds and says it in its exit code — 0 when a
/// rule matched, 1 when none did — so nothing depends on its localised text.
/// `Get-NetFirewallRule` would be tidier to read but loads a PowerShell
/// module first, which is most of the several seconds the diagnostics page
/// used to take. `None` off Windows or when netsh cannot be run at all.
pub async fn firewall_rule_present() -> Option<bool> {
    if !cfg!(target_os = "windows") {
        return None;
    }
    let mut cmd = tokio::process::Command::new("netsh");
    let out = lanlauncher_core::launch::elevate::hide_window(&mut cmd)
        .args([
            "advfirewall",
            "firewall",
            "show",
            "rule",
            &format!("name={}", diagnostics::FIREWALL_RULE_IN),
        ])
        .output()
        .await
        .ok()?;
    // 0: a rule was printed. 1: none matched. Anything else — a stopped
    // firewall service, an unreachable policy store — is not an answer, and
    // claiming the rule is missing would offer a repair that fails the same
    // way.
    match out.status.code() {
        Some(0) => Some(true),
        Some(1) => Some(false),
        _ => None,
    }
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

#[cfg(target_os = "windows")]
async fn powershell(script: &str) -> Result<String, String> {
    let mut cmd = tokio::process::Command::new("powershell");
    let out = lanlauncher_core::launch::elevate::hide_window(&mut cmd)
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
    let out = lanlauncher_core::launch::elevate::hide_window(&mut cmd)
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
                    vec![powershell_line(&script).into()],
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
            // The stale rules are cleared first; `netsh` treats "no rule
            // matched" as a failure, so only the allow rules are checked.
            let stale = diagnostics::firewall_stale_rules(&binary);
            let rules = diagnostics::firewall_rules(&binary, port);
            if needs_runas() {
                use lanlauncher_core::launch::elevate::BatchLine;
                // The deletes are expected to fail when no rule matched; only
                // the allow rules decide whether the repair worked.
                let lines = stale
                    .iter()
                    .map(|r| BatchLine::optional(netsh_line(r)))
                    .chain(rules.iter().map(|r| BatchLine::from(netsh_line(r))))
                    .collect();
                run_admin_lines(state, "firewall-rules", lines, &state.dirs.data).await?;
            } else {
                for rule in &stale {
                    if let Err(e) = netsh(rule).await {
                        log::debug!("netsh delete rule: {e}");
                    }
                }
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
            // "Erledigt" is what an empty message reads as, and that is a
            // strange thing to say about opening a folder.
            Ok("msg.folder_opened".into())
        }
        FixAction::OpenUrl { url } => {
            app.opener()
                .open_url(url, None::<&str>)
                .map_err(|e| e.to_string())?;
            Ok("msg.link_opened".into())
        }
        FixAction::AddDefenderExclusion { path } => {
            let escaped = path.replace('\'', "''");
            let script = format!("Add-MpPreference -ExclusionPath '{escaped}'");
            if needs_runas() {
                run_admin_lines(
                    state,
                    "defender-exclusion",
                    vec![powershell_line(&script).into()],
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
