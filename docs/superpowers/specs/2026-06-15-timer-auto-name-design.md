# Auto-named, locale-aware timers

- **Date:** 2026-06-15
- **Status:** Approved (design)
- **Area:** Countdown timers (`crates/gomaju-core`, `src-tauri`, `src`)

## Summary

Remove the user-editable timer **name**. A timer is now identified solely by its duration, and its
display name is **computed** from the duration + the current locale: `"{clock} {word}"` —
English `"02:30 timer"`, 繁體中文 `"02:30 計時器"`. The clock is `mm:ss`, or `h:mm:ss` once the
duration reaches an hour (`"1:00:00 timer"`), matching the app's existing clock convention. Because
the name is computed (never stored), it follows the active locale automatically.

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

## Out of scope

- No change to the toast windows / capability / `cmd_dismiss_timer_done` (the prior feature).
- No `CONFIG_VERSION` bump or migration code.
- No localized name in the Timers window (it has no name field at all).
- No change to break-rule or alarm names (those keep their editable names).

## Verification

- `cargo test -p gomaju-core` (format_clock + sanitize) and `cargo test -p gomaju`
  (timer_display_name).
- `cargo clippy --workspace --all-targets`.
- `npm run build`.
- Manual (`npm run tauri dev`): Timers window shows no name field; start a 2:30 timer with toasts
  off → "Time's up!" toast and the notification both read "02:30 timer" (or "02:30 計時器" after a
  language switch); with toasts on, the running toast's label reads "02:30 timer".
