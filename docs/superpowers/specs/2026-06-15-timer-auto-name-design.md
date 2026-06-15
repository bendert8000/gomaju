# Timer enhancements: auto-name + count-up mode

- **Date:** 2026-06-15
- **Status:** Approved (design)
- **Area:** Countdown timers (`crates/gomaju-core`, `src-tauri`, `src`)

## Summary

Two related timer changes:

1. **Auto-name (locale-aware).** Remove the user-editable timer **name**. A timer is identified
   solely by its duration, and its display name is **computed** from the duration + the current
   locale: `"{clock} {word}"` — English `"02:30 timer"`, 繁體中文 `"02:30 計時器"`. The clock is
   `mm:ss`, or `h:mm:ss` once the duration reaches an hour (`"1:00:00 timer"`), matching the app's
   existing clock convention. Because the name is computed (never stored), it follows the active
   locale automatically.

2. **Count-up mode (global).** A new global setting in the Timers card switches all timers between
   **countdown** (default — counts the configured duration down to zero) and **count-up** (counts
   from zero up to the configured duration). This is a **display-only** transform: the engine and
   the moment a timer fires are unchanged; only the live number shown flips from *remaining* to
   *elapsed*.

## Current behavior (baseline)

- `CountdownDto` (`crates/gomaju-core/src/countdown.rs`) has a `pub name: String`, persisted in
  `config.toml` under `[[countdowns]]`. The 7 seeded presets in `default_config.toml` are named
  `"1m"`, `"3m"`, `"5m"`, `"10m"`, `"15m"`, `"30m"`, `"1h"`.
- The name is consumed in three places:
  - Fire **notification** body — `countdown.rs::spawn_scheduler` passes `def.name` as the body
    (title is `notif.timer_title`).
  - **"Time's up!" toast** name — same scheduler, `to_finish.push((id, def.name))`.
  - **Running toast** name — `timer_toast::sync` reads `c.name`.
- The **Timers window** (`src/timers.ts` / `timers.html`) renders an editable `.timer-name`
  `<input>`; `collectTimers` reads it (defaulting to `timers.default_name`); add-timer seeds
  `timers.new_name`.

## Desired behavior

