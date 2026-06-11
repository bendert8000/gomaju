use serde::Serialize;
use tauri::{
    AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

/// Label of the pre-break countdown toast. `commands::require_toast` imports it so the IPC gate
/// (for the "Delay 1 min" snooze) can't drift from the real label.
pub const TOAST_LABEL: &str = "warning-toast";

/// Info injected into the pre-break countdown toast before its page loads.
#[derive(Debug, Clone, Serialize)]
pub struct WarningInfo {
    /// Rule whose break is imminent — sent back by the toast's "Delay 1 min" snooze.
    pub rule_id: String,
    /// "soft" | "strict"
    pub kind: String,
    pub name: String,
    pub lead_secs: u64,
}

/// Show the pre-break countdown toast (bottom-right of the primary monitor, just above the
/// system tray). It does NOT take focus, so it won't interrupt what the user is typing.
pub fn show(app: &AppHandle, info: WarningInfo) {
    close(app);
    let app = app.clone();
    let _ = app
        .clone()
        .run_on_main_thread(move || build_toast(&app, &info));
}

/// Close the countdown toast if present.
pub fn close(app: &AppHandle) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Some(window) = app.get_webview_window(TOAST_LABEL) {
            let _ = window.close();
        }
    });
}

fn build_toast(app: &AppHandle, info: &WarningInfo) {
    let json = serde_json::to_string(info).unwrap_or_else(|_| "null".into());
    // One combined init script (payload + locale).
    let init = format!(
        "{}{}",
        crate::webview::guarded_init("__GOMAJU_WARNING__", &json),
        crate::webview::locale_init(&crate::i18n::current_locale(app)),
    );
    match WebviewWindowBuilder::new(app, TOAST_LABEL, WebviewUrl::App("toast.html".into()))
        .title("Gomaju")
        .decorations(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false) // never steal focus from the user's work
        .inner_size(410.0, 112.0)
        .visible(false)
        .initialization_script(&init)
        .build()
    {
        Ok(window) => {
            position_bottom_right(app, &window);
            let _ = window.show();
        }
        Err(e) => crate::rlog!("gomaju: failed to create warning toast: {e}"),
    }
}

fn position_bottom_right(app: &AppHandle, window: &WebviewWindow) {
    let Some(monitor) = app.primary_monitor().ok().flatten() else {
        return;
    };
    // Use the work area (excludes the taskbar) so the toast sits just above the system tray
    // rather than underneath the taskbar. `work_area()` is available in Tauri 2.11+.
    let work = monitor.work_area();
    let outer = window.outer_size().unwrap_or_default();
    let margin = (16.0 * monitor.scale_factor()).round() as i32;
    let x = work.position.x + work.size.width as i32 - outer.width as i32 - margin;
    let y = work.position.y + work.size.height as i32 - outer.height as i32 - margin;
    let _ = window.set_position(PhysicalPosition::new(x, y));
}
