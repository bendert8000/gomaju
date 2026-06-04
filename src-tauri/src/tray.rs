use std::sync::Mutex;

use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, SubmenuBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

use restee_core::RunState;

use crate::app_state::AppState;
use crate::{i18n, runtime};

const TRAY_ID: &str = "restee-tray";

/// Caches the last rendered menu key so the ticker only rebuilds the tray menu when a
/// visible value actually changes. The menu items themselves are not stored: a
/// variable-length list of break lines means we rebuild + `set_menu` rather than mutate
/// fixed handles (Tauri v2 has no `MenuItem::set_visible`).
pub struct TrayMenu {
    cache: Mutex<String>,
}

/// Build the full tray menu: the status `lines` as disabled info items at the top, then
/// the control items. Start/Pause check state + the Start label are baked in here, so a
/// rebuild reflects run state without separate setter calls. Item ids are stable across
/// rebuilds, so the tray's `on_menu_event` keeps routing.
fn build_menu(
    app: &AppHandle,
    locale: &str,
    started: bool,
    paused: bool,
    start_text: &str,
    lines: &[String],
    alarm_lines: &[String],
) -> tauri::Result<Menu<tauri::Wry>> {
    let status_items: Vec<MenuItem<tauri::Wry>> = lines
        .iter()
        .enumerate()
        .map(|(i, line)| MenuItem::with_id(app, format!("status-{i}"), line, false, None::<&str>))
        .collect::<tauri::Result<Vec<_>>>()?;
    // Today's upcoming alarms — disabled info lines shown in their own section below the breaks.
    let alarm_items: Vec<MenuItem<tauri::Wry>> = alarm_lines
        .iter()
        .enumerate()
        .map(|(i, line)| MenuItem::with_id(app, format!("alarm-line-{i}"), line, false, None::<&str>))
        .collect::<tauri::Result<Vec<_>>>()?;
    let sep_alarms = PredefinedMenuItem::separator(app)?;

    let sep0 = PredefinedMenuItem::separator(app)?;
    // CheckMenuItems render a native check before the text to mark the active state.
    let start = CheckMenuItem::with_id(app, "start", start_text, true, started, None::<&str>)?;
    let pause =
        CheckMenuItem::with_id(app, "pause", i18n::tr(locale, "tray.pause"), true, paused, None::<&str>)?;
    let reset = MenuItem::with_id(app, "reset", i18n::tr(locale, "tray.reset"), true, None::<&str>)?;
    let break_now =
        MenuItem::with_id(app, "break_now", i18n::tr(locale, "tray.break_now"), true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let rules = MenuItem::with_id(app, "breaks", i18n::tr(locale, "tray.rules"), true, None::<&str>)?;
    let alarms = MenuItem::with_id(app, "alarms", i18n::tr(locale, "tray.alarms"), true, None::<&str>)?;
    let chimes = MenuItem::with_id(app, "chimes", i18n::tr(locale, "tray.chimes"), true, None::<&str>)?;
    let settings =
        MenuItem::with_id(app, "settings", i18n::tr(locale, "tray.settings"), true, None::<&str>)?;
    // Language submenu: each language shown in its own name; check reflects the current locale.
    let lang_zh =
        CheckMenuItem::with_id(app, "lang-zh-hant", "繁體中文", true, locale != "en", None::<&str>)?;
    let lang_en = CheckMenuItem::with_id(app, "lang-en", "English", true, locale == "en", None::<&str>)?;
    let language = SubmenuBuilder::new(app, i18n::tr(locale, "tray.language"))
        .item(&lang_zh)
        .item(&lang_en)
        .build()?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", i18n::tr(locale, "tray.quit"), true, None::<&str>)?;

    let mut items: Vec<&dyn IsMenuItem<tauri::Wry>> =
        Vec::with_capacity(status_items.len() + alarm_items.len() + 13);
    for it in &status_items {
        items.push(it);
    }
    // Divider + today's upcoming alarms, only when there are any.
    if !alarm_items.is_empty() {
        items.push(&sep_alarms as &dyn IsMenuItem<tauri::Wry>);
        for it in &alarm_items {
            items.push(it);
        }
    }
    for it in [
        &sep0 as &dyn IsMenuItem<tauri::Wry>,
        &start,
        &pause,
        &reset,
        &break_now,
        &sep1,
        &rules,
        &alarms,
        &chimes,
        &settings,
        &language,
        &sep2,
        &quit,
    ] {
        items.push(it);
    }
    Menu::with_items(app, &items)
}

/// Build the system-tray icon and its menu. This is the app's primary control
/// surface; there is no main window.
pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    // Placeholder menu (built in the current locale so a zh-Hant default doesn't flash
    // English); the immediate `refresh_tray` in setup swaps in live data. The control items
    // carry stable ids so `on_menu_event` routes across later rebuilds.
    let locale = i18n::current_locale(app);
    let placeholder = [i18n::tr(&locale, "tray.placeholder").to_string()];
    let menu = build_menu(app, &locale, true, false, i18n::tr(&locale, "tray.start"), &placeholder, &[])?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("default window icon".into()))?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip(i18n::tr(&locale, "tray.tooltip"))
        .show_menu_on_left_click(true)
        .menu(&menu)
        .on_menu_event(|app, event| {
            let state = app.state::<AppState>();
            match event.id().as_ref() {
                "start" => runtime::action_start(app, state.inner()),
                "pause" => runtime::action_pause(app, state.inner()),
                "reset" => runtime::confirm_then_reset(app),
                "break_now" => runtime::action_break_now(app, state.inner()),
                "breaks" => crate::breaks_window::open(app),
                "settings" => crate::settings_window::open(app),
                "alarms" => crate::alarms_window::open(app),
                "chimes" => crate::chimes_window::open(app),
                "lang-zh-hant" => runtime::set_locale(app, "zh-Hant"),
                "lang-en" => runtime::set_locale(app, "en"),
                "quit" => {
                    eprintln!("restee: quit requested");
                    app.exit(0);
                }
                _ => {} // disabled status-* info lines never fire
            }
        })
        .build(app)?;

    app.manage(TrayMenu {
        cache: Mutex::new(String::new()),
    });
    Ok(())
}

