//! Wall-clock alarm scheduler (the Tauri-side counterpart to the pure recurrence
//! logic in `restee_core::alarm`). The engine is clock-free, so anything that fires
//! at an absolute local time lives here.

use std::time::Duration;

use chrono::{Datelike, Local, Timelike};
use tauri::{AppHandle, Manager};

use restee_core::alarm::{alarm_is_due, days_in_month, AlarmDto, RepeatDto};

use crate::app_state::AppState;
use crate::{audio, runtime};

/// Spawn the alarm scheduler on a dedicated 1-second thread.
///
/// It is **edge-triggered on the wall-clock minute**: an alarm fires exactly once
/// when the clock *enters* a minute it matches. Alarms fire regardless of run state
/// (paused / mid-break). Minutes missed because the process was suspended or closed
/// are skipped — there is no catch-up. The first tick only seeds the current minute,
/// so a partial minute at startup never retro-fires.
pub fn spawn_scheduler(app: AppHandle) {
    std::thread::spawn(move || {
        // Track the last minute we evaluated as date+time components — cheaper and more
        // robust than formatting/parsing a string every second.
        let mut last_min: Option<(i32, u32, u32, u32, u32)> = None;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let now = Local::now();
            let stamp = (
                now.year(),
                now.month(),
                now.day(),
                now.hour(),
                now.minute(),
            );
            if last_min == Some(stamp) {
                continue; // still inside a minute we already handled
            }
            let first_tick = last_min.is_none();
            last_min = Some(stamp);
            if first_tick {
                continue; // seed only — don't retro-fire the startup partial minute
            }

            let hh = now.hour() as u8;
            let mm = now.minute() as u8;
            let weekday_mon0 = now.weekday().num_days_from_monday() as u8;
            let month = now.month() as u8;
            let day = now.day() as u8;
            let dim = days_in_month(now.year(), month);
            let ymd = now.format("%Y-%m-%d").to_string();

            // Snapshot under lock, then release it before running side effects.
            let alarms: Vec<AlarmDto> = {
                let st = app.state::<AppState>();
                let cfg = st.config.lock().unwrap();
                cfg.alarms.clone()
            };

            // Notify per alarm (names are distinct + informative), but play the tone at
            // most once per minute so several alarms at the same time don't overlap into
            // a cacophony of audio streams.
            let mut any_fired = false;
            for a in &alarms {
                if !alarm_is_due(a, hh, mm, weekday_mon0, month, day, dim, &ymd) {
                    continue;
                }
                eprintln!("restee: alarm fired ({})", a.name);
                runtime::show_notification(&app, &a.name);
                any_fired = true;
                if a.repeat == RepeatDto::Once {
                    disable_once(&app, &a.id);
                }
            }
            if any_fired {
                audio::play_alarm();
            }
        }
    });
}

/// Disable a fired one-time alarm so it never fires again (including across restarts).
/// Writes to disk first, then flips the flag in the live cache only on success — so a
/// failed write never leaves the cache claiming "disabled" while disk says "enabled".
fn disable_once(app: &AppHandle, id: &str) {
    let st = app.state::<AppState>();

    // Build the snapshot to persist (clone with the flag flipped) without touching the cache.
    let to_save = {
        let cfg = st.config.lock().unwrap();
        let mut clone = cfg.clone();
        match clone.alarms.iter_mut().find(|a| a.id == id) {
            Some(a) => a.enabled = false,
            None => return, // already gone — nothing to disable
        }
        clone
    };

    if let Err(e) = restee_core::config::save(&st.config_path, &to_save) {
        eprintln!("restee: failed to persist once-alarm disable ({e})");
        return; // leave the cache untouched so it stays consistent with disk
    }

    // Disk write succeeded — flip the flag in the live cache (re-find by id so a concurrent
    // alarms-window save isn't clobbered).
    if let Some(a) = st.config.lock().unwrap().alarms.iter_mut().find(|a| a.id == id) {
        a.enabled = false;
    }
    eprintln!("restee: once-alarm '{id}' disabled after firing");
}
