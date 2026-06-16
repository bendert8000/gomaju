# Persistent Alarm Toast — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the alarm's auto-dismissing native OS notification with a persistent in-app toast (one per fired alarm) that shows name + `{time} · {recurrence}` and stays until the user clicks ✕.

**Architecture:** A new `alarm-toast-<id>` window family modeled on the timer-done toast: an in-memory `fired_alarm_toasts` set filled by the alarm scheduler, reconciled into windows by `alarm_toast::sync` driven from the scheduler's 1s background thread (never from a command — that would deadlock WebView2 on the main thread), dismissed by `cmd_dismiss_alarm_toast`.

**Tech Stack:** Rust (Tauri v2), TypeScript/HTML, existing `.timer-toast` CSS, `tauri-plugin-notification` (now bypassed for alarms).

**Spec:** `docs/superpowers/specs/2026-06-16-alarm-toast-design.md`

---

### Task 1: Backend state — `FiredAlarmToast` + `fired_alarm_toasts`

**Files:**
- Modify: `src-tauri/src/app_state.rs` (struct + field + test-state init)
- Modify: `src-tauri/src/lib.rs` (real `AppState` construction)

- [ ] **Step 1: Add the struct + field** in `app_state.rs` (mirror `FinishedToast` / `finished_toasts`). `Instant` is already imported there.

```rust
/// A fired alarm awaiting its `alarm-toast-<id>` toast: the alarm's name, scheduled "HH:MM"
/// time, and recurrence key (lowercase, matches the frontend `alarms.repeat_*` labels), plus
/// the fire instant for a stable stack order. Captured at fire time, so editing/deleting the
/// alarm afterward doesn't disturb a toast already on screen.
#[derive(Debug, Clone)]
pub struct FiredAlarmToast {
    pub name: String,
    pub time: String,
    pub recurrence: &'static str,
    pub fired_at: Instant,
}
```

Add to `AppState`:

```rust
    /// Pending alarm toasts (alarm id -> name/time/recurrence + fire instant). Filled by the alarm
    /// scheduler on every fire; reconciled into `alarm-toast-<id>` windows by `alarm_toast::sync`.
    /// Cleared by `cmd_dismiss_alarm_toast` (the ✕). In-memory only, like `finished_toasts`.
    pub fired_alarm_toasts: Mutex<HashMap<String, FiredAlarmToast>>,
```

- [ ] **Step 2: Init in the test-state** (`app_state.rs`, `test_state`) — add `fired_alarm_toasts: Mutex::new(HashMap::new()),`.

- [ ] **Step 3: Init in the real construction** (`lib.rs`, where `AppState { ... }` is built) — add `fired_alarm_toasts: Mutex::new(std::collections::HashMap::new()),` (match the existing import style; `HashMap` is already in scope there if `finished_toasts` is).

- [ ] **Step 4: Verify it compiles**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: Finishes with no errors.

---

### Task 2: Reconciler module `alarm_toast.rs` (TDD on the pure helper)

**Files:**
- Create: `src-tauri/src/alarm_toast.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod alarm_toast;`)

- [ ] **Step 1: Create `alarm_toast.rs`** with the full module (pure `desired_toasts` + tests, `sync`, `build_toast`, `relayout`, label helpers):

```rust
//! Persistent on-screen toasts for fired wall-clock alarms — small always-on-top windows stacked
//! bottom-right above the tray, one `alarm-toast-<alarm id>` per fired alarm. Unlike a native OS
//! notification (auto-dismissed by the OS), this toast stays until the user clicks ✕. It shows the
//! alarm's name plus its time and recurrence. The pending set lives in
//! [`AppState::fired_alarm_toasts`] (filled by the alarm scheduler on every fire, cleared by
//! `cmd_dismiss_alarm_toast`); [`sync`] reconciles it into windows.
//!
//! `sync` runs from the **alarm scheduler's 1s background thread**, NOT the dismiss command:
//! creating/closing a webview window from a command runs on the main thread inside a WebView2 IPC
//! callback and deadlocks on Windows. The background thread keeps window ops in a clean main-thread
//! context (same rule as the timer toasts and the break toast).

use std::collections::HashSet;

use serde::Serialize;
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

use crate::app_state::AppState;

/// Label prefix for an alarm toast window: `alarm-toast-<alarm id>`.
pub const ALARM_TOAST_PREFIX: &str = "alarm-toast-";

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

/// Pure: map ordered (id, name, time, recurrence) tuples to desired toasts, preserving order.
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

#[derive(Serialize)]
struct ToastInfo<'a> {
    id: &'a str,
    name: &'a str,
    time: &'a str,
    recurrence: &'a str,
}

/// Reconcile open alarm toasts with the pending fired-alarm set. Idempotent.
pub fn sync(app: &AppHandle) {
    let st = app.state::<AppState>();

    // Snapshot (released), ordered oldest-first by fire instant for a stable stack.
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

    let desired_labels: HashSet<String> = desired.iter().map(|d| d.label.clone()).collect();
    let actual_labels: HashSet<String> = app
        .webview_windows()
        .into_keys()
        .filter(|l| l.starts_with(ALARM_TOAST_PREFIX))
        .collect();
    if desired_labels == actual_labels {
        return;
    }

    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
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
        for d in &desired {
            if app.get_webview_window(&d.label).is_none() {
                build_toast(&app, d);
            }
        }
        relayout(&app, &desired);
    });
}

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
        .focused(false)
        .inner_size(300.0, 84.0)
        .visible(false)
        .initialization_script(&init)
        .build()
    {
        Ok(_) => crate::rlog!("gomaju: alarm toast opened ({})", d.name),
        Err(e) => crate::rlog!("gomaju: failed to create alarm toast: {e}"),
    }
}

/// Stack the desired toasts bottom-right, growing upward in `desired` order.
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
```

