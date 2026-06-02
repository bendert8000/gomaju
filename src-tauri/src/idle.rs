use std::time::Duration;

use serde::Serialize;

/// Reported health of the active idle backend, surfaced to the UI so platform
/// limitations (notably Wayland) are visible rather than silent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IdleStatus {
    /// Reliable idle detection.
    Active,
    /// Partial/best-effort (e.g. some Wayland compositors). Used once the
    /// Wayland-specific backend lands.
    #[allow(dead_code)]
    Degraded,
    /// No idle detection available; idle features are disabled.
    Off,
}

/// A source of "seconds since last user input". Implementations must be cheap to
/// poll (~1/second) and never block.
pub trait IdleSource: Send {
    fn idle_for(&self) -> Duration;
    fn status(&self) -> IdleStatus;
}

/// Cross-platform idle via the `user-idle` crate (Windows/macOS/Linux-X11).
struct UserIdleSource;

impl IdleSource for UserIdleSource {
    fn idle_for(&self) -> Duration {
        user_idle::UserIdle::get_time()
            .map(|i| i.duration())
            .unwrap_or(Duration::ZERO)
    }
    fn status(&self) -> IdleStatus {
        IdleStatus::Active
    }
}

/// Fallback when no idle backend works (e.g. some Wayland sessions): reports
/// "never idle" so counting simply continues, and advertises `Off`.
struct NullIdleSource;

impl IdleSource for NullIdleSource {
    fn idle_for(&self) -> Duration {
        Duration::ZERO
    }
    fn status(&self) -> IdleStatus {
        IdleStatus::Off
    }
}

/// Pick the best available idle source by probing it once.
/// (Wayland-specific D-Bus/ext-idle backends are a later enhancement.)
pub fn detect() -> Box<dyn IdleSource> {
    if user_idle::UserIdle::get_time().is_ok() {
        Box::new(UserIdleSource)
    } else {
        Box::new(NullIdleSource)
    }
}
