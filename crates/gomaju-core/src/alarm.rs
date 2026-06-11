//! Clock alarms: TOML-friendly DTOs plus a pure, clock-free recurrence matcher.
//!
//! Like the rest of `gomaju-core`, this module has no clock or OS dependency: the
//! host (the Tauri layer) reads the current local time, extracts its components, and
//! asks [`alarm_is_due`] whether a given alarm should fire this minute. That keeps the
//! recurrence logic fully unit-testable in isolation.
//!
//! Field/kind matrix — only the fields listed for a `repeat` kind are meaningful;
//! the rest are ignored (and left untouched by [`sanitize_alarms`]):
//! - `Once`     -> `date` ("YYYY-MM-DD")
//! - `Daily`    -> (none)
//! - `Weekly`   -> `weekdays` (0=Mon … 6=Sun)
//! - `Biweekly` -> `weekdays` (0=Mon … 6=Sun) + `date` (start week "YYYY-MM-DD"): fires the ticked days every OTHER Monday-aligned week, counting from the start week
//! - `Monthly`  -> `day_of_month` (1..31; a value past the month length fires on its last day, so "31" means end-of-month)
//! - `Yearly`   -> `month` (1..12) + `day_of_month` (Feb-29 fires Feb-28 in common years)

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepeatDto {
    Once,
    Daily,
    Weekly,
    Biweekly,
    Monthly,
    Yearly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlarmDto {
    pub id: String,
    pub name: String,
    /// 24-hour "HH:MM".
    pub time: String,
    pub repeat: RepeatDto,
    /// Weekly / Biweekly only: 0=Mon … 6=Sun.
    #[serde(default)]
    pub weekdays: Vec<u8>,
    /// Monthly / Yearly only: 1..31.
    #[serde(default)]
    pub day_of_month: u8,
    /// Yearly only: 1..12.
    #[serde(default)]
    pub month: u8,
    /// Once: the single fire date. Biweekly: the start ("anchor") week. "YYYY-MM-DD".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Optional id of a saved chime to play when this alarm fires (empty = the default tone).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub chime_id: String,
    /// Volume for this alarm's selected/default chime.
    #[serde(
        default = "default_chime_volume",
        skip_serializing_if = "is_default_chime_volume"
    )]
    pub chime_volume_pct: u8,
}

fn default_true() -> bool {
    true
}

pub fn default_chime_volume() -> u8 {
    20
}

fn is_default_chime_volume(v: &u8) -> bool {
    *v == default_chime_volume()
}

/// Parse a 24-hour "HH:MM" string into `(hour, minute)` if well-formed.
pub fn parse_hhmm(s: &str) -> Option<(u8, u8)> {
    let (h, m) = s.split_once(':')?;
    if h.len() != 2 || m.len() != 2 {
        return None;
    }
    let h: u8 = h.parse().ok()?;
    let m: u8 = m.parse().ok()?;
    (h < 24 && m < 60).then_some((h, m))
}

/// Proleptic Gregorian leap-year rule.
fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Number of days in the given (year, month). `month` is 1..=12; out-of-range months
/// return 30 defensively (callers always pass a real month). Kept here, chrono-free,
/// so both the recurrence matcher and the host scheduler share one tested definition.
pub fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// Parse "YYYY-MM-DD" into `(year, month, day)` if the shape is right.
fn parse_ymd(s: &str) -> Option<(i32, u8, u8)> {
    let p: Vec<&str> = s.split('-').collect();
    if p.len() != 3 || p[0].len() != 4 || p[1].len() != 2 || p[2].len() != 2 {
        return None;
    }
    Some((
        p[0].parse::<i32>().ok()?,
        p[1].parse::<u8>().ok()?,
        p[2].parse::<u8>().ok()?,
    ))
}

/// Whether `s` is a real calendar date "YYYY-MM-DD" (rejects e.g. 2026-02-30).
fn is_real_ymd(s: &str) -> bool {
    match parse_ymd(s) {
        Some((y, mo, d)) => y >= 0 && (1..=12).contains(&mo) && d >= 1 && d <= days_in_month(y, mo),
        None => false,
    }
}

