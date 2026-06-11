import { invoke } from "@tauri-apps/api/core";
import { applyI18n, LOCALE, t } from "./i18n";
import { fillChimeSelect, type ChimeOption } from "./rule-editor";
import { installUnsavedGuard, type UnsavedGuard } from "./unsaved-guard";
import {
  installPreviewEndedListener,
  resetActivePreview,
  wirePreviewButton,
} from "./chime-preview";

// Assigned in init() once the alarms are first rendered; referenced only afterwards.
let guard!: UnsavedGuard;
// Saved chimes (for each alarm's chime picker), loaded once in init().
let chimes: ChimeOption[] = [];

// --- Types mirroring gomaju_core::alarm DTOs ---

type Repeat = "once" | "daily" | "weekly" | "biweekly" | "monthly" | "yearly";

interface AlarmDto {
  id: string;
  name: string;
  time: string; // "HH:MM" (24h)
  repeat: Repeat;
  weekdays: number[]; // 0=Mon … 6=Sun (Weekly / Biweekly)
  day_of_month: number; // 1..31 (Monthly / Yearly)
  month: number; // 1..12 (Yearly)
  date: string | null; // "YYYY-MM-DD" (Once: fire date; Biweekly: start week)
  enabled: boolean;
  chime_id: string; // id of a saved chime to play (empty = default alarm tone)
  chime_volume_pct: number; // volume for this alarm chime/default tone, 0..=100
}

const $ = <T extends HTMLElement>(id: string): T => document.getElementById(id) as T;
const q = <T extends HTMLElement>(root: HTMLElement, selector: string): T =>
  root.querySelector(selector) as T;

const WEEKDAYS = Array.from({ length: 7 }, (_, i) => t(`weekday.${i}`));

// The 7 day-of-week checkboxes, shared by the Weekly and Bi-weekly detail rows. Constant
// (labels are fixed per locale), built once. collectAlarms() scopes by detail container.
const WEEKDAY_CHECKBOXES = WEEKDAYS.map(
  (w, i) => `<label class="wd-label"><input class="wd" type="checkbox" value="${i}" />${w}</label>`,
).join("");

function clampInt(value: string, lo: number, hi: number, fallback: number): number {
  const n = Math.round(Number(value));
  return Number.isFinite(n) ? Math.min(hi, Math.max(lo, n)) : fallback;
}

