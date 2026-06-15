# Timer "Time's up!" toast + Timers settings card

- **Date:** 2026-06-15
- **Status:** Approved (design)
- **Area:** Countdown timers (`src-tauri`, `src`, `crates/gomaju-core` untouched)

## Summary

Redefine the existing `settings.show_timer_toasts` checkbox so that **unchecked** no longer
means "no timer toast at all". Instead:

- **Checked** — show a live countdown toast the whole time a timer runs (today's behavior,
  unchanged; the toast closes at `00:00`).
- **Unchecked** — do **not** show the running countdown toast, but when a timer fires
  ("time's up") show a small toast for that timer that **stays until the user clicks ✕**.

Also move this setting out of the general settings card into a dedicated **Timers** card in
the settings window (UI grouping only — the config stays flat as `settings.show_timer_toasts`).

## Current behavior (baseline)

- `settings.show_timer_toasts: bool` (default `true`), a checkbox in the general settings card
  of the settings window (`index.html` / `src/main.ts`).
- `ON` → `timer_toast::sync(app)` keeps one always-on-top, frameless, non-focus-stealing toast
  window per **running** timer (label `timer-toast-<id>`), stacked bottom-right, counting down
  locally in JS. Closed on finish/stop.
- `OFF` → no toast windows at all.
- `sync` is driven from the **countdown scheduler's ~250 ms background thread**
  (`countdown::spawn_scheduler`), never from commands. This is load-bearing: creating a webview
  window from a command runs on the main thread inside a WebView2 IPC callback and **deadlocks
  on Windows**. All `timer-toast-*` window create/close/relayout happens via
  `run_on_main_thread` inside `sync`, called from the background tick.
- A timer is one-shot: when it fires, the scheduler removes it from
  `AppState.countdown_runtime: Mutex<HashMap<id, CountdownRun>>`
  (`CountdownRun = Running{finish_at:Instant} | Paused{remaining}`; absent = idle). On fire it
  also shows a notification (gated by `settings.notifications`) + plays the chime (always).
- Lock order is always **config → runtime**. Run state is in-memory only, reset on cold start.

## Desired behavior

| Setting | While running | When the timer fires |
| --- | --- | --- |
| **Checked** (`true`) | live countdown toast per running timer | toast closes at `00:00` (unchanged) |
| **Unchecked** (`false`) | no toast | **"Time's up!"** toast appears, stays until ✕ |

- The "Time's up!" toast is **independent of `settings.notifications`** (that gate only controls
  the OS notification banner). It is purely a function of being in unchecked mode.
- **One "Time's up!" toast per timer id.** If a timer fires again while its "Time's up!" toast
  still lingers, that single toast is refreshed — no duplicate stacking.

## Design decisions

Chosen approach: **Approach A — a separate window family for finished toasts.**

### A separate `timer-done-<id>` window family (not a flag on `timer-toast-<id>`)

The "Time's up!" toast uses its own label prefix `timer-done-` so it can never collide with a
running countdown toast `timer-toast-<id>` for the same id. This matters because finished toasts
linger ("stay until ✕"): if the user re-checks the setting and restarts the same timer while its
"Time's up!" toast is still open, reusing `timer-toast-<id>` would paint "Time's up!" over a live
countdown. Two independent families, each reconciled against its own desired set, removes that
class of bug. (Codex review agreed the split is justified.)

### Finished-toast state lives in its own in-memory map

`AppState.finished_toasts: Mutex<HashMap<String, String>>` (countdown id → timer name captured at
fire time). In-memory only, like `countdown_runtime`, so cold start has none. The name is captured
at fire time so toast creation needs no config lookup. This map does **not** extend `CountdownRun`
— polluting the run-state enum with a `Finished` variant would leak into `state_str`, the timers
window poll, and the "absent = idle, never persisted" model. A separate map keeps the existing
run-state semantics intact.

### `sync` filters the finished set by current-config membership (self-healing)

Each tick, the finished desired set = entries of `finished_toasts` whose id is **still present in
the current config** (in config order). Computed fresh every tick. This makes the reconcile
self-healing and resolves a race Codex found: if `cmd_save_countdowns` deletes a timer at the same
instant the scheduler inserts that timer's finished entry, the next tick simply does not display it
(and it is pruned), so a deleted timer can never resurrect a "Time's up!" toast. Deleting a timer
therefore also clears its lingering finished toast — the intended product behavior.

