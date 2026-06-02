//! On-disk configuration, expressed as TOML-friendly DTOs that convert to the
//! engine's runtime types. Durations are stored as whole seconds (engineer-
//! friendly to hand-edit). This module owns load/save, validation, defaults, and
//! corrupt-file recovery, but performs no Tauri/path resolution: the host passes
//! in the concrete file path.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::alarm::{self, AlarmDto};
use crate::rule::{Enforcement, Rule};
use crate::settings::{EscapeMode, IdlePolicy, Settings};

/// Bumped when the on-disk schema changes; drives migrations.
pub const CONFIG_VERSION: u32 = 1;

/// Safety cap: no break may auto-hold longer than this, so a bad value can never
/// lock the user out for an unreasonable time.
const MAX_BREAK_SECS: u64 = 60 * 60; // 1 hour
const MAX_WARN_SECS: u64 = 60 * 60; // 1 hour (0 = off is still allowed)
const MIN_INTERVAL_SECS: u64 = 5;
const MIN_BREAK_SECS: u64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnforcementDto {
    Soft,
    Strict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IdlePolicyDto {
    Credit,
    Pause,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscapeModeDto {
    Friction,
    Easy,
    NoEasyEscape,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleDto {
    pub id: String,
    pub name: String,
    pub interval_secs: u64,
    pub break_secs: u64,
    pub enforcement: EnforcementDto,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Whether the rule recurs. Defaults to `true` (repeating) so configs written before
    /// this field existed keep their old behavior; `false` = a one-time break.
    #[serde(default = "default_true")]
    pub repeat: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsDto {
    pub idle_policy: IdlePolicyDto,
    pub away_threshold_secs: u64,
    pub gap_threshold_secs: u64,
    pub escape_mode: EscapeModeDto,
    /// Seconds to warn before a break starts (0 = no warning).
    #[serde(default = "default_warn_seconds")]
    pub warn_seconds: u64,
    /// Play a chime when a break starts.
    #[serde(default = "default_true")]
    pub sound: bool,
    /// Show an OS notification when a soft break starts.
    #[serde(default = "default_true")]
    pub notifications: bool,
}

fn default_warn_seconds() -> u64 {
    30
}

/// Optional global-hotkey accelerators (e.g. "CommandOrControl+Alt+B"). `None`
/// means unbound. Omitted from the file when unset.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotkeysDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toggle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub break_now: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigFile {
    // Scalar fields first so the serialized TOML stays valid (tables/arrays last).
    pub version: u32,
    #[serde(default)]
    pub autostart: bool,
    pub settings: SettingsDto,
    #[serde(default)]
    pub hotkeys: HotkeysDto,
    pub rules: Vec<RuleDto>,
    // Table arrays are serialized last; keep `alarms` after `rules`. Omitted from the
    // file when empty so existing configs stay clean and round-trips don't emit a
    // stray `alarms = []` that TOML would attach to the final `[[rules]]` table.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alarms: Vec<AlarmDto>,
}

fn default_true() -> bool {
    true
}

impl Default for SettingsDto {
    fn default() -> Self {
        Self {
            idle_policy: IdlePolicyDto::Pause,
            away_threshold_secs: 120,
            gap_threshold_secs: 30,
            escape_mode: EscapeModeDto::Friction,
            warn_seconds: 30,
            sound: true,
            notifications: true,
        }
    }
}

impl Default for ConfigFile {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            autostart: false,
            settings: SettingsDto::default(),
            hotkeys: HotkeysDto::default(),
            rules: vec![
                RuleDto {
                    id: "eye-rest".into(),
                    name: "Eye rest".into(),
                    interval_secs: 30 * 60,
                    break_secs: 60,
                    enforcement: EnforcementDto::Soft,
                    enabled: true,
                    repeat: true,
                },
                RuleDto {
                    id: "long-break".into(),
                    name: "Long break".into(),
                    interval_secs: 45 * 60,
                    break_secs: 10 * 60,
                    enforcement: EnforcementDto::Strict,
                    enabled: true,
                    repeat: true,
                },
            ],
            alarms: vec![],
        }
    }
}

impl From<EnforcementDto> for Enforcement {
    fn from(v: EnforcementDto) -> Self {
        match v {
            EnforcementDto::Soft => Enforcement::Soft,
            EnforcementDto::Strict => Enforcement::Strict,
        }
    }
}

impl From<IdlePolicyDto> for IdlePolicy {
    fn from(v: IdlePolicyDto) -> Self {
        match v {
            IdlePolicyDto::Credit => IdlePolicy::Credit,
            IdlePolicyDto::Pause => IdlePolicy::Pause,
        }
    }
}

impl From<EscapeModeDto> for EscapeMode {
    fn from(v: EscapeModeDto) -> Self {
        match v {
            EscapeModeDto::Friction => EscapeMode::Friction,
            EscapeModeDto::Easy => EscapeMode::Easy,
            EscapeModeDto::NoEasyEscape => EscapeMode::NoEasyEscape,
        }
    }
}

impl RuleDto {
    fn to_rule(&self) -> Rule {
        Rule {
            id: self.id.clone(),
            name: self.name.clone(),
            interval: Duration::from_secs(self.interval_secs),
            break_duration: Duration::from_secs(self.break_secs),
            enforcement: self.enforcement.into(),
            enabled: self.enabled,
            repeat: self.repeat,
        }
    }
}

impl SettingsDto {
    fn to_settings(self) -> Settings {
        Settings {
            idle_policy: self.idle_policy.into(),
            away_threshold: Duration::from_secs(self.away_threshold_secs),
            gap_threshold: Duration::from_secs(self.gap_threshold_secs),
            escape_mode: self.escape_mode.into(),
            warn: Duration::from_secs(self.warn_seconds),
        }
    }
}

/// Clamp each rule's durations into safe bounds; returns true if anything changed.
/// Extracted so the rules-only save path (`cmd_save_rules`) can validate just rules,
/// the way `alarm::sanitize_alarms` does for the alarms-only path.
pub fn sanitize_rules(rules: &mut [RuleDto]) -> bool {
    let mut changed = false;
    for r in rules {
        let interval = r.interval_secs.clamp(MIN_INTERVAL_SECS, u64::MAX);
        if interval != r.interval_secs {
            r.interval_secs = interval;
            changed = true;
        }
        let brk = r.break_secs.clamp(MIN_BREAK_SECS, MAX_BREAK_SECS);
        if brk != r.break_secs {
            r.break_secs = brk;
            changed = true;
        }
    }
    changed
}

impl ConfigFile {
    /// Clamp any out-of-range values into safe bounds. Returns `true` if anything
    /// was changed, so the caller can decide to persist the corrected file.
    pub fn sanitize(&mut self) -> bool {
        let mut changed = false;
        if sanitize_rules(&mut self.rules) {
            changed = true;
        }
        let clamp = |v: &mut u64, lo: u64, hi: u64, changed: &mut bool| {
            let c = (*v).clamp(lo, hi);
            if c != *v {
                *v = c;
                *changed = true;
            }
        };
        clamp(&mut self.settings.warn_seconds, 0, MAX_WARN_SECS, &mut changed);
        if self.settings.away_threshold_secs < 1 {
            self.settings.away_threshold_secs = 1;
            changed = true;
        }
        if self.settings.gap_threshold_secs < 1 {
            self.settings.gap_threshold_secs = 1;
            changed = true;
        }
        if alarm::sanitize_alarms(&mut self.alarms) {
            changed = true;
        }
        changed
    }

    /// Convert to the engine's runtime inputs.
    pub fn to_engine_inputs(&self) -> (Vec<Rule>, Settings) {
        (
            self.rules.iter().map(RuleDto::to_rule).collect(),
            self.settings.to_settings(),
        )
    }
}

/// Outcome of loading the config.
#[derive(Debug)]
pub struct LoadOutcome {
    pub config: ConfigFile,
    /// True if defaults were used because the file was missing.
    pub created: bool,
    /// `Some(path)` if a corrupt file was backed up and defaults restored.
    pub recovered_backup: Option<PathBuf>,
}

/// Load config from `path`, self-healing on missing/corrupt files:
/// - missing  -> write defaults, return them (`created = true`)
/// - corrupt  -> back up the bad file alongside it, write+return defaults
/// - valid    -> parse, sanitize (persisting if clamped)
pub fn load(path: &Path) -> std::io::Result<LoadOutcome> {
    if !path.exists() {
        let config = ConfigFile::default();
        save(path, &config)?;
        return Ok(LoadOutcome {
            config,
            created: true,
            recovered_backup: None,
        });
    }

    let text = fs::read_to_string(path)?;
    match toml::from_str::<ConfigFile>(&text) {
        Ok(mut config) => {
            if config.sanitize() {
                let _ = save(path, &config);
            }
            Ok(LoadOutcome {
                config,
                created: false,
                recovered_backup: None,
            })
        }
        Err(_) => {
            // Preserve the bad file for debugging, then restore defaults.
            let backup = path.with_extension("toml.bak");
            let _ = fs::rename(path, &backup);
            let config = ConfigFile::default();
            save(path, &config)?;
            Ok(LoadOutcome {
                config,
                created: false,
                recovered_backup: Some(backup),
            })
        }
    }
}

/// Persist config atomically (write temp in the same dir, then rename over the target).
pub fn save(path: &Path, config: &ConfigFile) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("restee-config-tests");
        let _ = fs::create_dir_all(&dir);
        let p = dir.join(format!("{name}.toml"));
        let _ = fs::remove_file(&p);
        let _ = fs::remove_file(p.with_extension("toml.bak"));
        p
    }

    #[test]
    fn default_config_yields_two_sane_rules() {
        let cfg = ConfigFile::default();
        let (rules, settings) = cfg.to_engine_inputs();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].interval, Duration::from_secs(1800));
        assert_eq!(rules[1].enforcement, Enforcement::Strict);
        assert_eq!(settings.idle_policy, IdlePolicy::Pause);
    }

    #[test]
    fn round_trips_through_toml() {
        let cfg = ConfigFile::default();
        let text = toml::to_string_pretty(&cfg).unwrap();
        let parsed: ConfigFile = toml::from_str(&text).unwrap();
        assert_eq!(cfg, parsed);
    }

    #[test]
    fn round_trips_with_alarms_after_rules() {
        use crate::alarm::{AlarmDto, RepeatDto};
        let cfg = ConfigFile {
            alarms: vec![
                AlarmDto {
                    id: "a1".into(),
                    name: "Standup".into(),
                    time: "09:30".into(),
                    repeat: RepeatDto::Weekly,
                    weekdays: vec![0, 1, 2, 3, 4],
                    day_of_month: 0,
                    month: 0,
                    date: None,
                    enabled: true,
                },
                AlarmDto {
                    id: "a2".into(),
                    name: "New Year".into(),
                    time: "00:00".into(),
                    repeat: RepeatDto::Yearly,
                    weekdays: vec![],
                    day_of_month: 1,
                    month: 1,
                    date: None,
                    enabled: true,
                },
            ],
            ..ConfigFile::default()
        };
        // The serialized [[alarms]] tables must follow [[rules]] and parse back as
        // top-level alarms, not as fields of the last rule.
        let text = toml::to_string_pretty(&cfg).unwrap();
        let parsed: ConfigFile = toml::from_str(&text).unwrap();
        assert_eq!(cfg, parsed);
        assert_eq!(parsed.alarms.len(), 2);
        assert_eq!(parsed.rules.len(), 2);
    }

    #[test]
    fn missing_file_is_created_with_defaults() {
        let path = temp_path("missing");
        let outcome = load(&path).unwrap();
        assert!(outcome.created);
        assert!(path.exists());
        assert_eq!(outcome.config, ConfigFile::default());
    }

    #[test]
    fn corrupt_file_is_backed_up_and_defaults_restored() {
        let path = temp_path("corrupt");
        fs::write(&path, "this is not valid toml = = =").unwrap();
        let outcome = load(&path).unwrap();
        assert!(outcome.recovered_backup.is_some());
        assert!(outcome.recovered_backup.unwrap().exists());
        assert_eq!(outcome.config, ConfigFile::default());
        // The restored file must now parse cleanly.
        let reloaded = load(&path).unwrap();
        assert_eq!(reloaded.config, ConfigFile::default());
    }

    #[test]
    fn sanitize_clamps_an_absurd_break_length() {
        let mut cfg = ConfigFile::default();
        cfg.rules[0].break_secs = 99_999; // far beyond the 1h safety cap
        assert!(cfg.sanitize());
        assert_eq!(cfg.rules[0].break_secs, MAX_BREAK_SECS);
    }

    #[test]
    fn sanitize_clamps_an_absurd_warn_length() {
        let mut cfg = ConfigFile::default();
        cfg.settings.warn_seconds = 99_999;
        assert!(cfg.sanitize());
        assert_eq!(cfg.settings.warn_seconds, MAX_WARN_SECS);
    }
}
