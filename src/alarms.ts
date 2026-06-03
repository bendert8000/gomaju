import { invoke } from "@tauri-apps/api/core";
import { applyI18n, LOCALE, t } from "./i18n";
import { installUnsavedGuard, type UnsavedGuard } from "./unsaved-guard";

// Assigned in init() once the alarms are first rendered; referenced only afterwards.
let guard!: UnsavedGuard;

// --- Types mirroring restee_core::alarm DTOs ---

type Repeat = "once" | "daily" | "weekly" | "monthly" | "yearly";

interface AlarmDto {
  id: string;
  name: string;
  time: string; // "HH:MM" (24h)
  repeat: Repeat;
  weekdays: number[]; // 0=Mon … 6=Sun (Weekly)
  day_of_month: number; // 1..31 (Monthly / Yearly)
  month: number; // 1..12 (Yearly)
  date: string | null; // "YYYY-MM-DD" (Once)
  enabled: boolean;
}

const $ = <T extends HTMLElement>(id: string): T => document.getElementById(id) as T;
const q = <T extends HTMLElement>(root: HTMLElement, selector: string): T =>
  root.querySelector(selector) as T;

const WEEKDAYS = Array.from({ length: 7 }, (_, i) => t(`weekday.${i}`));

function clampInt(value: string, lo: number, hi: number, fallback: number): number {
  const n = Math.round(Number(value));
  return Number.isFinite(n) ? Math.min(hi, Math.max(lo, n)) : fallback;
}

// --- Alarm rows ---

function alarmRow(a: AlarmDto): HTMLElement {
  const row = document.createElement("div");
  row.className = "alarm-item";
  row.dataset.id = a.id;
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
        <option value="monthly">${t("alarms.repeat_monthly")}</option>
        <option value="yearly">${t("alarms.repeat_yearly")}</option>
      </select>
      <span class="alarm-detail alarm-detail-once"><input class="alarm-date" type="date" /></span>
      <span class="alarm-detail alarm-detail-weekly">${WEEKDAYS.map(
        (w, i) => `<label class="wd-label"><input class="wd" type="checkbox" value="${i}" />${w}</label>`,
      ).join("")}</span>
      <span class="alarm-detail alarm-detail-monthly">${t("alarms.day")} <input class="alarm-dom" type="number" min="1" max="31" /></span>
      <span class="alarm-detail alarm-detail-yearly">
        <select class="alarm-month">${Array.from(
          { length: 12 },
          (_, i) => `<option value="${i + 1}">${i + 1}</option>`,
        ).join("")}</select>
        ${t("alarms.day")} <input class="alarm-doy" type="number" min="1" max="31" />
      </span>
    </div>
    <div class="alarm-next muted"></div>
  `;

  q<HTMLInputElement>(row, ".alarm-name").value = a.name;
  q<HTMLInputElement>(row, ".alarm-time").value = a.time || "08:00";
  q<HTMLInputElement>(row, ".alarm-enabled").checked = a.enabled;
  q<HTMLSelectElement>(row, ".alarm-repeat").value = a.repeat;
  q<HTMLInputElement>(row, ".alarm-date").value = a.date ?? "";
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
  };
  repeatSel.addEventListener("change", updateDetail);
  updateDetail();

  q(row, ".alarm-remove").addEventListener("click", () => row.remove());
  return row;
}

function renderAlarms(alarms: AlarmDto[]): void {
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
      weekdays = Array.from(row.querySelectorAll<HTMLInputElement>(".wd"))
        .filter((c) => c.checked)
        .map((c) => Number(c.value));
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

/** Fill each row's "Next: …" label from the backend; enabled alarms only. */
async function refreshFires(): Promise<void> {
  let fires: AlarmFireDto[];
  try {
    fires = await invoke<AlarmFireDto[]>("cmd_get_alarm_fires");
  } catch {
    return; // non-fatal
  }
  const byId = new Map(fires.map((f) => [f.id, f.at_secs]));
  for (const row of document.querySelectorAll<HTMLElement>(".alarm-item")) {
    const at = byId.get(row.dataset.id ?? "");
    q<HTMLElement>(row, ".alarm-next").textContent =
      at == null ? "" : t("alarms.next", { when: fmtFire(at) });
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
  renderAlarms(await invoke<AlarmDto[]>("cmd_get_alarms"));
  await refreshFires();
  // Guard against closing with unsaved edits (Close button + OS window X). Installed after the
  // first render so the dirty baseline matches the loaded alarms.
  guard = installUnsavedGuard({
    collect: collectAlarms,
    save,
    close: () => void invoke("cmd_close_alarms"),
  });
  // Keep the labels current (e.g. a once-alarm that fired + auto-disabled) without
  // re-rendering rows, which would discard any in-progress edits.
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
      }),
    );
  });
  $("save-btn").addEventListener("click", () => void save());
  $("close-btn").addEventListener("click", () => void guard.requestClose());
}

window.addEventListener("DOMContentLoaded", () => {
  init().catch((err) => console.error("restee alarms init failed", err));
});
