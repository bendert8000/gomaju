import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { applyI18n, t } from "./i18n";
import {
  DURATIONS,
  durationSymbol,
  durationTier,
  midiToName,
  notesToSteps,
  SOLFEGE,
  solfegeToMidi,
  stepsToNotes,
  type MajorKey,
  type Note,
  type ToneStep,
} from "./notes";
import { installUnsavedGuard, type UnsavedGuard } from "./unsaved-guard";

// Assigned in init() once the chimes are first rendered; referenced only afterwards.
let guard!: UnsavedGuard;

// --- Types mirroring restee_core::chime DTOs (ToneStep is imported from ./notes) ---

type ChimeKind = "tones" | "file";

interface ChimeDto {
  id: string;
  name: string;
  kind: ChimeKind;
  /** Volume of the whole chime (its tones or imported file), 0..=100. */
  volume_pct: number;
  // The backend omits these when empty (serde `skip_serializing_if`): an incoming **file** chime
  // has no `steps`, an incoming **tones** chime has no `file` — so both arrive as `undefined`.
  // Normalize on read (see `chimeCard`); `collectChime` always sets them when sending back.
  file?: string;
  steps?: ToneStep[];
}

const $ = <T extends HTMLElement>(id: string): T => document.getElementById(id) as T;
const q = <T extends HTMLElement>(root: HTMLElement, selector: string): T =>
  root.querySelector(selector) as T;

function clampInt(value: string, lo: number, hi: number, fallback: number): number {
  const n = Math.round(Number(value));
  return Number.isFinite(n) ? Math.min(hi, Math.max(lo, n)) : fallback;
}

// --- Melody (note chips) ---

/** A pleasant default so a new (or just-switched-to-tones) chime previews immediately: C-E-G in
 * octave 4 (the new-chime baseline). */
const DEFAULT_MELODY: Note[] = [
  { midi: 60, durationMs: 300 }, // C4
  { midi: 64, durationMs: 300 }, // E4
  { midi: 67, durationMs: 300 }, // G4
];

/** One melody note as a removable chip. The note lives in `data-midi` (empty = rest) + `data-dur`
 * (ms) — semantic state on the DOM, never pixel coordinates. */
function noteChip(note: Note): HTMLElement {
  const chip = document.createElement("span");
  chip.className = "melody-chip";
  chip.draggable = true; // drag to reorder within the melody strip
  chip.dataset.midi = note.midi == null ? "" : String(note.midi);
  chip.dataset.dur = String(note.durationMs);
  chip.dataset.len = durationTier(note.durationMs); // drives the per-length badge color (CSS)
  // Chip width grows with the note's duration, so the melody shows its rhythm at a glance. A log
  // scale (each length-doubling adds ~1.2rem) keeps even long notes a sensible width; capped at 8rem.
  const widthRem = Math.min(8, 3.2 + Math.max(0, Math.log2(note.durationMs / 75)) * 1.2);
  chip.style.minWidth = `${widthRem.toFixed(2)}rem`;
  const label = document.createElement("span");
  const name = note.midi == null ? t("chimes.rest") : midiToName(note.midi);
  label.textContent = `${name} ${durationSymbol(note.durationMs)}`;
  const rm = document.createElement("button");
  rm.type = "button";
  rm.className = "melody-chip-x";
  rm.textContent = "✕";
  rm.addEventListener("click", () => chip.remove());
  // Drag-to-reorder: the strip's dragover handler moves this chip live; we just flag it. setData
  // keeps the drag valid across engines; the ✕ button is not draggable so it still clicks.
  chip.addEventListener("dragstart", (e) => {
    chip.classList.add("dragging");
    e.dataTransfer?.setData("text/plain", "");
  });
  chip.addEventListener("dragend", () => chip.classList.remove("dragging"));
  chip.append(label, rm);
  return chip;
}

/** The chip the dragged one should be inserted *before* for a given cursor x (null = append at end).
 * Picks the not-dragging chip whose horizontal midpoint is just right of the cursor. */
function dragAfterChip(strip: HTMLElement, x: number): HTMLElement | null {
  let closest: { offset: number; el: HTMLElement | null } = {
    offset: Number.NEGATIVE_INFINITY,
    el: null,
  };
  for (const chip of strip.querySelectorAll<HTMLElement>(".melody-chip:not(.dragging)")) {
    const box = chip.getBoundingClientRect();
    const offset = x - box.left - box.width / 2;
    if (offset < 0 && offset > closest.offset) closest = { offset, el: chip };
  }
  return closest.el;
}

