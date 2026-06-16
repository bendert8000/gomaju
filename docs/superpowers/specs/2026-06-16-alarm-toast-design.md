# Persistent alarm toast — design spec

**Date:** 2026-06-16
**Status:** Approved (design), pending implementation

## Problem

When a wall-clock alarm fires, gomaju shows a **native OS notification**
(`alarm.rs` → `runtime::show_notification` → `tauri-plugin-notification`). Native
notifications are owned by the OS: they auto-dismiss (Windows shunts them to the Action
Center after a few seconds; Linux DEs apply their own timeout), and the plugin exposes no
control over lifetime or rich content. So the alarm message vanishes on its own, and it
can't carry the alarm's schedule.

The user wants the alarm message to:

1. **Stay on screen until explicitly dismissed** with an ✕.
2. **Show the alarm's time info** — its scheduled time and recurrence.

## Decision

Replace the native notification with a **custom in-app webview toast**, modeled directly
on the existing **timer-done toast** (`timer_toast.rs` / `timer-toast.html`), which already
solves "persistent until ✕" with a background-thread reconciler. This also makes the
behavior identical across Windows/macOS/Linux — it no longer depends on the OS notification
daemon.

Confirmed choices:

| Decision | Choice |
| --- | --- |
| Content | Alarm **name** + **`{time} · {recurrence}`** (e.g. `07:30 · Daily`) |
| Native OS notification | **Replaced** by the custom toast (no duplicate) |
| When shown | **Always on fire**, independent of the `notifications` setting (like the alarm tone and the timer-done toast) |
| Lifetime | Persists until the user clicks ✕ |
| Scope | All recurrences (Once/Daily/Weekly/Bi-weekly/Monthly/Yearly) |
| Tone | Unchanged (`audio::play_alarm_chime`, once per minute) |

## Architecture

A self-contained family mirroring `timer-done-<id>`, in its own `alarm-toast.rs`.

### Window
- **Label:** `alarm-toast-<alarm_id>` (one per fired alarm).
- **Page:** `alarm-toast.html` + `src/alarm-toast.ts`, reusing the existing `.timer-toast`
  CSS layout (brand tag; a row of icon · name · time · ✕; a subtitle note).
- **Flags:** frameless, always-on-top, `skip_taskbar`, `focused(false)` (never steals
  focus), `visible_on_all_workspaces`, fixed inner size (300×84, the note-sized variant) —
  same flags as the timer toast.
- **Injected data** (`__GOMAJU_ALARM_TOAST__`): `{ id, name, time, recurrence }` where
  `time` is the stored `"HH:MM"` and `recurrence` is the lowercase repeat key
  (`"once"|"daily"|"weekly"|"biweekly"|"monthly"|"yearly"`).
- **Rendering (`alarm-toast.ts`):** row shows `⏰`, the name, and the time in the
  clock slot; the note subtitle shows the localized recurrence via
  `t("alarms.repeat_" + recurrence)` — **reusing the alarms-window labels** already in
  `src/i18n.ts` (`alarms.repeat_*`), so no new strings. The ✕ calls
  `cmd_dismiss_alarm_toast`. No countdown, no progress bar, no chime call (the scheduler
  owns the tone).

### State
- `AppState.fired_alarm_toasts: Mutex<HashMap<String, FiredAlarmToast>>`, in-memory only
  (reset on cold start), mirroring `finished_toasts`.
- `FiredAlarmToast { name: String, time: String, recurrence: &'static str, fired_at: Instant }`.
  Captured **at fire time** so editing/deleting the alarm afterward doesn't disturb a toast
  already on screen. `fired_at` gives a stable stacking order.

### Reconciler (`alarm_toast::sync(app)`)
- Builds the **desired** set from `fired_alarm_toasts` (sorted by `fired_at`), diffs it
  against the live `alarm-toast-*` windows, and creates/closes only the difference, then
  re-stacks bottom-right — structurally identical to `timer_toast::sync` (cheap label-set
  early-out; all native window ops inside one `run_on_main_thread`).
- **Driven from the alarm scheduler's 1s background thread**, never from a command — the
  same rule that avoids the WebView2 main-thread deadlock. `sync` is called once near the
  top of every scheduler iteration (so a dismissal closes its window within ~1s) and once
  more immediately after a fire (so the toast appears with the chime).
- A pure helper `desired_alarm_toasts(&[(id, name, time, recurrence)]) -> Vec<DesiredAlarmToast>`
  does label generation + field mapping (unit-tested); `sync` only supplies the ordered
  input and performs the window I/O.

### Fire path (`alarm.rs`)
On each due alarm, instead of `runtime::show_notification(...)`:
- insert/refresh `fired_alarm_toasts[id] = FiredAlarmToast { name, time, recurrence, fired_at }`,
- keep the existing `disable_once` and tone behavior unchanged.
A re-fire of a recurring alarm just refreshes the entry (same id → same window).

### Dismiss (`cmd_dismiss_alarm_toast`)
- Gated by `require_alarm_toast` (label starts with `alarm-toast-`), like
  `require_timer_done`.
- Removes the id (derived from the window's **own** label, not a spoofable arg) from
  `fired_alarm_toasts`. It does **not** close the window — the next scheduler tick does
  (avoiding the command-thread window-op deadlock).

### Plumbing
- `vite.config.ts`: add `alarmToast: "alarm-toast.html"` input.
- `capabilities/overlay.json`: add `"alarm-toast-*"` to `windows` (empty permissions, like
  the other toasts).
- `lib.rs`: `mod alarm_toast;`, register `cmd_dismiss_alarm_toast`, and init
  `fired_alarm_toasts` in the real `AppState` construction; `app_state.rs` test-state too.

## Stacking caveat (accepted for v1)

Alarm toasts and timer-done toasts both anchor bottom-right but are reconciled by separate
loops, so if both are on screen at the same instant they could overlap. Rare in practice;
ship as-is and unify positioning later if it bites.

## Testing

- `desired_alarm_toasts` + `id_from_label` round-trip: pure unit tests (label generation,
  field mapping, order preservation).
- Engine and alarm-recurrence logic are untouched — their tests are unaffected.
- The toast visuals + dismissal are a manual check (fire an alarm, confirm it persists, the
  time/recurrence read correctly, and ✕ closes it within ~1s).

## Out of scope (YAGNI)

- No click-to-open-alarms action (✕ only).
- No cross-family stacking coordination with timer toasts.
- No per-alarm "snooze" from the toast.
- No new recurrence detail (weekday list, date) — just the recurrence label.