- [ ] **Step 2: Register the module** in `lib.rs` — add `mod alarm_toast;` next to `mod timer_toast;` (or the other `mod` lines).

- [ ] **Step 3: Run the pure tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml alarm_toast`
Expected: 3 tests pass (`desired_maps_fields_and_label`, `desired_preserves_order`, `id_round_trips_through_label`).

---

### Task 3: Dismiss command + gate

**Files:**
- Modify: `src-tauri/src/commands.rs` (gate + command)
- Modify: `src-tauri/src/lib.rs` (register in `invoke_handler`)

- [ ] **Step 1: Add gate + command** in `commands.rs` (mirror `is_timer_done` / `require_timer_done` / `cmd_dismiss_timer_done`):

```rust
/// True for an alarm toast window (`alarm-toast-<id>`).
fn is_alarm_toast(label: &str) -> bool {
    label.starts_with(crate::alarm_toast::ALARM_TOAST_PREFIX)
}

/// Reject the dismiss command invoked from any window other than an alarm toast window.
fn require_alarm_toast(window: &WebviewWindow) -> Result<(), String> {
    gate(is_alarm_toast(window.label()), "alarm-toast")
}

/// The ✕ on an alarm toast: drop its entry so the scheduler's next reconcile tick closes the
/// window. The id comes from the toast's **own** window label (no spoofable arg); we never close
/// windows from this command (that would risk the WebView2 main-thread deadlock) — only mutate state.
#[tauri::command]
pub fn cmd_dismiss_alarm_toast(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require_alarm_toast(&window)?;
    if let Some(id) = crate::alarm_toast::id_from_label(window.label()) {
        state.fired_alarm_toasts.lock().unwrap().remove(id);
    }
    Ok(())
}
```

- [ ] **Step 2: Register** in `lib.rs` `invoke_handler` list — add `commands::cmd_dismiss_alarm_toast,` next to `commands::cmd_dismiss_timer_done,`.

- [ ] **Step 3: Verify it compiles**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: no errors.

---

### Task 4: Fire path — replace the native notification, drive sync

**Files:**
- Modify: `src-tauri/src/alarm.rs` (`spawn_scheduler` loop + a `recurrence_key` helper)

- [ ] **Step 1: Add the recurrence-key helper** in `alarm.rs`:

```rust
/// The lowercase recurrence key for the toast, matching the frontend `alarms.repeat_*` labels.
fn recurrence_key(repeat: RepeatDto) -> &'static str {
    match repeat {
        RepeatDto::Once => "once",
        RepeatDto::Daily => "daily",
        RepeatDto::Weekly => "weekly",
        RepeatDto::Biweekly => "biweekly",
        RepeatDto::Monthly => "monthly",
        RepeatDto::Yearly => "yearly",
    }
}
```

- [ ] **Step 2: Replace the notification call** in the fire loop. Change:

```rust
                crate::rlog!("gomaju: alarm fired ({})", a.name);
                runtime::show_notification(&app, crate::i18n::tr(&locale, "notif.alarm_title"), &a.name);
```

to:

```rust
                crate::rlog!("gomaju: alarm fired ({})", a.name);
                {
                    let st = app.state::<AppState>();
                    st.fired_alarm_toasts.lock().unwrap().insert(
                        a.id.clone(),
                        crate::app_state::FiredAlarmToast {
                            name: a.name.clone(),
                            time: a.time.clone(),
                            recurrence: recurrence_key(a.repeat),
                            fired_at: std::time::Instant::now(),
                        },
                    );
                }
```

(`runtime` may now be unused in `alarm.rs` — if the compiler warns, drop `runtime` from the `use crate::{audio, runtime};` import, leaving `use crate::audio;`. `locale` is still used for the chime dir/logging path; if it becomes unused, prefix it `_locale` or drop it.)

- [ ] **Step 3: Drive `sync`** — add a call at the **top of the loop body** (right after `std::thread::sleep(Duration::from_secs(1));`) so a dismissal closes within ~1s and a fire from the previous second shows promptly:

```rust
            std::thread::sleep(Duration::from_secs(1));
            crate::alarm_toast::sync(&app);
