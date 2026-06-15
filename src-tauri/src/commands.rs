use serde::Serialize;
use tauri::{AppHandle, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

use gomaju_core::alarm::{self, AlarmDto};
use gomaju_core::chime::{self, ChimeDto, ChimesFile};
use gomaju_core::config::{self, ConfigFile, RuleDto};
use gomaju_core::countdown::CountdownDto;

use crate::alarms_window::{self, ALARMS_LABEL};
use crate::app_state::AppState;
use crate::breaks_window::{self, BREAKS_LABEL};
use crate::chimes_window::{self, CHIMES_LABEL};
use crate::idle::IdleStatus;
use crate::settings_window::{self, SETTINGS_LABEL};
use crate::timers_window::{self, TIMERS_LABEL};
use crate::{autostart, hotkeys, runtime};

/// Pure, unit-testable predicate: is this window label the settings window?
fn is_settings(label: &str) -> bool {
    label == SETTINGS_LABEL
}

/// Pure, unit-testable predicate: is this window label the alarms window?
fn is_alarms(label: &str) -> bool {
    label == ALARMS_LABEL
}

/// Pure, unit-testable predicate: is this window label the breaks window?
fn is_breaks(label: &str) -> bool {
    label == BREAKS_LABEL
}

/// Pure, unit-testable predicate: is this window label the chimes window?
fn is_chimes(label: &str) -> bool {
    label == CHIMES_LABEL
}

/// Pure, unit-testable predicate: is this window label the timers window?
fn is_timers(label: &str) -> bool {
    label == TIMERS_LABEL
}

/// Windows that show a chime picker (`<select>`), and so may preview a chime by id: the settings
/// rules editor, the alarms window, and the timers window. (The chimes window previews unsaved
/// definitions via `cmd_preview_chime` instead, so it is intentionally not here.)
fn shows_chime_picker(label: &str) -> bool {
    is_settings(label) || is_alarms(label) || is_timers(label)
}

/// Windows that carry a chime preview ▶/⏸ button, and so may stop a running preview: the chime
/// pickers plus the chimes editor.
fn has_chime_preview(label: &str) -> bool {
    shows_chime_picker(label) || is_chimes(label)
}

/// Shared gate body: app commands are not gated per-window by Tauri's capability system,
/// so this caller-label check is the real least-privilege enforcement. `what` names the
/// privileged scope for the rejection message.
fn gate(allowed: bool, what: &str) -> Result<(), String> {
    if allowed {
        Ok(())
    } else {
        Err(format!("forbidden: {what}-only command"))
    }
}

/// Reject a command invoked from any window other than the settings window.
fn require_settings(window: &WebviewWindow) -> Result<(), String> {
    gate(is_settings(window.label()), "settings")
}

/// Reject an alarms command invoked from any window other than the alarms window.
fn require_alarms(window: &WebviewWindow) -> Result<(), String> {
    gate(is_alarms(window.label()), "alarms")
}

/// Reject a breaks-dashboard command invoked from any window other than the breaks window.
fn require_breaks(window: &WebviewWindow) -> Result<(), String> {
    gate(is_breaks(window.label()), "breaks")
}

/// Reject a chimes-write command invoked from any window other than the chimes window.
fn require_chimes(window: &WebviewWindow) -> Result<(), String> {
    gate(is_chimes(window.label()), "chimes")
}

/// Reject a timers command invoked from any window other than the timers window.
fn require_timers(window: &WebviewWindow) -> Result<(), String> {
    gate(is_timers(window.label()), "timers")
}

/// True for a per-timer toast window (`timer-toast-<id>`).
fn is_timer_toast(label: &str) -> bool {
    label.starts_with(crate::timer_toast::TIMER_TOAST_PREFIX)
}

/// Reject the toast ✕ command invoked from any window other than a timer-toast window.
fn require_timer_toast(window: &WebviewWindow) -> Result<(), String> {
    gate(is_timer_toast(window.label()), "timer-toast")
}

/// True for a finished-timer "time's up" toast window (`timer-done-<id>`).
fn is_timer_done(label: &str) -> bool {
    label.starts_with(crate::timer_toast::TIMER_DONE_PREFIX)
}

/// Reject the dismiss command invoked from any window other than a timer-done toast window.
fn require_timer_done(window: &WebviewWindow) -> Result<(), String> {
    gate(is_timer_done(window.label()), "timer-done")
}

/// Reject the snooze command invoked from any window other than the pre-break warning toast.
fn require_toast(window: &WebviewWindow) -> Result<(), String> {
    gate(window.label() == crate::toast::TOAST_LABEL, "toast")
}

/// Reject pause-reminder actions invoked from any window other than the pause reminder toast.
fn require_pause_toast(window: &WebviewWindow) -> Result<(), String> {
    gate(
        window.label() == crate::pause_toast::PAUSE_TOAST_LABEL,
        "pause-toast",
    )
}

/// Push a config's rules+settings into the live engine and apply any resulting effects.
/// Shared by `cmd_save_config` and `cmd_set_rule_flags`; deliberately narrow — it does NOT
/// touch hotkeys or autostart (those stay in `cmd_save_config`).
fn reconfigure_engine(app: &AppHandle, state: &AppState, config: &ConfigFile) {
    let (rules, settings) = config.to_engine_inputs();
    let fx = state.engine.lock().unwrap().reconfigure(rules, settings);
    runtime::apply_effects(app, &fx);
}

/// Open to overlay windows — they legitimately end the current break.
#[tauri::command]
pub fn cmd_skip(app: AppHandle, state: State<'_, AppState>) {
    runtime::action_skip(&app, state.inner());
}

/// Per-break reset: restart a single rule's countdown (the window banners' per-row Reset).
/// Reset-all lives on the tray "Reset timer" item (`runtime::confirm_then_reset`).
#[tauri::command]
pub fn cmd_reset_timer(
    window: WebviewWindow,
    app: AppHandle,
    rule_id: String,
) -> Result<(), String> {
    gate(
        is_settings(window.label()) || is_breaks(window.label()),
        "settings/rules",
    )?;
    runtime::confirm_then_reset_one(&app, rule_id);
    Ok(())
}

/// Snooze the imminent break (the pre-break toast's "Delay 1 min"): push `rule_id`'s break back by
/// `secs` and close the warning toast (via the cancelled-warning effect). Toast-only.
#[tauri::command]
pub fn cmd_delay_break(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, AppState>,
    rule_id: String,
    secs: u64,
) -> Result<(), String> {
    require_toast(&window)?;
    let fx = state
        .engine
        .lock()
        .unwrap()
        .delay_break(&rule_id, std::time::Duration::from_secs(secs));
    runtime::apply_effects(&app, &fx);
    Ok(())
}

/// Take the imminent break **now** (the pre-break toast's "Break now"): fire `rule_id`'s break
/// immediately. The resulting `StartBreak` effect opens the overlay and closes this toast. Toast-only.
#[tauri::command]
pub fn cmd_break_now_rule(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, AppState>,
    rule_id: String,
) -> Result<(), String> {
    require_toast(&window)?;
    runtime::action_break_now_rule(&app, state.inner(), &rule_id);
    Ok(())
}

#[tauri::command]
pub fn cmd_resume_from_pause_reminder(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require_pause_toast(&window)?;
    runtime::resume_from_pause_reminder(&app, state.inner());
    Ok(())
}

#[tauri::command]
pub fn cmd_stay_paused_from_reminder(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require_pause_toast(&window)?;
    runtime::stay_paused_from_reminder(&app, state.inner());
    Ok(())
}

#[tauri::command]
pub fn cmd_get_config(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<ConfigFile, String> {
    require_settings(&window)?;
    Ok(state.config.lock().unwrap().clone())
}

#[tauri::command]
pub fn cmd_get_idle_status(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<IdleStatus, String> {
    require_settings(&window)?;
    Ok(state.idle_status)
}

#[tauri::command]
pub fn cmd_close_settings(window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    require_settings(&window)?;
    settings_window::close(&app);
    Ok(())
}

#[tauri::command]
pub fn cmd_get_app_version(window: WebviewWindow, app: AppHandle) -> Result<String, String> {
    require_settings(&window)?;
    Ok(app.package_info().version.to_string())
}

/// One enabled break's countdown, for the status banners + dashboard cards.
#[derive(Serialize)]
pub struct NextBreakDto {
    pub rule_id: String,
    pub rule_name: String,
    pub remaining_secs: u64,
}

/// Current run state + every enabled break (soonest-first), for the settings /
/// Today's breaks banners and the per-card countdowns.
#[derive(Serialize)]
pub struct StatusDto {
    pub state: &'static str,
    pub all: Vec<NextBreakDto>,
}

#[tauri::command]
pub fn cmd_get_status(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<StatusDto, String> {
    // Read-only status, shown in both the Settings banner and the Break-rules dashboard.
    gate(
        is_settings(window.label()) || is_breaks(window.label()),
        "settings/rules",
    )?;
    let snapshot = state.engine.lock().unwrap().status();
    Ok(StatusDto {
        state: snapshot.state.as_str(),
        all: snapshot
            .all
            .into_iter()
            .map(|n| NextBreakDto {
                rule_id: n.rule_id,
                rule_name: n.rule_name,
                remaining_secs: n.remaining_secs,
            })
            .collect(),
    })
}

/// Open to all windows — each window pings on load to confirm it rendered. The label
/// is caller-controllable, so sanitize it (drop non-token chars, cap length) before
/// logging to avoid log injection.
#[tauri::command]
pub fn cmd_window_ready(label: String) {
    let safe: String = label
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(32)
        .collect();
    crate::rlog!("gomaju: window content loaded: {safe}");
}

/// Result of saving config: the (possibly sanitized) config echoed back, plus any
/// hotkey accelerators that could not be registered.
#[derive(Serialize)]
pub struct SaveOutcome {
    pub config: ConfigFile,
    pub hotkey_errors: Vec<String>,
}

/// Validate + persist edited config, then apply it live: reconfigure the engine,
/// re-register hotkeys, and sync autostart.
#[tauri::command]
pub fn cmd_save_config(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, AppState>,
    mut config: ConfigFile,
) -> Result<SaveOutcome, String> {
    require_settings(&window)?;
    config.sanitize();
    // Replace the persisted config under one held lock (write+swap atomic vs. the ticker's
    // once-disable). Locale is backend-owned (only the tray / Language card changes it). The
    // Settings form never edits it, and an incoming payload without the field would serde-default
    // to "zh-Hant" — so preserve the stored value rather than let a Save clobber a language switch.
    let config = state
        .with_config_write(move |cur| {
            let locale = cur.locale.clone();
            *cur = config;
            cur.locale = locale;
            true
        })?
        .expect("save always writes");

    reconfigure_engine(&app, state.inner(), &config);
    runtime::sync_pause_reminder(&app);
    // A `show_timer_toasts` toggle is picked up by the scheduler's next reconcile tick (toast
    // windows must be created off this main-thread command path — see `timer_toast`).

    let hotkey_errors = hotkeys::apply(&app, &config.hotkeys);
    autostart::apply(&app, config.autostart);

    Ok(SaveOutcome {
        config,
        hotkey_errors,
    })
}

// --- Break quotes (settings-window only; stored in quotes.toml, separate from config.toml) ---

/// One locale's break quotes (sanitized). Read by the Settings "Quotes" card on load, on focus, and
/// on locale switch, and re-read inside the card's Save to detect external edits to `quotes.toml`.
/// A read never writes (`read_quotes`); `locale` canonicalizes to one of the two supported sets.
#[tauri::command]
pub fn cmd_get_quotes(
    window: WebviewWindow,
    state: State<'_, AppState>,
    locale: String,
) -> Result<Vec<String>, String> {
    require_settings(&window)?;
    let file = gomaju_core::quotes::read_quotes(&state.quotes_path);
    Ok(file.get(&locale).to_vec())
}

/// Persist one locale's edited quote list into `quotes.toml` via read-modify-write: read the current
/// file (non-writing), replace only this locale's array, sanitize, write. Replacing one locale means
/// saving `en` never clobbers `zh-Hant` (the Settings window saves each locale in a separate call).
/// Uses `read_quotes` (not `load_quotes`) so a save never triggers migration/backup writes and stays
/// symmetric with `cmd_get_quotes`. Returns the sanitized list so the form reflects any trimmed/dropped
/// rows (like `cmd_save_config`). Quotes are re-read live on each break, so there's no in-memory cache.
#[tauri::command]
pub fn cmd_save_quotes(
    window: WebviewWindow,
    state: State<'_, AppState>,
    locale: String,
    quotes: Vec<String>,
) -> Result<Vec<String>, String> {
    require_settings(&window)?;
    let mut file = gomaju_core::quotes::read_quotes(&state.quotes_path);
    file.set(&locale, quotes);
    file.sanitize();
    gomaju_core::quotes::save_quotes(&state.quotes_path, &file).map_err(|e| e.to_string())?;
    Ok(file.get(&locale).to_vec())
}

// --- Alarms (alarms-window only) ---

#[tauri::command]
pub fn cmd_get_alarms(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<AlarmDto>, String> {
    require_alarms(&window)?;
    Ok(state.config.lock().unwrap().alarms.clone())
}

/// The next fire instant for one enabled alarm, for the Alarms window's "Next: …" label.
#[derive(Serialize)]
pub struct AlarmFireDto {
    pub id: String,
    /// Unix timestamp (seconds) of the next fire.
    pub at_secs: i64,
}

/// Compute the next fire time of every *enabled* alarm whose schedule still has one.
/// Reflects the saved config (the window refreshes this on load / save / focus).
#[tauri::command]
pub fn cmd_get_alarm_fires(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<AlarmFireDto>, String> {
    require_alarms(&window)?;
    let now = chrono::Local::now();
    let alarms = state.config.lock().unwrap().alarms.clone();
    let fires = alarms
        .iter()
        .filter_map(|a| {
            crate::alarm::next_fire(a, now).map(|when| AlarmFireDto {
                id: a.id.clone(),
                at_secs: when.timestamp(),
            })
        })
        .collect();
    Ok(fires)
}

/// Persist the edited alarm list. Clone the current config, swap in the new alarms,
/// sanitize, write to disk, and only then update the in-memory cache — so a failed
/// write never leaves the cache ahead of the file. Returns the sanitized alarms so the
/// UI reflects any normalization (disabled empty-weekly alarms, regenerated ids, etc.).
#[tauri::command]
pub fn cmd_save_alarms(
    window: WebviewWindow,
    state: State<'_, AppState>,
    alarms: Vec<AlarmDto>,
) -> Result<Vec<AlarmDto>, String> {
    require_alarms(&window)?;

    // Swap in the new alarms on a clone taken *inside* the held lock, so a concurrent ticker
    // once-disable (or a language switch) between the clone and the write can't be clobbered.
    // Only the alarms changed here, so validate just those (rules/settings were already sanitized
    // at load / their own save); the cached locale rides along untouched.
    let config = state
        .with_config_write(move |cur| {
            cur.alarms = alarms;
            alarm::sanitize_alarms(&mut cur.alarms);
            true
        })?
        .expect("save always writes");
    Ok(config.alarms)
}

#[tauri::command]
pub fn cmd_close_alarms(window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    require_alarms(&window)?;
    alarms_window::close(&app);
    Ok(())
}

// --- Countdown timers (timers-window only) ---

/// One timer's saved definition plus its live run state, for the Timers window.
#[derive(Serialize)]
pub struct CountdownView {
    pub def: CountdownDto,
    /// "idle" | "running" | "paused".
    pub state: &'static str,
    /// Whole seconds left (ceil); the full duration when idle.
    pub remaining_secs: u32,
}

/// The saved timers joined with their in-memory run state. The window polls this each second.
#[tauri::command]
pub fn cmd_get_countdowns(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<CountdownView>, String> {
    require_timers(&window)?;
    let now = std::time::Instant::now();
    let defs = state.config.lock().unwrap().countdowns.clone();
    let map = state.countdown_runtime.lock().unwrap();
    let views = defs
        .into_iter()
        .map(|def| {
            let run = map.get(&def.id);
            let remaining = match run {
                Some(r) => crate::countdown::remaining_secs(r, now),
                None => def.duration_secs, // idle -> show the full duration
            };
            CountdownView {
                state: crate::countdown::state_str(run),
                remaining_secs: remaining,
                def,
            }
        })
        .collect();
    Ok(views)
}

/// Persist the edited timer list (clone → swap → sanitize → write → swap cache, like
/// `cmd_save_alarms`), then prune run state for any timers that were deleted. Returns the
/// sanitized list **with live run state** (so the UI re-render keeps running timers' state
/// instead of flashing them idle). Note: an id regenerated by sanitize loses its run entry on
/// prune — i.e. saving an edited *running* timer resets it, acceptable for a local single-user app.
#[tauri::command]
pub fn cmd_save_countdowns(
    window: WebviewWindow,
    state: State<'_, AppState>,
    countdowns: Vec<CountdownDto>,
) -> Result<Vec<CountdownView>, String> {
    require_timers(&window)?;
    let config = state
        .with_config_write(move |cur| {
            cur.countdowns = countdowns;
            gomaju_core::countdown::sanitize_countdowns(&mut cur.countdowns);
            true
        })?
        .expect("save always writes");
    // Drop any pending "time's up" toast for a timer that was just deleted (separate lock scope —
    // never held with the runtime lock). sync() also prunes by config membership; this is immediate.
    {
        let valid: std::collections::HashSet<&str> =
            config.countdowns.iter().map(|c| c.id.as_str()).collect();
        state
            .finished_toasts
            .lock()
            .unwrap()
            .retain(|id, _| valid.contains(id.as_str()));
    }
    let now = std::time::Instant::now();
    let mut map = state.countdown_runtime.lock().unwrap();
    {
        let valid: std::collections::HashSet<&str> =
            config.countdowns.iter().map(|c| c.id.as_str()).collect();
        map.retain(|id, _| valid.contains(id.as_str()));
    }
    let views = config
        .countdowns
        .iter()
        .map(|def| {
            let run = map.get(&def.id);
            CountdownView {
                state: crate::countdown::state_str(run),
                remaining_secs: run
                    .map(|r| crate::countdown::remaining_secs(r, now))
                    .unwrap_or(def.duration_secs),
                def: def.clone(),
            }
        })
        .collect();
    // Deleted timers' toasts are closed by the scheduler's next reconcile tick.
    Ok(views)
}

/// Start (or resume) a timer. No-op if `id` isn't a saved timer (avoids orphan run state).
// Toast windows are reconciled by the countdown scheduler thread (see `timer_toast::sync`), NOT
// here — creating a webview from a command (main-thread WebView2 IPC callback) deadlocks on
// Windows. These commands only mutate the in-memory run state; the toast appears/closes on the
// next ~250 ms scheduler tick.
#[tauri::command]
pub fn cmd_start_countdown(
    window: WebviewWindow,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    require_timers(&window)?;
    let duration = state
        .config
        .lock()
        .unwrap()
        .countdowns
        .iter()
        .find(|c| c.id == id)
        .map(|c| std::time::Duration::from_secs(c.duration_secs as u64));
    let Some(duration) = duration else {
        return Ok(()); // unknown/unsaved id -> no-op
    };
    let now = std::time::Instant::now();
    let mut map = state.countdown_runtime.lock().unwrap();
    crate::countdown::start(&mut map, &id, duration, now);
    Ok(())
}

/// Pause a running timer (no-op otherwise).
#[tauri::command]
pub fn cmd_pause_countdown(
    window: WebviewWindow,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    require_timers(&window)?;
    let now = std::time::Instant::now();
    let mut map = state.countdown_runtime.lock().unwrap();
    crate::countdown::pause(&mut map, &id, now);
    Ok(())
}

/// Reset a timer back to idle.
#[tauri::command]
pub fn cmd_reset_countdown(
    window: WebviewWindow,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    require_timers(&window)?;
    let mut map = state.countdown_runtime.lock().unwrap();
    crate::countdown::reset(&mut map, &id);
    Ok(())
}

#[tauri::command]
pub fn cmd_close_countdowns(window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    require_timers(&window)?;
    timers_window::close(&app);
    Ok(())
}

/// The ✕ on a running-timer toast: stop (reset) that timer. The id comes from the toast's **own**
/// window label, so it can't target another timer; resetting it makes the scheduler's next
/// reconcile tick close this toast.
#[tauri::command]
pub fn cmd_toast_stop_countdown(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require_timer_toast(&window)?;
    if let Some(id) = crate::timer_toast::id_from_label(window.label()) {
        let id = id.to_string();
        let mut map = state.countdown_runtime.lock().unwrap();
        crate::countdown::reset(&mut map, &id);
    }
    Ok(())
}

/// The ✕ on a finished-timer "time's up" toast: drop its entry so the scheduler's next reconcile
/// tick closes the window. The id comes from the toast's **own** window label (no spoofable arg);
/// we never create or close windows from this command (that would risk the WebView2 main-thread
/// deadlock) — only mutate state.
#[tauri::command]
pub fn cmd_dismiss_timer_done(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require_timer_done(&window)?;
    if let Some(id) = crate::timer_toast::id_from_done_label(window.label()) {
        state.finished_toasts.lock().unwrap().remove(id);
    }
    Ok(())
}

// --- Chimes (chimes-window writes; settings/alarms/timers/chimes may read the list) ---

/// The saved chime list. Readable from the windows that show a chime picker (settings rules
/// editor + alarms) and the chimes editor itself; writes stay chimes-only.
#[tauri::command]
pub fn cmd_get_chimes(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<ChimeDto>, String> {
    gate(
        is_settings(window.label())
            || is_alarms(window.label())
            || is_timers(window.label())
            || is_chimes(window.label()),
        "settings/alarms/timers/chimes",
    )?;
    Ok(state.chimes.lock().unwrap().clone())
}

/// Persist the edited chime list (clone → sanitize → save → swap, like `cmd_save_alarms`). Uses the
/// full `sanitize` so a chime deleted here also clears any now-dangling rule/alarm references, then
/// prunes imported files no longer referenced by any saved chime. Returns the sanitized list.
#[tauri::command]
pub fn cmd_save_chimes(
    window: WebviewWindow,
    state: State<'_, AppState>,
    chimes: Vec<ChimeDto>,
) -> Result<Vec<ChimeDto>, String> {
    require_chimes(&window)?;

    let mut file = ChimesFile { chimes };
    file.sanitize();
    let sanitized = file.chimes.clone();

    // Hold the cache lock across the write + swap so a failed write never leaves the cache ahead of
    // disk. Chimes live in chimes.toml — this never touches config.toml.
    let mut guard = state.chimes.lock().unwrap();
    chime::save_chimes(&state.chimes_path, &file).map_err(|e| e.to_string())?;
    *guard = file.chimes;
    drop(guard);

    prune_orphan_chime_files(&state, &sanitized);
    Ok(sanitized)
}

/// Best-effort: delete any unreferenced **audio** file in the chimes folder. Restricted to known
/// audio extensions so it never removes `chimes.toml` (or its `.bak`/`.tmp`), which share the folder.
fn prune_orphan_chime_files(state: &AppState, chimes: &[ChimeDto]) {
    const AUDIO_EXTS: [&str; 4] = ["wav", "mp3", "ogg", "flac"];
    let Some(dir) = state.chimes_path.parent() else {
        return;
    };
    let referenced: std::collections::HashSet<&str> = chimes
        .iter()
        .filter(|c| matches!(c.kind, gomaju_core::chime::ChimeKindDto::File))
        .map(|c| c.file.as_str())
        .collect();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let is_audio = entry
            .path()
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| AUDIO_EXTS.contains(&e.to_ascii_lowercase().as_str()))
            .unwrap_or(false);
        if is_audio {
            if let Some(name) = entry.file_name().to_str() {
                if !referenced.contains(name) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
}

/// Preview a chime live — the in-editor (possibly unsaved) definition. The chime is sanitized
/// first so an extreme unsaved value can't blast; tones play from `steps`, a file chime plays the
/// already-imported file from the chimes dir. The preview is **stoppable**: this returns a
/// generation token, the playback emits `preview-ended` with that token when it finishes, and
/// `cmd_stop_preview` halts it. Returns `0` when there's nothing to play (so the UI stays idle).
#[tauri::command]
pub fn cmd_preview_chime(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, AppState>,
    chime: ChimeDto,
) -> Result<u64, String> {
    require_chimes(&window)?;
    let mut one = vec![chime];
    chime::sanitize_chimes(&mut one);
    let Some(chime) = one.into_iter().next() else {
        return Ok(0); // sanitized away (e.g. tones with no steps) — nothing to play
    };
    let gen = match chime.kind {
        gomaju_core::chime::ChimeKindDto::Tones => {
            crate::audio::preview_chime_spec(app, chime.steps, config::default_chime_volume())
        }
        gomaju_core::chime::ChimeKindDto::File => {
            match state.config_path.parent().map(|p| p.join("chimes")) {
                Some(dir) => crate::audio::preview_chime_file(
                    app,
                    dir.join(&chime.file),
                    config::default_chime_volume(),
                ),
                None => 0,
            }
        }
    };
    Ok(gen)
}

/// Preview the chime currently selected in a rule/alarm picker, **by id** and picker volume: the
/// saved chime, or the context's built-in default tone when the id is empty/unknown. `kind`
/// (`break_start` | `break_over` | `alarm`) picks which default tone — an unknown kind is rejected. Stoppable like
/// `cmd_preview_chime`; returns the generation token (0 = nothing played). Readable from the windows
/// that show a chime picker (settings rules editor + alarms); the chimes window uses the def-based
/// `cmd_preview_chime` instead.
#[tauri::command]
pub fn cmd_preview_chime_by_id(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, AppState>,
    chime_id: String,
    volume_pct: u8,
    kind: String,
) -> Result<u64, String> {
    gate(shows_chime_picker(window.label()), "settings/alarms")?;
    let tone = match kind.as_str() {
        "break_start" => crate::audio::DefaultTone::BreakStart,
        "break_over" => crate::audio::DefaultTone::BreakOver,
        "alarm" => crate::audio::DefaultTone::Alarm,
        other => return Err(format!("unknown chime kind: {other}")),
    };
    let dir = state
        .config_path
        .parent()
        .map(|p| p.join("chimes"))
        .unwrap_or_default();
    let chimes = state.chimes.lock().unwrap();
    Ok(crate::audio::preview_assigned_or(
        app,
        &chime_id,
        volume_pct.min(100),
        &chimes,
        &dir,
        tone,
    ))
}

/// Stop the currently-playing chime preview (the ⏸ Pause in the Chimes, Settings, or Alarms window).
#[tauri::command]
pub fn cmd_stop_preview(window: WebviewWindow) -> Result<(), String> {
    gate(has_chime_preview(window.label()), "settings/alarms/chimes")?;
    crate::audio::stop_preview();
    Ok(())
}

/// Open a native file picker, copy the chosen audio file into `<config_dir>/chimes/<chime_id>.<ext>`,
/// and return the stored **bare** filename (or `None` if cancelled). The editor then sets the
/// chime's `kind = "file"` + `file = <returned>`. Async so the dialog runs off the main thread.
#[tauri::command]
pub async fn cmd_import_chime_file(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, AppState>,
    chime_id: String,
) -> Result<Option<String>, String> {
    require_chimes(&window)?;
    // `chime_id` is a frontend-generated UUID; require it to be a safe bare filename component
    // (it becomes the stored file's stem), so a crafted id can't escape the chimes dir.
    if !chime::is_safe_filename(&chime_id) {
        return Err("invalid chime id".into());
    }
    let dir = state
        .config_path
        .parent()
        .map(|p| p.join("chimes"))
        .ok_or("no config dir")?;

    // Brand the native picker so the user can tell it's Gomaju asking. Read the locale in a short
    // scope so the config lock is released before the (blocking) dialog opens.
    let title = {
        let cfg = state.config.lock().unwrap();
        crate::i18n::tr(&cfg.locale, "dialog.import_chime_title").to_string()
    };
    let picked = app
        .dialog()
        .file()
        .set_title(title)
        .add_filter("Audio", &["wav", "mp3", "ogg", "flac"])
        .blocking_pick_file();
    let Some(src) = picked.and_then(|fp| fp.into_path().ok()) else {
        return Ok(None); // cancelled
    };
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .filter(|e| ["wav", "mp3", "ogg", "flac"].contains(&e.as_str()))
        .ok_or("unsupported audio format")?;

    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let filename = format!("{chime_id}.{ext}");
    std::fs::copy(&src, dir.join(&filename)).map_err(|e| e.to_string())?;
    Ok(Some(filename))
}

#[tauri::command]
pub fn cmd_close_chimes(window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    require_chimes(&window)?;
    chimes_window::close(&app);
    Ok(())
}

/// Open the chimes folder (where `chimes.toml` + imported sounds live) in the OS file manager.
/// Creates it first so the button always works, even before any import.
#[tauri::command]
pub fn cmd_open_chimes_folder(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require_chimes(&window)?;
    let Some(dir) = state.chimes_path.parent() else {
        return Err("no chimes folder".into());
    };
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    open_in_file_manager(dir).map_err(|e| e.to_string())?;
    Ok(())
}

/// Launch the platform file manager on `path` (best-effort, detached). `spawn` returns immediately
/// (no blocking / event-loop pumping), so this is safe to call from a synchronous command.
fn open_in_file_manager(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = "xdg-open";
    std::process::Command::new(program).arg(path).spawn()?;
    Ok(())
}

// --- Break rules (breaks-window only) ---

#[tauri::command]
pub fn cmd_get_rules(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<RuleDto>, String> {
    require_breaks(&window)?;
    Ok(state.config.lock().unwrap().rules.clone())
}

/// Set just the `enabled`/`repeat` flags of one rule (the quick-select dashboard's only
/// edits) and apply live. Merge-by-id onto the *fresh* cached config so it can never clobber
/// detail edits made in Settings. Clone/edit/sanitize/write/commit under one held `config`
/// lock (so the ticker's once-rule auto-disable can't interleave a stale snapshot); drop the
/// lock before reconfiguring the engine. The JS side passes camelCase `ruleId` (Tauri maps
/// it to `rule_id`). Returns `()` — the dashboard updates optimistically.
#[tauri::command]
pub fn cmd_set_rule_flags(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, AppState>,
    rule_id: String,
    enabled: bool,
    repeat: bool,
) -> Result<(), String> {
    require_breaks(&window)?;

    // Merge the flag change onto a fresh in-lock clone (so it can never clobber Settings detail
    // edits or a concurrent once-disable), then write+swap under the held lock.
    let written = state.with_config_write(|cur| {
        let Some(rule) = cur.rules.iter_mut().find(|r| r.id == rule_id) else {
            return false; // rule no longer exists (e.g. deleted in Settings) — no-op
        };
        rule.enabled = enabled;
        rule.repeat = repeat;
        config::sanitize_rules(&mut cur.rules);
        true
    })?;

    if let Some(config) = written {
        reconfigure_engine(&app, state.inner(), &config);
    }
    Ok(())
}

#[tauri::command]
pub fn cmd_close_breaks(window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    require_breaks(&window)?;
    breaks_window::close(&app);
    Ok(())
}

/// Open the Settings window from the rules dashboard's "Edit in Settings…" button.
#[tauri::command]
pub async fn cmd_open_settings(window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    // Async so this runs off the main thread. Creating a window *synchronously* from within
    // a webview's IPC handler pumps a nested event loop and deadlocks the app (the tray path
    // is a native menu event, so it's unaffected). `settings_window::open` marshals the actual
    // window build to the main thread via `run_on_main_thread`, which now posts cleanly from
    // this off-main-thread command instead of re-entering the loop.
    require_breaks(&window)?;
    settings_window::open(&app);
    Ok(())
}

/// Open the Chimes window from the Settings "Chimes" card's "Open chime editor" button.
/// Async + main-thread-marshalled for the same deadlock reason as `cmd_open_settings`.
#[tauri::command]
pub async fn cmd_open_chimes(window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    require_settings(&window)?;
    chimes_window::open(&app);
    Ok(())
}

/// Switch the whole-app UI language from the Settings "Language" card — same effect as the tray's
/// Language menu (persist + relabel the tray). Open windows pick up the new language when reopened.
/// Sync (creates no window), so it runs on the main thread like the tray path.
#[tauri::command]
pub fn cmd_set_locale(window: WebviewWindow, app: AppHandle, locale: String) -> Result<(), String> {
    require_settings(&window)?;
    if locale != "en" && locale != "zh-Hant" {
        return Err(format!("unsupported locale: {locale}"));
    }
    runtime::set_locale(&app, &locale);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        has_chime_preview, is_alarms, is_breaks, is_chimes, is_settings, is_timer_done, is_timers,
        shows_chime_picker,
    };

    #[test]
    fn only_the_settings_window_is_privileged() {
        assert!(is_settings("settings"));
        // Overlay, toast, alarms, and anything else must be rejected.
        assert!(!is_settings("overlay-0"));
        assert!(!is_settings("overlay-1"));
        assert!(!is_settings("warning-toast"));
        assert!(!is_settings("alarms"));
        assert!(!is_settings("breaks"));
        assert!(!is_settings("Settings")); // case-sensitive
        assert!(!is_settings(""));
    }

    #[test]
    fn only_the_alarms_window_is_privileged_for_alarm_commands() {
        assert!(is_alarms("alarms"));
        assert!(!is_alarms("settings"));
        assert!(!is_alarms("breaks"));
        assert!(!is_alarms("overlay-0"));
        assert!(!is_alarms("warning-toast"));
        assert!(!is_alarms("Alarms")); // case-sensitive
        assert!(!is_alarms(""));
    }

    #[test]
    fn only_the_breaks_window_is_privileged_for_rule_commands() {
        assert!(is_breaks("breaks"));
        assert!(!is_breaks("settings"));
        assert!(!is_breaks("alarms"));
        assert!(!is_breaks("overlay-0"));
        assert!(!is_breaks("Breaks")); // case-sensitive
        assert!(!is_breaks(""));
    }

    #[test]
    fn only_the_chimes_window_is_privileged_for_chime_writes() {
        assert!(is_chimes("chimes"));
        assert!(!is_chimes("settings"));
        assert!(!is_chimes("alarms"));
        assert!(!is_chimes("breaks"));
        assert!(!is_chimes("Chimes")); // case-sensitive
        assert!(!is_chimes(""));
    }

    #[test]
    fn only_the_timers_window_is_privileged_for_timer_commands() {
        assert!(is_timers("timers"));
        assert!(!is_timers("settings"));
        assert!(!is_timers("alarms"));
        assert!(!is_timers("breaks"));
        assert!(!is_timers("overlay-0"));
        assert!(!is_timers("Timers")); // case-sensitive
        assert!(!is_timers(""));
    }

    #[test]
    fn only_chime_picker_windows_may_preview_by_id() {
        // cmd_preview_chime_by_id: the windows that show a chime <select>.
        assert!(shows_chime_picker("settings"));
        assert!(shows_chime_picker("alarms"));
        assert!(shows_chime_picker("timers"));
        // Chimes previews unsaved defs via cmd_preview_chime, not by id; everything else is denied.
        assert!(!shows_chime_picker("chimes"));
        assert!(!shows_chime_picker("breaks"));
        assert!(!shows_chime_picker("overlay-0"));
        assert!(!shows_chime_picker("warning-toast"));
        assert!(!shows_chime_picker(""));
    }

    #[test]
    fn chime_preview_windows_may_stop_preview() {
        // cmd_stop_preview: the chime pickers plus the chimes editor.
        assert!(has_chime_preview("settings"));
        assert!(has_chime_preview("alarms"));
        assert!(has_chime_preview("timers"));
        assert!(has_chime_preview("chimes"));
        assert!(!has_chime_preview("breaks"));
        assert!(!has_chime_preview("overlay-0"));
        assert!(!has_chime_preview("warning-toast"));
        assert!(!has_chime_preview(""));
    }

    #[test]
    fn only_a_timer_done_window_may_dismiss() {
        assert!(is_timer_done("timer-done-abc"));
        // The running-toast family and every other window must be rejected.
        assert!(!is_timer_done("timer-toast-abc"));
        assert!(!is_timer_done("timers"));
        assert!(!is_timer_done("settings"));
        assert!(!is_timer_done(""));
    }
}
