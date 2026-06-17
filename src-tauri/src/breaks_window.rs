use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// Label of the breaks window (the break-rules quick-select dashboard). Owned here;
/// `commands::require_breaks` imports it so the IPC gate can't drift from the real label.
pub const BREAKS_LABEL: &str = "breaks";

/// Close the breaks window if it is open. Uses `destroy()` (not the cooperative `close()`) to
/// match the other window modules; the breaks dashboard has no close-requested guard, so there's
/// nothing to re-enter, and a wedged webview is still reaped.
pub fn close(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(BREAKS_LABEL) {
        let _ = window.destroy();
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
                crate::rlog!("gomaju: breaks window opened");
            }
            Err(e) => crate::rlog!("gomaju: failed to open breaks window: {e}"),
        }
    });
}
