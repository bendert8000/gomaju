# CLAUDE.md

Guidance for AI agents (and humans) working in this repo. Keep it short and current.

gomaju is a cross-platform, tray-resident break reminder built with **Tauri v2**
(Rust core + TypeScript/HTML/CSS UI). The dependency-free `gomaju-core` crate decides
*when* to break; the `src-tauri` layer turns those decisions into windows, sounds,
tray UI, and notifications.

## Build & run

| Goal | Command |
| --- | --- |
| Dev (hot reload, runs Vite dev server) | `npm run tauri dev` |
| Full release + installers | `npm run tauri build` |
| Quick runnable release binary (no installers) | `cargo build --release --features custom-protocol` |
| Core engine tests (fast, no Tauri) | `cargo test -p gomaju-core` |
| Lint | `cargo clippy --workspace --all-targets` |

### ⚠️ Never build a runnable app with plain `cargo build [--release]`

The frontend is loaded via `WebviewUrl::App(...)`, which resolves to either the dev
server (`devUrl` = `http://localhost:1420`) or the **embedded** assets. Tauri's build
script decides this:

```rust
let dev = !custom_protocol;   // dev mode UNLESS the `custom-protocol` feature is on
```

- `npm run tauri dev` / `npm run tauri build` toggle `custom-protocol` automatically.
- A bare `cargo build`/`cargo build --release` does **not** → the binary comes out in
  **dev mode** → every window (settings, overlays) tries to load from the Vite dev
  server. With no server running you get **`ERR_CONNECTION_REFUSED`** / a blank window.

So for a standalone binary, always pass `--features custom-protocol` (declared in
`src-tauri/Cargo.toml`). The release binary lands at `target/release/gomaju.exe`
(workspace target dir at the repo root, not under `src-tauri/`).

`npm run tauri build` also runs `npm run build` (`version:check && tsc && vite build`)
to refresh `dist/`. If you only `cargo build --features custom-protocol`, you reuse the
existing `dist/` — rebuild the frontend separately (`npm run build`) if `src/` changed.

Versioning: `package.json` is canonical. Use `npm run version:set -- 0.2.0` to update
`package.json`, `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml` together.

## Notifications (platform notes)

- Break/soft notifications use `tauri-plugin-notification` (`runtime::show_notification`).
- The **startup** "Running in the system tray" toast is special: `runtime::show_startup_notification`
  auto-dismisses after ~3s. It fires on every cold start (gated by the `notifications`
  setting) to remind the user the app keeps running in the tray after its windows close.
  The plugin exposes no control over toast lifetime, and a native Windows banner can't be
  shown for less than the OS minimum (~5s). So on Windows we drive the WinRT toast directly
  (`windows` crate) and call `ToastNotifier::Hide` after 3s, which clears both the banner and
  the Action Center entry. Other platforms (and any WinRT failure) fall back to the plugin.

## Alarms (clock alarms, separate from breaks)

- Wall-clock alarms (name + time + recurrence Once/Daily/Weekly/Bi-weekly/Monthly/Yearly)
  live in `config.toml` as `[[alarms]]`. They fire a notification + a distinct repeating
  tone (`audio::play_alarm`) regardless of run state.
- **Bi-weekly** reuses Weekly's `weekdays` plus the `date` field as a start-week anchor:
  fires the ticked days every *other* Monday-aligned week from that week, never before the
  start date. Week-parity is pure integer math — `days_from_civil` + `monday_week` in
  `alarm.rs` (chrono-free, unit-tested in isolation).
- The engine stays clock-free: recurrence is a pure, tested matcher in
  `crates/gomaju-core/src/alarm.rs` (`alarm_is_due` + `sanitize_alarms`); the firing
  loop is `src-tauri/src/alarm.rs::spawn_scheduler` — a 1s thread **edge-triggered on the
  wall minute** (fires once per matching minute; no catch-up for missed minutes; "once"
  alarms auto-disable + persist after firing).
- The **Alarms window** (`alarms.html` / `src/alarms.ts`, label `alarms`, opened from the
  tray) does CRUD via `cmd_get_alarms`/`cmd_save_alarms`/`cmd_close_alarms`, gated by
  `require_alarms`. Save is clone→sanitize→write→swap (never mutate cache before disk).

