mod alarm;
mod alarms_window;
mod app_state;
mod audio;
mod autostart;
mod breaks_window;
mod chimes_window;
mod commands;
mod hotkeys;
mod i18n;
mod idle;
mod overlay;
mod pause_toast;
mod quotes;
mod runtime;
mod settings_window;
mod toast;
mod tray;
mod webview;

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::Manager;

use restee_core::{config, Engine};

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
                .unwrap_or_else(|_| PathBuf::from("restee-config.toml"));

            let outcome = config::load(&config_path).map_err(|e| e.to_string())?;
            if outcome.created {
                eprintln!("restee: wrote default config to {}", config_path.display());
            }
            if let Some(backup) = &outcome.recovered_backup {
                eprintln!(
                    "restee: config was unreadable; backed up to {} and restored defaults",
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
            if let Err(e) = restee_core::quotes::load_quotes(&quotes_path) {
                eprintln!("restee: could not initialize quotes.toml ({e})");
            }

            // Load saved chimes from their own file (chimes/chimes.toml), kept separate from
            // config.toml. This also creates the chimes folder (which holds imported sounds) and
            // seeds the default presets on first run.
            let chimes_path: PathBuf = config_path
                .parent()
                .map(|dir| dir.join("chimes").join("chimes.toml"))
                .unwrap_or_else(|| PathBuf::from("chimes/chimes.toml"));
            let chimes = match restee_core::chime::load_chimes(&chimes_path) {
                Ok(file) => file.chimes,
                Err(e) => {
                    eprintln!("restee: could not load chimes.toml ({e}); starting with none");
                    Vec::new()
                }
            };

            let cfg = outcome.config;
            let autostart_wanted = cfg.autostart;
            let hotkeys_cfg = cfg.hotkeys.clone();
            let notify_on_start = cfg.settings.notifications;

            let (rules, settings) = cfg.to_engine_inputs();
            let mut engine = Engine::new(rules, settings);
            // Begin counting on launch; the user can pause from the tray.
            let _ = engine.start();

            let idle = idle::detect();
            let idle_status = idle.status();
            eprintln!("restee: idle source status = {idle_status:?}");

            app.manage(AppState {
                engine: Mutex::new(engine),
                config: Mutex::new(cfg),
                config_path,
                chimes: Mutex::new(chimes),
                chimes_path,
                quotes_path,
                idle_status,
                // The engine starts Running, so the clock is already ticking.
                running_since: Mutex::new(Some(std::time::Instant::now())),
                pause_reminder: Mutex::new(Default::default()),
            });

            tray::build_tray(&handle)?;
            runtime::refresh_tray(&handle, restee_core::RunState::Running);
            runtime::spawn_ticker(handle.clone(), idle);
            // Wall-clock alarms run on their own thread, independent of the break engine.
            alarm::spawn_scheduler(handle.clone());

            // Apply persisted hotkeys + autostart preference.
            let hotkey_errors = hotkeys::apply(&handle, &hotkeys_cfg);
            for err in &hotkey_errors {
                eprintln!("restee: hotkey not registered — {err}");
            }
            autostart::apply(&handle, autostart_wanted);

            // Cold start: open the break-rules window so setup is front-and-center. The
            // visible window already signals the app started, so we skip the otherwise-
            // redundant "Restee is running now" toast when opening it. Debug builds honor
            // RESTEE_NO_OPEN_RULES to suppress the auto-open (handy under `tauri dev`).
            #[cfg(debug_assertions)]
            let open_rules = std::env::var("RESTEE_NO_OPEN_RULES").is_err();
            #[cfg(not(debug_assertions))]
            let open_rules = true;

            if open_rules {
                breaks_window::open(&handle);
            } else if notify_on_start {
                let loc = i18n::current_locale(&handle);
                runtime::show_startup_notification(&handle, i18n::tr(&loc, "notif.startup"));
            }

            // Test/demo aids, compiled only into debug builds (dev / `tauri dev`).
            // Release ignores these env vars. For a hard guarantee one could use a
            // dedicated `dev-hooks` Cargo feature instead; debug_assertions suffices
            // here since the release profile does not enable them.
            #[cfg(debug_assertions)]
            {
                if std::env::var("RESTEE_OPEN_SETTINGS").is_ok() {
                    settings_window::open(&handle);
                }
                if std::env::var("RESTEE_OPEN_ALARMS").is_ok() {
                    alarms_window::open(&handle);
                }
                if std::env::var("RESTEE_BREAK_ON_START").is_ok() {
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
            commands::cmd_resume_from_pause_reminder,
            commands::cmd_stay_paused_from_reminder,
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
            commands::cmd_get_rules,
            commands::cmd_set_rule_flags,
            commands::cmd_close_breaks,
            commands::cmd_open_settings,
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
        .expect("error while building the restee application")
        .run(|_app, event| {
            // No persistent window: keep the app alive (tray-resident) when an
            // overlay/settings window closes (`code == None`). But honor an explicit
            // quit — `app.exit(code)` arrives here with `code == Some(_)`, and must
            // NOT be prevented, or the tray "Quit" does nothing.
            if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
