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
    /// Optional inspirational quote shown under the note (empty = no quote).
    pub quote: String,
}

/// Show the break: one fullscreen, always-on-top overlay per monitor.
/// Any stale overlays are force-destroyed first, in the SAME main-thread closure as the rebuild,
/// so a wedged `overlay-*` left over from a transient WebView2 hiccup can't collide with (and
/// block) the new break. Window work is marshalled to the main thread.
pub fn show_break(app: &AppHandle, info: BreakInfo) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        destroy_all_overlays(&app);
        create_overlays(&app, &info);
    });
}

/// Close all overlay windows (end of break / skip).
pub fn close_all(app: &AppHandle) {
    let app = app.clone();
    let _ = app
        .clone()
        .run_on_main_thread(move || destroy_all_overlays(&app));
}

/// Force-destroy every overlay window. MUST run on the main thread. Uses `destroy()` (not the
/// cooperative `close()`): a webview whose renderer wedged never honors a close-request, so only
/// a forcible destroy reaps it — freeing its `overlay-N` label and releasing its WebView2 host.
fn destroy_all_overlays(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label.starts_with(OVERLAY_LABEL_PREFIX) {
            let _ = window.destroy();
        }
    }
}

fn create_overlays(app: &AppHandle, info: &BreakInfo) {
    let json = serde_json::to_string(info).unwrap_or_else(|_| "null".into());
    // One combined init script (payload + locale); Tauri's multi-script append isn't relied on.
    let init = format!(
        "{}{}",
        crate::webview::guarded_init("__GOMAJU_BREAK__", &json),
        crate::webview::locale_init(&crate::i18n::current_locale(app)),
    );

    let monitors = app.available_monitors().unwrap_or_default();
    crate::rlog!(
        "gomaju: creating overlays for {} monitor(s)",
        monitors.len()
    );
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

type Geom = Option<(tauri::PhysicalPosition<i32>, tauri::PhysicalSize<u32>)>;

fn build_one(app: &AppHandle, index: usize, init_script: &str, geom: Geom) {
    let label = format!("{OVERLAY_LABEL_PREFIX}{index}");
    match try_build_overlay(app, &label, init_script) {
        Ok(window) => {
            crate::rlog!("gomaju: overlay {label} built ok");
            place_and_show(&window, geom);
        }
        Err(e) => {
            // A same-label window is still registered — e.g. a wedged overlay that never tore
            // down. Force-destroy it and rebuild once, so one transient WebView2 hiccup can't
            // permanently brick every future break with "already exists".
            if let Some(stale) = app.get_webview_window(&label) {
                crate::rlog!("gomaju: overlay {label} exists; destroying + retrying ({e})");
                let _ = stale.destroy();
                match try_build_overlay(app, &label, init_script) {
                    Ok(window) => {
                        crate::rlog!("gomaju: overlay {label} rebuilt ok");
                        place_and_show(&window, geom);
                    }
                    Err(e2) => crate::rlog!("gomaju: overlay {label} rebuild failed: {e2}"),
                }
            } else {
                crate::rlog!("gomaju: failed to create overlay {label}: {e}");
            }
        }
    }
}

fn try_build_overlay(
    app: &AppHandle,
    label: &str,
    init_script: &str,
) -> tauri::Result<tauri::WebviewWindow> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::App("overlay.html".into()))
        .title("Gomaju")
        .decorations(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .shadow(false)
        .resizable(false)
        .visible(false)
        .initialization_script(init_script)
        .build()
}

fn place_and_show(window: &tauri::WebviewWindow, geom: Geom) {
    // Per-step instrumentation: each line lands in gomaju.log only after the *previous* native
    // call returned. If the overlay wedges, the LAST line printed names the call that blocked the
    // Windows UI thread (so the WebView2 controller's async init never completes and the page never
    // loads). Remove once the wedge is root-caused.
    let label = window.label().to_string();
    if let Some((pos, size)) = geom {
        let _ = window.set_position(pos);
        let _ = window.set_size(size);
    }
    crate::rlog!("gomaju: overlay {label} -> set_fullscreen");
    let _ = window.set_fullscreen(true);
    crate::rlog!("gomaju: overlay {label} -> set_always_on_top");
    let _ = window.set_always_on_top(true);
    crate::rlog!("gomaju: overlay {label} -> show");
    let _ = window.show();
    crate::rlog!("gomaju: overlay {label} -> set_focus");
    let _ = window.set_focus();
    crate::rlog!("gomaju: overlay {label} -> place_and_show done");
}