## Break rules (two editors, shared UI)

- Break rules live in **two** windows: **Settings** (`index.html` / `src/main.ts`, "Rules"
  card) is the full editor (shared `src/rule-editor.ts` grid). The **standalone Break-rules
  window** (`breaks.html` / `src/breaks.ts`, label `breaks`, tray "Breaks…"; window title is
  still "Gomaju — Break rules") is a
  **quick-select dashboard**: big read-only cards where only On/Off (tap the card) and
  Repeat/Once (segmented control) are editable; each toggle auto-saves via
  `cmd_set_rule_flags` (merge-by-id, so it never clobbers Settings detail edits) and
  reconfigures the engine live. "Edit in Settings…" → `cmd_open_settings`. The dashboard
  renders its own cards (does NOT use the shared `ruleRow`); it imports only the `RuleDto`
  type.
- The standalone window **auto-opens on every cold start** (`lib.rs` setup), alongside the
  startup "Running in the system tray" toast (see Notifications). Debug builds honor
  `GOMAJU_NO_OPEN_RULES` to suppress the auto-open.
- The **tray menu** lists each enabled break as a clickable status line (`🟢 ☕ {name} · {dur}`,
  soonest first). Clicking one prompts "take this break?" (`runtime::confirm_then_break_one`) and,
  on confirm, fires *that specific* rule's break immediately via `Engine::break_now_rule(rule_id)`
  (the per-rule sibling of `break_now`). The menu item carries id `break:<rule_id>`; the placeholder
  lines ("On a break now" / "No breaks enabled") stay non-actionable `status-{i}` items. The whole
  break list is rebuilt each tick only when a rendered line changes (`tray.rs` cache key).
- Each rule has a `repeat` flag (default true). A **once** rule (`repeat=false`) fires one
  break, then the engine disables it (`Effect::RuleDisabled`) and the host persists
  `enabled=false` (`runtime::persist_rule_disabled`) — same auto-disable model as alarm
  "Once"; re-check "On" to re-arm. **All** config writers go through
  `AppState::with_config_write` (`app_state.rs`), which clones → mutates → saves → swaps the
  cache under one held `config` lock, so the ticker's auto-disable can't clobber a concurrent
  window Save (and vice-versa). Don't hand-roll a `config.lock()` + `config::save` write — use
  the helper.
- Both save paths reconfigure the live engine via `commands.rs::reconfigure_engine`.
  `cmd_save_rules` (gated by `require_breaks`) sanitizes **rules only**
  (`config::sanitize_rules`), like `cmd_save_alarms` does for alarms. To prevent a stale
  Settings save from clobbering rules edited in the other window, both pages **refresh their
  rules grid on window focus**.
- Multi-window caveat: a true concurrent edit (both visible, save one then the other without
  refocusing) can still lose the earlier edit — acceptable for a single-user local app.

## Custom chimes (sounds for breaks + alarms)

- Audio is pure `rodio` sine-wave synthesis (`src-tauri/src/audio.rs`); three built-in tones
  (break-start, break-over, alarm) are the **defaults**. Users can also create **saved chimes**:
  named presets that are either a synthesized `ToneStep` sequence (`kind = "tones"`) or an
  imported audio file (`kind = "file"`, decoded by `rodio::Decoder`). The model + `sanitize_chimes`
  live in the dependency-free `crates/gomaju-core/src/chime.rs` (integer fields only — `ChimesFile`
  derives `Eq`; `is_safe_filename` rejects path-escaping names). Chimes persist in their **own**
  `chimes.toml` — at `<config_dir>/chimes/chimes.toml`, in the same folder as imported sound files —
  **not** in `config.toml`. `chime::load_chimes`/`save_chimes` self-heal + seed from the embedded
  `default_chimes.toml` on first run (which also creates the chimes folder). The host caches the
  list in `AppState.chimes` (`Mutex<Vec<ChimeDto>>`); `AppState.chimes_path` is the toml's path.
