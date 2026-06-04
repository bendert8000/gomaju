import { t } from "./i18n";

// A custom 3-button "unsaved changes" modal. Native dialogs (tauri-plugin-dialog) support at
// most two buttons, so this is built in-window. Shared by the Settings and Alarms windows via
// `unsaved-guard.ts`. The guard serializes calls (one modal at a time), so this module keeps no
// open-state of its own beyond the single in-flight promise it creates.

export type CloseChoice = "save" | "dont_save" | "cancel";

/** Show the modal; resolves with the user's choice. Esc / overlay-click = cancel, Enter = save. */
export function confirmUnsaved(): Promise<CloseChoice> {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "modal-overlay";
    // Static scaffolding only; all text is set via textContent below (no interpolation, no XSS).
    overlay.innerHTML = `
      <div class="modal" role="dialog" aria-modal="true" aria-labelledby="modal-title">
        <h2 id="modal-title" class="modal__title"></h2>
        <p class="modal__msg"></p>
        <div class="modal__actions">
          <button class="btn-ghost modal__dont-save" type="button"></button>
          <button class="btn-ghost modal__cancel" type="button"></button>
          <button class="btn-primary modal__save" type="button"></button>
        </div>
      </div>`;

    const $ = <T extends HTMLElement>(sel: string): T => overlay.querySelector(sel) as T;
    $(".modal__title").textContent = t("confirm.unsaved_title");
    $(".modal__msg").textContent = t("confirm.unsaved_msg");
    $(".modal__dont-save").textContent = t("confirm.dont_save");
    $(".modal__cancel").textContent = t("confirm.cancel");
    $(".modal__save").textContent = t("common.save");

    const prevFocus = document.activeElement as HTMLElement | null;

    function done(choice: CloseChoice): void {
      document.removeEventListener("keydown", onKey, true);
      overlay.remove();
      prevFocus?.focus?.();
      resolve(choice);
    }

    function onKey(e: KeyboardEvent): void {
      if (e.key === "Escape") {
        e.preventDefault();
        done("cancel");
      } else if (e.key === "Enter") {
        e.preventDefault();
        done("save");
      }
    }

    $<HTMLButtonElement>(".modal__save").addEventListener("click", () => done("save"));
    $<HTMLButtonElement>(".modal__dont-save").addEventListener("click", () => done("dont_save"));
    $<HTMLButtonElement>(".modal__cancel").addEventListener("click", () => done("cancel"));
    overlay.addEventListener("mousedown", (e) => {
      if (e.target === overlay) done("cancel"); // click outside the dialog
    });
    document.addEventListener("keydown", onKey, true);

    document.body.appendChild(overlay);
    $<HTMLButtonElement>(".modal__save").focus(); // primary action gets focus
  });
}

export type QuotesConflictChoice = "overwrite" | "keep_disk";

/** Shown on Save when `quotes.txt` changed on disk since the Quotes editor last synced. Resolves
 * with the user's choice. The user clicked Save, so "overwrite" (their list wins) is the primary
 * action; Esc / overlay-click = keep_disk, the non-destructive escape (preserve the on-disk edit). */
export function confirmQuotesConflict(): Promise<QuotesConflictChoice> {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "modal-overlay";
    // Static scaffolding only; all text is set via textContent below (no interpolation, no XSS).
    overlay.innerHTML = `
      <div class="modal" role="dialog" aria-modal="true" aria-labelledby="qc-title">
        <h2 id="qc-title" class="modal__title"></h2>
        <p class="modal__msg"></p>
        <div class="modal__actions">
          <button class="btn-ghost modal__keep" type="button"></button>
          <button class="btn-primary modal__overwrite" type="button"></button>
        </div>
      </div>`;

    const $ = <T extends HTMLElement>(sel: string): T => overlay.querySelector(sel) as T;
    $(".modal__title").textContent = t("confirm.quotes_conflict_title");
    $(".modal__msg").textContent = t("confirm.quotes_conflict_msg");
    $(".modal__keep").textContent = t("confirm.quotes_keep_disk");
    $(".modal__overwrite").textContent = t("confirm.quotes_overwrite");

    const prevFocus = document.activeElement as HTMLElement | null;

    function done(choice: QuotesConflictChoice): void {
      document.removeEventListener("keydown", onKey, true);
      overlay.remove();
      prevFocus?.focus?.();
      resolve(choice);
    }

    function onKey(e: KeyboardEvent): void {
      if (e.key === "Escape") {
        e.preventDefault();
        done("keep_disk");
      } else if (e.key === "Enter") {
        e.preventDefault();
        done("overwrite");
      }
    }

    $<HTMLButtonElement>(".modal__overwrite").addEventListener("click", () => done("overwrite"));
    $<HTMLButtonElement>(".modal__keep").addEventListener("click", () => done("keep_disk"));
    overlay.addEventListener("mousedown", (e) => {
      if (e.target === overlay) done("keep_disk"); // click outside = the safe choice
    });
    document.addEventListener("keydown", onKey, true);

    document.body.appendChild(overlay);
    $<HTMLButtonElement>(".modal__overwrite").focus(); // user invoked Save → overwrite is primary
  });
}
