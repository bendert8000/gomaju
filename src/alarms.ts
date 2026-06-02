import { invoke } from "@tauri-apps/api/core";

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

const WEEKDAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

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
      <input class="alarm-name" type="text" placeholder="Alarm name" />
      <input class="alarm-time" type="time" />
      <label class="alarm-on"><input class="alarm-enabled" type="checkbox" /> On</label>
      <button class="alarm-remove btn-ghost" type="button" title="Remove">✕</button>
    </div>
    <div class="alarm-line alarm-sched">
      <select class="alarm-repeat">
        <option value="once">Once</option>
        <option value="daily">Daily</option>
        <option value="weekly">Weekly</option>
        <option value="monthly">Monthly</option>
        <option value="yearly">Yearly</option>
      </select>
      <span class="alarm-detail alarm-detail-once"><input class="alarm-date" type="date" /></span>
      <span class="alarm-detail alarm-detail-weekly">${WEEKDAYS.map(
        (w, i) => `<label class="wd-label"><input class="wd" type="checkbox" value="${i}" />${w}</label>`,
      ).join("")}</span>
      <span class="alarm-detail alarm-detail-monthly">Day <input class="alarm-dom" type="number" min="1" max="31" /></span>
      <span class="alarm-detail alarm-detail-yearly">
        <select class="alarm-month">${Array.from(
          { length: 12 },
          (_, i) => `<option value="${i + 1}">${i + 1}</option>`,
        ).join("")}</select>
        Day <input class="alarm-doy" type="number" min="1" max="31" />
      </span>
    </div>
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
      name: q<HTMLInputElement>(row, ".alarm-name").value.trim() || "Alarm",
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

async function save(): Promise<void> {
  const msg = $("save-msg");
  try {
    // Backend sanitizes (disables empty-weekly / dateless-once alarms, dedups ids, …)
    // and echoes the normalized list back, so re-render to reflect it.
    const saved = await invoke<AlarmDto[]>("cmd_save_alarms", { alarms: collectAlarms() });
    renderAlarms(saved);
    msg.textContent = "Saved ✓";
    msg.className = "ok";
  } catch (err) {
    msg.textContent = `Save failed: ${err}`;
    msg.className = "warn";
  }
}

async function init(): Promise<void> {
  invoke("cmd_window_ready", { label: "alarms" }).catch(() => {});
  renderAlarms(await invoke<AlarmDto[]>("cmd_get_alarms"));

  $("add-alarm").addEventListener("click", () => {
    $("alarms").appendChild(
      alarmRow({
        id: crypto.randomUUID(),
        name: "New alarm",
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
  $("save-btn").addEventListener("click", save);
  $("close-btn").addEventListener("click", () => invoke("cmd_close_alarms"));
}

window.addEventListener("DOMContentLoaded", () => {
  init().catch((err) => console.error("restee alarms init failed", err));
});
