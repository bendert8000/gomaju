# Design: "Quotes" card in the Settings window

**Date:** 2026-06-04
**Status:** Implemented (2026-06-04). Design approved after 3 Codex review passes.

## Goal

Let users edit the break-screen quotes from inside the app. Today quotes are only editable
by hand-editing `quotes.txt` (next to `config.toml`); there is **no editor UI and no save
path** in code (`src-tauri/src/quotes.rs` has only `parse`/`load`/`pick`/`seed_if_missing`).

## Decisions (locked)

1. **A new Settings "Quotes" card** (not a tab bar). Settings is a single scrolling page of
   `.card` sections; we append one more, consistent with the current design.
2. **Add/remove row editor** (one text input + remove button per quote, like the
   Alarms/Chimes editors), not a raw textarea. Consequence: blank lines and `#` comments in
   `quotes.txt` are not represented and are dropped on the first save through this UI — the
   quotes themselves are untouched.
3. **Shared Save** — the existing Settings **Save** button persists quotes too (no auto-save).
4. **Minimal conflict guard** — on Save, if `quotes.txt` changed on disk since the editor last
   synced, prompt before overwriting (Overwrite vs. Keep on-disk). See "Conflict guard" below.

## Architecture

`quotes.txt` lives next to `config.toml` and is **separate from `config.toml`**. It is re-read
live on every break (`quotes::pick`), so there is **no in-memory cache to keep in sync** (unlike
chimes). Editing it therefore needs its own commands, independent of the config save.

### Frontend

- **`src/quotes-editor.ts` (new)** — mirrors the `rule-editor.ts` split so `main.ts` stays lean:
  - `quoteRow(text: string): HTMLElement` — a `.quote-row` with a `.quote-text` input and a
    `.quote-remove` button (reuses existing `.btn-ghost` styling; remove handler calls
    `row.remove()`). Text is set via `value`/`textContent` only (no interpolation → no XSS),
    matching `ruleRow`.
  - `renderQuotes(container, quotes: string[]): void` — replace children with one row per quote.
  - `collectQuotes(container): string[]` — return each row input's value (as typed; backend
    sanitizes). Used both for the save payload and the dirty snapshot.
- **`index.html`** — new `<section class="card">` titled **Quotes**, placed after the
  *Break behavior* card. Contains:
  - the existing `id="show-quotes"` checkbox **moved here** from the Behavior card (still read/
    written by id in `render()`/`collectConfig()`; the parent card is irrelevant — verified),
  - a `<div id="quotes">` rows container,
  - a `<button id="add-quote" class="btn-ghost">+ Add quote</button>`,
  - a `<p class="muted">` hint (see i18n `settings.quotes_hint`).
- **`src/styles.css`** — minimal `.quote-row` layout (input grows, remove button fixed), reusing
  existing color/spacing tokens. No new design language.
- **`src/confirm-save.ts`** — add `confirmQuotesConflict(): Promise<"overwrite" | "keep_disk">`,
  a 2-button modal reusing the existing `.modal-overlay`/`.modal` scaffold (primary = Overwrite,
  ghost = Keep on-disk version; Esc / overlay-click = keep_disk, the safe choice).

### `src/main.ts` wiring

- Module state: `let quotes: string[] = []` and `let quotesBaseline: string[] = []`
  (`quotesBaseline` = the list as last synced from disk: after load, after a clean focus-refresh,
  and after each successful save).
- `init()`: `quotes = await invoke("cmd_get_quotes"); quotesBaseline = quotes;
  renderQuotes($("quotes"), quotes);` and wire `#add-quote` → append `quoteRow("")`.
- **Dirty tracking** — the unsaved-guard `collect` becomes
  `() => ({ config: collectConfig(), quotes: collectQuotes($("quotes")) })`, so quote edits mark
  the window dirty and the close-confirm covers quotes.
- **`onFocusRefresh()`** (the not-dirty-only refresh) also reloads quotes:
  if `guard.isDirty()` return; else refresh **rules** (`cmd_get_config`) **and quotes**
  (`cmd_get_quotes`) → re-render both → set `quotesBaseline = <disk>` → `markSaved()`.
  This proactively syncs a *clean* window to external `quotes.txt` edits (so a later clean save
  never silently clobbers them, and never spuriously prompts).
