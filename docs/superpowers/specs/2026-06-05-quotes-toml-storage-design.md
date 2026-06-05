# Quotes: migrate from per-locale `.txt` to a single `quotes.toml`

**Date:** 2026-06-05
**Status:** Design approved (decisions A/B) + Codex-reviewed; pending user spec review
**Topic:** Replace the plain-text per-locale break-quote files with one structured `quotes.toml`, mirroring the existing `chimes.toml` architecture.

## Background / current state

Break quotes are shown on the break overlay, picked from the **active locale's** set. Today they are stored as **plain-text, one-quote-per-line** files next to `config.toml` in the OS config dir (`<config_dir>` = `%APPDATA%\com.restee.app\` on Windows):

- `quotes.en.txt`
- `quotes.zh-Hant.txt`
- (legacy `quotes.txt`, pre-localization, migrated into the English set on first run if `quotes.en.txt` is absent)

All quote logic lives host-side in `src-tauri/src/quotes.rs` (no `restee-core` involvement) because plain text needs no serde. The embedded seed defaults are `src-tauri/default_quotes.en.txt` / `default_quotes.zh-Hant.txt`. The only quote-related field in `config.toml` is the `settings.show_quotes` boolean (unchanged by this work).

Relevant touchpoints:
- `src-tauri/src/quotes.rs` — `parse`, `sanitize`, `seed_if_missing`, `save`, `load`, `pick`, `pseudo_random_index`, `canonical_locale`, `quotes_path`.
- `src-tauri/src/lib.rs:66-68` — `quotes::seed_if_missing(dir)` at startup.
- `src-tauri/src/runtime.rs:92` — `quotes::pick(dir, &cfg.locale)` injects into `BreakInfo.quote`.
- `src-tauri/src/commands.rs:246-279` — `cmd_get_quotes(locale)` / `cmd_save_quotes(locale, quotes)`, `require_settings`-gated.
- Frontend: `src/main.ts` (Quotes card, `quotesByLocale`, locale toggle, conflict guard `confirmQuotesConflict`, `onFocusRefresh`), shared `src/quotes-editor.ts`.

## Goal

Store quotes as a single structured `quotes.toml` at `<config_dir>/quotes.toml`, with both locales as top-level arrays:

```toml
en = [
  "Rest is not idleness, and to lie sometimes on the grass is by no means a waste of time.",
  "Almost everything will work again if you unplug it for a few minutes — including you.",
]

"zh-Hant" = [
  "休息一下。",
  "看向遠方。",
]
```

This makes quote storage consistent with `chimes.toml` (a single file separate from `config.toml`), and moves the validated model into `restee-core` for pure unit testing.

### Non-goals

- No change to `settings.show_quotes` (stays in `config.toml`).
- No change to the Settings "Quotes" card UX, the locale toggle, or the conflict-guard behavior (frontend stays effectively untouched).
- No new quote metadata (author, weight, etc.) — plain strings only (YAGNI). The "single file, array-of-tables" shape was considered and rejected for being verbose and awkward to hand-edit.
- No in-memory cache of quote content (preserve "edit the file, see it next break").

## Design

### 1. Data model in `restee-core` (mirrors `chime.rs`)

New module `crates/restee-core/src/quotes.rs`:

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotesFile {
    #[serde(default)]
    pub en: Vec<String>,
    #[serde(rename = "zh-Hant", default)]
    pub zh_hant: Vec<String>,
}
```

- Exactly two known locales — matches the two supported quote sets and keeps the path-safety / unknown-locale invariant of today's `canonical_locale`.
- `get(&self, locale: &str) -> &[String]` and `set(&mut self, locale: &str, quotes: Vec<String>)` canonicalize `"en" → en`, anything else `→ zh_hant` (matching `i18n::pick` / current `canonical_locale`).
- **Unknown-key policy:** the two fields are the only supported locales. Serde's default (ignore unknown fields) is intentional — a hand-edited extra key (e.g. `fr = [...]`) is read without error and simply dropped on the next save round-trip. We do **not** use `deny_unknown_fields` (that would push such a file into corrupt-recovery and discard both real locales). The drop-on-resave behavior is documented, not a bug.
- `sanitize(&mut self) -> bool` — per locale: trim each, drop empty and `#`-comment-leading lines; idempotent (`sanitize(sanitize(x)) == sanitize(x)`). Returns whether anything changed (so callers persist the corrected file). Mirrors `sanitize_chimes`. **Behavior preserved:** a quote starting with `#` is still dropped, so the existing editor (which also drops `#`/blank rows) needs no change. (In TOML `#` inside a quoted string is not a comment, but we keep the rule to preserve current UX and round-trip safety.)
- Embedded default: `pub const DEFAULT_QUOTES_TOML: &str = include_str!("../default_quotes.toml");`, file at `crates/restee-core/default_quotes.toml` (alongside `default_chimes.toml` / `default_config.toml`). Content = the current English + Traditional Chinese default quote sets.

