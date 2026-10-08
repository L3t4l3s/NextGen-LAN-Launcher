//! The player's name and language in a game's own settings, before every
//! start, on every platform (`lanlauncher_core::player_settings`).

use crate::state::AppState;
use crate::wine_setup::SetupClaim;
use lanlauncher_core::launch::{self, LaunchPlan};
use lanlauncher_core::manifest::Manifest;
use lanlauncher_core::paths::GamePaths;
use lanlauncher_core::player_settings::{self, Applied, Player, RegistryValue};
use std::sync::Arc;
use std::time::Duration;

/// Setting a few registry values takes seconds; after this the game starts
/// all the same.
const REGISTRY_LIMIT: Duration = Duration::from_secs(60);

/// Set what the profile names and what a Steam emulator in the package
/// reads, then the profile's registry values: on Windows with `reg add`,
/// elsewhere through `registry_plan` in the game's prefix. What cannot be
/// set goes to the log and the game starts either way — except a registry
/// run still going at its limit: the game would start beside a Wine still
/// writing (and maybe still making) its prefix. That run keeps the game
/// claimed until it ends.
pub(crate) async fn apply(
    state: &Arc<AppState>,
    game_id: &str,
    paths: &GamePaths,
    manifest: Option<&Manifest>,
    registry_plan: Option<LaunchPlan>,
) -> Result<(), String> {
    let player = {
        let s = state.settings.read().await;
        Player {
            name: s.safe_player_name(),
            lang: s.game_language.clone(),
        }
    };
    let settings = manifest.map(|m| m.settings.clone()).unwrap_or_default();
    let (local, who) = (paths.local_dir.clone(), player.clone());
    let done = tauri::async_runtime::spawn_blocking(move || {
        let (mut done, registry) = player_settings::apply(&local, &settings, &who);
        let (goldberg, smart_steam_emu) = player_settings::emulators(&local, &who);
        done.extend(goldberg);
        done.extend(smart_steam_emu);
        (done, registry)
    })
    .await;
    let (done, registry) = match done {
        Ok(done) => done,
        Err(e) => {
            log::warn!("player settings {game_id}: {e}");
            return Ok(());
        }
    };
    for applied in done {
        match applied {
            Applied::Written(path) => {
                log::info!("player settings {game_id}: set in {}", path.display())
            }
            Applied::Unchanged(_) => {}
            Applied::Left(why) => log::info!("player settings {game_id}: left alone: {why}"),
        }
    }
    if registry.is_empty() {
        return Ok(());
    }
    if cfg!(target_os = "windows") {
        set_with_reg(game_id, registry).await;
        Ok(())
    } else if let Some(plan) = registry_plan {
        set_in_prefix(state, game_id, paths, &player, plan, &registry).await
    } else {
        Ok(())
    }
}

/// Windows: `reg add` for each value. HKLM wants administrator rights the
/// launcher does not have; that one fails into the log, the start goes on.
async fn set_with_reg(game_id: &str, registry: Vec<RegistryValue>) {
    let game_id = game_id.to_string();
    let _ = tauri::async_runtime::spawn_blocking(move || {
        for v in registry {
            let mut reg = std::process::Command::new("reg");
            launch::elevate::hide_window_std(&mut reg);
            let status = reg
                .args([
                    "add",
                    &v.key,
                    "/v",
                    &v.name,
                    "/t",
                    v.reg_type.reg_name(),
                    "/d",
                    &v.value,
                    "/f",
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            match status {
                Ok(s) if s.success() => {
                    log::info!("player settings {game_id}: set {}\\{}", v.key, v.name)
                }
                other => log::warn!(
                    "player settings {game_id}: cannot set {}\\{} ({other:?})",
                    v.key,
                    v.name
                ),
            }
        }
    })
    .await;
}

/// macOS/Linux: the values through a batch in the prefix, waited for, the
/// game held like during a setup.
async fn set_in_prefix(
    state: &Arc<AppState>,
    game_id: &str,
    paths: &GamePaths,
    player: &Player,
    mut plan: LaunchPlan,
    registry: &[RegistryValue],
) -> Result<(), String> {
    let claim = SetupClaim::claim(state, game_id, true)?;
    // Not beside a running copy of the game: its registry is in use.
    if let Ok(target) = launch::winetricks::target(&plan) {
        let in_use = tauri::async_runtime::spawn_blocking(move || {
            launch::winetricks::prefix_in_use(&target)
        })
        .await
        .unwrap_or(false);
        if in_use {
            log::info!("player settings {game_id}: the game runs already; registry left as it is");
            return Ok(());
        }
    }
    let written = {
        let (share, id, lang, values) = (
            paths.share_dir.clone(),
            game_id.to_string(),
            player.lang.clone(),
            registry.to_vec(),
        );
        tauri::async_runtime::spawn_blocking(move || {
            player_settings::write_registry_script(&share, &id, &lang, &values)
        })
        .await
        .map_err(std::io::Error::other)
        .and_then(|r| r)
    };
    let env = match written {
        Ok(env) => env,
        Err(e) => {
            log::warn!("player settings {game_id}: cannot write the registry script ({e})");
            return Ok(());
        }
    };
    plan.env.extend(env);
    let log = state.launch_log(game_id, "settings");
    let mut watch = match launch::spawn(&plan, Some(&log)).await {
        Ok((_, watch)) => watch,
        Err(e) => {
            log::warn!("player settings {game_id}: cannot set the registry values ({e})");
            return Ok(());
        }
    };
    match tokio::time::timeout(REGISTRY_LIMIT, &mut watch).await {
        Ok(Ok(Some(0))) => {
            log::info!(
                "player settings {game_id}: set {} registry value(s) in the prefix",
                registry.len()
            );
            Ok(())
        }
        Ok(code) => {
            log::warn!(
                "player settings {game_id}: setting the registry values ended with {:?} (transcript: {})",
                code.ok().flatten(),
                log.display()
            );
            Ok(())
        }
        Err(_) => {
            log::warn!(
                "player settings {game_id}: the registry values are still being set after a minute; the game does not start beside it"
            );
            tauri::async_runtime::spawn(async move {
                let _ = watch.await;
                drop(claim);
            });
            Err("err.prepare_timeout".into())
        }
    }
}
