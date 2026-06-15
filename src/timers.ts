import { invoke } from "@tauri-apps/api/core";
import { applyI18n, t } from "./i18n";
import { fillChimeSelect, type ChimeOption } from "./rule-editor";
import { installUnsavedGuard, type UnsavedGuard } from "./unsaved-guard";
import {
  installPreviewEndedListener,
  resetActivePreview,
  wirePreviewButton,
} from "./chime-preview";

// Assigned in init() once the timers are first rendered; referenced only afterwards.
let guard!: UnsavedGuard;
// Saved chimes (for each timer's chime picker), loaded once in init().
let chimes: ChimeOption[] = [];

// --- Types mirroring gomaju_core::countdown + the host CountdownView ---

interface CountdownDto {
  id: string;
  duration_secs: number; // 1..=86_399 (00:00:01 .. 23:59:59)
  chime_id: string; // id of a saved chime to play (empty = default tone)
  chime_volume_pct: number; // 0..=100
}

type RunState = "idle" | "running" | "paused";

interface CountdownView {
  def: CountdownDto;
  state: RunState;
  remaining_secs: number;
  count_up: boolean;
}

const MIN_SECS = 1;
const MAX_SECS = 99 * 3600 + 59 * 60 + 59; // 359_999 (99:59:59)
// Per-section caps for the hh / mm / ss sub-fields.
const MAX_H = 99;
const MAX_M = 59;
const MAX_S = 59;

const $ = <T extends HTMLElement>(id: string): T => document.getElementById(id) as T;
const q = <T extends HTMLElement>(root: HTMLElement, selector: string): T =>
  root.querySelector(selector) as T;

function clampInt(value: string, lo: number, hi: number, fallback: number): number {
  const n = Math.round(Number(value));
  return Number.isFinite(n) ? Math.min(hi, Math.max(lo, n)) : fallback;
}

function splitHMS(total: number): { h: number; m: number; s: number } {
  const t = Math.max(0, Math.floor(total));
  return { h: Math.floor(t / 3600), m: Math.floor((t % 3600) / 60), s: t % 60 };
}

/** The live number to show for a view: elapsed (0→duration) in count-up mode, else remaining. */
function displaySecs(v: CountdownView): number {
  return v.count_up ? Math.max(0, v.def.duration_secs - v.remaining_secs) : v.remaining_secs;
}

