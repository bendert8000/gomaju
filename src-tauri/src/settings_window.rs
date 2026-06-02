use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// Label of the (single) privileged window. Owned here because this module builds it;
/// `commands::require_settings` imports it so the gate can't drift from the real label.
pub const SETTINGS_LABEL: &str = "settings";

/// Close the settings window if it is open.
pub fn close(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(SETTINGS_LABEL) {
        let _ = window.close();
    }
}

/// Open the settings window, or focus it if already open.
pub fn open(app: &AppHandle) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Some(existing) = app.get_webview_window(SETTINGS_LABEL) {
            let _ = existing.show();
            let _ = existing.set_focus();
            return;
        }
        match WebviewWindowBuilder::new(&app, SETTINGS_LABEL, WebviewUrl::App("index.html".into()))
            .title("restee — Settings")
            .inner_size(760.0, 720.0)
            .min_inner_size(560.0, 480.0)
            .resizable(true)
            .center()
            .build()
        {
            Ok(window) => {
                let _ = window.set_focus();
                eprintln!("restee: settings window opened");
            }
            Err(e) => eprintln!("restee: failed to open settings window: {e}"),
        }
    });
}
