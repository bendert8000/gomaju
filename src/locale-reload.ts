import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// When the user changes the UI language in Settings and Saves, the backend broadcasts the
// "locale-reload" event to every open window. The locale is injected at window creation, so a
// window can only adopt a new language by being recreated — each window recreates ITSELF here via
// `cmd_reload_window`. `canClose`, when supplied, is the window's unsaved-changes guard gate: if it
// resolves `false` (the user cancelled a dirty close), the window is left as-is so in-progress
// edits aren't lost; otherwise the window is recreated in the new locale.
export function installLocaleReload(canClose?: () => Promise<boolean>): void {
  void listen("locale-reload", async () => {
    if (canClose && !(await canClose())) return;
    invoke("cmd_reload_window").catch((err) =>
      console.error("gomaju: locale reload failed", err),
    );
  });
}
