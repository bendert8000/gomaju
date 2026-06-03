import { readInjected } from "./util";

// Lightweight hand-rolled i18n. The locale is injected per-window at creation by the Rust
// window builder (`window.__RESTEE_LOCALE__`), read synchronously here, and fixed for the
// window's lifetime — switching language in the tray takes effect when a window is reopened.

export type Locale = "zh-Hant" | "en";

export const LOCALE: Locale =
  readInjected<string>("__RESTEE_LOCALE__", "zh-Hant") === "en" ? "en" : "zh-Hant";

type Entry = { "zh-Hant": string; en: string };

const MESSAGES: Record<string, Entry> = {
  // --- Document / window titles ---
  "title.settings": { en: "Restee — Settings", "zh-Hant": "Restee — 設定" },
  "title.rules": { en: "Restee — Break rules", "zh-Hant": "Restee — 休息規則" },
  "title.alarms": { en: "Restee — Alarms", "zh-Hant": "Restee — 鬧鐘" },

  // --- Common ---
  "common.close": { en: "Close", "zh-Hant": "關閉" },
  "common.save": { en: "Save", "zh-Hant": "儲存" },
  "common.saved": { en: "Saved ✓", "zh-Hant": "已儲存 ✓" },
  "common.remove": { en: "Remove", "zh-Hant": "移除" },

  // --- Unsaved-changes confirm modal (Settings + Alarms) ---
  "confirm.unsaved_title": { en: "Save changes before closing?", "zh-Hant": "關閉前要儲存變更嗎？" },
  "confirm.unsaved_msg": {
    en: "Your changes will be lost if you don't save them.",
    "zh-Hant": "若不儲存，您的變更將會遺失。",
  },
  "confirm.dont_save": { en: "Don't Save", "zh-Hant": "不儲存" },
  "confirm.cancel": { en: "Cancel", "zh-Hant": "取消" },

  // --- Settings window ---
  "settings.idle_title": { en: "Idle detection backend", "zh-Hant": "閒置偵測後端" },
  "settings.idle_badge": { en: "idle: {status}", "zh-Hant": "閒置偵測：{status}" },
  "settings.rules_heading": { en: "Break rules", "zh-Hant": "休息規則" },
  "settings.col_name": { en: "Name", "zh-Hant": "名稱" },
  "settings.col_every": { en: "Every (min)", "zh-Hant": "間隔（分鐘）" },
  "settings.col_break": { en: "Break (sec)", "zh-Hant": "休息（秒）" },
  "settings.col_type": { en: "Type", "zh-Hant": "類型" },
  "settings.col_on": { en: "On", "zh-Hant": "啟用" },
  "settings.col_repeat": { en: "Repeat", "zh-Hant": "重複" },
  "settings.add_rule": { en: "+ Add rule", "zh-Hant": "＋ 新增規則" },
  "settings.behavior_heading": { en: "Break behavior", "zh-Hant": "休息行為" },
  "settings.idle_label": { en: "When you go idle", "zh-Hant": "當你閒置時" },
  "settings.idle_pause": { en: "Pause counting", "zh-Hant": "暫停計時" },
  "settings.idle_credit": { en: "Count it as a break", "zh-Hant": "視為已休息" },
  "settings.escape_label": { en: "Strict-break escape", "zh-Hant": "強制休息的退出方式" },
  "settings.escape_friction": { en: "Hold to skip", "zh-Hant": "按住以略過" },
  "settings.escape_easy": { en: "Easy skip button", "zh-Hant": "簡易略過按鈕" },
  "settings.escape_none": { en: "No easy escape", "zh-Hant": "無法輕易退出" },
  "settings.break_display_label": { en: "Break screen display", "zh-Hant": "休息畫面顯示" },
  "settings.break_display_countdown": { en: "Countdown text", "zh-Hant": "倒數文字" },
  "settings.break_display_bar": { en: "Progress bar", "zh-Hant": "進度條" },
  "settings.warn_label": {
    en: "Warn before break (seconds, 0 = off)",
    "zh-Hant": "休息前提醒（秒，0 ＝ 關閉）",
  },
  "settings.idle_threshold_label": { en: "Idle threshold (seconds)", "zh-Hant": "閒置門檻（秒）" },
  "settings.sound_label": {
    en: "Play a chime when a break starts or ends",
    "zh-Hant": "休息開始或結束時播放提示音",
  },
  "settings.notif_label": {
    en: "Show a notification on soft breaks",
    "zh-Hant": "柔性休息時顯示通知",
  },
  "settings.autostart_label": { en: "Launch at login", "zh-Hant": "登入時啟動" },
  "settings.hotkeys_heading": { en: "Break global hotkeys", "zh-Hant": "休息全域快速鍵" },
  "settings.hotkeys_optional": { en: "(optional)", "zh-Hant": "（選用）" },
  "settings.hotkeys_eg": { en: "e.g. ", "zh-Hant": "例如 " },
  "settings.hotkeys_unbind": {
    en: ". Leave blank to unbind.",
    "zh-Hant": "。留空即可取消綁定。",
  },
  "settings.hk_toggle": { en: "Start / pause", "zh-Hant": "開始／暫停" },
  "settings.hk_break": { en: "Break now", "zh-Hant": "立即休息" },
  "settings.hk_skip": { en: "Skip break", "zh-Hant": "略過休息" },
  "settings.hk_placeholder": { en: "unset", "zh-Hant": "未設定" },
  "settings.save_hotkey_fail": {
    en: "Saved, but some hotkeys failed: {errors}",
    "zh-Hant": "已儲存，但部分快速鍵設定失敗：{errors}",
  },
  "settings.save_fail": { en: "Save failed: {err}", "zh-Hant": "儲存失敗：{err}" },

  // --- Today's breaks (rules) window + cards ---
  "rules.heading": { en: "Today's breaks", "zh-Hant": "今日休息" },
  "rules.edit_in_settings": { en: "Edit in Settings…", "zh-Hant": "在設定中編輯…" },
  "rules.empty": {
    en: "No break rules yet — add some in Settings.",
    "zh-Hant": "尚無休息規則 — 請在設定中新增。",
  },
  "rules.couldnt_update": { en: "Couldn't update: {err}", "zh-Hant": "無法更新：{err}" },
  "card.on": { en: "ON", "zh-Hant": "開" },
  "card.off": { en: "OFF", "zh-Hant": "關" },
  "card.reset": { en: "Reset", "zh-Hant": "重設" },
  "card.repeat": { en: "Repeat", "zh-Hant": "重複" },
  "card.repeat_title": {
    en: "Repeats on its schedule — turn off to fire once, then auto-disable",
    "zh-Hant": "依排程重複 — 關閉則只執行一次後自動停用",
  },
  "card.soft": { en: "Soft", "zh-Hant": "柔性" },
  "card.strict": { en: "Strict", "zh-Hant": "強制" },
  "card.every": { en: "every {n} min", "zh-Hant": "每 {n} 分鐘" },
  "card.break_min": { en: "{n} min break", "zh-Hant": "休息 {n} 分鐘" },
  "card.break_sec": { en: "{n}s break", "zh-Hant": "休息 {n} 秒" },
  "card.next_in": { en: "next in {mmss}", "zh-Hant": "{mmss} 後" },

  // --- Alarms window ---
  "alarms.heading": { en: "Restee — Alarms", "zh-Hant": "Restee — 鬧鐘" },
  "alarms.section_heading": { en: "Alarms", "zh-Hant": "鬧鐘" },
  "alarms.desc": {
    en: "A notification and sound at a set clock time. Alarms fire even while the break timer is paused or a break is on screen. An alarm only fires if Restee is running at that minute — there's no catch-up for times missed while it was closed.",
    "zh-Hant":
      "在設定的時刻發出通知與聲音。即使休息計時已暫停或正在休息，鬧鐘仍會響起。鬧鐘僅在該分鐘 Restee 正在執行時才會響 — 關閉期間錯過的時間不會補響。",
  },
  "alarms.add": { en: "+ Add alarm", "zh-Hant": "＋ 新增鬧鐘" },
  "alarms.name_ph": { en: "Alarm name", "zh-Hant": "鬧鐘名稱" },
  "alarms.on": { en: "On", "zh-Hant": "啟用" },
  "alarms.repeat_once": { en: "Once", "zh-Hant": "一次" },
  "alarms.repeat_daily": { en: "Daily", "zh-Hant": "每日" },
  "alarms.repeat_weekly": { en: "Weekly", "zh-Hant": "每週" },
  "alarms.repeat_monthly": { en: "Monthly", "zh-Hant": "每月" },
  "alarms.repeat_yearly": { en: "Yearly", "zh-Hant": "每年" },
  "alarms.day": { en: "Day", "zh-Hant": "日" },
  "alarms.in": { en: "in {dur}", "zh-Hant": "{dur} 後" },
  "alarms.default_name": { en: "Alarm", "zh-Hant": "鬧鐘" },
  "alarms.new_name": { en: "New alarm", "zh-Hant": "新鬧鐘" },

  // Weekday abbreviations (Mon … Sun)
  "weekday.0": { en: "Mon", "zh-Hant": "一" },
  "weekday.1": { en: "Tue", "zh-Hant": "二" },
  "weekday.2": { en: "Wed", "zh-Hant": "三" },
  "weekday.3": { en: "Thu", "zh-Hant": "四" },
  "weekday.4": { en: "Fri", "zh-Hant": "五" },
  "weekday.5": { en: "Sat", "zh-Hant": "六" },
  "weekday.6": { en: "Sun", "zh-Hant": "日" },

  // --- Rule editor (Settings rules grid) ---
  "editor.new_break": { en: "New break", "zh-Hant": "新休息" },
  "editor.fallback_break": { en: "Break", "zh-Hant": "休息" },
  "editor.note_placeholder": {
    en: "Optional note shown on the break screen",
    "zh-Hant": "選填：顯示在休息畫面的備註",
  },
  "editor.repeat_title": {
    en: "Repeat after each break — uncheck for a one-time break",
    "zh-Hant": "每次休息後重複 — 取消勾選則只休息一次",
  },

  // --- Break overlay ---
  "overlay.default_name": { en: "Time for a break", "zh-Hant": "該休息了" },
  "overlay.skip": { en: "Skip", "zh-Hant": "略過" },
  "overlay.emergency": {
    en: "Hold Esc to exit in an emergency",
    "zh-Hant": "緊急情況請按住 Esc 退出",
  },
  "overlay.soft_hint": {
    en: "Look ~20 feet away and relax your eyes.",
    "zh-Hant": "看向約 6 公尺外，放鬆雙眼。",
  },
  "overlay.hold_to_skip": { en: "Hold to skip", "zh-Hant": "按住以略過" },
  "overlay.keep_holding": { en: "Keep holding…", "zh-Hant": "繼續按住…" },
  "overlay.strict_hint": {
    en: "Step away from the screen for a bit.",
    "zh-Hant": "暫時離開螢幕一下。",
  },
  "overlay.keep_holding_esc": { en: "Keep holding Esc…", "zh-Hant": "繼續按住 Esc…" },

  // --- Pre-break toast ---
  "toast.default_title": { en: "Break soon", "zh-Hant": "即將休息" },
  "toast.title": { en: "{name} soon", "zh-Hant": "{name} 即將開始" },
  "toast.starting_in": { en: "starting in {mmss}", "zh-Hant": "{mmss} 後開始" },
  "toast.starting_soon": { en: "starting soon…", "zh-Hant": "即將開始…" },
};

