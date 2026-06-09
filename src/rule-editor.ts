// Shared break-rule editor used by both the Settings window (src/main.ts) and the
// standalone Break-rules window (src/breaks.ts) — single source of truth for the rule row.

import { t } from "./i18n";
import { resetActivePreview, wirePreviewButton } from "./chime-preview";

export type Enforcement = "soft" | "strict";

export interface RuleDto {
  id: string;
  name: string;
  interval_secs: number;
  break_secs: number;
  enforcement: Enforcement;
  enabled: boolean;
  repeat: boolean;
  /** Optional note shown read-only under the break name on the overlay. */
  note?: string;
  /** Optional id of a saved chime to play at break start (empty = default tone). */
  chime_id?: string;
  /** Volume for the break-start chime assignment, 0..=100. */
  chime_volume_pct?: number;
  /** Optional id of a saved chime to play when the break ends (empty = default break-over tone). */
  end_chime_id?: string;
  /** Volume for the break-end chime assignment, 0..=100. */
  end_chime_volume_pct?: number;
}

/** Minimal shape of a saved chime, for populating chime-picker dropdowns. */
export interface ChimeOption {
  id: string;
  name: string;
}

/** Reserved chime-picker value meaning "play no sound" — distinct from "" (the built-in default
 * tone). Must stay in sync with `NONE_CHIME_ID` in `src-tauri/src/audio.rs`. */
export const NONE_CHIME = "__none__";

/** Populate a chime `<select>` with "Default" (value ""), "None" (silent), then one option per saved
 * chime, and select `selected`. Names are set via `textContent` (never interpolated), so a chime
 * name can't inject markup. Shared by the rules editor and the alarms window — so adding the "None"
 * option here covers every chime picker in the app. */
export function fillChimeSelect(
  sel: HTMLSelectElement,
  chimes: ChimeOption[],
  selected: string,
): void {
  sel.replaceChildren();
  const def = document.createElement("option");
  def.value = "";
  def.textContent = t("chime.default");
  sel.appendChild(def);
  const none = document.createElement("option");
  none.value = NONE_CHIME;
  none.textContent = t("chime.none");
  sel.appendChild(none);
  for (const c of chimes) {
    const opt = document.createElement("option");
    opt.value = c.id;
    opt.textContent = c.name;
    sel.appendChild(opt);
  }
  sel.value = selected;
}

const rowInput = (row: HTMLElement, cls: string): HTMLInputElement =>
  row.querySelector(cls) as HTMLInputElement;
const rowSelect = (row: HTMLElement, cls: string): HTMLSelectElement =>
  row.querySelector(cls) as HTMLSelectElement;

const DEFAULT_CHIME_VOLUME = 20;

function clampVolume(value: string | number | undefined): number {
  const n = Math.round(Number(value));
  return Number.isFinite(n) ? Math.min(100, Math.max(0, n)) : DEFAULT_CHIME_VOLUME;
}

/** A fresh rule for the "+ Add rule" button. */
export function defaultRule(): RuleDto {
  return {
    id: crypto.randomUUID(),
    name: t("editor.new_break"),
    interval_secs: 20 * 60,
    break_secs: 30,
    enforcement: "soft",
    enabled: true,
    repeat: true,
    note: "",
    chime_id: "",
    chime_volume_pct: DEFAULT_CHIME_VOLUME,
    end_chime_id: "",
    end_chime_volume_pct: DEFAULT_CHIME_VOLUME,
  };
}

/** Build one editable rule row. Scaffolding is static; user values are set via DOM
 * setters (never interpolated), so there's no XSS surface. `chimes` populates the chime picker. */
