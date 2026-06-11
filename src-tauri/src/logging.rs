//! Tiny zero-dependency file logger.
//!
//! The app's diagnostics have always gone to stderr via `eprintln!`, which is invisible once the
//! app is installed (no attached console). This module tees every `gomaju:` line to a **rolling log
//! file** next to `config.toml`, so field issues (audio-device failures, config/quote/chime write
//! failures, notification failures, once-disable persistence, …) leave a trace a user can send.
//!
//! Best-effort by design: logging never fails the app, and until [`init`] runs (e.g. in unit tests)
//! the [`rlog!`](crate::rlog) macro simply behaves like `eprintln!`. Stderr output is unchanged, so
//! `tauri dev` looks exactly as before.

use std::borrow::Cow;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Where the log file lives (set once at startup). `None` until [`init`] runs, in which case
/// [`write_line`] is a no-op and `rlog!` only writes to stderr.
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
/// Serializes appends so interleaved lines from the ticker / audio / scheduler threads stay whole.
static LOG_LOCK: Mutex<()> = Mutex::new(());

/// Size cap: at startup, a `gomaju.log` larger than this is rotated aside so it can't grow
/// without bound on a long-running install (see [`init`] / [`rotate_if_oversized`]).
const MAX_LOG_BYTES: u64 = 1_000_000;

/// Point the logger at `<config_dir>/gomaju.log`, first rotating an over-cap file to
/// `gomaju.log.old` (single-generation rotation). Call once at startup, after the config dir
/// exists. Best-effort.
pub fn init(config_dir: &Path) {
    let path = config_dir.join("gomaju.log");
    rotate_if_oversized(&path, config_dir);
    let _ = LOG_PATH.set(path);
}

/// Rotate `gomaju.log` aside to `gomaju.log.old` when it has grown past [`MAX_LOG_BYTES`]
/// (single-generation rotation). Pure filesystem work with no global state, so it's exercised
/// directly in tests — unlike [`init`], which also sets the process-global [`LOG_PATH`].
/// Best-effort: a missing file or a failed rename is silently ignored.
fn rotate_if_oversized(log_path: &Path, config_dir: &Path) {
    if std::fs::metadata(log_path)
        .map(|m| m.len() > MAX_LOG_BYTES)
        .unwrap_or(false)
    {
        let _ = std::fs::rename(log_path, config_dir.join("gomaju.log.old"));
    }
}

/// Append one timestamped line to the log file. Best-effort, and used only by the `rlog!` macro —
/// call the macro, not this. No-op until [`init`] has set the path.
pub fn write_line(line: &str) {
    let Some(path) = LOG_PATH.get() else {
        return;
    };
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    // Collapse embedded newlines so a value carried into the message (e.g. a user-set alarm name)
    // can't forge extra log lines — one diagnostic stays one physical line. Stderr (in the macro)
    // still receives the unescaped text.
    let body = if line.contains(['\n', '\r']) {
        Cow::Owned(line.replace('\r', "").replace('\n', "⏎"))
    } else {
        Cow::Borrowed(line)
    };
    // Recover a poisoned lock: a logging panic must never wedge logging for the rest of the run.
    let _guard = LOG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{stamp} {body}");
    }
}

/// Tee a `gomaju:`-style diagnostic to **both** stderr (unchanged, so `tauri dev` looks the same)
/// and the rolling log file. Drop-in for `eprintln!`: same format args, plus the file sink.
#[macro_export]
macro_rules! rlog {
    ($($arg:tt)*) => {{
        let line = ::std::format!($($arg)*);
        ::std::eprintln!("{line}");
        $crate::logging::write_line(&line);
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    // `LOG_PATH` is a process-global OnceLock, and no other unit test triggers logging, so this is
    // the only test that calls `init` — it reliably wins the `set`.
    #[test]
    fn init_then_write_line_appends_a_timestamped_line() {
        let dir = std::env::temp_dir().join(format!("gomaju-logtest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        init(&dir);

        write_line("gomaju: hello from the logger test");
        // A value with an embedded newline must not forge a second physical line.
        write_line("gomaju: line one\ngomaju: forged line two");
        let contents =
            std::fs::read_to_string(dir.join("gomaju.log")).expect("the log file was written");

        assert!(
            contents.contains("gomaju: hello from the logger test"),
            "the message reached the file, got: {contents}"
        );
        assert!(
            contents.contains("gomaju: line one⏎gomaju: forged line two"),
            "embedded newlines are collapsed, not passed through, got: {contents}"
        );
        // The embedded newline was collapsed (not passed through), so the forged tail never
        // becomes its own physical line. Asserted structurally rather than by total line count, so a
        // stray log line from another test (logging shares a process-global path) can't flake this.
        assert!(
            !contents.lines().any(|l| l == "gomaju: forged line two"),
            "embedded newline must be collapsed, not split into its own line, got: {contents}"
        );
        // Each line is timestamped: it begins with a 4-digit year.
        assert!(
            contents.trim_start().chars().take(4).all(|c| c.is_ascii_digit()),
            "line should start with a YYYY timestamp, got: {contents}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotate_if_oversized_moves_an_over_cap_log_to_old() {
        let dir = std::env::temp_dir().join(format!("gomaju-rotatetest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let log = dir.join("gomaju.log");

        // A sparse file just past the cap — no need to actually write a megabyte.
        let f = std::fs::File::create(&log).expect("create log");
        f.set_len(MAX_LOG_BYTES + 1).expect("grow past the cap");
        drop(f);

        rotate_if_oversized(&log, &dir);

        assert!(
            dir.join("gomaju.log.old").exists(),
            "an over-cap log is rotated aside to gomaju.log.old"
        );
        assert!(!log.exists(), "the over-cap gomaju.log was renamed away");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotate_if_oversized_leaves_a_small_log_in_place() {
        let dir =
            std::env::temp_dir().join(format!("gomaju-rotatetest-small-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let log = dir.join("gomaju.log");
        std::fs::write(&log, b"one small line\n").expect("write small log");

        rotate_if_oversized(&log, &dir);

        assert!(log.exists(), "an under-cap log is left untouched");
        assert!(
            !dir.join("gomaju.log.old").exists(),
            "nothing is rotated when under the cap"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
