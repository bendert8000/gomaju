use serde::Serialize;
use tauri::{AppHandle, State, WebviewWindow};

use restee_core::alarm::{self, AlarmDto};
use restee_core::config::{self, ConfigFile, RuleDto};

use crate::alarms_window::{self, ALARMS_LABEL};
use crate::app_state::AppState;
use crate::idle::IdleStatus;
use crate::breaks_window::{self, BREAKS_LABEL};
use crate::settings_window::{self, SETTINGS_LABEL};
use crate::{autostart, hotkeys, runtime};

/// Pure, unit-testable predicate: is this window label the settings window?
fn is_settings(label: &str) -> bool {
    label == SETTINGS_LABEL
}

/// Pure, unit-testable predicate: is this window label the alarms window?
fn is_alarms(label: &str) -> bool {
    label == ALARMS_LABEL
}

/// Pure, unit-testable predicate: is this window label the breaks window?
fn is_breaks(label: &str) -> bool {
    label == BREAKS_LABEL
}

/// Shared gate body: app commands are not gated per-window by Tauri's capability system,
/// so this caller-label check is the real least-privilege enforcement. `what` names the
/// privileged scope for the rejection message.
fn gate(allowed: bool, what: &str) -> Result<(), String> {
    if allowed {
        Ok(())
    } else {
        Err(format!("forbidden: {what}-only command"))
    }
}

/// Reject a command invoked from any window other than the settings window.
fn require_settings(window: &WebviewWindow) -> Result<(), String> {
    gate(is_settings(window.label()), "settings")
}

/// Reject an alarms command invoked from any window other than the alarms window.
fn require_alarms(window: &WebviewWindow) -> Result<(), String> {
    gate(is_alarms(window.label()), "alarms")
}

/// Reject a breaks-dashboard command invoked from any window other than the breaks window.
fn require_breaks(window: &WebviewWindow) -> Result<(), String> {
    gate(is_breaks(window.label()), "breaks")
}

/// Push a config's rules+settings into the live engine and apply any resulting effects.
/// Shared by `cmd_save_config` and `cmd_set_rule_flags`; deliberately narrow — it does NOT
/// touch hotkeys or autostart (those stay in `cmd_save_config`).
fn reconfigure_engine(app: &AppHandle, state: &AppState, config: &ConfigFile) {
    let (rules, settings) = config.to_engine_inputs();
    let fx = state.engine.lock().unwrap().reconfigure(rules, settings);
    runtime::apply_effects(app, &fx);
}

/// Open to overlay windows — they legitimately end the current break.
#[tauri::command]
pub fn cmd_skip(app: AppHandle, state: State<'_, AppState>) {
    runtime::action_skip(&app, state.inner());
}

/// Per-break reset: restart a single rule's countdown (the window banners' per-row Reset).
/// Reset-all lives on the tray "Reset timer" item (`runtime::confirm_then_reset`).
#[tauri::command]
pub fn cmd_reset_timer(
    window: WebviewWindow,
    app: AppHandle,
    rule_id: String,
) -> Result<(), String> {
    gate(
        is_settings(window.label()) || is_breaks(window.label()),
        "settings/rules",
    )?;
    runtime::confirm_then_reset_one(&app, rule_id);
    Ok(())
}

