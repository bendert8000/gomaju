//! On-screen toasts for countdown timers — small always-on-top windows stacked bottom-right above
//! the tray. Two families:
//! - `timer-toast-<id>` — a live **countdown** toast per *running* timer, shown only when the
//!   `show_timer_toasts` setting is on. It closes at 00:00 (the run leaves `countdown_runtime`).
//! - `timer-done-<id>` — a persistent **finish** toast, created on **every** fire (drawn from
//!   [`AppState::finished_toasts`]) and kept until the user dismisses it. With per-timer toasts
//!   **on** it counts **overtime** past zero — a countdown into the negative (`-00:12`), a count-up
//!   restarting from zero (`00:16`); with them **off** it shows a static **"Time's up!"**.
//!
//! So at finish the running toast closes and the finish toast takes over (a brief window swap that
//! coincides with the chime). Both families are reconciled by [`sync`]: it builds the *desired* set
//! ([`desired_toasts`] — running timers when the setting is on, plus the finished set pruned to
//! config-member ids, which self-heals any delete/fire race) and diffs it against the *actual*
//! `timer-toast-*` / `timer-done-*` windows, creating/closing only the difference, then re-stacking.
//! `sync` is driven from the **countdown scheduler's background thread** (~every 250 ms, with a cheap
//! label-set early-out recomputed from live windows) — NOT from the start/pause/reset/dismiss
//! commands. That matters: those commands run on the main thread inside a WebView2 IPC callback, and
//! creating a webview window from there re-enters the message loop and deadlocks on Windows. Driving
//! it from a background thread (like the break toast) makes window creation happen in a clean
//! main-thread context.
//!
//! Both toasts count locally in JS — the running one down from an injected `remaining_secs`, the
//! finish one up from an injected `overtime_secs` (its origin is the timer's `finish_at`, so the
//! up-to-250 ms tick lag doesn't skew the count). The host authoritatively closes the running toast
//! on finish/stop; the finish toast is closed when its `finished_toasts` entry is dropped (the ✕ →
//! `cmd_dismiss_timer_done`, re-arming/resetting the timer, or deleting it). (No events are pushed to
//! the windows, so their capability needs no extra permissions.)

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
    count_up: bool,
    duration_secs: u32,
    progress: bool,
    /// Whole seconds elapsed since this timer finished (finished toasts only; 0 for running).
    overtime_secs: u32,
    /// Finished toasts only: count the overtime past zero (per-timer toasts on) vs. show a static
    /// "Time's up!" (off). Ignored by running toasts.
    count: bool,
}

