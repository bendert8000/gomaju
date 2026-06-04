//! User-editable break quotes, one set per UI locale.
//!
//! Each locale keeps its own plain-text file next to `config.toml`: `quotes.<locale>.txt`
//! (`quotes.en.txt`, `quotes.zh-Hant.txt`) — one quote per line; blank lines and `#`-comment
//! lines are ignored. Each is **seeded once** from an embedded per-locale default so a fresh
//! install isn't empty, and **never overwritten** afterward (it's the user's file). The file for
//! the *active* locale is re-read on every break (it's tiny), so edits take effect live, and there
//! is **no cross-locale fallback** — an empty active set shows no quote. Everything here is
//! best-effort: any failure simply yields no quote.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Per-locale default quotes; written to the matching file on first run only.
const DEFAULT_QUOTES_EN: &str = include_str!("../default_quotes.en.txt");
const DEFAULT_QUOTES_ZH: &str = include_str!("../default_quotes.zh-Hant.txt");

/// The supported quote locales paired with their embedded defaults (seed source + the canonical
/// file-name set, so no other `quotes.*.txt` is ever created).
const SEEDS: [(&str, &str); 2] = [("en", DEFAULT_QUOTES_EN), ("zh-Hant", DEFAULT_QUOTES_ZH)];

/// Legacy single-file name from before quotes were localized. Migrated into the English set on
/// first run of this version (the old default seed was English).
const LEGACY_FILE_NAME: &str = "quotes.txt";

/// Canonicalize a config locale to one of the two supported quote locales: anything that isn't
/// exactly `"en"` maps to `"zh-Hant"` (the default), matching `i18n::pick`. This also keeps the
/// locale safe to embed in a file name — only the two known names are ever produced, so a
/// frontend-supplied locale can't escape the config dir.
fn canonical_locale(locale: &str) -> &'static str {
    if locale == "en" {
        "en"
    } else {
        "zh-Hant"
    }
}

fn quotes_path(config_dir: &Path, locale: &str) -> PathBuf {
    config_dir.join(format!("quotes.{}.txt", canonical_locale(locale)))
}

