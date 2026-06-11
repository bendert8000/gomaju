use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// Label of the (single) privileged window. Owned here because this module builds it;
/// `commands::require_settings` imports it so the gate can't drift from the real label.
pub const SETTINGS_LABEL: &str = "settings";

/// Close the settings window if it is open. Uses `destroy()` (not `close()`) so it does NOT
/// re-emit `close-requested`: the frontend's unsaved-changes guard already ran and approved the
/// close, and re-emitting would re-enter that guard. See `src/unsaved-guard.ts`.
pub fn close(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(SETTINGS_LABEL) {
        let _ = window.destroy();
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
        let locale = crate::i18n::current_locale(&app);
        match WebviewWindowBuilder::new(&app, SETTINGS_LABEL, WebviewUrl::App("index.html".into()))
            .title(crate::i18n::tr(&locale, "title.settings"))
            .initialization_script(crate::webview::locale_init(&locale))
            .inner_size(760.0, 720.0)
            .min_inner_size(560.0, 480.0)
            .resizable(true)
            .center()
            .build()
        {
            Ok(window) => {
                let _ = window.set_focus();
                crate::rlog!("gomaju: settings window opened");
            }
            Err(e) => crate::rlog!("gomaju: failed to open settings window: {e}"),
        }
    });
}