/** Read a card's melody back from its chips, in order. */
function collectMelody(card: HTMLElement): Note[] {
  return Array.from(card.querySelectorAll<HTMLElement>(".melody-chip")).map((chip) => ({
    midi: chip.dataset.midi ? Number(chip.dataset.midi) : null,
    durationMs: Number(chip.dataset.dur) || 300,
  }));
}

// --- Chime cards ---

function chimeCard(c: ChimeDto): HTMLElement {
  const card = document.createElement("div");
  card.className = "chime-card";
  card.dataset.id = c.id;
  card.dataset.kind = c.kind;
  card.dataset.file = c.file ?? "";
  card.innerHTML = `
    <div class="chime-head">
      <input class="chime-name" type="text" placeholder="${t("chimes.name_ph")}" />
      <div class="chime-kind">
        <button class="chime-kind-tones btn-ghost" type="button" data-kind="tones">${t("chimes.kind_tones")}</button>
        <button class="chime-kind-file btn-ghost" type="button" data-kind="file">${t("chimes.kind_file")}</button>
      </div>
      <button class="chime-preview btn-ghost" type="button">▶ ${t("chimes.preview")}</button>
      <button class="chime-remove btn-ghost" type="button" title="${t("common.remove")}">✕</button>
    </div>
    <label class="chime-volume-row">${t("chimes.volume")} <input class="chime-volume" type="number" min="0" max="100" /></label>
    <div class="chime-tones">
      <div class="chime-controls">
        <label>${t("chimes.key")} <select class="chime-key"><option value="C">C</option><option value="G">G</option><option value="F">F</option></select></label>
        <label>${t("chimes.octave")} <select class="chime-octave"><option>3</option><option>4</option><option>5</option><option>6</option></select></label>
        <label>${t("chimes.length")} <select class="chime-length">${DURATIONS.map((d) => `<option value="${d.id}">${d.symbol}(${d.fraction})</option>`).join("")}</select></label>
      </div>
      <div class="note-palette">
        ${SOLFEGE.map((s, i) => `<button class="note-btn btn-ghost" type="button" data-degree="${i + 1}">${s}</button>`).join("")}
        <button class="note-btn btn-ghost" type="button" data-degree="8">Do+</button>
        <button class="note-btn btn-ghost note-rest" type="button" data-rest="1">${t("chimes.rest")}</button>
      </div>
      <div class="melody-row">
        <span class="melody-label">${t("chimes.melody")}</span>
        <div class="melody-strip"></div>
        <button class="chime-clear btn-ghost" type="button">${t("chimes.clear")}</button>
      </div>
    </div>
    <div class="chime-file">
      <span class="chime-file-name"></span>
      <button class="chime-import btn-ghost" type="button">${t("chimes.import")}</button>
    </div>
  `;
  q<HTMLInputElement>(card, ".chime-name").value = c.name;

  // Controls + melody. Default the picker to C major / octave 4 / quarter note / volume 20; seed the
  // strip from the saved steps, or a C-E-G (octave 4) default when there are none (new chime, or a
  // file -> tones switch). `volumeFromSteps` returns 20 for an empty (new) chime, and the saved
  // volume for an existing one.
  q<HTMLSelectElement>(card, ".chime-key").value = "C";
  q<HTMLSelectElement>(card, ".chime-octave").value = "4";
  q<HTMLSelectElement>(card, ".chime-length").value = "quarter";
  q<HTMLInputElement>(card, ".chime-volume").value = String(c.volume_pct ?? 20);
  // A file chime arrives with `steps` omitted (undefined); treat it as an empty melody so the
  // strip setup below never calls `.length` on undefined.
  const steps = c.steps ?? [];

  const strip = q<HTMLElement>(card, ".melody-strip");
  for (const n of steps.length ? stepsToNotes(steps) : DEFAULT_MELODY) {
    strip.appendChild(noteChip(n));
  }
  // Drag-to-reorder: while a chip from THIS strip is being dragged, move it to the cursor position
  // live. Scoped to this strip (`.dragging` lives on the dragged chip), so a drag can't cross cards.
  strip.addEventListener("dragover", (e) => {
    const dragging = strip.querySelector<HTMLElement>(".melody-chip.dragging");
    if (!dragging) return; // not our drag (e.g. a chip from another card) — ignore
    e.preventDefault(); // allow the drop
    const after = dragAfterChip(strip, e.clientX);
    if (after === null) strip.appendChild(dragging);
    else strip.insertBefore(dragging, after);
  });

  const keySel = q<HTMLSelectElement>(card, ".chime-key");
  const octSel = q<HTMLSelectElement>(card, ".chime-octave");
  const lengthSel = q<HTMLSelectElement>(card, ".chime-length");
  const noteDur = (): number => DURATIONS.find((d) => d.id === lengthSel.value)?.ms ?? 300;
  for (const btn of card.querySelectorAll<HTMLElement>(".note-palette .note-btn")) {
    btn.addEventListener("click", () => {
      let midi: number | null = null;
      if (!btn.dataset.rest) {
        const degree = Number(btn.dataset.degree);
        const key = keySel.value as MajorKey;
        const octave = Number(octSel.value);
        // "Do+" (degree 8) is the tonic one octave up.
        midi = degree === 8 ? solfegeToMidi(key, 1, octave) + 12 : solfegeToMidi(key, degree, octave);
      }
      const note: Note = { midi, durationMs: noteDur() };
      strip.appendChild(noteChip(note));
      if (midi != null) void playNote(card, note); // immediate feedback; rests are silent
    });
  }
  q(card, ".chime-clear").addEventListener("click", () => strip.replaceChildren());

  const syncKind = (): void => {
    const kind = card.dataset.kind as ChimeKind;
    q<HTMLElement>(card, ".chime-tones").classList.toggle("show", kind === "tones");
    q<HTMLElement>(card, ".chime-file").classList.toggle("show", kind === "file");
    for (const btn of card.querySelectorAll<HTMLElement>(".chime-kind button")) {
      btn.classList.toggle("active", btn.dataset.kind === kind);
    }
    const fileName = card.dataset.file || "";
    q<HTMLElement>(card, ".chime-file-name").textContent = fileName || t("chimes.no_file");
  };
  syncKind();

  for (const btn of card.querySelectorAll<HTMLElement>(".chime-kind button")) {
    btn.addEventListener("click", () => {
      card.dataset.kind = btn.dataset.kind as ChimeKind;
      syncKind();
    });
  }
  q(card, ".chime-remove").addEventListener("click", () => card.remove());
  q(card, ".chime-preview").addEventListener("click", () => void preview(card));
  q(card, ".chime-import").addEventListener("click", () => void importFile(card));
  return card;
}