- A break **rule** picks a **start** chime (`RuleDto.chime_id`) and an **end** chime
  (`RuleDto.end_chime_id`), each with its own volume (`chime_volume_pct` /
  `end_chime_volume_pct`, default 20); an **alarm** picks one (`AlarmDto.chime_id` +
  `chime_volume_pct`) — all still in `config.toml`; empty (or an unknown id) = the built-in
  default tone at that picker volume. `audio::play_break_chime` (start) /
  `play_break_over_chime` (end, `runtime.rs` `EndBreak`, only on a **completed** break, not a skip)
  / `play_alarm_chime` resolve the id against `AppState.chimes` and fall back to the default (the
  end chime falls back to the break-over tone). The Settings rule grid (`rule-editor.ts`) shows two
  pickers per rule (Start chime / End chime). Alarms keep the "one tone per minute" policy — if
  several fire at once, the first one's chime and volume win.
- The **Chimes window** (`chimes.html` / `src/chimes.ts`, label `chimes`, opened from Settings via
  "Open chime editor" → `cmd_open_chimes`) composes
  with **musical notes** (`src/notes.ts`: Do-Re-Mi in C/G/F major → MIDI → Hz; tones stored as the
  resulting `freq_hz`). **Volume is not part of a saved chime**: `tone_source` synthesizes
  full-scale sines and playback applies the rule/alarm picker's `*_volume_pct` via
  `Sink::set_volume`, so the same preset can be quiet in one place and louder in another. Older
  `chimes.toml` files with `volume_pct` still load; the field is ignored and dropped on re-save.
  CRUD via `cmd_get_chimes` (reads the cache) / `cmd_save_chimes`
  (sanitize → write `chimes.toml` → swap cache → prune orphaned **audio** files only, so
  `chimes.toml` survives) / `cmd_preview_chime` (plays the unsaved def) / `cmd_import_chime_file`
  (native picker in **Rust** via tauri-plugin-dialog → copies into `<config_dir>/chimes/<id>.<ext>`).
  Writes are `require_chimes`-gated; `cmd_get_chimes` is readable from settings/alarms/chimes to fill
  the picker dropdowns (`fillChimeSelect` in `rule-editor.ts`). Clicking a note-palette button also
  **auditions that single note** (`playNote` → `cmd_preview_chime` with a one-step tones chime, at
  fixed volume 20) for immediate feedback as you compose; rests are silent.
- Preview is **stoppable** (the Preview button toggles ▶ Preview ⇄ ⏸ Pause). Unlike the
  fire-and-forget break/alarm cues, `audio.rs` tracks one current preview behind a generation token
  (`PREVIEW: Mutex<{gen, Arc<Sink>}>`): `cmd_preview_chime` returns the gen and `start_preview`
  registers the sink; `cmd_stop_preview` (the ⏸ Pause) stops it; on natural finish the thread emits
  `preview-ended` with its gen. The gen makes concurrent clicks safe — a new preview/stop supersedes
  the old, and a superseded thread never emits, so `src/chimes.ts` reverts only the matching button.
  Pause = stop (replay restarts from the beginning), for both tones and file chimes.

## Break quotes + pre-break toast

- The break overlay shows an optional inspirational **quote**, picked from the **active locale's**
  list in `quotes.toml` (next to `config.toml`), re-read each break by `quotes::pick(&quotes_path,
  &locale)` using `cfg.locale`. **No cross-locale fallback** — an empty active set shows no quote.
  Toggle: `settings.show_quotes`. Injected into `BreakInfo.quote` on both soft + strict overlays,
  like the per-rule `note`.
- Quotes are stored in a single **`quotes.toml`** (`<config_dir>/quotes.toml`) with two top-level
  arrays: `en = [...]` and `"zh-Hant" = [...]`. The data model + validation live in
  `crates/gomaju-core/src/quotes.rs` (mirrors `chime.rs`): `QuotesFile` struct, `sanitize()` (trim +
  drop blank/`#`-comment lines per locale), `read_quotes` (best-effort, **never writes** — used for
  all reads and the per-break pick), `save_quotes` (atomic temp+rename), and `load_quotes`
  (self-healing: missing → migrate-or-seed + write; corrupt → back up `.toml.bak` + reseed from
  embedded `crates/gomaju-core/default_quotes.toml`; valid → sanitize + persist only if changed).
  **First-run migration:** `load_quotes` builds `quotes.toml` from the old `quotes.en.txt` /
  `quotes.zh-Hant.txt` (and legacy `quotes.txt` → `en` if `quotes.en.txt` absent), then **deletes**
  those `.txt` files. `lib.rs` calls `load_quotes` once at startup; `AppState.quotes_path` holds the
  path. No in-memory cache — `read_quotes` runs each break.
