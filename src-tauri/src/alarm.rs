//! Wall-clock alarm scheduler (the Tauri-side counterpart to the pure recurrence
//! logic in `gomaju_core::alarm`). The engine is clock-free, so anything that fires
//! at an absolute local time lives here.

use std::time::Duration;

use chrono::{DateTime, Datelike, Days, Local, NaiveDate, TimeZone, Timelike};
use tauri::{AppHandle, Manager};

use gomaju_core::alarm::{alarm_is_due, days_in_month, parse_hhmm, AlarmDto, RepeatDto};

use crate::app_state::{AppState, FiredAlarmToast};
use crate::audio;

/// The next local time this alarm will fire, or `None` if it can't (disabled, a past
/// one-time alarm, or an unparseable time).
///
/// Reuses the tested [`alarm_is_due`] matcher rather than duplicating recurrence rules:
/// for recurring alarms it scans forward day-by-day (≤ ~1 year covers every kind) and
/// returns the first matching day at the alarm's own clock time that is still ahead of
/// `now`. One-time alarms are resolved directly from their fixed date (which can be
/// arbitrarily far out). A strict `> now` skips an alarm that already fired this minute.
pub fn next_fire(a: &AlarmDto, now: DateTime<Local>) -> Option<DateTime<Local>> {
    if !a.enabled {
        return None;
    }
    let (ah, am) = parse_hhmm(&a.time)?;
    let at = |date: NaiveDate| -> Option<DateTime<Local>> {
        Local
            .with_ymd_and_hms(
                date.year(),
                date.month(),
                date.day(),
                ah as u32,
                am as u32,
                0,
            )
            .single()
    };

    if a.repeat == RepeatDto::Once {
        let date = NaiveDate::parse_from_str(a.date.as_deref()?, "%Y-%m-%d").ok()?;
        return at(date).filter(|when| *when > now);
    }

    let today = now.date_naive();
    for offset in 0u64..=366 {
        let date = today.checked_add_days(Days::new(offset))?;
        let due = alarm_is_due(
            a,
            ah,
            am,
            date.weekday().num_days_from_monday() as u8,
            date.month() as u8,
            date.day() as u8,
            days_in_month(date.year(), date.month() as u8),
            &date.format("%Y-%m-%d").to_string(),
        );
        if due {
            if let Some(when) = at(date) {
                if when > now {
                    return Some(when);
                }
            }
        }
    }
    None
}

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
            // Reconcile alarm toasts every second so a ✕ dismissal closes its window promptly (and a
            // toast from the previous second's fire shows). Window ops run from this background
            // thread — the safe pattern — never from the dismiss command.
            crate::alarm_toast::sync(&app);
            let now = Local::now();
            let stamp = (now.year(), now.month(), now.day(), now.hour(), now.minute());
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
            let (alarms, chimes): (Vec<AlarmDto>, Vec<gomaju_core::chime::ChimeDto>) = {
                let st = app.state::<AppState>();
                let alarms = {
                    let cfg = st.config.lock().unwrap();
                    cfg.alarms.clone()
                };
                let chimes = st.chimes.lock().unwrap().clone();
                (alarms, chimes)
            };

            // Show a persistent toast per fired alarm (this replaces the old auto-dismissing native
            // notification — see alarm_toast.rs), but play the tone at most once per minute so
            // several alarms at the same time don't overlap into a cacophony of audio streams. When
            // several fire together, the first one's chime wins (its assigned chime, or the default).
            let mut fired_chime: Option<(String, u8)> = None;
            for a in &alarms {
                if !alarm_is_due(a, hh, mm, weekday_mon0, month, day, dim, &ymd) {
                    continue;
                }
                crate::rlog!("gomaju: alarm fired ({})", a.name);
                {
                    let st = app.state::<AppState>();
                    st.fired_alarm_toasts.lock().unwrap().insert(
                        a.id.clone(),
                        FiredAlarmToast {
                            name: a.name.clone(),
                            time: a.time.clone(),
                            recurrence: recurrence_key(a.repeat),
                            fired_at: std::time::Instant::now(),
                        },
                    );
                }
                if fired_chime.is_none() {
                    fired_chime = Some((a.chime_id.clone(), a.chime_volume_pct));
                }
                if a.repeat == RepeatDto::Once {
                    disable_once(&app, &a.id);
                }
            }
            if let Some((chime_id, chime_volume_pct)) = fired_chime {
                let dir = {
                    let st = app.state::<AppState>();
                    st.config_path
                        .parent()
                        .map(|p| p.join("chimes"))
                        .unwrap_or_default()
                };
                audio::play_alarm_chime(&chime_id, chime_volume_pct, &chimes, &dir);
            }
            // Show any just-fired alarm toasts immediately, so the toast lands with the chime
            // rather than ~1s later on the next tick.
            crate::alarm_toast::sync(&app);
        }
    });
}

/// The lowercase recurrence key shown in the alarm toast, matching the frontend `alarms.repeat_*`
/// labels (the toast localizes it on the JS side).
fn recurrence_key(repeat: RepeatDto) -> &'static str {
    match repeat {
        RepeatDto::Once => "once",
        RepeatDto::Daily => "daily",
        RepeatDto::Weekly => "weekly",
        RepeatDto::Biweekly => "biweekly",
        RepeatDto::Monthly => "monthly",
        RepeatDto::Yearly => "yearly",
    }
}

