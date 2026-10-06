//! The launcher's icon in the notification area (Windows, Linux desktops) or
//! menu bar (macOS).
//!
//! Closing the window only hides it: the sync engine keeps seeding and
//! downloading, the chat stays online, and the icon brings the window back.
//! Quitting for real goes through the icon's menu, which ends the event loop
//! and with it the engine (`RunEvent::Exit` in `lib.rs`).
//!
//! Linux is the odd one out. Game Mode on the Steam Deck has no tray, and
//! plain GNOME accepts an icon it never shows; hiding the window there would
//! make the launcher disappear. So the icon is only built where the
//! appindicator library is present (the crate behind it panics otherwise)
//! and outside gamescope, and the window only hides while a tray host is
//! actually listening.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

const OPEN: &str = "tray-open";
const QUIT: &str = "tray-quit";

/// Set once quitting has begun: a second start must then wait for this
/// launcher to finish instead of being told its window was shown.
pub static QUITTING: AtomicBool = AtomicBool::new(false);

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
    #[cfg(target_os = "linux")]
    {
        if lanlauncher_core::instance::in_gamescope(|name| std::env::var(name).ok()) {
            log::info!("tray: none in gamescope (Game Mode); closing the window quits");
            return Ok(());
        }
        if !appindicator_present() {
            log::info!("tray: no appindicator library; closing the window quits");
            return Ok(());
        }
    }
    let (open_text, quit_text) = labels(language);
    let open = MenuItem::with_id(app, OPEN, open_text, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT, quit_text, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &separator, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("NextGen LAN Launcher")
        .menu(&menu)
        // Left click opens the window, right click the menu — the way
        // Windows users expect a tray icon to behave. (Linux trays always
        // open the menu; "Open launcher" is in it.)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            OPEN => {
                show_window(app);
            }
            QUIT => {
                log::info!("quit chosen from the tray menu");
                QUITTING.store(true, Ordering::SeqCst);
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
    #[cfg(target_os = "linux")]
    log::info!(
        "tray: icon built; a tray host is {}",
        if tray_host_listening() {
            "listening"
        } else {
            "not listening yet (closing the window quits until one is)"
        }
    );
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
/// False when there is no window to bring, as during shutdown.
pub fn show_window(app: &AppHandle) -> bool {
    if QUITTING.load(Ordering::SeqCst) {
        return false;
    }
    let Some(window) = app.get_webview_window("main") else {
        return false;
    };
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
    true
}

/// Closing the window hides it instead; the launcher keeps running.
pub fn hide_on_close(window: &tauri::Window, event: &tauri::WindowEvent) {
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        if window.label() != "main" || window.app_handle().tray_by_id("main").is_none() {
            return;
        }
        // Asked now rather than at start-up: a panel that came up after
        // the launcher (autostart) still counts, one that crashed does not.
        #[cfg(target_os = "linux")]
        if !tray_host_listening() {
            log::info!("window closed with no tray host listening; quitting");
            return;
        }
        api.prevent_close();
        let _ = window.hide();
        log::info!("window closed; the launcher keeps running in the tray");
    }
}

/// The names `libappindicator-sys` tries, in its order; it panics when none
/// of them loads.
#[cfg(target_os = "linux")]
fn appindicator_present() -> bool {
    [
        c"libayatana-appindicator3.so.1",
        c"libappindicator3.so.1",
        c"libayatana-appindicator3.so",
        c"libappindicator3.so",
    ]
    .iter()
    .any(|name| {
        // SAFETY: a valid C string; the handle is kept, the library is
        // about to be used anyway.
        !unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_LAZY) }.is_null()
    })
}

/// Whether something shows tray icons: KDE, XFCE, Cinnamon, Budgie and
/// GNOME with the AppIndicator extension (Ubuntu) all register
/// `org.kde.StatusNotifierWatcher` on the session bus; plain GNOME and
/// gamescope do not.
#[cfg(target_os = "linux")]
fn tray_host_listening() -> bool {
    use gio::prelude::*;

    let Ok(bus) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else {
        return false;
    };
    bus.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        Some(&("org.kde.StatusNotifierWatcher",).to_variant()),
        Some(gio::glib::VariantTy::new("(b)").expect("a valid type string")),
        gio::DBusCallFlags::NONE,
        1000,
        gio::Cancellable::NONE,
    )
    .ok()
    .and_then(|reply| reply.get::<(bool,)>())
    .is_some_and(|(owned,)| owned)
}
