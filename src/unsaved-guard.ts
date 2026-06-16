import { getCurrentWindow } from "@tauri-apps/api/window";
import { confirmUnsaved } from "./confirm-save";

// Shared "unsaved changes" close guard for the Settings and Alarms windows. Dirty state lives
// in the form, so the guard lives in the frontend. Both close paths (in-app Close button and the
// OS title-bar X / Alt+F4) funnel through one `requestClose`, coalesced by an in-flight flag.
//
// The actual close calls `hooks.close()` (which invokes the existing cmd_close_* command). Those
// commands are implemented in Rust with `window.destroy()`, which does NOT re-emit
// `close-requested`, so there is no re-entrancy here and no JS window-destroy permission needed.

interface GuardHooks {
  /** Current form -> a JSON-serializable snapshot (e.g. collectConfig / collectAlarms). */
  collect: () => unknown;
  /** Persist the form. Resolve `true` on success (data written), `false` on failure. */
  save: () => Promise<boolean>;
  /** Perform the actual window close (invoke cmd_close_*). */
  close: () => void;
}

export interface UnsavedGuard {
  /** True when the live form differs from the last saved/loaded baseline. */
  isDirty: () => boolean;
  /** Re-baseline to the current form (call after a successful save, or a clean focus-refresh). */
  markSaved: () => void;
  /** Run the guard, then close if appropriate. Wire this to the in-app Close button. */
  requestClose: () => Promise<void>;
  /** Prompt about unsaved edits (if any) WITHOUT closing. Resolves `true` when it's OK to proceed
   *  (form was clean, saved, or discarded), `false` when the user cancelled. */
  confirmCanClose: () => Promise<boolean>;
}

const snapshot = (v: unknown): string => JSON.stringify(v);

export function installUnsavedGuard(hooks: GuardHooks): UnsavedGuard {
  let baseline = snapshot(hooks.collect());
  const isDirty = (): boolean => snapshot(hooks.collect()) !== baseline;
  const markSaved = (): void => {
    baseline = snapshot(hooks.collect());
  };

  // Prompt about unsaved edits without closing; resolve whether it's OK to proceed.
  async function confirmCanClose(): Promise<boolean> {
    if (!isDirty()) return true;
    const choice = await confirmUnsaved();
    if (choice === "save") return await hooks.save(); // proceed only if persisted
    if (choice === "dont_save") return true; // discard
    return false; // cancel -> stay open
  }

  async function decideAndClose(): Promise<void> {
    if (await confirmCanClose()) hooks.close();
  }

  let inFlight = false;
  async function requestClose(): Promise<void> {
    if (inFlight) return; // coalesce rapid X/Close so modals can't stack
    inFlight = true;
    try {
      await decideAndClose();
    } finally {
      inFlight = false;
    }
  }

  // OS title-bar X / Alt+F4: always stop the native close, then drive it ourselves.
  void getCurrentWindow().onCloseRequested((event) => {
    event.preventDefault();
    void requestClose();
  });

  return { isDirty, markSaved, requestClose, confirmCanClose };
}
