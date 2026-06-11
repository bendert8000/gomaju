//! Persisted break-timer progress (`session.toml`), so per-rule countdowns survive an app
//! restart — including a forced/ungraceful kill (e.g. a Windows-Update reboot), which is the
//! whole point: the host autosaves frequently so progress isn't tied to a clean exit.
//!
//! Like `quotes.rs` / `chime.rs` this module is dependency-free (serde/toml only): it owns the
//! DTO, an atomic save, and a best-effort read. The host (Tauri layer) owns the wall-clock
//! `saved_at` timestamp, the autosave cadence, and the cold-start "resume vs start fresh" prompt;
//! the engine owns `snapshot_progress` / `restore_progress`.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Current `session.toml` schema version (kept in sync with the host writer).
pub const PROGRESS_VERSION: u32 = 1;

/// One rule's accumulated active work, keyed by its stable rule id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleProgress {
    pub rule_id: String,
    pub work_secs: u64,
}

/// The on-disk `session.toml`: every rule's accumulated work plus when it was saved.
///
/// `deny_unknown_fields` is intentionally NOT set and every field has a serde default, so a
/// forward/older file (e.g. missing `version`) still parses rather than being discarded.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressFile {
    /// Schema version. Defaults to 0 for a pre-versioning file, which the host still treats as
    /// readable (it re-saves with the current `PROGRESS_VERSION`).
    #[serde(default)]
    pub version: u32,
    /// Wall-clock save time as a **UTC** Unix timestamp (seconds), set by the host. The host
    /// rejects stale or future-dated snapshots when deciding whether to prompt.
    #[serde(default)]
    pub saved_at: i64,
    #[serde(default)]
    pub rules: Vec<RuleProgress>,
}

/// Atomically write `session.toml` (temp + rename), creating its parent dir. Mirrors
/// `quotes::save_quotes` / `config::save`, so a failed write never truncates the live file.
pub fn save_progress(path: &Path, file: &ProgressFile) -> std::io::Result<()> {
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

/// Best-effort read used at cold start: parse `session.toml`, returning `None` if it is missing
/// or unparseable (a missing file is the normal first-run case — never write or panic here).
pub fn read_progress(path: &Path) -> Option<ProgressFile> {
    let text = fs::read_to_string(path).ok()?;
    toml::from_str::<ProgressFile>(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("gomaju-progress-{name}"));
        let _ = fs::create_dir_all(&dir);
        dir.join("session.toml")
    }

    #[test]
    fn save_then_read_round_trips() {
        let path = temp_path("roundtrip");
        let file = ProgressFile {
            version: PROGRESS_VERSION,
            saved_at: 1_700_000_000,
            rules: vec![
                RuleProgress { rule_id: "eye".into(), work_secs: 120 },
                RuleProgress { rule_id: "stretch".into(), work_secs: 0 },
            ],
        };
        save_progress(&path, &file).unwrap();
        assert_eq!(read_progress(&path).unwrap(), file);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn missing_file_reads_none() {
        let path = temp_path("missing");
        let _ = fs::remove_file(&path);
        assert!(read_progress(&path).is_none());
    }

    #[test]
    fn corrupt_file_reads_none() {
        let path = temp_path("corrupt");
        fs::write(&path, "this is { not valid toml").unwrap();
        assert!(read_progress(&path).is_none());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn missing_version_and_unknown_fields_tolerated() {
        let path = temp_path("partial");
        // No `version`, plus an unknown top-level field: serde defaults + ignores unknown.
        fs::write(
            &path,
            "saved_at = 42\nextra = \"ignored\"\n[[rules]]\nrule_id = \"a\"\nwork_secs = 7\n",
        )
        .unwrap();
        let read = read_progress(&path).expect("should parse");
        assert_eq!(read.version, 0);
        assert_eq!(read.saved_at, 42);
        assert_eq!(read.rules, vec![RuleProgress { rule_id: "a".into(), work_secs: 7 }]);
        let _ = fs::remove_file(&path);
    }
}
