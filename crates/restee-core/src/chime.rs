//! User-defined chimes: named sound presets used for break and alarm cues.
//!
//! A chime is either a **synthesized** tone sequence (`kind = "tones"`, played from `steps`) or a
//! reference to an **imported** audio file (`kind = "file"`, the bare `file` name under the app's
//! `chimes/` dir). Like the rest of `restee-core`, this module is dependency-free (serde/toml only)
//! and has no audio backend — it owns the DTOs and a [`sanitize_chimes`] validation pass; the host
//! (Tauri layer) turns a [`ChimeDto`] into actual sound via rodio.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Upper bounds, so a hand-edited or malformed chime can never produce a runaway/painful sound.
const MAX_FREQ_HZ: u32 = 20_000;
const MAX_STEP_MS: u32 = 10_000;
const MIN_STEP_MS: u32 = 1;
const MAX_STEPS: usize = 64;

/// One step of a synthesized chime: a tone (or silence, when `freq_hz == 0`) for `duration_ms`.
/// Loudness is supplied by the rule/alarm picker that plays the chime, not by the chime itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToneStep {
    /// Frequency in Hz; `0` = silence (a gap between tones).
    pub freq_hz: u32,
    /// Duration of this step in milliseconds.
    pub duration_ms: u32,
    /// Fade-in over the first `fade_in_ms` of the step (softens the attack). Capped at the step length.
    #[serde(default)]
    pub fade_in_ms: u32,
}

/// How a chime produces sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChimeKindDto {
    /// Synthesized from `steps`.
    Tones,
    /// Plays the imported audio `file`.
    File,
}

/// A named chime preset. Scalars are listed before `steps` so the serialized TOML keeps the
/// `[[chimes.steps]]` array-of-tables last (TOML requires scalars before nested tables/arrays).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChimeDto {
    pub id: String,
    pub name: String,
    pub kind: ChimeKindDto,
    /// File chimes only: the **bare** filename under the app's `chimes/` dir (no path components).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub file: String,
    /// Tone chimes only: the sequence of tone steps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<ToneStep>,
}

/// Whether `name` is a safe **bare** filename to store/read under the chimes dir: non-empty, with
/// no path separators, no parent-dir (`..`) component, not absolute, and no drive/NUL characters.
/// The host must still join it only under `<config_dir>/chimes/` — never trust a raw path.
pub fn is_safe_filename(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\0')
        && !name.contains(':') // drive letters / NTFS alternate data streams
        && !name.contains("..")
}

fn sanitize_step(s: &mut ToneStep) -> bool {
    let mut changed = false;
    if s.freq_hz > MAX_FREQ_HZ {
        s.freq_hz = MAX_FREQ_HZ;
        changed = true;
    }
    let dur = s.duration_ms.clamp(MIN_STEP_MS, MAX_STEP_MS);
    if dur != s.duration_ms {
        s.duration_ms = dur;
        changed = true;
    }
    if s.fade_in_ms > s.duration_ms {
        s.fade_in_ms = s.duration_ms;
        changed = true;
    }
    changed
}

/// Validate + clamp chimes in place. Drops invalid chimes — empty or duplicate `id`, a `tones`
/// chime with no steps, or a `file` chime whose filename isn't a safe bare name. Clamps each tone
/// step into safe ranges and caps the step count. Returns whether anything changed (so the caller
/// can persist the corrected file). Mirrors `alarm::sanitize_alarms` / `config::sanitize_rules`.
pub fn sanitize_chimes(chimes: &mut Vec<ChimeDto>) -> bool {
    let before = chimes.len();
    let mut changed = false;
    let mut seen: HashSet<String> = HashSet::new();

    chimes.retain_mut(|c| {
        if c.id.trim().is_empty() || !seen.insert(c.id.clone()) {
            return false; // empty or duplicate id
        }
        match c.kind {
            ChimeKindDto::Tones => {
                if !c.file.is_empty() {
                    c.file.clear();
                    changed = true;
                }
                if c.steps.len() > MAX_STEPS {
                    c.steps.truncate(MAX_STEPS);
                    changed = true;
                }
                for step in &mut c.steps {
                    if sanitize_step(step) {
                        changed = true;
                    }
                }
                !c.steps.is_empty() // a tones chime needs at least one step
            }
            ChimeKindDto::File => {
                if !c.steps.is_empty() {
                    c.steps.clear();
                    changed = true;
                }
                is_safe_filename(&c.file)
            }
        }
    });

    if chimes.len() != before {
        changed = true;
    }
    changed
}

/// The starter chime presets a fresh install is seeded with — editable TOML, embedded at compile
/// time, so `chimes.toml` isn't empty on first run.
pub const DEFAULT_CHIMES_TOML: &str = include_str!("../default_chimes.toml");

/// The on-disk `chimes.toml`: the saved chime library, kept **separate** from `config.toml` so the
/// user's tones/imports live in their own file (in the same folder as the imported sound files).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChimesFile {
    #[serde(default)]
    pub chimes: Vec<ChimeDto>,
}

impl ChimesFile {
    /// Validate + clamp the chimes; returns whether anything changed.
    pub fn sanitize(&mut self) -> bool {
        sanitize_chimes(&mut self.chimes)
    }
}

fn embedded_default_chimes() -> ChimesFile {
    toml::from_str(DEFAULT_CHIMES_TOML).expect("embedded default_chimes.toml must parse")
}

