import { invoke } from "@tauri-apps/api/core";
import { applyI18n, t } from "./i18n";
import { fmtMMSS } from "./util";
import type { RuleDto } from "./rule-editor";
import { type StatusDto } from "./status";
import { installLocaleReload } from "./locale-reload";

// "Quick select today's breaks" dashboard: each rule is a big tappable card. Details are
// read-only (edit them in Settings); only On/Off (tap the card) and Repeat/Once (segmented
// control) are editable here, and every change auto-saves via cmd_set_rule_flags.

const $ = <T extends HTMLElement>(id: string): T => document.getElementById(id) as T;

function humanEvery(intervalSecs: number): string {
  return t("card.every", { n: Math.max(1, Math.round(intervalSecs / 60)) });
}
function humanBreak(breakSecs: number): string {
  return breakSecs >= 60 && breakSecs % 60 === 0
    ? t("card.break_min", { n: breakSecs / 60 })
    : t("card.break_sec", { n: breakSecs });
}

let msgTimer: number | undefined;
function showMsg(text: string, warn = false): void {
  const el = $("msg");
  el.textContent = text;
  el.className = warn ? "warn" : "muted";
  if (msgTimer) clearTimeout(msgTimer);
  msgTimer = window.setTimeout(() => {
    el.textContent = "";
    el.className = "muted";
  }, 4000);
}

function applyOn(item: HTMLElement, on: boolean): void {
  item.dataset.on = String(on);
  (item.querySelector(".rule-card") as HTMLElement).setAttribute("aria-pressed", String(on));
  (item.querySelector(".rule-card__state") as HTMLElement).textContent = on
    ? t("card.on")
    : t("card.off");
}
function applyRepeat(item: HTMLElement, repeat: boolean): void {
  item.dataset.repeat = String(repeat);
  (item.querySelector(".rule-repeat") as HTMLElement).setAttribute(
    "aria-pressed",
    String(repeat),
  );
}

/** Persist the current On/Repeat of `item`; revert the UI if the backend rejects. */
async function persist(item: HTMLElement, revert: () => void): Promise<void> {
  try {
    await invoke("cmd_set_rule_flags", {
      ruleId: item.dataset.id,
      enabled: item.dataset.on === "true",
      repeat: item.dataset.repeat === "true",
    });
  } catch (err) {
    revert();
    showMsg(t("rules.couldnt_update", { err: String(err) }), true);
  }
}

function toggleOn(item: HTMLElement): void {
  const prev = item.dataset.on === "true";
  applyOn(item, !prev); // optimistic
  void persist(item, () => applyOn(item, prev));
}
function setRepeat(item: HTMLElement, repeat: boolean): void {
  const prev = item.dataset.repeat === "true";
  if (prev === repeat) return;
  applyRepeat(item, repeat); // optimistic
  void persist(item, () => applyRepeat(item, prev));
}

function card(rule: RuleDto, index: number): HTMLElement {
  const accent = rule.enforcement === "strict" ? "#ff8c6a" : "#6aa6ff";
  const item = document.createElement("div");
  item.className = "rule-item";
  item.dataset.id = rule.id;
  item.dataset.on = String(rule.enabled);
  item.dataset.repeat = String(rule.repeat);
  item.style.setProperty("--accent", accent);
  item.style.setProperty("--i", String(index));
  // Static scaffolding only; user-supplied text is set via textContent below (no XSS).
  item.innerHTML = `
    <button class="rule-card" type="button" aria-pressed="${rule.enabled}">
      <span class="rule-card__status">
        <span class="rule-card__lamp" aria-hidden="true"></span>
        <span class="rule-card__state">${rule.enabled ? t("card.on") : t("card.off")}</span>
      </span>
      <span class="rule-card__body">
        <span class="rule-card__name"></span>
        <span class="rule-card__meta"></span>
        <span class="rule-card__note"></span>
      </span>
      <span class="rule-card__badge"></span>
    </button>
    <div class="rule-card__foot">
      <span class="rule-card__countdown"></span>
      <button class="rule-card__reset" type="button">${t("card.reset")}</button>
      <button class="rule-repeat" type="button" aria-pressed="${rule.repeat}" title="${t("card.repeat_title")}">
        <span class="rule-repeat__dot" aria-hidden="true"></span>
        <span class="rule-repeat__label">${t("card.repeat")}</span>
      </button>
    </div>
  `;
  (item.querySelector(".rule-card__name") as HTMLElement).textContent = rule.name;
  (item.querySelector(".rule-card__meta") as HTMLElement).textContent =
    `${humanEvery(rule.interval_secs)} · ${humanBreak(rule.break_secs)}`;
  (item.querySelector(".rule-card__badge") as HTMLElement).textContent =
    rule.enforcement === "strict" ? t("card.strict") : t("card.soft");
  const noteEl = item.querySelector(".rule-card__note") as HTMLElement;
  noteEl.textContent = rule.note ?? "";
  noteEl.hidden = !rule.note; // collapse when there's no note

  (item.querySelector(".rule-card") as HTMLButtonElement).addEventListener("click", () =>
    toggleOn(item),
  );
  (item.querySelector(".rule-card__reset") as HTMLButtonElement).addEventListener("click", () => {
    // Backend pops a Reset/Cancel confirm; the live poll reflects the reset.
    void invoke("cmd_reset_timer", { ruleId: rule.id }).catch((err) =>
      console.error("gomaju: reset failed", err),
    );
  });
  const repeatBtn = item.querySelector(".rule-repeat") as HTMLButtonElement;
  repeatBtn.addEventListener("click", (e) => {
    e.stopPropagation();
    setRepeat(item, item.dataset.repeat !== "true");
  });
  return item;
}

