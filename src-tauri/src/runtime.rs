use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use restee_core::{Effect, Enforcement, RunState};

use crate::app_state::AppState;
use crate::idle::IdleSource;
use crate::overlay::{self, BreakInfo};
use crate::toast::{self, WarningInfo};

#[derive(Clone, Serialize)]
struct BreakTickPayload {
    rule_id: String,
    remaining_secs: u64,
}

/// Spawn the once-per-second ticker on a dedicated OS thread (no async runtime
/// needed). It reads idle time, advances the engine, and applies the effects.
pub fn spawn_ticker(app: AppHandle, idle: Box<dyn IdleSource>) {
    std::thread::spawn(move || {
        let mut last = Instant::now();
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let now = Instant::now();
            let delta = now.duration_since(last);
            last = now;

            let idle_dur = idle.idle_for();
            let (effects, state) = {
                let st = app.state::<AppState>();
                let mut engine = st.engine.lock().unwrap();
                (engine.tick(delta, idle_dur), engine.state())
            };
            if !effects.is_empty() {
                apply_effects(&app, &effects);
            }
            // Keep the tray's elapsed time fresh (no-op unless the rendered text changed).
            refresh_tray(&app, state);
        }
    });
}

/// Interpret engine effects: emit events for the UI and create/destroy overlays.
pub fn apply_effects(app: &AppHandle, effects: &[Effect]) {
    for effect in effects {
        match effect {
            Effect::BreakWarning {
                name,
                enforcement,
                lead_secs,
                ..
            } => {
                toast::show(
                    app,
                    WarningInfo {
                        kind: enforcement.as_str().into(),
                        name: name.clone(),
                        lead_secs: *lead_secs,
                    },
                );
            }
            Effect::BreakWarningCancelled => {
                toast::close(app);
            }
            Effect::StartBreak {
                rule_id,
                name,
                enforcement,
                duration,
                escape_mode,
            } => {
                toast::close(app); // the countdown is over; replace it with the break
                overlay::show_break(
                    app,
                    BreakInfo {
                        kind: enforcement.as_str().into(),
                        name: name.clone(),
                        duration_secs: duration.as_secs(),
                        escape_mode: escape_mode.as_str().into(),
                    },
                );
                let _ = app.emit("break-start", rule_id.clone());

                let (sound, notify) = {
                    let state = app.state::<AppState>();
                    let cfg = state.config.lock().unwrap();
                    (cfg.settings.sound, cfg.settings.notifications)
                };
                if sound {
                    crate::audio::play_chime();
                }
                // Notifications augment soft breaks; strict breaks already take
                // over the whole screen, so a toast would be redundant.
                if notify && *enforcement == Enforcement::Soft {
                    show_notification(app, &format!("{name} — time for a quick break"));
                }
            }
            Effect::BreakTick {
                rule_id,
                remaining,
            } => {
                let _ = app.emit(
                    "break-tick",
                    BreakTickPayload {
                        rule_id: rule_id.clone(),
                        remaining_secs: remaining.as_secs(),
                    },
                );
            }
            Effect::EndBreak { .. } => {
                overlay::close_all(app);
                let _ = app.emit("break-end", ());
            }
            Effect::StateChanged(state) => {
                let _ = app.emit("state-changed", state.as_str());
                update_running_since(app, *state);
                refresh_tray(app, *state);
            }
            Effect::RuleReset { .. } => {}
        }
    }
}

// --- Actions shared by the tray menu and the IPC commands ---

pub fn action_start(app: &AppHandle, state: &AppState) {
    let fx = state.engine.lock().unwrap().start();
    apply_effects(app, &fx);
}

pub fn action_pause(app: &AppHandle, state: &AppState) {
    let fx = state.engine.lock().unwrap().pause();
    apply_effects(app, &fx);
}

pub fn action_skip(app: &AppHandle, state: &AppState) {
    let fx = state.engine.lock().unwrap().skip();
    apply_effects(app, &fx);
}

pub fn action_break_now(app: &AppHandle, state: &AppState) {
    let fx = state.engine.lock().unwrap().break_now();
    apply_effects(app, &fx);
}

pub fn action_reset(app: &AppHandle, state: &AppState) {
    let fx = state.engine.lock().unwrap().reset_timers();
    apply_effects(app, &fx);
}

