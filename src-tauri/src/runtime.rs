use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use restee_core::{config, Effect, Enforcement, RunState};

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

                // Read presentation settings from the cached config before building the
                // overlay (the display mode is purely presentational — the engine never sees it).
                let (sound, notify, locale, break_display, note) = {
                    let state = app.state::<AppState>();
                    let cfg = state.config.lock().unwrap();
                    let note = cfg
                        .rules
                        .iter()
                        .find(|r| r.id == *rule_id)
                        .map(|r| r.note.clone())
                        .unwrap_or_default();
                    (
                        cfg.settings.sound,
                        cfg.settings.notifications,
                        cfg.locale.clone(),
                        cfg.settings.break_display.as_str().to_string(),
                        note,
                    )
                };

                overlay::show_break(
                    app,
                    BreakInfo {
                        kind: enforcement.as_str().into(),
                        name: name.clone(),
                        duration_secs: duration.as_secs(),
                        escape_mode: escape_mode.as_str().into(),
                        break_display,
                        note,
                    },
                );
                let _ = app.emit("break-start", rule_id.clone());

                if sound {
                    crate::audio::play_chime();
                }
                // Notifications augment soft breaks; strict breaks already take
                // over the whole screen, so a toast would be redundant.
                if notify && *enforcement == Enforcement::Soft {
                    let body = crate::i18n::tr(&locale, "notif.soft_break").replace("{name}", name);
                    show_notification(app, &body);
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
            Effect::EndBreak { completed, .. } => {
                overlay::close_all(app);
                let _ = app.emit("break-end", ());
                // A short cue when a break runs its full course (not on a manual skip), if the
                // user enabled break sounds — signals "you're done, back to work".
                if *completed {
                    let sound = {
                        let state = app.state::<AppState>();
                        let cfg = state.config.lock().unwrap();
                        cfg.settings.sound
                    };
                    if sound {
                        crate::audio::play_break_over();
                    }
                }
            }
            Effect::StateChanged(state) => {
                let _ = app.emit("state-changed", state.as_str());
                update_running_since(app, *state);
                refresh_tray(app, *state);
            }
            Effect::RuleReset { .. } => {}
            Effect::RuleDisabled { rule_id } => {
                persist_rule_disabled(app, rule_id);
            }
        }
    }
}

/// Persist a fired "once" rule as `enabled = false` so it doesn't re-arm on restart. The
/// `config` lock is held across the disk write so it serializes with the window save
/// commands — a concurrent Save can't interleave a stale snapshot. Save-first: the cache is
/// only committed after the write succeeds. Best-effort; the engine already disabled the
/// rule for this session, so a write failure only costs restart-survival.
fn persist_rule_disabled(app: &AppHandle, rule_id: &str) {
    let st = app.state::<AppState>();
    let mut guard = st.config.lock().unwrap();
    if !guard.rules.iter().any(|r| r.id == rule_id && r.enabled) {
        return; // already disabled / gone — nothing to persist
    }
    let mut snapshot = guard.clone();
    if let Some(r) = snapshot.rules.iter_mut().find(|r| r.id == rule_id) {
        r.enabled = false;
    }
    match config::save(&st.config_path, &snapshot) {
        Ok(()) => {
            *guard = snapshot;
            eprintln!("restee: once-rule '{rule_id}' disabled after firing");
        }
        Err(e) => eprintln!("restee: failed to persist once-rule disable ({e})"),
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

/// Ask the user to confirm before wiping the countdown, then reset on "Reset".
///
/// Shared by the tray "Reset timer" item and the Settings "Reset" button. Uses the
/// non-blocking callback form of the dialog so it is safe from the tray's main-thread
/// menu handler (a `blocking_show` there could deadlock the event loop) and from the
/// command thread alike. The state is re-fetched from the handle inside the callback,
/// since a borrowed `&AppState` can't outlive into the `'static` closure.
pub fn confirm_then_reset(app: &AppHandle) {
    let loc = crate::i18n::current_locale(app);
    let handle = app.clone();
    app.dialog()
        .message(crate::i18n::tr(&loc, "dialog.reset_timer_msg"))
        .title(crate::i18n::tr(&loc, "dialog.reset_timer_title"))
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            crate::i18n::tr(&loc, "dialog.reset").to_string(),
            crate::i18n::tr(&loc, "dialog.cancel").to_string(),
        ))
        .show(move |confirmed| {
            if confirmed {
                let state = handle.state::<AppState>();
                action_reset(&handle, state.inner());
            } else {
                eprintln!("restee: timer reset cancelled");
            }
        });
}