function render(rules: RuleDto[]): void {
  const deck = $("rules");
  deck.innerHTML = "";
  if (rules.length === 0) {
    const empty = document.createElement("p");
    empty.className = "muted";
    empty.textContent = t("rules.empty");
    deck.appendChild(empty);
    return;
  }
  rules.forEach((rule, i) => deck.appendChild(card(rule, i)));
}

async function getStatus(): Promise<StatusDto> {
  try {
    return await invoke<StatusDto>("cmd_get_status");
  } catch {
    return { state: "stopped", all: [] }; // non-fatal: fall back to config order
  }
}

// Sort key by next fire time: a rule's index in `status.all` (enabled, soonest-first), or
// +Infinity for rules with no pending fire (disabled / engine reports none) so they sort last.
function rankByNextFire(status: StatusDto): (id: string) => number {
  const idx = new Map(status.all.map((b, i) => [b.rule_id, i] as const));
  return (id) => idx.get(id) ?? Number.POSITIVE_INFINITY;
}

// Comparator: soonest next-fire first. Equal ranks (incl. two +Infinity disabled rules) compare 0
// and keep their current relative order — Array.sort is stable — which avoids both the
// `Infinity - Infinity = NaN` trap and an O(n) indexOf tiebreaker per comparison.
function byRank<T>(rank: (id: string) => number, idOf: (x: T) => string) {
  return (a: T, b: T): number => {
    const ra = rank(idOf(a));
    const rb = rank(idOf(b));
    return ra === rb ? 0 : ra - rb;
  };
}

// Per-card countdowns: only while actually running, and only on cards shown ON (so a stale poll
// can't contradict an optimistic OFF toggle made before the engine reconfigures).
function fillCountdowns(status: StatusDto): void {
  const remaining = new Map(status.all.map((b) => [b.rule_id, b.remaining_secs]));
  for (const item of document.querySelectorAll<HTMLElement>(".rule-item")) {
    const secs = remaining.get(item.dataset.id ?? "");
    const show = status.state === "running" && item.dataset.on === "true" && secs != null;
    (item.querySelector(".rule-card__countdown") as HTMLElement).textContent = show
      ? t("card.next_in", { mmss: fmtMMSS(secs) })
      : "";
  }
}

async function load(): Promise<void> {
  const [rules, status] = await Promise.all([invoke<RuleDto[]>("cmd_get_rules"), getStatus()]);
  // Soonest-first; rules with no pending fire (disabled) keep config order at the tail.
  render([...rules].sort(byRank(rankByNextFire(status), (r: RuleDto) => r.id)));
  fillCountdowns(status); // reuse the status we just fetched — no second round-trip
}

// Live poll (1s): refresh the countdown text and keep the deck sorted.
async function refreshStatus(): Promise<void> {
  let s: StatusDto;
  try {
    s = await invoke<StatusDto>("cmd_get_status");
  } catch {
    return; // non-fatal: keep the last-good countdowns and order
  }
  fillCountdowns(s);
  // Only re-sort live while running — when paused/stopped there's no visible countdown to explain
  // a move, and the cards should stay put.
  if (s.state === "running") reorderCards(s);
}

// Keep the deck sorted by next fire time as countdowns tick (a fired/reset break jumps to the
// tail). Touches the DOM only when the order actually changed, so a steady poll never churns it.
function reorderCards(status: StatusDto): void {
  const deck = $("rules");
  // Don't reorder while the user is interacting with a control in the deck — re-parenting a node
  // blurs a focused descendant and drops an in-flight click. The next tick re-checks once focus
  // leaves, so the order still catches up.
  if (deck.contains(document.activeElement)) return;
  const items = Array.from(deck.querySelectorAll<HTMLElement>(".rule-item"));
  if (items.length < 2) return;
  const ordered = [...items].sort(byRank(rankByNextFire(status), (el: HTMLElement) => el.dataset.id ?? ""));
  if (ordered.every((el, i) => el === items[i])) return; // already in order
  ordered.forEach((el) => deck.appendChild(el));
}

async function init(): Promise<void> {
  document.title = t("title.rules");
  applyI18n(document.body);
  invoke("cmd_window_ready", { label: "breaks" }).catch(() => {});
  await load(); // also fills the initial countdowns from the status it fetched
  window.setInterval(refreshStatus, 1000);
  // No unsaved edits here (each toggle auto-saves), so recreate freely on a Saved locale change.
  installLocaleReload();
  $("settings-btn").addEventListener("click", () => invoke("cmd_open_settings"));
  $("close-btn").addEventListener("click", () => invoke("cmd_close_breaks"));
  // Re-sync from disk when returning to this window (e.g. after editing in Settings, or a
  // once-rule auto-disabling while we were away).
  window.addEventListener("focus", () => {
    load().catch(() => {});
  });
}

window.addEventListener("DOMContentLoaded", () => {
  init().catch((err) => console.error("gomaju rules init failed", err));
});
