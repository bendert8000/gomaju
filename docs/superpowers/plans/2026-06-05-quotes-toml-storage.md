# Quotes → single `quotes.toml` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the per-locale plain-text `quotes.<locale>.txt` break-quote files with one structured `quotes.toml`, with a first-run migration that folds the old files in and deletes them.

**Architecture:** Move the quote data model + validation + storage into the dependency-free `restee-core` crate (a new `quotes` module mirroring `chime.rs`); the Tauri host keeps only the wall-clock random `pick`. Commands keep their existing signatures so the frontend is untouched; `cmd_save_quotes` becomes load-modify-write so saving one locale never clobbers the other.

**Tech Stack:** Rust (serde + `toml` 0.8, already deps of `restee-core`), Tauri v2 host, TypeScript/HTML frontend (comment-only edits).

**Spec:** `docs/superpowers/specs/2026-06-05-quotes-toml-storage-design.md` (Codex-reviewed).

**TDD note for the executor:** within each task, write the test(s) **first** and run them to watch them fail (a compile error for a not-yet-defined symbol counts as red) **before** writing the implementation. The steps below group test and implementation code for readability, but follow strict red→green order.

**Conventions in this codebase:**
- Core engine tests run with `cargo test -p restee-core` (fast, no Tauri).
- Host crate builds/tests need the Tauri toolchain; a quick check is `cargo build -p restee --features custom-protocol` (workspace target dir at repo root). Full app: `npm run tauri build`.
- Lint: `cargo clippy --workspace --all-targets`.
- Mirror the existing `chime.rs` patterns (atomic temp+rename writes, self-healing load, embedded default via `include_str!`, `sanitize` returns `bool` "changed").

---

## File Structure

**Create:**
- `crates/restee-core/src/quotes.rs` — `QuotesFile` DTO, `get`/`set`/`sanitize`, `parse_text`, `read_quotes`, `save_quotes`, `load_quotes`, migration helpers, embedded default, unit tests.
- `crates/restee-core/default_quotes.toml` — embedded seed (both locales), `include_str!`'d by the module.

**Modify:**
- `crates/restee-core/src/lib.rs` — add `pub mod quotes;`.
- `src-tauri/src/quotes.rs` — shrink to the host `pick` + `pseudo_random_index` (delete `parse`/`sanitize`/`save`/`load`/`seed_if_missing`/`canonical_locale`/`quotes_path`).
- `src-tauri/src/app_state.rs` — add `quotes_path: PathBuf`.
- `src-tauri/src/lib.rs` — compute `quotes_path`, call `restee_core::quotes::load_quotes`, manage it in `AppState`.
- `src-tauri/src/runtime.rs:88-96` — `pick` via `state.quotes_path`.
- `src-tauri/src/commands.rs:246-279` — `cmd_get_quotes` → `read_quotes`; `cmd_save_quotes` → load-modify-write; fix the section comment.
- `crates/restee-core/src/config.rs:113`, `src/main.ts:74,204,212,252`, `src/quotes-editor.ts:2` — stale comments (`quotes.txt`/`quotes.<locale>.txt` → `quotes.toml`).
- `CLAUDE.md` — rewrite the "Break quotes + pre-break toast" storage description.

**Delete:**
- `src-tauri/default_quotes.en.txt`, `src-tauri/default_quotes.zh-Hant.txt` (replaced by the one core `default_quotes.toml`).

---

## Task 1: `restee-core` quotes module — model, get/set, sanitize

**Files:**
- Create: `crates/restee-core/src/quotes.rs`
- Modify: `crates/restee-core/src/lib.rs` (add module export)
- Test: inline `#[cfg(test)]` in `crates/restee-core/src/quotes.rs`

- [ ] **Step 1: Create the module with the model + helpers (no embedded default yet)**

Create `crates/restee-core/src/quotes.rs`:

```rust
//! User-editable break quotes, stored as a single `quotes.toml` (separate from `config.toml`).
//!
//! Both supported UI locales' quote lists live in one file as top-level arrays (`en` and
//! `zh-Hant`). Like `chime.rs`, this module is dependency-free (serde/toml only): it owns the DTO,
//! a `sanitize` pass, atomic load/save, and a one-time migration from the old per-locale
//! `quotes.<locale>.txt` text files. The host (Tauri layer) keeps only the wall-clock random pick.
//!
//! A quote starting with `#` is dropped (preserving the old text-file editor behavior, where `#`
//! marked a comment), and blank/whitespace-only quotes are dropped. There is no cross-locale
//! fallback — an empty active locale shows no quote.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The on-disk `quotes.toml`: both supported locales' quote lists as top-level arrays. Exactly two
/// known locales (matching `i18n::pick`'s `"en"` vs. default split). Unknown top-level keys are
/// ignored by serde and dropped on the next save — `deny_unknown_fields` is intentionally NOT used
/// (it would push a hand-edited stray key into corrupt-recovery and discard both real locales).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotesFile {
    #[serde(default)]
    pub en: Vec<String>,
    #[serde(rename = "zh-Hant", default)]
    pub zh_hant: Vec<String>,
}

