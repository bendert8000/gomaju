//! Host-side break-quote selection. The TOML model, validation, storage, and `.txt` migration live
//! in `gomaju_core::quotes` (pure + unit-tested). This module keeps only the wall-clock random pick
//! — the pure engine is clock-free, so the host owns anything that reads the clock.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Pick one quote (pseudo-randomly) from the active locale in `quotes.toml`. `None` if that locale
/// has no quotes — there is no cross-locale fallback. Re-reads the file each break (no cache), so
/// edits take effect live; the read never writes.
pub fn pick(quotes_path: &Path, locale: &str) -> Option<String> {
    let file = gomaju_core::quotes::read_quotes(quotes_path);
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
    use gomaju_core::quotes::{save_quotes, QuotesFile};

    fn temp_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("gomaju-host-quotes-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("quotes.toml")
    }

    #[test]
    fn pick_returns_a_quote_from_the_active_locale() {
        let path = temp_path("pick-active");
        save_quotes(
            &path,
            &QuotesFile {
                en: vec!["only-en".into()],
                zh_hant: vec!["only-zh".into()],
            },
        )
        .unwrap();
        assert_eq!(pick(&path, "en").as_deref(), Some("only-en"));
        assert_eq!(pick(&path, "zh-Hant").as_deref(), Some("only-zh"));
    }

    #[test]
    fn pick_none_when_active_locale_empty_no_cross_fallback() {
        let path = temp_path("pick-empty");
        save_quotes(
            &path,
            &QuotesFile {
                en: vec![],
                zh_hant: vec!["有中文".into()],
            },
        )
        .unwrap();
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
