import { invoke } from "@tauri-apps/api/core";
import { t } from "./i18n";
import { fmtMMSS, readInjected } from "./util";

// Confirm the embedded toast actually rendered.
invoke("cmd_window_ready", { label: "toast" }).catch(() => {});

interface WarningInfo {
  kind: "soft" | "strict";
  name: string;
  lead_secs: number;
}

const info = readInjected<WarningInfo>("__RESTEE_WARNING__", {
  kind: "soft",
  name: "Break",
  lead_secs: 30,
});

document.body.classList.add(info.kind === "strict" ? "toast--strict" : "toast--soft");

const titleEl = document.getElementById("toast-title")!;
const subEl = document.getElementById("toast-sub")!;
const barEl = document.getElementById("toast-bar") as HTMLElement;

titleEl.textContent = t("toast.title", { name: info.name });

const total = Math.max(1, info.lead_secs);
let remaining = info.lead_secs;

function render(): void {
  // "soon" (not "now") at zero: under idle/suspend the engine may delay the actual
  // start, so the countdown reaching zero doesn't guarantee the break is starting.
  subEl.textContent =
    remaining > 0 ? t("toast.starting_in", { mmss: fmtMMSS(remaining) }) : t("toast.starting_soon");
  // Bar fills toward 100% as the break approaches.
  const pct = Math.max(0, Math.min(100, ((total - remaining) / total) * 100));
  barEl.style.width = `${pct}%`;
}

render();
// Cosmetic countdown; the engine is authoritative and will open the break (which
// closes this toast) or cancel it if the break gets credited away.
const ticker = window.setInterval(() => {
  remaining = Math.max(0, remaining - 1);
  render();
  if (remaining <= 0) window.clearInterval(ticker);
}, 1000);
