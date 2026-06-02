use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// Label of the break-rules window. Owned here; `commands::require_rules` imports it so the
/// IPC gate can't drift from the real label.
pub const RULES_LABEL: &str = "rules";

/// Close the rules window if it is open.
pub fn close(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(RULES_LABEL) {
        let _ = window.close();
    }
}

/// Open the rules window, or focus it if already open.
pub fn open(app: &AppHandle) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Some(existing) = app.get_webview_window(RULES_LABEL) {
            let _ = existing.show();
            let _ = existing.set_focus();
            return;
        }
        match WebviewWindowBuilder::new(&app, RULES_LABEL, WebviewUrl::App("rules.html".into()))
            .title("restee — Break rules")
            .inner_size(640.0, 600.0)
            .min_inner_size(560.0, 420.0)
            .resizable(true)
            .center()
            .build()
        {
            Ok(window) => {
                let _ = window.set_focus();
                eprintln!("restee: rules window opened");
            }
            Err(e) => eprintln!("restee: failed to open rules window: {e}"),
        }
    });
}
