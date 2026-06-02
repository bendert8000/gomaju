# Capabilities

restee uses least-privilege capabilities, scoped by window label:

- **`settings.json`** — the `settings` window. Gets `core:default`.
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