/// Reflect run state in the tray: the Start check + elapsed text, the Pause check, and
/// one info line per enabled break (soonest first). Cheap to call every tick — it only
/// rebuilds the OS menu when a rendered line actually changes.
pub fn refresh(
    app: &AppHandle,
    state: RunState,
    running_secs: u64,
    breaks: Vec<(String, u64)>,
    alarms: Vec<(String, String)>,
) {
    let Some(menu) = app.try_state::<TrayMenu>() else {
        return;
    };

    let locale = i18n::current_locale(app);
    let started = matches!(state, RunState::Running | RunState::InBreak);
    let paused = state == RunState::Paused;
    let start_text = if started {
        i18n::tr(&locale, "tray.start_running").replace("{dur}", &i18n::human_dur(&locale, running_secs))
    } else if paused {
        i18n::tr(&locale, "tray.resume").to_string()
    } else {
        i18n::tr(&locale, "tray.start").to_string()
    };
    let lines = status_lines(&locale, state, &breaks);
    let alarm_lines: Vec<String> = alarms
        .iter()
        .map(|(name, time)| format!("⏰ {name} · {time}"))
        .collect();

    // Key from the exact rendered strings (+ locale), so the menu rebuilds only when a
    // rendered line actually changes (break countdowns, today's alarms, checks, or language).
    let key = format!(
        "{locale}|{started}|{paused}|{start_text}|{}|{}",
        lines.join("\u{1f}"),
        alarm_lines.join("\u{1f}")
    );
    {
        let mut cache = menu.cache.lock().unwrap();
        if *cache == key {
            return;
        }
        *cache = key;
    }

    // Build the menu and swap it on the main thread: native menu ops aren't thread-safe,
    // and building off-thread to set on-thread is unsafe on some platforms.
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        match build_menu(&handle, &locale, started, paused, &start_text, &lines, &alarm_lines) {
            Ok(menu) => match handle.tray_by_id(TRAY_ID) {
                Some(tray) => {
                    if let Err(e) = tray.set_menu(Some(menu)) {
                        eprintln!("restee: tray set_menu failed ({e})");
                    }
                }
                None => eprintln!("restee: tray icon not found; skipped menu update"),
            },
            Err(e) => eprintln!("restee: tray build_menu failed ({e})"),
        }
    });
}

/// The disabled info lines at the top of the tray menu. Minute granularity (via
/// `human_dur`) to avoid per-second OS-menu churn. State takes precedence over the list:
/// a break in progress shows "On a break now", never pending countdowns. Paused state is
/// already conveyed by the Pause check, so the lines stay plain.
fn status_lines(locale: &str, state: RunState, breaks: &[(String, u64)]) -> Vec<String> {
    if state == RunState::InBreak {
        return vec![i18n::tr(locale, "status.on_break").to_string()];
    }
    if breaks.is_empty() {
        return vec![i18n::tr(locale, "status.no_breaks").to_string()];
    }
    breaks
        .iter()
        .map(|(name, secs)| format!("☕ {name} · {}", i18n::human_dur(locale, *secs)))
        .collect()
}

/// On a language switch: clear the cache so the next refresh can't be skipped, and update
/// the (localized) tray tooltip (`set_menu` alone doesn't touch the tooltip).
pub fn invalidate_for_locale(app: &AppHandle, locale: &str) {
    if let Some(menu) = app.try_state::<TrayMenu>() {
        *menu.cache.lock().unwrap() = String::new();
    }
    let tooltip = i18n::tr(locale, "tray.tooltip");
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(tray) = handle.tray_by_id(TRAY_ID) {
            let _ = tray.set_tooltip(Some(tooltip));
        }
    });
}
