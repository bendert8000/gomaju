import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { applyI18n, t } from "./i18n";
import { installLocaleReload } from "./locale-reload";
import { confirmStopwatchClose } from "./confirm-save";

// A single, window-scoped stopwatch. All state lives here in the frontend (no backend run-state),
// so closing the window — or an app-language change, which recreates the window — resets it.
// Elapsed time is derived from `performance.now()` (monotonic), not by accumulating frames, so it
// stays accurate even when requestAnimationFrame is throttled while the window is backgrounded:
// the next visible frame recomputes the true elapsed.

const $ = <T extends HTMLElement>(id: string): T => document.getElementById(id) as T;

// 99:59:59.99 in milliseconds — the maximum the stopwatch counts to, then freezes.
const MAX_MS = 359_999_990;

let running = false;
let baseMs = 0; // accumulated elapsed at the last pause/start (0 when idle/reset)
let startPerf = 0; // performance.now() captured at the last Start/Resume
let laps: number[] = []; // cumulative elapsed (ms) at each Lap press
let rafId: number | undefined;
let closing = false; // a close confirm is in flight (coalesce repeated X / Close)

// Single source of truth for the current elapsed, clamped to the cap on EVERY read (so a
// background-throttled frame can't overshoot it).
function elapsedMs(): number {
  const raw = running ? baseMs + (performance.now() - startPerf) : baseMs;
  return Math.min(MAX_MS, raw);
}

// Format milliseconds as MM:SS.cc, or H:MM:SS.cc once past an hour (hours unpadded, max 99).
function fmt(ms: number): string {
  const totalCs = Math.floor(ms / 10);
  const cc = String(totalCs % 100).padStart(2, "0");
  const totalSec = Math.floor(totalCs / 100);
  const ss = String(totalSec % 60).padStart(2, "0");
  const m = Math.floor(totalSec / 60) % 60;
  const h = Math.floor(totalSec / 3600);
  if (h > 0) return `${h}:${String(m).padStart(2, "0")}:${ss}.${cc}`;
  return `${String(m).padStart(2, "0")}:${ss}.${cc}`;
}

// Write the big display, splitting off the ".cc" centiseconds into its own (smaller) span at the
// decimal point. The only "." in the format separates seconds from centiseconds.
function setDisplay(ms: number): void {
  const s = fmt(ms);
  const dot = s.lastIndexOf(".");
  mainEl.textContent = s.slice(0, dot);
  csEl.textContent = s.slice(dot);
}

// Element refs (assigned in init).
let mainEl: HTMLElement; // the MM:SS / H:MM:SS part
let csEl: HTMLElement; // the ".cc" centiseconds part (rendered smaller)
let toggleBtn: HTMLButtonElement;
let lapBtn: HTMLButtonElement;
let resetBtn: HTMLButtonElement;
let lapsWrap: HTMLElement;
let lapsList: HTMLElement;
let noLaps: HTMLElement;

function stopLoop(): void {
  if (rafId !== undefined) {
    cancelAnimationFrame(rafId);
    rafId = undefined;
  }
}

function tick(): void {
  const e = elapsedMs();
  setDisplay(e);
  if (running && e >= MAX_MS) {
    pause(); // hit the cap — freeze
    return;
  }
  if (running) rafId = requestAnimationFrame(tick);
}

function startLoop(): void {
  stopLoop();
  rafId = requestAnimationFrame(tick);
}

function start(): void {
  if (running || baseMs >= MAX_MS) return; // already running, or already at the cap
  running = true;
  startPerf = performance.now();
  startLoop();
  render();
}

function pause(): void {
  if (!running) return;
  baseMs = elapsedMs(); // clamped
  running = false;
  stopLoop();
  render();
}

function toggle(): void {
  // Short audio feedback for the Start/Pause action (best-effort; played by the backend).
  invoke("cmd_stopwatch_beep").catch(() => {});
  running ? pause() : start();
}

function lap(): void {
  if (!running) return;
  laps.push(elapsedMs());
  renderLaps();
}