- **`save()`** — collect config + quotes once, then:
  1. **Conflict check:** `const disk = await invoke("cmd_get_quotes")`. If
     `JSON.stringify(disk) !== JSON.stringify(quotesBaseline)` → external change since last sync:
     - `await confirmQuotesConflict()`:
       - `keep_disk` → adopt disk into the editor (`renderQuotes(disk)`, `quotesBaseline = disk`),
         and **skip** the quote write (disk already holds the desired content).
       - `overwrite` → proceed to write the collected quotes.
  2. **Write quotes first** (cheap file write, no live side effects), unless `keep_disk`:
     `const savedQ = await invoke("cmd_save_quotes", { quotes: collected }); quotes = savedQ;
     quotesBaseline = savedQ; renderQuotes(savedQ);`
  3. **Then config:** the existing `cmd_save_config` flow (engine/hotkeys/autostart + echoed
     sanitized config).
  4. `markSaved()` + success message **only if both steps succeed**. On any throw: show the error
     and `return false` (window stays dirty → user retries; quote writes are idempotent, so a
     retry after a config-only failure re-writes the same quotes harmlessly).

### Backend

- **`src-tauri/src/quotes.rs`**
  - `pub fn sanitize(quotes: &[String]) -> Vec<String>` — `trim` each; drop empty and
    `#`-leading lines (so save→reload round-trips identically, since `parse()` already strips
    those on load). Idempotent.
  - `pub fn save(config_dir: &Path, quotes: &[String]) -> std::io::Result<()>` — **atomic**,
    mirroring `config::save` (`crates/gomaju-core/src/config.rs:369`): `create_dir_all(config_dir)`,
    write `quotes.txt.tmp` in the same dir, `fs::rename` over `quotes.txt`. Writes the lines it is
    given (the command sanitizes first), joined by `\n` with a trailing newline (empty list →
    empty file). Never truncates the live file on a failed write.
  - `load`/`parse`/`pick`/`seed_if_missing` unchanged.
- **`src-tauri/src/commands.rs`** — two `require_settings`-gated commands (config dir =
  `state.config_path.parent().ok_or("no config dir")?`; no `AppState` change). To avoid the
  module-vs-parameter name clash, reference the module **fully qualified** as `crate::quotes::…`
  (the IPC arg key stays `quotes`):
  - `cmd_get_quotes(window, state) -> Result<Vec<String>, String>` → `crate::quotes::load(dir)`.
  - `cmd_save_quotes(window, state, quotes: Vec<String>) -> Result<Vec<String>, String>` →
    `let clean = crate::quotes::sanitize(&quotes);
    crate::quotes::save(dir, &clean).map_err(|e| e.to_string())?; Ok(clean)`
    (`quotes::save` returns `io::Result<()>`, so the `String`-error command must `.map_err` it —
    the same pattern as every other writer command; echoes the sanitized list so the form reflects
    any drops, like `cmd_save_config`).
- **`src-tauri/src/lib.rs`** — register `cmd_get_quotes` and `cmd_save_quotes` in
  `generate_handler!`.

### i18n

`src/i18n.ts` gains keys across the **same locale set the file already defines** (zh-Hant is the
default): `settings.quotes_heading`, `settings.add_quote`, `settings.quote_remove`,
`settings.quotes_hint`, `confirm.quotes_conflict_title`, `confirm.quotes_conflict_msg`,
`confirm.quotes_overwrite`, `confirm.quotes_keep_disk`. Hint text states plainly: *one quote per
line; one is shown at random each break; blank lines and lines starting with `#` are not kept.*

### Docs

Update the "Break quotes + pre-break toast" section of `CLAUDE.md`: quotes are now editable in the
Settings **Quotes** card (not only by hand), saved via the shared Save button, with the minimal
on-disk conflict guard. Note the row editor drops comments/blank lines.

## Conflict guard — why this closes the blocker

Codex's surviving blocker: with **unsaved** quote edits open (`isDirty()` true), `onFocusRefresh`
returns early and never reloads, so a simultaneous external edit to `quotes.txt` is silently
overwritten on Save. The guard closes it by **re-reading disk inside `save()`** and comparing to
`quotesBaseline`. Any external change (parsed-content difference) triggers an explicit
Overwrite/Keep-disk prompt — no silent clobber. The focus-refresh reload handles the *clean*-window
case proactively, so in practice the prompt only appears in the dirty + simultaneous-external-edit
case. There is a negligible read→write TOCTOU window, equivalent to and no worse than every other
file the app writes; this is a single-user local app.

## Edge cases

- **Empty list** → `save` writes an empty file → `pick` returns `None` → overlay shows no quote
  (existing behavior).
