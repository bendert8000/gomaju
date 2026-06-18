import { invoke } from "@tauri-apps/api/core";
import { readInjected } from "./util";

// Confirm to the backend that the prompt rendered.
invoke("cmd_window_ready", { label: "confirm" }).catch(() => {});

interface ConfirmInfo {
  // Action descriptor, echoed back so the backend knows what to run.
  kind: string;
  rule_id: string;
  // Already-localized display strings (the backend computes them per the active locale).
  title: string;
  message: string;
  primary: string;
  secondary: string;
}

const info = readInjected<ConfirmInfo>("__GOMAJU_CONFIRM__", {
  kind: "",
  rule_id: "",
  title: "",
  message: "",
  primary: "OK",
  secondary: "Cancel",
});

const titleEl = document.getElementById("confirm-title")!;
const messageEl = document.getElementById("confirm-message")!;
const primaryBtn = document.getElementById("confirm-primary") as HTMLButtonElement;
const secondaryBtn = document.getElementById("confirm-secondary") as HTMLButtonElement;

titleEl.textContent = info.title;
messageEl.textContent = info.message;
primaryBtn.textContent = info.primary;
secondaryBtn.textContent = info.secondary;

// One answer only: disable both buttons the instant either is chosen so a double-click (or
// Esc-after-click) can't fire a second action before the window closes.
let answered = false;
function resolve(choice: "primary" | "secondary"): void {
  if (answered) return;
  answered = true;
  primaryBtn.disabled = true;
  secondaryBtn.disabled = true;
  invoke("cmd_confirm_resolve", { kind: info.kind, ruleId: info.rule_id, choice }).catch((err) =>
    console.error("gomaju: confirm resolve failed", err),
  );
}

primaryBtn.addEventListener("click", () => resolve("primary"));
secondaryBtn.addEventListener("click", () => resolve("secondary"));
// Esc = the secondary choice (cancel / start-fresh), matching the old native prompt's close key.
window.addEventListener("keydown", (e) => {
  if (e.key === "Escape") resolve("secondary");
});

primaryBtn.focus();