### 2. Load / seed / migrate (in `restee-core`, mirrors `load_chimes`)

```rust
pub fn load_quotes(path: &Path) -> std::io::Result<QuotesFile>
pub fn save_quotes(path: &Path, file: &QuotesFile) -> std::io::Result<()>
pub fn read_quotes(path: &Path) -> QuotesFile   // best-effort, no write (hot path)
```

- `load_quotes` (called once at startup only — the self-healing/migrating init; `cmd_save_quotes` deliberately uses the non-writing `read_quotes` instead, so a save never triggers migration/backup side-effects):
  - **Missing** `quotes.toml` → run **migration** (below) to build the initial file, then `save_quotes`. Migration-from-`.txt` happens **only** here.
  - **Corrupt** → rename to `quotes.toml.bak`, reseed from the **embedded default only** (NOT from `.txt` migration), write. Rationale (per Codex review): the `.txt` files were deleted after a successful first migration; if a delete had failed, re-running migration on a *corrupt* (but newer) toml could resurrect stale `.txt` content over the user's recent edits. So once `quotes.toml` exists, the `.txt` siblings are never read again.
  - **Valid** → parse + `sanitize`; persist only if sanitize changed something.
- `save_quotes` — atomic temp + rename (`quotes.toml.tmp` → `quotes.toml`), creating the parent dir; mirrors `save_chimes` / `config::save`.
- `read_quotes` — best-effort parse + in-memory sanitize, **no disk write**, returns `QuotesFile::default()` on any error. Used by the per-break `pick` so a break-time read never writes.

#### Migration (the heart of this change)

When `quotes.toml` does not exist, build the initial `QuotesFile` from existing runtime files — **user edits win over embedded defaults**, per locale, with this fallback chain:

- `en` ← parse `quotes.en.txt` if present, else parse legacy `quotes.txt` if present (folds in the pre-localization path), else embedded default's `en`.
- `zh-Hant` ← parse `quotes.zh-Hant.txt` if present, else embedded default's `zh_hant`.