/// Days since 1970-01-01 (proleptic Gregorian). Pure integer math — no chrono — so the
/// bi-weekly week-parity check stays unit-testable in isolation. `month` is 1..=12.
/// (Howard Hinnant's `days_from_civil` algorithm.)
fn days_from_civil(y: i32, m: u8, d: u8) -> i64 {
    let m = m as i64;
    let d = d as i64;
    let y = (y as i64) - i64::from(m <= 2);
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400; // [0, 399]
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

/// Monday-aligned absolute week index for a day-count (days since 1970-01-01). 1970-01-05
/// (a Monday) has count 4, so Mondays satisfy `(count - 4) % 7 == 0`. `div_euclid` keeps the
/// index correct for pre-1970 (negative) counts.
fn monday_week(ord: i64) -> i64 {
    (ord - 4).div_euclid(7)
}

/// Whether `a` should fire given the current local-time components. Pure and
/// clock-free: the caller supplies the components (e.g. via `chrono::Local::now()`).
#[allow(clippy::too_many_arguments)]
pub fn alarm_is_due(
    a: &AlarmDto,
    hh: u8,
    mm: u8,
    weekday_mon0: u8,
    month: u8,
    day: u8,
    days_in_month: u8,
    ymd: &str,
) -> bool {
    if !a.enabled {
        return false;
    }
    // Must match to the minute.
    match parse_hhmm(&a.time) {
        Some((h, m)) if h == hh && m == mm => {}
        _ => return false,
    }
    // Day-of-month with end-of-month clamp: a target past the month length fires on
    // the month's last day (so "31" fires on Feb 28/29, Apr 30, etc.).
    let dom_matches =
        |target: u8| target == day || (target > days_in_month && day == days_in_month);
    match a.repeat {
        RepeatDto::Once => a.date.as_deref() == Some(ymd),
        RepeatDto::Daily => true,
        RepeatDto::Weekly => a.weekdays.contains(&weekday_mon0),
        RepeatDto::Biweekly => {
            // Same day-of-week selection as Weekly, but only on every other Monday-aligned
            // week, counting from the start date's week. Never fires before the start date.
            if !a.weekdays.contains(&weekday_mon0) {
                return false;
            }
            let cur = match parse_ymd(ymd) {
                Some((y, m, d)) => days_from_civil(y, m, d),
                None => return false,
            };
            let anc = match a.date.as_deref().and_then(parse_ymd) {
                Some((y, m, d)) => days_from_civil(y, m, d),
                None => return false,
            };
            cur >= anc && (monday_week(cur) - monday_week(anc)).rem_euclid(2) == 0
        }
        RepeatDto::Monthly => dom_matches(a.day_of_month),
        RepeatDto::Yearly => month == a.month && dom_matches(a.day_of_month),
    }
}

/// Clamp `*v` into `[lo, hi]`, flagging `changed` if it moved.
fn clamp(v: &mut u8, lo: u8, hi: u8, changed: &mut bool) {
    let c = (*v).clamp(lo, hi);
    if c != *v {
        *v = c;
        *changed = true;
    }
}

/// Normalize a weekday list in place (drop out-of-range, sort, dedup). Returns whether it
/// changed. Shared by the Weekly and Biweekly sanitize arms.
fn normalize_weekdays(weekdays: &mut Vec<u8>) -> bool {
    let before = weekdays.clone();
    weekdays.retain(|d| *d <= 6);
    weekdays.sort_unstable();
    weekdays.dedup();
    *weekdays != before
}

/// Validate/normalize alarms in place; returns true if anything changed (so the
/// caller can persist). Only fields relevant to each `repeat` kind are touched, so a
/// daily alarm's stray `day_of_month` is never mutated. Blank/duplicate ids are
/// regenerated to keep each alarm's identity unique (the scheduler keys on it).
pub fn sanitize_alarms(alarms: &mut [AlarmDto]) -> bool {
    let mut changed = false;

    // Ids that already exist (so generated ones never collide with a later original).
    let originals: HashSet<String> = alarms
        .iter()
        .map(|a| a.id.clone())
        .filter(|s| !s.trim().is_empty())
        .collect();
    let mut used: HashSet<String> = HashSet::new();
    let mut counter = 0u32;

    for a in alarms.iter_mut() {
        // id: non-blank and unique across the list.
        if a.id.trim().is_empty() || used.contains(&a.id) {
            let new_id = loop {
                counter += 1;
                let candidate = format!("alarm-{counter}");
                if !used.contains(&candidate) && !originals.contains(&candidate) {
                    break candidate;
                }
            };
            a.id = new_id;
            changed = true;
        }
        used.insert(a.id.clone());

        // time
        if parse_hhmm(&a.time).is_none() {
            a.time = "08:00".to_string();
            changed = true;
        }
        clamp(&mut a.chime_volume_pct, 0, 100, &mut changed);

        // per-kind normalization
        match a.repeat {
            RepeatDto::Daily => {}
            RepeatDto::Once => {
                let ok = a.date.as_deref().map(is_real_ymd).unwrap_or(false);
                if !ok && a.enabled {
                    a.enabled = false; // a once-alarm without a real date can never fire
                    changed = true;
                }
            }
            RepeatDto::Weekly => {
                if normalize_weekdays(&mut a.weekdays) {
                    changed = true;
                }
                if a.weekdays.is_empty() && a.enabled {
                    a.enabled = false; // nothing selected => never fires; disable explicitly
                    changed = true;
                }
            }
            RepeatDto::Biweekly => {
                // Weekly-style weekday normalization plus an Once-style start-date check.
                if normalize_weekdays(&mut a.weekdays) {
                    changed = true;
                }
                let date_ok = a.date.as_deref().map(is_real_ymd).unwrap_or(false);
                if (a.weekdays.is_empty() || !date_ok) && a.enabled {
                    a.enabled = false; // needs days AND a real anchor date
                    changed = true;
                }
            }
            RepeatDto::Monthly => clamp(&mut a.day_of_month, 1, 31, &mut changed),
            RepeatDto::Yearly => {
                clamp(&mut a.month, 1, 12, &mut changed);
                clamp(&mut a.day_of_month, 1, 31, &mut changed);
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alarm(repeat: RepeatDto, time: &str) -> AlarmDto {
        AlarmDto {
            id: "a1".into(),
            name: "Test".into(),
            time: time.into(),
            repeat,
            weekdays: vec![],
            day_of_month: 0,
            month: 0,
            date: None,
            enabled: true,
            chime_id: String::new(),
            chime_volume_pct: default_chime_volume(),
        }
    }

    #[test]
    fn parse_hhmm_accepts_valid_and_rejects_junk() {
        assert_eq!(parse_hhmm("08:30"), Some((8, 30)));
        assert_eq!(parse_hhmm("00:00"), Some((0, 0)));
        assert_eq!(parse_hhmm("23:59"), Some((23, 59)));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("08:60"), None);
        assert_eq!(parse_hhmm("8:30"), None); // not zero-padded
        assert_eq!(parse_hhmm("0830"), None);
    }

    #[test]
    fn daily_fires_only_at_its_minute() {
        let a = alarm(RepeatDto::Daily, "08:30");
        assert!(alarm_is_due(&a, 8, 30, 2, 6, 15, 30, "2026-06-15"));
        assert!(!alarm_is_due(&a, 8, 31, 2, 6, 15, 30, "2026-06-15"));
        assert!(!alarm_is_due(&a, 9, 30, 2, 6, 15, 30, "2026-06-15"));
    }

    #[test]
    fn disabled_never_fires() {
        let mut a = alarm(RepeatDto::Daily, "08:30");
        a.enabled = false;
        assert!(!alarm_is_due(&a, 8, 30, 2, 6, 15, 30, "2026-06-15"));
    }

    #[test]
    fn weekly_respects_weekday_membership() {
        let mut a = alarm(RepeatDto::Weekly, "08:30");
        a.weekdays = vec![0, 4]; // Mon, Fri
        assert!(alarm_is_due(&a, 8, 30, 0, 6, 15, 30, "2026-06-15")); // Mon
        assert!(alarm_is_due(&a, 8, 30, 4, 6, 19, 30, "2026-06-19")); // Fri
        assert!(!alarm_is_due(&a, 8, 30, 2, 6, 17, 30, "2026-06-17")); // Wed
    }

    // June 2026 Mondays: 1, 8, 15, 22, 29; Jul 6 is the next Monday (consecutive weeks).
    #[test]
    fn days_from_civil_and_monday_week_are_correct() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1970, 1, 5), 4); // a Monday -> monday_week 0
        assert_eq!(monday_week(4), 0);
        // Real Mondays satisfy (count - 4) % 7 == 0, including pre-1970 (negative) counts.
        assert_eq!((days_from_civil(2026, 6, 15) - 4).rem_euclid(7), 0);
        assert_eq!((days_from_civil(1969, 12, 29) - 4).rem_euclid(7), 0);
        // Consecutive Mondays are consecutive week indices.
        let w1 = monday_week(days_from_civil(2026, 6, 8));
        let w2 = monday_week(days_from_civil(2026, 6, 15));
        assert_eq!(w2, w1 + 1);
        // Leap day: Feb spans 29 days in 2024 but 28 in 2023.
        assert_eq!(
            days_from_civil(2024, 3, 1) - days_from_civil(2024, 2, 28),
            2
        );
        assert_eq!(
            days_from_civil(2023, 3, 1) - days_from_civil(2023, 2, 28),
            1
        );
    }

    #[test]
    fn biweekly_fires_on_week_and_skips_off_week() {
        let mut a = alarm(RepeatDto::Biweekly, "08:30");
        a.weekdays = vec![0]; // Mon
        a.date = Some("2026-06-08".into()); // start week = week of Mon Jun 8
                                            // On-weeks: Jun 8, Jun 22, Jul 6 (every other Monday from the start).
        assert!(alarm_is_due(&a, 8, 30, 0, 6, 8, 30, "2026-06-08"));
        assert!(alarm_is_due(&a, 8, 30, 0, 6, 22, 30, "2026-06-22"));
        assert!(alarm_is_due(&a, 8, 30, 0, 7, 6, 31, "2026-07-06"));
        // Off-weeks: Jun 15, Jun 29.
        assert!(!alarm_is_due(&a, 8, 30, 0, 6, 15, 30, "2026-06-15"));
        assert!(!alarm_is_due(&a, 8, 30, 0, 6, 29, 30, "2026-06-29"));
        // Wrong minute / non-ticked weekday still excluded.
        assert!(!alarm_is_due(&a, 9, 30, 0, 6, 8, 30, "2026-06-08"));
        assert!(!alarm_is_due(&a, 8, 30, 2, 6, 10, 30, "2026-06-10")); // Wed not ticked
    }

    #[test]
    fn biweekly_supports_multiple_weekdays() {
        let mut a = alarm(RepeatDto::Biweekly, "08:30");
        a.weekdays = vec![0, 2]; // Mon + Wed
        a.date = Some("2026-06-08".into());
        assert!(alarm_is_due(&a, 8, 30, 0, 6, 8, 30, "2026-06-08")); // Mon on-week
        assert!(alarm_is_due(&a, 8, 30, 2, 6, 10, 30, "2026-06-10")); // Wed on-week
        assert!(!alarm_is_due(&a, 8, 30, 2, 6, 17, 30, "2026-06-17")); // Wed off-week
        assert!(alarm_is_due(&a, 8, 30, 0, 6, 22, 30, "2026-06-22")); // Mon next on-week
    }

    #[test]
    fn biweekly_never_fires_before_start_date() {
        let mut a = alarm(RepeatDto::Biweekly, "08:30");
        a.weekdays = vec![0]; // Mon
        a.date = Some("2026-06-22".into());
        // Jun 8 shares Jun 22's week-parity but precedes the start -> must NOT fire.
        assert!(!alarm_is_due(&a, 8, 30, 0, 6, 8, 30, "2026-06-08"));
        assert!(alarm_is_due(&a, 8, 30, 0, 6, 22, 30, "2026-06-22"));
        assert!(alarm_is_due(&a, 8, 30, 0, 7, 6, 31, "2026-07-06"));
    }

    #[test]
    fn biweekly_skips_ticked_day_before_anchor_in_first_week() {
        let mut a = alarm(RepeatDto::Biweekly, "08:30");
        a.weekdays = vec![0]; // Mon
        a.date = Some("2026-06-10".into()); // anchor is the Wed in the week of Mon Jun 8
                                            // Mon Jun 8 is in the anchor's week but precedes the anchor date -> skipped.
        assert!(!alarm_is_due(&a, 8, 30, 0, 6, 8, 30, "2026-06-08"));
        // Next on-week Monday (Jun 22) fires; the off-week Monday (Jun 15) never does.
        assert!(alarm_is_due(&a, 8, 30, 0, 6, 22, 30, "2026-06-22"));
        assert!(!alarm_is_due(&a, 8, 30, 0, 6, 15, 30, "2026-06-15"));
    }

    #[test]
    fn sanitize_disables_biweekly_without_date() {
        let mut a = alarm(RepeatDto::Biweekly, "08:30");
        a.weekdays = vec![0];
        let mut v = vec![a]; // date None
        assert!(sanitize_alarms(&mut v));
        assert!(!v[0].enabled);
    }

    #[test]
    fn sanitize_disables_biweekly_with_no_days() {
        let mut a = alarm(RepeatDto::Biweekly, "08:30");
        a.date = Some("2026-06-08".into());
        let mut v = vec![a]; // weekdays empty
        assert!(sanitize_alarms(&mut v));
        assert!(!v[0].enabled);
    }

    #[test]
    fn sanitize_keeps_valid_biweekly_enabled() {
        let mut a = alarm(RepeatDto::Biweekly, "08:30");
        a.weekdays = vec![0];
        a.date = Some("2026-06-08".into());
        let mut v = vec![a];
        sanitize_alarms(&mut v);
        assert!(v[0].enabled);
    }

    #[test]
    fn monthly_exact_and_end_of_month_clamp() {
        let mut a = alarm(RepeatDto::Monthly, "08:30");
        a.day_of_month = 15;
        assert!(alarm_is_due(&a, 8, 30, 0, 6, 15, 30, "2026-06-15"));
        assert!(!alarm_is_due(&a, 8, 30, 0, 6, 14, 30, "2026-06-14"));

        // "31" fires on the last day of a short month (Feb 28 in 2026).
        a.day_of_month = 31;
        assert!(alarm_is_due(&a, 8, 30, 5, 2, 28, 28, "2026-02-28"));
        assert!(!alarm_is_due(&a, 8, 30, 0, 2, 27, 28, "2026-02-27"));
        // In a 31-day month it only fires on the 31st.
        assert!(alarm_is_due(&a, 8, 30, 0, 1, 31, 31, "2026-01-31"));
        assert!(!alarm_is_due(&a, 8, 30, 0, 1, 30, 31, "2026-01-30"));
    }

    #[test]
    fn yearly_matches_month_and_day_with_feb29_fallback() {
        let mut a = alarm(RepeatDto::Yearly, "08:30");
        a.month = 12;
        a.day_of_month = 25;
        assert!(alarm_is_due(&a, 8, 30, 4, 12, 25, 31, "2026-12-25"));
        assert!(!alarm_is_due(&a, 8, 30, 4, 11, 25, 30, "2026-11-25"));

        // Feb 29 yearly fires Feb 28 in a common year.
        a.month = 2;
        a.day_of_month = 29;
        assert!(alarm_is_due(&a, 8, 30, 5, 2, 28, 28, "2027-02-28"));
        // and on Feb 29 in a leap year.
        assert!(alarm_is_due(&a, 8, 30, 0, 2, 29, 29, "2028-02-29"));
    }

    #[test]
    fn once_matches_exact_date() {
        let mut a = alarm(RepeatDto::Once, "08:30");
        a.date = Some("2026-06-15".into());
        assert!(alarm_is_due(&a, 8, 30, 0, 6, 15, 30, "2026-06-15"));
        assert!(!alarm_is_due(&a, 8, 30, 0, 6, 16, 30, "2026-06-16"));
    }

    #[test]
    fn sanitize_resets_bad_time() {
        let mut v = vec![alarm(RepeatDto::Daily, "99:99")];
        assert!(sanitize_alarms(&mut v));
        assert_eq!(v[0].time, "08:00");
    }

    #[test]
    fn sanitize_disables_weekly_with_no_days() {
        let mut v = vec![alarm(RepeatDto::Weekly, "08:30")]; // weekdays empty
        assert!(sanitize_alarms(&mut v));
        assert!(!v[0].enabled);
    }

    #[test]
    fn sanitize_disables_once_without_valid_date() {
        let mut v = vec![alarm(RepeatDto::Once, "08:30")]; // date None
        assert!(sanitize_alarms(&mut v));
        assert!(!v[0].enabled);
    }

    #[test]
    fn sanitize_disables_once_with_impossible_date() {
        let mut a = alarm(RepeatDto::Once, "08:30");
        a.date = Some("2026-02-30".into()); // well-shaped but not a real date
        let mut v = vec![a];
        assert!(sanitize_alarms(&mut v));
        assert!(!v[0].enabled);
    }

    #[test]
    fn sanitize_keeps_once_with_real_date() {
        let mut a = alarm(RepeatDto::Once, "08:30");
        a.date = Some("2028-02-29".into()); // valid leap day
        let mut v = vec![a];
        sanitize_alarms(&mut v);
        assert!(v[0].enabled);
    }

    #[test]
    fn days_in_month_covers_the_calendar() {
        assert_eq!(days_in_month(2026, 1), 31);
        assert_eq!(days_in_month(2026, 4), 30);
        assert_eq!(days_in_month(2026, 12), 31);
        assert_eq!(days_in_month(2026, 2), 28); // common year
        assert_eq!(days_in_month(2028, 2), 29); // leap year
        assert_eq!(days_in_month(2000, 2), 29); // divisible by 400
        assert_eq!(days_in_month(1900, 2), 28); // divisible by 100, not 400
    }

    #[test]
    fn is_real_ymd_rejects_impossible_dates() {
        assert!(is_real_ymd("2026-06-15"));
        assert!(is_real_ymd("2028-02-29"));
        assert!(!is_real_ymd("2026-02-30"));
        assert!(!is_real_ymd("2026-02-29")); // common year
        assert!(!is_real_ymd("2026-13-01"));
        assert!(!is_real_ymd("2026-00-10"));
        assert!(!is_real_ymd("2026-6-15")); // not zero-padded
        assert!(!is_real_ymd("garbage"));
    }

    #[test]
    fn sanitize_regenerates_blank_and_duplicate_ids() {
        let mut a = alarm(RepeatDto::Daily, "08:30");
        a.id = String::new();
        let mut b = alarm(RepeatDto::Daily, "09:30");
        b.id = "dup".into();
        let mut c = alarm(RepeatDto::Daily, "10:30");
        c.id = "dup".into();
        let mut v = vec![a, b, c];
        assert!(sanitize_alarms(&mut v));
        let ids: HashSet<&String> = v.iter().map(|a| &a.id).collect();
        assert_eq!(ids.len(), 3, "all ids must be unique after sanitize");
        assert!(v.iter().all(|a| !a.id.trim().is_empty()));
    }

    #[test]
    fn sanitize_leaves_irrelevant_fields_alone() {
        // A daily alarm with a stray day_of_month=0 must not be "fixed" to 1.
        let mut v = vec![alarm(RepeatDto::Daily, "08:30")];
        let changed = sanitize_alarms(&mut v);
        assert!(!changed);
        assert_eq!(v[0].day_of_month, 0);
    }

    #[test]
    fn chime_volume_defaults_clamps_and_round_trips() {
        let text = "id = \"a\"\nname = \"A\"\ntime = \"08:30\"\nrepeat = \"daily\"\n";
        let parsed: AlarmDto = toml::from_str(text).unwrap();
        assert_eq!(parsed.chime_volume_pct, default_chime_volume());

        let mut v = vec![parsed.clone()];
        v[0].chime_volume_pct = 250;
        assert!(sanitize_alarms(&mut v));
        assert_eq!(v[0].chime_volume_pct, 100);

        v[0].chime_volume_pct = 0;
        let saved = toml::to_string_pretty(&v[0]).unwrap();
        assert!(saved.contains("chime_volume_pct = 0"));
        let reparsed: AlarmDto = toml::from_str(&saved).unwrap();
        assert_eq!(reparsed.chime_volume_pct, 0);
    }
}
