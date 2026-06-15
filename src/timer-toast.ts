import { invoke } from "@tauri-apps/api/core";
import { t } from "./i18n";
import { readInjected } from "./util";

// Injected by timer_toast.rs::build_toast before the page loads.
interface ToastInfo {
  id: string;
  name: string;
  remaining_secs: number;
}

const info = readInjected<ToastInfo>("__GOMAJU_TIMER_TOAST__", {
  id: "",
  name: "",
  remaining_secs: 0,
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
  // Signal the page loaded (a useful "embedded assets actually loaded" trace).
  invoke("cmd_window_ready", { label: `timer-toast-${info.id}` }).catch(() => {});

  $("name").textContent = info.name;
  const stop = $("stop") as HTMLButtonElement;
  stop.title = t("timers.stop");
  stop.setAttribute("aria-label", t("timers.stop"));
  // The id is derived from this window's own label on the Rust side — no arg to spoof.
  stop.addEventListener("click", () => {
    invoke("cmd_toast_stop_countdown").catch(() => {});
  });

  // Count down locally; the host closes this window on finish/stop.
  let remaining = info.remaining_secs;
  const time = $("time");
  time.textContent = fmt(remaining);
  window.setInterval(() => {
    remaining = Math.max(0, remaining - 1);
    time.textContent = fmt(remaining);
  }, 1000);
});
