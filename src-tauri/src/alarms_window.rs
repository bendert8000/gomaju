use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// Label of the alarms window. Owned here; `commands::require_alarms` imports it so the
/// IPC gate can't drift from the real label.
pub const ALARMS_LABEL: &str = "alarms";

/// Close the alarms window if it is open.
pub fn close(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(ALARMS_LABEL) {
        let _ = window.close();
    }
}

/// Open the alarms window, or focus it if already open.
pub fn open(app: &AppHandle) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Some(existing) = app.get_webview_window(ALARMS_LABEL) {
            let _ = existing.show();
            let _ = existing.set_focus();
            return;
        }
        match WebviewWindowBuilder::new(&app, ALARMS_LABEL, WebviewUrl::App("alarms.html".into()))
            .title("restee — Alarms")
            .inner_size(640.0, 720.0)
            .min_inner_size(520.0, 480.0)
            .resizable(true)
            .center()
            .build()
        {
            Ok(window) => {
                let _ = window.set_focus();
                eprintln!("restee: alarms window opened");
            }
            Err(e) => eprintln!("restee: failed to open alarms window: {e}"),
        }
    });
}
