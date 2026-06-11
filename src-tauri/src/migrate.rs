//! One-time migration from the pre-rename identifier (`com.restee.app` → `com.gomaju.app`).
//!
//! Renaming the Tauri bundle identifier moves the OS config dir, which would otherwise orphan an
//! existing user's settings/alarms/chimes/quotes. On first run under the new identifier we copy the
//! old dir's contents across (best-effort), and clean up the now-dead autostart entry the old
//! "Restee" app registered. This is the only place the legacy name intentionally survives.

use std::path::Path;

/// The pre-rename bundle identifier — its OS config dir is the sibling of the new one.
const LEGACY_IDENTIFIER: &str = "com.restee.app";
/// The pre-rename app name — the value the old autostart entry was registered under.
#[cfg_attr(target_os = "macos", allow(dead_code))]
const LEGACY_APP_NAME: &str = "Restee";

/// If this is a fresh launch under the new identifier (`new_dir` has no `config.toml`) but the old
/// identifier's dir does, copy the user's data across. **`config.toml` is copied LAST** so the
/// "already migrated?" guard (which keys off `config.toml`) is effectively transactional: a partial
/// copy leaves no `config.toml`, so the next launch retries instead of locking in half the data.
///
/// Best-effort throughout — any failure just means the app starts from defaults (`config::load`
/// writes them). Runs before `logging::init`, so `rlog!` here is stderr-only (which is fine).
pub fn from_legacy_identifier(new_dir: &Path) {
    // A config already exists under the new id → normal launch or prior migration; nothing to do.
    if new_dir.join("config.toml").exists() {
        return;
    }
    let Some(old_dir) = new_dir.parent().map(|p| p.join(LEGACY_IDENTIFIER)) else {
        return;
    };
    // Don't migrate onto ourselves, and only when the old install actually has a config.
    if old_dir == *new_dir || !old_dir.join("config.toml").exists() {
        return;
    }

    crate::rlog!(
        "gomaju: migrating data from legacy {} -> {}",
        old_dir.display(),
        new_dir.display()
    );
    if let Err(e) = std::fs::create_dir_all(new_dir) {
        crate::rlog!("gomaju: migration could not create new config dir ({e})");
        return;
    }

    // Everything EXCEPT config.toml first; config.toml last (it is the "complete" marker).
    copy_file(&old_dir, new_dir, "quotes.toml");
    copy_file(&old_dir, new_dir, "session.toml");
    // Legacy plain-text quote files, in case the old install never upgraded to quotes.toml —
    // load_quotes can still migrate these once they sit beside the new dir.
    copy_file(&old_dir, new_dir, "quotes.en.txt");
    copy_file(&old_dir, new_dir, "quotes.zh-Hant.txt");
    copy_file(&old_dir, new_dir, "quotes.txt");
    // Saved chimes live in chimes/ (chimes.toml + imported audio files) — copy the whole tree.
    copy_dir(&old_dir.join("chimes"), &new_dir.join("chimes"));
    // config.toml LAST — its presence is what makes the guard above transactional.
    copy_file(&old_dir, new_dir, "config.toml");

    cleanup_legacy_autostart();
}

/// Copy `old_dir/name` → `new_dir/name` if the source exists. Best-effort.
fn copy_file(old_dir: &Path, new_dir: &Path, name: &str) {
    let src = old_dir.join(name);
    if !src.exists() {
        return;
    }
    if let Err(e) = std::fs::copy(&src, new_dir.join(name)) {
        crate::rlog!("gomaju: migration failed to copy {name} ({e})");
    }
}

/// Recursively copy directory `src` into `dst` (creating `dst`). Best-effort; no-op if `src` is
/// absent. Used for the `chimes/` folder, which holds `chimes.toml` plus imported audio files.
fn copy_dir(src: &Path, dst: &Path) {
    if !src.is_dir() {
        return;
    }
    if let Err(e) = std::fs::create_dir_all(dst) {
        crate::rlog!("gomaju: migration could not create {} ({e})", dst.display());
        return;
    }
    let entries = match std::fs::read_dir(src) {
        Ok(e) => e,
        Err(e) => {
            crate::rlog!("gomaju: migration could not read {} ({e})", src.display());
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &target);
        } else if let Err(e) = std::fs::copy(&path, &target) {
            crate::rlog!("gomaju: migration failed to copy {} ({e})", path.display());
        }
    }
}

