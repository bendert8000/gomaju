use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use gomaju_core::{config, Effect, Enforcement, RunState};

use crate::app_state::AppState;
use crate::idle::IdleSource;
use crate::overlay::{self, BreakInfo};
use crate::toast::{self, WarningInfo};

#[derive(Clone, Serialize)]
struct BreakTickPayload {
    rule_id: String,
    remaining_secs: u64,
}

/// How often the ticker autosaves break progress to `session.toml` (seconds). Frequent enough
/// that a forced/ungraceful kill (e.g. a Windows-Update reboot) loses at most this much progress,
/// coarse enough to stay cheap.
const PROGRESS_SAVE_INTERVAL_SECS: u64 = 60;

/// Spawn the once-per-second ticker on a dedicated OS thread (no async runtime
/// needed). It reads idle time, advances the engine, and applies the effects.
pub fn spawn_ticker(app: AppHandle, idle: Box<dyn IdleSource>) {
    std::thread::spawn(move || {
        let mut last = Instant::now();
        let mut last_save = Instant::now();
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
            maybe_show_pause_reminder(&app, state);
            // Periodically persist break progress. Runs in every state (incl. InBreak, so the
            // post-fire_break reset is captured) — the lock is already released above, so the
            // snapshot-then-write helper can re-lock safely.
            if last_save.elapsed() >= Duration::from_secs(PROGRESS_SAVE_INTERVAL_SECS) {
                persist_progress(&app);
                last_save = Instant::now();
            }
        }
    });
}

/// Snapshot per-rule break progress and write it to `session.toml`. Best-effort: a failed write is
/// logged, not fatal. Takes the engine lock only to snapshot, then releases it **before** the disk
/// I/O so it never blocks the ticker/commands or self-deadlocks the non-reentrant engine mutex.
pub fn persist_progress(app: &AppHandle) {
    let st = app.state::<AppState>();
    let rules = st.engine.lock().unwrap().snapshot_progress();
    let file = gomaju_core::progress::ProgressFile {
        version: gomaju_core::progress::PROGRESS_VERSION,
        saved_at: chrono::Utc::now().timestamp(),
        rules,
    };
    if let Err(e) = gomaju_core::progress::save_progress(&st.session_path, &file) {
        crate::rlog!("gomaju: failed to persist break progress ({e})");
    }
}

