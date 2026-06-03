use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const OVERLAY_LABEL_PREFIX: &str = "overlay-";

/// Break details injected into each overlay window before its page loads, so the
/// overlay never depends on event timing to render correctly.
#[derive(Debug, Clone, Serialize)]
pub struct BreakInfo {
    /// "soft" | "strict"
    pub kind: String,
    pub name: String,
    pub duration_secs: u64,
    /// "friction" | "easy" | "no_easy_escape"
    pub escape_mode: String,
    /// "countdown" | "progress_bar" — how the overlay renders the timer.
    pub break_display: String,
    /// Optional per-rule note shown read-only under the break name (empty = no note).
    pub note: String,
}

/// Show the break: one fullscreen, always-on-top overlay per monitor.
/// Existing overlays are cleared first. Window work is marshalled to the main thread.
pub fn show_break(app: &AppHandle, info: BreakInfo) {
    close_all(app);
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        create_overlays(&app, &info);
    });
}

/// Close all overlay windows (end of break / skip).
pub fn close_all(app: &AppHandle) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        for (label, window) in app.webview_windows() {
            if label.starts_with(OVERLAY_LABEL_PREFIX) {
                let _ = window.close();
            }
        }
    });
}

fn create_overlays(app: &AppHandle, info: &BreakInfo) {
    let json = serde_json::to_string(info).unwrap_or_else(|_| "null".into());
    // One combined init script (payload + locale); Tauri's multi-script append isn't relied on.
    let init = format!(
        "{}{}",
        crate::webview::guarded_init("__RESTEE_BREAK__", &json),
        crate::webview::locale_init(&crate::i18n::current_locale(app)),
    );

    let monitors = app.available_monitors().unwrap_or_default();
    eprintln!("restee: creating overlays for {} monitor(s)", monitors.len());
    if monitors.is_empty() {
        build_one(app, 0, &init, None);
        return;
    }
    for (i, monitor) in monitors.iter().enumerate() {
        let pos = *monitor.position();
        let size = *monitor.size();
        build_one(app, i, &init, Some((pos, size)));
    }
}

fn build_one(
    app: &AppHandle,
    index: usize,
    init_script: &str,
    geom: Option<(tauri::PhysicalPosition<i32>, tauri::PhysicalSize<u32>)>,
) {
    let label = format!("{OVERLAY_LABEL_PREFIX}{index}");
    let builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("overlay.html".into()))
        .title("Restee")
        .decorations(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .shadow(false)
        .resizable(false)
        .visible(false)
        .initialization_script(init_script);

    match builder.build() {
        Ok(window) => {
            if let Some((pos, size)) = geom {
                let _ = window.set_position(pos);
                let _ = window.set_size(size);
            }
            let _ = window.set_fullscreen(true);
            let _ = window.set_always_on_top(true);
            let _ = window.show();
            let _ = window.set_focus();
        }
        Err(e) => eprintln!("restee: failed to create overlay {label}: {e}"),
    }
}
