import { invoke } from "@tauri-apps/api/core";
import { applyI18n, t } from "./i18n";
import { fmtMMSS } from "./util";
import type { RuleDto } from "./rule-editor";
import { renderStatusBanner, type StatusDto } from "./status";

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
  (item.querySelector('.seg[data-val="repeat"]') as HTMLElement).setAttribute(
    "aria-pressed",
    String(repeat),
  );
  (item.querySelector('.seg[data-val="once"]') as HTMLElement).setAttribute(
    "aria-pressed",
    String(!repeat),
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
      </span>
      <span class="rule-card__badge"></span>
    </button>
    <div class="rule-card__foot">
      <span class="rule-card__countdown"></span>
      <button class="rule-card__reset" type="button">${t("card.reset")}</button>
    </div>
    <div class="rule-repeat" role="group" aria-label="Repeat mode">
      <button class="seg" type="button" data-val="repeat" aria-pressed="${rule.repeat}">${t("card.repeat")}</button>
      <button class="seg" type="button" data-val="once" aria-pressed="${!rule.repeat}">${t("card.once")}</button>
    </div>
  `;
  (item.querySelector(".rule-card__name") as HTMLElement).textContent = rule.name;
  (item.querySelector(".rule-card__meta") as HTMLElement).textContent =
    `${humanEvery(rule.interval_secs)} · ${humanBreak(rule.break_secs)}`;
  (item.querySelector(".rule-card__badge") as HTMLElement).textContent =
    rule.enforcement === "strict" ? t("card.strict") : t("card.soft");

  (item.querySelector(".rule-card") as HTMLButtonElement).addEventListener("click", () =>
    toggleOn(item),
  );
  (item.querySelector(".rule-card__reset") as HTMLButtonElement).addEventListener("click", () => {
    // Backend pops a Reset/Cancel confirm; the live poll reflects the reset.
    void invoke("cmd_reset_timer", { ruleId: rule.id }).catch((err) =>
      console.error("restee: reset failed", err),
    );
  });
  for (const seg of item.querySelectorAll<HTMLButtonElement>(".seg")) {
    seg.addEventListener("click", (e) => {
      e.stopPropagation();
      setRepeat(item, seg.dataset.val === "repeat");
    });
  }
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

async function load(): Promise<void> {
  render(await invoke<RuleDto[]>("cmd_get_rules"));
}

// Live status — banner lists all enabled breaks (shared phrasing with Settings), and
// each enabled card gets its own countdown.
async function refreshStatus(): Promise<void> {
  let s: StatusDto;
  try {
    s = await invoke<StatusDto>("cmd_get_status");
  } catch {
    return; // non-fatal
  }
  renderStatusBanner(s, $("status-text"));
  $("status-banner").dataset.state = s.state;

  // Per-card countdowns: only while actually running, and only on cards shown ON (so a
  // stale poll can't contradict an optimistic OFF toggle made before the engine reconfigures).
  const remaining = new Map(s.all.map((b) => [b.rule_id, b.remaining_secs]));
  for (const item of document.querySelectorAll<HTMLElement>(".rule-item")) {
    const secs = remaining.get(item.dataset.id ?? "");
    const show = s.state === "running" && item.dataset.on === "true" && secs != null;
    (item.querySelector(".rule-card__countdown") as HTMLElement).textContent = show
      ? t("card.next_in", { mmss: fmtMMSS(secs) })
      : "";
  }
}

async function init(): Promise<void> {
  document.title = t("title.rules");
  applyI18n(document.body);
  invoke("cmd_window_ready", { label: "rules" }).catch(() => {});
  await load();
  await refreshStatus();
  window.setInterval(refreshStatus, 1000);
  $("settings-btn").addEventListener("click", () => invoke("cmd_open_settings"));
  $("close-btn").addEventListener("click", () => invoke("cmd_close_rules"));
  // Re-sync from disk when returning to this window (e.g. after editing in Settings, or a
  // once-rule auto-disabling while we were away).
  window.addEventListener("focus", () => {
    load().catch(() => {});
  });
}

window.addEventListener("DOMContentLoaded", () => {
  init().catch((err) => console.error("restee rules init failed", err));
});