/** Remaining as `mm:ss`, or `h:mm:ss` past an hour. Matches the tray's `fmt_clock`. */
function fmtClock(total: number): string {
  const { h, m, s } = splitHMS(total);
  const p = (n: number): string => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${p(m)}:${p(s)}` : `${p(m)}:${p(s)}`;
}

/** Parse one duration sub-field (hh/mm/ss) to an int in `[0, max]`. Non-digits / blank → 0;
 *  over `max` → `max` (the "set to maximum" rule). A controlled text field is used (not
 *  `<input type="number">`, whose `.select()` is a no-op in Chromium), so the value is a string. */
function clampField(value: string, max: number): number {
  const digits = value.replace(/\D/g, "");
  if (digits === "") return 0;
  return Math.min(max, Number(digits));
}

/** Clamp a total duration into the valid 1..=359_999 s (00:00:01 .. 99:59:59) range. */
function clampDuration(secs: number): number {
  return Number.isFinite(secs) ? Math.min(MAX_SECS, Math.max(MIN_SECS, secs)) : MIN_SECS;
}

/** Wire one hh/mm/ss sub-field: select the whole section on entry (so the user types straight
 *  in, no double-click), clamp to `max` live, auto-advance to `next` once 2 digits are entered,
 *  zero-pad on blur, and move between sections with ←/→. `type="text"` is required for
 *  `.select()` to work in WebView2. */
function wireDurationField(
  input: HTMLInputElement,
  max: number,
  next?: HTMLInputElement,
  prev?: HTMLInputElement,
): void {
  // Select the whole field on focus/click — deferred past the browser's click caret placement.
  const selectAll = (): void => {
    window.setTimeout(() => input.select(), 0);
  };
  input.addEventListener("focus", selectAll);
  input.addEventListener("click", selectAll);
  input.addEventListener("input", () => {
    const digits = input.value.replace(/\D/g, "").slice(0, 2);
    const clamped = digits === "" ? "" : String(Math.min(max, Number(digits)));
    if (clamped !== input.value) input.value = clamped;
    if (next && digits.length >= 2) next.focus(); // section full -> jump to the next
  });
  input.addEventListener("keydown", (e) => {
    // ←/→ move between the hh : mm : ss sections. From a full selection (the on-focus state) ←
    // goes to the previous section and → to the next; a mid-field caret moves within the field
    // first and only jumps once it reaches the edge (standard segmented-input behavior).
    if (e.key === "ArrowLeft" && prev && input.selectionStart === 0) {
      e.preventDefault();
      prev.focus();
    } else if (e.key === "ArrowRight" && next && input.selectionEnd === input.value.length) {
      e.preventDefault();
      next.focus();
    } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      // ↑/↓ step this section by 1, clamped to [0, max] (no wrap); key-repeat holds to scrub.
      e.preventDefault();
      const delta = e.key === "ArrowUp" ? 1 : -1;
      const val = Math.min(max, Math.max(0, clampField(input.value, max) + delta));
      input.value = String(val).padStart(2, "0");
      input.select();
    }
  });
  input.addEventListener("blur", () => {
    input.value = String(clampField(input.value, max)).padStart(2, "0");
  });
}

// --- Timer rows ---

/** Reflect run state in a row: the toggle label, the live readout (remaining or, in count-up mode,
 *  elapsed), and a state class. Never touches the editable inputs (duration / chime), so the
 *  per-second poll can't discard in-progress edits. The caller passes the display value via
 *  [`displaySecs`]. */
function applyRunState(row: HTMLElement, state: RunState, secs: number): void {
  row.dataset.state = state;
  const toggle = q<HTMLButtonElement>(row, ".timer-toggle");
  toggle.textContent =
    state === "running"
      ? t("timers.pause")
      : state === "paused"
        ? t("timers.resume")
        : t("timers.start");
  const rem = q<HTMLElement>(row, ".timer-remaining");
  rem.textContent = state === "idle" ? "" : fmtClock(secs);
}

function timerRow(v: CountdownView): HTMLElement {
  const row = document.createElement("div");
  row.className = "alarm-item timer-item"; // reuse the alarm card styling
  row.dataset.id = v.def.id;
  // Static scaffolding only (constant labels). User-supplied values are set via DOM setters
  // below, so there's no interpolation/XSS surface.
  row.innerHTML = `
    <div class="alarm-line">
      <label class="timer-duration"><span class="timer-duration-label">${t("timers.duration")}</span><span class="timer-dur-group"><input class="timer-dur-h" type="text" inputmode="numeric" maxlength="2" aria-label="${t("timers.hours")}" /><span class="timer-dur-sep">:</span><input class="timer-dur-m" type="text" inputmode="numeric" maxlength="2" aria-label="${t("timers.minutes")}" /><span class="timer-dur-sep">:</span><input class="timer-dur-s" type="text" inputmode="numeric" maxlength="2" aria-label="${t("timers.seconds")}" /></span></label>
      <button class="timer-remove btn-ghost" type="button" title="${t("common.remove")}">✕</button>
    </div>
    <div class="alarm-line alarm-chime-row">
      <label>${t("chime.label")} <select class="timer-chime"></select><span>${t("chimes.volume")}</span><input class="timer-chime-volume chime-volume-picker" type="number" min="0" max="100" /><button class="timer-chime-preview btn-ghost chime-preview-btn" type="button"></button></label>
    </div>
    <div class="alarm-line timer-controls">
      <button class="timer-toggle btn-primary" type="button"></button>
      <button class="timer-reset btn-ghost" type="button">${t("timers.reset")}</button>
      <span class="timer-remaining"></span>
    </div>
  `;

  const hh = q<HTMLInputElement>(row, ".timer-dur-h");
  const mm = q<HTMLInputElement>(row, ".timer-dur-m");
  const ss = q<HTMLInputElement>(row, ".timer-dur-s");
  const { h, m, s } = splitHMS(v.def.duration_secs);
  const pad2 = (n: number): string => String(n).padStart(2, "0");
  hh.value = pad2(h);
  mm.value = pad2(m);
  ss.value = pad2(s);
  // Each section: select-on-focus, per-section clamp, auto-advance hh -> mm -> ss, and ←/→
  // navigation between sections.
  wireDurationField(hh, MAX_H, mm, undefined);
  wireDurationField(mm, MAX_M, ss, hh);
  wireDurationField(ss, MAX_S, undefined, mm);

  const chimeSel = q<HTMLSelectElement>(row, ".timer-chime");
  fillChimeSelect(chimeSel, chimes, v.def.chime_id ?? "");
  q<HTMLInputElement>(row, ".timer-chime-volume").value = String(
    clampInt(String(v.def.chime_volume_pct ?? 20), 0, 100, 20),
  );
  // ▶/⏸ preview after the picker; "Default" (empty) auditions the built-in tone (alarm tone,
  // which is also the countdown default — see audio::play_countdown_chime).
  wirePreviewButton(
    q<HTMLButtonElement>(row, ".timer-chime-preview"),
    () => chimeSel.value,
    () => clampInt(q<HTMLInputElement>(row, ".timer-chime-volume").value, 0, 100, 20),
    "alarm",
  );

  q<HTMLButtonElement>(row, ".timer-toggle").addEventListener("click", () => void onToggle(row));
  q(row, ".timer-reset").addEventListener("click", () => void resetTimer(row.dataset.id ?? ""));
  q(row, ".timer-remove").addEventListener("click", () => row.remove());

  applyRunState(row, v.state, displaySecs(v));
  return row;
}

function renderTimers(views: CountdownView[]): void {
  resetActivePreview(); // rows (and their preview buttons) are about to be rebuilt
  const container = $("timers");
  container.innerHTML = "";
  for (const v of views) container.appendChild(timerRow(v));
}

function collectTimers(): CountdownDto[] {
  return Array.from(document.querySelectorAll<HTMLElement>(".timer-item")).map((row) => ({
    id: row.dataset.id || crypto.randomUUID(),
    duration_secs: clampDuration(
      clampField(q<HTMLInputElement>(row, ".timer-dur-h").value, MAX_H) * 3600 +
        clampField(q<HTMLInputElement>(row, ".timer-dur-m").value, MAX_M) * 60 +
        clampField(q<HTMLInputElement>(row, ".timer-dur-s").value, MAX_S),
    ),
    chime_id: q<HTMLSelectElement>(row, ".timer-chime").value,
    chime_volume_pct: clampInt(q<HTMLInputElement>(row, ".timer-chime-volume").value, 0, 100, 20),
  }));
}

// --- Live run state ---

/** Poll the backend for each timer's run state + remaining, updating only the live elements
 *  (toggle label + remaining), never the editable inputs. Mirrors the alarms window's poll. */
async function refresh(): Promise<void> {
  let views: CountdownView[];
  try {
    views = await invoke<CountdownView[]>("cmd_get_countdowns");
  } catch {
    return; // non-fatal
  }
  const byId = new Map(views.map((v) => [v.def.id, v]));
  for (const row of document.querySelectorAll<HTMLElement>(".timer-item")) {
    const v = byId.get(row.dataset.id ?? "");
    if (!v) continue; // a freshly-added, not-yet-saved row
    applyRunState(row, v.state, displaySecs(v));
  }
}

function onToggle(row: HTMLElement): Promise<void> {
  const id = row.dataset.id ?? "";
  return row.dataset.state === "running" ? pauseTimer(id) : startTimer(id);
}

async function startTimer(id: string): Promise<void> {
  // The backend reads each timer's duration from the *saved* config, so a new or edited timer
  // must be persisted before it can start. Save only when dirty to avoid surprise re-renders.
  // (Saving re-renders the rows, but we act by `id` and refresh() re-queries the DOM, so a stale
  // element reference is never used.)
  if (guard.isDirty()) {
    if (!(await save())) return;
  }
  await invoke("cmd_start_countdown", { id });
  await refresh();
}

async function pauseTimer(id: string): Promise<void> {
  await invoke("cmd_pause_countdown", { id });
  await refresh();
}

async function resetTimer(id: string): Promise<void> {
  await invoke("cmd_reset_countdown", { id });
  await refresh();
}

async function save(): Promise<boolean> {
  const msg = $("save-msg");
  try {
    // Backend sanitizes (clamps durations to 1..=86399, regenerates blank/dup ids) and echoes
    // the normalized list back *with* live run state, so re-render to reflect both.
    const saved = await invoke<CountdownView[]>("cmd_save_countdowns", {
      countdowns: collectTimers(),
    });
    renderTimers(saved);
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
  document.title = t("title.timers");
  applyI18n(document.body);
  invoke("cmd_window_ready", { label: "timers" }).catch(() => {});
  installPreviewEndedListener(); // revert a chime-picker ▶/⏸ button when its preview ends
  // Load saved chimes first so each timer row's chime picker can be populated.
  try {
    chimes = await invoke<ChimeOption[]>("cmd_get_chimes");
  } catch {
    chimes = []; // non-fatal: picker just shows "Default"
  }
  renderTimers(await invoke<CountdownView[]>("cmd_get_countdowns"));
  // Guard against closing with unsaved edits (Close button + OS window X). Installed after the
  // first render so the dirty baseline matches the loaded timers.
  guard = installUnsavedGuard({
    collect: collectTimers,
    save,
    close: () => void invoke("cmd_close_countdowns"),
  });
  // Live countdowns: poll each second (cheap; mirrors the alarms window). Only the toggle label
  // and remaining readout are rewritten — never the editable rows — so edits are never discarded.
  window.setInterval(() => void refresh(), 1000);
  window.addEventListener("focus", () => {
    refresh().catch(() => {});
  });

  $("add-timer").addEventListener("click", () => {
    $("timers").appendChild(
      timerRow({
        def: {
          id: crypto.randomUUID(),
          duration_secs: 5 * 60,
          chime_id: "",
          chime_volume_pct: 20,
        },
        state: "idle",
        remaining_secs: 5 * 60,
        count_up: false,
      }),
    );
  });
  $("save-btn").addEventListener("click", () => void save());
  $("close-btn").addEventListener("click", () => void guard.requestClose());
}

window.addEventListener("DOMContentLoaded", () => {
  init().catch((err) => console.error("gomaju timers init failed", err));
});
