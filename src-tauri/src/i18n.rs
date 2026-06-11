//! Backend string catalog for the two supported UI locales (`zh-Hant` default, `en`).
//!
//! Hand-rolled to keep deps out: [`tr`] returns the `&'static str` for a key in the given
//! locale (anything that isn't `"en"` falls back to Traditional Chinese, matching the
//! config default). Strings that interpolate runtime values (`{name}`, `{dur}`) are stored
//! as templates and the call site substitutes via [`str::replace`]. Window/notification
//! brand text (`Gomaju`) stays untranslated and is not in the catalog.

use tauri::{AppHandle, Manager};

use crate::app_state::AppState;

/// The current UI locale from the live config (the single source of truth).
pub fn current_locale(app: &AppHandle) -> String {
    app.state::<AppState>()
        .config
        .lock()
        .unwrap()
        .locale
        .clone()
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
        "tray.settings" => pick(locale, "Settings…", "設定…"),
        "tray.quit" => pick(locale, "Quit Gomaju", "結束 Gomaju"),
        "tray.tooltip" => pick(locale, "Gomaju — break reminder", "Gomaju — 休息提醒"),
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

        // "Take this break now" dialog (tray → click a break line). {name} = rule name.
        "dialog.break_now_title" => pick(locale, "Take this break?", "要現在休息嗎？"),
        "dialog.break_now_msg" => pick(
            locale,
            "Start the “{name}” break right now?",
            "要立即開始「{name}」休息嗎？",
        ),
        "dialog.break_now_ok" => pick(locale, "Take break", "立即休息"),

        // Cold-start "resume previous break progress?" dialog ({age} = how long ago it was saved)
        "dialog.resume_progress_title" => pick(locale, "Resume break progress?", "恢復休息進度？"),
        "dialog.resume_progress_msg" => pick(
            locale,
            "Resume your previous break progress? It was saved {age} ago. Choose Start fresh to begin every countdown from zero.",
            "要恢復先前的休息進度嗎？上次儲存於 {age} 前。選擇「重新開始」則所有倒數從零計算。",
        ),
        "dialog.resume" => pick(locale, "Resume", "恢復"),
        "dialog.start_fresh" => pick(locale, "Start fresh", "重新開始"),
        "dialog.import_chime_title" => {
            pick(locale, "Gomaju — Import chime sound", "Gomaju — 匯入鈴聲音檔")
        }

        // Notifications ({name} = rule/alarm name)
        "notif.soft_break" => pick(locale, "{name} — time for a quick break", "{name} — 該休息一下了"),
        "notif.startup" => pick(locale, "Running in the system tray", "正在系統匣中執行"),
        "notif.break_title" => pick(locale, "Gomaju · Break reminder", "Gomaju · 休息提醒"),
        "notif.alarm_title" => pick(locale, "Gomaju · Alarm", "Gomaju · 鬧鐘"),

        // Native window titles
        "title.settings" => pick(locale, "Gomaju — Settings", "Gomaju — 設定"),
        "title.rules" => pick(locale, "Gomaju — Break rules", "Gomaju — 休息規則"),
        "title.alarms" => pick(locale, "Gomaju — Alarms", "Gomaju — 鬧鐘"),
        "title.chimes" => pick(locale, "Gomaju — Chimes", "Gomaju — 鈴聲"),

        _ => {
            // All keys should be in the catalog; surface a miss in dev rather than panic.
            crate::rlog!("gomaju: missing i18n key '{key}'");
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

/// A localized "time until" countdown for the tray's upcoming-alarm lines, e.g. `in 2h 15m`
/// (en) / `2 時 15 分後` (zh-Hant). Reuses `human_dur` for the body so granularity and
/// localization stay in one place; minute-granular by design (no per-second menu churn).
pub fn human_countdown(locale: &str, secs: u64) -> String {
    let dur = human_dur(locale, secs);
    if locale == "en" {
        format!("in {dur}")
    } else {
        format!("{dur}後")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_countdown_en_prefixes_in() {
        assert_eq!(human_countdown("en", 2 * 3600 + 15 * 60), "in 2h 15m");
        assert_eq!(human_countdown("en", 19 * 60), "in 19m");
        assert_eq!(human_countdown("en", 30), "in <1m");
    }

    #[test]
    fn human_countdown_zh_suffixes_after() {
        assert_eq!(human_countdown("zh-Hant", 2 * 3600 + 15 * 60), "2 時 15 分後");
        assert_eq!(human_countdown("zh-Hant", 30), "<1 分後");
    }
}
