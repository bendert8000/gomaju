use std::time::Duration;

/// How forcefully a break interrupts the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enforcement {
    /// Skippable overlay; gentle.
    Soft,
    /// Opaque all-monitor screen cover; honors the escape switch and safety floor.
    Strict,
}

impl Enforcement {
    /// Stable lowercase string form passed to the overlay UI. Must match the
    /// `EnforcementDto` serde representation in `config`.
    pub fn as_str(self) -> &'static str {
        match self {
            Enforcement::Soft => "soft",
            Enforcement::Strict => "strict",
        }
    }
}

/// One break rule. Users can create any number of these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub id: String,
    pub name: String,
    /// Amount of *active work* before this rule fires a break.
    pub interval: Duration,
    /// How long the break lasts.
    pub break_duration: Duration,
    pub enforcement: Enforcement,
    pub enabled: bool,
}
