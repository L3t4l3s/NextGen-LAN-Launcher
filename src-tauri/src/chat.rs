//! The LAN chat of the shell: started and stopped with the setting, its
//! changes passed on to the interface (`lanlauncher_core::chat` does the work).

use crate::state::AppState;
use lanlauncher_core::chat::model::{
    ChatError, ItemView, PollChoice, PollKind, ERR_DISABLED, ERR_TOO_FAST,
};
use lanlauncher_core::chat::{Chat, ChatConfig, ChatSnapshot, PeerInfo};
use std::sync::Arc;
use tauri::{Emitter, State};
use tokio::sync::broadcast::error::RecvError;

/// New or changed messages, or the list of people (`ChatUpdate`).
pub const CHAT_EVENT: &str = "chat-update";
/// The interface should fetch the whole snapshot again: the chat started,
/// stopped, or updates were lost on the way.
pub const CHAT_RESET_EVENT: &str = "chat-reset";

type Cmd<T> = Result<T, String>;

/// Bring the running chat in line with the settings: start, stop, or pass a
/// new nickname on. Called at start and after every settings save.
pub(crate) async fn apply_settings(app: &tauri::AppHandle, state: &Arc<AppState>) {
    let _lifecycle = state.chat_lifecycle.lock().await;
    let (enabled, nick) = {
        let s = state.settings.read().await;
        (s.chat_enabled, s.player_name.clone())
    };
    let current = state.chat.read().await.clone();
    match (enabled, current) {
        (true, Some(chat)) => chat.set_nick(&nick).await,
        // Started or retried: the next settings save tries again.
        (true, None) => {
            let mut config = ChatConfig::new(state.dirs.data.join("chat"), &nick);
            // What the stats beacon tells the LANPage, for the others' list
            // of people. Read before the start, so the first `Hello` has it.
            let report = tauri::async_runtime::spawn_blocking(|| {
                lanlauncher_core::lanpage::StatsReport::collect("", None)
            })
            .await
            .unwrap_or_default();
            config.info = Some(PeerInfo {
                host: report.hostname,
                system: report.windows_edition,
                cpu: report.cpu,
                version: crate::app_version(),
            });
            match Chat::start(config).await {
                Ok(chat) => {
                    forward_updates(app.clone(), &chat);
                    *state.chat.write().await = Some(chat);
                    *state.chat_error.write().await = None;
                    trust_relays(state).await;
                    publish_playing(state).await;
                    let _ = app.emit(CHAT_RESET_EVENT, ());
                }
                Err(e) => {
                    log::warn!("chat not started: {e}");
                    *state.chat_error.write().await = Some(format!("err.chat_start|{e}"));
                    let _ = app.emit(CHAT_RESET_EVENT, ());
                }
            }
        }
        (false, Some(chat)) => {
            chat.stop().await;
            *state.chat.write().await = None;
            log::info!("chat stopped");
            *state.chat_error.write().await = None;
            let _ = app.emit(CHAT_RESET_EVENT, ());
        }
        (false, None) => *state.chat_error.write().await = None,
    }
}

/// Tell the others in the chat which game runs here: the same one the stats
/// beacon reports to the LANPage (the newest in `running`), by its title.
pub(crate) async fn publish_playing(state: &AppState) {
    let Some(chat) = state.chat.read().await.clone() else {
        return;
    };
    let current = state.running.read().await.first().map(|(g, _)| g.clone());
    let title = match current {
        Some(id) => Some(
            state
                .catalog()
                .await
                .game(&id)
                .map(|g| g.title.clone())
                .unwrap_or(id),
        ),
        None => None,
    };
    chat.set_playing(title.as_deref()).await;
}

/// Forget games of `running` that have ended: nothing works in the game's
/// folder any more (`launch::runs_from`; many games start through a script
/// that ends at once, so the started pid says little). Without this the
/// LANPage and the chat would name the last game until the launcher quits.
pub(crate) async fn prune_running(state: &AppState) {
    let running = state.running.read().await.clone();
    if running.is_empty() {
        return;
    }
    let dirs: Vec<(String, u32, Option<std::path::PathBuf>)> = {
        let settings = state.settings.read().await;
        running
            .into_iter()
            .map(|(g, pid)| {
                let dir = settings.library.game_paths(&g).map(|p| p.share_dir);
                (g, pid, dir)
            })
            .collect()
    };
    let ended: Vec<(String, u32)> = tauri::async_runtime::spawn_blocking(move || {
        dirs.into_iter()
            .filter(|(_, _, dir)| match dir {
                Some(dir) => !lanlauncher_core::launch::runs_from(dir),
                // No folder to look at: the game was removed meanwhile.
                None => true,
            })
            .map(|(g, pid, _)| (g, pid))
            .collect()
    })
    .await
    .unwrap_or_default();
    if ended.is_empty() {
        return;
    }
    let names: Vec<&str> = ended.iter().map(|(g, _)| g.as_str()).collect();
    log::info!("game(s) ended: {}", names.join(", "));
    // By game and pid: one started again during the scan stays.
    state
        .running
        .write()
        .await
        .retain(|entry| !ended.contains(entry));
    publish_playing(state).await;
}

