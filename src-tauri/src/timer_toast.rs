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

/// Label prefix for a finished-timer "time's up" toast window: `timer-done-<countdown id>`.
pub const TIMER_DONE_PREFIX: &str = "timer-done-";

/// The window label for a given countdown id's "time's up" toast.
fn done_label_for(id: &str) -> String {
    format!("{TIMER_DONE_PREFIX}{id}")
}

/// The countdown id encoded in a "time's up" toast window label, if it is one.
pub fn id_from_done_label(label: &str) -> Option<&str> {
    label.strip_prefix(TIMER_DONE_PREFIX)
}

/// One toast the reconciler wants open, in stack order (index 0 nearest the tray).
#[derive(Debug, Clone, PartialEq, Eq)]
struct DesiredToast {
    id: String,
    label: String,
    name: String,
    remaining_secs: u32,
    finished: bool,
}

/// Compute the ordered desired toast set.
/// - `running`: (id, name, remaining_secs) for currently-running timers, in config order; included
///   only when `show_running` (the `show_timer_toasts` setting) is true.
/// - `finished`: (id, name) for timers with a pending "time's up" toast, already pruned to
///   config-member ids, in config order; always included.
///
/// Order is running-first then finished, so finished toasts stack above running ones.
fn desired_toasts(
    show_running: bool,
    running: &[(String, String, u32)],
    finished: &[(String, String)],
) -> Vec<DesiredToast> {
    let mut out = Vec::new();
    if show_running {
        for (id, name, remaining) in running {
            out.push(DesiredToast {
                id: id.clone(),
                label: label_for(id),
                name: name.clone(),
                remaining_secs: *remaining,
                finished: false,
            });
        }
    }
    for (id, name) in finished {
        out.push(DesiredToast {
            id: id.clone(),
            label: done_label_for(id),
            name: name.clone(),
            remaining_secs: 0,
            finished: true,
        });
    }
    out
}

/// Data injected into a toast before its page loads.
#[derive(Serialize)]
struct ToastInfo<'a> {
    id: &'a str,
    name: &'a str,
    remaining_secs: u32,
    finished: bool,
}

/// Reconcile the open toasts with the running timers + the `show_timer_toasts` setting. Creates
/// toasts for newly-running timers, closes toasts for stopped/finished/removed timers (or all of
/// them when the setting is off), then re-stacks. Idempotent; safe to call on any transition.
pub fn sync(app: &AppHandle) {
    let st = app.state::<AppState>();
    let now = Instant::now();

    // Config first (released): stack/display order + the show-toasts setting.
    let (show_running, order): (bool, Vec<(String, String)>) = {
        let cfg = st.config.lock().unwrap();
        let order = cfg
            .countdowns
            .iter()
            .map(|c| (c.id.clone(), c.name.clone()))
            .collect();
        (cfg.settings.show_timer_toasts, order)
    };

    // Running set (released): running timers in config order, with current remaining for injection.
    let running: Vec<(String, String, u32)> = {
        let map = st.countdown_runtime.lock().unwrap();
        order
            .iter()
            .filter_map(|(id, name)| match map.get(id) {
                Some(run @ CountdownRun::Running { .. }) => {
                    Some((id.clone(), name.clone(), crate::countdown::remaining_secs(run, now)))
                }
                _ => None,
            })
            .collect()
    };

    // Finished set (released): prune to config-member ids (self-healing — kills the
    // delete-then-insert resurrection race), then take them in config order.
    let finished: Vec<(String, String)> = {
        let mut fin = st.finished_toasts.lock().unwrap();
        let valid: HashSet<&str> = order.iter().map(|(id, _)| id.as_str()).collect();
        fin.retain(|id, _| valid.contains(id.as_str()));
        order
            .iter()
            .filter_map(|(id, _)| fin.get(id).map(|name| (id.clone(), name.clone())))
            .collect()
    };

    let desired = desired_toasts(show_running, &running, &finished);

    // Early-out: desired label set vs actually-open toast windows (both families). Recomputed from
    // live windows every tick so a transient creation failure self-corrects next tick.
    let desired_labels: HashSet<String> = desired.iter().map(|d| d.label.clone()).collect();
    let actual_labels: HashSet<String> = app
        .webview_windows()
        .into_keys()
        .filter(|l| l.starts_with(TIMER_TOAST_PREFIX) || l.starts_with(TIMER_DONE_PREFIX))
        .collect();
    if desired_labels == actual_labels {
        return;
    }

    // Native window ops must run on the main thread.
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        // Close toast windows (either family) no longer desired.
        let actual: Vec<String> = app
            .webview_windows()
            .into_keys()
            .filter(|l| l.starts_with(TIMER_TOAST_PREFIX) || l.starts_with(TIMER_DONE_PREFIX))
            .collect();
        for label in &actual {
            if !desired.iter().any(|d| &d.label == label) {
                if let Some(w) = app.get_webview_window(label) {
                    let _ = w.close();
                }
            }
        }
        // Create toast windows for the newly-desired ones.
        for d in &desired {
            if app.get_webview_window(&d.label).is_none() {
                build_toast(&app, d);
            }
        }
        // Stack them bottom-right in desired order (index 0 nearest the tray).
        relayout(&app, &desired);
    });
}

