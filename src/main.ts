import { invoke } from "@tauri-apps/api/core";

// --- Types mirroring the Rust config DTOs ---

type Enforcement = "soft" | "strict";
type IdlePolicy = "pause" | "credit";
type EscapeMode = "friction" | "easy" | "no_easy_escape";

interface RuleDto {
  id: string;
  name: string;
  interval_secs: number;
  break_secs: number;
  enforcement: Enforcement;
  enabled: boolean;
}

interface SettingsDto {
  idle_policy: IdlePolicy;
  away_threshold_secs: number;
  gap_threshold_secs: number;
  escape_mode: EscapeMode;
  warn_seconds: number;
  sound: boolean;
  notifications: boolean;
}

interface HotkeysDto {
  toggle?: string | null;
  break_now?: string | null;
  skip?: string | null;
}

interface ConfigFile {
  version: number;
  autostart: boolean;
  settings: SettingsDto;
  hotkeys: HotkeysDto;
  rules: RuleDto[];
}

interface SaveOutcome {
  config: ConfigFile;
  hotkey_errors: string[];
}

const $ = <T extends HTMLElement>(id: string): T =>
  document.getElementById(id) as T;

// Typed accessors so call sites don't repeat `as HTMLInputElement` / `as HTMLSelectElement`.
const inp = (id: string): HTMLInputElement => $(id);
const sel = (id: string): HTMLSelectElement => $(id);
const rowInput = (row: HTMLElement, cls: string): HTMLInputElement =>
  row.querySelector(cls) as HTMLInputElement;
const rowSelect = (row: HTMLElement, cls: string): HTMLSelectElement =>
  row.querySelector(cls) as HTMLSelectElement;

// Read a non-negative integer from an input, keeping 0 (which `|| fallback` would drop).
function readNonNegative(id: string, fallback: number): number {
  const v = Number(inp(id).value);
  return Number.isFinite(v) && v >= 0 ? Math.round(v) : fallback;
}

let current: ConfigFile;

// --- Rule rows ---

function ruleRow(rule: RuleDto): HTMLElement {
  const row = document.createElement("div");
  row.className = "rule-row";
  row.dataset.id = rule.id;
  row.innerHTML = `
    <input class="rule-name" type="text" value="" />
    <input class="rule-interval" type="number" min="1" />
    <input class="rule-break" type="number" min="1" />
    <select class="rule-enforcement">
      <option value="soft">Soft</option>
      <option value="strict">Strict</option>
    </select>
    <input class="rule-enabled" type="checkbox" />
    <button class="rule-remove btn-ghost" type="button" title="Remove">✕</button>
  `;
  rowInput(row, ".rule-name").value = rule.name;
  rowInput(row, ".rule-interval").value = String(Math.round(rule.interval_secs / 60));
  rowInput(row, ".rule-break").value = String(rule.break_secs);
  rowSelect(row, ".rule-enforcement").value = rule.enforcement;
  rowInput(row, ".rule-enabled").checked = rule.enabled;
  row.querySelector(".rule-remove")!.addEventListener("click", () => row.remove());
  return row;
}

function renderRules(rules: RuleDto[]): void {
  const container = $("rules");
  container.innerHTML = "";
  for (const rule of rules) container.appendChild(ruleRow(rule));
}

function collectRules(): RuleDto[] {
  const rows = Array.from(document.querySelectorAll<HTMLElement>(".rule-row"));
  return rows.map((row) => {
    const minutes = Number(rowInput(row, ".rule-interval").value) || 1;
    const brk = Number(rowInput(row, ".rule-break").value) || 1;
    return {
      id: row.dataset.id || crypto.randomUUID(),
      name: rowInput(row, ".rule-name").value.trim() || "Break",
      interval_secs: Math.max(1, Math.round(minutes)) * 60,
      break_secs: Math.max(1, Math.round(brk)),
      enforcement: rowSelect(row, ".rule-enforcement").value as Enforcement,
      enabled: rowInput(row, ".rule-enabled").checked,
    };
  });
}

// --- Form <-> config ---

function render(cfg: ConfigFile): void {
  renderRules(cfg.rules);
  sel("idle-policy").value = cfg.settings.idle_policy;
  sel("escape-mode").value = cfg.settings.escape_mode;
  inp("warn-seconds").value = String(cfg.settings.warn_seconds);
  inp("away-threshold").value = String(cfg.settings.away_threshold_secs);
  inp("sound").checked = cfg.settings.sound;
  inp("notifications").checked = cfg.settings.notifications;
  inp("autostart").checked = cfg.autostart;
  inp("hk-toggle").value = cfg.hotkeys.toggle ?? "";
  inp("hk-break").value = cfg.hotkeys.break_now ?? "";
  inp("hk-skip").value = cfg.hotkeys.skip ?? "";
}

function blankToNull(v: string): string | null {
  const t = v.trim();
  return t.length ? t : null;
}

function collectConfig(): ConfigFile {
  return {
    ...current,
    autostart: inp("autostart").checked,
    settings: {
      ...current.settings,
      idle_policy: sel("idle-policy").value as IdlePolicy,
      escape_mode: sel("escape-mode").value as EscapeMode,
      warn_seconds: readNonNegative("warn-seconds", current.settings.warn_seconds),
      away_threshold_secs:
        Number(inp("away-threshold").value) || current.settings.away_threshold_secs,
      sound: inp("sound").checked,
      notifications: inp("notifications").checked,
    },
    hotkeys: {
      toggle: blankToNull(inp("hk-toggle").value),
      break_now: blankToNull(inp("hk-break").value),
      skip: blankToNull(inp("hk-skip").value),
    },
    rules: collectRules(),
  };
}

async function save(): Promise<void> {
  const msg = $("save-msg");
  try {
    const outcome = await invoke<SaveOutcome>("cmd_save_config", { config: collectConfig() });
    current = outcome.config;
    render(current); // reflect any clamping the backend applied
    if (outcome.hotkey_errors.length) {
      msg.textContent = `Saved, but some hotkeys failed: ${outcome.hotkey_errors.join("; ")}`;
      msg.className = "warn";
    } else {
      msg.textContent = "Saved ✓";
      msg.className = "ok";
    }
  } catch (err) {
    msg.textContent = `Save failed: ${err}`;
    msg.className = "warn";
  }
}

async function init(): Promise<void> {
  invoke("cmd_window_ready", { label: "settings" }).catch(() => {});
  current = await invoke<ConfigFile>("cmd_get_config");
  render(current);

  try {
    const status = await invoke<string>("cmd_get_idle_status");
    const badge = $("idle-status");
    badge.textContent = `idle: ${status}`;
    badge.dataset.status = status;
  } catch {
    /* non-fatal */
  }

  $("add-rule").addEventListener("click", () => {
    $("rules").appendChild(
      ruleRow({
        id: crypto.randomUUID(),
        name: "New break",
        interval_secs: 20 * 60,
        break_secs: 30,
        enforcement: "soft",
        enabled: true,
      }),
    );
  });
  $("save-btn").addEventListener("click", save);
  $("close-btn").addEventListener("click", () => invoke("cmd_close_settings"));
}

window.addEventListener("DOMContentLoaded", () => {
  init().catch((err) => console.error("restee settings init failed", err));
});