- No editable name anywhere. The Timers window shows **no** name field (the hh:mm:ss duration
  editor is the timer's identity).
- The notification body and both toasts show the computed `"{clock} {word}"` name in the active
  locale.
- Clock format: `mm:ss` zero-padded for `< 1h` (`90s → "01:30"`), `h:mm:ss` with non-padded hours
  for `>= 1h` (`3600s → "1:00:00"`, `5400s → "1:30:00"`). Matches the existing `fmtClock` in
  `src/timers.ts` and the toast's `fmt`.

## Design

### Data model — drop `name` from `CountdownDto`

- Remove `pub name: String` from `CountdownDto` (`crates/gomaju-core/src/countdown.rs`). The struct
  becomes `{ id, duration_secs, chime_id, chime_volume_pct }`.
- **Backward compatibility:** `CountdownDto` has no `#[serde(deny_unknown_fields)]`, so a leftover
  `name = "…"` line in an existing `config.toml` is ignored on load and dropped on the next save. No
  `CONFIG_VERSION` bump, no explicit migration. (Verify during implementation that neither
  `CountdownDto` nor any countdown-bearing container uses `deny_unknown_fields`.)
- Users who customized a timer name lose that string and get the auto-name instead — intended.
- Remove the `name = "…"` line from each of the 7 `[[countdowns]]` blocks in `default_config.toml`.
- `sanitize_countdowns` needs no logic change (it never touched `name`); only its test helper drops
  the field.

### Name computation — backend only

The Timers window no longer displays a name, so name formatting lives **only** in the backend (the
two consumers — notification and toasts — are both driven from Rust). No TypeScript name logic.

- **Core (pure):** `pub fn format_clock(duration_secs: u32) -> String` in
  `crates/gomaju-core/src/countdown.rs` → `"02:30"` / `"1:00:00"`. Dependency-free, unit-tested in
  the core crate.
- **Host:** `timer_display_name(duration_secs: u32, locale: &str) -> String` (in
  `src-tauri/src/countdown.rs`) = `format!("{} {}", gomaju_core::countdown::format_clock(secs),
  crate::i18n::tr(locale, "timers.timer_word"))`. Word order `{clock} {word}` works for both target
  locales.
- **i18n (Rust):** add `"timers.timer_word" => pick(locale, "timer", "計時器")` to
  `src-tauri/src/i18n.rs::tr`.
- **Call sites:**
  - `countdown.rs::spawn_scheduler`: the notification body and the `to_finish` entry both use
    `timer_display_name(def.duration_secs, &locale)` instead of `def.name`. (`locale` is already
    snapshotted in step 1.)
  - `timer_toast::sync`: `sync` computes only the **running** toast name. Replace `c.name` with
    `timer_display_name(c.duration_secs, &locale)`, where `locale` comes from
    `crate::i18n::current_locale(app)` (already used in `build_toast`). The finished set keeps the
    name captured by the scheduler at fire time (already `timer_display_name`), so a finished toast
    shows whatever locale was active when it fired — acceptable for a terminal notice; the running
    path recomputes live each tick.

> Note: `finished_toasts` still stores a `name: String` (captured at fire time). That captured value
> is now the computed `timer_display_name`, not a user string. No structural change to
> `finished_toasts`.

### Frontend — remove the name field

`src/timers.ts`:
- Drop `name` from the `CountdownDto` interface.
- Remove the `.timer-name` `<input>` from the `timerRow` template and the
  `q(".timer-name").value = v.def.name` assignment.
- Remove `name` from the object built in `collectTimers`.
- Remove `name` from the add-timer default def.

`src/i18n.ts`: remove the now-unused keys `timers.name_ph`, `timers.default_name`, `timers.new_name`.

(No change to `src/main.ts`. No change to the toast frontend — it still receives a ready `name`.)

### Tests

- **Core:** unit tests for `format_clock`: `90 → "01:30"`, `150 → "02:30"`, `3600 → "1:00:00"`,
  `5400 → "1:30:00"`, `1 → "00:01"`. Adjust the `sanitize_countdowns` test `cd()` helper to drop
  `name`.
- **Host:** unit test for `timer_display_name`: `(150, "en") → "02:30 timer"`,
  `(150, "zh-Hant") → "02:30 計時器"`, `(3600, "en") → "1:00:00 timer"`.
- Existing core/app tests stay green (notably the toast `desired_toasts` tests are unaffected).

### Docs

Update the Timers section of `CLAUDE.md`: a countdown is a reusable **duration** preset (no name);
its display name is auto-derived as `"{mm:ss|h:mm:ss} {timer-word}"` per locale (`format_clock` +
`timer_display_name`), used in the notification and toasts; the Timers window has no name field.

## Design — count-up mode

### Setting (global)

- Add `pub timer_count_up: bool` to `Settings` (`crates/gomaju-core/src/config.rs`), default
  `false`, mirroring `show_timer_toasts` (same `serde` default behavior, so existing `config.toml`
  files without it load as `false`). No `CONFIG_VERSION` bump.
- Seed `timer_count_up = false` in `default_config.toml` (next to `show_timer_toasts`).
- **Timers settings card** (`index.html`): add a labeled `<select id="timer-mode">` with two options
  — Countdown (default) / Count up — below the `show-timer-toasts` checkbox. `src/main.ts` maps it
  to/from the bool: `value = cfg.settings.timer_count_up ? "countup" : "countdown"` on load,
  `timer_count_up: sel.value === "countup"` on save.
- i18n (`src/i18n.ts`): `settings.timer_mode_label`, `settings.timer_mode_countdown`,
  `settings.timer_mode_countup`.

### Engine: unchanged

`CountdownRun`, `start`/`pause`/`reset`/`remaining_secs`, the scheduler, and the fire instant are
**not** touched. Count-up is purely a presentation transform: `elapsed = duration_secs −
remaining_secs` (both already available wherever the live value is shown).

### Display changes

- **Running toast** (`src-tauri/src/timer_toast.rs` + `src/timer-toast.ts`): the running-toast
  `DesiredToast`/`ToastInfo` gain `count_up: bool` and `duration_secs: u32` (finished toasts ignore
  them). `sync` already needs each timer's `duration_secs` (to compute the auto-name), so its
  per-timer tuple carries `duration_secs`; it also reads `settings.timer_count_up`. In
  `timer-toast.ts`, the running branch: when `count_up`, start the local readout at
  `duration_secs − remaining_secs` and **increment** each second toward `duration_secs` (capped);
  otherwise decrement toward 0 as today. The finished ("Time's up!") toast shows no live number and
  is unaffected.
- **Timers window readout** (`src/timers.ts`): `CountdownView` gains `count_up: bool`. The
  `.timer-remaining` readout shows `fmtClock(count_up ? def.duration_secs − remaining_secs :
  remaining_secs)` while running/paused (still blank when idle). The window picks up a mode change on
  its next 1s poll.

### Backend plumbing

- `CountdownView` (`src-tauri/src/commands.rs`) gains `count_up: bool`, set from
  `cfg.settings.timer_count_up` in both `cmd_get_countdowns` and `cmd_save_countdowns` (each reads
  the setting once and stamps it on every view).
- The running-toast injection in `timer_toast::sync`/`build_toast` adds `count_up` +
  `duration_secs` to `ToastInfo`.

### Tests (count-up)

- The transform is `duration − remaining` (trivial); covered by reasoning + the existing
  `remaining_secs` tests. No new pure helper. Primary verification is manual (watch a timer count
  up, confirm it still fires at the configured time).

## Out of scope

- No change to the toast windows' capability or `cmd_dismiss_timer_done` (the prior feature); the
  only toast change is the injected `ToastInfo` (adds `count_up` + `duration_secs`).
- No `CONFIG_VERSION` bump or migration code.
- No localized name in the Timers window (it has no name field at all).
- No change to break-rule or alarm names (those keep their editable names).
- Count-up is a single **global** setting, not per-timer; the engine/firing is not changed (display
  transform only).

## Verification

- `cargo test -p gomaju-core` (format_clock + sanitize) and `cargo test -p gomaju`
  (timer_display_name).
- `cargo clippy --workspace --all-targets`.
- `npm run build`.
- Manual (`npm run tauri dev`):
  - **Auto-name:** Timers window shows no name field; start a 2:30 timer with toasts off →
    "Time's up!" toast and the notification both read "02:30 timer" (or "02:30 計時器" after a
    language switch); with toasts on, the running toast's label reads "02:30 timer".
  - **Count-up:** set the Timers card mode to Count up; start a 2:30 timer → the running toast and
    the Timers-window readout count **up** from 00:00 toward 02:30, and the timer still fires at the
    configured time (notification + "Time's up!"/close). Switch back to Countdown → counts down from
    02:30 to 00:00 as before. Default (fresh/old config) is Countdown.