function collectChime(card: HTMLElement): ChimeDto {
  const kind = (card.dataset.kind as ChimeKind) || "tones";
  const volume_pct = clampInt(q<HTMLInputElement>(card, ".chime-volume").value, 0, 100, 20);
  let steps: ToneStep[] = [];
  if (kind === "tones") {
    const melody = collectMelody(card);
    // A melody with no sounded note collects to empty steps, so the backend drops the chime
    // (its "empty tones chime" path) rather than saving a silent one.
    if (melody.some((n) => n.midi != null)) {
      steps = notesToSteps(melody);
    }
  }
  return {
    id: card.dataset.id || crypto.randomUUID(),
    name: q<HTMLInputElement>(card, ".chime-name").value.trim() || t("chimes.default_name"),
    kind,
    volume_pct,
    file: kind === "file" ? card.dataset.file || "" : "",
    steps,
  };
}

function renderChimes(chimes: ChimeDto[]): void {
  setPreviewIdle(); // cards are about to be rebuilt — drop the stale button reference
  const container = $("chimes");
  container.innerHTML = "";
  for (const c of chimes) container.appendChild(chimeCard(c));
}

function collectChimes(): ChimeDto[] {
  return Array.from(document.querySelectorAll<HTMLElement>(".chime-card")).map(collectChime);
}

// --- Preview + import ---

// One preview plays at a time. `previewGen` is the backend generation token of the playing preview
// (0 = none); `previewBtn` is the button currently showing "⏸ Pause". The backend emits
// `preview-ended` with the gen when playback finishes, so we revert only the matching button.
let previewGen = 0;
let previewBtn: HTMLButtonElement | null = null;

/** Revert the active preview button to "▶ Preview" and clear the playing state. */
function setPreviewIdle(): void {
  if (previewBtn) {
    previewBtn.textContent = `▶ ${t("chimes.preview")}`;
    previewBtn.classList.remove("playing");
    previewBtn = null;
  }
  previewGen = 0;
}

/** Toggle preview for a card: start it (button → ⏸ Pause), or stop it if this card is already
 * playing. Starting a different card supersedes the current one (the backend stops its audio). */
