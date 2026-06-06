use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use restee_core::chime::ChimeDto;
use restee_core::{config::ConfigFile, Engine};

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
    /// Health of the idle backend, decided once at startup.
    pub idle_status: IdleStatus,
    /// When the timer last (re)entered Running; `None` while paused/stopped. Used to
    /// show elapsed running time in the tray. Kept across breaks (InBreak doesn't reset).
    pub running_since: Mutex<Option<Instant>>,
    /// Runtime-only scheduler for the "still paused?" reminder dialog.
    pub pause_reminder: Mutex<PauseReminderState>,
}
