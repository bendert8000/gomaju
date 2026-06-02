# Capabilities

restee uses least-privilege capabilities, scoped by window label:

- **`settings.json`** — the `settings` window. Gets `core:default`.
- **`rules.json`** — the `rules` (break-rules) window. **Empty** permission set (like the
  overlays); it only invokes app commands. The rule commands (`cmd_get_rules` /
  `cmd_set_rule_flags` / `cmd_close_rules` / `cmd_open_settings`) are restricted to this
  window by `require_rules()` in `src/commands.rs`.
- **`alarms.json`** — the `alarms` window. **Empty** permission set (like the overlays):
  it only invokes app-defined commands, which aren't capability-gated. The alarm commands
  (`cmd_get_alarms` / `cmd_save_alarms` / `cmd_close_alarms`) are restricted to this window
  by the `require_alarms()` caller-check in `src/commands.rs`.
- **`overlay.json`** — break overlays (`overlay-*`) and the countdown `warning-toast`.
  **Empty** permission set: no core API access. They can still invoke app-defined
  commands (e.g. `cmd_skip`, `cmd_window_ready`) because app commands are not gated by
  capabilities; the real restriction is the `require_settings()` caller-check in
  `src/commands.rs`.

> **Adding a new window?** Its label must be matched by one of these capability files
> (or a new one), otherwise the window gets **no capability and its IPC is denied** —
> it will silently fail to call commands. Add the label to the appropriate `windows`
> array. Also remember: any command that should be settings-only must call
> `require_settings(&window)` (capabilities alone do not gate app commands).
