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
  "title.chimes": { en: "Restee — Chimes", "zh-Hant": "Restee — 鈴聲" },

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

  // --- Quotes "changed on disk" conflict modal (Settings Quotes card) ---
  "confirm.quotes_conflict_title": {
    en: "Quotes changed outside Restee",
    "zh-Hant": "語錄已在 Restee 外被變更",
  },
  "confirm.quotes_conflict_msg": {
    en: "A quotes file was edited outside Restee since you opened this window. Overwrite it with your current list, or keep the version on disk (your quote edits here will be discarded)?",
    "zh-Hant":
      "自您開啟此視窗後，語錄檔已在 Restee 外被編輯。要以您目前的清單覆寫，還是保留磁碟上的版本（將捨棄您在此處的語錄變更）？",
  },
  "confirm.quotes_overwrite": { en: "Overwrite", "zh-Hant": "覆寫" },
  "confirm.quotes_keep_disk": { en: "Keep on-disk version", "zh-Hant": "保留磁碟版本" },

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
  "settings.escape_friction": { en: "Hold to cancel break", "zh-Hant": "長按取消休息" },
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
  "settings.pause_reminder_enabled": {
    en: "When paused, ask whether to resume counting",
    "zh-Hant": "暫停時，詢問是否恢復計時",
  },
  "settings.pause_reminder_interval": {
    en: "Ask every (minutes)",
    "zh-Hant": "每隔多久詢問（分鐘）",
  },
  "settings.sound_label": {
    en: "Play a chime when a break starts or ends",
    "zh-Hant": "休息開始或結束時播放提示音",
  },
  "settings.show_quotes_label": {
    en: "Show a quote on the break screen",
    "zh-Hant": "在休息畫面顯示語錄",
  },
  "settings.quotes_heading": { en: "Quotes", "zh-Hant": "語錄" },
  "settings.add_quote": { en: "+ Add quote", "zh-Hant": "＋ 新增語錄" },
  "settings.quotes_hint": {
    en: "Quotes are shown in the app's language — edit each set with the toggle above. One quote per line; blank lines and lines starting with # aren't kept.",
    "zh-Hant": "語錄會以應用程式的語言顯示 — 用上方切換來編輯各語言。每行一句；空白行與以 # 開頭的行不會保留。",
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
  "alarms.repeat_biweekly": { en: "Bi-weekly", "zh-Hant": "每兩週" },
  "alarms.repeat_monthly": { en: "Monthly", "zh-Hant": "每月" },
  "alarms.repeat_yearly": { en: "Yearly", "zh-Hant": "每年" },
  "alarms.day": { en: "Day", "zh-Hant": "日" },
  "alarms.start": { en: "Starts", "zh-Hant": "開始" },
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

  // --- Chimes window ---
  "chimes.heading": { en: "Restee — Chimes", "zh-Hant": "Restee — 鈴聲" },
  "chimes.section_heading": { en: "Chimes", "zh-Hant": "鈴聲" },
  "chimes.desc": {
    en: "Create custom sounds — a sequence of tones, or an imported audio file. A break rule or alarm can then pick a saved chime; leave a rule/alarm's chime unset to use the default.",
    "zh-Hant":
      "建立自訂聲音 — 一連串音調，或匯入的音訊檔。休息規則或鬧鐘即可選用已儲存的鈴聲；未設定則使用預設。",
  },
  "chimes.add": { en: "+ Add chime", "zh-Hant": "＋ 新增鈴聲" },
  "chimes.name_ph": { en: "Chime name", "zh-Hant": "鈴聲名稱" },
  "chimes.kind_tones": { en: "Tones", "zh-Hant": "音調" },
  "chimes.kind_file": { en: "File", "zh-Hant": "檔案" },
  "chimes.preview": { en: "Preview", "zh-Hant": "試聽" },
  "chimes.pause": { en: "Pause", "zh-Hant": "暫停" },
  "chimes.key": { en: "Key", "zh-Hant": "調" },
  "chimes.octave": { en: "Octave", "zh-Hant": "八度" },
  "chimes.length": { en: "Length", "zh-Hant": "長度" },
  "chimes.volume": { en: "Volume", "zh-Hant": "音量" },
  "chimes.melody": { en: "Melody", "zh-Hant": "旋律" },
  "chimes.rest": { en: "Rest", "zh-Hant": "休止" },
  "chimes.clear": { en: "Clear", "zh-Hant": "清除" },
  "chimes.import": { en: "Import file…", "zh-Hant": "匯入檔案…" },
  "chimes.open_folder": { en: "Open folder", "zh-Hant": "開啟資料夾" },
  "chimes.no_file": { en: "No file selected", "zh-Hant": "尚未選擇檔案" },
  "chimes.new_name": { en: "New chime", "zh-Hant": "新鈴聲" },
  "chimes.default_name": { en: "Chime", "zh-Hant": "鈴聲" },

  // Chime picker (shared by the rules editor + alarms)
  "chime.label": { en: "Chime", "zh-Hant": "鈴聲" },
  "chime.start_label": { en: "Start chime", "zh-Hant": "開始鈴聲" },
  "chime.end_label": { en: "End chime", "zh-Hant": "結束鈴聲" },
  "chime.default": { en: "Default", "zh-Hant": "預設" },

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
  "overlay.hold_to_skip": { en: "Hold to cancel break", "zh-Hant": "長按取消休息" },
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
  "toast.delay": { en: "Delay 1 min", "zh-Hant": "延後 1 分鐘" },

  // --- Pause reminder toast ---
  "pause_toast.title": { en: "Still paused?", "zh-Hant": "仍在暫停？" },
  "pause_toast.sub": {
    en: "Resume break counting now?",
    "zh-Hant": "要現在恢復休息計時嗎？",
  },
  "pause_toast.resume": { en: "Resume counting", "zh-Hant": "恢復計時" },
  "pause_toast.stay_paused": { en: "Stay paused", "zh-Hant": "繼續暫停" },
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
