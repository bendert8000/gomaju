import { invoke } from "@tauri-apps/api/core";
import type { RuleDto } from "./rule-editor";

// "Quick select today's breaks" dashboard: each rule is a big tappable card. Details are
// read-only (edit them in Settings); only On/Off (tap the card) and Repeat/Once (segmented
// control) are editable here, and every change auto-saves via cmd_set_rule_flags.

const $ = <T extends HTMLElement>(id: string): T => document.getElementById(id) as T;

function humanEvery(intervalSecs: number): string {
  return `every ${Math.max(1, Math.round(intervalSecs / 60))} min`;
}
function humanBreak(breakSecs: number): string {
  return breakSecs >= 60 && breakSecs % 60 === 0
    ? `${breakSecs / 60} min break`
    : `${breakSecs}s break`;
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
  (item.querySelector(".rule-card__state") as HTMLElement).textContent = on ? "ON" : "OFF";
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
    showMsg(`Couldn't update: ${err}`, true);
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
        <span class="rule-card__state">${rule.enabled ? "ON" : "OFF"}</span>
      </span>
      <span class="rule-card__body">
        <span class="rule-card__name"></span>
        <span class="rule-card__meta"></span>
      </span>
      <span class="rule-card__badge"></span>
    </button>
    <div class="rule-repeat" role="group" aria-label="Repeat mode">
      <button class="seg" type="button" data-val="repeat" aria-pressed="${rule.repeat}">⟳ Repeat</button>
      <button class="seg" type="button" data-val="once" aria-pressed="${!rule.repeat}">1× Once</button>
    </div>
  `;
  (item.querySelector(".rule-card__name") as HTMLElement).textContent = rule.name;
  (item.querySelector(".rule-card__meta") as HTMLElement).textContent =
    `${humanEvery(rule.interval_secs)} · ${humanBreak(rule.break_secs)}`;
  (item.querySelector(".rule-card__badge") as HTMLElement).textContent =
    rule.enforcement === "strict" ? "Strict" : "Soft";

  (item.querySelector(".rule-card") as HTMLButtonElement).addEventListener("click", () =>
    toggleOn(item),
  );
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
    empty.textContent = "No break rules yet — add some in Settings.";
    deck.appendChild(empty);
    return;
  }
  rules.forEach((rule, i) => deck.appendChild(card(rule, i)));
}

async function load(): Promise<void> {
  render(await invoke<RuleDto[]>("cmd_get_rules"));
}

async function init(): Promise<void> {
  invoke("cmd_window_ready", { label: "rules" }).catch(() => {});
  await load();
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
