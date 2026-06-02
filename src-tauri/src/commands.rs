use serde::Serialize;
use tauri::{AppHandle, State, WebviewWindow};

use restee_core::config::{self, ConfigFile};

use crate::app_state::AppState;
use crate::idle::IdleStatus;
use crate::settings_window::{self, SETTINGS_LABEL};
use crate::{autostart, hotkeys, runtime};

/// Pure, unit-testable predicate: is this window label the settings window?
fn is_settings(label: &str) -> bool {
    label == SETTINGS_LABEL
}

/// Reject a command invoked from any window other than the settings window.
/// App commands are not gated per-window by Tauri's capability system, so this
/// caller-label check is the real least-privilege enforcement.
fn require_settings(window: &WebviewWindow) -> Result<(), String> {
    if is_settings(window.label()) {
        Ok(())
    } else {
        Err("forbidden: settings-only command".into())
    }
}

/// Open to overlay windows — they legitimately end the current break.
#[tauri::command]
pub fn cmd_skip(app: AppHandle, state: State<'_, AppState>) {
    runtime::action_skip(&app, state.inner());
}

#[tauri::command]
pub fn cmd_reset_timers(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require_settings(&window)?;
    runtime::action_reset(&app, state.inner());
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

/// Current run state + time to the soonest break, for the settings status banner.
#[derive(Serialize)]
pub struct StatusDto {
    pub state: &'static str,
    pub next_rule: Option<String>,
    pub next_secs: Option<u64>,
}

#[tauri::command]
pub fn cmd_get_status(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<StatusDto, String> {
    require_settings(&window)?;
    let snapshot = state.engine.lock().unwrap().status();
    Ok(StatusDto {
        state: snapshot.state.as_str(),
        next_rule: snapshot.next.as_ref().map(|n| n.rule_name.clone()),
        next_secs: snapshot.next.map(|n| n.remaining_secs),
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
    config::save(&state.config_path, &config).map_err(|e| e.to_string())?;

    let (rules, settings) = config.to_engine_inputs();
    let fx = state.engine.lock().unwrap().reconfigure(rules, settings);
    runtime::apply_effects(&app, &fx);

    let hotkey_errors = hotkeys::apply(&app, &config.hotkeys);
    autostart::apply(&app, config.autostart);

    *state.config.lock().unwrap() = config.clone();

    Ok(SaveOutcome {
        config,
        hotkey_errors,
    })
}

#[cfg(test)]
mod tests {
    use super::is_settings;

    #[test]
    fn only_the_settings_window_is_privileged() {
        assert!(is_settings("settings"));
        // Overlay, toast, and anything else must be rejected.
        assert!(!is_settings("overlay-0"));
        assert!(!is_settings("overlay-1"));
        assert!(!is_settings("warning-toast"));
        assert!(!is_settings("Settings")); // case-sensitive
        assert!(!is_settings(""));
    }
}