impl QuotesFile {
    /// Borrow one locale's quotes. Canonicalizes like `i18n::pick`: `"en"` -> English, anything
    /// else -> Traditional Chinese (the default).
    pub fn get(&self, locale: &str) -> &[String] {
        if locale == "en" {
            &self.en
        } else {
            &self.zh_hant
        }
    }

    /// Replace one locale's quotes (canonicalized as in `get`).
    pub fn set(&mut self, locale: &str, quotes: Vec<String>) {
        if locale == "en" {
            self.en = quotes;
        } else {
            self.zh_hant = quotes;
        }
    }

    /// Trim each quote and drop empty + `#`-comment lines, per locale. Idempotent
    /// (`sanitize` twice == once). Returns whether anything changed, so a loaded-but-dirty file can
    /// be re-persisted (mirrors `sanitize_chimes`).
    pub fn sanitize(&mut self) -> bool {
        let en = sanitize_list(&self.en);
        let zh = sanitize_list(&self.zh_hant);
        let changed = en != self.en || zh != self.zh_hant;
        self.en = en;
        self.zh_hant = zh;
        changed
    }
}

/// Trim each; drop empty + `#`-comment lines.
fn sanitize_list(quotes: &[String]) -> Vec<String> {
    quotes
        .iter()
        .map(|q| q.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// Parse plain-text quote-file contents (one quote per line; trim; drop blank + `#`-comment lines).
/// Used only by the one-time `.txt` -> `quotes.toml` migration.
fn parse_text(contents: &str) -> Vec<String> {
    contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}
```

- [ ] **Step 2: Export the module**

Modify `crates/restee-core/src/lib.rs` — add `pub mod quotes;` in the module list (alphabetical, after `mod engine;`/before `mod rule;` is fine; keep the `pub mod` ones grouped). Resulting top:

```rust
pub mod alarm;
pub mod chime;
pub mod config;
mod engine;
pub mod quotes;
mod rule;
mod settings;
```

- [ ] **Step 3: Write the tests** (append to `crates/restee-core/src/quotes.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_set_canonicalize_locale() {
        let mut f = QuotesFile::default();
        f.set("en", vec!["hi".into()]);
        f.set("zh-Hant", vec!["你好".into()]);
        assert_eq!(f.get("en"), ["hi"]);
        assert_eq!(f.get("zh-Hant"), ["你好"]);
        // Unknown locale canonicalizes to zh-Hant.
        assert_eq!(f.get("fr"), ["你好"]);
        // Setting en does not touch zh-Hant.
        f.set("en", vec!["bye".into()]);
        assert_eq!(f.get("zh-Hant"), ["你好"]);
    }

    #[test]
    fn sanitize_trims_drops_blank_and_comment_lines_per_locale() {
        let mut f = QuotesFile {
            en: vec!["  Rest.  ".into(), String::new(), "# c".into(), "Stretch.".into()],
            zh_hant: vec!["  休息。 ".into(), "   ".into()],
        };
        assert!(f.sanitize());
        assert_eq!(f.en, ["Rest.", "Stretch."]);
        assert_eq!(f.zh_hant, ["休息。"]);
    }

    #[test]
    fn sanitize_is_idempotent_and_reports_no_change_when_clean() {
        let mut f = QuotesFile { en: vec!["Rest.".into()], zh_hant: vec!["休息。".into()] };
        assert!(!f.sanitize());
        let once = f.clone();
        assert!(!f.sanitize());
        assert_eq!(f, once);
    }

    #[test]
    fn round_trips_through_toml_with_both_locales() {
        let f = QuotesFile {
            en: vec!["Rest well.".into(), "Look away.".into()],
            zh_hant: vec!["好好休息。".into(), "望向遠方。".into()],
        };
        let text = toml::to_string_pretty(&f).unwrap();
        let parsed: QuotesFile = toml::from_str(&text).unwrap();
        assert_eq!(f, parsed);
    }

    #[test]
    fn unknown_locale_key_is_ignored_not_an_error() {
        let text = "en = [\"hi\"]\nfr = [\"bonjour\"]\n";
        let parsed: QuotesFile = toml::from_str(text).expect("stray key must not error");
        assert_eq!(parsed.en, ["hi"]);
        assert!(parsed.zh_hant.is_empty());
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p restee-core quotes::tests`
Expected: all 5 PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/restee-core/src/quotes.rs crates/restee-core/src/lib.rs
git commit -m "feat(core): QuotesFile model + sanitize for quotes.toml"
```

---

## Task 2: Embedded `default_quotes.toml`

**Files:**
- Create: `crates/restee-core/default_quotes.toml`
- Modify: `crates/restee-core/src/quotes.rs` (add const + `embedded_default_quotes`)
- Test: inline

- [ ] **Step 1: Create `crates/restee-core/default_quotes.toml`** (content = the current shipped defaults, now in one TOML file)

```toml
# Default break quotes seeded on first run (one entry per quote). The user's quotes.toml lives in
# the OS config dir next to config.toml; Restee never overwrites it after seeding/migration.

en = [
  "Rest is not idleness, and to lie sometimes on the grass is by no means a waste of time.",
  "Almost everything will work again if you unplug it for a few minutes — including you.",
  "Take care of your body. It's the only place you have to live.",
  "Your eyes deserve a horizon now and then.",
  "The time to relax is when you don't have time for it.",
  "Breathe. The rest can wait a moment.",
  "A short pause now saves a long ache later.",
  "Stand up, stretch, and let your mind wander.",
  "Sometimes the most productive thing you can do is rest.",
  "Look up. Look far. Let your eyes loosen.",
]

"zh-Hant" = [
  "休息不是懶惰，偶爾躺在草地上看天空，絕非浪費時間。",
  "幾乎所有東西重新插電後都能再運作 —— 包括你自己。",
  "照顧好你的身體，那是你唯一的棲身之所。",
  "讓眼睛偶爾望向遠方的地平線。",
  "越是沒空休息的時候，越該休息。",
  "深呼吸，其餘的事可以稍等片刻。",
  "此刻短短的暫停，能省去日後長長的痠痛。",
  "站起來，伸展一下，讓思緒隨意漫遊。",
  "有時候，最有生產力的事就是休息。",
  "抬頭，望遠，讓雙眼放鬆。",
]
```

- [ ] **Step 2: Add the embedded const + helper** (in `crates/restee-core/src/quotes.rs`, after the `parse_text` fn)

```rust
/// The starter quotes a fresh install is seeded with — editable TOML, embedded at compile time, so
/// `quotes.toml` isn't empty on first run (and is the corrupt-recovery source).
pub const DEFAULT_QUOTES_TOML: &str = include_str!("../default_quotes.toml");

fn embedded_default_quotes() -> QuotesFile {
    toml::from_str(DEFAULT_QUOTES_TOML).expect("embedded default_quotes.toml must parse")
}
```

- [ ] **Step 3: Write the test** (add to the `tests` module)

```rust
    #[test]
    fn embedded_default_quotes_parse_and_are_clean() {
        let mut f = embedded_default_quotes();
        assert!(!f.en.is_empty(), "default en quotes should be non-empty");
        assert!(!f.zh_hant.is_empty(), "default zh-Hant quotes should be non-empty");
        // The shipped default must already be valid — sanitize should change nothing.
        assert!(!f.sanitize());
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p restee-core quotes::tests::embedded_default_quotes_parse_and_are_clean`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/restee-core/default_quotes.toml crates/restee-core/src/quotes.rs
git commit -m "feat(core): embedded default_quotes.toml seed"
```

---

## Task 3: `save_quotes` + `read_quotes`

**Files:**
- Modify: `crates/restee-core/src/quotes.rs`
- Test: inline

- [ ] **Step 1: Implement both functions** (in `crates/restee-core/src/quotes.rs`, after `embedded_default_quotes`)

```rust
/// Atomically write `quotes.toml` (temp + rename), creating its parent dir. Mirrors
/// `chime::save_chimes` / `config::save`, so a failed write never truncates the live file.
pub fn save_quotes(path: &Path, file: &QuotesFile) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(file)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Best-effort read for the hot path (the per-break pick): parse + in-memory sanitize, and **never
/// writes**. Returns an empty `QuotesFile` on any read/parse error (a corrupt hand-edit shows no
/// quote until the next startup `load_quotes` self-heals it).
pub fn read_quotes(path: &Path) -> QuotesFile {
    match fs::read_to_string(path) {
        Ok(text) => match toml::from_str::<QuotesFile>(&text) {
            Ok(mut file) => {
                file.sanitize();
                file
            }
            Err(_) => QuotesFile::default(),
        },
        Err(_) => QuotesFile::default(),
    }
}
```

- [ ] **Step 2: Write the tests** (add to the `tests` module; uses a unique temp dir per test)

```rust
    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("restee-quotes-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn save_then_read_round_trips() {
        let dir = temp_dir("save-read");
        let path = dir.join("quotes.toml");
        let f = QuotesFile {
            en: vec!["Take a breath.".into()],
            zh_hant: vec!["深呼吸。".into()],
        };
        save_quotes(&path, &f).unwrap();
        assert_eq!(read_quotes(&path), f);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_missing_or_corrupt_yields_empty() {
        let dir = temp_dir("read-bad");
        let missing = dir.join("quotes.toml");
        assert_eq!(read_quotes(&missing), QuotesFile::default());
        fs::write(&missing, "this is not = valid toml [[[").unwrap();
        assert_eq!(read_quotes(&missing), QuotesFile::default());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_sanitizes_in_memory_without_writing() {
        let dir = temp_dir("read-sanitize");
        let path = dir.join("quotes.toml");
        fs::write(&path, "en = [\"  Rest.  \", \"# c\"]\n").unwrap();
        let f = read_quotes(&path);
        assert_eq!(f.en, ["Rest."]);
        // The file on disk is untouched by a read.
        assert!(fs::read_to_string(&path).unwrap().contains("# c"));
        let _ = fs::remove_dir_all(&dir);
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p restee-core quotes::tests`
Expected: all PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/restee-core/src/quotes.rs
git commit -m "feat(core): atomic save_quotes + best-effort read_quotes"
```

---

## Task 4: Migration helpers (`.txt` → `QuotesFile`, then delete)

**Files:**
- Modify: `crates/restee-core/src/quotes.rs`
- Test: inline

- [ ] **Step 1: Implement the helpers** (in `crates/restee-core/src/quotes.rs`, after `read_quotes`)

```rust
/// Build the initial `QuotesFile` from any existing legacy `.txt` files, user edits winning over the
/// embedded default, per locale. Only used on the missing-`quotes.toml` (first-run) path.
///
/// Fallback chain:
/// - `en`      <- `quotes.en.txt`, else legacy `quotes.txt`, else embedded default.
/// - `zh-Hant` <- `quotes.zh-Hant.txt`, else embedded default.
///
/// Both-present (`quotes.en.txt` AND `quotes.txt`): `quotes.en.txt` wins; `quotes.txt` is a stale,
/// already-migrated orphan (the old pre-localization migration copied it into `quotes.en.txt`).
fn migrate_from_txt(config_dir: &Path) -> QuotesFile {
    let mut file = embedded_default_quotes();
    if let Ok(text) = fs::read_to_string(config_dir.join("quotes.en.txt")) {
        file.en = parse_text(&text);
    } else if let Ok(text) = fs::read_to_string(config_dir.join("quotes.txt")) {
        file.en = parse_text(&text);
    }
    if let Ok(text) = fs::read_to_string(config_dir.join("quotes.zh-Hant.txt")) {
        file.zh_hant = parse_text(&text);
    }
    file
}

/// Best-effort delete of the consumed legacy `.txt` files after a successful migration write. A
/// failed delete is logged and ignored — the orphan is never read again (migration only runs when
/// `quotes.toml` is absent, and by now it exists).
fn delete_legacy_txt(config_dir: &Path) {
    for name in ["quotes.en.txt", "quotes.zh-Hant.txt", "quotes.txt"] {
        let p = config_dir.join(name);
        if p.exists() {
            match fs::remove_file(&p) {
                Ok(()) => eprintln!("restee: removed migrated quote file {}", p.display()),
                Err(e) => eprintln!("restee: could not remove {} ({e})", p.display()),
            }
        }
    }
}
```

- [ ] **Step 2: Write the tests** (add to the `tests` module)

```rust
    #[test]
    fn migrate_prefers_locale_txt_then_legacy_then_default() {
        // en.txt present -> wins; zh-Hant.txt absent -> embedded default.
        let dir = temp_dir("migrate-prefer");
        fs::write(dir.join("quotes.en.txt"), "From en.txt.\n# skip\n").unwrap();
        let f = migrate_from_txt(&dir);
        assert_eq!(f.en, ["From en.txt."]);
        assert!(!f.zh_hant.is_empty()); // embedded default
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrate_falls_back_to_legacy_quotes_txt_for_en() {
        // Only legacy quotes.txt (no quotes.en.txt) -> becomes en.
        let dir = temp_dir("migrate-legacy");
        fs::write(dir.join("quotes.txt"), "Legacy line.\n").unwrap();
        let f = migrate_from_txt(&dir);
        assert_eq!(f.en, ["Legacy line."]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrate_both_present_prefers_en_txt_over_legacy() {
        let dir = temp_dir("migrate-both");
        fs::write(dir.join("quotes.en.txt"), "Authoritative.\n").unwrap();
        fs::write(dir.join("quotes.txt"), "Stale orphan.\n").unwrap();
        let f = migrate_from_txt(&dir);
        assert_eq!(f.en, ["Authoritative."]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_legacy_removes_all_three_when_present() {
        let dir = temp_dir("migrate-delete");
        for n in ["quotes.en.txt", "quotes.zh-Hant.txt", "quotes.txt"] {
            fs::write(dir.join(n), "x\n").unwrap();
        }
        delete_legacy_txt(&dir);
        for n in ["quotes.en.txt", "quotes.zh-Hant.txt", "quotes.txt"] {
            assert!(!dir.join(n).exists(), "{n} should be deleted");
        }
        let _ = fs::remove_dir_all(&dir);
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p restee-core quotes::tests`
Expected: all PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/restee-core/src/quotes.rs
git commit -m "feat(core): quotes migration helpers (.txt -> quotes.toml, then delete)"
```

---

## Task 5: `load_quotes` (self-healing + migrate-on-missing)

**Files:**
- Modify: `crates/restee-core/src/quotes.rs`
- Test: inline

- [ ] **Step 1: Implement `load_quotes`** (in `crates/restee-core/src/quotes.rs`, after `delete_legacy_txt`)

```rust
/// Load `quotes.toml`, self-healing like `chime::load_chimes`:
/// - **missing** -> migrate from the legacy `.txt` files (or seed from the embedded default when
///   none exist), write, then delete the consumed `.txt` files. Migration runs ONLY here.
/// - **corrupt** -> back up `quotes.toml.bak` and reseed from the **embedded default only** (never
///   re-read the `.txt` siblings — they were deleted after the first migration, and re-reading a
///   failed-delete orphan could resurrect stale content over a newer-but-corrupt file).
/// - **valid** -> parse + sanitize, persisting only if sanitize changed something.
///
/// `path` is the `quotes.toml` path; its parent is the config dir that holds the legacy `.txt`
/// files. Called once at startup, and by `cmd_save_quotes` to read the current file before editing.
pub fn load_quotes(path: &Path) -> std::io::Result<QuotesFile> {
    let config_dir = path.parent().unwrap_or_else(|| Path::new("."));

    if !path.exists() {
        let mut file = migrate_from_txt(config_dir);
        file.sanitize();
        save_quotes(path, &file)?;
        delete_legacy_txt(config_dir);
        return Ok(file);
    }

    let text = fs::read_to_string(path)?;
    match toml::from_str::<QuotesFile>(&text) {
        Ok(mut file) => {
            if file.sanitize() {
                let _ = save_quotes(path, &file);
            }
            Ok(file)
        }
        Err(_) => {
            let backup = path.with_extension("toml.bak");
            let _ = fs::rename(path, &backup);
            let file = embedded_default_quotes();
            save_quotes(path, &file)?;
            Ok(file)
        }
    }
}
```

- [ ] **Step 2: Write the tests** (add to the `tests` module)

```rust
    #[test]
    fn load_missing_with_no_txt_seeds_embedded_default() {
        let dir = temp_dir("load-seed");
        let path = dir.join("quotes.toml");
        let f = load_quotes(&path).unwrap();
        assert!(path.exists());
        assert_eq!(f, embedded_default_quotes());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_missing_migrates_txt_and_deletes_them() {
        let dir = temp_dir("load-migrate");
        let path = dir.join("quotes.toml");
        fs::write(dir.join("quotes.en.txt"), "Migrated EN.\n").unwrap();
        fs::write(dir.join("quotes.zh-Hant.txt"), "中文遷移。\n").unwrap();

        let f = load_quotes(&path).unwrap();
        assert_eq!(f.en, ["Migrated EN."]);
        assert_eq!(f.zh_hant, ["中文遷移。"]);
        assert!(path.exists());
        assert!(!dir.join("quotes.en.txt").exists());
        assert!(!dir.join("quotes.zh-Hant.txt").exists());
        // Re-load reads the toml, not the (now-deleted) txt.
        assert_eq!(load_quotes(&path).unwrap(), f);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_corrupt_backs_up_and_reseeds_from_embedded_not_txt() {
        let dir = temp_dir("load-corrupt");
        let path = dir.join("quotes.toml");
        fs::write(&path, "garbage = [[[ not toml").unwrap();
        // A leftover stray .txt must NOT be re-consumed on corrupt recovery.
        fs::write(dir.join("quotes.en.txt"), "Should be ignored.\n").unwrap();

        let f = load_quotes(&path).unwrap();
        assert_eq!(f, embedded_default_quotes());
        assert!(dir.join("quotes.toml.bak").exists());
        assert_ne!(f.en, ["Should be ignored."]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_valid_clean_file_is_not_rewritten() {
        let dir = temp_dir("load-clean");
        let path = dir.join("quotes.toml");
        let f = QuotesFile { en: vec!["Rest.".into()], zh_hant: vec!["休息。".into()] };
        save_quotes(&path, &f).unwrap();
        let before = fs::read_to_string(&path).unwrap();
        assert_eq!(load_quotes(&path).unwrap(), f);
        assert_eq!(fs::read_to_string(&path).unwrap(), before, "clean load must not rewrite");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_one_locale_preserves_the_other() {
        // Mirrors cmd_save_quotes's load-modify-write: editing en must not touch zh-Hant.
        // (Spec requirement: saving one locale preserves the other.)
        let dir = temp_dir("save-isolation");
        let path = dir.join("quotes.toml");
        save_quotes(
            &path,
            &QuotesFile { en: vec!["old en".into()], zh_hant: vec!["保留中文".into()] },
        )
        .unwrap();
        // Load full file, replace only en, sanitize, write — exactly what the command does.
        let mut file = load_quotes(&path).unwrap();
        file.set("en", vec!["new en".into()]);
        file.sanitize();
        save_quotes(&path, &file).unwrap();
        let reloaded = load_quotes(&path).unwrap();
        assert_eq!(reloaded.en, ["new en"]);
        assert_eq!(reloaded.zh_hant, ["保留中文"], "the other locale must be preserved");
        let _ = fs::remove_dir_all(&dir);
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p restee-core quotes`
Expected: all PASS. Also run the whole crate: `cargo test -p restee-core` → green.

- [ ] **Step 4: Commit**

```bash
git add crates/restee-core/src/quotes.rs
git commit -m "feat(core): self-healing load_quotes with first-run .txt migration"
```

---

## Task 6: Host transition — thin `pick`, AppState, lib.rs, runtime.rs, commands.rs

This task is **one atomic change/commit**: the `pick` signature change and the removal of the old host functions couple `quotes.rs`, `runtime.rs`, `lib.rs`, and `commands.rs`, so the tree only compiles once all are updated.

**Files:**
- Modify: `src-tauri/src/quotes.rs` (rewrite), `src-tauri/src/app_state.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/runtime.rs`, `src-tauri/src/commands.rs`
- Test: inline `#[cfg(test)]` in `src-tauri/src/quotes.rs`

- [ ] **Step 1: Rewrite `src-tauri/src/quotes.rs`** to the thin host wrapper (replace the entire file)

```rust
//! Host-side break-quote selection. The TOML model, validation, storage, and `.txt` migration live
//! in `restee_core::quotes` (pure + unit-tested). This module keeps only the wall-clock random pick
//! — the pure engine is clock-free, so the host owns anything that reads the clock.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Pick one quote (pseudo-randomly) from the active locale in `quotes.toml`. `None` if that locale
/// has no quotes — there is no cross-locale fallback. Re-reads the file each break (no cache), so
/// edits take effect live; the read never writes.
pub fn pick(quotes_path: &Path, locale: &str) -> Option<String> {
    let file = restee_core::quotes::read_quotes(quotes_path);
    let quotes = file.get(locale);
    if quotes.is_empty() {
        return None;
    }
    Some(quotes[pseudo_random_index(quotes.len())].clone())
}

/// A throwaway index in `0..len`, seeded from the wall clock. A clock error falls back to index 0.
fn pseudo_random_index(len: usize) -> usize {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0);
    nanos % len
}

#[cfg(test)]
mod tests {
    use super::*;
    use restee_core::quotes::{save_quotes, QuotesFile};

    fn temp_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("restee-host-quotes-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("quotes.toml")
    }

    #[test]
    fn pick_returns_a_quote_from_the_active_locale() {
        let path = temp_path("pick-active");
        save_quotes(
            &path,
            &QuotesFile { en: vec!["only-en".into()], zh_hant: vec!["only-zh".into()] },
        )
        .unwrap();
        assert_eq!(pick(&path, "en").as_deref(), Some("only-en"));
        assert_eq!(pick(&path, "zh-Hant").as_deref(), Some("only-zh"));
    }

    #[test]
    fn pick_none_when_active_locale_empty_no_cross_fallback() {
        let path = temp_path("pick-empty");
        save_quotes(&path, &QuotesFile { en: vec![], zh_hant: vec!["有中文".into()] }).unwrap();
        // en is empty -> None, even though zh-Hant has quotes (no cross-locale fallback).
        assert_eq!(pick(&path, "en"), None);
    }

    #[test]
    fn pseudo_random_index_is_in_range() {
        for len in 1..=10usize {
            assert!(pseudo_random_index(len) < len);
        }
    }
}
```

- [ ] **Step 2: Add `quotes_path` to `AppState`** — modify `src-tauri/src/app_state.rs`, adding the field after `chimes_path` (around line 21):

```rust
    pub chimes_path: PathBuf,
    /// Path to `quotes.toml` (break quotes, separate from config.toml, next to it in the config
    /// dir). Read live on each break by `quotes::pick`; the Settings "Quotes" card edits it.
    pub quotes_path: PathBuf,
```

- [ ] **Step 3: Wire startup in `src-tauri/src/lib.rs`** — replace the seed block (lines 65-68):

```rust
            // Seed the user-editable break-quotes file next to config.toml (first run only).
            if let Some(dir) = config_path.parent() {
                quotes::seed_if_missing(dir);
            }
```

with:

```rust
            // Break quotes live in their own quotes.toml (separate from config.toml). On first run
            // this migrates the old per-locale quotes.<locale>.txt files into it and deletes them;
            // it also self-heals a missing/corrupt file. pick re-reads it live each break.
            let quotes_path: PathBuf = config_path
                .parent()
                .map(|dir| dir.join("quotes.toml"))
                .unwrap_or_else(|| PathBuf::from("quotes.toml"));
            if let Err(e) = restee_core::quotes::load_quotes(&quotes_path) {
                eprintln!("restee: could not initialize quotes.toml ({e})");
            }
```

Then add `quotes_path,` to the `AppState { ... }` initializer (after `chimes_path,`, around line 104):

```rust
                chimes: Mutex::new(chimes),
                chimes_path,
                quotes_path,
                idle_status,
```

- [ ] **Step 4: Update the pick call in `src-tauri/src/runtime.rs`** — replace lines 88-96:

```rust
                    let quote = if cfg.settings.show_quotes {
                        state
                            .config_path
                            .parent()
                            .and_then(|dir| crate::quotes::pick(dir, &cfg.locale))
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };
```

with:

```rust
                    let quote = if cfg.settings.show_quotes {
                        crate::quotes::pick(&state.quotes_path, &cfg.locale).unwrap_or_default()
                    } else {
                        String::new()
                    };
```

(The comment on lines 85-87 still reads correctly; optionally tweak "quotes file" → "quotes.toml".)

- [ ] **Step 5: Update the commands in `src-tauri/src/commands.rs`** — replace the section comment (line 246) and both command bodies (lines 246-279):

```rust
// --- Break quotes (settings-window only; stored in quotes.toml, separate from config.toml) ---

/// One locale's break quotes (sanitized). Read by the Settings "Quotes" card on load, on focus, and
/// on locale switch, and re-read inside the card's Save to detect external edits to `quotes.toml`.
/// A read never writes (`read_quotes`); `locale` canonicalizes to one of the two supported sets.
#[tauri::command]
pub fn cmd_get_quotes(
    window: WebviewWindow,
    state: State<'_, AppState>,
    locale: String,
) -> Result<Vec<String>, String> {
    require_settings(&window)?;
    let file = restee_core::quotes::read_quotes(&state.quotes_path);
    Ok(file.get(&locale).to_vec())
}

/// Persist one locale's edited quote list into `quotes.toml` via load-modify-write: load the full
/// file, replace only this locale's array, sanitize, write. Replacing one locale means saving `en`
/// never clobbers `zh-Hant` (the Settings window saves each locale in a separate call). Returns the
/// sanitized list so the form reflects any trimmed/dropped rows (like `cmd_save_config`). Quotes are
/// re-read live on each break, so there's no in-memory cache to update (unlike config/alarms/chimes).
#[tauri::command]
pub fn cmd_save_quotes(
    window: WebviewWindow,
    state: State<'_, AppState>,
    locale: String,
    quotes: Vec<String>,
) -> Result<Vec<String>, String> {
    require_settings(&window)?;
    let mut file =
        restee_core::quotes::load_quotes(&state.quotes_path).map_err(|e| e.to_string())?;
    file.set(&locale, quotes);
    file.sanitize();
    restee_core::quotes::save_quotes(&state.quotes_path, &file).map_err(|e| e.to_string())?;
    Ok(file.get(&locale).to_vec())
}
```

- [ ] **Step 6: Build + run host quotes tests**

Run: `cargo build -p restee --features custom-protocol`
Expected: compiles (no references to the removed `quotes::seed_if_missing` / `quotes::load` / `quotes::sanitize` / `quotes::save` remain).

Run: `cargo test -p restee quotes::tests`
Expected: 3 host tests PASS.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/quotes.rs src-tauri/src/app_state.rs src-tauri/src/lib.rs src-tauri/src/runtime.rs src-tauri/src/commands.rs
git commit -m "feat: host uses restee-core quotes.toml (thin pick, load-modify-write save)"
```

---

## Task 7: Remove old `.txt` seeds + fix stale comments

**Files:**
- Delete: `src-tauri/default_quotes.en.txt`, `src-tauri/default_quotes.zh-Hant.txt`
- Modify: `crates/restee-core/src/config.rs:113`, `src/main.ts:74,204,212,252`, `src/quotes-editor.ts:2`

- [ ] **Step 1: Delete the orphaned seed files** (their `include_str!` was removed in Task 6)

```bash
git rm src-tauri/default_quotes.en.txt src-tauri/default_quotes.zh-Hant.txt
```

- [ ] **Step 2: Fix `crates/restee-core/src/config.rs:113`** — change the doc comment:

Find:
```rust
    /// Show an inspirational quote (from the user-editable `quotes.txt`) on the break overlay.
```
Replace with:
```rust
    /// Show an inspirational quote (from the user-editable `quotes.toml`) on the break overlay.
```

- [ ] **Step 3: Fix `src/quotes-editor.ts:2`** — change the comment:

Find:
```ts
// module (like rule-editor.ts) so src/main.ts stays lean. Quotes persist to `quotes.txt` (separate
```
Replace with:
```ts
// module (like rule-editor.ts) so src/main.ts stays lean. Quotes persist to `quotes.toml` (separate
```

- [ ] **Step 4: Fix `src/main.ts` comments** — these are descriptive comments; update the file references:
  - Line 74: `Break quotes are per-locale (`quotes.<locale>.txt`, separate from config.toml).` → `Break quotes are per-locale, stored in quotes.toml (separate from config.toml).`
  - Line 204: `Also re-sync every locale's quotes from disk, so an external `quotes.<locale>.txt` edit made` → `... so an external quotes.toml edit made`
  - Line 212: `Persist every locale's quote rows, guarding against an edit made to any `quotes.<locale>.txt`` → `... guarding against an edit made to quotes.toml`
  - Line 252: `// Quotes first: a plain quotes.txt write with no live side-effects, and the one that can` → `// Quotes first: a quotes.toml write with no live side-effects, and the one that can`

(Use Edit on each exact string; these are comments only — no behavior change.)

- [ ] **Step 5: Verify frontend still type-checks**

Run: `npm run build`
Expected: `tsc && vite build` succeed (comment-only edits; no type changes).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "chore: drop old default_quotes.*.txt; fix stale quotes.txt comments"
```

---

## Task 8: Update `CLAUDE.md`

**Files:**
- Modify: `CLAUDE.md` ("Break quotes + pre-break toast" section)

- [ ] **Step 1: Rewrite the storage description** in the "Break quotes + pre-break toast" section.

Replace the first bullet (the one starting "The break overlay shows an optional inspirational **quote**, picked from the **active locale's** quotes file (next to `config.toml`: `quotes.en.txt` / `quotes.zh-Hant.txt` ...") and the editor bullet's storage details to describe the new model. Key points the new text must state:
- Quotes live in a single **`quotes.toml`** at `<config_dir>/quotes.toml` (next to `config.toml`), both locales as top-level arrays (`en`, `"zh-Hant"`).
- The model + `sanitize` + `load_quotes`/`save_quotes`/`read_quotes` live in `crates/restee-core/src/quotes.rs` (mirrors `chime.rs`), seeded from embedded `crates/restee-core/default_quotes.toml`; `load_quotes` self-heals (seed-on-missing / backup-on-corrupt-from-embedded-default).
- First run **migrates** the old `quotes.<locale>.txt` (and legacy `quotes.txt` → `en`) into `quotes.toml`, then **deletes** the `.txt` files. Migration runs only on the missing-file path; corrupt recovery never re-reads `.txt`.
- Host `src-tauri/src/quotes.rs` keeps only `pick` (wall-clock random; re-read live each break, no cache, no cross-locale fallback).
- `cmd_get_quotes`/`cmd_save_quotes` keep per-locale signatures; save is load-modify-write (one locale never clobbers the other). Reads never write.
- `settings.show_quotes` stays in `config.toml`.

- [ ] **Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: CLAUDE.md describes quotes.toml storage + migration"
```

---

## Task 9: Full verification

**Files:** none (verification only)

- [ ] **Step 1: Core tests**

Run: `cargo test -p restee-core`
Expected: all green, including the new `quotes::tests`.

- [ ] **Step 2: Lint**

Run: `cargo clippy --workspace --all-targets`
Expected: no new warnings in `quotes.rs` / touched files.

- [ ] **Step 3: Release build (embedded assets)**

Run: `cargo build --release --features custom-protocol`
Expected: builds. (Stop any running `restee.exe` first — a running tray instance locks the exe; `Stop-Process -Name restee -Force` if needed.)

- [ ] **Step 4: Manual migration check** (the real config dir currently has `quotes.en.txt` + `quotes.zh-Hant.txt`, no `quotes.txt`)

Run the built `target\release\restee.exe`, then inspect `%APPDATA%\com.restee.app\`:
- Expected: `quotes.toml` now exists with `en` + `zh-Hant` arrays matching the old `.txt` contents.
- Expected: `quotes.en.txt` and `quotes.zh-Hant.txt` are **gone** (deleted by migration).
- Trigger a break (debug builds: `RESTEE_BREAK_ON_START=1`) with `show_quotes` on and confirm a quote shows on the overlay.
- Open Settings → Quotes, toggle locale, add/remove a quote, Save, reopen — confirm the edit round-trips and the other locale is intact (load-modify-write).

- [ ] **Step 5: Final confirmation**

No commit needed (verification only). If any step fails, return to the relevant task; do not mark the plan complete with failing checks.

---

## Self-Review notes (author)

- **Spec coverage:** model+sanitize (T1), embedded default (T2), save/read (T3), migration+delete (T4), self-healing load (T5), host pick + AppState + commands/runtime/lib wiring with preserved signatures (T6), `.txt` removal + stale comments incl. frontend ones from Codex (T7), CLAUDE.md (T8), build/lint/manual-migration verify (T9). Codex correctness fix (corrupt-recovery never re-reads `.txt`) is encoded in T5 Step 1 + tested in `load_corrupt_backs_up_and_reseeds_from_embedded_not_txt`. Locale-isolation (saving one locale preserves the other — a spec requirement) is automated in T5 `save_one_locale_preserves_the_other` (not just the T9 manual check).
- **Type consistency:** `QuotesFile`, `get(&str)->&[String]`, `set(&str, Vec<String>)`, `sanitize()->bool`, `read_quotes(&Path)->QuotesFile`, `save_quotes(&Path,&QuotesFile)->io::Result`, `load_quotes(&Path)->io::Result<QuotesFile>`, `pick(&Path,&str)->Option<String>` used consistently across tasks.
- **No placeholders:** every code/test/command step is concrete.
