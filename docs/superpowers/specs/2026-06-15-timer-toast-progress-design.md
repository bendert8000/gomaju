# Progress bar on timer toasts

- **Date:** 2026-06-15
- **Status:** Approved (design)
- **Area:** Countdown timers — running on-screen toast (`src-tauri`, `src`, `crates/gomaju-core`)

## Summary

Add a thin progress bar to each **running** timer toast, gated by a new global setting in the Timers
settings card (**default on**). The bar **fills 0→100% with elapsed/duration** — progress toward
firing — independent of the countdown/count-up direction, matching the app's existing pre-break
warning-toast bar (`.toast__bar`) and the universal "progress" convention. The terminal
"Time's up!" toast has no bar.

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
- The bar fill width = `elapsed / duration_secs` (elapsed = `duration_secs − remaining_secs`),
  updated each second, animating smoothly (`transition: width 1s linear`). At 100% the timer fires
  (and a countdown toast then closes). Mode-independent: count-up and countdown both fill toward
  completion.
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
  - **Running branch:** if `!info.progress`, hide the bar track. If on, define
    `const setBar = (elapsed) => { $("bar").style.width = duration > 0 ? `${Math.min(100, (elapsed/duration)*100)}%` : "0"; }`,
    call it with the initial elapsed, and call it again inside the existing 1s interval (both the
    count-up and countdown branches already compute the running value; compute `elapsed = count_up ?
    elapsed : duration − remaining`). The CSS transition animates the fill between ticks.
  - The bar reflects `elapsed/duration` in **both** modes (fills toward completion).
- Toast window stays `300×64`; a 4px bar + small gap fits within the existing column padding (verify
  during implementation; only bump `inner_size` height if it visibly clips).

### Tests

- Extend the `desired_toasts` unit test (in `timer_toast.rs`) to assert `progress` propagates to
  running toasts (true when passed) and is `false` on finished toasts — mirroring the `count_up`
  assertions. Update the test helper/call sites for the new `progress` parameter.
- Manual verification: the bar fills smoothly and reaches full as the timer fires; hidden when the
  setting is off and on the "Time's up!" toast; works in both count-up and countdown modes.

### Docs

Update the Timers-toast bullet in `CLAUDE.md`: the running toast injects
`{…,progress}` and shows a 4px fill bar (`elapsed/duration`) gated by `settings.timer_toast_progress`
(default on); the toggle lives in the Timers settings card.

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