/// Disable a fired one-time alarm so it never fires again (including across restarts).
/// Writes to disk first, then flips the flag in the live cache only on success — so a
/// failed write never leaves the cache claiming "disabled" while disk says "enabled".
fn disable_once(app: &AppHandle, id: &str) {
    let st = app.state::<AppState>();
    // Flip enabled=false on a clone, then write+swap under one held lock — so a concurrent
    // alarms-window save can't interleave a stale snapshot. Mirrors runtime::persist_rule_disabled.
    let result = st.with_config_write(|cur| match cur.alarms.iter_mut().find(|a| a.id == id) {
        Some(a) => {
            a.enabled = false;
            true
        }
        None => false, // already gone — nothing to disable
    });
    match result {
        Ok(Some(_)) => crate::rlog!("gomaju: once-alarm '{id}' disabled after firing"),
        Ok(None) => {}
        Err(e) => crate::rlog!("gomaju: failed to persist once-alarm disable ({e})"),
    }
}

#[cfg(test)]
mod tests {
    use super::next_fire;
    use chrono::{DateTime, Datelike, Local, TimeZone, Timelike};
    use gomaju_core::alarm::{AlarmDto, RepeatDto};

    fn local(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(y, mo, d, h, mi, 0)
            .single()
            .expect("valid local time")
    }

    fn alarm(repeat: RepeatDto, time: &str) -> AlarmDto {
        AlarmDto {
            id: "a".into(),
            name: "n".into(),
            time: time.into(),
            repeat,
            weekdays: vec![],
            day_of_month: 0,
            month: 0,
            date: None,
            enabled: true,
            chime_id: String::new(),
            chime_volume_pct: gomaju_core::alarm::default_chime_volume(),
        }
    }

    /// `(year, month, day, hour, minute)` of a fire instant, for terse assertions.
    fn parts(when: DateTime<Local>) -> (i32, u32, u32, u32, u32) {
        (
            when.year(),
            when.month(),
            when.day(),
            when.hour(),
            when.minute(),
        )
    }

    #[test]
    fn daily_fires_today_when_time_is_ahead() {
        let when = next_fire(&alarm(RepeatDto::Daily, "08:30"), local(2026, 6, 2, 7, 0)).unwrap();
        assert_eq!(parts(when), (2026, 6, 2, 8, 30));
    }

    #[test]
    fn daily_rolls_to_tomorrow_once_the_time_has_passed() {
        let when = next_fire(&alarm(RepeatDto::Daily, "08:30"), local(2026, 6, 2, 9, 0)).unwrap();
        assert_eq!(parts(when), (2026, 6, 3, 8, 30));
    }

    #[test]
    fn weekly_jumps_to_the_next_listed_weekday() {
        // 2026-06-02 is a Tuesday (weekday_mon0 = 1); ask for Friday (4).
        let mut a = alarm(RepeatDto::Weekly, "08:30");
        a.weekdays = vec![4];
        let when = next_fire(&a, local(2026, 6, 2, 9, 0)).unwrap();
        assert_eq!(parts(when), (2026, 6, 5, 8, 30)); // Fri 2026-06-05
    }

    #[test]
    fn disabled_alarm_has_no_next_fire() {
        let mut a = alarm(RepeatDto::Daily, "08:30");
        a.enabled = false;
        assert!(next_fire(&a, local(2026, 6, 2, 7, 0)).is_none());
    }

    #[test]
    fn once_in_the_past_has_no_next_fire() {
        let mut a = alarm(RepeatDto::Once, "08:30");
        a.date = Some("2020-01-01".into());
        assert!(next_fire(&a, local(2026, 6, 2, 7, 0)).is_none());
    }

    #[test]
    fn once_in_the_future_fires_on_its_date() {
        let mut a = alarm(RepeatDto::Once, "08:30");
        a.date = Some("2026-12-25".into());
        let when = next_fire(&a, local(2026, 6, 2, 7, 0)).unwrap();
        assert_eq!(parts(when), (2026, 12, 25, 8, 30));
    }

    #[test]
    fn biweekly_skips_the_off_week() {
        // Start week = Mon 2026-06-08; from Tue Jun 9 the next Monday (Jun 15) is an OFF
        // week, so the next fire is the following on-week Monday, Jun 22.
        let mut a = alarm(RepeatDto::Biweekly, "08:30");
        a.weekdays = vec![0]; // Mon
        a.date = Some("2026-06-08".into());
        let when = next_fire(&a, local(2026, 6, 9, 9, 0)).unwrap();
        assert_eq!(parts(when), (2026, 6, 22, 8, 30));
    }

    #[test]
    fn biweekly_does_not_fire_before_its_start_week() {
        // From Mon Jun 1 (before the start) the first fire is the start Monday, Jun 8.
        let mut a = alarm(RepeatDto::Biweekly, "08:30");
        a.weekdays = vec![0]; // Mon
        a.date = Some("2026-06-08".into());
        let when = next_fire(&a, local(2026, 6, 1, 7, 0)).unwrap();
        assert_eq!(parts(when), (2026, 6, 8, 8, 30));
    }
}