// Today as a LOCAL "YYYY-MM-DD" (NOT toISOString(), which is UTC and can be a day off vs
// the host's Local::now() scheduling). Used to prefill a new bi-weekly alarm's start date.
function todayLocal(): string {
  const d = new Date();
  const p = (n: number): string => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

// --- Alarm rows ---

function alarmRow(a: AlarmDto): HTMLElement {
  const row = document.createElement("div");
  row.className = "alarm-item";
  row.dataset.id = a.id;
  row.dataset.on = String(a.enabled); // drives the dimmed-when-disabled styling
  // Static scaffolding only (constant labels/indices). User-supplied values are never
  // interpolated here — they are set via DOM setters below, so there's no XSS surface.
  row.innerHTML = `
    <div class="alarm-line">
      <input class="alarm-name" type="text" placeholder="${t("alarms.name_ph")}" />
      <input class="alarm-time" type="time" />
      <label class="alarm-on"><input class="alarm-enabled" type="checkbox" /> ${t("alarms.on")}</label>
      <button class="alarm-remove btn-ghost" type="button" title="${t("common.remove")}">✕</button>
    </div>
    <div class="alarm-line alarm-sched">
      <select class="alarm-repeat">
        <option value="once">${t("alarms.repeat_once")}</option>
        <option value="daily">${t("alarms.repeat_daily")}</option>
        <option value="weekly">${t("alarms.repeat_weekly")}</option>
        <option value="biweekly">${t("alarms.repeat_biweekly")}</option>
        <option value="monthly">${t("alarms.repeat_monthly")}</option>
        <option value="yearly">${t("alarms.repeat_yearly")}</option>
      </select>
      <span class="alarm-detail alarm-detail-once"><input class="alarm-date" type="date" /></span>
      <span class="alarm-detail alarm-detail-weekly">${WEEKDAY_CHECKBOXES}</span>
      <span class="alarm-detail alarm-detail-biweekly">${t("alarms.start")} <input class="alarm-biweekly-start" type="date" />${WEEKDAY_CHECKBOXES}</span>
      <span class="alarm-detail alarm-detail-monthly">${t("alarms.day")} <input class="alarm-dom" type="number" min="1" max="31" /></span>
      <span class="alarm-detail alarm-detail-yearly">
        <select class="alarm-month">${Array.from(
          { length: 12 },
          (_, i) => `<option value="${i + 1}">${i + 1}</option>`,
        ).join("")}</select>
        ${t("alarms.day")} <input class="alarm-doy" type="number" min="1" max="31" />
      </span>
    </div>
    <div class="alarm-line alarm-chime-row">
      <label>${t("chime.label")} <select class="alarm-chime"></select><span>${t("chimes.volume")}</span><input class="alarm-chime-volume chime-volume-picker" type="number" min="0" max="100" /><button class="alarm-chime-preview btn-ghost chime-preview-btn" type="button"></button></label>
    </div>
    <div class="alarm-next"></div>
  `;

  q<HTMLInputElement>(row, ".alarm-name").value = a.name;
  const chimeSel = q<HTMLSelectElement>(row, ".alarm-chime");
  fillChimeSelect(chimeSel, chimes, a.chime_id ?? "");
  q<HTMLInputElement>(row, ".alarm-chime-volume").value = String(
    clampInt(String(a.chime_volume_pct ?? 20), 0, 100, 20),
  );
  // ▶/⏸ preview after the picker; "Default" (empty) auditions the built-in alarm tone.
  wirePreviewButton(
    q<HTMLButtonElement>(row, ".alarm-chime-preview"),
    () => chimeSel.value,
    () => clampInt(q<HTMLInputElement>(row, ".alarm-chime-volume").value, 0, 100, 20),
    "alarm",
  );
  q<HTMLInputElement>(row, ".alarm-time").value = a.time || "08:00";
  q<HTMLInputElement>(row, ".alarm-enabled").checked = a.enabled;
  q<HTMLSelectElement>(row, ".alarm-repeat").value = a.repeat;
  q<HTMLInputElement>(row, ".alarm-date").value = a.date ?? "";
  q<HTMLInputElement>(row, ".alarm-biweekly-start").value = a.date ?? "";
  // Sets both the weekly and biweekly `.wd` sets identically; only the active kind is read back.
  for (const cb of row.querySelectorAll<HTMLInputElement>(".wd")) {
    cb.checked = a.weekdays.includes(Number(cb.value));
  }
  q<HTMLInputElement>(row, ".alarm-dom").value = String(a.day_of_month || 1);
  q<HTMLSelectElement>(row, ".alarm-month").value = String(a.month || 1);
  q<HTMLInputElement>(row, ".alarm-doy").value = String(a.day_of_month || 1);

  const repeatSel = q<HTMLSelectElement>(row, ".alarm-repeat");
  const updateDetail = (): void => {
    const r = repeatSel.value;
    for (const el of row.querySelectorAll<HTMLElement>(".alarm-detail")) {
      el.classList.toggle("show", el.classList.contains(`alarm-detail-${r}`));
    }
    // Prefill a fresh bi-weekly alarm's start date with today (local) so it isn't
    // immediately disabled by the sanitizer for lacking an anchor.
    if (r === "biweekly") {
      const start = q<HTMLInputElement>(row, ".alarm-biweekly-start");
      if (!start.value) start.value = todayLocal();
    }
  };
  repeatSel.addEventListener("change", updateDetail);
  updateDetail();

  // Keep the dimmed-when-disabled styling in sync as the user toggles On.
  const enabledBox = q<HTMLInputElement>(row, ".alarm-enabled");
  enabledBox.addEventListener("change", () => {
    row.dataset.on = String(enabledBox.checked);
  });

  q(row, ".alarm-remove").addEventListener("click", () => row.remove());
  return row;
}

function renderAlarms(alarms: AlarmDto[]): void {
  resetActivePreview(); // rows (and their preview buttons) are about to be rebuilt
  const container = $("alarms");
  container.innerHTML = "";
  for (const a of alarms) container.appendChild(alarmRow(a));
}

function collectAlarms(): AlarmDto[] {
  const rows = Array.from(document.querySelectorAll<HTMLElement>(".alarm-item"));
  return rows.map((row) => {
    const repeat = q<HTMLSelectElement>(row, ".alarm-repeat").value as Repeat;
    // Collect only the fields relevant to this repeat kind; the rest stay at their
    // "unused" defaults (the backend ignores them per kind anyway).
    let weekdays: number[] = [];
    let day_of_month = 0;
    let month = 0;
    let date: string | null = null;
    if (repeat === "weekly") {
      // Scope to the weekly detail: the biweekly detail also has `.wd` boxes.
      weekdays = Array.from(row.querySelectorAll<HTMLInputElement>(".alarm-detail-weekly .wd"))
        .filter((c) => c.checked)
        .map((c) => Number(c.value));
    } else if (repeat === "biweekly") {
      weekdays = Array.from(row.querySelectorAll<HTMLInputElement>(".alarm-detail-biweekly .wd"))
        .filter((c) => c.checked)
        .map((c) => Number(c.value));
      date = q<HTMLInputElement>(row, ".alarm-biweekly-start").value || null;
    } else if (repeat === "monthly") {
      day_of_month = clampInt(q<HTMLInputElement>(row, ".alarm-dom").value, 1, 31, 1);
    } else if (repeat === "yearly") {
      day_of_month = clampInt(q<HTMLInputElement>(row, ".alarm-doy").value, 1, 31, 1);
      month = clampInt(q<HTMLSelectElement>(row, ".alarm-month").value, 1, 12, 1);
    } else if (repeat === "once") {
      date = q<HTMLInputElement>(row, ".alarm-date").value || null;
    }
    return {
      id: row.dataset.id || crypto.randomUUID(),
      name: q<HTMLInputElement>(row, ".alarm-name").value.trim() || t("alarms.default_name"),
      time: q<HTMLInputElement>(row, ".alarm-time").value || "08:00",
      repeat,
      weekdays,
      day_of_month,
      month,
      date,
      enabled: q<HTMLInputElement>(row, ".alarm-enabled").checked,
      chime_id: q<HTMLSelectElement>(row, ".alarm-chime").value,
      chime_volume_pct: clampInt(q<HTMLInputElement>(row, ".alarm-chime-volume").value, 0, 100, 20),
    };
  });
}

// --- Next-fire labels (computed by the backend from the saved config) ---

interface AlarmFireDto {
  id: string;
  at_secs: number; // Unix timestamp of the next fire
}

function fmtFire(epochSecs: number): string {
  return new Date(epochSecs * 1000).toLocaleString(LOCALE, {
    weekday: "short",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** Compact, localized countdown to the next fire: 2d 3h / 14h 23m / 23m / 45s
 *  (zh-Hant: 2天3小時 / 14小時23分 / 23分鐘 / 45秒). Unit text comes from the i18n catalog. */
function fmtCountdown(secs: number): string {
  const total = Math.max(0, Math.floor(secs));
  const d = Math.floor(total / 86400);
  const h = Math.floor((total % 86400) / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  if (d > 0) return t("dur.dh", { d, h });
  if (h > 0) return t("dur.hm", { h, m });
  if (m > 0) return t("dur.m", { m });
  return t("dur.s", { s });
}

/** Refresh each enabled alarm's "in <countdown> — <next fire>" line from the backend. Polled
 *  every second (see init) so the countdown ticks live; disabled alarms get no line. Only the
 *  ".alarm-next" element is touched, never the editable inputs. */
async function refreshFires(): Promise<void> {
  let fires: AlarmFireDto[];
  try {
    fires = await invoke<AlarmFireDto[]>("cmd_get_alarm_fires");
  } catch {
    return; // non-fatal
  }
  const byId = new Map(fires.map((f) => [f.id, f.at_secs]));
  const nowSecs = Date.now() / 1000;
  for (const row of document.querySelectorAll<HTMLElement>(".alarm-item")) {
    const at = byId.get(row.dataset.id ?? "");
    const el = q<HTMLElement>(row, ".alarm-next");
    if (at == null) {
      el.replaceChildren(); // disabled / no upcoming fire -> hidden via :empty
      continue;
    }
    const countdown = document.createElement("span");
    countdown.textContent = t("alarms.in", { dur: fmtCountdown(at - nowSecs) });
    const when = document.createElement("span");
    when.className = "alarm-next__at";
    when.textContent = ` — ${fmtFire(at)}`;
    el.replaceChildren(countdown, when);
  }
}

async function save(): Promise<boolean> {
  const msg = $("save-msg");
  try {
    // Backend sanitizes (disables empty-weekly / dateless-once alarms, dedups ids, …)
    // and echoes the normalized list back, so re-render to reflect it.
    const saved = await invoke<AlarmDto[]>("cmd_save_alarms", { alarms: collectAlarms() });
    renderAlarms(saved);
    await refreshFires();
    msg.textContent = t("common.saved");
    msg.className = "ok";
    guard.markSaved(); // persisted (normalized) -> no longer dirty
    return true;
  } catch (err) {
    msg.textContent = t("settings.save_fail", { err: String(err) });
    msg.className = "warn";
    return false;
  }
}

async function init(): Promise<void> {
  document.title = t("title.alarms");
  applyI18n(document.body);
  invoke("cmd_window_ready", { label: "alarms" }).catch(() => {});
  installPreviewEndedListener(); // revert a chime-picker ▶/⏸ button when its preview ends
  // Load saved chimes first so each alarm row's chime picker can be populated.
  try {
    chimes = await invoke<ChimeOption[]>("cmd_get_chimes");
  } catch {
    chimes = []; // non-fatal: picker just shows "Default"
  }
  renderAlarms(await invoke<AlarmDto[]>("cmd_get_alarms"));
  await refreshFires();
  // Guard against closing with unsaved edits (Close button + OS window X). Installed after the
  // first render so the dirty baseline matches the loaded alarms.
  guard = installUnsavedGuard({
    collect: collectAlarms,
    save,
    close: () => void invoke("cmd_close_alarms"),
  });
  // Live countdowns: poll the backend each second (cheap; mirrors the rules window's status
  // poll, and auto-picks-up an alarm re-arming after it fires). Only the ".alarm-next" lines
  // are rewritten — never the editable rows — so in-progress edits are never discarded.
  window.setInterval(() => void refreshFires(), 1000);
  // Refresh immediately on focus too (snappy return from another window).
  window.addEventListener("focus", () => {
    refreshFires().catch(() => {});
  });

  $("add-alarm").addEventListener("click", () => {
    $("alarms").appendChild(
      alarmRow({
        id: crypto.randomUUID(),
        name: t("alarms.new_name"),
        time: "08:00",
        repeat: "daily",
        weekdays: [],
        day_of_month: 1,
        month: 1,
        date: null,
        enabled: true,
        chime_id: "",
        chime_volume_pct: 20,
      }),
    );
  });
  $("save-btn").addEventListener("click", () => void save());
  $("close-btn").addEventListener("click", () => void guard.requestClose());
}

window.addEventListener("DOMContentLoaded", () => {
  init().catch((err) => console.error("gomaju alarms init failed", err));
});
