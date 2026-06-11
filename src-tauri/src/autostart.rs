use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

/// Enable or disable launch-at-login to match the desired state. Only toggles when
/// the current state differs, so disabling an already-disabled entry is a no-op
/// (the registry/LaunchAgent delete would otherwise error spuriously).
pub fn apply(app: &AppHandle, enabled: bool) {
    let manager = app.autolaunch();
    if manager.is_enabled().unwrap_or(false) == enabled {
        return;
    }
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    if let Err(e) = result {
        crate::rlog!("gomaju: failed to set autostart={enabled}: {e}");
    }
}
