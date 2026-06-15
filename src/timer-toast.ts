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

  // Progress bar — mirrors the displayed value over the duration, following the Timer direction:
  // counting up it fills from empty (elapsed/duration); counting down it drains from full
  // (remaining/duration). Hidden when the setting is off.
  const barTrack = $("bar-track");
  barTrack.hidden = !info.progress;
  const bar = $("bar");
  const setBar = (shown: number): void => {
    if (info.progress && info.duration_secs > 0) {
      bar.style.width = `${Math.min(100, (shown / info.duration_secs) * 100)}%`;
    }
  };

  // Running toast: count down to 0, or up to the configured duration; the bar tracks the same value.
  let shown = info.count_up
    ? Math.max(0, info.duration_secs - info.remaining_secs)
    : info.remaining_secs;
  time.textContent = fmt(shown);
  // Paint the initial bar without the 1s intro animation, so a countdown bar starts full rather than
  // animating up from empty before it drains.
  if (info.progress) {
    bar.style.transition = "none";
    setBar(shown);
    void bar.offsetWidth; // force a reflow to commit the un-transitioned width
    bar.style.transition = "";
  }
  window.setInterval(() => {
    shown = info.count_up
      ? Math.min(info.duration_secs, shown + 1)
      : Math.max(0, shown - 1);
    time.textContent = fmt(shown);
    setBar(shown);
  }, 1000);
});
