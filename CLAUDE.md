# CLAUDE.md

Guidance for AI agents (and humans) working in this repo. Keep it short and current.

restee is a cross-platform, tray-resident break reminder built with **Tauri v2**
(Rust core + TypeScript/HTML/CSS UI). The dependency-free `restee-core` crate decides
*when* to break; the `src-tauri` layer turns those decisions into windows, sounds,
tray UI, and notifications.

## Build & run

| Goal | Command |
| --- | --- |
| Dev (hot reload, runs Vite dev server) | `npm run tauri dev` |
| Full release + installers | `npm run tauri build` |
| Quick runnable release binary (no installers) | `cargo build --release --features custom-protocol` |
| Core engine tests (fast, no Tauri) | `cargo test -p restee-core` |
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
`src-tauri/Cargo.toml`). The release binary lands at `target/release/restee.exe`
(workspace target dir at the repo root, not under `src-tauri/`).

`npm run tauri build` also runs `npm run build` (`tsc && vite build`) to refresh
`dist/`. If you only `cargo build --features custom-protocol`, you reuse the existing
`dist/` — rebuild the frontend separately (`npm run build`) if `src/` changed.

## Notifications (platform notes)

- Break/soft notifications use `tauri-plugin-notification` (`runtime::show_notification`).
- The **startup** "Restee is running now" toast is special: `runtime::show_startup_notification`
  auto-dismisses after ~2s. The plugin exposes no control over toast lifetime, and a
  native Windows banner can't be shown for less than the OS minimum (~5s). So on
  Windows we drive the WinRT toast directly (`windows` crate) and call
  `ToastNotifier::Hide` after 2s, which clears both the banner and the Action Center
  entry. Other platforms (and any WinRT failure) fall back to the plugin.

## Alarms (clock alarms, separate from breaks)

- Wall-clock alarms (name + time + recurrence Once/Daily/Weekly/Bi-weekly/Monthly/Yearly)
  live in `config.toml` as `[[alarms]]`. They fire a notification + a distinct repeating
  tone (`audio::play_alarm`) regardless of run state.
- **Bi-weekly** reuses Weekly's `weekdays` plus the `date` field as a start-week anchor:
  fires the ticked days every *other* Monday-aligned week from that week, never before the
  start date. Week-parity is pure integer math — `days_from_civil` + `monday_week` in
  `alarm.rs` (chrono-free, unit-tested in isolation).
- The engine stays clock-free: recurrence is a pure, tested matcher in
  `crates/restee-core/src/alarm.rs` (`alarm_is_due` + `sanitize_alarms`); the firing
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
  still "Restee — Break rules") is a
  **quick-select dashboard**: big read-only cards where only On/Off (tap the card) and
  Repeat/Once (segmented control) are editable; each toggle auto-saves via
  `cmd_set_rule_flags` (merge-by-id, so it never clobbers Settings detail edits) and
  reconfigures the engine live. "Edit in Settings…" → `cmd_open_settings`. The dashboard
  renders its own cards (does NOT use the shared `ruleRow`); it imports only the `RuleDto`
  type.
- The standalone window **auto-opens on every cold start** (`lib.rs` setup) and replaces the
  "Restee is running now" startup toast (the window is the signal). Debug builds honor
  `RESTEE_NO_OPEN_RULES` to suppress it.
- Each rule has a `repeat` flag (default true). A **once** rule (`repeat=false`) fires one
  break, then the engine disables it (`Effect::RuleDisabled`) and the host persists
  `enabled=false` (`runtime::persist_rule_disabled`) — same auto-disable model as alarm
  "Once"; re-check "On" to re-arm. All config writers hold the `config` lock across
  save+cache so the ticker's auto-disable can't clobber a concurrent window Save.
- Both save paths reconfigure the live engine via `commands.rs::reconfigure_engine`.
  `cmd_save_rules` (gated by `require_breaks`) sanitizes **rules only**
  (`config::sanitize_rules`), like `cmd_save_alarms` does for alarms. To prevent a stale
  Settings save from clobbering rules edited in the other window, both pages **refresh their
  rules grid on window focus**.
- Multi-window caveat: a true concurrent edit (both visible, save one then the other without
  refocusing) can still lose the earlier edit — acceptable for a single-user local app.

## Config defaults

The seed config a fresh install writes lives as editable TOML at
`crates/restee-core/default_config.toml`, embedded via `include_str!` and parsed by
`ConfigFile::default()` (tests assert it parses and is sanitize-clean). `config::load`
generates `config.toml` from it on first run / corrupt-file recovery. Keep `CONFIG_VERSION`
in sync with the file's `version`.

## Layout

```
crates/restee-core/   # pure engine + config DTOs + alarm recurrence (no Tauri/OS deps); ships default_config.toml
src/                  # frontend: settings (index.html/main.ts), breaks.html, alarms.html, overlay.html, toast.html; shared rule-editor.ts
src-tauri/            # Tauri app: tray, idle, overlays, hotkeys, autostart, audio, notifications, alarm scheduler, window modules
```

## Dev/test hooks (debug builds only)

- `RESTEE_BREAK_ON_START=1` — fire a break ~2s after launch.
- `RESTEE_OPEN_SETTINGS=1` — open the settings window on launch.
- `RESTEE_OPEN_ALARMS=1` — open the alarms window on launch.
- `RESTEE_NO_OPEN_RULES=1` — suppress the break-rules window's cold-start auto-open.
- Frontends log `restee: window content loaded: <label>` once their page renders —
  a useful signal that embedded assets actually loaded (it never fires in a broken
  dev-mode binary).