/// Build one toast window (hidden; `relayout` positions then shows it). Mirrors `toast.rs`'s
/// flags: frameless, always-on-top, never focus-stealing, off the taskbar.
fn build_toast(app: &AppHandle, d: &DesiredToast) {
    let json = serde_json::to_string(&ToastInfo {
        id: &d.id,
        name: &d.name,
        remaining_secs: d.remaining_secs,
        finished: d.finished,
    })
    .unwrap_or_else(|_| "null".into());
    let init = format!(
        "{}{}",
        crate::webview::guarded_init("__GOMAJU_TIMER_TOAST__", &json),
        crate::webview::locale_init(&crate::i18n::current_locale(app)),
    );
    match WebviewWindowBuilder::new(app, &d.label, WebviewUrl::App("timer-toast.html".into()))
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
        Ok(_) => crate::rlog!(
            "gomaju: {} toast opened ({})",
            if d.finished { "timer-done" } else { "timer" },
            d.name
        ),
        Err(e) => crate::rlog!("gomaju: failed to create timer toast: {e}"),
    }
}

/// Stack the desired toasts at the bottom-right of the primary monitor's work area, growing
/// upward in `desired` order (index 0 nearest the tray). Positions then shows each window.
fn relayout(app: &AppHandle, desired: &[DesiredToast]) {
    let Some(monitor) = app.primary_monitor().ok().flatten() else {
        return;
    };
    let work = monitor.work_area();
    let scale = monitor.scale_factor();
    let margin = (16.0 * scale).round() as i32;
    let gap = (8.0 * scale).round() as i32;
    let mut y_bottom = work.position.y + work.size.height as i32 - margin;
    for d in desired {
        let Some(window) = app.get_webview_window(&d.label) else {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn run(id: &str, name: &str, secs: u32) -> (String, String, u32) {
        (id.to_string(), name.to_string(), secs)
    }
    fn fin(id: &str, name: &str) -> (String, String) {
        (id.to_string(), name.to_string())
    }

    #[test]
    fn unchecked_mode_shows_only_finished_toasts() {
        let running = vec![run("a", "A", 30)];
        let finished = vec![fin("b", "B")];
        let d = desired_toasts(false, &running, &finished);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].label, "timer-done-b");
        assert!(d[0].finished);
        assert_eq!(d[0].id, "b");
    }

    #[test]
    fn checked_mode_with_no_finished_shows_running_only() {
        let running = vec![run("a", "A", 30), run("b", "B", 5)];
        let d = desired_toasts(true, &running, &[]);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].label, "timer-toast-a");
        assert_eq!(d[0].remaining_secs, 30);
        assert!(!d[0].finished);
        assert_eq!(d[1].label, "timer-toast-b");
    }

    #[test]
    fn running_first_then_finished_in_order() {
        let running = vec![run("a", "A", 30)];
        let finished = vec![fin("b", "B"), fin("c", "C")];
        let d = desired_toasts(true, &running, &finished);
        let labels: Vec<&str> = d.iter().map(|x| x.label.as_str()).collect();
        assert_eq!(labels, ["timer-toast-a", "timer-done-b", "timer-done-c"]);
        assert_eq!((d[1].finished, d[2].finished), (true, true));
    }

    #[test]
    fn id_round_trips_through_done_label() {
        assert_eq!(id_from_done_label("timer-done-xyz"), Some("xyz"));
        assert_eq!(id_from_done_label("timer-toast-xyz"), None);
    }
}