/// Parse quotes-file contents into trimmed, non-empty, non-comment lines.
pub fn parse(contents: &str) -> Vec<String> {
    contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// Normalize an edited quote list for storage: trim each, drop empty and comment (`#`-leading)
/// lines. This mirrors what `parse` keeps on read, so a saved list reloads identically
/// (idempotent: `sanitize(sanitize(x)) == sanitize(x)`). A quote that would read as a comment
/// is dropped here rather than silently vanishing on the next break's re-read.
pub fn sanitize(quotes: &[String]) -> Vec<String> {
    quotes
        .iter()
        .map(|q| q.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// Seed each locale's quotes file from its embedded default if it doesn't exist yet, and migrate a
/// legacy pre-localization `quotes.txt` into the English set (the old default seed was English).
/// All best-effort.
pub fn seed_if_missing(config_dir: &Path) {
    let _ = std::fs::create_dir_all(config_dir);

    // Migrate the legacy single file into the English set, if present and not already migrated.
    let legacy = config_dir.join(LEGACY_FILE_NAME);
    let en_path = quotes_path(config_dir, "en");
    if legacy.exists() && !en_path.exists() {
        match std::fs::read_to_string(&legacy).and_then(|text| std::fs::write(&en_path, text)) {
            Ok(()) => eprintln!("restee: migrated quotes.txt -> {}", en_path.display()),
            Err(e) => eprintln!("restee: could not migrate quotes.txt ({e})"),
        }
    }

    for (locale, default) in SEEDS {
        let path = quotes_path(config_dir, locale);
        if path.exists() {
            continue;
        }
        match std::fs::write(&path, default) {
            Ok(()) => eprintln!("restee: seeded {}", path.display()),
            Err(e) => eprintln!("restee: could not seed {} ({e})", path.display()),
        }
    }
}

/// Persist a locale's quotes atomically: write a temp file in the same dir, then rename over
/// `quotes.<locale>.txt` — mirroring `restee_core::config::save`, so a failed write never truncates
/// the live file. Writes one quote per line with a trailing newline (an empty list yields an empty
/// file). The caller is expected to pass an already-`sanitize`d list (the command does this).
pub fn save(config_dir: &Path, locale: &str, quotes: &[String]) -> std::io::Result<()> {
    std::fs::create_dir_all(config_dir)?;
    let path = quotes_path(config_dir, locale);
    let mut text = quotes.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    let tmp = path.with_extension("txt.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Load a locale's quotes (parsed). Empty if the file is missing/empty/unreadable.
pub fn load(config_dir: &Path, locale: &str) -> Vec<String> {
    match std::fs::read_to_string(quotes_path(config_dir, locale)) {
        Ok(text) => parse(&text),
        Err(_) => Vec::new(),
    }
}

/// Pick one quote (pseudo-randomly) from the active locale's file. `None` if that locale has no
/// quotes — there is no cross-locale fallback, so an empty active set shows no quote.
pub fn pick(config_dir: &Path, locale: &str) -> Option<String> {
    let mut quotes = load(config_dir, locale);
    if quotes.is_empty() {
        return None;
    }
    let idx = pseudo_random_index(quotes.len());
    Some(quotes.swap_remove(idx))
}

/// A throwaway index in `0..len`, seeded from the wall clock. The host (not the pure engine)
/// may use the clock; a clock error just falls back to the first quote.
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

    #[test]
    fn parse_skips_blank_and_comment_lines_and_trims() {
        let input = "\n# a comment\n  Rest well.  \n\n#another\nStretch often.\n";
        assert_eq!(
            parse(input),
            vec!["Rest well.".to_string(), "Stretch often.".to_string()]
        );
    }

    #[test]
    fn parse_empty_or_all_comments_is_empty() {
        assert!(parse("").is_empty());
        assert!(parse("#only comment\n   \n").is_empty());
    }

    #[test]
    fn embedded_default_quotes_are_nonempty_for_both_locales() {
        assert!(
            !parse(DEFAULT_QUOTES_EN).is_empty(),
            "shipped default_quotes.en.txt should contain quotes"
        );
        assert!(
            !parse(DEFAULT_QUOTES_ZH).is_empty(),
            "shipped default_quotes.zh-Hant.txt should contain quotes"
        );
    }

    #[test]
    fn canonical_locale_maps_unknown_to_default() {
        assert_eq!(canonical_locale("en"), "en");
        assert_eq!(canonical_locale("zh-Hant"), "zh-Hant");
        assert_eq!(canonical_locale("fr"), "zh-Hant");
        assert_eq!(canonical_locale(""), "zh-Hant");
        assert_eq!(canonical_locale("../escape"), "zh-Hant");
    }

    #[test]
    fn pseudo_random_index_is_in_range() {
        for len in 1..=10usize {
            assert!(pseudo_random_index(len) < len);
        }
    }

    #[test]
    fn sanitize_trims_and_drops_blank_and_comment_lines() {
        let input = vec![
            "  Rest well.  ".to_string(),
            String::new(),
            "   ".to_string(),
            "# a comment".to_string(),
            "Stretch often.".to_string(),
        ];
        assert_eq!(
            sanitize(&input),
            vec!["Rest well.".to_string(), "Stretch often.".to_string()]
        );
    }

    #[test]
    fn sanitize_is_idempotent() {
        let input = vec![
            "  Hi  ".to_string(),
            "# c".to_string(),
            "There".to_string(),
        ];
        let once = sanitize(&input);
        assert_eq!(sanitize(&once), once);
    }

    #[test]
    fn save_then_load_round_trips_sanitized_per_locale() {
        let dir = std::env::temp_dir().join("restee-quotes-roundtrip-test");
        let _ = std::fs::remove_dir_all(&dir);
        let input = vec![
            "  Take a breath.  ".to_string(),
            String::new(),
            "# header".to_string(),
            "Look away.".to_string(),
        ];
        let clean = sanitize(&input);
        save(&dir, "en", &clean).expect("save should succeed");
        assert_eq!(load(&dir, "en"), clean);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn locales_have_independent_files() {
        let dir = std::env::temp_dir().join("restee-quotes-locale-isolation-test");
        let _ = std::fs::remove_dir_all(&dir);
        let en = vec!["English quote.".to_string()];
        let zh = vec!["中文語錄。".to_string()];
        save(&dir, "en", &en).unwrap();
        save(&dir, "zh-Hant", &zh).unwrap();
        assert_eq!(load(&dir, "en"), en);
        assert_eq!(load(&dir, "zh-Hant"), zh);
        // An unknown locale canonicalizes to zh-Hant, so it reads the Chinese file.
        assert_eq!(load(&dir, "fr"), zh);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_empty_list_yields_no_quotes() {
        let dir = std::env::temp_dir().join("restee-quotes-empty-test");
        let _ = std::fs::remove_dir_all(&dir);
        save(&dir, "en", &[]).expect("save should succeed");
        assert!(load(&dir, "en").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn seed_migrates_legacy_quotes_into_english_and_seeds_chinese() {
        let dir = std::env::temp_dir().join("restee-quotes-migrate-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("quotes.txt"), "Legacy line.\n").unwrap();

        seed_if_missing(&dir);

        assert_eq!(load(&dir, "en"), vec!["Legacy line.".to_string()]);
        assert!(!load(&dir, "zh-Hant").is_empty()); // seeded from the embedded default
        let _ = std::fs::remove_dir_all(&dir);
    }
}
