use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// Label of the chimes window. Owned here; `commands::require_chimes` imports it so the IPC
/// gate can't drift from the real label.
pub const CHIMES_LABEL: &str = "chimes";

/// Close the chimes window if it is open. Uses `destroy()` (not `close()`) so it does NOT
/// re-emit `close-requested`: the frontend's unsaved-changes guard already ran and approved the
/// close, and re-emitting would re-enter that guard. See `src/unsaved-guard.ts`.
pub fn close(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(CHIMES_LABEL) {
        let _ = window.destroy();
    }
}

/// Open the chimes window, or focus it if already open.
pub fn open(app: &AppHandle) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Some(existing) = app.get_webview_window(CHIMES_LABEL) {
            let _ = existing.show();
            let _ = existing.set_focus();
            return;
        }
        let locale = crate::i18n::current_locale(&app);
        match WebviewWindowBuilder::new(&app, CHIMES_LABEL, WebviewUrl::App("chimes.html".into()))
            .title(crate::i18n::tr(&locale, "title.chimes"))
            .initialization_script(crate::webview::locale_init(&locale))
            .inner_size(680.0, 720.0)
            .min_inner_size(560.0, 480.0)
            .resizable(true)
            // Turn off Tauri's OS-level drag-drop (file-drop) handler: on Windows it intercepts
            // drag events before the webview, which breaks the HTML5 drag-and-drop used to reorder
            // melody chips (`src/chimes.ts`). This window has no file-drop feature, so disabling it
            // is safe; chime import uses a native picker, not drag-drop.
            .disable_drag_drop_handler()
            .center()
            .build()
        {
            Ok(window) => {
                let _ = window.set_focus();
                crate::rlog!("gomaju: chimes window opened");
            }
            Err(e) => crate::rlog!("gomaju: failed to open chimes window: {e}"),
        }
    });
}
