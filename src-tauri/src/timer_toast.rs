//! On-screen toasts for **running** countdown timers — one small always-on-top window per timer,
//! stacked bottom-right above the tray. Gated by the `show_timer_toasts` setting.
//!
//! The set of toasts is reconciled by [`sync`]: it diffs the *desired* set (running timers, in
//! config order, when the setting is on) against the *actual* `timer-toast-*` windows, creating
//! and closing only the difference, then re-stacking. `sync` is driven from the **countdown
//! scheduler's background thread** (~every 250 ms, with a cheap early-out when nothing changed) —
//! NOT from the start/pause/reset commands. That matters: those commands run on the main thread
//! inside a WebView2 IPC callback, and creating a webview window from there re-enters the message
//! loop and deadlocks on Windows. Driving it from a background thread (like the break toast) makes
//! window creation happen in a clean main-thread context.
//!
//! Each toast counts down locally in JS from an injected `remaining_secs`; the host authoritatively
//! closes it on finish/stop, so any sub-second drift is cosmetic. (No events are pushed to the
//! window, so its capability needs no extra permissions.)

use std::collections::HashSet;
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

use crate::app_state::AppState;
use crate::countdown::CountdownRun;

/// Label prefix for a per-timer toast window: `timer-toast-<countdown id>`.
pub const TIMER_TOAST_PREFIX: &str = "timer-toast-";

/// The window label for a given countdown id.
fn label_for(id: &str) -> String {
    format!("{TIMER_TOAST_PREFIX}{id}")
}

/// The countdown id encoded in a toast window label, if it is one.
pub fn id_from_label(label: &str) -> Option<&str> {
    label.strip_prefix(TIMER_TOAST_PREFIX)
}

/// Data injected into a toast before its page loads.
#[derive(Serialize)]
struct ToastInfo<'a> {
    id: &'a str,
    name: &'a str,
    remaining_secs: u32,
}

/// Reconcile the open toasts with the running timers + the `show_timer_toasts` setting. Creates
/// toasts for newly-running timers, closes toasts for stopped/finished/removed timers (or all of
/// them when the setting is off), then re-stacks. Idempotent; safe to call on any transition.
pub fn sync(app: &AppHandle) {
    let st = app.state::<AppState>();
    let now = Instant::now();

    // Config first (released), then runtime (released) — never both held, matching the scheduler.
    let (enabled, order): (bool, Vec<(String, String)>) = {
        let cfg = st.config.lock().unwrap();
        let order = cfg
            .countdowns
            .iter()
            .map(|c| (c.id.clone(), c.name.clone()))
            .collect();
        (cfg.settings.show_timer_toasts, order)
    };
    // Desired toasts: running timers in config order, with their current remaining for injection.
    let desired: Vec<(String, String, u32)> = if !enabled {
        Vec::new()
    } else {
        let map = st.countdown_runtime.lock().unwrap();
        order
            .into_iter()
            .filter_map(|(id, name)| match map.get(&id) {
                Some(run @ CountdownRun::Running { .. }) => {
                    Some((id, name, crate::countdown::remaining_secs(run, now)))
                }
                _ => None,
            })
            .collect()
    };

    // Early-out: if the open toast set already matches the desired set, there's nothing to do —
    // skip the main-thread hop entirely. This makes per-tick polling from the scheduler cheap.
    // (Stack order is config order, which is stable, so an unchanged set never needs a reposition.)
    let desired_ids: HashSet<String> = desired.iter().map(|(id, _, _)| id.clone()).collect();
    let actual_ids: HashSet<String> = app
        .webview_windows()
        .into_keys()
        .filter_map(|l| id_from_label(&l).map(str::to_string))
        .collect();
    if desired_ids == actual_ids {
        return;
    }

    // Native window ops must run on the main thread.
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        // Close toasts whose timer is no longer desired.
        let actual: Vec<String> = app
            .webview_windows()
            .into_keys()
            .filter(|l| l.starts_with(TIMER_TOAST_PREFIX))
            .collect();
        for label in &actual {
            let keep = id_from_label(label)
                .map(|id| desired.iter().any(|(d, _, _)| d == id))
                .unwrap_or(false);
            if !keep {
                if let Some(w) = app.get_webview_window(label) {
                    let _ = w.close();
                }
            }
        }
        // Create toasts for newly-running timers.
        for (id, name, remaining) in &desired {
            if app.get_webview_window(&label_for(id)).is_none() {
                build_toast(&app, id, name, *remaining);
            }
        }
        // Stack them bottom-right (config order: first at the very bottom, next above it).
        relayout(&app, &desired);
    });
}

/// Build one toast window (hidden; `relayout` positions then shows it). Mirrors `toast.rs`'s
/// flags: frameless, always-on-top, never focus-stealing, off the taskbar.
fn build_toast(app: &AppHandle, id: &str, name: &str, remaining_secs: u32) {
    let json = serde_json::to_string(&ToastInfo {
        id,
        name,
        remaining_secs,
    })
    .unwrap_or_else(|_| "null".into());
    let init = format!(
        "{}{}",
        crate::webview::guarded_init("__GOMAJU_TIMER_TOAST__", &json),
        crate::webview::locale_init(&crate::i18n::current_locale(app)),
    );
    let label = label_for(id);
    match WebviewWindowBuilder::new(app, &label, WebviewUrl::App("timer-toast.html".into()))
        .title("Gomaju")
        .decorations(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false) // never steal focus from the user's work
        .inner_size(300.0, 64.0)
        .visible(false)
        .initialization_script(&init)
        .build()
    {
        Ok(_) => crate::rlog!("gomaju: timer toast opened ({name})"),
        Err(e) => crate::rlog!("gomaju: failed to create timer toast: {e}"),
    }
}

/// Stack the desired toasts at the bottom-right of the primary monitor's work area, growing
/// upward in `desired` order (index 0 nearest the tray). Positions then shows each window.
fn relayout(app: &AppHandle, desired: &[(String, String, u32)]) {
    let Some(monitor) = app.primary_monitor().ok().flatten() else {
        return;
    };
    let work = monitor.work_area();
    let scale = monitor.scale_factor();
    let margin = (16.0 * scale).round() as i32;
    let gap = (8.0 * scale).round() as i32;
    let mut y_bottom = work.position.y + work.size.height as i32 - margin;
    for (id, _, _) in desired {
        let Some(window) = app.get_webview_window(&label_for(id)) else {
            continue;
        };
        let outer = window.outer_size().unwrap_or_default();
        let x = work.position.x + work.size.width as i32 - outer.width as i32 - margin;
        let y = y_bottom - outer.height as i32;
        let _ = window.set_position(PhysicalPosition::new(x, y));
        let _ = window.show();
        y_bottom = y - gap;
    }
}