function reset(): void {
  running = false;
  stopLoop();
  baseMs = 0;
  laps = [];
  render();
  renderLaps();
}

// The stopwatch carries unsaved "data" once it has started, accumulated time, or recorded laps.
function hasData(): boolean {
  return running || baseMs > 0 || laps.length > 0;
}

function doClose(): void {
  invoke("cmd_close_stopwatch").catch((err) => console.error("gomaju: close stopwatch failed", err));
}

// Close the window — but if the stopwatch holds data, confirm first (closing resets it, since the
// state is window-only). Drives BOTH close paths (the Close button and the OS title-bar X), so
// neither can silently discard a running stopwatch.
async function requestClose(): Promise<void> {
  if (closing) return; // a confirm is already up — ignore the repeated request
  if (!hasData()) {
    doClose();
    return;
  }
  closing = true;
  try {
    if (await confirmStopwatchClose()) doClose();
  } finally {
    closing = false;
  }
}

function render(): void {
  setDisplay(elapsedMs());
  toggleBtn.textContent = running
    ? t("stopwatch.pause")
    : baseMs > 0
      ? t("stopwatch.resume")
      : t("stopwatch.start");
  toggleBtn.disabled = !running && baseMs >= MAX_MS; // at the cap there's nothing to resume
  lapBtn.disabled = !running; // lap only while running
  resetBtn.disabled = running || (baseMs === 0 && laps.length === 0); // pause first; nothing to clear
}

function renderLaps(): void {
  lapsList.replaceChildren();
  // Newest on top; cap the rendered rows so a very long session can't grow the DOM unbounded.
  const stop = Math.max(0, laps.length - 999);
  for (let i = laps.length - 1; i >= stop; i--) {
    const total = laps[i];
    const split = total - (i > 0 ? laps[i - 1] : 0);
    const li = document.createElement("li");
    li.className = "stopwatch-lap";
    const n = document.createElement("span");
    n.className = "stopwatch-lap__n";
    n.textContent = `${t("stopwatch.lap")} ${i + 1}`;
    const s = document.createElement("span");
    s.className = "stopwatch-lap__split";
    s.textContent = fmt(split);
    const tt = document.createElement("span");
    tt.className = "stopwatch-lap__total";
    tt.textContent = fmt(total);
    li.append(n, s, tt);
    lapsList.appendChild(li);
  }
  lapsWrap.hidden = laps.length === 0;
  noLaps.hidden = laps.length !== 0;
}

function init(): void {
  document.title = t("title.stopwatch");
  applyI18n(document.body);
  invoke("cmd_window_ready", { label: "stopwatch" }).catch(() => {});

  mainEl = $("sw-main");
  csEl = $("sw-cs");
  toggleBtn = $<HTMLButtonElement>("sw-toggle");
  lapBtn = $<HTMLButtonElement>("sw-lap");
  resetBtn = $<HTMLButtonElement>("sw-reset");
  lapsWrap = $("sw-laps-wrap");
  lapsList = $("sw-laps");
  noLaps = $("sw-no-laps");

  toggleBtn.addEventListener("click", toggle);
  lapBtn.addEventListener("click", lap);
  resetBtn.addEventListener("click", reset);
  $("close-btn").addEventListener("click", () => void requestClose());
  // OS title-bar X / Alt+F4: intercept so a close with data is confirmed first. `cmd_close_stopwatch`
  // uses destroy() (no close-requested re-emit), so the confirmed close doesn't re-enter this.
  void getCurrentWindow().onCloseRequested((event) => {
    event.preventDefault();
    void requestClose();
  });
  // Spacebar = Start/Pause (ignored while a button is focused, so Enter/Space on a button still
  // activates that button rather than toggling).
  window.addEventListener("keydown", (e) => {
    if (e.code === "Space" && e.target === document.body) {
      e.preventDefault();
      toggle();
    }
  });
  // A Saved language change recreates this window (and resets the stopwatch — its chosen behavior).
  installLocaleReload();

  render();
  renderLaps();
}

window.addEventListener("DOMContentLoaded", () => init());
