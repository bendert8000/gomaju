//! Persistent on-screen toasts for fired wall-clock alarms — small always-on-top windows stacked
//! bottom-right above the tray, one `alarm-toast-<alarm id>` per fired alarm. Unlike a native OS
//! notification (which the OS auto-dismisses), this toast **stays until the user clicks ✕**. It
//! shows the alarm's name plus its time and recurrence. The pending set lives in
//! [`AppState::fired_alarm_toasts`] (filled by the alarm scheduler on every fire, cleared by
//! `cmd_dismiss_alarm_toast`); [`sync`] reconciles that set into windows.
//!
//! `sync` runs from the **alarm scheduler's 1s background thread**, NOT from the dismiss command:
//! creating/closing a webview window from a command runs on the main thread inside a WebView2 IPC
//! callback and deadlocks on Windows. Driving it from the background thread keeps window creation in
//! a clean main-thread context (the same rule as the timer toasts and the break toast).

use std::collections::HashSet;

use serde::Serialize;
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

use crate::app_state::AppState;

/// Label prefix for an alarm toast window: `alarm-toast-<alarm id>`.
pub const ALARM_TOAST_PREFIX: &str = "alarm-toast-";

/// The window label for a given alarm id.
fn label_for(id: &str) -> String {
    format!("{ALARM_TOAST_PREFIX}{id}")
}

/// The alarm id encoded in a toast window label, if it is one.
pub fn id_from_label(label: &str) -> Option<&str> {
    label.strip_prefix(ALARM_TOAST_PREFIX)
}

/// One alarm toast the reconciler wants open, in stack order (index 0 nearest the tray).
#[derive(Debug, Clone, PartialEq, Eq)]
struct DesiredToast {
    id: String,
    label: String,
    name: String,
    time: String,
    recurrence: String,
}

/// Pure: map ordered `(id, name, time, recurrence)` tuples to desired toasts, preserving order.
fn desired_toasts(fired: &[(String, String, String, String)]) -> Vec<DesiredToast> {
    fired
        .iter()
        .map(|(id, name, time, recurrence)| DesiredToast {
            id: id.clone(),
            label: label_for(id),
            name: name.clone(),
            time: time.clone(),
            recurrence: recurrence.clone(),
        })
        .collect()
}

/// Data injected into a toast before its page loads.
#[derive(Serialize)]
struct ToastInfo<'a> {
    id: &'a str,
    name: &'a str,
    time: &'a str,
    recurrence: &'a str,
}

/// Reconcile the open alarm toasts with the pending fired-alarm set: create toasts for newly fired
/// alarms, close toasts whose entry was dismissed, then re-stack. Idempotent; safe every tick.
pub fn sync(app: &AppHandle) {
    let st = app.state::<AppState>();

    // Snapshot the fired set (lock released), ordered oldest-first by fire instant for a stable stack.
    let mut fired: Vec<(String, String, String, String, std::time::Instant)> = {
        let map = st.fired_alarm_toasts.lock().unwrap();
        map.iter()
            .map(|(id, f)| {
                (
                    id.clone(),
                    f.name.clone(),
                    f.time.clone(),
                    f.recurrence.to_string(),
                    f.fired_at,
                )
            })
            .collect()
    };
    fired.sort_by_key(|(_, _, _, _, at)| *at);
    let ordered: Vec<(String, String, String, String)> = fired
        .into_iter()
        .map(|(id, name, time, rec, _)| (id, name, time, rec))
        .collect();

    let desired = desired_toasts(&ordered);

    // Early-out: desired label set vs actually-open alarm toast windows (recomputed from live
    // windows each tick so a transient creation failure self-corrects next tick).
    let desired_labels: HashSet<String> = desired.iter().map(|d| d.label.clone()).collect();
    let actual_labels: HashSet<String> = app
        .webview_windows()
        .into_keys()
        .filter(|l| l.starts_with(ALARM_TOAST_PREFIX))
        .collect();
    if desired_labels == actual_labels {
        return;
    }

    // Native window ops must run on the main thread.
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        // Close alarm toasts no longer desired (dismissed).
        let actual: Vec<String> = app
            .webview_windows()
            .into_keys()
            .filter(|l| l.starts_with(ALARM_TOAST_PREFIX))
            .collect();
        for label in &actual {
            if !desired.iter().any(|d| &d.label == label) {
                if let Some(w) = app.get_webview_window(label) {
                    let _ = w.close();
                }
            }
        }
        // Create toasts for newly-fired alarms.
        for d in &desired {
            if app.get_webview_window(&d.label).is_none() {
                build_toast(&app, d);
            }
        }
        // Stack them bottom-right in desired order (index 0 nearest the tray).
        relayout(&app, &desired);
    });
}

/// Build one alarm toast window (hidden; `relayout` positions then shows it). Mirrors the timer
/// toast's flags: frameless, always-on-top, never focus-stealing, off the taskbar.
fn build_toast(app: &AppHandle, d: &DesiredToast) {
    let json = serde_json::to_string(&ToastInfo {
        id: &d.id,
        name: &d.name,
        time: &d.time,
        recurrence: &d.recurrence,
    })
    .unwrap_or_else(|_| "null".into());
    let init = format!(
        "{}{}",
        crate::webview::guarded_init("__GOMAJU_ALARM_TOAST__", &json),
        crate::webview::locale_init(&crate::i18n::current_locale(app)),
    );
    match WebviewWindowBuilder::new(app, &d.label, WebviewUrl::App("alarm-toast.html".into()))
        .title("Gomaju")
        .decorations(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false) // never steal focus from the user's work
        .inner_size(300.0, 84.0)
        .visible(false)
        .initialization_script(&init)
        .build()
    {
        Ok(_) => crate::rlog!("gomaju: alarm toast opened ({})", d.name),
        Err(e) => crate::rlog!("gomaju: failed to create alarm toast: {e}"),
    }
}

/// Stack the desired toasts at the bottom-right of the primary monitor's work area, growing upward
/// in `desired` order (index 0 nearest the tray). Positions then shows each window.
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

    fn t(id: &str, name: &str, time: &str, rec: &str) -> (String, String, String, String) {
        (id.into(), name.into(), time.into(), rec.into())
    }

    #[test]
    fn desired_maps_fields_and_label() {
        let d = desired_toasts(&[t("a1", "Wake up", "07:30", "daily")]);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].label, "alarm-toast-a1");
        assert_eq!(d[0].id, "a1");
        assert_eq!(d[0].name, "Wake up");
        assert_eq!(d[0].time, "07:30");
        assert_eq!(d[0].recurrence, "daily");
    }

    #[test]
    fn desired_preserves_order() {
        let d = desired_toasts(&[t("a", "A", "07:00", "once"), t("b", "B", "08:00", "weekly")]);
        let labels: Vec<&str> = d.iter().map(|x| x.label.as_str()).collect();
        assert_eq!(labels, ["alarm-toast-a", "alarm-toast-b"]);
    }

    #[test]
    fn id_round_trips_through_label() {
        assert_eq!(id_from_label("alarm-toast-xyz"), Some("xyz"));
        assert_eq!(id_from_label("timer-done-xyz"), None);
    }
}