async function preview(card: HTMLElement): Promise<void> {
  const btn = q<HTMLButtonElement>(card, ".chime-preview");
  if (btn === previewBtn) {
    // This card is playing — the button is now ⏸ Pause, so stop it.
    setPreviewIdle();
    invoke("cmd_stop_preview").catch((err) => console.error("restee: stop preview failed", err));
    return;
  }
  setPreviewIdle(); // revert any other card's button; the backend stops its audio when we start
  try {
    const gen = await invoke<number>("cmd_preview_chime", { chime: collectChime(card) });
    if (!gen) return; // nothing to play (e.g. empty tones)
    previewGen = gen;
    previewBtn = btn;
    btn.textContent = `⏸ ${t("chimes.pause")}`;
    btn.classList.add("playing");
  } catch (err) {
    setPreviewIdle();
    console.error("restee: chime preview failed", err);
  }
}

/** Play a single just-added melody note for immediate audio feedback while composing. Reuses the
 * chime-preview path with a one-step `tones` chime, at the card's current volume. The caller skips
 * rests (`midi == null`), so the note here is always sounded. */
async function playNote(card: HTMLElement, note: Note): Promise<void> {
  setPreviewIdle(); // a note audition supersedes (stops) any running chime preview
  const vol = clampInt(q<HTMLInputElement>(card, ".chime-volume").value, 0, 100, 20);
  try {
    await invoke("cmd_preview_chime", {
      chime: {
        id: card.dataset.id || crypto.randomUUID(),
        name: "",
        kind: "tones",
        volume_pct: vol,
        file: "",
        steps: notesToSteps([note]),
      },
    });
  } catch (err) {
    console.error("restee: note preview failed", err);
  }
}

async function importFile(card: HTMLElement): Promise<void> {
  try {
    const id = card.dataset.id || crypto.randomUUID();
    card.dataset.id = id;
    const file = await invoke<string | null>("cmd_import_chime_file", { chimeId: id });
    if (!file) return; // cancelled
    card.dataset.kind = "file";
    card.dataset.file = file;
    q<HTMLElement>(card, ".chime-tones").classList.remove("show");
    q<HTMLElement>(card, ".chime-file").classList.add("show");
    q<HTMLElement>(card, ".chime-file-name").textContent = file;
    for (const btn of card.querySelectorAll<HTMLElement>(".chime-kind button")) {
      btn.classList.toggle("active", btn.dataset.kind === "file");
    }
  } catch (err) {
    const msg = $("save-msg");
    msg.textContent = t("settings.save_fail", { err: String(err) });
    msg.className = "warn";
  }
}

async function save(): Promise<boolean> {
  const msg = $("save-msg");
  try {
    // Backend sanitizes (clamps tones, drops empty/invalid chimes, prunes orphan files) and
    // echoes the normalized list back, so re-render to reflect it.
    const saved = await invoke<ChimeDto[]>("cmd_save_chimes", { chimes: collectChimes() });
    renderChimes(saved);
    msg.textContent = t("common.saved");
    msg.className = "ok";
    guard.markSaved();
    return true;
  } catch (err) {
    msg.textContent = t("settings.save_fail", { err: String(err) });
    msg.className = "warn";
    return false;
  }
}

async function init(): Promise<void> {
  document.title = t("title.chimes");
  applyI18n(document.body);
  invoke("cmd_window_ready", { label: "chimes" }).catch(() => {});
  // Revert the Preview button when its chime finishes playing (or is stopped by a newer preview).
  void listen<number>("preview-ended", (e) => {
    if (e.payload === previewGen) setPreviewIdle();
  });
  renderChimes(await invoke<ChimeDto[]>("cmd_get_chimes"));
  guard = installUnsavedGuard({
    collect: collectChimes,
    save,
    close: () => void invoke("cmd_close_chimes"),
  });

  $("add-chime").addEventListener("click", () => {
    $("chimes").appendChild(
      chimeCard({
        id: crypto.randomUUID(),
        name: t("chimes.new_name"),
        kind: "tones",
        volume_pct: 20,
        file: "",
        steps: [], // chimeCard seeds a C-E-G default melody when there are no steps
      }),
    );
  });
  $("open-folder").addEventListener("click", () => {
    invoke("cmd_open_chimes_folder").catch((err) =>
      console.error("restee: open chimes folder failed", err),
    );
  });
  $("save-btn").addEventListener("click", () => void save());
  $("close-btn").addEventListener("click", () => void guard.requestClose());
}

window.addEventListener("DOMContentLoaded", () => {
  init().catch((err) => console.error("restee chimes init failed", err));
});
