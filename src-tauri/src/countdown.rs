//! Countdown-timer run state + firing thread (the Tauri-side counterpart to the pure
//! `gomaju_core::countdown` definitions).
//!
//! A countdown's *definition* (name / duration / chime) is persisted in
//! `config.toml` and owned by the core crate. Its *live* state — running, paused, or
//! idle — is in-memory only ([`AppState::countdown_runtime`]) and reset on every cold
//! start. The transition helpers below are pure (they take `now: Instant`) so they're
//! unit-testable without a real clock; [`spawn_scheduler`] drives them on a thread.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use gomaju_core::countdown::CountdownDto;

use crate::app_state::AppState;
use crate::{audio, runtime};

/// Live state of one countdown. Absent from the runtime map = **idle**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountdownRun {
    /// Counting down; fires when `Instant::now() >= finish_at`.
    Running { finish_at: Instant },
    /// Paused with this much time left; resumes from here.
    Paused { remaining: Duration },
}

/// Start a countdown, or resume a paused one. Resuming uses the saved remaining; starting
/// from idle (or restarting a running one) uses the full `duration`. A paused timer with
/// zero remaining (paused right at/after the end before the tick caught it) restarts full,
/// so "Start" always actually counts down rather than firing on the next tick.
pub fn start(map: &mut HashMap<String, CountdownRun>, id: &str, duration: Duration, now: Instant) {
    let remaining = match map.get(id) {
        Some(CountdownRun::Paused { remaining }) if !remaining.is_zero() => *remaining,
        _ => duration,
    };
    map.insert(
        id.to_string(),
        CountdownRun::Running {
            finish_at: now + remaining,
        },
    );
}

/// Pause a running countdown, capturing its remaining time (saturating, so pausing past the
/// end clamps to zero). No-op if the timer isn't currently running.
pub fn pause(map: &mut HashMap<String, CountdownRun>, id: &str, now: Instant) {
    if let Some(CountdownRun::Running { finish_at }) = map.get(id).copied() {
        let remaining = finish_at.saturating_duration_since(now);
        map.insert(id.to_string(), CountdownRun::Paused { remaining });
    }
}

/// Reset a countdown back to idle (remove its run state).
pub fn reset(map: &mut HashMap<String, CountdownRun>, id: &str) {
    map.remove(id);
}

/// Whole seconds left, rounded **up** so a still-running countdown never displays `00:00`
/// for its last sub-second.
pub fn remaining_secs(run: &CountdownRun, now: Instant) -> u32 {
    let d = match run {
        CountdownRun::Running { finish_at } => finish_at.saturating_duration_since(now),
        CountdownRun::Paused { remaining } => *remaining,
    };
    let ceil = d.as_secs() + if d.subsec_nanos() > 0 { 1 } else { 0 };
    ceil.min(u32::MAX as u64) as u32
}

/// The UI state string for a (maybe absent) run entry.
pub fn state_str(run: Option<&CountdownRun>) -> &'static str {
    match run {
        Some(CountdownRun::Running { .. }) => "running",
        Some(CountdownRun::Paused { .. }) => "paused",
        None => "idle",
    }
}

