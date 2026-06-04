// Quote-list editor for the Settings "Quotes" card — one editable row per quote. Kept in its own
// module (like rule-editor.ts) so src/main.ts stays lean. Quotes persist to `quotes.txt` (separate
// from config.toml) via cmd_get_quotes / cmd_save_quotes; the backend sanitizes on save.

import { t } from "./i18n";

const rowInput = (row: HTMLElement): HTMLInputElement =>
  row.querySelector(".quote-text") as HTMLInputElement;

/** Build one editable quote row. Scaffolding is static; the quote text is set via `value` (never
 * interpolated), so there's no XSS surface — matching the rule editor. */
export function quoteRow(text: string): HTMLElement {
  const row = document.createElement("div");
  row.className = "quote-row";
  row.innerHTML = `
    <input class="quote-text" type="text" />
    <button class="quote-remove btn-ghost" type="button" title="${t("common.remove")}">✕</button>
  `;
  rowInput(row).value = text;
  row.querySelector(".quote-remove")!.addEventListener("click", () => row.remove());
  return row;
}

/** Replace the contents of `container` with rows for `quotes`. */
export function renderQuotes(container: HTMLElement, quotes: string[]): void {
  container.replaceChildren();
  for (const q of quotes) container.appendChild(quoteRow(q));
}

/** Read the quote rows inside `container` back into a string[] (as typed; the backend sanitizes). */
export function collectQuotes(container: HTMLElement): string[] {
  return Array.from(container.querySelectorAll<HTMLElement>(".quote-row")).map(
    (row) => rowInput(row).value,
  );
}
