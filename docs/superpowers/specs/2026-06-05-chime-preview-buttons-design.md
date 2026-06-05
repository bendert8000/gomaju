# Chime preview play/pause buttons (Settings + Alarms)

**Date:** 2026-06-05
**Status:** reviewed by Codex (gpt-5.5, read-only) — NO P1 blockers; P2/P3 findings folded in
(unconditional supersede emit; side-effect-free preview module; `resetActivePreview()` before
re-render; `chimes.ts` left untouched).

## Goal

Add a `▶`/`⏸` play/pause button after every "choose chime" `<select>` in the **Settings**
window (the rule editor's Start-chime and End-chime pickers) and the **Alarms** window (each
alarm's chime picker). Clicking ▶ auditions the **currently selected** option; clicking ⏸ stops.
Only **one** preview ever plays at a time, across all windows.

## Background (current code)

- Chime preview infra lives in `src-tauri/src/audio.rs`: a single active preview behind a global
  `PREVIEW: Mutex<PreviewState>` with a generation token. `start_preview(app, fill) -> u64` and
  `stop_preview()` bump the gen and stop the old `Sink`; `finish_preview` emits `preview-ended`
  carrying the gen **only if it is still current**, so a superseded preview never reverts the wrong
  button. `app.emit("preview-ended", gen)` broadcasts to all windows.
- `preview_chime_spec(app, steps, vol)` / `preview_chime_file(app, path, vol)` are the stoppable
  preview entry points (tones / imported file).
- The three built-in tones are `play_chime` (break-start), `play_break_over` (break-end),
  `play_alarm` (alarm). Each is `play("...", |sink| { ... })` where `play<F: FnOnce(&Sink) + Send +
  'static>`. `start_preview` takes the **same** `FnOnce(&Sink)` bound, so the fill closures are
  reusable as previews unchanged.
- `play_assigned_or(chime_id, chimes, dir, default: fn())` resolves an id to a saved chime (tones →
  `play_chime_spec`, file → `play_chime_file`), else calls the `default` built-in tone. Used by
  `play_break_chime` / `play_break_over_chime` / `play_alarm_chime`.
- Commands (`src-tauri/src/commands.rs`): `cmd_preview_chime(app, state, window, chime: ChimeDto)`
  and `cmd_stop_preview(window)` are **`require_chimes`-gated** (chimes window only). `cmd_get_chimes`
  is readable from settings/alarms/chimes (`is_settings || is_alarms || is_chimes`).
- Frontend: `src/chimes.ts` has the canonical toggle UX — module-level `previewGen`/`previewBtn`,
  `setPreviewIdle()`, a single `listen("preview-ended", ...)` that reverts only the matching button,
  and a `preview(card)` toggle. `src/rule-editor.ts` `ruleRow()` builds each rule row incl. the
  `.rule-chime` (start) and `.rule-end-chime` (end) selects via `fillChimeSelect`. `src/alarms.ts`
  builds each alarm row's `.alarm-chime` select. `src/breaks.ts` imports only the `RuleDto` **type**
  from rule-editor — it does **not** call `ruleRow`, so a button added there won't appear in breaks.
- The chime pickers store a chime **id** (empty string = "Default" = the built-in tone). The selects
  are populated from `cmd_get_chimes`.

## Design

### Behavior

- Button after each chime `<select>`; re-reads the select's **current value at click time** (so
  changing the dropdown then pressing ▶ auditions the new pick).
- Selected saved chime → preview that chime (tones or file). "Default" (empty value) → preview the
  context's built-in tone: Start → break-start, End → break-over, Alarm → alarm.
- Single active preview, including across windows: starting a new preview (or a different button)
  supersedes the running one, stops its audio, and reverts the previous button to ▶.

### Backend — `audio.rs`

1. Lift the three inline tone closures into named fns: `fill_break_start(&Sink)`,
   `fill_break_over(&Sink)`, `fill_alarm(&Sink)`. `play_chime`/`play_break_over`/`play_alarm` keep
   working by passing the named fn to `play(...)`.
2. Add a small enum `DefaultTone { BreakStart, BreakOver, Alarm }` and:
   - `preview_default(app, tone: DefaultTone) -> u64` = `start_preview(app, fill_for(tone))`.
   - `preview_assigned_or(app, chime_id, chimes, dir, tone: DefaultTone) -> u64` — mirrors
     `play_assigned_or` but routes to `preview_chime_spec` / `preview_chime_file`, falling back to
     `preview_default(app, tone)`. Returns the gen token (0 only if nothing plays).
