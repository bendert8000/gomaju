use std::time::Duration;

/// What happens to work accumulation while the user is away from the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdlePolicy {
    /// Idle for at least a rule's break length credits that break and resets it.
    Credit,
    /// Idle merely pauses counting; nothing is credited.
    Pause,
}

/// How a strict break may be escaped early. The safety floor (auto-release at the
/// break's end, plus a hidden emergency exit) always applies regardless of this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscapeMode {
    /// Skip requires holding a key briefly.
    Friction,
    /// A plain visible skip button.
    Easy,
    /// No easy skip; the break runs to its end (emergency exit still exists).
    NoEasyEscape,
}

impl EscapeMode {
    /// Stable string form passed to the overlay UI. Must match the `EscapeModeDto`
    /// serde representation in `config`.
    pub fn as_str(self) -> &'static str {
        match self {
            EscapeMode::Friction => "friction",
            EscapeMode::Easy => "easy",
            EscapeMode::NoEasyEscape => "no_easy_escape",
        }
    }
}

/// Global engine settings (user-configurable).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub idle_policy: IdlePolicy,
    /// Idle >= this means the user is "away": work stops accumulating.
    pub away_threshold: Duration,
    /// A tick delta larger than this means missed ticks / suspend: treat as away.
    pub gap_threshold: Duration,
    pub escape_mode: EscapeMode,
    /// Lead time to warn before a break starts. Zero disables the warning.
    pub warn: Duration,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            idle_policy: IdlePolicy::Pause,
            away_threshold: Duration::from_secs(30),
            gap_threshold: Duration::from_secs(60),
            escape_mode: EscapeMode::Friction,
            warn: Duration::ZERO,
        }
    }
}
