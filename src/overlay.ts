import { invoke } from "@tauri-apps/api/core";
import { t } from "./i18n";
import { fmtMMSS, readInjected } from "./util";

// Confirm to the backend that the embedded overlay actually rendered.
invoke("cmd_window_ready", { label: "overlay" }).catch(() => {});

type Escape = "friction" | "easy" | "no_easy_escape";
type BreakDisplay = "countdown" | "progress_bar";

interface BreakInfo {
  kind: "soft" | "strict";
  name: string;
  duration_secs: number;
  escape_mode: Escape;
  break_display: BreakDisplay;
  note: string;
  quote: string;
}

// Injected by the Rust side via an initialization script, so the overlay renders
// correctly without depending on event timing.
const info = readInjected<BreakInfo>("__RESTEE_BREAK__", {
  kind: "soft",
  name: "Break",
  duration_secs: 60,
  escape_mode: "easy",
  break_display: "countdown",
  note: "",
  quote: "",
});

document.body.classList.add(info.kind === "strict" ? "overlay--strict" : "overlay--soft");

const nameEl = document.getElementById("break-name")!;
const timerEl = document.getElementById("break-timer")!;
const hintEl = document.getElementById("break-hint")!;
const skipBtn = document.getElementById("skip-btn") as HTMLButtonElement;
const skipLabel = document.getElementById("skip-label")!;
const skipFill = document.getElementById("skip-fill") as HTMLElement;
const emergencyLabel = document.getElementById("emergency-label")!;
const emergencyFill = document.getElementById("emergency-fill") as HTMLElement;
const progressEl = document.getElementById("break-progress") as HTMLElement;
const progressFill = document.getElementById("break-progress-fill") as HTMLElement;
const progressTextEl = document.getElementById("break-progress-text")!;
const noteEl = document.getElementById("break-note")!;
const quoteEl = document.getElementById("break-quote")!;

nameEl.textContent = info.name;
// Optional per-rule note (read-only), shown under the name. Hidden when empty.
if (info.note) {
  noteEl.textContent = info.note;
  noteEl.hidden = false;
}
// Optional inspirational quote (from the user's quotes.toml), shown above the break name. Hidden when empty.
if (info.quote) {
  quoteEl.textContent = info.quote;
  quoteEl.hidden = false;
}

// Display mode: the big countdown text (default) or a draining progress bar with the
// countdown text inside it. Pick the element that shows the remaining time, and reveal the bar.
const useBar = info.break_display === "progress_bar";
const timeEl = useBar ? progressTextEl : timerEl;
if (useBar) {
  timerEl.hidden = true;
  progressEl.hidden = false;
}

// Bar fill drains full -> empty as the break elapses (the inner text still counts down).
const totalSecs = info.duration_secs || 1; // guard a degraded 0 fallback
const setFill = (rem: number): void => {
  if (useBar) {
    progressFill.style.width = `${Math.min(100, Math.max(0, (rem / totalSecs) * 100))}%`;
  }
};

// Local visual countdown. The engine is authoritative and closes the window at
// the real end of the break.
let remaining = info.duration_secs;
timeEl.textContent = fmtMMSS(remaining);
setFill(remaining); // 100% — matches the CSS base so there's no startup animation
const countdown = window.setInterval(() => {
  remaining = Math.max(0, remaining - 1);
  timeEl.textContent = fmtMMSS(remaining);
  setFill(remaining);
  if (remaining <= 0) window.clearInterval(countdown);
}, 1000);

async function skip(): Promise<void> {
  try {
    await invoke("cmd_skip");
  } catch (err) {
    console.error("restee: skip failed", err);
  }
}

// How long the friction "hold to cancel break" affordance must be held.
const HOLD_MS = 3000;

// Skip affordance per escape mode.
if (info.kind === "soft" || info.escape_mode === "easy") {
  skipBtn.hidden = false;
  skipLabel.textContent = t("overlay.skip");
  skipBtn.addEventListener("click", skip);
} else if (info.escape_mode === "friction") {
  skipBtn.hidden = false;
  skipLabel.textContent = t("overlay.hold_to_skip");
  let holdTimer: number | undefined;
  const beginHold = () => {
    if (holdTimer !== undefined) return;
    // The fill animates 0 -> 100% over HOLD_MS, in lockstep with the timer, so
    // the bar reaching the end is exactly when the skip fires.
    skipFill.style.transition = `width ${HOLD_MS}ms linear`;
    skipFill.style.width = "100%";
    skipLabel.textContent = t("overlay.keep_holding");
    holdTimer = window.setTimeout(skip, HOLD_MS);
  };
  const cancelHold = () => {
    if (holdTimer !== undefined) {
      window.clearTimeout(holdTimer);
      holdTimer = undefined;
    }
    skipFill.style.transition = "none";
    skipFill.style.width = "0";
    skipLabel.textContent = t("overlay.hold_to_skip");
  };
  skipBtn.addEventListener("pointerdown", beginHold);
  skipBtn.addEventListener("pointerup", cancelHold);
  skipBtn.addEventListener("pointerleave", cancelHold);
} else {
  hintEl.textContent = t("overlay.strict_hint");
}

// Safety floor: hold Esc ~10s for an emergency exit, regardless of escape mode.
// The fill grows 0 -> 100% over the same duration, in lockstep with the timer.
const EMERGENCY_HOLD_MS = 10000;
const EMERGENCY_HINT = t("overlay.emergency");
emergencyLabel.textContent = EMERGENCY_HINT;
let escTimer: number | undefined;
window.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && escTimer === undefined) {
    emergencyLabel.textContent = t("overlay.keep_holding_esc");
    emergencyFill.style.transition = `width ${EMERGENCY_HOLD_MS}ms linear`;
    emergencyFill.style.width = "100%";
    escTimer = window.setTimeout(skip, EMERGENCY_HOLD_MS);
  }
});
window.addEventListener("keyup", (e) => {
  if (e.key === "Escape") {
    if (escTimer !== undefined) {
      window.clearTimeout(escTimer);
      escTimer = undefined;
    }
    emergencyFill.style.transition = "none";
    emergencyFill.style.width = "0";
    emergencyLabel.textContent = EMERGENCY_HINT;
  }
});
