//! Pure timer/state engine for restee.
//!
//! This crate has no Tauri, OS, clock, or I/O dependencies. The host feeds it
//! `(delta, idle)` each tick and interprets the [`Effect`]s it returns. That keeps
//! all the timing/priority logic fully unit-testable in isolation.

pub mod config;
mod engine;
mod rule;
mod settings;

pub use engine::{Effect, Engine, EngineStatus, NextBreak, RunState};
pub use rule::{Enforcement, Rule};
pub use settings::{EscapeMode, IdlePolicy, Settings};
