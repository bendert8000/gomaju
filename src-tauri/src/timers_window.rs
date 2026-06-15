use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// Label of the timers window. Owned here; `commands::require_timers` imports it so the
/// IPC gate can't drift from the real label. (Backend commands use the `countdown` noun,
/// but the window/UI keeps the user-facing "timers" name.)
pub const TIMERS_LABEL: &str = "timers";

/// Close the timers window if it is open. Uses `destroy()` (not `close()`) so it does NOT
/// re-emit `close-requested`: the frontend's unsaved-changes guard already ran and approved the
/// close, and re-emitting would re-enter that guard. See `src/unsaved-guard.ts`.
pub fn close(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(TIMERS_LABEL) {
        let _ = window.destroy();
    }
}

/// Open the timers window, or focus it if already open.
pub fn open(app: &AppHandle) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Some(existing) = app.get_webview_window(TIMERS_LABEL) {
            let _ = existing.show();
            let _ = existing.set_focus();
            return;
        }
        let locale = crate::i18n::current_locale(&app);
        match WebviewWindowBuilder::new(&app, TIMERS_LABEL, WebviewUrl::App("timers.html".into()))
            .title(crate::i18n::tr(&locale, "title.timers"))
            .initialization_script(crate::webview::locale_init(&locale))
            .inner_size(640.0, 720.0)
            .min_inner_size(520.0, 480.0)
            .resizable(true)
            .center()
            .build()
        {
            Ok(window) => {
                let _ = window.set_focus();
                crate::rlog!("gomaju: timers window opened");
            }
            Err(e) => crate::rlog!("gomaju: failed to open timers window: {e}"),
        }
    });
}
