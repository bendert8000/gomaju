//! Backend string catalog for the two supported UI locales (`zh-Hant` default, `en`).
//!
//! Hand-rolled to keep deps out: [`tr`] returns the `&'static str` for a key in the given
//! locale (anything that isn't `"en"` falls back to Traditional Chinese, matching the
//! config default). Strings that interpolate runtime values (`{name}`, `{dur}`) are stored
//! as templates and the call site substitutes via [`str::replace`]. Window/notification
//! brand text (`Restee`) stays untranslated and is not in the catalog.

use tauri::{AppHandle, Manager};

use crate::app_state::AppState;

/// The current UI locale from the live config (the single source of truth).
pub fn current_locale(app: &AppHandle) -> String {
    app.state::<AppState>().config.lock().unwrap().locale.clone()
}

/// English iff the locale is exactly `"en"`; otherwise Traditional Chinese (the default).
fn pick(locale: &str, en: &'static str, zh: &'static str) -> &'static str {
    if locale == "en" {
        en
    } else {
        zh
    }
}

/// Translate a catalog `key` for `locale`. Unknown keys return the key itself (a visible
/// signal in dev that a string wasn't added to the catalog).
pub fn tr(locale: &str, key: &str) -> &'static str {
    match key {
        // Tray menu
        "tray.start" => pick(locale, "Start", "開始"),
        "tray.resume" => pick(locale, "Resume", "繼續"),
        "tray.pause" => pick(locale, "Pause", "暫停"),
        "tray.reset" => pick(locale, "Reset break timer", "重設休息計時器"),
        "tray.break_now" => pick(locale, "Break now", "立即休息"),
        "tray.rules" => pick(locale, "Breaks…", "休息…"),
        "tray.alarms" => pick(locale, "Alarms…", "鬧鐘…"),
        "tray.chimes" => pick(locale, "Chimes…", "鈴聲…"),
        "tray.settings" => pick(locale, "Settings…", "設定…"),
        "tray.language" => pick(locale, "Language", "語言"),
        "tray.quit" => pick(locale, "Quit Restee", "結束 Restee"),
        "tray.tooltip" => pick(locale, "Restee — break reminder", "Restee — 休息提醒"),
        // Tray status lines
        "tray.start_running" => pick(locale, "Running", "執行中"),
        "tray.placeholder" => pick(locale, "Next break in …", "下次休息還有…"),
        "status.on_break" => pick(locale, "On a break now", "休息中"),
        "status.no_breaks" => pick(locale, "No breaks enabled", "未啟用任何休息"),

        // Reset dialogs
        "dialog.reset_timer_title" => pick(locale, "Reset break timer", "重設休息計時器"),
        "dialog.reset_timer_msg" => pick(
            locale,
            "Restart all break countdowns? Every break timer is cleared and starts again from its full interval.",
            "要重新開始所有休息的倒數嗎？每個休息計時都會清除，並從完整間隔重新計算。",
        ),
        "dialog.reset_break_title" => pick(locale, "Reset break", "重設休息"),
        "dialog.reset_break_msg" => pick(
            locale,
            "Restart the countdown for “{name}”? It starts again from its full interval.",
            "要重新開始「{name}」的倒數嗎？將從完整間隔重新計算。",
        ),
        "dialog.reset" => pick(locale, "Reset", "重設"),
        "dialog.cancel" => pick(locale, "Cancel", "取消"),

        // Notifications ({name} = rule/alarm name)
        "notif.soft_break" => pick(locale, "{name} — time for a quick break", "{name} — 該休息一下了"),
        "notif.startup" => pick(locale, "Restee is running now", "Restee 已啟動"),

        // Native window titles
        "title.settings" => pick(locale, "Restee — Settings", "Restee — 設定"),
        "title.rules" => pick(locale, "Restee — Break rules", "Restee — 休息規則"),
        "title.alarms" => pick(locale, "Restee — Alarms", "Restee — 鬧鐘"),
        "title.chimes" => pick(locale, "Restee — Chimes", "Restee — 鈴聲"),

        _ => {
            // All keys should be in the catalog; surface a miss in dev rather than panic.
            eprintln!("restee: missing i18n key '{key}'");
            "?"
        }
    }
}

/// Coarse, minute-granularity duration for the tray menu, localized. en: `<1m` / `19m` /
/// `1h 5m`; zh-Hant: `<1 分` / `19 分` / `1 時 5 分`. Kept compact so the menu stays tight.
pub fn human_dur(locale: &str, secs: u64) -> String {
    let m = secs / 60;
    let en = locale == "en";
    if m == 0 {
        if en {
            "<1m".to_string()
        } else {
            "<1 分".to_string()
        }
    } else if m < 60 {
        if en {
            format!("{m}m")
        } else {
            format!("{m} 分")
        }
    } else if en {
        format!("{}h {}m", m / 60, m % 60)
    } else {
        format!("{} 時 {} 分", m / 60, m % 60)
    }
}