/// Interpret engine effects: emit events for the UI and create/destroy overlays.
pub fn apply_effects(app: &AppHandle, effects: &[Effect]) {
    for effect in effects {
        match effect {
            Effect::BreakWarning {
                rule_id,
                name,
                enforcement,
                lead_secs,
            } => {
                toast::show(
                    app,
                    WarningInfo {
                        rule_id: rule_id.clone(),
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
                let (notify, locale, break_display, note, quote) = {
                    let state = app.state::<AppState>();
                    let cfg = state.config.lock().unwrap();
                    let rule = cfg.rules.iter().find(|r| r.id == *rule_id);
                    let note = rule.map(|r| r.note.clone()).unwrap_or_default();
                    // Inspirational quote from the active locale's quotes file (next to
                    // config.toml), picked per break when enabled. Best-effort: no/empty file ->
                    // empty string (no cross-locale fallback).
                    let quote = if cfg.settings.show_quotes {
                        crate::quotes::pick(&state.quotes_path, &cfg.locale).unwrap_or_default()
                    } else {
                        String::new()
                    };
                    // Break-start cue: the rule's assigned chime, or the built-in default. Played
                    // from inside the lock — the audio fns spawn a thread and return immediately,
                    // so the config lock is held only for the spawn, not for playback.
                    if cfg.settings.sound {
                        let chime_id = rule.map(|r| r.chime_id.as_str()).unwrap_or("");
                        let chime_volume_pct = rule
                            .map(|r| r.chime_volume_pct)
                            .unwrap_or_else(config::default_chime_volume);
                        let dir = state
                            .config_path
                            .parent()
                            .map(|p| p.join("chimes"))
                            .unwrap_or_default();
                        let chimes = state.chimes.lock().unwrap();
                        crate::audio::play_break_chime(chime_id, chime_volume_pct, &chimes, &dir);
                    }
                    (
                        cfg.settings.notifications,
                        cfg.locale.clone(),
                        cfg.settings.break_display.as_str().to_string(),
                        note,
                        quote,
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
                        quote,
                    },
                );
                let _ = app.emit("break-start", rule_id.clone());

                // Notifications augment soft breaks; strict breaks already take
                // over the whole screen, so a toast would be redundant.
                if notify && *enforcement == Enforcement::Soft {
                    let body = crate::i18n::tr(&locale, "notif.soft_break").replace("{name}", name);
                    show_notification(app, crate::i18n::tr(&locale, "notif.break_title"), &body);
                }
            }
            Effect::BreakTick { rule_id, remaining } => {
                let _ = app.emit(
                    "break-tick",
                    BreakTickPayload {
                        rule_id: rule_id.clone(),
                        remaining_secs: remaining.as_secs(),
                    },
                );
            }
            Effect::EndBreak { rule_id, completed } => {
                overlay::close_all(app);
                let _ = app.emit("break-end", ());
                // A short cue when a break runs its full course (not on a manual skip), if the
                // user enabled break sounds — signals "you're done, back to work". Uses the rule's
                // assigned end chime, falling back to the default break-over tone.
                if *completed {
                    let state = app.state::<AppState>();
                    let (sound, end_chime_id, end_chime_volume_pct) = {
                        let cfg = state.config.lock().unwrap();
                        let rule = cfg.rules.iter().find(|r| r.id == *rule_id);
                        let end = rule.map(|r| r.end_chime_id.clone()).unwrap_or_default();
                        let volume = rule
                            .map(|r| r.end_chime_volume_pct)
                            .unwrap_or_else(config::default_chime_volume);
                        (cfg.settings.sound, end, volume)
                    };
                    if sound {
                        let dir = state
                            .config_path
                            .parent()
                            .map(|p| p.join("chimes"))
                            .unwrap_or_default();
                        let chimes = state.chimes.lock().unwrap();
                        crate::audio::play_break_over_chime(
                            &end_chime_id,
                            end_chime_volume_pct,
                            &chimes,
                            &dir,
                        );
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

    // A break just started: `fire_break` reset the fired (+ shorter/co-due) rules' work before
    // entering InBreak. Persist immediately (the engine lock is already released by callers) so a
    // forced reboot mid-break resumes the post-reset state and won't re-fire the same break,
    // rather than the stale pre-break snapshot the 60s autosave might not have caught yet.
    if effects.iter().any(|e| matches!(e, Effect::StartBreak { .. })) {
        persist_progress(app);
    }
}

/// Persist a fired "once" rule as `enabled = false` so it doesn't re-arm on restart. The
/// `config` lock is held across the disk write so it serializes with the window save
/// commands — a concurrent Save can't interleave a stale snapshot. Save-first: the cache is
/// only committed after the write succeeds. Best-effort; the engine already disabled the
/// rule for this session, so a write failure only costs restart-survival.
fn persist_rule_disabled(app: &AppHandle, rule_id: &str) {
    let st = app.state::<AppState>();
    let result = st.with_config_write(|cur| {
        match cur.rules.iter_mut().find(|r| r.id == rule_id && r.enabled) {
            Some(r) => {
                r.enabled = false;
                true
            }
            None => false, // already disabled / gone — nothing to persist
        }
    });
    match result {
        Ok(Some(_)) => crate::rlog!("gomaju: once-rule '{rule_id}' disabled after firing"),
        Ok(None) => {}
        Err(e) => crate::rlog!("gomaju: failed to persist once-rule disable ({e})"),
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

/// Take a **specific** rule's break immediately (the tray "click a break line" action).
pub fn action_break_now_rule(app: &AppHandle, state: &AppState, rule_id: &str) {
    let fx = state.engine.lock().unwrap().break_now_rule(rule_id);
    apply_effects(app, &fx);
}

pub fn action_reset(app: &AppHandle, state: &AppState) {
    let fx = state.engine.lock().unwrap().reset_timers();
    apply_effects(app, &fx);
}

/// A native-dialog title prefixed with the app name, so an OS popup is identifiable as Gomaju's
/// (matches the "Gomaju — …" window-title convention).
fn gomaju_dialog_title(loc: &str, key: &str) -> String {
    format!("Gomaju — {}", crate::i18n::tr(loc, key))
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
        .title(gomaju_dialog_title(&loc, "dialog.reset_timer_title"))
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
                crate::rlog!("gomaju: timer reset cancelled");
            }
        });
}

/// Cold-start prompt: a recent saved break-progress snapshot exists — ask whether to resume it or
/// start every countdown from zero. Non-blocking (callback form), like `confirm_then_reset`. The
/// engine is left **Stopped** until the user answers; the callback no-ops if the user already
/// started it via the tray, so it never clobbers their action.
pub fn confirm_resume_break_progress(app: &AppHandle, age_secs: u64) {
    let loc = crate::i18n::current_locale(app);
    let age = crate::i18n::human_dur(&loc, age_secs);
    let handle = app.clone();
    app.dialog()
        .message(crate::i18n::tr(&loc, "dialog.resume_progress_msg").replace("{age}", &age))
        .title(gomaju_dialog_title(&loc, "dialog.resume_progress_title"))
        .kind(MessageDialogKind::Info)
        // OK / default (Enter) = Resume (the safe, non-destructive path); the other button —
        // and Esc/close, which a bool dialog can't distinguish — = Start fresh.
        .buttons(MessageDialogButtons::OkCancelCustom(
            crate::i18n::tr(&loc, "dialog.resume").to_string(),
            crate::i18n::tr(&loc, "dialog.start_fresh").to_string(),
        ))
        .show(move |resume| {
            let st = handle.state::<AppState>();
            // If the user already started/changed state from the tray, don't override them; just
            // make sure their current progress is on disk.
            let already_acted = st.engine.lock().unwrap().state() != RunState::Stopped;
            if already_acted {
                persist_progress(&handle);
                return;
            }
            if resume {
                // Keep the restored work; start() emits StateChanged so apply_effects updates
                // running_since, the state-changed event, pause reminders, and the tray.
                action_start(&handle, st.inner());
            } else {
                action_reset(&handle, st.inner()); // zero every rule's work
                action_start(&handle, st.inner());
                persist_progress(&handle); // overwrite session.toml now so a crash can't re-prompt
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
        .title(gomaju_dialog_title(&loc, "dialog.reset_break_title"))
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
                crate::rlog!("gomaju: break reset cancelled ({rule_id})");
            }
        });
}

/// Tray "take this break" prompt: confirm, then immediately start just this rule's break.
/// Names the break in the dialog. Mirrors [`confirm_then_reset_one`] (non-blocking callback
/// form, safe from the tray's main-thread menu handler). No-op if the rule id is gone.
pub fn confirm_then_break_one(app: &AppHandle, rule_id: String) {
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
        .message(crate::i18n::tr(&loc, "dialog.break_now_msg").replace("{name}", &name))
        .title(gomaju_dialog_title(&loc, "dialog.break_now_title"))
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            crate::i18n::tr(&loc, "dialog.break_now_ok").to_string(),
            crate::i18n::tr(&loc, "dialog.cancel").to_string(),
        ))
        .show(move |confirmed| {
            if confirmed {
                let st = handle.state::<AppState>();
                action_break_now_rule(&handle, st.inner(), &rule_id);
            } else {
                crate::rlog!("gomaju: take-break cancelled ({rule_id})");
            }
        });
}

/// Switch the UI language (from the tray): persist it to config, then relabel the tray
/// immediately. Open windows pick up the new language when reopened (the locale is injected
/// at window creation), so no window event is emitted.
pub fn set_locale(app: &AppHandle, code: &str) {
    let st = app.state::<AppState>();
    match st.with_config_write(|cur| {
        if cur.locale == code {
            return false; // already this language
        }
        cur.locale = code.to_string();
        true
    }) {
        Ok(Some(_)) => crate::rlog!("gomaju: locale set to {code}"),
        Ok(None) => return, // unchanged
        Err(e) => {
            crate::rlog!("gomaju: failed to persist locale ({e})");
            return;
        }
    }
    // Read the run state under the engine lock and drop it before refresh_tray (which
    // re-locks the engine — don't nest). Clear the tray cache + update the tooltip first.
    let state = st.engine.lock().unwrap().state();
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
    sync_pause_reminder_for_state(app, state);
}

pub fn sync_pause_reminder(app: &AppHandle) {
    let state = app.state::<AppState>().engine.lock().unwrap().state();
    sync_pause_reminder_for_state(app, state);
}

fn sync_pause_reminder_for_state(app: &AppHandle, state: RunState) {
    match state {
        RunState::Paused => arm_pause_reminder_from_now(app),
        RunState::Stopped | RunState::Running | RunState::InBreak => clear_pause_reminder(app),
    }
}

fn arm_pause_reminder_from_now(app: &AppHandle) {
    let st = app.state::<AppState>();
    let (enabled, interval) = pause_reminder_config(st.inner());
    crate::pause_toast::close(app);
    let mut reminder = st.pause_reminder.lock().unwrap();
    reminder.generation = reminder.generation.wrapping_add(1);
    reminder.prompt_open = false;
    reminder.next_due = enabled.then(|| Instant::now() + interval);
}

fn clear_pause_reminder(app: &AppHandle) {
    let st = app.state::<AppState>();
    let mut reminder = st.pause_reminder.lock().unwrap();
    if reminder.next_due.is_some() || reminder.prompt_open {
        reminder.generation = reminder.generation.wrapping_add(1);
    }
    reminder.next_due = None;
    reminder.prompt_open = false;
    crate::pause_toast::close(app);
}

fn pause_reminder_config(st: &AppState) -> (bool, Duration) {
    let cfg = st.config.lock().unwrap();
    (
        cfg.settings.pause_reminder_enabled,
        Duration::from_secs(cfg.settings.pause_reminder_interval_secs.max(60)),
    )
}

fn maybe_show_pause_reminder(app: &AppHandle, state: RunState) {
    if state != RunState::Paused {
        return;
    }

    let st = app.state::<AppState>();
    let (enabled, interval) = pause_reminder_config(st.inner());
    if !enabled {
        clear_pause_reminder(app);
        return;
    }

    let now = Instant::now();
    let show = {
        let mut reminder = st.pause_reminder.lock().unwrap();
        if reminder.prompt_open {
            return;
        }
        let due = *reminder.next_due.get_or_insert(now + interval);
        if now < due {
            return;
        }
        reminder.prompt_open = true;
        true
    };

    if show {
        crate::pause_toast::show(app);
    }
}

pub fn resume_from_pause_reminder(app: &AppHandle, st: &AppState) {
    crate::pause_toast::close(app);
    let state = st.engine.lock().unwrap().state();
    if state == RunState::Paused {
        action_start(app, st);
    } else {
        clear_pause_reminder(app);
    }
}

pub fn stay_paused_from_reminder(app: &AppHandle, st: &AppState) {
    crate::pause_toast::close(app);
    if st.engine.lock().unwrap().state() != RunState::Paused {
        clear_pause_reminder(app);
        return;
    }
    let (enabled, interval) = pause_reminder_config(st);
    let mut reminder = st.pause_reminder.lock().unwrap();
    reminder.prompt_open = false;
    reminder.next_due = enabled.then(|| Instant::now() + interval);
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
    // Carry the rule_id alongside name + remaining so the tray can make each line a clickable
    // "take this break" item (routed by id back to confirm_then_break_one).
    let breaks: Vec<(String, String, u64)> = st
        .engine
        .lock()
        .unwrap()
        .status()
        .all
        .into_iter()
        .map(|n| (n.rule_id, n.rule_name, n.remaining_secs))
        .collect();
    crate::tray::refresh(
        app,
        state,
        running_secs,
        breaks,
        todays_upcoming_alarms(&st),
    );
}

/// Enabled alarms whose next fire is still ahead **today**, as `(name, "HH:MM", secs_until)`
/// soonest first. Reuses the tested `alarm::next_fire` and keeps only fires landing on today's
/// date; the seconds-until-fire drive the tray's live (minute-granular) countdown.
fn todays_upcoming_alarms(st: &AppState) -> Vec<(String, String, u64)> {
    use chrono::Local;
    let now = Local::now();
    let today = now.date_naive();
    let mut list: Vec<(u64, String, String)> = st
        .config
        .lock()
        .unwrap()
        .alarms
        .iter()
        .filter_map(|a| {
            let when = crate::alarm::next_fire(a, now)?;
            (when.date_naive() == today).then(|| {
                // next_fire guarantees when > now; max(0) defends against a 1s clock advance.
                let secs = (when - now).num_seconds().max(0) as u64;
                (secs, a.name.clone(), when.format("%H:%M").to_string())
            })
        })
        .collect();
    list.sort_by_key(|(secs, _, _)| *secs);
    list.into_iter()
        .map(|(secs, name, time)| (name, time, secs))
        .collect()
}

/// Show an OS notification with `title` (always brand-led — e.g. "Gomaju · Break reminder" — so
/// it's recognizable among other system notifications) and `body`. Best-effort: on Windows the
/// toast renders reliably only once the app is installed (has an app identity).
pub fn show_notification(app: &AppHandle, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    match app
        .notification()
        .builder()
        .title(title)
        .body(body)
        .show()
    {
        Ok(_) => crate::rlog!("gomaju: notification shown ({title}: {body})"),
        Err(e) => crate::rlog!("gomaju: notification failed ({e})"),
    }
}

/// Show the startup tray-reminder notification, auto-dismissed after ~3s.
///
/// On Windows we drive the WinRT toast directly so we can `Hide` it after 3s — the
/// notification plugin offers no control over toast lifetime, and Windows won't show
/// a banner for less than the OS minimum (~5s). `Hide` removes the banner *and* the
/// Action Center entry, so the message doesn't linger. Other platforms (and any
/// WinRT failure) fall back to the standard plugin notification.
pub fn show_startup_notification(app: &AppHandle, body: &str) {
    #[cfg(windows)]
    {
        match win_startup_toast(app, body) {
            Ok(()) => return,
            Err(e) => crate::rlog!("gomaju: WinRT startup toast failed ({e}); using plugin"),
        }
    }
    show_notification(app, "Gomaju", body);
}

/// Drive the Windows toast directly and schedule its dismissal ~3s later.
#[cfg(windows)]
fn win_startup_toast(app: &AppHandle, body: &str) -> windows::core::Result<()> {
    use windows::core::HSTRING;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

    let app_id = win_toast_app_id(app);

    // Minimal ToastGeneric payload: title "Gomaju" plus the body line.
    let xml = format!(
        "<toast duration=\"short\"><visual><binding template=\"ToastGeneric\">\
         <text>Gomaju</text><text>{}</text></binding></visual></toast>",
        xml_escape(body)
    );

    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(xml))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(&app_id))?;
    notifier.Show(&toast)?;
    crate::rlog!("gomaju: startup toast shown ({body}); auto-dismiss in 3s");

    // Remove it after ~3s so it doesn't linger on screen or in the Action Center.
    // WinRT toast objects are agile (Send + Sync); hide on the main thread to keep
    // COM usage on the thread Tauri initialized.
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(3));
        let _ = app.run_on_main_thread(move || {
            if let Err(e) = notifier.Hide(&toast) {
                crate::rlog!("gomaju: startup toast hide failed ({e})");
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
