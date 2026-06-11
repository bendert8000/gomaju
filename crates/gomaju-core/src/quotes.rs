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

/// The starter quotes a fresh install is seeded with — editable TOML, embedded at compile time, so
/// `quotes.toml` isn't empty on first run (and is the corrupt-recovery source). The
/// `embedded_default_quotes()` parser that consumes it is introduced in Task 4, where it's first used.
pub const DEFAULT_QUOTES_TOML: &str = include_str!("../default_quotes.toml");

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

/// Parse the embedded `default_quotes.toml` into a `QuotesFile` (first-run seed + corrupt-recovery
/// source). Introduced here because this is its first non-test use (the migration + `load_quotes`).
fn embedded_default_quotes() -> QuotesFile {
    toml::from_str(DEFAULT_QUOTES_TOML).expect("embedded default_quotes.toml must parse")
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
                Ok(()) => eprintln!("gomaju: removed migrated quote file {}", p.display()),
                Err(e) => eprintln!("gomaju: could not remove {} ({e})", p.display()),
            }
        }
    }
}

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
            // Best-effort re-persist: the returned value is always sanitized even if this write
            // fails (the next startup re-sanitizes and retries). Mirrors `chime::load_chimes`.
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
            en: vec![
                "  Rest.  ".into(),
                String::new(),
                "# c".into(),
                "Stretch.".into(),
            ],
            zh_hant: vec!["  休息。 ".into(), "   ".into()],
        };
        assert!(f.sanitize());
        assert_eq!(f.en, ["Rest.", "Stretch."]);
        assert_eq!(f.zh_hant, ["休息。"]);
    }

    #[test]
    fn sanitize_is_idempotent_and_reports_no_change_when_clean() {
        let mut f = QuotesFile {
            en: vec!["Rest.".into()],
            zh_hant: vec!["休息。".into()],
        };
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

    #[test]
    fn embedded_default_quotes_parse_and_are_clean() {
        let mut f: QuotesFile =
            toml::from_str(DEFAULT_QUOTES_TOML).expect("default_quotes.toml must parse");
        assert!(!f.en.is_empty(), "default en quotes should be non-empty");
        assert!(
            !f.zh_hant.is_empty(),
            "default zh-Hant quotes should be non-empty"
        );
        // The shipped default must already be valid — sanitize should change nothing.
        assert!(!f.sanitize());
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("gomaju-quotes-{name}"));
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
        let f = QuotesFile {
            en: vec!["Rest.".into()],
            zh_hant: vec!["休息。".into()],
        };
        save_quotes(&path, &f).unwrap();
        let before = fs::read_to_string(&path).unwrap();
        assert_eq!(load_quotes(&path).unwrap(), f);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            before,
            "clean load must not rewrite"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_dirty_valid_file_is_rewritten() {
        // A valid TOML whose entries need sanitizing: load_quotes returns the clean value AND
        // re-persists it, so the dirty content is gone from disk (the `if file.sanitize()` arm).
        let dir = temp_dir("load-dirty");
        let path = dir.join("quotes.toml");
        fs::write(&path, "en = [\"  Rest.  \", \"# c\", \"Stretch.\"]\n").unwrap();
        let f = load_quotes(&path).unwrap();
        assert_eq!(f.en, ["Rest.", "Stretch."]);
        // The on-disk file was rewritten clean — the dirty markers are gone.
        let on_disk = fs::read_to_string(&path).unwrap();
        assert!(!on_disk.contains("# c"));
        assert!(!on_disk.contains("  Rest.  "));
        // Reloading yields the same clean value (now no rewrite needed).
        assert_eq!(load_quotes(&path).unwrap(), f);
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
            &QuotesFile {
                en: vec!["old en".into()],
                zh_hant: vec!["保留中文".into()],
            },
        )
        .unwrap();
        // Load full file, replace only en, sanitize, write — exactly what the command does.
        let mut file = load_quotes(&path).unwrap();
        file.set("en", vec!["new en".into()]);
        file.sanitize();
        save_quotes(&path, &file).unwrap();
        let reloaded = load_quotes(&path).unwrap();
        assert_eq!(reloaded.en, ["new en"]);
        assert_eq!(
            reloaded.zh_hant,
            ["保留中文"],
            "the other locale must be preserved"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
