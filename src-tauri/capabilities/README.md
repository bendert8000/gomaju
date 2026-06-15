# Capabilities

gomaju uses least-privilege capabilities, scoped by window label:

- **`settings.json`** — the `settings` window. Gets `core:default`.
- **`breaks.json`** — the `breaks` (break-rules dashboard) window. **Empty** permission set
  (like the overlays); it only invokes app commands. The rule commands (`cmd_get_rules` /
  `cmd_set_rule_flags` / `cmd_close_breaks` / `cmd_open_settings`) are restricted to this
  window by `require_breaks()` in `src/commands.rs`.
- **`alarms.json`** — the `alarms` window. **Empty** permission set (like the overlays):
  it only invokes app-defined commands, which aren't capability-gated. The alarm commands
  (`cmd_get_alarms` / `cmd_save_alarms` / `cmd_close_alarms`) are restricted to this window
  by the `require_alarms()` caller-check in `src/commands.rs`.
- **`timers.json`** — the `timers` (countdown) window. Only `core:event:allow-listen` (the
  unsaved-changes guard + chime-preview `preview-ended` event); it otherwise only invokes
  app-defined commands. The timer commands (`cmd_get_countdowns` / `cmd_save_countdowns` /
  `cmd_start_countdown` / `cmd_pause_countdown` / `cmd_reset_countdown` / `cmd_close_countdowns`)
  are restricted to this window by `require_timers()` in `src/commands.rs`; the chime picker
  reads `cmd_get_chimes` and previews via `cmd_preview_chime_by_id` (allowed by
  `shows_chime_picker()`, which now includes `timers`).
- **`chimes.json`** — the `chimes` (chime editor) window. **Empty** permission set: it only
  invokes app commands. The write commands (`cmd_save_chimes` / `cmd_import_chime_file` /
  `cmd_preview_chime` / `cmd_close_chimes`) are restricted to this window by `require_chimes()`;
  the read command `cmd_get_chimes` is allowed from settings/alarms/timers/chimes (the chime picker).
  The native file picker for imports runs in **Rust** (tauri-plugin-dialog), so no dialog JS
  permission is needed.
- **`overlay.json`** — break overlays (`overlay-*`), the countdown `warning-toast`, and the
  pause reminder `pause-toast`.
  **Empty** permission set: no core API access. They can still invoke app-defined
  commands (e.g. `cmd_skip`, `cmd_window_ready`) because app commands are not gated by
  capabilities; the real restriction is the caller-label checks in `src/commands.rs`.

> **Adding a new window?** Its label must be matched by one of these capability files
> (or a new one), otherwise the window gets **no capability and its IPC is denied** —
> it will silently fail to call commands. Add the label to the appropriate `windows`
> array. Also remember: any command that should be settings-only must call
> `require_settings(&window)` (capabilities alone do not gate app commands).
