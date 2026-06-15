//! Countdown timers: TOML-friendly definitions plus in-place validation.
//!
//! Like the rest of `gomaju-core`, this module has no clock or OS dependency: a
//! countdown is just a named duration + chime. The host (`src-tauri`) owns the live
//! run state (start / pause / reset) and the firing thread; here we only model and
//! validate the persisted *definition*.
//!
//! Named `countdown`, not `timer`, on purpose: the engine already uses "timer" for
//! the break work/rest clocks (`Engine::reset_timer`, "Reset break timer"), so the
//! user-facing **Timers** feature keeps a distinct backend noun to avoid identifier
//! and terminology collisions.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::config::{default_chime_volume, is_default_chime_volume};

/// Shortest countdown: 1 second.
pub const MIN_DURATION_SECS: u32 = 1;
/// Longest countdown: 99:59:59.
pub const MAX_DURATION_SECS: u32 = 99 * 3600 + 59 * 60 + 59; // 359_999

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CountdownDto {
    pub id: String,
    pub name: String,
    /// Countdown length in seconds, clamped to `MIN_DURATION_SECS..=MAX_DURATION_SECS`.
    pub duration_secs: u32,
    /// Optional id of a saved chime to play when this countdown fires (empty = the default tone).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub chime_id: String,
    /// Volume for this countdown's selected/default chime. Reuses the shared default (20).
    #[serde(
        default = "default_chime_volume",
        skip_serializing_if = "is_default_chime_volume"
    )]
    pub chime_volume_pct: u8,
}

/// Validate/normalize countdowns in place; returns true if anything changed (so the
/// caller can persist). Regenerates blank/duplicate ids (the host keys run state on
/// them), clamps `duration_secs` to 1..=86_399, and clamps the volume to 0..=100.
/// Mirrors [`crate::alarm::sanitize_alarms`], minus any recurrence fields.
pub fn sanitize_countdowns(items: &mut [CountdownDto]) -> bool {
    let mut changed = false;

    // Ids that already exist (so generated ones never collide with a later original).
    let originals: HashSet<String> = items
        .iter()
        .map(|c| c.id.clone())
        .filter(|s| !s.trim().is_empty())
        .collect();
    let mut used: HashSet<String> = HashSet::new();
    let mut counter = 0u32;

    for c in items.iter_mut() {
        // id: non-blank and unique across the list.
        if c.id.trim().is_empty() || used.contains(&c.id) {
            let new_id = loop {
                counter += 1;
                let candidate = format!("countdown-{counter}");
                if !used.contains(&candidate) && !originals.contains(&candidate) {
                    break candidate;
                }
            };
            c.id = new_id;
            changed = true;
        }
        used.insert(c.id.clone());

        let dur = c.duration_secs.clamp(MIN_DURATION_SECS, MAX_DURATION_SECS);
        if dur != c.duration_secs {
            c.duration_secs = dur;
            changed = true;
        }

        if c.chime_volume_pct > 100 {
            c.chime_volume_pct = 100;
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cd(id: &str, dur: u32) -> CountdownDto {
        CountdownDto {
            id: id.into(),
            name: "T".into(),
            duration_secs: dur,
            chime_id: String::new(),
            chime_volume_pct: default_chime_volume(),
        }
    }

    #[test]
    fn max_is_99_59_59() {
        assert_eq!(MAX_DURATION_SECS, 359_999);
    }

    #[test]
    fn clamps_duration_below_min_to_one() {
        let mut v = vec![cd("a", 0)];
        assert!(sanitize_countdowns(&mut v));
        assert_eq!(v[0].duration_secs, MIN_DURATION_SECS);
    }

    #[test]
    fn clamps_duration_above_max() {
        let mut v = vec![cd("a", 999_999)];
        assert!(sanitize_countdowns(&mut v));
        assert_eq!(v[0].duration_secs, MAX_DURATION_SECS);
    }

    #[test]
    fn regenerates_blank_and_duplicate_ids() {
        let mut v = vec![cd("", 60), cd("dup", 60), cd("dup", 60)];
        assert!(sanitize_countdowns(&mut v));
        let ids: HashSet<String> = v.iter().map(|c| c.id.clone()).collect();
        assert_eq!(ids.len(), 3, "all ids unique");
        assert!(v.iter().all(|c| !c.id.trim().is_empty()));
    }

    #[test]
    fn clamps_volume_above_100() {
        let mut v = vec![cd("a", 60)];
        v[0].chime_volume_pct = 250;
        assert!(sanitize_countdowns(&mut v));
        assert_eq!(v[0].chime_volume_pct, 100);
    }

    #[test]
    fn clean_input_is_unchanged() {
        let mut v = vec![cd("a", 60), cd("b", 300)];
        assert!(!sanitize_countdowns(&mut v));
    }
}
