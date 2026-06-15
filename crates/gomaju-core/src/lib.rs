//! Pure timer/state engine for gomaju.
//!
//! This crate has no Tauri, OS, clock, or I/O dependencies. The host feeds it
//! `(delta, idle)` each tick and interprets the [`Effect`]s it returns. That keeps
//! all the timing/priority logic fully unit-testable in isolation.

pub mod alarm;
pub mod chime;
pub mod config;
pub mod countdown;
mod engine;
pub mod progress;
pub mod quotes;
mod rule;
mod settings;

pub use engine::{Effect, Engine, EngineStatus, NextBreak, RunState};
pub use rule::{Enforcement, Rule};
pub use settings::{EscapeMode, IdlePolicy, Settings};