/// Spawn the countdown firing thread: a dedicated ~250 ms loop (finer than the alarm
/// scheduler's 1 s, so a 1-second timer fires within a quarter-second of zero).
///
/// **Race-safe by construction.** Each tick takes a config snapshot first (config lock,
/// released), then does the due-check **and** the state transition (a due timer is one-shot:
/// fire, then back to idle) together under a single `countdown_runtime` lock — never split
/// across the audio/notify side effects. Lock order is always config → runtime (matching
/// `cmd_save_countdowns`), so a concurrent start/pause/reset/save can't be clobbered, and a
/// timer reset or deleted in the same instant can't be resurrected or fired.
pub fn spawn_scheduler(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(250));
        let now = Instant::now();

        // 1) Snapshot the defs + locale + notification gate under the config lock. The chimes
        //    list is deliberately NOT cloned here — only when a timer actually fires (step 3) —
        //    so an idle app (the common case) doesn't clone it 4×/second forever.
        let (defs, locale, notify): (HashMap<String, CountdownDto>, String, bool) = {
            let st = app.state::<AppState>();
            let cfg = st.config.lock().unwrap();
            let defs = cfg
                .countdowns
                .iter()
                .map(|c| (c.id.clone(), c.clone()))
                .collect();
            (defs, cfg.locale.clone(), cfg.settings.notifications)
        };

        // 2) Atomic due-check + transition under the runtime lock. Collect what to fire.
        let mut fired: Vec<(String, String, u8)> = Vec::new(); // (name, chime_id, volume)
        {
            let st = app.state::<AppState>();
            let mut map = st.countdown_runtime.lock().unwrap();
            let due: Vec<String> = map
                .iter()
                .filter_map(|(id, run)| match run {
                    CountdownRun::Running { finish_at } if *finish_at <= now => Some(id.clone()),
                    _ => None,
                })
                .collect();
            for id in due {
                // Either way the timer leaves the running set (one-shot). Fire only if its def
                // still exists; a def deleted out from under us just drops the orphan silently.
                if let Some(def) = defs.get(&id) {
                    fired.push((def.name.clone(), def.chime_id.clone(), def.chime_volume_pct));
                }
                map.remove(&id);
            }
        }

        // 3) Side effects only after both locks are released.
        if !fired.is_empty() {
            // Something fired — only now snapshot the chimes list + dir for playback.
            let (chimes, dir) = {
                let st = app.state::<AppState>();
                let chimes = st.chimes.lock().unwrap().clone();
                let dir = st
                    .config_path
                    .parent()
                    .map(|p| p.join("chimes"))
                    .unwrap_or_default();
                (chimes, dir)
            };
            for (name, chime_id, volume) in fired {
                crate::rlog!("gomaju: countdown fired ({name})");
                if notify {
                    runtime::show_notification(
                        &app,
                        crate::i18n::tr(&locale, "notif.timer_title"),
                        &name,
                    );
                }
                audio::play_countdown_chime(&chime_id, volume, &chimes, &dir);
            }
        }
        // Reconcile the running-timer toasts every tick (cheap no-op when unchanged). Done here on
        // this background thread — NOT in the start/pause/reset commands — so toast windows are
        // created off the main-thread WebView2-IPC path, which would otherwise deadlock on Windows.
        crate::timer_toast::sync(&app);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dur(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn start_from_idle_runs_full_duration() {
        let mut m = HashMap::new();
        let now = Instant::now();
        start(&mut m, "a", dur(60), now);
        assert_eq!(
            m.get("a"),
            Some(&CountdownRun::Running {
                finish_at: now + dur(60)
            })
        );
    }

    #[test]
    fn pause_captures_remaining_then_resume_continues_from_it() {
        let mut m = HashMap::new();
        let now = Instant::now();
        start(&mut m, "a", dur(60), now);
        pause(&mut m, "a", now + dur(10)); // 50s left
        assert_eq!(
            m.get("a"),
            Some(&CountdownRun::Paused { remaining: dur(50) })
        );
        let resume_at = now + dur(20);
        start(&mut m, "a", dur(60), resume_at); // resumes from 50s, not full
        assert_eq!(
            m.get("a"),
            Some(&CountdownRun::Running {
                finish_at: resume_at + dur(50)
            })
        );
    }

    #[test]
    fn pause_past_zero_clamps_to_zero() {
        let mut m = HashMap::new();
        let now = Instant::now();
        start(&mut m, "a", dur(5), now);
        pause(&mut m, "a", now + dur(10)); // 5s timer paused at +10s
        assert_eq!(
            m.get("a"),
            Some(&CountdownRun::Paused {
                remaining: Duration::ZERO
            })
        );
    }

    #[test]
    fn resume_from_zero_remaining_restarts_full() {
        let mut m = HashMap::new();
        m.insert(
            "a".to_string(),
            CountdownRun::Paused {
                remaining: Duration::ZERO,
            },
        );
        let now = Instant::now();
        start(&mut m, "a", dur(30), now);
        assert_eq!(
            m.get("a"),
            Some(&CountdownRun::Running {
                finish_at: now + dur(30)
            })
        );
    }

    #[test]
    fn pause_is_a_noop_when_not_running() {
        let mut m = HashMap::new();
        pause(&mut m, "missing", Instant::now());
        assert!(!m.contains_key("missing"));
    }

    #[test]
    fn reset_removes_run_state() {
        let mut m = HashMap::new();
        start(&mut m, "a", dur(60), Instant::now());
        reset(&mut m, "a");
        assert!(!m.contains_key("a"));
    }

    #[test]
    fn remaining_secs_rounds_up() {
        let now = Instant::now();
        let run = CountdownRun::Running {
            finish_at: now + Duration::from_millis(4500),
        };
        assert_eq!(remaining_secs(&run, now), 5);
        let paused = CountdownRun::Paused { remaining: dur(50) };
        assert_eq!(remaining_secs(&paused, now), 50);
        let done = CountdownRun::Running { finish_at: now };
        assert_eq!(remaining_secs(&done, now), 0);
    }

    #[test]
    fn state_str_maps_run_states() {
        let now = Instant::now();
        assert_eq!(state_str(None), "idle");
        assert_eq!(
            state_str(Some(&CountdownRun::Running { finish_at: now })),
            "running"
        );
        assert_eq!(
            state_str(Some(&CountdownRun::Paused { remaining: dur(1) })),
            "paused"
        );
    }
}
