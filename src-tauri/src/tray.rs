//! The launcher's icon in the notification area (Windows) or menu bar (macOS).
//!
//! Closing the window only hides it: the sync engine keeps seeding and
//! downloading, the chat stays online, and the icon brings the window back.
//! Quitting for real goes through the icon's menu, which ends the event loop
//! and with it the engine (`RunEvent::Exit` in `lib.rs`).

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

const OPEN: &str = "tray-open";
const QUIT: &str = "tray-quit";

/// The menu entries, kept so a language change can relabel them.
struct TrayMenu {
    open: MenuItem<Wry>,
    quit: MenuItem<Wry>,
}

/// The menu runs outside the webview, so its two labels cannot go through
/// `src/lib/i18n`.
fn labels(language: &str) -> (&'static str, &'static str) {
    match language {
        "en" => ("Open launcher", "Quit"),
        _ => ("Launcher öffnen", "Beenden"),
    }
}

pub fn install(app: &AppHandle, language: &str) -> tauri::Result<()> {
    let (open_text, quit_text) = labels(language);
    let open = MenuItem::with_id(app, OPEN, open_text, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT, quit_text, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &separator, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("NextGen LAN Launcher")
        .menu(&menu)
        // Left click opens the window, right click the menu — the way
        // Windows users expect a tray icon to behave.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            OPEN => show_window(app),
            QUIT => {
                log::info!("quit chosen from the tray menu");
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    app.manage(TrayMenu { open, quit });
    Ok(())
}

/// Follow the UI language after the settings were saved.
pub fn relabel(app: &AppHandle, language: &str) {
    let Some(menu) = app.try_state::<TrayMenu>() else {
        return;
    };
    let (open_text, quit_text) = labels(language);
    let _ = menu.open.set_text(open_text);
    let _ = menu.quit.set_text(quit_text);
}

/// Bring the main window back from the tray, the taskbar or behind others.
pub fn show_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

/// Closing the window hides it instead; the launcher keeps running.
pub fn hide_on_close(window: &tauri::Window, event: &tauri::WindowEvent) {
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        if window.label() == "main" && window.app_handle().tray_by_id("main").is_some() {
            api.prevent_close();
            let _ = window.hide();
            log::info!("window closed; the launcher keeps running in the tray");
        }
    }
}
