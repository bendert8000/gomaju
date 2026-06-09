import { invoke } from "@tauri-apps/api/core";
import { t } from "./i18n";
import { readInjected } from "./util";

invoke("cmd_window_ready", { label: "pause-toast" }).catch(() => {});

// The backend injects the configured reminder interval so the hint can name it.
const pause = readInjected<{ minutes: number }>("__RESTEE_PAUSE__", { minutes: 10 });
const minutes = Math.max(1, Math.round(pause.minutes));

const titleEl = document.getElementById("pause-toast-title")!;
const subEl = document.getElementById("pause-toast-sub")!;
const hintEl = document.getElementById("pause-toast-hint")!;
const stayEl = document.getElementById("pause-toast-stay") as HTMLButtonElement;
const resumeEl = document.getElementById("pause-toast-resume") as HTMLButtonElement;

titleEl.textContent = t("pause_toast.title");
subEl.textContent = t("pause_toast.sub");
hintEl.textContent = t("pause_toast.hint", { minutes });
stayEl.textContent = t("pause_toast.stay_paused");
resumeEl.textContent = t("pause_toast.resume");

stayEl.addEventListener("click", () => {
  stayEl.disabled = true;
  resumeEl.disabled = true;
  invoke("cmd_stay_paused_from_reminder").catch((err) =>
    console.error("restee: stay-paused reminder action failed", err),
  );
});

resumeEl.addEventListener("click", () => {
  stayEl.disabled = true;
  resumeEl.disabled = true;
  invoke("cmd_resume_from_pause_reminder").catch((err) =>
    console.error("restee: resume reminder action failed", err),
  );
});
