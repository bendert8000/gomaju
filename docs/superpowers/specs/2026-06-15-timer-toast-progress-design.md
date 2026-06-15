# Progress bar on timer toasts

- **Date:** 2026-06-15
- **Status:** Approved (design)
- **Area:** Countdown timers — running on-screen toast (`src-tauri`, `src`, `crates/gomaju-core`)

## Summary

Add a thin progress bar to each **running** timer toast, gated by a new global setting in the Timers
settings card (**default on**). The bar **mirrors the displayed value, following the Timer direction
setting**: counting **up** it fills from empty (`elapsed/duration`); counting **down** it starts full
and drains (`remaining/duration`). It reuses the styling of the app's existing pre-break warning-toast
bar (`.toast__bar`). The terminal "Time's up!" toast has no bar.

> **Revision (2026-06-15):** the bar originally always filled with `elapsed/duration`
> (mode-independent). It now follows the Timer direction — fill when counting up, drain from full when
> counting down — so the bar and the on-screen number always represent the same quantity.

## Current state (baseline)

- The running timer toast (`timer-toast.html` / `src/timer-toast.ts`, windows `timer-toast-<id>`) is
  a single row: icon, name, live time, ✕. It counts locally each second (down to 0, or up to
  `duration_secs` in count-up mode). Injected `ToastInfo = {id, name, remaining_secs, finished,
  count_up, duration_secs}` (built by `timer_toast.rs::build_toast` from a `DesiredToast`).
- `styles.css` already has a `.toast__bar` (4px track) + `.toast__bar-fill` (`width:0`,
  `transition: width 1s linear`) used by the pre-break warning toast — the styling precedent.
- The Timers settings card (`index.html`) already holds `show-timer-toasts` + the `timer-mode`
  select; `crates/gomaju-core/src/config.rs` `Settings` uses per-field serde defaults incl. a
  `default_true` helper; `default_config.toml` seeds each setting.

## Desired behavior

- A 4px progress bar at the bottom of each **running** toast, shown only when the new setting is on.
- The bar fill width = `shown / duration_secs`, where `shown` is the same value the toast displays:
  **elapsed** when counting up (`duration_secs − remaining_secs`; fills 0→full) and **remaining** when
  counting down (starts full, drains to 0). Updated each second, animating smoothly
  (`transition: width 1s linear`); the initial paint skips the intro animation so a countdown bar
  starts full instead of filling up first.
- The "Time's up!" (finished) toast has no bar.

## Design

### Setting (global)

- Add `pub timer_toast_progress: bool` to `Settings` (`crates/gomaju-core/src/config.rs`) with
  `#[serde(default = "default_true")]`, and `timer_toast_progress: true` in the `Default` impl.
- Seed `timer_toast_progress = true` in `default_config.toml` (next to `show_timer_toasts`).
- **Timers settings card** (`index.html`): a checkbox below the `timer-mode` select:
  `#timer-toast-progress` with label `settings.timer_toast_progress_label`.
- `src/main.ts`: `SettingsDto.timer_toast_progress: boolean`; load
  `inp("timer-toast-progress").checked = cfg.settings.timer_toast_progress;`; save
  `timer_toast_progress: inp("timer-toast-progress").checked,`.
- i18n (`src/i18n.ts`): `settings.timer_toast_progress_label` (en/zh-Hant).
- No `CONFIG_VERSION` bump (additive field with a serde default; old configs default to on).

### Backend (inject the flag)

- `ToastInfo` (`src-tauri/src/timer_toast.rs`) gains `progress: bool`.
- `DesiredToast` gains `progress: bool`. In `desired_toasts`, running toasts get the passed-in
  `progress`; finished toasts get `false`.
- `desired_toasts` signature gains a `progress: bool` parameter (alongside `count_up`); `sync` reads
  `cfg.settings.timer_toast_progress` (add to its config-block tuple) and passes it.
- `build_toast` injects `progress: d.progress` into `ToastInfo`.
- No `CountdownView` change — the Timers window shows no bar.

### Frontend (render the bar)

- `timer-toast.html`: after `<div class="timer-toast__row">…</div>`, add:
  `<div class="timer-toast__bar" id="bar-track"><div class="timer-toast__bar-fill" id="bar"></div></div>`.
- `styles.css`: add `.timer-toast__bar` (height 4px, rounded, faint track, `overflow:hidden`) and
  `.timer-toast__bar-fill` (height 100%, `width:0`, accent background, `transition: width 1s linear`)
  — mirroring `.toast__bar` / `.toast__bar-fill`.
- `src/timer-toast.ts`:
  - Add `progress: boolean` to the `ToastInfo` interface + `readInjected` default (`false`).
  - **Finished branch:** hide the bar track (`$("bar-track").hidden = true`) — terminal toast, no bar.
  - **Running branch:** set `barTrack.hidden = !info.progress`. Track a single `shown` value (the same
    value the toast displays): `count_up ? duration − remaining` (counts up) `: remaining` (counts
    down). `setBar(shown)` sets the fill width to `shown/duration` (guarded `duration > 0`, clamped
    100%). Paint the initial bar with `transition: none` (then restore) so a countdown bar starts full
    instead of animating up from empty. The 1s interval moves `shown` (+1 up / −1 down) and re-sets
    the time text + bar; the CSS transition animates the fill between ticks.
  - The bar mirrors the on-screen number: **fills** when counting up, **drains from full** when
    counting down (follows the Timer direction setting).
- Toast window stays `300×64`; a 4px bar + small gap fits within the existing column padding (verify
  during implementation; only bump `inner_size` height if it visibly clips).

### Tests

- Extend the `desired_toasts` unit test (in `timer_toast.rs`) to assert `progress` propagates to
  running toasts (true when passed) and is `false` on finished toasts — mirroring the `count_up`
  assertions. Update the test helper/call sites for the new `progress` parameter.
- Manual verification: the bar fills smoothly and reaches full as the timer fires; hidden when the
  setting is off and on the "Time's up!" toast; works in both count-up and countdown modes.

### Docs

Update the Timers-toast bullet in `CLAUDE.md`: the running toast injects `{…,progress}` and shows a
4px bar (gated by `settings.timer_toast_progress`, default on) that mirrors the on-screen number —
fills when counting up, drains from full when counting down; the toggle lives in the Timers settings
card.

## Out of scope

- No bar on the "Time's up!" toast or in the Timers window.
- No per-timer progress setting (one global toggle).
- No `CONFIG_VERSION` bump / migration.
- No change to the count-up/auto-name behavior (this composes with them).

## Verification

- `cargo test -p gomaju-core` + `cargo test -p gomaju` (the extended `desired_toasts` test).
- `cargo clippy --workspace --all-targets`; `npm run build`.
- Manual (`npm run tauri dev`): start a short timer with toasts on → a bar fills toward full as it
  counts (down or up) and the toast closes/fires at full; toggle the setting off → no bar; the
  "Time's up!" toast shows no bar.
