import { invoke } from "@tauri-apps/api/core";
import { applyI18n, t } from "./i18n";
import { renderStatusBanner, type StatusDto } from "./status";
import { collectRules, defaultRule, renderRules, ruleRow, type RuleDto } from "./rule-editor";
import { installUnsavedGuard, type UnsavedGuard } from "./unsaved-guard";

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
// Assigned in init() once the form is first rendered; referenced only afterwards.
let guard!: UnsavedGuard;

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

// On focus, pick up rules edited in the standalone Break-rules window — but ONLY when this form
// has no unsaved edits, so the refresh never silently discards in-progress work. After a clean
// refresh, re-baseline so the freshly-pulled disk state doesn't read as a pending change.
async function onFocusRefresh(): Promise<void> {
  if (guard.isDirty()) return;
  await refreshRulesFromDisk();
  guard.markSaved();
}

async function save(): Promise<boolean> {
  const msg = $("save-msg");
  try {
    const outcome = await invoke<SaveOutcome>("cmd_save_config", { config: collectConfig() });
    current = outcome.config;
    render(current); // reflect any clamping the backend applied
    if (outcome.hotkey_errors.length) {
      msg.textContent = t("settings.save_hotkey_fail", {
        errors: outcome.hotkey_errors.join("; "),
      });
      msg.className = "warn";
    } else {
      msg.textContent = t("common.saved");
      msg.className = "ok";
    }
    guard.markSaved(); // config persisted (even with hotkey warnings) -> no longer dirty
    return true;
  } catch (err) {
    msg.textContent = t("settings.save_fail", { err: String(err) });
    msg.className = "warn";
    return false;
  }
}

// --- Live status banner (time to next break) ---

function renderStatus(s: StatusDto): void {
  renderStatusBanner(s, $("status-text"));
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
  document.title = t("title.settings");
  applyI18n(document.body);
  invoke("cmd_window_ready", { label: "settings" }).catch(() => {});
  current = await invoke<ConfigFile>("cmd_get_config");
  render(current);
  // Guard against closing with unsaved edits (Close button + OS window X). Installed after the
  // first render so the dirty baseline matches the loaded config.
  guard = installUnsavedGuard({
    collect: collectConfig,
    save,
    close: () => void invoke("cmd_close_settings"),
  });

  try {
    const status = await invoke<string>("cmd_get_idle_status");
    const badge = $("idle-status");
    badge.textContent = t("settings.idle_badge", { status });
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
    void onFocusRefresh();
  });
  $("save-btn").addEventListener("click", () => void save());
  $("close-btn").addEventListener("click", () => void guard.requestClose());
}

window.addEventListener("DOMContentLoaded", () => {
  init().catch((err) => console.error("restee settings init failed", err));
});
