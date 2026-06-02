use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use restee_core::{Effect, Enforcement};

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
            let effects = {
                let state = app.state::<AppState>();
                let mut engine = state.engine.lock().unwrap();
                engine.tick(delta, idle_dur)
            };
            if !effects.is_empty() {
                apply_effects(&app, &effects);
            }
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
                    use tauri_plugin_notification::NotificationExt;
                    match app
                        .notification()
                        .builder()
                        .title("restee")
                        .body(format!("{name} — time for a quick break"))
                        .show()
                    {
                        Ok(_) => eprintln!("restee: notification shown"),
                        Err(e) => eprintln!("restee: notification failed ({e})"),
                    }
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