/// Per-break variant of [`confirm_then_reset`]: confirm, then restart just one rule's
/// countdown. Names the break in the dialog. No-op if the rule id is gone.
pub fn confirm_then_reset_one(app: &AppHandle, rule_id: String) {
    let (name, loc) = {
        let st = app.state::<AppState>();
        let cfg = st.config.lock().unwrap();
        match cfg.rules.iter().find(|r| r.id == rule_id) {
            Some(r) => (r.name.clone(), cfg.locale.clone()),
            None => return,
        }
    };
    let handle = app.clone();
    app.dialog()
        .message(crate::i18n::tr(&loc, "dialog.reset_break_msg").replace("{name}", &name))
        .title(crate::i18n::tr(&loc, "dialog.reset_break_title"))
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            crate::i18n::tr(&loc, "dialog.reset").to_string(),
            crate::i18n::tr(&loc, "dialog.cancel").to_string(),
        ))
        .show(move |confirmed| {
            if confirmed {
                let st = handle.state::<AppState>();
                let fx = st.engine.lock().unwrap().reset_timer(&rule_id);
                apply_effects(&handle, &fx);
            } else {
                eprintln!("restee: break reset cancelled ({rule_id})");
            }
        });
}

/// Switch the UI language (from the tray): persist it to config, then relabel the tray
/// immediately. Open windows pick up the new language when reopened (the locale is injected
/// at window creation), so no window event is emitted.
pub fn set_locale(app: &AppHandle, code: &str) {
    {
        let st = app.state::<AppState>();
        let mut guard = st.config.lock().unwrap();
        if guard.locale == code {
            return; // already this language
        }
        let mut snapshot = guard.clone();
        snapshot.locale = code.to_string();
        if let Err(e) = config::save(&st.config_path, &snapshot) {
            eprintln!("restee: failed to persist locale ({e})");
            return;
        }
        *guard = snapshot;
        eprintln!("restee: locale set to {code}");
    }
    // Read the run state under the engine lock and drop it before refresh_tray (which
    // re-locks the engine — don't nest). Clear the tray cache + update the tooltip first.
    let state = app.state::<AppState>().engine.lock().unwrap().state();
    crate::tray::invalidate_for_locale(app, code);
    refresh_tray(app, state);
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

/// Update the tray Start/Pause checks, elapsed text, and "next break in …" info line.
pub fn refresh_tray(app: &AppHandle, state: RunState) {
    let st = app.state::<AppState>();
    let running_secs = st
        .running_since
        .lock()
        .unwrap()
        .map(|since| since.elapsed().as_secs())
        .unwrap_or(0);
    // Re-lock the engine (released by callers before refresh) for the per-rule countdowns.
    let breaks: Vec<(String, u64)> = st
        .engine
        .lock()
        .unwrap()
        .status()
        .all
        .into_iter()
        .map(|n| (n.rule_name, n.remaining_secs))
        .collect();
    crate::tray::refresh(app, state, running_secs, breaks, todays_upcoming_alarms(&st));
}

/// Enabled alarms whose next fire is still ahead **today**, as `(name, "HH:MM")` soonest
/// first. Reuses the tested `alarm::next_fire` and keeps only fires landing on today's date.
fn todays_upcoming_alarms(st: &AppState) -> Vec<(String, String)> {
    use chrono::{Local, Timelike};
    let now = Local::now();
    let today = now.date_naive();
    let mut list: Vec<(u32, String, String)> = st
        .config
        .lock()
        .unwrap()
        .alarms
        .iter()
        .filter_map(|a| {
            let when = crate::alarm::next_fire(a, now)?;
            (when.date_naive() == today).then(|| {
                (
                    when.hour() * 60 + when.minute(),
                    a.name.clone(),
                    when.format("%H:%M").to_string(),
                )
            })
        })
        .collect();
    list.sort_by_key(|(mins, _, _)| *mins);
    list.into_iter().map(|(_, name, time)| (name, time)).collect()
}

/// Show an OS notification titled "restee" with `body`. Best-effort: on Windows the
/// toast renders reliably only once the app is installed (has an app identity).
pub fn show_notification(app: &AppHandle, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    match app.notification().builder().title("Restee").body(body).show() {
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
