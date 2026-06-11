// Shared ▶/⏸ chime-preview toggle for the chime pickers in the Settings rule editor and the Alarms
// window. Mirrors the Chimes window's preview UX (src/chimes.ts) but previews a *saved* chime by id
// (or the context's built-in default tone), so both windows share one singleton "only one playing"
// state. The backend (audio.rs) enforces a single active preview process-wide and broadcasts
// `preview-ended`; a superseding start reverts the other window's button via that same event.
//
// IMPORTANT: this module has NO top-level side effects. The `preview-ended` listener is installed
// only by `installPreviewEndedListener()`, called from the Settings/Alarms init — never at import.
// (The Breaks window type-imports rule-editor.ts, which imports this; keeping it side-effect-free
// guarantees Breaks never inherits a preview listener even if that import stops being type-only.)

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { t } from "./i18n";

/** Which built-in tone a picker's "Default" (empty id) maps to. Matches the Rust `DefaultTone`. */
export type PreviewKind = "break_start" | "break_over" | "alarm";

// `previewGen` is the backend generation token of the playing preview (0 = none); `previewBtn` is
// the button currently showing ⏸. The backend emits `preview-ended` with the gen when playback
// finishes (or is superseded), so we revert only the matching button.
let previewGen = 0;
let previewBtn: HTMLButtonElement | null = null;

function setIdleLabel(btn: HTMLButtonElement): void {
  btn.textContent = "▶";
  btn.title = t("chimes.preview");
  btn.setAttribute("aria-label", t("chimes.preview"));
  btn.classList.remove("playing");
}

function setPlayingLabel(btn: HTMLButtonElement): void {
  btn.textContent = "⏸";
  btn.title = t("chimes.pause");
  btn.setAttribute("aria-label", t("chimes.pause"));
  btn.classList.add("playing");
}

/** Revert the active preview button to ▶ and clear the playing state. Does NOT stop audio. */
function setPreviewIdle(): void {
  if (previewBtn) {
    setIdleLabel(previewBtn);
    previewBtn = null;
  }
  previewGen = 0;
}

/** Install the single `preview-ended` listener for this window. Call once from init(). */
export function installPreviewEndedListener(): void {
  void listen<number>("preview-ended", (e) => {
    if (e.payload === previewGen) setPreviewIdle();
  });
}

/** Revert the active button AND stop any playing audio. Call before re-rendering rows that carry
 * preview buttons, so the helper never points at a detached button (mirrors chimes.ts). */
export function resetActivePreview(): void {
  const wasPlaying = previewBtn !== null;
  setPreviewIdle();
  if (wasPlaying) {
    invoke("cmd_stop_preview").catch((err) => console.error("gomaju: stop preview failed", err));
  }
}

/** Wire a ▶/⏸ toggle button. `getChimeId` and `getVolumePct` are read at click time (so changing
 * either control then pressing ▶ auditions the current assignment); `kind` picks the built-in tone
 * for an empty ("Default") id. */
export function wirePreviewButton(
  btn: HTMLButtonElement,
  getChimeId: () => string,
  getVolumePct: () => number,
  kind: PreviewKind,
): void {
  setIdleLabel(btn);
  btn.addEventListener("click", () => void toggle(btn, getChimeId, getVolumePct, kind));
}

async function toggle(
  btn: HTMLButtonElement,
  getChimeId: () => string,
  getVolumePct: () => number,
  kind: PreviewKind,
): Promise<void> {
  if (btn === previewBtn) {
    // This button is playing — the icon is ⏸, so stop it.
    setPreviewIdle();
    invoke("cmd_stop_preview").catch((err) => console.error("gomaju: stop preview failed", err));
    return;
  }
  setPreviewIdle(); // revert any other active button; the backend stops its audio when we start
  try {
    const gen = await invoke<number>("cmd_preview_chime_by_id", {
      chimeId: getChimeId(),
      volumePct: Math.min(100, Math.max(0, Math.round(getVolumePct()))),
      kind,
    });
    if (!gen) return; // nothing to play (e.g. an empty/missing chime with no default)
    previewGen = gen;
    previewBtn = btn;
    setPlayingLabel(btn);
  } catch (err) {
    setPreviewIdle();
    console.error("gomaju: chime preview failed", err);
  }
}
