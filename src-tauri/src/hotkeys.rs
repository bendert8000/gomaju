use std::str::FromStr;

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use gomaju_core::{config::HotkeysDto, RunState};

use crate::app_state::AppState;
use crate::runtime;

#[derive(Clone, Copy)]
enum Action {
    Toggle,
    BreakNow,
    Skip,
}

/// Re-register all global hotkeys from config. Returns a list of human-readable
/// errors for accelerators that failed (invalid syntax, or already owned by
/// another app — note that registration succeeding does not guarantee no other
/// app also holds the combo).
pub fn apply(app: &AppHandle, hotkeys: &HotkeysDto) -> Vec<String> {
    let _ = app.global_shortcut().unregister_all();
    let mut errors = Vec::new();
    register(app, hotkeys.toggle.as_deref(), Action::Toggle, &mut errors);
    register(
        app,
        hotkeys.break_now.as_deref(),
        Action::BreakNow,
        &mut errors,
    );
    register(app, hotkeys.skip.as_deref(), Action::Skip, &mut errors);
    errors
}

fn register(app: &AppHandle, accel: Option<&str>, action: Action, errors: &mut Vec<String>) {
    let Some(accel) = accel.filter(|a| !a.trim().is_empty()) else {
        return;
    };
    let shortcut = match Shortcut::from_str(accel) {
        Ok(s) => s,
        Err(e) => {
            errors.push(format!("{accel}: invalid shortcut ({e})"));
            return;
        }
    };
    let result = app
        .global_shortcut()
        .on_shortcut(shortcut, move |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                let state = app.state::<AppState>();
                perform(app, state.inner(), action);
            }
        });
    if let Err(e) = result {
        errors.push(format!("{accel}: {e}"));
    }
}

fn perform(app: &AppHandle, state: &AppState, action: Action) {
    match action {
        Action::Toggle => {
            let current = state.engine.lock().unwrap().state();
            match current {
                RunState::Running => runtime::action_pause(app, state),
                RunState::Paused | RunState::Stopped => runtime::action_start(app, state),
                RunState::InBreak => runtime::action_skip(app, state),
            }
        }
        Action::BreakNow => runtime::action_break_now(app, state),
        Action::Skip => runtime::action_skip(app, state),
    }
}
