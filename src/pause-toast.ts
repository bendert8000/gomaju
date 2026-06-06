import { invoke } from "@tauri-apps/api/core";
import { t } from "./i18n";

invoke("cmd_window_ready", { label: "pause-toast" }).catch(() => {});

const titleEl = document.getElementById("pause-toast-title")!;
const subEl = document.getElementById("pause-toast-sub")!;
const stayEl = document.getElementById("pause-toast-stay") as HTMLButtonElement;
const resumeEl = document.getElementById("pause-toast-resume") as HTMLButtonElement;

titleEl.textContent = t("pause_toast.title");
subEl.textContent = t("pause_toast.sub");
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
