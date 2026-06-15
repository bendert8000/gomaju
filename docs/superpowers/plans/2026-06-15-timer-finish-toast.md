# Timer "Time's up!" toast + Timers settings card — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `settings.show_timer_toasts` mean "countdown toast while running" when checked and "a persistent *Time's up!* toast on finish" when unchecked, and group it into a dedicated Timers settings card.

**Architecture:** A new `timer-done-<id>` window family (separate from the running `timer-toast-<id>` family) carries the "Time's up!" toast. Its pending set lives in a new in-memory `AppState.finished_toasts: Mutex<HashMap<id,name>>`, populated by the countdown scheduler when a timer fires in unchecked mode, and reconciled (alongside running toasts) by `timer_toast::sync` on the scheduler's ~250 ms background tick — so all window creation stays off the main-thread WebView2 IPC path. A new `cmd_dismiss_timer_done` only mutates state; the next tick closes the window.

**Tech Stack:** Rust (Tauri v2), TypeScript/HTML/CSS frontend, `cargo` workspace (`gomaju` = `src-tauri` crate, `gomaju-core` = engine).

**Spec:** `docs/superpowers/specs/2026-06-15-timer-finish-toast-design.md`

**Conventions used below:**
- Compile the app crate: `cargo build -p gomaju` (compiles only; never *run* a plain build — see CLAUDE.md).
- Run app-crate unit tests: `cargo test -p gomaju`.
- Lint: `cargo clippy --workspace --all-targets`.
- Frontend type-check: `npm run build` (runs `tsc` then `vite build`).
- Commit after each task. We are on branch `main` with a large in-progress working tree; create a feature branch in Task 0 so these commits are isolated.

---

### Task 0: Create a feature branch

**Files:** none (git only)

- [ ] **Step 1: Branch off main**

Run:
```bash
git checkout -b feat/timer-finish-toast
```
Expected: `Switched to a new branch 'feat/timer-finish-toast'`

(The working tree already has uncommitted timers work; leave it staged-as-is. New commits below are scoped to the files each task names.)

---

### Task 1: Add the `finished_toasts` state field

**Files:**
- Modify: `src-tauri/src/app_state.rs` (struct field + `test_state` initializer)
- Modify: `src-tauri/src/lib.rs:166-180` (the `app.manage(AppState { ... })` block)

- [ ] **Step 1: Add the struct field**

In `src-tauri/src/app_state.rs`, add this field to `struct AppState` immediately after the
`countdown_runtime` field (keep its doc comment):

```rust
    /// Pending "Time's up!" toasts (countdown id -> timer name captured at fire time). Populated by
    /// the countdown scheduler when a timer fires while `show_timer_toasts` is OFF; reconciled into
    /// `timer-done-<id>` windows by `timer_toast::sync`; cleared by `cmd_dismiss_timer_done` (the ✕)
    /// and self-pruned to config-member ids by `sync`. In-memory only, like `countdown_runtime`.
    pub finished_toasts: Mutex<HashMap<String, String>>,
```

- [ ] **Step 2: Initialize it in `test_state`**

In `src-tauri/src/app_state.rs`, inside `fn test_state(...)`, add after the `countdown_runtime` line:

```rust
            finished_toasts: Mutex::new(HashMap::new()),
```

- [ ] **Step 3: Initialize it in the real `AppState`**

In `src-tauri/src/lib.rs`, inside the `app.manage(AppState { ... })` block, add after the
`countdown_runtime: Mutex::new(Default::default()),` line:

```rust
                // "Time's up!" toasts are never persisted (cold start has none), like run state.
                finished_toasts: Mutex::new(Default::default()),
```

- [ ] **Step 4: Compile**

