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

    #[test]
    fn embedded_default_quotes_parse_and_are_clean() {
        let mut f: QuotesFile =
            toml::from_str(DEFAULT_QUOTES_TOML).expect("default_quotes.toml must parse");
        assert!(!f.en.is_empty(), "default en quotes should be non-empty");
        assert!(!f.zh_hant.is_empty(), "default zh-Hant quotes should be non-empty");
        // The shipped default must already be valid — sanitize should change nothing.
        assert!(!f.sanitize());
    }

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
}
