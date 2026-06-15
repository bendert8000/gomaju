use std::sync::Mutex;

use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

use gomaju_core::RunState;

use crate::app_state::AppState;
use crate::{i18n, runtime};

const TRAY_ID: &str = "gomaju-tray";

/// Menu-item id prefix for a clickable break line: `break:<rule_id>`. Clicking one prompts
/// "take this break now?" and, on confirm, fires that rule's break (see `on_menu_event`).
/// Distinct from the exact-match control ids (and from `break_now`, which has no colon).
const BREAK_ITEM_PREFIX: &str = "break:";

/// Caches the last rendered menu key so the ticker only rebuilds the tray menu when a
/// visible value actually changes. The menu items themselves are not stored: a
/// variable-length list of break lines means we rebuild + `set_menu` rather than mutate
/// fixed handles (Tauri v2 has no `MenuItem::set_visible`).
pub struct TrayMenu {
    cache: Mutex<String>,
    /// OS id of the GUI (main) thread, captured at tray build. On Windows the ticker uses it to
    /// skip rebuilding the menu while it's open — replacing the menu (`set_menu`) dismisses the
    /// popup, so updating a countdown would close the menu the user is reading. Unused elsewhere.
    gui_thread_id: u32,
}

/// True while the given GUI thread is showing a menu (Windows). Replacing the tray menu then would
/// dismiss the open popup, so the ticker skips its rebuild until the menu closes. Tauri exposes no
/// menu open/close event, so we ask the OS directly via the `GUI_INMENUMODE` flag (set for any
/// tracked popup/menu, including the tray context menu). Always `false` off Windows.
#[cfg(windows)]
fn menu_is_open(gui_thread_id: u32) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetGUIThreadInfo, GUITHREADINFO, GUI_INMENUMODE};
    // SAFETY: a zeroed GUITHREADINFO is valid (null handles, zero rects); `cbSize` is set as the
    // API requires, and `GetGUIThreadInfo` only writes into the struct we own.
    let mut gui: GUITHREADINFO = unsafe { std::mem::zeroed() };
    gui.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
    if unsafe { GetGUIThreadInfo(gui_thread_id, &mut gui) }.is_err() {
        return false;
    }
    (gui.flags.0 & GUI_INMENUMODE.0) != 0
}