### Early-out: set-equality over both prefixes (self-correcting)

The reconcile keeps the existing early-out shape: compare the **set** of desired toast labels
against the **set** of actually-open windows whose label starts with `timer-toast-` *or*
`timer-done-`, recomputed from the live window list every tick. If equal, return before the
main-thread hop. This is deliberately recomputed from live windows (not a cached signature) so a
transient window-creation failure self-corrects on the next tick — a cache would mark the work
"done" and never retry. Labels alone encode finished-ness (the two prefixes differ), so the set
comparison distinguishes the families.

> **Documented cosmetic / positional edges (accepted, same as today's running-toast path):**
> - Renaming a timer *while* its "Time's up!" toast is open and re-firing before dismissal: the
>   visible name lags until dismissed (the toast injects a static name at creation).
> - Reordering timers without changing the set of open toasts: stack positions are not re-laid-out
>   until the open set next changes.
> These match the existing running-toast behavior and are not specially handled.

### Lock ordering

Global order extends to **config → runtime → finished_toasts**, and every lock is
acquire-use-release — never held across another acquisition. Specifically:

- `countdown::spawn_scheduler` step-1 locks config (release), step-2 locks runtime (release),
  step-3 locks chimes (release) then `finished_toasts` (release). No nesting.
- `timer_toast::sync` locks config (release) → runtime (release) → `finished_toasts` (release).
- `cmd_save_countdowns` retains `countdown_runtime` and `finished_toasts` in **separate** lock
  scopes (never holds `runtime` while taking `finished_toasts`).
- `cmd_dismiss_timer_done` locks only `finished_toasts`.

### All window ops stay on the scheduler tick (Windows safety)

No `timer-done-*` window is created or closed from a command. `cmd_dismiss_timer_done` only mutates
`finished_toasts`; the next ~250 ms tick closes the window via `sync`/`run_on_main_thread`. This
mirrors `cmd_toast_stop_countdown` and preserves the WebView2 main-thread deadlock invariant.

> **Accepted ≤250 ms races (declined to special-case):**
> - Dismiss → window stays open until the next tick. If the same timer re-fires within that window,
>   the existing window may serve as the refreshed toast without a rebuild. Benign.
> - The setting is read at the scheduler snapshot, microseconds before the fire decision (the same
>   staleness the existing `notifications` gate has). Read as close to the fire decision as
>   practical; not treated as a correctness bug.

### Pile-up: no cap

Finished toasts stack deterministically bottom-up in config order and are each dismissed with ✕.
No cap and no "dismiss all" affordance (matches the explicit "stay until ✕" choice; many timers
firing unchecked without any dismissal is rare for a personal timer app).

## Architecture / file-by-file changes

### Rust — `src-tauri`

1. **`app_state.rs`**
   - Add field `finished_toasts: Mutex<HashMap<String, String>>`.
   - Initialize it in `test_state` (`Mutex::new(HashMap::new())`).

2. **`lib.rs`**
   - Initialize `finished_toasts` where `AppState` is constructed / managed.
   - Register `cmd_dismiss_timer_done` in the `invoke_handler` list.

3. **`countdown.rs` (`spawn_scheduler`)**
   - Step-1 config snapshot also reads `cfg.settings.show_timer_toasts`.
   - Step-2 (under the runtime lock, id known): for each **due** timer whose def still exists,
     if `!show_timer_toasts`, collect `(id, name)` into a local `to_finish` vec.
   - Step-3 (after locks released): if `!to_finish.is_empty()`, lock `finished_toasts` alone and
     insert each `(id, name)` (overwrite = one toast per id).

4. **`timer_toast.rs`**
   - Add `pub const TIMER_DONE_PREFIX: &str = "timer-done-";`, `done_label_for(id)`, and
     `id_from_done_label(label) -> Option<&str>` (separate from `id_from_label`).
   - `ToastInfo` gains `finished: bool`.
   - Pure helper (unit-tested):
     `desired_toasts(show_running: bool, running_order: &[(id,name,remaining)], finished_order: &[(id,name)]) -> Vec<DesiredToast>`
     where `DesiredToast { label, name, remaining_secs, finished }`. Running entries included only
     when `show_running`; finished entries always (caller passes only config-member finished ids).
     **Order within the returned list:** running entries (config order) first, then finished
     entries (config order). With `relayout` stacking index 0 nearest the tray, this means finished
     toasts stack above running ones in the rare both-present (toggled-mid-run) case — deterministic.
   - `sync`: read config (order + `show_timer_toasts`), runtime (running set), `finished_toasts`
     (prune to config-member ids, then take them in config order); build the combined desired list;
     early-out on desired-label-**set** vs the set of actually-open `timer-toast-*` + `timer-done-*`
     windows; on the main thread close non-desired windows of **both** families, create missing ones
     with the right label + injected `finished`, then relayout all in desired order.
   - `build_toast` takes a `finished` flag → picks label (`timer-toast-` vs `timer-done-`) and sets
     `ToastInfo.finished`.

5. **`commands.rs`**
   - Add `require_timer_done(window)` gate (`timer-done-` prefix), mirroring `require_timer_toast`.
   - Add `cmd_dismiss_timer_done(window, state)`: `require_timer_done`, derive id from the window's
     own label via `id_from_done_label`, remove it from `finished_toasts`.
   - `cmd_save_countdowns`: after the existing `countdown_runtime.retain(valid)` scope (closed),
     open a **separate** scope locking `finished_toasts` and `retain(|id, _| valid.contains(id))`.

6. **`capabilities/overlay.json`**
   - Add `"timer-done-*"` to the `windows` array; update the description to mention it.

### Frontend — `src`

7. **`timer-toast.html` / `src/timer-toast.ts`**
   - `ToastInfo` interface gains `finished: boolean`.
   - On load, if `finished`: set the icon to ⏰, set the message element to `t("timers.times_up")`,
     do **not** start the 1 s countdown interval, and wire ✕ → `invoke("cmd_dismiss_timer_done")`.
   - Else: unchanged (countdown interval, ✕ → `cmd_toast_stop_countdown`).

8. **`index.html`**
   - New `<section class="card">` with `<h2 data-i18n="settings.timers_heading">Timers</h2>`,
     containing the `#show-timer-toasts` checkbox (moved out of the general settings card) plus a
     muted hint paragraph (`settings.show_timer_toasts_hint`). Placed after the general settings
     card, before the Quotes card.

9. **`src/i18n.ts`**
   - `settings.timers_heading` — "Timers" / "計時器".
   - Update `settings.show_timer_toasts_label` — e.g. "Show a live countdown toast while a timer
     runs" / Chinese equivalent.
   - `settings.show_timer_toasts_hint` — "When off, a 'Time's up!' toast still appears when a timer
     finishes." / Chinese equivalent.
   - `timers.times_up` — "Time's up!" / "時間到！".

   (`src/main.ts` needs no logic change — the checkbox id `show-timer-toasts` is unchanged; it just
   lives in a different card.)

### Tests

10. **`timer_toast.rs`** — unit tests for `desired_toasts`:
    - `show_running=false` → no running toasts, finished toasts present.
    - `show_running=true` → running toasts present, in config order.
    - finished entries always present and after/with running entries in deterministic order.
    - a finished id not in the running set coexists with running toasts (the toggle-mid-run case).
11. **`commands.rs`** — gate test that `require_timer_done` accepts `timer-done-x` and rejects
    `timer-toast-x` / `settings` / others (mirror the existing `require_timer_toast` test).
12. Existing `countdown.rs` transition tests are untouched.

### Docs

13. **`CLAUDE.md`** — update the "Timers" section: document the two-mode `show_timer_toasts`
    behavior, the `timer-done-*` family, `finished_toasts`, `cmd_dismiss_timer_done`, and the new
    Timers settings card.

## Out of scope

- No config schema change / migration (the bool already exists; `CONFIG_VERSION` unchanged).
- No `[settings.timer]` nested TOML block — "Timers" is a UI card only.
- No cap / "dismiss all" for piled-up finished toasts.
- No change to the chime/notification behavior on fire.

## Verification

- `cargo test -p gomaju-core` (unchanged) and `cargo test` for the new `src-tauri` unit tests.
- `cargo clippy --workspace --all-targets`.
- Manual (`npm run tauri dev`), both modes:
  - **Checked**: start a short timer → countdown toast appears, closes at `00:00`.
  - **Unchecked**: start a short timer → no toast while running; at `00:00` a "Time's up!" toast
    appears and persists until ✕.
  - Toggle the setting mid-run; delete a timer with a lingering finished toast; restart a timer
    whose finished toast is still open — verify no stale/duplicated toasts.
  - `GOMAJU_OPEN_TIMERS=1` to open the timers window quickly during testing.