Run: `cargo build -p gomaju`
Expected: builds clean (the new field is unused for now — that's fine, it's `pub` so no dead-code warning).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/app_state.rs src-tauri/src/lib.rs
git commit -m "feat(timers): add finished_toasts state for time's-up toasts"
```

---

### Task 2: `timer_toast` — finished prefix, `ToastInfo.finished`, and the pure `desired_toasts` helper (TDD)

**Files:**
- Modify: `src-tauri/src/timer_toast.rs` (constants/helpers, `ToastInfo`, `desired_toasts` + `DesiredToast`, tests)

- [ ] **Step 1: Write the failing tests**

Add this test module at the bottom of `src-tauri/src/timer_toast.rs`:

```rust
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p gomaju --lib timer_toast`
Expected: FAIL — `cannot find function 'desired_toasts'` / `id_from_done_label` / no field `finished`.

- [ ] **Step 3: Add the prefix + helpers**

In `src-tauri/src/timer_toast.rs`, just below the existing `TIMER_TOAST_PREFIX` block (a
`label_for` already exists there — do **not** duplicate it), add:

```rust
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
```

> `ToastInfo` gains its `finished` field in Task 3 (where `build_toast` is rewritten to set it), so
> this task stays compilable on its own.

- [ ] **Step 4: Add `DesiredToast` + `desired_toasts`**

Add above `sync` in `src-tauri/src/timer_toast.rs`:

```rust
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
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p gomaju --lib timer_toast`
Expected: builds clean (this task does not touch `ToastInfo`/`build_toast`); the 4 new tests PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/timer_toast.rs
git commit -m "feat(timers): add timer-done prefix + pure desired_toasts helper"
```

---

### Task 3: `timer_toast::sync` reconciles both window families

**Files:**
- Modify: `src-tauri/src/timer_toast.rs` (`sync`, `build_toast`, `relayout`)

- [ ] **Step 1: Rewrite `sync`**

Replace the body of `pub fn sync(app: &AppHandle)` with:

```rust
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
```

- [ ] **Step 2: Add `finished` to `ToastInfo`, then rewrite `build_toast` to take a `DesiredToast`**

First add the `finished` field to the `ToastInfo` struct:

```rust
#[derive(Serialize)]
struct ToastInfo<'a> {
    id: &'a str,
    name: &'a str,
    remaining_secs: u32,
    finished: bool,
}
```

Then replace the `build_toast` signature + `ToastInfo` construction so it reads from `DesiredToast`:

```rust
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
```

- [ ] **Step 3: Update `relayout` to take `&[DesiredToast]`**

Change `relayout`'s signature and the lookup inside it:

```rust
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
```

- [ ] **Step 4: Verify build + all timer_toast tests pass**

Run: `cargo test -p gomaju --lib timer_toast`
Expected: builds clean; the 4 tests from Task 2 PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/timer_toast.rs
git commit -m "feat(timers): reconcile running + time's-up toast families in sync"
```

---

### Task 4: Scheduler records finished timers in unchecked mode

**Files:**
- Modify: `src-tauri/src/countdown.rs` (`spawn_scheduler` steps 1–3)

- [ ] **Step 1: Read `show_timer_toasts` in the step-1 snapshot**

In `spawn_scheduler`, change the step-1 snapshot tuple from `(defs, locale, notify)` to also carry
the setting:

```rust
        let (defs, locale, notify, show_toasts): (HashMap<String, CountdownDto>, String, bool, bool) = {
            let st = app.state::<AppState>();
            let cfg = st.config.lock().unwrap();
            let defs = cfg
                .countdowns
                .iter()
                .map(|c| (c.id.clone(), c.clone()))
                .collect();
            (defs, cfg.locale.clone(), cfg.settings.notifications, cfg.settings.show_timer_toasts)
        };
```

- [ ] **Step 2: Collect `(id, name)` of due timers to "finish" (unchecked mode only)**

In step 2, declare a `to_finish` vec next to `fired`, and push to it inside the `for id in due` loop
when the def exists and the setting is off. The loop body becomes:

```rust
        let mut fired: Vec<(String, String, u8)> = Vec::new(); // (name, chime_id, volume)
        let mut to_finish: Vec<(String, String)> = Vec::new(); // (id, name) for "time's up" toasts
        {
            let st = app.state::<AppState>();
            let mut map = st.countdown_runtime.lock().unwrap();
            let due: Vec<String> = map
                .iter()
                .filter_map(|(id, run)| match run {
                    CountdownRun::Running { finish_at } if *finish_at <= now => Some(id.clone()),
                    _ => None,
                })
                .collect();
            for id in due {
                if let Some(def) = defs.get(&id) {
                    fired.push((def.name.clone(), def.chime_id.clone(), def.chime_volume_pct));
                    if !show_toasts {
                        to_finish.push((id.clone(), def.name.clone()));
                    }
                }
                map.remove(&id);
            }
        }
