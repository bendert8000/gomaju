import { invoke } from "@tauri-apps/api/core";
import { applyI18n, LOCALE, t } from "./i18n";
import {
  collectRules,
  defaultRule,
  renderRules,
  ruleRow,
  type ChimeOption,
  type RuleDto,
} from "./rule-editor";
import { installUnsavedGuard, type UnsavedGuard } from "./unsaved-guard";
import { installLocaleReload } from "./locale-reload";
import { collectQuotes, quoteRow, renderQuotes } from "./quotes-editor";
import { confirmQuotesConflict } from "./confirm-save";
import { installPreviewEndedListener } from "./chime-preview";

// --- Types mirroring the Rust config DTOs ---

type IdlePolicy = "pause" | "credit";
type EscapeMode = "friction" | "easy" | "no_easy_escape";
type BreakDisplay = "countdown" | "progress_bar";

interface SettingsDto {
  idle_policy: IdlePolicy;
  away_threshold_secs: number;
  gap_threshold_secs: number;
  escape_mode: EscapeMode;
  warn_seconds: number;
  sound: boolean;
  notifications: boolean;
  break_display: BreakDisplay;
  show_quotes: boolean;
  pause_reminder_enabled: boolean;
  pause_reminder_interval_secs: number;
  resume_prompt_enabled: boolean;
  show_timer_toasts: boolean;
  timer_count_up: boolean;
  timer_toast_progress: boolean;
  stopwatch_beep_enabled: boolean;
  stopwatch_beep_volume_pct: number;
}

interface HotkeysDto {
  toggle?: string | null;
  break_now?: string | null;
  skip?: string | null;
}