export function ruleRow(rule: RuleDto, chimes: ChimeOption[] = []): HTMLElement {
  const row = document.createElement("div");
  row.className = "rule-row";
  row.dataset.id = rule.id;
  row.innerHTML = `
    <input class="rule-name" type="text" value="" />
    <input class="rule-interval" type="number" min="1" />
    <input class="rule-break" type="number" min="1" />
    <select class="rule-enforcement">
      <option value="soft">${t("card.soft")}</option>
      <option value="strict">${t("card.strict")}</option>
    </select>
    <input class="rule-enabled" type="checkbox" />
    <input class="rule-repeat" type="checkbox" title="${t("editor.repeat_title")}" />
    <button class="rule-remove btn-ghost" type="button" title="${t("common.remove")}">✕</button>
    <textarea class="rule-note" rows="2" placeholder="${t("editor.note_placeholder")}"></textarea>
    <label class="rule-chime-row">${t("chime.start_label")} <select class="rule-chime"></select><span>${t("chimes.volume")}</span><input class="rule-chime-volume chime-volume-picker" type="number" min="0" max="100" /><button class="rule-chime-preview btn-ghost chime-preview-btn" type="button"></button></label>
    <label class="rule-chime-row">${t("chime.end_label")} <select class="rule-end-chime"></select><span>${t("chimes.volume")}</span><input class="rule-end-chime-volume chime-volume-picker" type="number" min="0" max="100" /><button class="rule-end-chime-preview btn-ghost chime-preview-btn" type="button"></button></label>
  `;
  rowInput(row, ".rule-name").value = rule.name;
  rowInput(row, ".rule-interval").value = String(Math.round(rule.interval_secs / 60));
  rowInput(row, ".rule-break").value = String(rule.break_secs);
  rowSelect(row, ".rule-enforcement").value = rule.enforcement;
  rowInput(row, ".rule-enabled").checked = rule.enabled;
  rowInput(row, ".rule-repeat").checked = rule.repeat;
  (row.querySelector(".rule-note") as HTMLTextAreaElement).value = rule.note ?? "";
  const startSel = rowSelect(row, ".rule-chime");
  const endSel = rowSelect(row, ".rule-end-chime");
  fillChimeSelect(startSel, chimes, rule.chime_id ?? "");
  fillChimeSelect(endSel, chimes, rule.end_chime_id ?? "");
  rowInput(row, ".rule-chime-volume").value = String(
    clampVolume(rule.chime_volume_pct ?? DEFAULT_CHIME_VOLUME),
  );
  rowInput(row, ".rule-end-chime-volume").value = String(
    clampVolume(rule.end_chime_volume_pct ?? DEFAULT_CHIME_VOLUME),
  );
  // ▶/⏸ preview after each picker; reads the select's current value at click time. "Default"
  // (empty) auditions the context's built-in tone (break-start / break-over).
  const rowBtn = (cls: string): HTMLButtonElement => row.querySelector(cls) as HTMLButtonElement;
  wirePreviewButton(
    rowBtn(".rule-chime-preview"),
    () => startSel.value,
    () => clampVolume(rowInput(row, ".rule-chime-volume").value),
    "break_start",
  );
  wirePreviewButton(
    rowBtn(".rule-end-chime-preview"),
    () => endSel.value,
    () => clampVolume(rowInput(row, ".rule-end-chime-volume").value),
    "break_over",
  );
  row.querySelector(".rule-remove")!.addEventListener("click", () => row.remove());
  return row;
}

/** Replace the contents of `container` with rows for `rules`. */
export function renderRules(
  container: HTMLElement,
  rules: RuleDto[],
  chimes: ChimeOption[] = [],
): void {
  resetActivePreview(); // rows (and their preview buttons) are about to be rebuilt
  container.innerHTML = "";
  for (const rule of rules) container.appendChild(ruleRow(rule, chimes));
}

/** Read the rule rows inside `container` back into `RuleDto[]`. */
export function collectRules(container: HTMLElement): RuleDto[] {
  const rows = Array.from(container.querySelectorAll<HTMLElement>(".rule-row"));
  return rows.map((row) => {
    const minutes = Number(rowInput(row, ".rule-interval").value) || 1;
    const brk = Number(rowInput(row, ".rule-break").value) || 1;
    return {
      id: row.dataset.id || crypto.randomUUID(),
      name: rowInput(row, ".rule-name").value.trim() || t("editor.fallback_break"),
      interval_secs: Math.max(1, Math.round(minutes)) * 60,
      break_secs: Math.max(1, Math.round(brk)),
      enforcement: rowSelect(row, ".rule-enforcement").value as Enforcement,
      enabled: rowInput(row, ".rule-enabled").checked,
      repeat: rowInput(row, ".rule-repeat").checked,
      note: (row.querySelector(".rule-note") as HTMLTextAreaElement).value.trim(),
      chime_id: rowSelect(row, ".rule-chime").value,
      chime_volume_pct: clampVolume(rowInput(row, ".rule-chime-volume").value),
      end_chime_id: rowSelect(row, ".rule-end-chime").value,
      end_chime_volume_pct: clampVolume(rowInput(row, ".rule-end-chime-volume").value),
    };
  });
}