```

- [ ] **Step 3: Insert into `finished_toasts` in step 3, before the `sync` call**

In step 3, after the existing fired-side-effects block and **before** the
`crate::timer_toast::sync(&app);` line, add:

```rust
        // Record "time's up" toasts (unchecked mode). Own lock, no nesting — config -> runtime ->
        // finished_toasts. The sync() call below (same tick) creates the windows off the main thread.
        if !to_finish.is_empty() {
            let st = app.state::<AppState>();
            let mut fin = st.finished_toasts.lock().unwrap();
            for (id, name) in to_finish {
                fin.insert(id, name); // one toast per id; a re-fire just refreshes the entry
            }
        }
```

- [ ] **Step 4: Compile + run existing countdown tests**

Run: `cargo test -p gomaju --lib countdown`
Expected: builds clean; existing transition tests still PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/countdown.rs
git commit -m "feat(timers): scheduler records time's-up toasts when toasts are off"
```

---

### Task 5: `cmd_dismiss_timer_done` + gate + save-time prune (TDD for the gate)

**Files:**
- Modify: `src-tauri/src/commands.rs` (gate helper, gate test, dismiss command, `cmd_save_countdowns`)
- Modify: `src-tauri/src/lib.rs:252-...` (register the command)

- [ ] **Step 1: Write the failing gate test**

In `src-tauri/src/commands.rs`, add `is_timer_done` to the test module's `use super::{...}` import
list, then add this test inside `mod tests`:

```rust
    #[test]
    fn only_a_timer_done_window_may_dismiss() {
        assert!(is_timer_done("timer-done-abc"));
        // The running-toast family and every other window must be rejected.
        assert!(!is_timer_done("timer-toast-abc"));
        assert!(!is_timer_done("timers"));
        assert!(!is_timer_done("settings"));
        assert!(!is_timer_done(""));
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p gomaju --lib commands::tests::only_a_timer_done_window_may_dismiss`
Expected: FAIL — `cannot find function 'is_timer_done' in this scope`.

- [ ] **Step 3: Add `is_timer_done` + `require_timer_done`**

In `src-tauri/src/commands.rs`, right after the existing `require_timer_toast` function, add:

```rust
/// True for a finished-timer "time's up" toast window (`timer-done-<id>`).
fn is_timer_done(label: &str) -> bool {
    label.starts_with(crate::timer_toast::TIMER_DONE_PREFIX)
}

/// Reject the dismiss command invoked from any window other than a timer-done toast window.
fn require_timer_done(window: &WebviewWindow) -> Result<(), String> {
    gate(is_timer_done(window.label()), "timer-done")
}
```

- [ ] **Step 4: Run the gate test to verify it passes**

Run: `cargo test -p gomaju --lib commands::tests::only_a_timer_done_window_may_dismiss`
Expected: PASS.

- [ ] **Step 5: Add the dismiss command**

In `src-tauri/src/commands.rs`, right after `cmd_toast_stop_countdown`, add:

```rust
/// The ✕ on a finished-timer "time's up" toast: drop its entry so the scheduler's next reconcile
/// tick closes the window. The id comes from the toast's **own** window label (no spoofable arg);
/// we never create or close windows from this command (that would risk the WebView2 main-thread
/// deadlock) — only mutate state.
#[tauri::command]
pub fn cmd_dismiss_timer_done(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require_timer_done(&window)?;
    if let Some(id) = crate::timer_toast::id_from_done_label(window.label()) {
        state.finished_toasts.lock().unwrap().remove(id);
    }
    Ok(())
}
```

- [ ] **Step 6: Prune `finished_toasts` in `cmd_save_countdowns`**

In `cmd_save_countdowns`, after `with_config_write` returns `config` and **before** the existing
`let mut map = state.countdown_runtime.lock().unwrap();` line, add a separate lock scope:

```rust
    // Drop any pending "time's up" toast for a timer that was just deleted (separate lock scope —
    // never held with the runtime lock). sync() also prunes by config membership; this is immediate.
    {
        let valid: std::collections::HashSet<&str> =
            config.countdowns.iter().map(|c| c.id.as_str()).collect();
        state
            .finished_toasts
            .lock()
            .unwrap()
            .retain(|id, _| valid.contains(id.as_str()));
    }
```

- [ ] **Step 7: Register the command**

In `src-tauri/src/lib.rs`, in the `tauri::generate_handler![ ... ]` list, add after
`commands::cmd_toast_stop_countdown,`:

```rust
            commands::cmd_dismiss_timer_done,
```

- [ ] **Step 8: Compile + run command tests**