```

And add one more `crate::alarm_toast::sync(&app);` immediately after the chime block (end of the minute-change processing) so a just-fired toast appears with the chime rather than ~1s later.

- [ ] **Step 4: Verify it compiles (no warnings)**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: no errors, no unused-import warnings.

---

### Task 5: Frontend — toast page + plumbing

**Files:**
- Create: `alarm-toast.html` (repo root)
- Create: `src/alarm-toast.ts`
- Modify: `vite.config.ts` (rollup input)
- Modify: `src-tauri/capabilities/overlay.json` (window glob)

- [ ] **Step 1: Create `alarm-toast.html`** (reuse the `.timer-toast` layout: row with icon · name · time · ✕, plus the note subtitle for the recurrence):

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Gomaju</title>
    <link rel="stylesheet" href="/src/styles.css" />
  </head>
  <body class="timer-toast">
    <div class="brand-tag">Gomaju</div>
    <div class="timer-toast__row">
      <span class="timer-toast__icon" id="icon" aria-hidden="true">⏰</span>
      <span class="timer-toast__name" id="name"></span>
      <span class="timer-toast__time" id="time"></span>
      <button class="timer-toast__stop" id="stop" type="button">✕</button>
    </div>
    <div class="timer-toast__note" id="note"></div>
    <script type="module" src="/src/alarm-toast.ts"></script>
  </body>
</html>
```

- [ ] **Step 2: Create `src/alarm-toast.ts`**:

```ts
import { invoke } from "@tauri-apps/api/core";
import { t } from "./i18n";
import { readInjected } from "./util";

// Injected by alarm_toast.rs::build_toast before the page loads.
interface AlarmToastInfo {
  id: string;
  name: string;
  time: string; // "HH:MM"
  recurrence: string; // "once" | "daily" | "weekly" | "biweekly" | "monthly" | "yearly"
}

const info = readInjected<AlarmToastInfo>("__GOMAJU_ALARM_TOAST__", {
  id: "",
  name: "",
  time: "",
  recurrence: "daily",
});

const $ = (id: string): HTMLElement => document.getElementById(id) as HTMLElement;

window.addEventListener("DOMContentLoaded", () => {
  // The id is derived from this window's own label on the Rust side — no arg to spoof.
  invoke("cmd_window_ready", { label: `alarm-toast-${info.id}` }).catch(() => {});

  $("name").textContent = info.name;
  $("time").textContent = info.time;
  // Recurrence label reuses the alarms-window strings (alarms.repeat_*).
  $("note").textContent = t(`alarms.repeat_${info.recurrence}`);

  const stop = $("stop") as HTMLButtonElement;
  stop.title = t("timers.dismiss");
  stop.setAttribute("aria-label", t("timers.dismiss"));
  stop.addEventListener("click", () => {
    invoke("cmd_dismiss_alarm_toast").catch(() => {});
  });
});
```

- [ ] **Step 3: Add the Vite input** in `vite.config.ts` `rollupOptions.input`:

```ts
        timerToast: "timer-toast.html",
        alarmToast: "alarm-toast.html",
```

- [ ] **Step 4: Add the capability window glob** — in `src-tauri/capabilities/overlay.json`, add `"alarm-toast-*"` to the `windows` array (and mention it in the description):

```json
  "windows": ["overlay-*", "warning-toast", "pause-toast", "timer-toast-*", "timer-done-*", "alarm-toast-*"],
```

- [ ] **Step 5: Type-check the frontend**

Run: `npm run build`
Expected: tsc passes; `dist/` includes an `alarm-toast` asset and `alarm-toast.html`.

---

### Task 6: Build, test, manual verify

- [ ] **Step 1: Run the host tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: all pass (incl. the 3 new `alarm_toast` tests).

- [ ] **Step 2: Clippy**

Run: `cargo clippy --manifest-path src-tauri/Cargo.toml`
Expected: clean (no warnings on the new code).

- [ ] **Step 3: Build the runnable binary**

Run: `cargo build --release --features custom-protocol`
Expected: `target/release/gomaju.exe` rebuilt.

- [ ] **Step 4: Manual verify** (quit any running gomaju first): create a Daily alarm one minute out (or use the alarms window), let it fire. Confirm: a bottom-right toast shows the name, the time, and "Daily"; it **stays** (no auto-dismiss); ✕ closes it within ~1s; the alarm tone still plays; no native/PowerShell notification appears.

---

## Self-Review

- **Spec coverage:** persistent-until-✕ (Tasks 2–4 reconciler + dismiss), time+recurrence content (Task 5 render), replaces native notification (Task 4 removes `show_notification`), always-on-fire/independent of `notifications` (Task 4 inserts unconditionally), all recurrences (Task 4 `recurrence_key` covers all six), tone unchanged (Task 4 leaves the chime block). ✓
- **Placeholders:** none — every step has concrete code/commands.
- **Type consistency:** `ALARM_TOAST_PREFIX`, `id_from_label`, `FiredAlarmToast{name,time,recurrence,fired_at}`, `__GOMAJU_ALARM_TOAST__`, `cmd_dismiss_alarm_toast` used identically across tasks. ✓
