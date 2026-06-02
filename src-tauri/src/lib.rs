mod app_state;
mod audio;
mod autostart;
mod commands;
mod hotkeys;
mod idle;
mod overlay;
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
                idle_status,
                // The engine starts Running, so the clock is already ticking.
                running_since: Mutex::new(Some(std::time::Instant::now())),
            });

            tray::build_tray(&handle)?;
            runtime::refresh_tray(&handle, restee_core::RunState::Running);
            runtime::spawn_ticker(handle.clone(), idle);

            // Apply persisted hotkeys + autostart preference.
            let hotkey_errors = hotkeys::apply(&handle, &hotkeys_cfg);
            for err in &hotkey_errors {
                eprintln!("restee: hotkey not registered — {err}");
            }
            autostart::apply(&handle, autostart_wanted);

            // Let the user know the (windowless) app is up and running in the tray.
            // Auto-dismissed after ~2s so it doesn't linger.
            if notify_on_start {
                runtime::show_startup_notification(&handle, "Restee is running now");
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
            commands::cmd_reset_timers,
            commands::cmd_get_config,
            commands::cmd_get_idle_status,
            commands::cmd_get_status,
            commands::cmd_save_config,
            commands::cmd_close_settings,
            commands::cmd_window_ready,
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