/** Translate `key` for the current window locale, substituting `{param}` placeholders. */
export function t(key: string, params?: Record<string, string | number>): string {
  const entry = MESSAGES[key];
  if (!entry) {
    console.warn(`restee: missing i18n key '${key}'`);
    return key;
  }
  let s = entry[LOCALE];
  if (params) {
    for (const [k, v] of Object.entries(params)) s = s.split(`{${k}}`).join(String(v));
  }
  return s;
}

/**
 * Apply translations to static markup under `root`: `data-i18n` → textContent,
 * `data-i18n-ph` → placeholder, `data-i18n-title` → title. Also sets `<html lang>`.
 * Dynamically-built nodes should call `t()` at creation instead.
 */
export function applyI18n(root: ParentNode = document.body): void {
  document.documentElement.lang = LOCALE;
  root.querySelectorAll<HTMLElement>("[data-i18n]").forEach((el) => {
    el.textContent = t(el.dataset.i18n as string);
  });
  root.querySelectorAll<HTMLElement>("[data-i18n-ph]").forEach((el) => {
    (el as HTMLInputElement).placeholder = t(el.dataset.i18nPh as string);
  });
  root.querySelectorAll<HTMLElement>("[data-i18n-title]").forEach((el) => {
    el.title = t(el.dataset.i18nTitle as string);
  });
}
