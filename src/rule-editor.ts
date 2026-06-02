// Shared break-rule editor used by both the Settings window (src/main.ts) and the
// standalone Break-rules window (src/rules.ts) — single source of truth for the rule row.

import { t } from "./i18n";

export type Enforcement = "soft" | "strict";

export interface RuleDto {
  id: string;
  name: string;
  interval_secs: number;
  break_secs: number;
  enforcement: Enforcement;
  enabled: boolean;
  repeat: boolean;
}

const rowInput = (row: HTMLElement, cls: string): HTMLInputElement =>
  row.querySelector(cls) as HTMLInputElement;
const rowSelect = (row: HTMLElement, cls: string): HTMLSelectElement =>
  row.querySelector(cls) as HTMLSelectElement;

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
  };
}

/** Build one editable rule row. Scaffolding is static; user values are set via DOM
 * setters (never interpolated), so there's no XSS surface. */
export function ruleRow(rule: RuleDto): HTMLElement {
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
  `;
  rowInput(row, ".rule-name").value = rule.name;
  rowInput(row, ".rule-interval").value = String(Math.round(rule.interval_secs / 60));
  rowInput(row, ".rule-break").value = String(rule.break_secs);
  rowSelect(row, ".rule-enforcement").value = rule.enforcement;
  rowInput(row, ".rule-enabled").checked = rule.enabled;
  rowInput(row, ".rule-repeat").checked = rule.repeat;
  row.querySelector(".rule-remove")!.addEventListener("click", () => row.remove());
  return row;
}

/** Replace the contents of `container` with rows for `rules`. */
export function renderRules(container: HTMLElement, rules: RuleDto[]): void {
  container.innerHTML = "";
  for (const rule of rules) container.appendChild(ruleRow(rule));
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
    };
  });
}
