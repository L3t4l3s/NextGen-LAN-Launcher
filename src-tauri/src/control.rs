//! The command line's requests (`lanlauncher_core::control`: `--install <id>`
//! and the like), carried out in the running launcher through the very
//! commands its buttons call.

use crate::commands;
use crate::state::AppState;
use lanlauncher_core::control::Command;
use std::sync::Arc;
use tauri::{Emitter, Manager};

/// Carry out `command` and say what came of it: the start's process id for
/// `play`, the report for `components`, one JSON line per game for `status`,
/// nothing for `install`/`uninstall` (their progress shows in `status`).
/// Errors are the commands' codes (`err.components_needed|dotnet48`).
pub fn run(app: &tauri::AppHandle, command: Command) -> Result<String, String> {
    let Some(state) = app.try_state::<Arc<AppState>>() else {
        return Err("err.not_ready".into());
    };
    let line = command.to_line();
    log::info!("command line: {line}");
    let answer = tauri::async_runtime::block_on(async move {
        match command {
            Command::Install(id) => commands::install_game(state, id)
                .await
                .map(|()| String::new()),
            Command::Uninstall(id) => commands::uninstall_game(state, id)
                .await
                .map(|()| String::new()),
            Command::Play {
                game_id,
                skip_components,
            } => commands::play_game(state, game_id, None, None, Some(skip_components))
                .await
                .map(|pid| format!("{pid}\n")),
            Command::Components(id) => commands::install_components(state, id, false, None)
                .await
                .and_then(|report| json_line(&report)),
            Command::Status(id) => commands::get_statuses(state).await.map(|all| {
                all.iter()
                    .filter(|s| id.as_ref().is_none_or(|id| &s.game_id == id))
                    .filter_map(|s| json_line(s).ok())
                    .collect()
            }),
        }
    });
    // The window reloads its library after its own install and uninstall
    // buttons; for these it has to be told.
    if answer.is_ok() && matches!(line.split(' ').next(), Some("install" | "uninstall")) {
        let _ = app.emit(crate::CATALOG_EVENT, 0usize);
    }
    match &answer {
        Ok(out) if !out.is_empty() => log::info!("command line: {line}: {}", out.trim_end()),
        Ok(_) => log::info!("command line: {line}: done"),
        Err(e) => log::warn!("command line: {line}: {e}"),
    }
    answer
}

fn json_line(value: &impl serde::Serialize) -> Result<String, String> {
    serde_json::to_string(value)
        .map(|json| json + "\n")
        .map_err(|e| e.to_string())
}