Run: `cargo test -p gomaju --lib commands`
Expected: builds clean; all gate tests (including the new one) PASS.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(timers): add cmd_dismiss_timer_done + prune finished toasts on save"
```

---

### Task 6: Grant the capability for `timer-done-*` windows

**Files:**
- Modify: `src-tauri/capabilities/overlay.json`

- [ ] **Step 1: Add the window pattern**

In `src-tauri/capabilities/overlay.json`, add `"timer-done-*"` to the `windows` array and mention
it in the description. The file becomes:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "overlay",
  "description": "Capability for break overlays and reminder toasts (incl. per-timer running toasts timer-toast-* and finished 'time's up' toasts timer-done-*). Empty permission set: these windows get no core API access. App-defined commands are not capability-gated, so they can still invoke their narrow commands; the Rust-side caller-label checks block every settings-only command.",
  "windows": ["overlay-*", "warning-toast", "pause-toast", "timer-toast-*", "timer-done-*"],
  "permissions": []
}
```

- [ ] **Step 2: Verify it still compiles (the schema is consumed by tauri-build)**

Run: `cargo build -p gomaju`
Expected: builds clean (a malformed capability JSON fails the build).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/capabilities/overlay.json
git commit -m "feat(timers): allow timer-done-* toast windows in overlay capability"
```

---

### Task 7: Frontend — render the "Time's up!" toast

**Files:**
- Modify: `timer-toast.html` (give the icon span an id)
- Modify: `src/timer-toast.ts` (read `finished`, branch the rendering + ✕ command)

- [ ] **Step 1: Add an id to the icon span**

In `timer-toast.html`, change the icon span to have `id="icon"`:

```html
      <span class="timer-toast__icon" id="icon" aria-hidden="true">⏳</span>
```

- [ ] **Step 2: Branch the renderer on `finished`**

Replace the contents of `src/timer-toast.ts` with:

```ts
import { invoke } from "@tauri-apps/api/core";
import { t } from "./i18n";
import { readInjected } from "./util";

// Injected by timer_toast.rs::build_toast before the page loads.
interface ToastInfo {
  id: string;
  name: string;
  remaining_secs: number;
  finished: boolean;
}

const info = readInjected<ToastInfo>("__GOMAJU_TIMER_TOAST__", {
  id: "",
  name: "",
  remaining_secs: 0,
  finished: false,
});

const $ = (id: string): HTMLElement => document.getElementById(id) as HTMLElement;

