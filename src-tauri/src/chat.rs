//! The LAN chat of the shell: started and stopped with the setting, its
//! changes passed on to the interface (`lanlauncher_core::chat` does the work).

use crate::state::AppState;
use lanlauncher_core::chat::model::{ItemView, PollChoice, PollKind, ERR_DISABLED};
use lanlauncher_core::chat::{Chat, ChatConfig, ChatSnapshot};
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
            let config = ChatConfig::new(state.dirs.data.join("chat"), &nick);
            match Chat::start(config).await {
                Ok(chat) => {
                    forward_updates(app.clone(), &chat);
                    *state.chat.write().await = Some(chat);
                    *state.chat_error.write().await = None;
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
pub async fn chat_send(
    state: State<'_, Arc<AppState>>,
    to: Option<String>,
    text: String,
    reply_to: Option<String>,
) -> Cmd<ItemView> {
    running(&state)
        .await?
        .send_text(to, &text, reply_to)
        .map_err(|e| e.to_string())
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
    to: Option<String>,
    question: String,
    options: Vec<PollChoice>,
    kind: PollKind,
    open: bool,
) -> Cmd<ItemView> {
    running(&state)
        .await?
        .create_poll(to, &question, options, kind, open)
        .map_err(|e| e.to_string())
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
    running(&state)
        .await?
        .add_poll_option(&poll, PollChoice { text, game })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn chat_close_poll(state: State<'_, Arc<AppState>>, poll: String) -> Cmd<()> {
    running(&state)
        .await?
        .close_poll(&poll)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn chat_delete(state: State<'_, Arc<AppState>>, target: String) -> Cmd<()> {
    running(&state)
        .await?
        .delete(&target)
        .map_err(|e| e.to_string())
}
