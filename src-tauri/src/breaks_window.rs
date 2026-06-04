use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// Label of the breaks window (the break-rules quick-select dashboard). Owned here;
/// `commands::require_breaks` imports it so the IPC gate can't drift from the real label.
pub const BREAKS_LABEL: &str = "breaks";

/// Close the breaks window if it is open.
pub fn close(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(BREAKS_LABEL) {
        let _ = window.close();
    }
}

/// Open the breaks window, or focus it if already open.
pub fn open(app: &AppHandle) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Some(existing) = app.get_webview_window(BREAKS_LABEL) {
            let _ = existing.show();
            let _ = existing.set_focus();
            return;
        }
        let locale = crate::i18n::current_locale(&app);
        match WebviewWindowBuilder::new(&app, BREAKS_LABEL, WebviewUrl::App("breaks.html".into()))
            .title(crate::i18n::tr(&locale, "title.rules"))
            .initialization_script(crate::webview::locale_init(&locale))
            .inner_size(640.0, 600.0)
            .min_inner_size(560.0, 420.0)
            .resizable(true)
            .center()
            .build()
        {
            Ok(window) => {
                let _ = window.set_focus();
                eprintln!("restee: breaks window opened");
            }
            Err(e) => eprintln!("restee: failed to open breaks window: {e}"),
        }
    });
}