- Quotes are editable in the Settings **Quotes** card (`index.html` / `src/main.ts`, shared
  `src/quotes-editor.ts` add/remove rows). A **locale toggle** (`.quote-locale-btn`, English /
  繁體中文) switches which set the rows show — `src/main.ts` keeps a per-locale map
  (`quotesByLocale`) and captures the visible rows on switch. Saved by the Settings **Save** button
  alongside config, **all locales at once**. `cmd_get_quotes`/`cmd_save_quotes` (require_settings)
  keep per-locale signatures (frontend untouched). `cmd_get_quotes` uses `read_quotes`;
  `cmd_save_quotes` is **read-modify-write** (`read_quotes` → set locale → `sanitize` →
  `save_quotes`) so saving one locale never clobbers the other, and uses `read_quotes` (not
  `load_quotes`) so no migration/backup side-effects. The row editor drops blank/`#`-comment lines.
  Save is conflict-guarded per locale: re-reads `quotes.toml` and, if changed outside Gomaju since
  last sync, prompts Overwrite/Keep-disk (`confirmQuotesConflict`) before writing. `onFocusRefresh`
  re-syncs all locales (like rules) when the window is clean.
- The pre-break countdown toast (`toast.html`) is positioned **bottom-right** near the tray via
  `Monitor::work_area()` (`toast.rs::position_bottom_right`), so it clears the taskbar. It carries a
  **Delay 1 min** snooze button: `toast.ts` → `cmd_delay_break(rule_id, 60)` (toast-window-gated) →
  `Engine::delay_break` subtracts 60s from that rule's accumulated `work` (pushing the break back)
  and emits `BreakWarningCancelled`, which closes the toast (the warning re-fires when the break is
  imminent again). The warning toast's injected `WarningInfo` now includes the `rule_id`.

## Config defaults

The seed config a fresh install writes lives as editable TOML at
`crates/gomaju-core/default_config.toml`, embedded via `include_str!` and parsed by
`ConfigFile::default()` (tests assert it parses and is sanitize-clean). `config::load`
generates `config.toml` from it on first run / corrupt-file recovery. Keep `CONFIG_VERSION`
in sync with the file's `version`.

## Layout

```
crates/gomaju-core/   # pure engine + config DTOs + alarm recurrence (no Tauri/OS deps); ships default_config.toml
src/                  # frontend: settings (index.html/main.ts), breaks.html, alarms.html, chimes.html, overlay.html, toast.html; shared rule-editor.ts
src-tauri/            # Tauri app: tray, idle, overlays, hotkeys, autostart, audio, notifications, alarm scheduler, window modules
```

## Logging

All `gomaju:` diagnostics go through the `rlog!` macro (`logging.rs`), a zero-dependency
drop-in for `eprintln!` that tees each line to **stderr** (so `tauri dev` is unchanged) **and**
to a rolling log file at `<config_dir>/gomaju.log` (rotated to `gomaju.log.old` **at startup** when
it exceeds ~1 MB). Embedded newlines in a logged value are collapsed so one diagnostic is one line.
`logging::init` is called once in `lib.rs` setup after the config dir exists; before that (and
in unit tests) `rlog!` is stderr-only. **Use `crate::rlog!(...)`, not `eprintln!`,** for any new
`gomaju:`-prefixed diagnostic so installed users (who have no console) leave a trace.

## Dev/test hooks (debug builds only)

- `GOMAJU_BREAK_ON_START=1` — fire a break ~2s after launch.
- `GOMAJU_OPEN_SETTINGS=1` — open the settings window on launch.
- `GOMAJU_OPEN_ALARMS=1` — open the alarms window on launch.
- `GOMAJU_NO_OPEN_RULES=1` — suppress the break-rules window's cold-start auto-open.
- Frontends log `gomaju: window content loaded: <label>` once their page renders —
  a useful signal that embedded assets actually loaded (it never fires in a broken
  dev-mode binary).
