import { invoke } from "@tauri-apps/api/core";
import { t } from "./i18n";
import { readInjected } from "./util";

// Injected by timer_toast.rs::build_toast before the page loads.
interface ToastInfo {
  id: string;
  name: string;
  remaining_secs: number;
  finished: boolean;
  count_up: boolean;
  duration_secs: number;
  progress: boolean;
}

const info = readInjected<ToastInfo>("__GOMAJU_TIMER_TOAST__", {
  id: "",
  name: "",
  remaining_secs: 0,
  finished: false,
  count_up: false,
  duration_secs: 0,
  progress: false,
});

const $ = (id: string): HTMLElement => document.getElementById(id) as HTMLElement;

/** Remaining as `mm:ss`, or `h:mm:ss` past an hour. */
function fmt(total: number): string {
  const secs = Math.max(0, Math.floor(total));
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  const p = (n: number): string => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${p(m)}:${p(s)}` : `${p(m)}:${p(s)}`;
}

window.addEventListener("DOMContentLoaded", () => {
  // This window's own label: running -> timer-toast-<id>, finished -> timer-done-<id>.
  // Source of truth for these prefixes is Rust (TIMER_TOAST_PREFIX / TIMER_DONE_PREFIX in
  // timer_toast.rs); keep them in sync if either side is renamed.
  const label = `${info.finished ? "timer-done-" : "timer-toast-"}${info.id}`;
  // Signal the page loaded (a useful "embedded assets actually loaded" trace).
  invoke("cmd_window_ready", { label }).catch(() => {});

  $("name").textContent = info.name;
  const stop = $("stop") as HTMLButtonElement;
  const time = $("time");

  if (info.finished) {
    // Terminal "Time's up!" toast: no countdown, the ✕ just dismisses (the id is derived from this
    // window's own label on the Rust side — no arg to spoof).
    $("icon").textContent = "⏰";
    time.textContent = t("timers.times_up");
    stop.title = t("timers.dismiss");
    stop.setAttribute("aria-label", t("timers.dismiss"));
    stop.addEventListener("click", () => {
      invoke("cmd_dismiss_timer_done").catch(() => {});
    });
    $("bar-track").hidden = true; // terminal toast: no progress bar
    return;
  }

  // Running countdown toast.
  stop.title = t("timers.stop");
  stop.setAttribute("aria-label", t("timers.stop"));
  stop.addEventListener("click", () => {
    invoke("cmd_toast_stop_countdown").catch(() => {});
  });

  // Progress bar (fills with elapsed/duration, both modes). Hidden when the setting is off.
  const barTrack = $("bar-track");
  barTrack.hidden = !info.progress;
  const bar = $("bar");
  const setBar = (elapsed: number): void => {
    if (info.progress && info.duration_secs > 0) {
      bar.style.width = `${Math.min(100, (elapsed / info.duration_secs) * 100)}%`;
    }
  };

  // Running toast: count down to 0, or up to the configured duration.
  if (info.count_up) {
    let elapsed = Math.max(0, info.duration_secs - info.remaining_secs);
    time.textContent = fmt(elapsed);
    setBar(elapsed);
    window.setInterval(() => {
      elapsed = Math.min(info.duration_secs, elapsed + 1);
      time.textContent = fmt(elapsed);
      setBar(elapsed);
    }, 1000);
  } else {
    let remaining = info.remaining_secs;
    time.textContent = fmt(remaining);
    setBar(info.duration_secs - remaining);
    window.setInterval(() => {
      remaining = Math.max(0, remaining - 1);
      time.textContent = fmt(remaining);
      setBar(info.duration_secs - remaining);
    }, 1000);
  }
});
