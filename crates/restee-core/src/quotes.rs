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

#[allow(unused_imports)]
use std::fs;
#[allow(unused_imports)]
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
}
