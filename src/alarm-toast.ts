import { invoke } from "@tauri-apps/api/core";
import { t } from "./i18n";
import { readInjected } from "./util";

// Injected by alarm_toast.rs::build_toast before the page loads.
interface AlarmToastInfo {
  id: string;
  name: string;
  time: string; // 24-hour "HH:MM"
  recurrence: string; // "once" | "daily" | "weekly" | "biweekly" | "monthly" | "yearly"
}

const info = readInjected<AlarmToastInfo>("__GOMAJU_ALARM_TOAST__", {
  id: "",
  name: "",
  time: "",
  recurrence: "daily",
});

const $ = (id: string): HTMLElement => document.getElementById(id) as HTMLElement;

window.addEventListener("DOMContentLoaded", () => {
  // This window's own label is alarm-toast-<id>; the prefix's source of truth is Rust
  // (ALARM_TOAST_PREFIX in alarm_toast.rs). Signal the page loaded (a useful trace).
  invoke("cmd_window_ready", { label: `alarm-toast-${info.id}` }).catch(() => {});

  $("name").textContent = info.name;
  $("time").textContent = info.time;
  // Recurrence label reuses the alarms-window strings (alarms.repeat_*).
  $("note").textContent = t(`alarms.repeat_${info.recurrence}`);

  // The ✕ just dismisses: the id is derived from this window's own label on the Rust side (no arg
  // to spoof), and the scheduler's next tick closes the window — we never close it from the command.
  const stop = $("stop") as HTMLButtonElement;
  stop.title = t("timers.dismiss");
  stop.setAttribute("aria-label", t("timers.dismiss"));
  stop.addEventListener("click", () => {
    invoke("cmd_dismiss_alarm_toast").catch(() => {});
  });
});
