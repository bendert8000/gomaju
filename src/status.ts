import { invoke } from "@tauri-apps/api/core";
import { t } from "./i18n";
import { fmtMMSS } from "./util";

// Shared "time to next break" status, surfaced in both the Settings banner and the
// Today's breaks dashboard. Mirrors the Rust StatusDto (commands::cmd_get_status).

export interface NextBreakDto {
  rule_id: string;
  rule_name: string;
  remaining_secs: number;
}

export interface StatusDto {
  state: "stopped" | "running" | "paused" | "in_break";
  /** Every enabled break, soonest-first. */
  all: NextBreakDto[];
}

/** Render the status banner into `textEl`: one row per enabled break (soonest first),
 *  each with its own Reset button (the backend confirms before resetting just that break),
 *  or a single text row for the special states. Rebuilt on each ~1s poll. */
export function renderStatusBanner(s: StatusDto, textEl: HTMLElement): void {
  if (s.state === "in_break") {
    textEl.textContent = t("status.on_break");
    return;
  }
  if (s.all.length === 0) {
    textEl.textContent = s.state === "paused" ? t("status.paused_no_rules") : t("status.no_rules");
    return;
  }
  const rows: HTMLElement[] = [];
  if (s.state === "paused") {
    const note = document.createElement("div");
    note.className = "status-row status-row--note";
    note.textContent = t("status.paused");
    rows.push(note);
  }
  for (const b of s.all) rows.push(breakRow(b));
  textEl.replaceChildren(...rows);
}

function breakRow(b: NextBreakDto): HTMLElement {
  const row = document.createElement("div");
  row.className = "status-row";

  const label = document.createElement("span");
  label.className = "status-row__label";
  label.textContent = `${b.rule_name} ${fmtMMSS(b.remaining_secs)}`;

  const reset = document.createElement("button");
  reset.type = "button";
  reset.className = "status-row__reset btn-ghost btn-sm";
  reset.textContent = t("card.reset");
  reset.title = t("status.restart_title", { name: b.rule_name });
  reset.addEventListener("click", () => {
    // Backend pops a Reset/Cancel confirm; the live poll reflects the reset.
    void invoke("cmd_reset_timer", { ruleId: b.rule_id }).catch((err) =>
      console.error("restee: reset failed", err),
    );
  });

  row.append(label, reset);
  return row;
}