/// Track when the timer (re)entered Running. Cleared on pause/stop; left intact
/// across breaks (InBreak) so elapsed time keeps counting through a break.
fn update_running_since(app: &AppHandle, state: RunState) {
    let st = app.state::<AppState>();
    let mut since = st.running_since.lock().unwrap();
    match state {
        RunState::Running => {
            if since.is_none() {
                *since = Some(Instant::now());
            }
        }
        RunState::Paused | RunState::Stopped => *since = None,
        RunState::InBreak => {}
    }
}

/// Update the tray Start/Pause checks + elapsed text for the given state.
pub fn refresh_tray(app: &AppHandle, state: RunState) {
    let running_secs = app
        .state::<AppState>()
        .running_since
        .lock()
        .unwrap()
        .map(|since| since.elapsed().as_secs())
        .unwrap_or(0);
    crate::tray::refresh(app, state, running_secs);
}

/// Show an OS notification titled "restee" with `body`. Best-effort: on Windows the
/// toast renders reliably only once the app is installed (has an app identity).
pub fn show_notification(app: &AppHandle, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    match app.notification().builder().title("restee").body(body).show() {
        Ok(_) => eprintln!("restee: notification shown ({body})"),
        Err(e) => eprintln!("restee: notification failed ({e})"),
    }
}

/// Show the "app is running" startup notification, auto-dismissed after ~2s.
///
/// On Windows we drive the WinRT toast directly so we can `Hide` it after 2s — the
/// notification plugin offers no control over toast lifetime, and Windows won't show
/// a banner for less than the OS minimum (~5s). `Hide` removes the banner *and* the
/// Action Center entry, so the message doesn't linger. Other platforms (and any
/// WinRT failure) fall back to the standard plugin notification.
pub fn show_startup_notification(app: &AppHandle, body: &str) {
    #[cfg(windows)]
    {
        match win_startup_toast(app, body) {
            Ok(()) => return,
            Err(e) => eprintln!("restee: WinRT startup toast failed ({e}); using plugin"),
        }
    }
    show_notification(app, body);
}

/// Drive the Windows toast directly and schedule its dismissal ~2s later.
#[cfg(windows)]
fn win_startup_toast(app: &AppHandle, body: &str) -> windows::core::Result<()> {
    use windows::core::HSTRING;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

    let app_id = win_toast_app_id(app);

    // Minimal ToastGeneric payload: title "restee" plus the body line.
    let xml = format!(
        "<toast duration=\"short\"><visual><binding template=\"ToastGeneric\">\
         <text>restee</text><text>{}</text></binding></visual></toast>",
        xml_escape(body)
    );

    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(xml))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(&app_id))?;
    notifier.Show(&toast)?;
    eprintln!("restee: startup toast shown ({body}); auto-dismiss in 2s");

    // Remove it after ~2s so it doesn't linger on screen or in the Action Center.
    // WinRT toast objects are agile (Send + Sync); hide on the main thread to keep
    // COM usage on the thread Tauri initialized.
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(2));
        let _ = app.run_on_main_thread(move || {
            if let Err(e) = notifier.Hide(&toast) {
                eprintln!("restee: startup toast hide failed ({e})");
            }
        });
    });

    Ok(())
}

/// Pick the AppUserModelID for the toast, mirroring tauri-plugin-notification:
/// use the bundle identifier only when actually installed; otherwise fall back to
/// the always-registered PowerShell AUMID (toasts won't render under an
/// unregistered AUMID, e.g. when running from `target/{debug,release}`).
#[cfg(windows)]
fn win_toast_app_id(app: &AppHandle) -> String {
    use std::path::MAIN_SEPARATOR as SEP;
    const POWERSHELL_AUMID: &str =
        "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe";

    let identifier = app.config().identifier.clone();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let d = dir.display().to_string();
            let in_target = d.ends_with(&format!("{SEP}target{SEP}debug"))
                || d.ends_with(&format!("{SEP}target{SEP}release"));
            if !in_target {
                return identifier;
            }
        }
    }
    POWERSHELL_AUMID.to_string()
}

/// Escape the five XML predefined entities so arbitrary text is safe inside the
/// toast payload.
#[cfg(windows)]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
