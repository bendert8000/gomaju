use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// Label of the stopwatch window. Owned here; `commands::require_stopwatch` imports it so the
/// IPC gate can't drift from the real label. The stopwatch is a window-scoped frontend tool with
/// no backend run-state — closing the window discards it.
pub const STOPWATCH_LABEL: &str = "stopwatch";

/// Close the stopwatch window if it is open. Uses `destroy()` (not `close()`) to match the other
/// window modules; the stopwatch has no unsaved-changes guard, so there's nothing to re-enter.
pub fn close(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(STOPWATCH_LABEL) {
        let _ = window.destroy();
    }
}

/// Open the stopwatch window, or focus it if already open.
pub fn open(app: &AppHandle) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Some(existing) = app.get_webview_window(STOPWATCH_LABEL) {
            let _ = existing.show();
            let _ = existing.set_focus();
            return;
        }
        let locale = crate::i18n::current_locale(&app);
        match WebviewWindowBuilder::new(
            &app,
            STOPWATCH_LABEL,
            WebviewUrl::App("stopwatch.html".into()),
        )
        .title(crate::i18n::tr(&locale, "title.stopwatch"))
        .initialization_script(crate::webview::locale_init(&locale))
        .inner_size(420.0, 560.0)
        .min_inner_size(360.0, 420.0)
        .resizable(true)
        .center()
        .build()
        {
            Ok(window) => {
                let _ = window.set_focus();
                crate::rlog!("gomaju: stopwatch window opened");
            }
            Err(e) => crate::rlog!("gomaju: failed to open stopwatch window: {e}"),
        }
    });
}