/// Pass the relays the LANPage names (`chat_relay` in `launcher.ini`) on to
/// the chat: only those get private messages. Called after every LANPage
/// fetch and when the chat starts.
pub(crate) async fn trust_relays(state: &AppState) {
    let Some(chat) = state.chat.read().await.clone() else {
        return;
    };
    let relays = state
        .event
        .read()
        .await
        .config
        .as_ref()
        .map(|c| lanlauncher_core::chat::relays_from_launcher_ini(&c.extra))
        .unwrap_or_default();
    chat.set_trusted_relays(relays);
}

fn forward_updates(app: tauri::AppHandle, chat: &Chat) {
    let mut updates = chat.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match updates.recv().await {
                Ok(update) => {
                    let _ = app.emit(CHAT_EVENT, &update);
                }
                Err(RecvError::Lagged(_)) => {
                    let _ = app.emit(CHAT_RESET_EVENT, ());
                }
                // The chat was stopped and dropped.
                Err(RecvError::Closed) => break,
            }
        }
    });
}

/// The error code for the interface; a flood pause says how many seconds
/// are left (`err.chat_too_fast|<s>`).
fn code(chat: &Chat, e: ChatError) -> String {
    match chat.paused_for() {
        Some(left) if e == ERR_TOO_FAST => format!("{e}|{}", left.as_secs() + 1),
        _ => e.to_string(),
    }
}

async fn running(state: &AppState) -> Cmd<Chat> {
    state
        .chat
        .read()
        .await
        .clone()
        .ok_or_else(|| ERR_DISABLED.to_string())
}

/// The bell in the chat header: only this one setting, so it neither waits
/// for the sync engine's lifecycle nor reloads the library like Save does.
#[tauri::command]
pub async fn set_chat_sound(state: State<'_, Arc<AppState>>, on: bool) -> Cmd<()> {
    let mut settings = state.settings.write().await;
    if settings.chat_sound != on {
        settings.chat_sound = on;
        settings
            .save(&state.settings_path())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Everything the chat knows; `None` while it is switched off, an error when
/// it is on but could not start.
#[tauri::command]
pub async fn chat_snapshot(state: State<'_, Arc<AppState>>) -> Cmd<Option<ChatSnapshot>> {
    if let Some(e) = state.chat_error.read().await.clone() {
        return Err(e);
    }
    Ok(state.chat.read().await.as_ref().map(Chat::snapshot))
}

#[tauri::command]
/// `conversation`: `None` the public room, `#<id>` a topic, otherwise a peer id.
pub async fn chat_send(
    state: State<'_, Arc<AppState>>,
    conversation: Option<String>,
    text: String,
    reply_to: Option<String>,
) -> Cmd<ItemView> {
    let chat = running(&state).await?;
    chat.send_text(conversation, &text, reply_to)
        .map_err(|e| code(&chat, e))
}

/// Link a game of the catalog; its title comes from this launcher's
/// catalog, not from the interface.
#[tauri::command]
pub async fn chat_share_game(
    state: State<'_, Arc<AppState>>,
    conversation: Option<String>,
    game: String,
) -> Cmd<ItemView> {
    let title = state
        .catalog()
        .await
        .game(&game)
        .map(|g| g.title.clone())
        .ok_or_else(|| "err.unknown_game".to_string())?;
    let chat = running(&state).await?;
    chat.share_game(conversation, &game, &title)
        .map_err(|e| code(&chat, e))
}

#[tauri::command]
pub async fn chat_react(state: State<'_, Arc<AppState>>, target: String, emoji: String) -> Cmd<()> {
    running(&state)
        .await?
        .react(&target, &emoji)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn chat_create_poll(
    state: State<'_, Arc<AppState>>,
    conversation: Option<String>,
    question: String,
    options: Vec<PollChoice>,
    kind: PollKind,
    open: bool,
) -> Cmd<ItemView> {
    let chat = running(&state).await?;
    chat.create_poll(conversation, &question, options, kind, open)
        .map_err(|e| code(&chat, e))
}

#[tauri::command]
pub async fn chat_vote(
    state: State<'_, Arc<AppState>>,
    poll: String,
    choices: Vec<String>,
) -> Cmd<()> {
    running(&state)
        .await?
        .vote(&poll, choices)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn chat_add_poll_option(
    state: State<'_, Arc<AppState>>,
    poll: String,
    text: String,
    game: Option<String>,
) -> Cmd<()> {
    let chat = running(&state).await?;
    chat.add_poll_option(&poll, PollChoice { text, game })
        .map_err(|e| code(&chat, e))
}

#[tauri::command]
pub async fn chat_close_poll(state: State<'_, Arc<AppState>>, poll: String) -> Cmd<()> {
    running(&state)
        .await?
        .close_poll(&poll)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn chat_edit(state: State<'_, Arc<AppState>>, target: String, text: String) -> Cmd<()> {
    let chat = running(&state).await?;
    chat.edit(&target, &text).map_err(|e| code(&chat, e))
}

#[tauri::command]
pub async fn chat_create_topic(state: State<'_, Arc<AppState>>, name: String) -> Cmd<ItemView> {
    let chat = running(&state).await?;
    chat.create_topic(&name).map_err(|e| code(&chat, e))
}

#[tauri::command]
pub async fn chat_delete(state: State<'_, Arc<AppState>>, target: String) -> Cmd<()> {
    running(&state)
        .await?
        .delete(&target)
        .map_err(|e| e.to_string())
}
