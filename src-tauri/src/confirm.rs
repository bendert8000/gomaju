//! A single in-app confirm prompt, centered on the **primary** monitor.
//!
//! Replaces the native OS message dialogs that the tray (and the startup resume prompt) used to
//! show: a parentless native dialog lands on whatever monitor has focus, not reliably the user's
//! main screen. This frameless always-on-top window is positioned on the primary monitor's work
//! area instead.
//!
//! It is generic: the caller passes a [`ConfirmInfo`] carrying the action descriptor (`kind` +
//! `rule_id`, echoed back by the window so the backend knows what to run) and the already-localized
//! strings to display. The window reports which button was clicked via `cmd_confirm_resolve`, which
//! routes to [`crate::runtime::resolve_confirm`]. One prompt at a time — a new one replaces the old.
//!
//! Like the toasts, it is built off the main thread (a background thread hops to the main thread),
//! so creating the webview can't re-enter a tray-menu / WebView2-IPC callback and deadlock on
//! Windows.

use serde::Serialize;
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// The single confirm-prompt window label. Commands gate on it so only this webview can resolve.
pub const CONFIRM_LABEL: &str = "confirm";

/// Everything a confirm prompt needs. `kind`/`rule_id` are the action descriptor (echoed back by
/// the window); the rest are display strings, localized by the caller.
#[derive(Debug, Clone, Serialize)]
pub struct ConfirmInfo {
    /// "break_one" | "reset_all" | "reset_one" | "resume" — dispatched by `runtime::resolve_confirm`.
    pub kind: String,
    /// Payload for `break_one` / `reset_one` (empty otherwise).
    pub rule_id: String,
    pub title: String,
    pub message: String,
    /// Affirmative button label (take break / reset / resume).
    pub primary: String,
    /// Secondary button label (cancel / start fresh); also the Esc/close action.
    pub secondary: String,
}

/// Show the confirm prompt centered on the primary monitor. Built off the main thread so creating
/// the webview can't re-enter a tray-menu / IPC callback and deadlock WebView2 on Windows. A new
/// prompt replaces any open one.
pub fn show(app: &AppHandle, info: ConfirmInfo) {
    let app = app.clone();
    std::thread::spawn(move || {
        let _ = app.clone().run_on_main_thread(move || {
            destroy_existing(&app);
            build_window(&app, &info);
        });
    });
}

/// Close the confirm prompt if open. Marshalled to the main thread.
pub fn close(app: &AppHandle) {
    let app = app.clone();
    let _ = app
        .clone()
        .run_on_main_thread(move || destroy_existing(&app));
}

/// Destroy the confirm window if present. MUST run on the main thread. `destroy()` (forcible) so a
/// wedged webview is still reaped and its label freed before a rebuild in the same closure.
fn destroy_existing(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(CONFIRM_LABEL) {
        let _ = window.destroy();
    }
}

fn build_window(app: &AppHandle, info: &ConfirmInfo) {
    let json = serde_json::to_string(info).unwrap_or_else(|_| "null".into());
    let init = format!(
        "{}{}",
        crate::webview::guarded_init("__GOMAJU_CONFIRM__", &json),
        crate::webview::locale_init(&crate::i18n::current_locale(app)),
    );
    match WebviewWindowBuilder::new(app, CONFIRM_LABEL, WebviewUrl::App("confirm.html".into()))
        .title("Gomaju")
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(true) // a prompt should take focus so its buttons + Esc work
        .inner_size(380.0, 200.0)
        .visible(false)
        .initialization_script(&init)
        .build()
    {
        Ok(window) => {
            center_on_primary(app, &window);
            let _ = window.show();
            let _ = window.set_focus();
        }
        Err(e) => crate::rlog!("gomaju: failed to create confirm window: {e}"),
    }
}

/// Center the window on the primary monitor's work area — so the prompt lands on the user's MAIN
/// screen regardless of which monitor currently has focus.
fn center_on_primary(app: &AppHandle, window: &WebviewWindow) {
    let Some(monitor) = app.primary_monitor().ok().flatten() else {
        return;
    };
    let work = monitor.work_area();
    let outer = window.outer_size().unwrap_or_default();
    let x = work.position.x + (work.size.width as i32 - outer.width as i32) / 2;
    let y = work.position.y + (work.size.height as i32 - outer.height as i32) / 2;
    let _ = window.set_position(PhysicalPosition::new(
        x.max(work.position.x),
        y.max(work.position.y),
    ));
}
