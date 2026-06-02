use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

use crate::app_state::AppState;
use crate::runtime;

/// Build the system-tray icon and its menu. This is the app's primary control
/// surface; there is no main window.
pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let start = MenuItem::with_id(app, "start", "Start", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause", "Pause", true, None::<&str>)?;
    let break_now = MenuItem::with_id(app, "break_now", "Break now", true, None::<&str>)?;
    let skip = MenuItem::with_id(app, "skip", "Skip break", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit restee", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[&start, &pause, &break_now, &skip, &sep1, &settings, &sep2, &quit],
    )?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("default window icon".into()))?;

    TrayIconBuilder::with_id("restee-tray")
        .icon(icon)
        .tooltip("restee — break reminder")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            let state = app.state::<AppState>();
            match event.id().as_ref() {
                "start" => runtime::action_start(app, state.inner()),
                "pause" => runtime::action_pause(app, state.inner()),
                "break_now" => runtime::action_break_now(app, state.inner()),
                "skip" => runtime::action_skip(app, state.inner()),
                "settings" => crate::settings_window::open(app),
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .build(app)?;

    Ok(())
}
