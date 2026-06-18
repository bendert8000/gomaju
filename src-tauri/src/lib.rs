mod alarm;
mod alarm_toast;
mod alarms_window;
mod app_state;
mod audio;
mod autostart;
mod breaks_window;
mod chimes_window;
mod commands;
mod confirm;
mod countdown;
mod hotkeys;
mod i18n;
mod idle;
mod logging;
mod migrate;
mod overlay;
mod pause_toast;
mod quotes;
mod runtime;
mod settings_window;
mod stopwatch_window;
mod timer_toast;
mod timers_window;
mod toast;
mod tray;
mod webview;

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::Manager;

use gomaju_core::{config, Engine};

use app_state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // single-instance must be registered first. A second launch opens Settings
        // on the already-running instance so the user gets feedback + an entry point.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            settings_window::open(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle().clone();

            // Resolve the config file path under the OS config dir.
            let config_path: PathBuf = handle
                .path()
                .app_config_dir()
                .map(|dir| dir.join("config.toml"))
                .unwrap_or_else(|_| PathBuf::from("gomaju-config.toml"));

            // One-time rebrand migration: if this is a first run under the new identifier
            // (com.gomaju.app) but the old com.restee.app config dir exists, copy the user's data
            // across. MUST run before config::load, which would otherwise create a fresh default dir.
            if let Some(dir) = config_path.parent() {
                migrate::from_legacy_identifier(dir);
            }

            let outcome = config::load(&config_path).map_err(|e| e.to_string())?;
            // config::load created the config dir, so it's safe to point the file logger at it now.
            // Every `gomaju:` diagnostic from here on is teed to <config_dir>/gomaju.log.
            logging::init(
                config_path
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new(".")),
            );
            if outcome.created {
                crate::rlog!("gomaju: wrote default config to {}", config_path.display());
            }
            if let Some(backup) = &outcome.recovered_backup {
                crate::rlog!(
                    "gomaju: config was unreadable; backed up to {} and restored defaults",
                    backup.display()
                );
            }

            // Break quotes live in their own quotes.toml (separate from config.toml). On first run
            // this migrates the old per-locale quotes.<locale>.txt files into it and deletes them;
            // it also self-heals a missing/corrupt file. pick re-reads it live each break.
            let quotes_path: PathBuf = config_path
                .parent()
                .map(|dir| dir.join("quotes.toml"))
                .unwrap_or_else(|| PathBuf::from("quotes.toml"));
            if let Err(e) = gomaju_core::quotes::load_quotes(&quotes_path) {
                crate::rlog!("gomaju: could not initialize quotes.toml ({e})");
            }

            // Load saved chimes from their own file (chimes/chimes.toml), kept separate from
            // config.toml. This also creates the chimes folder (which holds imported sounds) and
            // seeds the default presets on first run.
            let chimes_path: PathBuf = config_path
                .parent()
                .map(|dir| dir.join("chimes").join("chimes.toml"))
                .unwrap_or_else(|| PathBuf::from("chimes/chimes.toml"));
            let chimes = match gomaju_core::chime::load_chimes(&chimes_path) {
                Ok(file) => file.chimes,
                Err(e) => {
                    crate::rlog!("gomaju: could not load chimes.toml ({e}); starting with none");
                    Vec::new()
                }
            };

            // Persisted break progress lives in its own session.toml next to config.toml. The
            // ticker autosaves it; cold start reads it to offer "resume previous progress?".
            let session_path: PathBuf = config_path
                .parent()
                .map(|dir| dir.join("session.toml"))
                .unwrap_or_else(|| PathBuf::from("session.toml"));

            let cfg = outcome.config;
            let autostart_wanted = cfg.autostart;
            let hotkeys_cfg = cfg.hotkeys.clone();
            let notify_on_start = cfg.settings.notifications;

            // Only offer to resume if a saved snapshot is recent (not stale/future-dated) and holds
            // meaningful work for a still-enabled rule. Constants are single-source + easy to tune.
            const SESSION_MAX_AGE_SECS: i64 = 12 * 3600;
            const MEANINGFUL_WORK_SECS: u64 = 60;

            let (rules, settings) = cfg.to_engine_inputs();
            let saved = gomaju_core::progress::read_progress(&session_path);
            let now_unix = chrono::Utc::now().timestamp();
            // Off in Settings -> never ask, always start fresh.
            let should_prompt = cfg.settings.resume_prompt_enabled
                && saved.as_ref().is_some_and(|s| {
                let age = now_unix - s.saved_at;
                if !(0..=SESSION_MAX_AGE_SECS).contains(&age) {
                    return false;
                }
                let enabled: std::collections::HashSet<&str> =
                    rules.iter().filter(|r| r.enabled).map(|r| r.id.as_str()).collect();
                s.rules.iter().any(|rp| {
                    rp.work_secs >= MEANINGFUL_WORK_SECS && enabled.contains(rp.rule_id.as_str())
                })
            });

            let mut engine = Engine::new(rules, settings);
            let running_since;
            let mut resume_age_secs = 0u64;
            if should_prompt {
                // Restore the saved work but leave the engine Stopped until the user answers the
                // resume dialog (so a due rule can't fire a break before they decide).
                let s = saved.as_ref().unwrap();
                engine.restore_progress(&s.rules);
                resume_age_secs = (now_unix - s.saved_at).max(0) as u64;
                running_since = None;
            } else {
                // No prompt: begin counting on launch (fresh, work stays zero); the user can pause.
                let _ = engine.start();
                running_since = Some(std::time::Instant::now());
            }
            let initial_state = engine.state();

            let idle = idle::detect();
            let idle_status = idle.status();
            crate::rlog!("gomaju: idle source status = {idle_status:?}");

            app.manage(AppState {
                engine: Mutex::new(engine),
                config: Mutex::new(cfg),
                config_path,
                chimes: Mutex::new(chimes),
                chimes_path,
                quotes_path,
                session_path,
                idle_status,
                // `Some` when we started counting now; `None` while the resume prompt is pending.
                running_since: Mutex::new(running_since),
                pause_reminder: Mutex::new(Default::default()),
                // Countdown timers always start idle (run state is never persisted).
                countdown_runtime: Mutex::new(Default::default()),
                // "Time's up!" toasts are never persisted (cold start has none), like run state.
                finished_toasts: Mutex::new(Default::default()),
                // Pending alarm toasts are never persisted either (cold start has none).
                fired_alarm_toasts: Mutex::new(Default::default()),
            });

            tray::build_tray(&handle)?;
            // Reflect the actual boot state (Stopped while the resume prompt is pending).
            runtime::refresh_tray(&handle, initial_state);
            runtime::spawn_ticker(handle.clone(), idle);
            // Wall-clock alarms run on their own thread, independent of the break engine.
            alarm::spawn_scheduler(handle.clone());
            // Countdown timers fire on their own faster (~250ms) thread, also engine-independent.
            countdown::spawn_scheduler(handle.clone());

            // Apply persisted hotkeys + autostart preference.
            let hotkey_errors = hotkeys::apply(&handle, &hotkeys_cfg);
            for err in &hotkey_errors {
                crate::rlog!("gomaju: hotkey not registered — {err}");
            }
            autostart::apply(&handle, autostart_wanted);

            // Cold start: open the break-rules window so setup is front-and-center. Debug
            // builds honor GOMAJU_NO_OPEN_RULES to suppress the auto-open (handy under
            // `tauri dev`).
            #[cfg(debug_assertions)]
            let open_rules = std::env::var("GOMAJU_NO_OPEN_RULES").is_err();
            #[cfg(not(debug_assertions))]
            let open_rules = true;

            if open_rules {
                breaks_window::open(&handle);
            }

            // Startup tray reminder: teach the user the app keeps running in the system
            // tray after the break-rules window is closed. Respects the notifications
            // setting; on Windows the WinRT toast auto-dismisses after ~3s.
            if notify_on_start {
                let loc = i18n::current_locale(&handle);
                runtime::show_startup_notification(&handle, i18n::tr(&loc, "notif.startup"));
            }

            // A recent saved snapshot exists: ask whether to resume it or start fresh. Shown after
            // the windows are up (layers over the breaks window); the engine stays Stopped until
            // answered, so nothing fires in the meantime.
            if should_prompt {
                runtime::confirm_resume_break_progress(&handle, resume_age_secs);
            }

            // Test/demo aids, compiled only into debug builds (dev / `tauri dev`).
            // Release ignores these env vars. For a hard guarantee one could use a
            // dedicated `dev-hooks` Cargo feature instead; debug_assertions suffices
            // here since the release profile does not enable them.
            #[cfg(debug_assertions)]
            {
                if std::env::var("GOMAJU_OPEN_SETTINGS").is_ok() {
                    settings_window::open(&handle);
                }
                if std::env::var("GOMAJU_OPEN_ALARMS").is_ok() {
                    alarms_window::open(&handle);
                }
                if std::env::var("GOMAJU_OPEN_TIMERS").is_ok() {
                    timers_window::open(&handle);
                }
                if std::env::var("GOMAJU_OPEN_STOPWATCH").is_ok() {
                    stopwatch_window::open(&handle);
                }
                if std::env::var("GOMAJU_BREAK_ON_START").is_ok() {
                    let h = handle.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_secs(2));
                        let state = h.state::<AppState>();
                        runtime::action_break_now(&h, state.inner());
                    });
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::cmd_skip,
            commands::cmd_reset_timer,
            commands::cmd_delay_break,
            commands::cmd_break_now_rule,
            commands::cmd_resume_from_pause_reminder,
            commands::cmd_stay_paused_from_reminder,
            commands::cmd_confirm_resolve,
            commands::cmd_get_config,
            commands::cmd_get_idle_status,
            commands::cmd_get_status,
            commands::cmd_save_config,
            commands::cmd_close_settings,
            commands::cmd_get_app_version,
            commands::cmd_get_quotes,
            commands::cmd_save_quotes,
            commands::cmd_window_ready,
            commands::cmd_get_alarms,
            commands::cmd_get_alarm_fires,
            commands::cmd_save_alarms,
            commands::cmd_close_alarms,
            commands::cmd_get_countdowns,
            commands::cmd_save_countdowns,
            commands::cmd_start_countdown,
            commands::cmd_pause_countdown,
            commands::cmd_reset_countdown,
            commands::cmd_close_countdowns,
            commands::cmd_close_stopwatch,
            commands::cmd_stopwatch_beep,
            commands::cmd_toast_stop_countdown,
            commands::cmd_dismiss_timer_done,
            commands::cmd_dismiss_alarm_toast,
            commands::cmd_toast_play_chime,
            commands::cmd_get_rules,
            commands::cmd_set_rule_flags,
            commands::cmd_close_breaks,
            commands::cmd_open_settings,
            commands::cmd_open_chimes,
            commands::cmd_set_locale,
            commands::cmd_reload_localized_windows,
            commands::cmd_reload_window,
            commands::cmd_get_chimes,
            commands::cmd_save_chimes,
            commands::cmd_preview_chime,
            commands::cmd_preview_chime_by_id,
            commands::cmd_stop_preview,
            commands::cmd_import_chime_file,
            commands::cmd_close_chimes,
            commands::cmd_open_chimes_folder,
        ])
        .build(tauri::generate_context!())
        .expect("error while building the gomaju application")
        .run(|app, event| {
            // No persistent window: keep the app alive (tray-resident) when an
            // overlay/settings window closes (`code == None`). But honor an explicit
            // quit — `app.exit(code)` arrives here with `code == Some(_)`, and must
            // NOT be prevented, or the tray "Quit" does nothing.
            if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                } else {
                    // Real quit. Arm the force-exit watchdog FIRST, so even if the save below — or
                    // Tauri's window teardown that follows — wedges on a stuck overlay webview, the
                    // process still dies on its own (no more Task-Manager kills).
                    runtime::arm_quit_watchdog();
                    // Clean quit (tray "Quit" -> app.exit(0)): final break-progress save. A forced
                    // OS reboot won't run this — the ticker's periodic autosave is the safety net.
                    runtime::persist_progress(app);
                }
            }
        });
}