#[tauri::command]
pub fn cmd_get_config(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<ConfigFile, String> {
    require_settings(&window)?;
    Ok(state.config.lock().unwrap().clone())
}

#[tauri::command]
pub fn cmd_get_idle_status(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<IdleStatus, String> {
    require_settings(&window)?;
    Ok(state.idle_status)
}

#[tauri::command]
pub fn cmd_close_settings(window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    require_settings(&window)?;
    settings_window::close(&app);
    Ok(())
}

/// One enabled break's countdown, for the status banners + dashboard cards.
#[derive(Serialize)]
pub struct NextBreakDto {
    pub rule_id: String,
    pub rule_name: String,
    pub remaining_secs: u64,
}

/// Current run state + every enabled break (soonest-first), for the settings /
/// Today's breaks banners and the per-card countdowns.
#[derive(Serialize)]
pub struct StatusDto {
    pub state: &'static str,
    pub all: Vec<NextBreakDto>,
}

#[tauri::command]
pub fn cmd_get_status(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<StatusDto, String> {
    // Read-only status, shown in both the Settings banner and the Break-rules dashboard.
    gate(
        is_settings(window.label()) || is_breaks(window.label()),
        "settings/rules",
    )?;
    let snapshot = state.engine.lock().unwrap().status();
    Ok(StatusDto {
        state: snapshot.state.as_str(),
        all: snapshot
            .all
            .into_iter()
            .map(|n| NextBreakDto {
                rule_id: n.rule_id,
                rule_name: n.rule_name,
                remaining_secs: n.remaining_secs,
            })
            .collect(),
    })
}

/// Open to all windows — each window pings on load to confirm it rendered. The label
/// is caller-controllable, so sanitize it (drop non-token chars, cap length) before
/// logging to avoid log injection.
#[tauri::command]
pub fn cmd_window_ready(label: String) {
    let safe: String = label
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(32)
        .collect();
    eprintln!("restee: window content loaded: {safe}");
}

/// Result of saving config: the (possibly sanitized) config echoed back, plus any
/// hotkey accelerators that could not be registered.
#[derive(Serialize)]
pub struct SaveOutcome {
    pub config: ConfigFile,
    pub hotkey_errors: Vec<String>,
}

/// Validate + persist edited config, then apply it live: reconfigure the engine,
/// re-register hotkeys, and sync autostart.
#[tauri::command]
pub fn cmd_save_config(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, AppState>,
    mut config: ConfigFile,
) -> Result<SaveOutcome, String> {
    require_settings(&window)?;
    config.sanitize();
    {
        // Hold the lock across the disk write AND the cache update so the ticker's
        // once-rule auto-disable (runtime::persist_rule_disabled) can't interleave a stale
        // snapshot between them. On a write error the guard drops with the cache untouched.
        let mut guard = state.config.lock().unwrap();
        // Locale is backend-owned (only the tray changes it). The Settings form never edits
        // it, and an incoming payload without the field would serde-default to "zh-Hant" —
        // so preserve the stored value rather than let a Save clobber a language switch.
        config.locale = guard.locale.clone();
        config::save(&state.config_path, &config).map_err(|e| e.to_string())?;
        *guard = config.clone();
    }

    reconfigure_engine(&app, state.inner(), &config);

    let hotkey_errors = hotkeys::apply(&app, &config.hotkeys);
    autostart::apply(&app, config.autostart);

    Ok(SaveOutcome {
        config,
        hotkey_errors,
    })
}

// --- Alarms (alarms-window only) ---

#[tauri::command]
pub fn cmd_get_alarms(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<AlarmDto>, String> {
    require_alarms(&window)?;
    Ok(state.config.lock().unwrap().alarms.clone())
}

/// The next fire instant for one enabled alarm, for the Alarms window's "Next: …" label.
#[derive(Serialize)]
pub struct AlarmFireDto {
    pub id: String,
    /// Unix timestamp (seconds) of the next fire.
    pub at_secs: i64,
}

/// Compute the next fire time of every *enabled* alarm whose schedule still has one.
/// Reflects the saved config (the window refreshes this on load / save / focus).
#[tauri::command]
pub fn cmd_get_alarm_fires(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<AlarmFireDto>, String> {
    require_alarms(&window)?;
    let now = chrono::Local::now();
    let alarms = state.config.lock().unwrap().alarms.clone();
    let fires = alarms
        .iter()
        .filter_map(|a| {
            crate::alarm::next_fire(a, now).map(|when| AlarmFireDto {
                id: a.id.clone(),
                at_secs: when.timestamp(),
            })
        })
        .collect();
    Ok(fires)
}

/// Persist the edited alarm list. Clone the current config, swap in the new alarms,
/// sanitize, write to disk, and only then update the in-memory cache — so a failed
/// write never leaves the cache ahead of the file. Returns the sanitized alarms so the
/// UI reflects any normalization (disabled empty-weekly alarms, regenerated ids, etc.).
#[tauri::command]
pub fn cmd_save_alarms(
    window: WebviewWindow,
    state: State<'_, AppState>,
    alarms: Vec<AlarmDto>,
) -> Result<Vec<AlarmDto>, String> {
    require_alarms(&window)?;

    let mut config = state.config.lock().unwrap().clone();
    config.alarms = alarms;
    // Only the alarms changed here, so validate just those (rules/settings in the cached
    // config were already sanitized at load / their own save).
    alarm::sanitize_alarms(&mut config.alarms);
    let sanitized = config.alarms.clone();

    // Hold the lock across save + cache swap, and re-read the backend-owned locale so a
    // language switch made since the clone above isn't lost.
    let mut guard = state.config.lock().unwrap();
    config.locale = guard.locale.clone();
    config::save(&state.config_path, &config).map_err(|e| e.to_string())?;
    *guard = config;
    Ok(sanitized)
}

#[tauri::command]
pub fn cmd_close_alarms(window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    require_alarms(&window)?;
    alarms_window::close(&app);
    Ok(())
}

// --- Break rules (breaks-window only) ---

#[tauri::command]
pub fn cmd_get_rules(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<RuleDto>, String> {
    require_breaks(&window)?;
    Ok(state.config.lock().unwrap().rules.clone())
}

/// Set just the `enabled`/`repeat` flags of one rule (the quick-select dashboard's only
/// edits) and apply live. Merge-by-id onto the *fresh* cached config so it can never clobber
/// detail edits made in Settings. Clone/edit/sanitize/write/commit under one held `config`
/// lock (so the ticker's once-rule auto-disable can't interleave a stale snapshot); drop the
/// lock before reconfiguring the engine. The JS side passes camelCase `ruleId` (Tauri maps
/// it to `rule_id`). Returns `()` — the dashboard updates optimistically.
#[tauri::command]
pub fn cmd_set_rule_flags(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, AppState>,
    rule_id: String,
    enabled: bool,
    repeat: bool,
) -> Result<(), String> {
    require_breaks(&window)?;

    let config = {
        let mut guard = state.config.lock().unwrap();
        let mut config = guard.clone();
        let Some(rule) = config.rules.iter_mut().find(|r| r.id == rule_id) else {
            return Ok(()); // rule no longer exists (e.g. deleted in Settings) — no-op
        };
        rule.enabled = enabled;
        rule.repeat = repeat;
        config::sanitize_rules(&mut config.rules);
        config::save(&state.config_path, &config).map_err(|e| e.to_string())?;
        *guard = config.clone();
        config
    };

    reconfigure_engine(&app, state.inner(), &config);
    Ok(())
}

#[tauri::command]
pub fn cmd_close_breaks(window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    require_breaks(&window)?;
    breaks_window::close(&app);
    Ok(())
}

/// Open the Settings window from the rules dashboard's "Edit in Settings…" button.
#[tauri::command]
pub async fn cmd_open_settings(window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    // Async so this runs off the main thread. Creating a window *synchronously* from within
    // a webview's IPC handler pumps a nested event loop and deadlocks the app (the tray path
    // is a native menu event, so it's unaffected). `settings_window::open` marshals the actual
    // window build to the main thread via `run_on_main_thread`, which now posts cleanly from
    // this off-main-thread command instead of re-entering the loop.
    require_breaks(&window)?;
    settings_window::open(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{is_alarms, is_breaks, is_settings};

    #[test]
    fn only_the_settings_window_is_privileged() {
        assert!(is_settings("settings"));
        // Overlay, toast, alarms, and anything else must be rejected.
        assert!(!is_settings("overlay-0"));
        assert!(!is_settings("overlay-1"));
        assert!(!is_settings("warning-toast"));
        assert!(!is_settings("alarms"));
        assert!(!is_settings("breaks"));
        assert!(!is_settings("Settings")); // case-sensitive
        assert!(!is_settings(""));
    }

    #[test]
    fn only_the_alarms_window_is_privileged_for_alarm_commands() {
        assert!(is_alarms("alarms"));
        assert!(!is_alarms("settings"));
        assert!(!is_alarms("breaks"));
        assert!(!is_alarms("overlay-0"));
        assert!(!is_alarms("warning-toast"));
        assert!(!is_alarms("Alarms")); // case-sensitive
        assert!(!is_alarms(""));
    }

    #[test]
    fn only_the_breaks_window_is_privileged_for_rule_commands() {
        assert!(is_breaks("breaks"));
        assert!(!is_breaks("settings"));
        assert!(!is_breaks("alarms"));
        assert!(!is_breaks("overlay-0"));
        assert!(!is_breaks("Breaks")); // case-sensitive
        assert!(!is_breaks(""));
    }
}