/// Compute the ordered desired toast set.
/// - `running`: (id, name, remaining_secs, duration_secs) for currently-running timers, in config
///   order; included only when `show_running` (the `show_timer_toasts` setting) is true.
/// - `finished`: (id, name, overtime_secs) for timers with a pending finish toast, already pruned to
///   config-member ids, in config order; always included.
/// - `count_up`: the global `timer_count_up` setting; propagated to running **and** finished toasts
///   (a finished toast counts overtime up from zero, or a countdown's negative, per this flag).
/// - `progress`: the global `timer_toast_progress` setting; propagated to running toasts only.
///
/// A finished toast counts its overtime past zero only when `show_running` is true (per-timer toasts
/// on); when off it shows a static "Time's up!". Order is running-first then finished, so finished
/// toasts stack above running ones.
fn desired_toasts(
    show_running: bool,
    count_up: bool,
    progress: bool,
    running: &[(String, String, u32, u32)], // (id, name, remaining_secs, duration_secs)
    finished: &[(String, String, u32)],     // (id, name, overtime_secs)
) -> Vec<DesiredToast> {
    let mut out = Vec::new();
    if show_running {
        for (id, name, remaining, duration) in running {
            out.push(DesiredToast {
                id: id.clone(),
                label: label_for(id),
                name: name.clone(),
                remaining_secs: *remaining,
                finished: false,
                count_up,
                duration_secs: *duration,
                progress,
                overtime_secs: 0,
                count: false,
            });
        }
    }
    for (id, name, overtime) in finished {
        out.push(DesiredToast {
            id: id.clone(),
            label: done_label_for(id),
            name: name.clone(),
            remaining_secs: 0,
            finished: true,
            count_up,
            duration_secs: 0,
            progress: false,
            overtime_secs: *overtime,
            count: show_running,
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
    count_up: bool,
    duration_secs: u32,
    progress: bool,
    /// Finished toasts: seconds elapsed since finish, where the local overtime count starts.
    overtime_secs: u32,
    /// Finished toasts: count overtime past zero (true) vs. show static "Time's up!" (false).
    count: bool,
}

/// Reconcile the open toasts with the running timers + the `show_timer_toasts` setting. Creates
/// toasts for newly-running timers, closes toasts for stopped/finished/removed timers (or all of
/// them when the setting is off), then re-stacks. Idempotent; safe to call on any transition.
pub fn sync(app: &AppHandle) {
    let st = app.state::<AppState>();
    let now = Instant::now();

    // Config first (released): stack/display order (id + duration) + the show-toasts setting.
    let (show_running, count_up, progress, order): (bool, bool, bool, Vec<(String, u32)>) = {
        let cfg = st.config.lock().unwrap();
        let order = cfg
            .countdowns
            .iter()
            .map(|c| (c.id.clone(), c.duration_secs))
            .collect();
        (
            cfg.settings.show_timer_toasts,
            cfg.settings.timer_count_up,
            cfg.settings.timer_toast_progress,
            order,
        )
    };
    let locale = crate::i18n::current_locale(app);

    // Running set (released): running timers in config order, with their computed name + remaining.
    let running: Vec<(String, String, u32, u32)> = {
        let map = st.countdown_runtime.lock().unwrap();
        order
            .iter()
            .filter_map(|(id, dur)| match map.get(id) {
                Some(run @ CountdownRun::Running { .. }) => Some((
                    id.clone(),
                    crate::countdown::timer_display_name(*dur, &locale),
                    crate::countdown::remaining_secs(run, now),
                    *dur,
                )),
                _ => None,
            })
            .collect()
    };

    // Finished set (released): prune to config-member ids (self-healing — kills the
    // delete-then-insert resurrection race), then take them in config order, computing each one's
    // overtime (now - finish_at) for the live "counting past zero" display.
    let finished: Vec<(String, String, u32)> = {
        let mut fin = st.finished_toasts.lock().unwrap();
        let valid: HashSet<&str> = order.iter().map(|(id, _)| id.as_str()).collect();
        fin.retain(|id, _| valid.contains(id.as_str()));
        order
            .iter()
            .filter_map(|(id, _dur)| {
                fin.get(id).map(|f| {
                    let overtime = now
                        .saturating_duration_since(f.finish_at)
                        .as_secs()
                        .min(u32::MAX as u64) as u32;
                    (id.clone(), f.name.clone(), overtime)
                })
            })
            .collect()
    };

    let desired = desired_toasts(show_running, count_up, progress, &running, &finished);

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
        count_up: d.count_up,
        duration_secs: d.duration_secs,
        progress: d.progress,
        overtime_secs: d.overtime_secs,
        count: d.count,
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
        // The overtime toast (finished + counting) is a touch taller to fit its "Time's up!" note.
        .inner_size(300.0, if d.finished && d.count { 84.0 } else { 64.0 })
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

    fn run(id: &str, name: &str, secs: u32, dur: u32) -> (String, String, u32, u32) {
        (id.to_string(), name.to_string(), secs, dur)
    }
    fn fin(id: &str, name: &str, overtime: u32) -> (String, String, u32) {
        (id.to_string(), name.to_string(), overtime)
    }

    #[test]
    fn unchecked_mode_shows_only_finished_toasts() {
        let running = vec![run("a", "A", 30, 60)];
        let finished = vec![fin("b", "B", 0)];
        let d = desired_toasts(false, false, true, &running, &finished);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].label, "timer-done-b");
        assert!(d[0].finished);
        assert_eq!(d[0].id, "b");
        assert!(!d[0].progress);
        // Toasts off -> finished toast is static "Time's up!", not a counting overtime clock.
        assert!(!d[0].count);
    }

    #[test]
    fn finished_toast_counts_overtime_only_when_toasts_on() {
        let finished = vec![fin("b", "B", 12)];
        // Toasts on: the finished toast counts overtime and follows the count-up direction.
        let on = desired_toasts(true, true, true, &[], &finished);
        assert_eq!(on[0].label, "timer-done-b");
        assert!(on[0].finished);
        assert!(on[0].count, "toasts on -> finished toast counts overtime");
        assert_eq!(on[0].overtime_secs, 12);
        assert!(on[0].count_up, "count_up flag reaches finished toasts (for the sign)");
        // Toasts off: same overtime value carried, but rendered static (count = false).
        let off = desired_toasts(false, false, true, &[], &finished);
        assert!(!off[0].count);
        assert_eq!(off[0].overtime_secs, 12);
    }

    #[test]
    fn checked_mode_with_no_finished_shows_running_only() {
        let running = vec![run("a", "A", 30, 60), run("b", "B", 5, 5)];
        let d = desired_toasts(true, false, true, &running, &[]);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].label, "timer-toast-a");
        assert_eq!(d[0].remaining_secs, 30);
        assert_eq!(d[0].duration_secs, 60);
        assert!(!d[0].finished);
        assert!(!d[0].count_up);
        assert!(d[0].progress);
        assert_eq!(d[1].label, "timer-toast-b");
    }

    #[test]
    fn count_up_flag_propagates_to_running_toasts() {
        let running = vec![run("a", "A", 30, 60)];
        let d = desired_toasts(true, true, true, &running, &[]);
        assert!(d[0].count_up);
        assert_eq!(d[0].duration_secs, 60);
    }

    #[test]
    fn running_first_then_finished_in_order() {
        let running = vec![run("a", "A", 30, 60)];
        let finished = vec![fin("b", "B", 0), fin("c", "C", 0)];
        let d = desired_toasts(true, false, true, &running, &finished);
        let labels: Vec<&str> = d.iter().map(|x| x.label.as_str()).collect();
        assert_eq!(labels, ["timer-toast-a", "timer-done-b", "timer-done-c"]);
        assert_eq!((d[1].finished, d[2].finished), (true, true));
    }

    #[test]
    fn id_round_trips_through_done_label() {
        assert_eq!(id_from_done_label("timer-done-xyz"), Some("xyz"));
        assert_eq!(id_from_done_label("timer-toast-xyz"), None);
    }

    #[test]
    fn progress_flag_propagates_to_running_only() {
        let running = vec![run("a", "A", 30, 60)];
        let finished = vec![fin("b", "B", 0)];
        let d = desired_toasts(true, false, true, &running, &finished);
        assert!(d[0].progress, "running toast carries progress");
        assert!(!d[1].progress, "finished toast never shows a bar");
        let off = desired_toasts(true, false, false, &running, &[]);
        assert!(!off[0].progress, "progress off -> running toast has no bar");
    }
}
