# gomaju — Fix Plan (from code review, revised after Codex review)

Addresses the actionable findings from the working-tree review. Scope: correctness,
least-privilege, and release hygiene. Codex reviewed v1 of this plan; its corrections
are folded in (notably: app commands are NOT gated per-window by capabilities, so the
Rust-side caller check is the real enforcement, and several commands were missed).

## 1. Least-privilege for IPC commands (PRIORITY)

**Problem:** `capabilities/default.json` applies `core:default` to `windows: ["*"]`.
Per Tauri v2 docs, **app commands registered via `invoke_handler` are callable from
any window by default** — the capability split alone does NOT restrict them. So the
**load-bearing enforcement is a Rust-side caller check**, not the capability files.

**Approach:**
- Add a shared helper and guard every command a non-settings window does not need:
  ```rust
  fn require_settings(window: &tauri::WebviewWindow) -> Result<(), String> {
      if window.label() == "settings" { Ok(()) } else { Err("forbidden".into()) }
  }
  ```
  `WebviewWindow` implements `CommandArg`, so protected commands take
  `window: tauri::WebviewWindow` and call `require_settings(&window)?` first.
- **Guard (settings-only):** `cmd_save_config`, `cmd_get_config`, `cmd_get_idle_status`,
  `cmd_close_settings`, **and the control commands `cmd_start`, `cmd_pause`,
  `cmd_break_now`** (Codex caught these — they mutate state and are currently callable
  from any window; the tray invokes the underlying `action_*` directly in Rust, so
  guarding the commands doesn't affect the tray).
- **Leave open (overlays/toast legitimately call these):** `cmd_skip`,
  `cmd_window_ready`.
- **Capabilities:** split into `settings.json` (`windows: ["settings"]`, `core:default`)
  and `overlay.json` (`windows: ["overlay-*", "warning-toast"]`) with the **smallest**
  permission set that still lets overlays do IPC + listen to events. **Verify overlays
  can still invoke `cmd_skip`/`cmd_window_ready` after trimming**; if a reduced set
  breaks IPC, keep `core:default` for overlays and rely on the caller checks (which are
  the real control). Do not describe the split as mere "forward-compat" — wire it for real.
- **Files:** `src-tauri/src/commands.rs` (helper + guards), `src-tauri/capabilities/*.json`.
- **Verify:** settings save/load/close still work; overlay skip + hold-Esc still work;
  a manual/debug probe shows a non-settings caller gets `forbidden` on a guarded command.

## 2. Pre-break warning should target the soonest-to-fire rule (CORRECTNESS)

**Problem:** `pick_imminent_warning` selects by priority, but the rule firing next is the
one reaching its interval soonest; the toast can name the wrong break.

**Approach:** select the in-window enabled rule with the smallest remaining
(`interval - work`); tiebreak by `higher_priority`, then list order.
- **Files:** `crates/gomaju-core/src/engine.rs`.
- **Tests (TDD):** (a) two in-window rules where the shorter-remaining one is the
  lower-priority soft rule → warning names the soft rule; (b) **tie test** — equal
  remaining strict vs soft → choose strict (Codex's addition).

## 3. Warning countdown must not "lie" when work stops (CORRECTNESS)

**Problem:** under idle-pause/suspend the engine keeps the warning active while work
stops advancing, but `toast.ts` counts down to "starting now…", which is false.

**Approach (minimal, honest):** when the local countdown reaches 0, show
"starting soon…" (not "now") and hold the bar at 100% — the engine remains
authoritative for the actual start. (Stronger option, deferred: have the engine push
warning pause/resume updates to the toast.)
- **Files:** `src/toast.ts`.

## 4. Define/clamp the warn-vs-interval relationship (CORRECTNESS)

**Problem:** v1's "clamp `warn_seconds <= 600`" rationale was false — intervals can be
5s, so a large warn still warns at cycle start. (Codex.)

**Approach:** make the behavior explicit. In `pick_imminent_warning`, use an effective
warn of `min(warn, interval - 1)` per rule so the warning always lands at least 1s into
the cycle (never at work=0), and document that `warn >= interval` means "warn as early
as possible." Keep a generous `warn_seconds` sanity clamp in config (e.g. `<= 3600`).
- **Files:** `crates/gomaju-core/src/engine.rs`, `crates/gomaju-core/src/config.rs` (+ test).

## 5. Don't honor test env hooks in release (HYGIENE)

**Approach:** gate `GOMAJU_BREAK_ON_START` / `GOMAJU_OPEN_SETTINGS` behind
`#[cfg(debug_assertions)]`. (Codex note: acceptable, but for a hard guarantee a
`dev-hooks` Cargo feature is stronger since a release profile *could* enable debug
assertions — ours doesn't, so `debug_assertions` is sufficient; mention the feature
alternative in a comment.)
- **Files:** `src-tauri/src/lib.rs`.

## 6. Second launch opens Settings (UX)

**Approach:** single-instance callback calls `settings_window::open(app)` — nonblocking,
log failures. Verify rapid double-launch during startup doesn't race.
- **Files:** `src-tauri/src/lib.rs`.

## 7. Harden the `initialization_script` injection (HARDENING)

**Problem:** init scripts run on every top-level navigation; Tauri docs recommend a
`window.location` origin/path check.

**Approach:** guard the injected assignment so it only sets `window.__GOMAJU_*__` on the
expected app origin/path, e.g. wrap in `if (location.protocol === 'tauri:' || …) { … }`
inside the injected script. Overlays/toast never navigate, so this is defense-in-depth.
- **Files:** `src-tauri/src/overlay.rs`, `src-tauri/src/toast.rs`.

## 8. Content-Security-Policy (OPTIONAL — careful, may break IPC)

**Approach:** if added, use Tauri's recommended shape **including IPC**:
`default-src 'self'; connect-src ipc: http://ipc.localhost; img-src 'self' data:;
style-src 'self' 'unsafe-inline'`. Don't add `'unsafe-inline'` for scripts (Tauri
injects hashes/nonces for bundled assets). **Test production IPC + rendering for all
three windows; if it breaks and isn't quickly fixable, revert/defer.** Lower priority
than 1–4.
- **Files:** `src-tauri/tauri.conf.json`.

## Verification (whole round)
- `cargo test -p gomaju-core` (existing 19 + new warning/tie/clamp tests) and
  `cargo clippy --workspace --all-targets` clean.
- `npx tauri build`; launch the production binary and confirm via logs/manual:
  settings opens from tray, saves, and closes; overlay skip + hold-Esc work; a natural
  break warns (correct rule named) then fires; release ignores env hooks; second launch
  opens Settings; (if CSP added) all three `window content loaded` pings still fire.

## Explicitly deferred (external resources / larger effort)
- **Wayland idle** backend (D-Bus / ext-idle) — needs Linux testing.
- **Code signing / notarization** — needs Windows cert + Apple account.
- **Glue-layer integration tests** and **property tests** — larger harness work.
- **Engine→toast pause/resume updates** (stronger fix for #3).
- **Audio `OutputStream` reuse** — micro-optimization.

## Codex review summary
Codex confirmed: `WebviewWindow`/`label()` caller-checks are valid and supported; the
soonest-to-fire change is correct (add the tie test); `debug_assertions` and the
single-instance approach are fine. It corrected: app commands aren't capability-gated
(label checks are the real control), the guarded-command list missed start/pause/
break_now, the `warn <= 600` rationale was false, the CSP needs an explicit `connect-src`
for IPC, and the init-script needs an origin guard. All folded in above.
