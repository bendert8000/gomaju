use tauri::{
    AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

use crate::app_state::AppState;

/// Label of the pause reminder toast. Commands gate on this label so only this webview can
/// answer the reminder prompt.
pub const PAUSE_TOAST_LABEL: &str = "pause-toast";

/// Show the pause reminder toast near the tray. Unlike a native dialog it does not take focus,
/// matching the pre-break countdown toast's lightweight behavior.
pub fn show(app: &AppHandle) {
    close(app);
    let app = app.clone();
    let _ = app
        .clone()
        .run_on_main_thread(move || build_pause_toast(&app));
}

pub fn close(app: &AppHandle) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Some(window) = app.get_webview_window(PAUSE_TOAST_LABEL) {
            let _ = window.close();
        }
    });
}

fn build_pause_toast(app: &AppHandle) {
    // Inject the configured interval (in whole minutes) so the toast hint can name it,
    // plus the locale — same pattern as the pre-break countdown toast.
    let payload = format!("{{\"minutes\":{}}}", pause_reminder_minutes(app));
    let init = format!(
        "{}{}",
        crate::webview::guarded_init("__GOMAJU_PAUSE__", &payload),
        crate::webview::locale_init(&crate::i18n::current_locale(app)),
    );
    match WebviewWindowBuilder::new(
        app,
        PAUSE_TOAST_LABEL,
        WebviewUrl::App("pause-toast.html".into()),
    )
    .title("Gomaju")
    .decorations(false)
    .always_on_top(true)
    .visible_on_all_workspaces(true)
    .skip_taskbar(true)
    .resizable(false)
    .focused(false)
    .inner_size(370.0, 198.0)
    .visible(false)
    .initialization_script(&init)
    .build()
    {
        Ok(window) => {
            position_bottom_right(app, &window);
            let _ = window.show();
        }
        Err(e) => crate::rlog!("gomaju: failed to create pause reminder toast: {e}"),
    }
}

/// The reminder interval rounded to whole minutes (the Settings UI is minute-granular),
/// never below 1 — matches how `main.ts` renders the same value.
fn pause_reminder_minutes(app: &AppHandle) -> u64 {
    let secs = app
        .state::<AppState>()
        .config
        .lock()
        .unwrap()
        .settings
        .pause_reminder_interval_secs;
    ((secs + 30) / 60).max(1)
}

fn position_bottom_right(app: &AppHandle, window: &WebviewWindow) {
    let Some(monitor) = app.primary_monitor().ok().flatten() else {
        return;
    };
    let work = monitor.work_area();
    let outer = window.outer_size().unwrap_or_default();
    let margin = (16.0 * monitor.scale_factor()).round() as i32;
    let x = work.position.x + work.size.width as i32 - outer.width as i32 - margin;
    let y = work.position.y + work.size.height as i32 - outer.height as i32 - margin;
    let _ = window.set_position(PhysicalPosition::new(x, y));
}