/// Best-effort removal of the autostart entry the *old* app registered, so it can't fire a dead
/// path or double-launch alongside Gomaju. Platform-specific; failures are ignored. The normal
/// `autostart::apply` re-registers the new `Gomaju` entry from the migrated setting.
fn cleanup_legacy_autostart() {
    // Never touch the real registry / login items from a unit test.
    if cfg!(test) {
        return;
    }
    #[cfg(windows)]
    {
        // tauri-plugin-autostart stores an HKCU\...\Run value named after the app (LEGACY_APP_NAME).
        let _ = std::process::Command::new("reg")
            .args([
                "delete",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                LEGACY_APP_NAME,
                "/f",
            ])
            .output();
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            let plist = Path::new(&home)
                .join("Library/LaunchAgents")
                .join(format!("{LEGACY_IDENTIFIER}.plist"));
            let _ = std::fs::remove_file(plist);
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(home) = std::env::var_os("HOME") {
            let desktop = Path::new(&home)
                .join(".config/autostart")
                .join(format!("{LEGACY_APP_NAME}.desktop"));
            let _ = std::fs::remove_file(desktop);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn tmp_root(name: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("gomaju-migtest-{}-{n}-{name}", std::process::id()))
    }

    #[test]
    fn migrates_legacy_data_into_an_empty_new_dir() {
        let root = tmp_root("ok");
        let old = root.join(LEGACY_IDENTIFIER);
        let new = root.join("com.gomaju.app");
        std::fs::create_dir_all(old.join("chimes")).unwrap();
        std::fs::write(old.join("config.toml"), b"locale = \"en\"\n").unwrap();
        std::fs::write(old.join("quotes.toml"), b"en = []\n").unwrap();
        std::fs::write(old.join("session.toml"), b"version = 1\n").unwrap();
        std::fs::write(old.join("chimes").join("chimes.toml"), b"version = 1\n").unwrap();
        std::fs::write(old.join("chimes").join("imported.wav"), b"RIFFfake").unwrap();

        from_legacy_identifier(&new);

        assert!(new.join("config.toml").exists(), "config.toml migrated");
        assert!(new.join("quotes.toml").exists(), "quotes.toml migrated");
        assert!(new.join("session.toml").exists(), "session.toml migrated");
        assert!(new.join("chimes").join("chimes.toml").exists(), "chimes.toml migrated");
        assert!(
            new.join("chimes").join("imported.wav").exists(),
            "imported chime audio migrated (recursive chimes/ copy)"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn is_a_noop_when_new_config_already_exists() {
        let root = tmp_root("exists");
        let old = root.join(LEGACY_IDENTIFIER);
        let new = root.join("com.gomaju.app");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(old.join("config.toml"), b"locale = \"en\"\n").unwrap();
        std::fs::write(old.join("quotes.toml"), b"en = []\n").unwrap();
        std::fs::write(new.join("config.toml"), b"locale = \"zh-Hant\"\n").unwrap();

        from_legacy_identifier(&new);

        assert_eq!(
            std::fs::read_to_string(new.join("config.toml")).unwrap(),
            "locale = \"zh-Hant\"\n",
            "an existing new config is never overwritten"
        );
        assert!(
            !new.join("quotes.toml").exists(),
            "nothing is pulled over once the new config exists (idempotent)"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn is_a_noop_when_there_is_no_legacy_dir() {
        let root = tmp_root("nolegacy");
        let new = root.join("com.gomaju.app");
        std::fs::create_dir_all(&new).unwrap();

        from_legacy_identifier(&new); // no sibling com.restee.app at all

        assert!(!new.join("config.toml").exists(), "nothing to migrate");
        let _ = std::fs::remove_dir_all(&root);
    }
}
