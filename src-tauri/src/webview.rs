// Shared webview helpers.

// Init scripts run on every top-level navigation; only set our global on the app's
// own origin (tauri:// on macOS/Linux, tauri.localhost on Windows, localhost in dev).
const INIT_GUARD_PREFIX: &str = "(function(){ var l = window.location; if (l.protocol === 'tauri:' || l.hostname === 'tauri.localhost' || l.hostname === 'localhost') { ";
const INIT_GUARD_SUFFIX: &str = "; } })();";

/// Build an initialization script that assigns `window.<global> = <json>` only on the
/// app's own origin. `json` must be valid JS (e.g. from `serde_json::to_string`).
pub fn guarded_init(global: &str, json: &str) -> String {
    format!("{INIT_GUARD_PREFIX}window.{global} = {json}{INIT_GUARD_SUFFIX}")
}
