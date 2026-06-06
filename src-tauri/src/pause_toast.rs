use tauri::{
    AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

/// Label of the pause reminder toast. Commands gate on this label so only this webview can
/// answer the reminder prompt.
pub const PAUSE_TOAST_LABEL: &str = "pause-toast";

/// Show the pause reminder toast near the tray. Unlike a native dialog it does not take focus,
/// matching the pre-break countdown toast's lightweight behavior.
pub fn show(app: &AppHandle) {
    close(app);
    let app = app.clone();
    let _ = app
        .clone()
        .run_on_main_thread(move || build_pause_toast(&app));
}

pub fn close(app: &AppHandle) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Some(window) = app.get_webview_window(PAUSE_TOAST_LABEL) {
            let _ = window.close();
        }
    });
}

fn build_pause_toast(app: &AppHandle) {
    let init = crate::webview::locale_init(&crate::i18n::current_locale(app));
    match WebviewWindowBuilder::new(
        app,
        PAUSE_TOAST_LABEL,
        WebviewUrl::App("pause-toast.html".into()),
    )
    .title("Restee")
    .decorations(false)
    .always_on_top(true)
    .visible_on_all_workspaces(true)
    .skip_taskbar(true)
    .resizable(false)
    .focused(false)
    .inner_size(370.0, 124.0)
    .visible(false)
    .initialization_script(&init)
    .build()
    {
        Ok(window) => {
            position_bottom_right(app, &window);
            let _ = window.show();
        }
        Err(e) => eprintln!("restee: failed to create pause reminder toast: {e}"),
    }
}

fn position_bottom_right(app: &AppHandle, window: &WebviewWindow) {
    let Some(monitor) = app.primary_monitor().ok().flatten() else {
        return;
    };
    let work = monitor.work_area();
    let outer = window.outer_size().unwrap_or_default();
    let margin = (16.0 * monitor.scale_factor()).round() as i32;
    let x = work.position.x + work.size.width as i32 - outer.width as i32 - margin;
    let y = work.position.y + work.size.height as i32 - outer.height as i32 - margin;
    let _ = window.set_position(PhysicalPosition::new(x, y));
}
