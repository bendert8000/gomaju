import { invoke } from "@tauri-apps/api/core";
import { fmtMMSS } from "./util";
import { collectRules, defaultRule, renderRules, ruleRow, type RuleDto } from "./rule-editor";

// --- Types mirroring the Rust config DTOs ---

type IdlePolicy = "pause" | "credit";
type EscapeMode = "friction" | "easy" | "no_easy_escape";

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
  // Preserved-but-not-edited here: kept (via the `{ ...current }` spread in
  // collectConfig) so a Settings save never drops alarms set in the Alarms window.
  alarms?: unknown[];
}

interface SaveOutcome {
  config: ConfigFile;
  hotkey_errors: string[];
}

interface StatusDto {
  state: "stopped" | "running" | "paused" | "in_break";
  next_rule: string | null;
  next_secs: number | null;
}

const $ = <T extends HTMLElement>(id: string): T =>
  document.getElementById(id) as T;

// Typed accessors so call sites don't repeat `as HTMLInputElement` / `as HTMLSelectElement`.
const inp = (id: string): HTMLInputElement => $(id);
const sel = (id: string): HTMLSelectElement => $(id);

// Read a non-negative integer from an input, keeping 0 (which `|| fallback` would drop).
function readNonNegative(id: string, fallback: number): number {
  const v = Number(inp(id).value);
  return Number.isFinite(v) && v >= 0 ? Math.round(v) : fallback;
}

let current: ConfigFile;

// --- Form <-> config ---

function render(cfg: ConfigFile): void {
  renderRules($("rules"), cfg.rules);
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
    rules: collectRules($("rules")),
  };
}

// Re-sync the rules grid from disk (e.g. after the standalone Break-rules window saved
// changes while this window was in the background). Only the rules grid is re-rendered,
// so in-progress behavior/hotkey edits are left intact. Updating `current` also keeps
// preserved fields (notably `alarms`) fresh for the next save.
async function refreshRulesFromDisk(): Promise<void> {
  try {
    const fresh = await invoke<ConfigFile>("cmd_get_config");
    current = fresh;
    renderRules($("rules"), fresh.rules);
  } catch {
    /* non-fatal */
  }
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

// --- Live status banner (time to next break) ---

function renderStatus(s: StatusDto): void {
  let text: string;
  if (s.state === "in_break") {
    text = "On a break now";
  } else if (s.next_secs == null) {
    text = s.state === "paused" ? "Paused — no enabled rules" : "No enabled rules";
  } else {
    const detail = `${fmtMMSS(s.next_secs)}${s.next_rule ? ` · ${s.next_rule}` : ""}`;
    text = s.state === "paused" ? `Paused — next break in ${detail}` : `Next break in ${detail}`;
  }
  $("status-text").textContent = text;
  $("status-banner").dataset.state = s.state;
}

async function refreshStatus(): Promise<void> {
  try {
    renderStatus(await invoke<StatusDto>("cmd_get_status"));
  } catch {
    /* non-fatal */
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

  // Live "time to next break" banner — poll while the settings window is open.
  await refreshStatus();
  window.setInterval(refreshStatus, 1000);

  $("add-rule").addEventListener("click", () => {
    $("rules").appendChild(ruleRow(defaultRule()));
  });
  // Keep the rules grid in sync with edits made in the standalone Break-rules window.
  window.addEventListener("focus", () => {
    refreshRulesFromDisk();
  });
  $("reset-btn").addEventListener("click", async () => {
    try {
      await invoke("cmd_reset_timers");
      await refreshStatus(); // reflect the restarted countdown immediately
    } catch (err) {
      console.error("restee: reset failed", err);
    }
  });
  $("save-btn").addEventListener("click", save);
  $("close-btn").addEventListener("click", () => invoke("cmd_close_settings"));
}

window.addEventListener("DOMContentLoaded", () => {
  init().catch((err) => console.error("restee settings init failed", err));
});