#[cfg(not(windows))]
fn menu_is_open(_gui_thread_id: u32) -> bool {
    false
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
    lines: &[(String, Option<String>)],
    alarm_lines: &[String],
) -> tauri::Result<Menu<tauri::Wry>> {
    // A break line (carrying a rule_id) becomes a clickable `break:<id>` item; a plain status
    // line ("On a break now" / "No breaks enabled") stays a no-op `status-{i}` info item.
    let status_items: Vec<MenuItem<tauri::Wry>> = lines
        .iter()
        .enumerate()
        .map(|(i, (text, rule_id))| {
            let id = match rule_id {
                Some(rid) => format!("{BREAK_ITEM_PREFIX}{rid}"),
                None => format!("status-{i}"),
            };
            MenuItem::with_id(app, id, text, true, None::<&str>)
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    // Today's upcoming alarms — disabled info lines shown in their own section above the breaks.
    let alarm_items: Vec<MenuItem<tauri::Wry>> = alarm_lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            MenuItem::with_id(app, format!("alarm-line-{i}"), line, true, None::<&str>)
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    let sep_alarms = PredefinedMenuItem::separator(app)?;

    let sep0 = PredefinedMenuItem::separator(app)?;
    // CheckMenuItems render a native check before the text to mark the active state.
    let start = CheckMenuItem::with_id(app, "start", start_text, true, started, None::<&str>)?;
    let pause = CheckMenuItem::with_id(
        app,
        "pause",
        i18n::tr(locale, "tray.pause"),
        true,
        paused,
        None::<&str>,
    )?;
    let reset = MenuItem::with_id(
        app,
        "reset",
        i18n::tr(locale, "tray.reset"),
        true,
        None::<&str>,
    )?;
    let break_now = MenuItem::with_id(
        app,
        "break_now",
        i18n::tr(locale, "tray.break_now"),
        true,
        None::<&str>,
    )?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let rules = MenuItem::with_id(
        app,
        "breaks",
        i18n::tr(locale, "tray.rules"),
        true,
        None::<&str>,
    )?;
    let alarms = MenuItem::with_id(
        app,
        "alarms",
        i18n::tr(locale, "tray.alarms"),
        true,
        None::<&str>,
    )?;
    let timers = MenuItem::with_id(
        app,
        "timers",
        i18n::tr(locale, "tray.timers"),
        true,
        None::<&str>,
    )?;
    let settings = MenuItem::with_id(
        app,
        "settings",
        i18n::tr(locale, "tray.settings"),
        true,
        None::<&str>,
    )?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(
        app,
        "quit",
        i18n::tr(locale, "tray.quit"),
        true,
        None::<&str>,
    )?;

    let mut items: Vec<&dyn IsMenuItem<tauri::Wry>> =
        Vec::with_capacity(status_items.len() + alarm_items.len() + 11);
    // Today's upcoming alarms first (when there are any), then a divider before the breaks.
    if !alarm_items.is_empty() {
        for it in &alarm_items {
            items.push(it);
        }
        items.push(&sep_alarms as &dyn IsMenuItem<tauri::Wry>);
    }
    // Enabled breaks (soonest first) — always shown.
    for it in &status_items {
        items.push(it);
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
        &timers,
        &settings,
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
    let placeholder = [(i18n::tr(&locale, "tray.placeholder").to_string(), None::<String>)];
    let menu = build_menu(
        app,
        &locale,
        true,
        false,
        i18n::tr(&locale, "tray.start"),
        &placeholder,
        &[],
    )?;

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
                "timers" => crate::timers_window::open(app),
                "quit" => {
                    crate::rlog!("gomaju: quit requested");
                    app.exit(0);
                }
                // A break line was clicked: prompt, then take that specific break on confirm.
                id if id.starts_with(BREAK_ITEM_PREFIX) => {
                    let rule_id = id[BREAK_ITEM_PREFIX.len()..].to_string();
                    runtime::confirm_then_break_one(app, rule_id);
                }
                _ => {} // status-* placeholder info lines never fire
            }
        })
        .build(app)?;

    // build_tray runs on the main thread, so this is the GUI thread that later shows the menu.
    #[cfg(windows)]
    let gui_thread_id = unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };
    #[cfg(not(windows))]
    let gui_thread_id = 0u32;
    app.manage(TrayMenu {
        cache: Mutex::new(String::new()),
        gui_thread_id,
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
    breaks: Vec<(String, String, u64)>,
    alarms: Vec<(String, String, u64)>,
) {
    let Some(menu) = app.try_state::<TrayMenu>() else {
        return;
    };

    // Don't rebuild the menu while it's open (the user is reading it): `set_menu` would dismiss the
    // popup. Skip this tick and leave the cache untouched, so the menu's content catches up on the
    // next tick once the menu closes. (Windows-only; a no-op elsewhere.)
    if menu_is_open(menu.gui_thread_id) {
        return;
    }

    let locale = i18n::current_locale(app);
    let started = matches!(state, RunState::Running | RunState::InBreak);
    let paused = state == RunState::Paused;
    let start_text = if started {
        i18n::tr(&locale, "tray.start_running")
            .replace("{dur}", &i18n::human_dur(&locale, running_secs))
    } else if paused {
        i18n::tr(&locale, "tray.resume").to_string()
    } else {
        i18n::tr(&locale, "tray.start").to_string()
    };
    let lines = status_lines(&locale, state, &breaks);
    let alarm_lines: Vec<String> = alarms
        .iter()
        .map(|(name, time, secs)| {
            format!("🟢 ⏰ {name} · {time} · {}", i18n::human_countdown(&locale, *secs))
        })
        .collect();

    // Key from the exact rendered strings (+ locale), so the menu rebuilds only when a
    // rendered line actually changes (break countdowns, today's alarms, checks, or language).
    // Fold each break line's rule_id into the key too, so a rebuild also happens if the clickable
    // target changes while the visible text somehow doesn't.
    let lines_key = lines
        .iter()
        .map(|(text, id)| format!("{text}\u{1e}{}", id.as_deref().unwrap_or("")))
        .collect::<Vec<_>>()
        .join("\u{1f}");
    let key = format!(
        "{locale}|{started}|{paused}|{start_text}|{lines_key}|{}",
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
        match build_menu(
            &handle,
            &locale,
            started,
            paused,
            &start_text,
            &lines,
            &alarm_lines,
        ) {
            Ok(menu) => match handle.tray_by_id(TRAY_ID) {
                Some(tray) => {
                    if let Err(e) = tray.set_menu(Some(menu)) {
                        crate::rlog!("gomaju: tray set_menu failed ({e})");
                    }
                }
                None => crate::rlog!("gomaju: tray icon not found; skipped menu update"),
            },
            Err(e) => crate::rlog!("gomaju: tray build_menu failed ({e})"),
        }
    });
}

/// The disabled info lines at the top of the tray menu. Minute granularity (via
/// `human_dur`) to avoid per-second OS-menu churn. State takes precedence over the list:
/// a break in progress shows "On a break now", never pending countdowns. Paused state is
/// already conveyed by the Pause check, so the lines stay plain.
fn status_lines(
    locale: &str,
    state: RunState,
    breaks: &[(String, String, u64)],
) -> Vec<(String, Option<String>)> {
    if state == RunState::InBreak {
        return vec![(i18n::tr(locale, "status.on_break").to_string(), None)];
    }
    if breaks.is_empty() {
        return vec![(i18n::tr(locale, "status.no_breaks").to_string(), None)];
    }
    breaks
        .iter()
        .map(|(id, name, secs)| {
            (
                format!("🟢 ☕ {name} · {}", i18n::human_dur(locale, *secs)),
                Some(id.clone()),
            )
        })
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
