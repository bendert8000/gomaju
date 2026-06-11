use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use gomaju_core::chime::ChimeDto;
use gomaju_core::{config::ConfigFile, Engine};

use crate::idle::IdleStatus;

#[derive(Default)]
pub struct PauseReminderState {
    pub next_due: Option<Instant>,
    pub prompt_open: bool,
    pub generation: u64,
}

/// Application state shared between the tray, commands, and the ticker thread.
/// The engine and config are each behind their own mutex (low contention: the
/// ticker locks the engine ~once/second; commands lock briefly).
pub struct AppState {
    pub engine: Mutex<Engine>,
    pub config: Mutex<ConfigFile>,
    pub config_path: PathBuf,
    /// Saved chimes, loaded from `chimes.toml` (separate from `config.toml`). The chimes window
    /// edits this; playback + the rule/alarm pickers read it. `chimes_path` is the toml's path; its
    /// parent folder also holds imported sound files.
    pub chimes: Mutex<Vec<ChimeDto>>,
    pub chimes_path: PathBuf,
    /// Path to `quotes.toml` (break quotes, separate from config.toml, next to it in the config
    /// dir). Read live on each break by `quotes::pick`; the Settings "Quotes" card edits it.
    pub quotes_path: PathBuf,
    /// Path to `session.toml` (persisted per-rule break progress, next to `config.toml`). The
    /// ticker autosaves it (+ a final save on clean quit); cold start reads it to offer "resume".
    pub session_path: PathBuf,
    /// Health of the idle backend, decided once at startup.
    pub idle_status: IdleStatus,
    /// When the timer last (re)entered Running; `None` while paused/stopped. Used to
    /// show elapsed running time in the tray. Kept across breaks (InBreak doesn't reset).
    pub running_since: Mutex<Option<Instant>>,
    /// Runtime-only scheduler for the "still paused?" reminder dialog.
    pub pause_reminder: Mutex<PauseReminderState>,
}

impl AppState {
    /// Atomically update the persisted config. Clones the cached config under the held `config`
    /// lock, runs `mutate` on the clone, writes it to disk, and only then swaps it into the cache —
    /// **without releasing the lock between the write and the swap**, so a concurrent writer (a
    /// window Save or the ticker's once-rule/once-alarm auto-disable) can never interleave a stale
    /// snapshot. This is the single chokepoint that makes the "hold the lock across save+cache"
    /// invariant structural instead of a per-call-site convention.
    ///
    /// `mutate` returns `false` to abort with no write (a no-op — e.g. the target is already gone or
    /// unchanged); on a disk-write error the cache is left untouched. Returns the written config
    /// (`Some`) so callers can reconfigure the engine / echo it back, or `None` when `mutate`
    /// aborted.
    pub fn with_config_write<F>(&self, mutate: F) -> Result<Option<ConfigFile>, String>
    where
        F: FnOnce(&mut ConfigFile) -> bool,
    {
        let mut guard = self.config.lock().unwrap();
        let mut next = guard.clone();
        if !mutate(&mut next) {
            return Ok(None);
        }
        gomaju_core::config::save(&self.config_path, &next).map_err(|e| e.to_string())?;
        *guard = next.clone();
        Ok(Some(next))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A unique temp path so concurrent test runs don't collide on the same config file.
    fn unique_tmp(name: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("gomaju-test-{}-{n}-{name}", std::process::id()))
    }

    /// A minimal AppState wired to a real (temp) config path — enough to exercise config writes.
    fn test_state(config_path: PathBuf) -> AppState {
        let (rules, settings) = ConfigFile::default().to_engine_inputs();
        AppState {
            engine: Mutex::new(Engine::new(rules, settings)),
            config: Mutex::new(ConfigFile::default()),
            config_path,
            chimes: Mutex::new(Vec::new()),
            chimes_path: PathBuf::new(),
            quotes_path: PathBuf::new(),
            session_path: PathBuf::new(),
            idle_status: IdleStatus::Active,
            running_since: Mutex::new(None),
            pause_reminder: Mutex::new(PauseReminderState::default()),
        }
    }

    #[test]
    fn with_config_write_persists_and_swaps_when_mutate_returns_true() {
        let path = unique_tmp("config.toml");
        let st = test_state(path.clone());
        let before = st.config.lock().unwrap().locale.clone();
        let new_locale = if before == "en" { "zh-Hant" } else { "en" };

        let written = st
            .with_config_write(|c| {
                c.locale = new_locale.to_string();
                true
            })
            .expect("write should succeed")
            .expect("a true mutate writes, so Some(config)");

        assert_eq!(written.locale, new_locale, "returns the written config");
        assert_eq!(
            st.config.lock().unwrap().locale,
            new_locale,
            "cache is swapped to the new config"
        );
        let on_disk = std::fs::read_to_string(&path).expect("config persisted to disk");
        assert!(
            on_disk.contains(&format!("locale = \"{new_locale}\"")),
            "the new locale reached disk, got: {on_disk}"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn with_config_write_is_a_noop_when_mutate_returns_false() {
        let path = unique_tmp("config-noop.toml");
        let st = test_state(path.clone());
        let before = st.config.lock().unwrap().locale.clone();

        let result = st
            .with_config_write(|c| {
                c.locale = "changed-on-the-throwaway-clone".to_string();
                false // abort: the mutation to the clone must not persist or swap
            })
            .expect("a no-op is not an error");

        assert!(result.is_none(), "an aborted mutate returns None");
        assert_eq!(
            st.config.lock().unwrap().locale,
            before,
            "the cache is untouched when mutate aborts"
        );
        assert!(!path.exists(), "nothing is written to disk on a no-op");
    }
}