interface ConfigFile {
  version: number;
  locale: string;
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
// Saved chimes (from chimes.toml, separate from config), for the per-rule chime pickers.
// Refreshed on load + on focus so chimes created in the Chimes window show up here.
let chimes: ChimeOption[] = [];
// Break quotes are per-locale, stored in quotes.toml (separate from config.toml). The editor shows
// one locale's rows at a time (`activeQuoteLocale`); the other locales' edits live in
// `quotesByLocale`. `quotesBaselineByLocale` is each set as last synced from disk (after load, a
// clean focus-refresh, or a successful save) — `saveQuotes()` compares disk against it per locale
// to catch an external edit before overwriting.
const QUOTE_LOCALES = ["en", "zh-Hant"] as const;
type QuoteLocale = (typeof QUOTE_LOCALES)[number];
const emptySets = (): Record<QuoteLocale, string[]> => ({ en: [], "zh-Hant": [] });
let quotesByLocale: Record<QuoteLocale, string[]> = emptySets();
let quotesBaselineByLocale: Record<QuoteLocale, string[]> = emptySets();
let activeQuoteLocale: QuoteLocale = LOCALE === "en" ? "en" : "zh-Hant";

async function loadChimes(): Promise<void> {
  try {
    chimes = await invoke<ChimeOption[]>("cmd_get_chimes");
  } catch {
    chimes = []; // non-fatal: the picker just shows "Default"
  }
}

// Pull every locale's quotes from disk into the per-locale map + baseline (does not touch the DOM).
async function loadAllQuotes(): Promise<void> {
  for (const loc of QUOTE_LOCALES) {
    let list: string[];
    try {
      list = await invoke<string[]>("cmd_get_quotes", { locale: loc });
    } catch {
      list = []; // non-fatal: that locale just shows no rows
    }
    quotesByLocale[loc] = list;
    quotesBaselineByLocale[loc] = list;
  }
}

// The full per-locale quote state for dirty-tracking: the stored map, but with the visible locale
// taken live from the DOM (its rows are the source of truth while it's shown).
function quotesSnapshot(): Record<QuoteLocale, string[]> {
  const snap = emptySets();
  for (const loc of QUOTE_LOCALES) {
    snap[loc] = loc === activeQuoteLocale ? collectQuotes($("quotes")) : quotesByLocale[loc];
  }
  return snap;
}

// Highlight the active locale button in the toggle.
function updateLocaleToggleUI(): void {
  document.querySelectorAll<HTMLElement>(".quote-locale-btn").forEach((b) => {
    b.classList.toggle("is-active", b.dataset.locale === activeQuoteLocale);
  });
}

// Switch which locale's rows are shown: capture the current rows back into the map first, so edits
// to the locale being left are not lost.
function switchQuoteLocale(loc: QuoteLocale): void {
  if (loc === activeQuoteLocale) return;
  quotesByLocale[activeQuoteLocale] = collectQuotes($("quotes"));
  activeQuoteLocale = loc;
  renderQuotes($("quotes"), quotesByLocale[loc]);
  updateLocaleToggleUI();
}

// --- Form <-> config ---

function render(cfg: ConfigFile): void {
  renderRules($("rules"), cfg.rules, chimes);
  sel("idle-policy").value = cfg.settings.idle_policy;
  sel("escape-mode").value = cfg.settings.escape_mode;
  sel("break-display").value = cfg.settings.break_display;
  inp("warn-seconds").value = String(cfg.settings.warn_seconds);
  inp("away-threshold").value = String(cfg.settings.away_threshold_secs);
  inp("sound").checked = cfg.settings.sound;
  inp("show-quotes").checked = cfg.settings.show_quotes;
  inp("pause-reminder-enabled").checked = cfg.settings.pause_reminder_enabled;
  inp("pause-reminder-minutes").value = String(
    Math.max(1, Math.round(cfg.settings.pause_reminder_interval_secs / 60)),
  );
  inp("resume-prompt-enabled").checked = cfg.settings.resume_prompt_enabled;
  inp("notifications").checked = cfg.settings.notifications;
  inp("show-timer-toasts").checked = cfg.settings.show_timer_toasts;
  sel("timer-mode").value = cfg.settings.timer_count_up ? "countup" : "countdown";
  inp("timer-toast-progress").checked = cfg.settings.timer_toast_progress;
  inp("stopwatch-beep-enabled").checked = cfg.settings.stopwatch_beep_enabled;
  inp("stopwatch-beep-volume").value = String(cfg.settings.stopwatch_beep_volume_pct);
  inp("autostart").checked = cfg.autostart;
  inp("hk-toggle").value = cfg.hotkeys.toggle ?? "";
  inp("hk-break").value = cfg.hotkeys.break_now ?? "";
  inp("hk-skip").value = cfg.hotkeys.skip ?? "";
  sel("app-locale").value = cfg.locale;
}

// True once the user has picked a different language in THIS window's dropdown (vs. the locale the
// window was built with). Gates the Save-time window reload, so an unrelated Save — or a Save after
// the language was changed elsewhere (e.g. the tray) — never reopens every window.
let localeReloadPending = false;

/** Switch the whole-app UI language (the Language card). Persists + relabels the tray immediately;
 * open windows re-render in the new language only when the user Saves (see `saveFromButton`). */
async function setAppLocale(code: string): Promise<void> {
  if (!code || code === current.locale) return;
  try {
    await invoke("cmd_set_locale", { locale: code });
    current.locale = code; // keep the in-memory config in sync so a later Save preserves it
    sel("app-locale").value = code; // reflect the applied locale in the dropdown
    localeReloadPending = code !== LOCALE; // back to the build-time locale ⇒ nothing to reload
  } catch (err) {
    console.error("gomaju: set locale failed", err);
    sel("app-locale").value = current.locale; // failed: restore the dropdown to the live locale
  }
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
      break_display: sel("break-display").value as BreakDisplay,
      warn_seconds: readNonNegative("warn-seconds", current.settings.warn_seconds),
      away_threshold_secs:
        Number(inp("away-threshold").value) || current.settings.away_threshold_secs,
      sound: inp("sound").checked,
      show_quotes: inp("show-quotes").checked,
      pause_reminder_enabled: inp("pause-reminder-enabled").checked,
      pause_reminder_interval_secs:
        Math.max(1, readNonNegative("pause-reminder-minutes", 10)) * 60,
      resume_prompt_enabled: inp("resume-prompt-enabled").checked,
      notifications: inp("notifications").checked,
      show_timer_toasts: inp("show-timer-toasts").checked,
      timer_count_up: sel("timer-mode").value === "countup",
      timer_toast_progress: inp("timer-toast-progress").checked,
      stopwatch_beep_enabled: inp("stopwatch-beep-enabled").checked,
      stopwatch_beep_volume_pct: Math.min(
        100,
        readNonNegative("stopwatch-beep-volume", current.settings.stopwatch_beep_volume_pct),
      ),
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
    await loadChimes();
    renderRules($("rules"), fresh.rules, chimes);
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
  // Also re-sync every locale's quotes from disk, so an external quotes.toml edit made
  // while this (clean) window was backgrounded is shown and re-baselined — otherwise markSaved()
  // below would lock in the stale quote rows and a later save would silently overwrite that edit.
  await loadAllQuotes();
  renderQuotes($("quotes"), quotesByLocale[activeQuoteLocale]);
  guard.markSaved();
}

// Persist every locale's quote rows, guarding against an edit made to quotes.toml
// outside Gomaju since this editor last synced. The visible locale's rows are captured first. Each
// locale's disk is compared to its baseline; if any diverged, the user gets one prompt: overwrite
// (our lists win) or keep-disk (adopt the on-disk version for the *conflicted* locales, discarding
// those local edits — no write). Non-conflicted locales are written normally. A real read/write
// failure throws and is handled by save()'s catch. Leaves the map/baseline and the visible rows
// consistent with disk.
async function saveQuotes(): Promise<void> {
  quotesByLocale[activeQuoteLocale] = collectQuotes($("quotes"));

  const disk = emptySets();
  const conflicted: QuoteLocale[] = [];
  for (const loc of QUOTE_LOCALES) {
    disk[loc] = await invoke<string[]>("cmd_get_quotes", { locale: loc });
    if (JSON.stringify(disk[loc]) !== JSON.stringify(quotesBaselineByLocale[loc])) {
      conflicted.push(loc);
    }
  }
  const keepDisk = conflicted.length > 0 && (await confirmQuotesConflict()) === "keep_disk";

  for (const loc of QUOTE_LOCALES) {
    if (keepDisk && conflicted.includes(loc)) {
      quotesByLocale[loc] = disk[loc]; // keep the on-disk version; write nothing
      quotesBaselineByLocale[loc] = disk[loc];
      continue;
    }
    const saved = await invoke<string[]>("cmd_save_quotes", {
      locale: loc,
      quotes: quotesByLocale[loc],
    });
    quotesByLocale[loc] = saved;
    quotesBaselineByLocale[loc] = saved;
  }
  // Reflect sanitization / any adopted disk version in the visible locale's rows.
  renderQuotes($("quotes"), quotesByLocale[activeQuoteLocale]);
}

async function save(): Promise<boolean> {
  const msg = $("save-msg");
  try {
    // Quotes first: a quotes.toml write with no live side-effects, and the one that can
    // prompt (conflict guard). Then config, which reconfigures engine/hotkeys/autostart. Only
    // mark the form saved if BOTH succeed — any throw leaves the window dirty for a retry.
    await saveQuotes();
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

// The explicit Save button: persist, then — if the user changed the language in this window —
// ask every open window to recreate itself in the new locale (the locale is injected at window
// creation, so a rebuild is the only way an open window adopts a new language; each window runs
// its own unsaved-changes guard before reloading). Kept out of `save()` itself so the
// unsaved-guard's save-on-close path never triggers a reload.
async function saveFromButton(): Promise<void> {
  const ok = await save();
  if (ok && localeReloadPending) {
    localeReloadPending = false;
    invoke("cmd_reload_localized_windows").catch((err) =>
      console.error("gomaju: reload windows for locale failed", err),
    );
  }
}

async function init(): Promise<void> {
  document.title = t("title.settings");
  applyI18n(document.body);
  invoke("cmd_window_ready", { label: "settings" }).catch(() => {});
  invoke<string>("cmd_get_app_version")
    .then((version) => {
      $("app-version").textContent = `v${version}`;
    })
    .catch(() => {
      $("app-version").textContent = "";
    });
  installPreviewEndedListener(); // revert a chime-picker ▶/⏸ button when its preview ends
  current = await invoke<ConfigFile>("cmd_get_config");
  await loadChimes();
  render(current);
  // Break quotes live in per-locale files (separate from config); load every locale and render the
  // active one's rows before installing the guard so the dirty baseline includes all quote sets.
  await loadAllQuotes();
  renderQuotes($("quotes"), quotesByLocale[activeQuoteLocale]);
  updateLocaleToggleUI();
  // Guard against closing with unsaved edits (Close button + OS window X). Installed after the
  // first render so the dirty baseline matches the loaded config + quotes.
  guard = installUnsavedGuard({
    collect: () => ({ config: collectConfig(), quotes: quotesSnapshot() }),
    save,
    close: () => void invoke("cmd_close_settings"),
  });
  // Recreate this window in the new language when the user Saves a locale change (any window),
  // honoring unsaved edits via the guard.
  installLocaleReload(() => guard.confirmCanClose());

  try {
    const status = await invoke<string>("cmd_get_idle_status");
    const badge = $("idle-status");
    badge.textContent = t("settings.idle_badge", { status });
    badge.dataset.status = status;
  } catch {
    /* non-fatal */
  }

  $("add-rule").addEventListener("click", () => {
    $("rules").appendChild(ruleRow(defaultRule(), chimes));
  });
  $("add-quote").addEventListener("click", () => {
    $("quotes").appendChild(quoteRow(""));
  });
  document.querySelectorAll<HTMLElement>(".quote-locale-btn").forEach((btn) => {
    btn.addEventListener("click", () => switchQuoteLocale(btn.dataset.locale as QuoteLocale));
  });
  // Keep the rules grid in sync with edits made in the standalone Break-rules window.
  window.addEventListener("focus", () => {
    void onFocusRefresh();
  });
  $("open-chimes").addEventListener("click", () => {
    invoke("cmd_open_chimes").catch((err) => console.error("gomaju: open chimes failed", err));
  });
  sel("app-locale").addEventListener("change", () => void setAppLocale(sel("app-locale").value));
  $("save-btn").addEventListener("click", () => void saveFromButton());
  $("close-btn").addEventListener("click", () => void guard.requestClose());
}

window.addEventListener("DOMContentLoaded", () => {
  init().catch((err) => console.error("gomaju settings init failed", err));
});