/// Load `chimes.toml`, self-healing like the main config: missing → seed from the embedded default;
/// corrupt → back up the bad file and reseed; valid → parse + sanitize (persisting if clamped).
pub fn load_chimes(path: &Path) -> std::io::Result<ChimesFile> {
    if !path.exists() {
        let file = embedded_default_chimes();
        save_chimes(path, &file)?;
        return Ok(file);
    }
    let text = fs::read_to_string(path)?;
    match toml::from_str::<ChimesFile>(&text) {
        Ok(mut file) => {
            if file.sanitize() {
                let _ = save_chimes(path, &file);
            }
            Ok(file)
        }
        Err(_) => {
            let backup = path.with_extension("toml.bak");
            let _ = fs::rename(path, &backup);
            let file = embedded_default_chimes();
            save_chimes(path, &file)?;
            Ok(file)
        }
    }
}

/// Atomically write `chimes.toml`, creating its parent folder (this is the chimes folder that also
/// holds imported sound files).
pub fn save_chimes(path: &Path, file: &ChimesFile) -> std::io::Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(id: &str) -> ChimeDto {
        ChimeDto {
            id: id.into(),
            name: id.into(),
            kind: ChimeKindDto::Tones,
            file: String::new(),
            steps: vec![ToneStep {
                freq_hz: 880,
                duration_ms: 200,
                fade_in_ms: 20,
            }],
        }
    }

    #[test]
    fn drops_empty_and_duplicate_ids() {
        let mut chimes = vec![tone("a"), tone(""), tone("a")];
        assert!(sanitize_chimes(&mut chimes));
        assert_eq!(chimes.len(), 1);
        assert_eq!(chimes[0].id, "a");
    }

    #[test]
    fn drops_tones_chime_with_no_steps() {
        let mut empty = tone("x");
        empty.steps.clear();
        let mut chimes = vec![empty];
        assert!(sanitize_chimes(&mut chimes));
        assert!(chimes.is_empty());
    }

    #[test]
    fn rejects_unsafe_file_names_keeps_safe() {
        assert!(is_safe_filename("bell.wav"));
        for bad in [
            "",
            ".",
            "..",
            "../x.wav",
            "a/b.wav",
            "a\\b.wav",
            "C:\\x.wav",
            "x..y",
        ] {
            assert!(!is_safe_filename(bad), "{bad} should be rejected");
        }
        let file = |id: &str, name: &str| ChimeDto {
            id: id.into(),
            name: id.into(),
            kind: ChimeKindDto::File,
            file: name.into(),
            steps: Vec::new(),
        };
        let mut chimes = vec![file("ok", "bell.wav"), file("bad", "../escape.wav")];
        assert!(sanitize_chimes(&mut chimes));
        assert_eq!(chimes.len(), 1);
        assert_eq!(chimes[0].id, "ok");
    }

    #[test]
    fn clamps_step_ranges() {
        let mut c = tone("c");
        c.steps[0] = ToneStep {
            freq_hz: 50_000,
            duration_ms: 0,
            fade_in_ms: 9_999,
        };
        let mut chimes = vec![c];
        assert!(sanitize_chimes(&mut chimes));
        let s = chimes[0].steps[0];
        assert_eq!(s.freq_hz, MAX_FREQ_HZ);
        assert_eq!(s.duration_ms, MIN_STEP_MS);
        assert_eq!(s.fade_in_ms, s.duration_ms);
    }

    #[test]
    fn a_clean_chime_list_is_unchanged() {
        let mut chimes = vec![tone("a"), tone("b")];
        assert!(!sanitize_chimes(&mut chimes));
        assert_eq!(chimes.len(), 2);
    }

    #[test]
    fn embedded_default_chimes_parse_and_are_clean() {
        let mut file = embedded_default_chimes();
        assert_eq!(file.chimes.len(), 2);
        // The shipped default must already be valid — sanitize should change nothing.
        assert!(!file.sanitize());
    }

    #[test]
    fn chimes_file_round_trips_with_nested_steps() {
        let file = ChimesFile {
            chimes: vec![
                tone("bell"),
                ChimeDto {
                    id: "two".into(),
                    name: "Two".into(),
                    kind: ChimeKindDto::Tones,
                    file: String::new(),
                    steps: vec![
                        ToneStep {
                            freq_hz: 660,
                            duration_ms: 200,
                            fade_in_ms: 20,
                        },
                        ToneStep {
                            freq_hz: 990,
                            duration_ms: 200,
                            fade_in_ms: 20,
                        },
                    ],
                },
            ],
        };
        let text = toml::to_string_pretty(&file).unwrap();
        let parsed: ChimesFile = toml::from_str(&text).unwrap();
        assert_eq!(file, parsed);
        assert_eq!(parsed.chimes[1].steps.len(), 2);
    }

    #[test]
    fn empty_chimes_file_round_trips() {
        let file = ChimesFile::default();
        let parsed: ChimesFile = toml::from_str(&toml::to_string_pretty(&file).unwrap()).unwrap();
        assert!(parsed.chimes.is_empty());
    }

    #[test]
    fn old_volume_field_is_ignored_and_dropped_on_save() {
        let text = r#"
[[chimes]]
id = "old"
name = "Old"
kind = "tones"
volume_pct = 99

[[chimes.steps]]
freq_hz = 660
duration_ms = 200
fade_in_ms = 20
"#;
        let parsed: ChimesFile = toml::from_str(text).unwrap();
        assert_eq!(parsed.chimes.len(), 1);
        let saved = toml::to_string_pretty(&parsed).unwrap();
        assert!(!saved.contains("volume_pct"));
    }
}
