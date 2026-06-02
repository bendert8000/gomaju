use std::sync::Mutex;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

use restee_core::RunState;

use crate::app_state::AppState;
use crate::runtime;

/// Handles to the live Start/Pause check items, kept so we can reflect run state in
/// the tray. `cache` holds the last rendered key to skip redundant OS updates.
pub struct TrayMenu {
    start: CheckMenuItem<tauri::Wry>,
    pause: CheckMenuItem<tauri::Wry>,
    cache: Mutex<String>,
}

/// Build the system-tray icon and its menu. This is the app's primary control
/// surface; there is no main window.
pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    // CheckMenuItems render a native check before the text to mark the active state.
    let start = CheckMenuItem::with_id(app, "start", "Start", true, false, None::<&str>)?;
    let pause = CheckMenuItem::with_id(app, "pause", "Pause", true, false, None::<&str>)?;
    let reset = MenuItem::with_id(app, "reset", "Reset timer", true, None::<&str>)?;
    let break_now = MenuItem::with_id(app, "break_now", "Break now", true, None::<&str>)?;
    let skip = MenuItem::with_id(app, "skip", "Skip break", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit restee", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &start, &pause, &reset, &break_now, &skip, &sep1, &settings, &sep2, &quit,
        ],
    )?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("default window icon".into()))?;

    TrayIconBuilder::with_id("restee-tray")
        .icon(icon)
        .tooltip("restee — break reminder")
        .show_menu_on_left_click(true)
        .menu(&menu)
        .on_menu_event(|app, event| {
            let state = app.state::<AppState>();
            match event.id().as_ref() {
                "start" => runtime::action_start(app, state.inner()),
                "pause" => runtime::action_pause(app, state.inner()),
                "reset" => runtime::action_reset(app, state.inner()),
                "break_now" => runtime::action_break_now(app, state.inner()),
                "skip" => runtime::action_skip(app, state.inner()),
                "settings" => crate::settings_window::open(app),
                "quit" => {
                    eprintln!("restee: quit requested");
                    app.exit(0);
                }
                _ => {}
            }
        })
        .build(app)?;

    app.manage(TrayMenu {
        start,
        pause,
        cache: Mutex::new(String::new()),
    });
    Ok(())
}

/// Reflect the current run state in the tray: check Start while running (with the
/// elapsed time) or Pause while paused. Cheap to call every tick — it only touches
/// the OS menu when the rendered text/check actually changes.
pub fn refresh(app: &AppHandle, state: RunState, running_secs: u64) {
    let Some(menu) = app.try_state::<TrayMenu>() else {
        return;
    };

    let started = matches!(state, RunState::Running | RunState::InBreak);
    let paused = state == RunState::Paused;
    let start_text = if started {
        format!("Running · {}", human_dur(running_secs))
    } else {
        "Start".to_string()
    };

    let key = format!("{started}|{paused}|{start_text}");
    {
        let mut cache = menu.cache.lock().unwrap();
        if *cache == key {
            return;
        }
        *cache = key;
    }

    let start_item = menu.start.clone();
    let pause_item = menu.pause.clone();
    let _ = app.run_on_main_thread(move || {
        let _ = start_item.set_text(&start_text);
        let _ = start_item.set_checked(started);
        let _ = pause_item.set_checked(paused);
    });
}

/// Coarse, minute-granularity duration for the menu (avoids per-second tray churn).
fn human_dur(secs: u64) -> String {
    let m = secs / 60;
    if m == 0 {
        "<1m".to_string()
    } else if m < 60 {
        format!("{m}m")
    } else {
        format!("{}h {}m", m / 60, m % 60)
    }
}