- **`#`-leading / blank inputs** → dropped by `sanitize`; the form re-renders from the echoed
  sanitized list so the user sees exactly what persisted.
- **Quote that looks like a comment** (`# foo`) → dropped (it would vanish on the next break's
  re-read otherwise); the hint warns of this.
- **Partial save** (quotes ok, config fails or vice-versa) → not `markSaved`; window stays dirty;
  retry is idempotent. No data loss.
- **keep_disk** discards the in-progress quote edits in favor of the on-disk version — explicit and
  user-chosen.

## Security

- Quote text is rendered via DOM setters only (no interpolation) → no XSS, matching `ruleRow`.
- Both commands are `require_settings`-gated (caller-label check, the app's real least-privilege
  boundary), identical to `cmd_get_config`/`cmd_save_config`.

## Testing & verification

- **Rust unit tests** (in `quotes.rs`, run via `cargo test`): `sanitize` trims and drops
  empty/`#`-leading; idempotency (`sanitize(sanitize(x)) == sanitize(x)`); a `save`→`load`
  round-trip in a temp dir returns the sanitized list.
- **Manual** (no TS test harness): `npm run build` + `cargo build --features custom-protocol`,
  launch with `GOMAJU_OPEN_SETTINGS=1`, confirm `gomaju: window content loaded: settings`, then
  add/edit/remove/save, reopen to confirm persistence, and exercise the conflict prompt by editing
  `quotes.txt` externally while a dirty editor is open. Verify via logs, not full-screen captures.

## Out of scope (YAGNI)

- No tab-bar restructuring of Settings. No merge UI / 3-way diff for conflicts. No reordering /
  drag-drop of quotes. No per-quote enable. No comment-preservation in the row editor.

## Review trail

- **Codex pass 1** (read-only, gpt-5.5, high): cleared moving the checkbox, `config_path.parent()`,
  and gating; raised P1 (focus-refresh clobber), P2 (two-command partial failure), P2 (atomic
  `quotes::save`), P3 (`#`-line drop). All folded in.
- **Codex pass 2** (corrected framing): cleared quotes-first ordering and the Tauri command
  signatures; surfaced the **dirty + external-edit** clobber path. Resolved here by the
  user-selected **minimal conflict guard** (this document, "Conflict guard").
## Follow-up: per-locale quotes (2026-06-04)

Extended so quotes are **localized** like the UI (the app has two locales: `en`, `zh-Hant`).

- **Storage:** one file per locale, `quotes.<locale>.txt` (replacing the single `quotes.txt`).
  `quotes.rs` `load`/`save`/`pick` take a `locale`; `canonical_locale` maps anything ≠ `"en"` to
  `"zh-Hant"` (matching `i18n::pick`) and bounds the filename to the two known names (a
  frontend-supplied locale can't escape the dir). Defaults: `default_quotes.en.txt` +
  `default_quotes.zh-Hant.txt`. `seed_if_missing` seeds both and **migrates** a legacy `quotes.txt`
  into the English set (old default seed was English).
- **Break-time pick:** `runtime.rs` calls `quotes::pick(dir, &cfg.locale)`. **No cross-locale
  fallback** — an empty active-locale set shows no quote (user's choice).
- **Editor:** a **locale toggle** (`.quote-locale-btn`) switches which set the rows show; `main.ts`
  holds a per-locale map (`quotesByLocale` / `quotesBaselineByLocale`), captures the visible rows on
  switch, and the dirty snapshot (`quotesSnapshot`) covers all locales. Commands gain a `locale`
  arg. `saveQuotes` writes every locale, with the conflict guard generalized to per-locale disk
  comparison (one Overwrite/Keep-disk prompt covers whichever locales diverged on disk).
- **Tests:** added `canonical_locale_maps_unknown_to_default`, per-locale round-trip, locale
  isolation, and legacy-migration tests. All pass; clippy clean; TS builds.

---

- **Codex pass 3** (on this doc): confirmed the conflict guard eliminates the silent overwrite,
  the `disk` vs `quotesBaseline` compare raises no false prompt on a clean save, the
  `crate::quotes::` reference compiles despite the local `quotes` param, and keep_disk / quotes-first
  add no new data-loss path. Lone finding: `cmd_save_quotes` pseudocode needed `.map_err(|e|
  e.to_string())?` on `quotes::save` (`io::Result` → `String`) — **fixed above**. No blockers remain.