/** Remaining as `mm:ss`, or `h:mm:ss` past an hour. */
function fmt(total: number): string {
  const secs = Math.max(0, Math.floor(total));
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  const p = (n: number): string => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${p(m)}:${p(s)}` : `${p(m)}:${p(s)}`;
}

window.addEventListener("DOMContentLoaded", () => {
  // This window's own label: running -> timer-toast-<id>, finished -> timer-done-<id>.
  const label = `${info.finished ? "timer-done-" : "timer-toast-"}${info.id}`;
  // Signal the page loaded (a useful "embedded assets actually loaded" trace).
  invoke("cmd_window_ready", { label }).catch(() => {});

  $("name").textContent = info.name;
  const stop = $("stop") as HTMLButtonElement;
  const time = $("time");

  if (info.finished) {
    // Terminal "Time's up!" toast: no countdown, the ✕ just dismisses (the id is derived from this
    // window's own label on the Rust side — no arg to spoof).
    $("icon").textContent = "⏰";
    time.textContent = t("timers.times_up");
    stop.title = t("timers.dismiss");
    stop.setAttribute("aria-label", t("timers.dismiss"));
    stop.addEventListener("click", () => {
      invoke("cmd_dismiss_timer_done").catch(() => {});
    });
    return;
  }

  // Running countdown toast.
  stop.title = t("timers.stop");
  stop.setAttribute("aria-label", t("timers.stop"));
  stop.addEventListener("click", () => {
    invoke("cmd_toast_stop_countdown").catch(() => {});
  });

  // Count down locally; the host closes this window on finish/stop.
  let remaining = info.remaining_secs;
  time.textContent = fmt(remaining);
  window.setInterval(() => {
    remaining = Math.max(0, remaining - 1);
    time.textContent = fmt(remaining);
  }, 1000);
});
```

- [ ] **Step 3: Type-check the frontend**

Run: `npm run build`
Expected: `tsc` passes (no type errors) and `vite build` writes `dist/`. (This requires the
`timers.times_up` / `timers.dismiss` i18n keys from Task 8 — if `tsc` errors on missing keys only
because `t()` is loosely typed it will still pass; if it fails, do Task 8 first then re-run.)

- [ ] **Step 4: Commit**

```bash
git add timer-toast.html src/timer-toast.ts
git commit -m "feat(timers): render time's-up toast variant in the toast window"
```

---

### Task 8: Settings UI — Timers card + i18n

**Files:**
- Modify: `index.html` (move the checkbox into a new Timers card)
- Modify: `src/i18n.ts` (heading, relabeled checkbox, hint, toast strings)

- [ ] **Step 1: Add the i18n keys**

In `src/i18n.ts`, replace the existing `settings.show_timer_toasts_label` entry with the relabeled
one plus a hint and heading (keep alphabetical-ish grouping near the other `settings.*` entries):

```ts
  "settings.timers_heading": { en: "Timers", "zh-Hant": "計時器" },
  "settings.show_timer_toasts_label": {
    en: "Show a live countdown toast while a timer runs",
    "zh-Hant": "計時器執行時顯示倒數提示窗",
  },
  "settings.show_timer_toasts_hint": {
    en: "When off, a \"Time's up!\" toast still appears when a timer finishes.",
    "zh-Hant": "關閉時，計時器結束仍會顯示「時間到！」提示窗。",
  },
```

Then add the two toast strings near the other `timers.*` entries (e.g. after `timers.stop`):

```ts
  "timers.times_up": { en: "Time's up!", "zh-Hant": "時間到！" },
  "timers.dismiss": { en: "Dismiss", "zh-Hant": "關閉" },
```

- [ ] **Step 2: Remove the checkbox from the general settings card**

In `index.html`, delete this block from the general settings card (currently around lines 84–87):

```html
        <label class="field field--checkbox">
          <input id="show-timer-toasts" type="checkbox" />
          <span data-i18n="settings.show_timer_toasts_label">Show a toast for each running timer</span>
        </label>
```

- [ ] **Step 3: Add the Timers card after the general settings card**

In `index.html`, immediately after the closing `</section>` of the general settings card (the one
that contains `#autostart`) and before the Quotes `<section class="card">`, insert:

```html
      <section class="card">
        <h2 data-i18n="settings.timers_heading">Timers</h2>
        <label class="field field--checkbox">
          <input id="show-timer-toasts" type="checkbox" />
          <span data-i18n="settings.show_timer_toasts_label">Show a live countdown toast while a timer runs</span>
        </label>
        <p class="muted" data-i18n="settings.show_timer_toasts_hint">When off, a "Time's up!" toast still appears when a timer finishes.</p>
      </section>
```

> `src/main.ts` needs **no** change: it references the checkbox by `id="show-timer-toasts"`
> (`inp("show-timer-toasts")` at load and save), which is unchanged by moving cards. Verify with a
> quick grep: `rg "show-timer-toasts" src/main.ts` should show the read (`.checked = ...`) and write
> (`show_timer_toasts: ...`) and nothing needs editing.

- [ ] **Step 4: Type-check + build the frontend**

Run: `npm run build`
Expected: `tsc` passes; `vite build` succeeds.

- [ ] **Step 5: Commit**

```bash
git add index.html src/i18n.ts
git commit -m "feat(timers): add Timers settings card + relabel toast toggle"
```

---

### Task 9: Docs, full build, lint, and manual verification

**Files:**
- Modify: `CLAUDE.md` (Timers section)

- [ ] **Step 1: Update CLAUDE.md**

In `CLAUDE.md`, in the "## Timers (countdown timers...)" section, update the
**Running-timer toasts** bullet to describe the two-mode behavior and the finished family. Replace
the first sentence of that bullet (the part describing the `show_timer_toasts` gate) so the bullet
reads (keep the rest of the bullet about the scheduler tick / deadlock intact):

```markdown
- **Timer toasts** (`src-tauri/src/timer_toast.rs`, `timer-toast.html` / `src/timer-toast.ts`):
  `settings.show_timer_toasts` (default on) now selects **which** toast, not whether there is one.
  **Checked** → one small frameless, always-on-top, non-focus-stealing **countdown** toast per
  **running** timer (windows `timer-toast-<id>`), stacked bottom-right, closed at 00:00. **Unchecked**
  → no countdown toast, but when a timer fires a **"Time's up!"** toast (windows `timer-done-<id>`,
  separate prefix) appears and **stays until the user clicks ✕** (independent of `settings.notifications`;
  one toast per timer id). Both families (capability `timer-toast-*` / `timer-done-*` in `overlay.json`)
  are reconciled by the single idempotent `timer_toast::sync(app)` — desired (running toasts if the
  setting is on, plus pending "time's up" toasts pruned to config-member ids) vs the actual windows;
  it creates/closes the diff and re-stacks (finished above running), with a cheap label-set early-out.
  The pending "time's up" set is `AppState.finished_toasts: Mutex<HashMap<id,name>>` (in-memory,
  reset on restart), filled by the scheduler when a timer fires while the setting is off, and cleared
  by `cmd_dismiss_timer_done` (the ✕, id from the window's own `timer-done-` label) — the next tick
  closes the window. **sync is driven by the countdown scheduler's ~250 ms background tick, NOT from
  commands** — load-bearing: creating a webview window from a command (main thread inside a WebView2
  IPC callback) deadlocks on Windows. The toggle lives in its own **Timers** card in Settings.
```

> If the existing bullet wording differs, preserve the load-bearing Windows-deadlock paragraph and
> the "not listed in the tray" note; only fold in the two-mode behavior + `timer-done-*` +
> `finished_toasts` + `cmd_dismiss_timer_done` + the Timers settings card.

- [ ] **Step 2: Full workspace test**

Run: `cargo test -p gomaju-core && cargo test -p gomaju`
Expected: all PASS (core unchanged; app crate includes the 5 new tests).

- [ ] **Step 3: Lint**

Run: `cargo clippy --workspace --all-targets`
Expected: no new warnings in the files touched.

- [ ] **Step 4: Build the app for real (standalone runnable binary)**

Run: `cargo build --release --features custom-protocol` (and `npm run build` first if not already).
Expected: builds; binary at `target/release/gomaju.exe`.

- [ ] **Step 5: Manual verification (dev)**

Run: `npm run tauri dev` (optionally `GOMAJU_OPEN_TIMERS=1 npm run tauri dev`). Verify:
  - Settings shows a **Timers** card with the relabeled checkbox + hint.
  - **Checked**: start a short timer → countdown toast appears, closes at 00:00. No "Time's up!" toast.
  - **Uncheck** the setting (Save). Start a short timer → **no** toast while running; at 00:00 a
    **"Time's up!"** toast (⏰ + name) appears and **persists** until you click ✕.
  - Start two short timers while unchecked → two stacked "Time's up!" toasts; ✕ each independently.
  - Re-fire the same timer while its "Time's up!" toast lingers → still one toast for that timer.
  - Delete a timer (Timers window) whose "Time's up!" toast is open → toast closes within ~250 ms.
  - Toggle the setting from unchecked→checked while a timer runs → running toast appears; a lingering
    "Time's up!" toast stays until ✕. No stale "Time's up!" painted over a live countdown.
  - Confirm no hang/freeze on Windows when starting/finishing/dismissing (the deadlock guard).

- [ ] **Step 6: Commit**

```bash
git add CLAUDE.md
git commit -m "docs(timers): document two-mode timer toasts + Timers settings card"
```

---

## Self-review notes (already reconciled against the spec)

- **Spec coverage:** finished_toasts (T1), timer-done family + desired_toasts + sync (T2/T3),
  scheduler hook (T4), cmd_dismiss_timer_done + gate + save-prune (T5), overlay capability (T6),
  frontend finished render (T7), Timers card + i18n (T8), CLAUDE.md + verification (T9). The
  resurrection-race fix is the `sync` config-membership prune (T3) plus the save-time prune (T5).
- **Lock order:** every new lock is acquire-use-release; scheduler is config→runtime→finished
  (T4), `sync` is config→runtime→finished (T3), save prunes finished in a scope separate from the
  runtime lock (T5), dismiss locks only finished (T5). No nesting anywhere.
- **Windows safety:** no `timer-done-*` window is created/closed from any command — only from
  `sync` on the background tick (T3/T5).
- **Type consistency:** `DesiredToast { id, label, name, remaining_secs, finished }`,
  `ToastInfo { id, name, remaining_secs, finished }`, `TIMER_DONE_PREFIX`, `id_from_done_label`,
  `is_timer_done` / `require_timer_done`, `cmd_dismiss_timer_done`, `finished_toasts` — names used
  identically across tasks and the frontend (`finished` flag, `timer-done-`/`timer-toast-` label).