**Both-present case (`quotes.en.txt` *and* `quotes.txt`):** `quotes.en.txt` is authoritative and wins; `quotes.txt` is treated as an already-migrated legacy orphan (the old pre-localization migration already copied it into `quotes.en.txt`) and is discarded on delete. This matches the real on-disk state we observed (the user's `quotes.txt` was a stale duplicate of `quotes.en.txt`). The fallback `else` ordering encodes exactly this — `quotes.txt` is only *read* when `quotes.en.txt` is absent — so the both-present case carries no data loss beyond the already-orphaned duplicate.

Parsing reuses the current line rules (trim; drop blank + `#`-comment lines). After `save_quotes` succeeds, **delete** the consumed legacy files (`quotes.en.txt`, `quotes.zh-Hant.txt`, `quotes.txt`) best-effort — their content is now in `quotes.toml`; leaving them would create confusing orphans (edits wouldn't take effect). Deletion is logged and never fatal; a delete failure leaves the (now-ignored) file but does not block migration.

> Path convention: `load_quotes`/`save_quotes`/`read_quotes` all take the **`quotes.toml` path** (matching `load_chimes(path)`). Migration needs the sibling `.txt` files, so `load_quotes` derives the config dir from `path.parent()` to find/delete them. `AppState.quotes_path` is that toml path.

### 3. Host wrapper `src-tauri/src/quotes.rs` (shrinks)

Keeps only the clock-using selection (the core is clock-free):

```rust
pub fn pick(quotes_path: &Path, locale: &str) -> Option<String>
fn pseudo_random_index(len: usize) -> usize   // unchanged (SystemTime-seeded)
```

- `pick` → `restee_core::quotes::read_quotes(quotes_path)`, take `get(locale)`, random-pick. `None` if that locale's set is empty (no cross-locale fallback, as today). Still **re-read each break, no cache**.
- All `parse`/`sanitize`/`save`/`load`/`seed_if_missing`/`canonical_locale`/`quotes_path` move to / are replaced by the core module.

### 4. Commands & frontend — no signature changes

`cmd_get_quotes(locale)` / `cmd_save_quotes(locale, quotes)` keep their exact shapes and return types, so the frontend is untouched.

- `cmd_get_quotes(locale)`: `read_quotes(quotes_path).get(locale).to_vec()`.
- `cmd_save_quotes(locale, quotes)`: **read-modify-write** — `read_quotes` the current file (no side-effects), `set(locale, quotes)`, `sanitize()`, `save_quotes`. Replacing only the one locale's array means saving `en` never clobbers `zh-Hant` (same merge-safety model as `cmd_set_rule_flags`). Returns the sanitized list (so the form reflects trimmed/dropped rows). `read_quotes` (not `load_quotes`) keeps the save free of migration/backup writes and symmetric with `cmd_get_quotes`.
- The frontend's per-locale conflict guard re-reads via `cmd_get_quotes` (now backed by `quotes.toml`) — behavior unchanged.

**Read vs. write, unambiguously (Codex Gap 2):** every *read* path (`cmd_get_quotes`, the conflict re-read, `cmd_save_quotes`'s read step, the per-break `pick`) uses `read_quotes` — **never writes**. Only two paths write: startup `load_quotes` (seed/migrate/self-heal) and `cmd_save_quotes`'s final `save_quotes`. So no command ever triggers self-healing or migration; those are confined to startup.

### 5. Startup + `AppState`

- Add `pub quotes_path: PathBuf` to `AppState` (mirrors `chimes_path`); set it to `<config_dir>/quotes.toml`.
- `lib.rs` setup: replace `quotes::seed_if_missing(dir)` with `restee_core::quotes::load_quotes(&quotes_path)` (seeds + migrates + self-heals). No content cache stored (pick re-reads).
- `runtime.rs:92`: `quotes::pick(&st.quotes_path, &cfg.locale)`.
- `commands.rs`: read `state.quotes_path` instead of deriving the dir.

### 6. Removals, comments, docs

- Delete `src-tauri/default_quotes.en.txt` and `src-tauri/default_quotes.zh-Hant.txt` (replaced by one `crates/restee-core/default_quotes.toml`).
- Fix stale comments that reference `quotes.txt` / `quotes.<locale>.txt`:
  - Rust: `commands.rs:246`, `config.rs:113`.
  - Frontend (per Codex review): `src/main.ts:74,204,212,252` and `src/quotes-editor.ts:2-3` ("quotes persist to `quotes.txt`"). Comments only — no behavior change, since the command signatures are preserved.
- Update CLAUDE.md "Break quotes + pre-break toast" section to describe `quotes.toml` (single file, `restee-core` model, migration-from-`.txt`, delete-after-migrate).

## Testing

Core unit tests in `crates/restee-core/src/quotes.rs` (mirroring `chime.rs` / `config.rs`):

- `QuotesFile` round-trips through `toml::to_string_pretty` → `from_str` with both locales populated (incl. CJK).
- `sanitize` trims, drops blank + `#`-comment lines, is idempotent, per locale.
- `get`/`set` canonicalize unknown locale → `zh-Hant`; `set("en")` does not touch `zh_hant`.
- `load_quotes`: missing → seeds from embedded default; corrupt → backs up `.bak` + reseeds; clean valid → unchanged (no rewrite).
- **Migration**: with `quotes.en.txt` + `quotes.zh-Hant.txt` present and no `quotes.toml`, `load_quotes` builds `quotes.toml` from them, user content preserved, and the `.txt` files are deleted afterward.
- **Legacy path**: only `quotes.txt` present (no `.en.txt`) → migrates into `en`; `quotes.txt` deleted.
- Embedded `default_quotes.toml` parses and is sanitize-clean (assert like the `default_config.toml` / `default_chimes.toml` tests).
- `save` one locale preserves the other (load-modify-write round-trip).

Host test in `src-tauri/src/quotes.rs`:

- `pick` returns a quote from the active locale; `None` when that locale is empty; no cross-locale fallback; `pseudo_random_index` in range.

`cargo test -p restee-core` stays the fast path; the host test runs under the Tauri crate's tests.

## Risks / edge cases

- **Partial migration / crash mid-migrate:** `save_quotes` is atomic (temp+rename) and the `.txt` deletes happen only *after* a successful write, so a crash leaves either the old `.txt` set (re-migrated next launch) or the new `quotes.toml` — never a torn state.
- **A delete fails** (file locked): logged, non-fatal; the orphan is ignored thereafter (migration won't re-run because `quotes.toml` now exists). Acceptable.
- **Concurrent saves from two windows:** same single-user caveat already documented for rules; load-modify-write narrows but doesn't eliminate it, matching existing behavior.
- **`#`-leading quote semantics:** intentionally still dropped to preserve current editor UX (documented above).
- **Live corrupt TOML is a new, accepted failure mode (Codex review):** unlike line-based `.txt` (where one malformed line doesn't kill the rest), a single TOML syntax error makes the *whole* `quotes.toml` unparseable. The hot-path `read_quotes` returns empty on a parse error, so a bad **hand-edit** shows no quote for that break. This is inherent to a structured format and accepted because: (a) the in-app Settings editor — the primary edit path — always writes valid TOML; and (b) the next startup `load_quotes` self-heals the corrupt file (backup `.bak` + reseed). We deliberately do **not** make `read_quotes` self-heal (it must not write during a break read). Net: a hand-edit typo costs at most quote-less breaks until the next launch, never data corruption.

## Out of scope / future

- Allowing `#`-leading quotes now that TOML would permit them (would require an editor change).
- Per-quote metadata (author, weighting).
- Caching quote content in `AppState` (rejected to keep live edits).