3. **Single-active across windows:** in `start_preview`, capture the previous gen **before** the
   `p.gen += 1`, and after releasing the lock `emit("preview-ended", old_gen)` **unconditionally**
   when `old_gen != 0` — *not* gated on a sink being present. (Race, per Codex review: a preview
   registers its sink later, inside its spawned thread; a superseding start can run while `p.sink` is
   still `None`, so gating the emit on `p.sink.take()` being `Some` would leave the superseded
   window's button stuck on ⏸. Emitting the old gen unconditionally reverts it; gen-matching in JS
   means only that button reverts, and a stale/duplicate emit no-ops.) `stop_preview` does **not**
   need to emit: only the window currently showing ⏸ calls it, and it already reverts its own button
   locally before invoking. Update the `start_preview` doc comment (today a superseded preview emits
   nothing).

### Backend — `commands.rs` (+ register in `lib.rs`)

- `cmd_preview_chime_by_id(app, state, window, chime_id: String, kind: String) -> Result<u64, String>`:
  gate `is_settings || is_alarms` (mirror `cmd_get_chimes`'s allowlist). Map `kind`
  (`"break_start"|"break_over"|"alarm"`) → `DefaultTone`; reject unknown kinds. Resolve the chimes
  dir like `cmd_preview_chime` (`config_path.parent()/chimes`). Call
  `audio::preview_assigned_or(app, &chime_id, &chimes, &dir, tone)`. Return the gen.
- Broaden `cmd_stop_preview`'s gate to `is_settings || is_alarms || is_chimes`.
- Register `cmd_preview_chime_by_id` in the `invoke_handler!` in `lib.rs`.

### Frontend — shared helper `src/chime-preview.ts`

Factor the `chimes.ts` toggle pattern into one module owning the singleton `previewGen`/`previewBtn`,
`setPreviewIdle()`, and a single `preview-ended` listener. **The module has NO top-level side
effects** — it must not call `listen()` (or anything else) at import time, only inside the exported
init below. (Per Codex review: `breaks.ts` currently does `import type { RuleDto }` from
`rule-editor.ts`, which is erased at compile time, so rule-editor's runtime code — and any
chime-preview import it pulls in — never executes in the breaks window. Keeping the module
side-effect-free guarantees this stays true even if that import ever stops being type-only.) Export:

- `installPreviewEndedListener()` — installs the single `listen("preview-ended", ...)`; call once in
  each window's `init()` (Settings, Alarms only).
- `wirePreviewButton(btn: HTMLButtonElement, getChimeId: () => string, kind: PreviewKind)` — attaches
  the click toggle: if this btn is active → `cmd_stop_preview` + idle; else `resetActivePreview()`
  then `invoke("cmd_preview_chime_by_id", { chimeId: getChimeId(), kind })`, set this btn active on a
  truthy gen.
- `resetActivePreview()` — revert the active button (`setPreviewIdle()`) **and** best-effort
  `invoke("cmd_stop_preview")` so a row rebuild also halts audio. Call it before any re-render that
  rebuilds preview-bearing rows (mirrors `chimes.ts` `renderChimes` → `setPreviewIdle()`), avoiding a
  stale reference to a detached button.

Consumers:
- `rule-editor.ts` `ruleRow`: add a `.rule-chime-preview` button after `.rule-chime`
  (kind `break_start`) and a `.rule-end-chime-preview` after `.rule-end-chime` (kind `break_over`),
  wired with `getChimeId = () => select.value`.
- `alarms.ts`: add a `.alarm-chime-preview` button after `.alarm-chime` (kind `alarm`).
- Both windows call `installPreviewEndedListener()` in `init()`, and `resetActivePreview()` before
  `renderRules()` / `renderAlarms()`.
- `chimes.ts` is **left untouched** — it previews unsaved in-editor definitions via the existing
  `cmd_preview_chime` and keeps its own toggle state. (Per Codex review: do not retrofit it onto the
  by-id helper.)

### i18n / CSS

- Reuse existing `chimes.preview` / `chimes.pause` strings for the button label/title (add a neutral
  alias key if clearer). en + zh-Hant.
- Style with existing `.btn-ghost`; add a small rule so the button sits inline after the select.

### Tests (`commands.rs`)

- Gating predicate tests: `cmd_preview_chime_by_id` allowed from settings/alarms, denied from
  breaks/toast/overlay/chimes-or-not as decided; broadened `cmd_stop_preview` allowed from
  settings/alarms/chimes, denied elsewhere. Mirror existing `is_chimes`/`is_alarms` tests.
- Audio playback itself remains untested (as today).

## Scope / non-goals

- Breaks window unchanged. Chimes window keeps its existing def-based `cmd_preview_chime`.
- Code change (Rust + TS) → requires a rebuild to take effect.
