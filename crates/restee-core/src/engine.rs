use std::time::Duration;

use crate::rule::{Enforcement, Rule};
use crate::settings::{EscapeMode, IdlePolicy, Settings};

/// High-level run state of the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    /// Not counting (initial; user hasn't started).
    Stopped,
    /// Counting active work.
    Running,
    /// Manually paused.
    Paused,
    /// A break is currently showing.
    InBreak,
}

impl RunState {
    /// Stable lowercase string form (used for the `state-changed` event payload).
    pub fn as_str(self) -> &'static str {
        match self {
            RunState::Stopped => "stopped",
            RunState::Running => "running",
            RunState::Paused => "paused",
            RunState::InBreak => "in_break",
        }
    }
}

/// Side effects the host (Tauri glue) should perform. The engine itself is pure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    StateChanged(RunState),
    /// A break for `rule_id` is imminent (in `lead_secs`). Show the countdown.
    BreakWarning {
        rule_id: String,
        name: String,
        enforcement: Enforcement,
        lead_secs: u64,
    },
    /// A pending warning no longer applies (e.g. the break was credited by idle).
    BreakWarningCancelled,
    StartBreak {
        rule_id: String,
        name: String,
        enforcement: Enforcement,
        duration: Duration,
        escape_mode: EscapeMode,
    },
    BreakTick {
        rule_id: String,
        remaining: Duration,
    },
    EndBreak {
        rule_id: String,
        /// True if the break ran to its full duration; false if the user skipped it early.
        completed: bool,
    },
    RuleReset {
        rule_id: String,
    },
    /// A non-repeating ("once") rule fired its break and the engine disabled it; the host
    /// should persist `enabled = false` for this rule so it doesn't re-arm on restart.
    RuleDisabled {
        rule_id: String,
    },
}

/// One upcoming break, for status display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextBreak {
    /// Stable rule id, so a UI can map a countdown back to a specific rule/card.
    pub rule_id: String,
    pub rule_name: String,
    pub remaining_secs: u64,
}

/// A read-only snapshot of the engine for status display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineStatus {
    pub state: RunState,
    /// Soonest enabled rule to fire (by remaining work); `None` if no rule is enabled.
    /// Equal to `all.first()`.
    pub next: Option<NextBreak>,
    /// Every enabled rule, soonest-first (by remaining work). Empty if none enabled.
    pub all: Vec<NextBreak>,
}

struct RuleState {
    rule: Rule,
    work: Duration,
    /// Whether this rule has already been credited during the current idle span.
    credited: bool,
}

impl RuleState {
    /// Active work remaining before this rule fires.
    fn remaining(&self) -> Duration {
        self.rule.interval.saturating_sub(self.work)
    }
}

struct ActiveBreak {
    rule_id: String,
    remaining: Duration,
}

/// The pure timer/state machine. No clock, no OS, no Tauri: the host feeds it
/// `(delta, idle)` each tick and interprets the returned effects.
pub struct Engine {
    rules: Vec<RuleState>,
    settings: Settings,
    state: RunState,
    active: Option<ActiveBreak>,
    /// Rule id currently being warned about (pre-break countdown shown), if any.
    warned: Option<String>,
}

impl Engine {
    pub fn new(rules: Vec<Rule>, settings: Settings) -> Self {
        Self {
            rules: rules
                .into_iter()
                .map(|rule| RuleState {
                    rule,
                    work: Duration::ZERO,
                    credited: false,
                })
                .collect(),
            settings,
            state: RunState::Stopped,
            active: None,
            warned: None,
        }
    }

    pub fn state(&self) -> RunState {
        self.state
    }

    /// Snapshot for status display: current state + every enabled rule, soonest-first.
    pub fn status(&self) -> EngineStatus {
        // Collect enabled rules and sort by the full `Duration` remaining (not the
        // truncated seconds) so sub-second ties keep their order, then map to secs.
        let mut enabled: Vec<&RuleState> =
            self.rules.iter().filter(|rs| rs.rule.enabled).collect();
        enabled.sort_by_key(|rs| rs.remaining());
        let all: Vec<NextBreak> = enabled
            .iter()
            .map(|rs| NextBreak {
                rule_id: rs.rule.id.clone(),
                rule_name: rs.rule.name.clone(),
                remaining_secs: rs.remaining().as_secs(),
            })
            .collect();
        let next = all.first().cloned();
        EngineStatus {
            state: self.state,
            next,
            all,
        }
    }

    /// Begin counting (from Stopped or Paused).
    pub fn start(&mut self) -> Vec<Effect> {
        match self.state {
            RunState::Stopped | RunState::Paused => {
                self.state = RunState::Running;
                vec![Effect::StateChanged(RunState::Running)]
            }
            _ => vec![],
        }
    }

    /// Manually pause counting (from Running). Also cancels a pending pre-break warning —
    /// while paused the break is no longer imminent, so the countdown toast should close. The
    /// warning re-fires once the break is imminent again after resuming.
    pub fn pause(&mut self) -> Vec<Effect> {
        if self.state == RunState::Running {
            self.state = RunState::Paused;
            let mut effects = vec![Effect::StateChanged(RunState::Paused)];
            if self.warned.take().is_some() {
                effects.push(Effect::BreakWarningCancelled);
            }
            effects
        } else {
            vec![]
        }
    }

    /// End the current break early (the host decides whether the user is allowed to).
    pub fn skip(&mut self) -> Vec<Effect> {
        if self.state != RunState::InBreak {
            return vec![];
        }
        let rule_id = self
            .active
            .take()
            .map(|a| a.rule_id)
            .unwrap_or_default();
        self.state = RunState::Running;
        vec![
            Effect::EndBreak {
                rule_id,
                completed: false, // user skipped before the duration elapsed
            },
            Effect::StateChanged(RunState::Running),
        ]
    }

    /// Replace rules and settings at runtime (e.g. after the user edits config).
    /// Accumulated work is preserved for rules whose id is unchanged; new rules
    /// start fresh and removed rules are dropped. Current state is left as-is.
    pub fn reconfigure(&mut self, rules: Vec<Rule>, settings: Settings) -> Vec<Effect> {
        self.settings = settings;
        let new_rules: Vec<RuleState> = rules
            .into_iter()
            .map(|rule| {
                let prior = self.rules.iter().find(|rs| rs.rule.id == rule.id);
                let (work, credited) = prior.map_or((Duration::ZERO, false), |rs| {
                    // Don't let preserved work exceed the (possibly shorter) new interval.
                    (rs.work.min(rule.interval), rs.credited)
                });
                RuleState {
                    rule,
                    work,
                    credited,
                }
            })
            .collect();
        self.rules = new_rules;
        self.warned = None;
        vec![]
    }

    /// Restart every rule's countdown (e.g. the user took a break on their own), so
    /// the next break is a full interval away. Cancels any pending warning. Does not
    /// change run state or an active break.
    pub fn reset_timers(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        if self.warned.take().is_some() {
            effects.push(Effect::BreakWarningCancelled);
        }
        for rs in &mut self.rules {
            rs.work = Duration::ZERO;
            rs.credited = false;
            effects.push(Effect::RuleReset {
                rule_id: rs.rule.id.clone(),
            });
        }
        effects
    }

    /// Restart a single rule's countdown (by id) back to a full interval. Cancels a
    /// pending warning only if it was for this rule. No-op if the id isn't found.
    pub fn reset_timer(&mut self, rule_id: &str) -> Vec<Effect> {
        let Some(rs) = self.rules.iter_mut().find(|rs| rs.rule.id == rule_id) else {
            return vec![];
        };
        rs.work = Duration::ZERO;
        rs.credited = false;
        let mut effects = vec![Effect::RuleReset {
            rule_id: rule_id.to_string(),
        }];
        if self.warned.as_deref() == Some(rule_id) {
            self.warned = None;
            effects.push(Effect::BreakWarningCancelled);
        }
        effects
    }

    /// Push a single rule's pending break back by `by` (subtract from accumulated work, bounded
    /// at zero) — the pre-break warning's "delay 1 minute" snooze. Cancels a pending warning for
    /// this rule (it re-fires once the break is imminent again). No-op if the id isn't found.
    pub fn delay_break(&mut self, rule_id: &str, by: Duration) -> Vec<Effect> {
        let Some(rs) = self.rules.iter_mut().find(|rs| rs.rule.id == rule_id) else {
            return vec![];
        };
        rs.work = rs.work.saturating_sub(by);
        let mut effects = Vec::new();
        if self.warned.as_deref() == Some(rule_id) {
            self.warned = None;
            effects.push(Effect::BreakWarningCancelled);
        }
        effects
    }

    /// Immediately start the highest-priority enabled rule's break (the "break now"
    /// action). No-op if already in a break or no rule is enabled.
    pub fn break_now(&mut self) -> Vec<Effect> {
        if self.state == RunState::InBreak {
            return vec![];
        }
        match self.pick_highest_priority_enabled() {
            Some(idx) => self.fire_break(idx),
            None => vec![],
        }
    }

    /// Advance time by `delta`, given the current `idle` duration reported by the OS.
    pub fn tick(&mut self, delta: Duration, idle: Duration) -> Vec<Effect> {
        match self.state {
            RunState::Running => self.tick_running(delta, idle),
            RunState::InBreak => self.tick_break(delta),
            RunState::Stopped | RunState::Paused => vec![],
        }
    }

    fn tick_running(&mut self, delta: Duration, idle: Duration) -> Vec<Effect> {
        // A delta beyond the gap threshold means we missed ticks (suspend/starvation):
        // we can't trust that the user was active, so treat it as away.
        let suspended = delta > self.settings.gap_threshold;
        let away = suspended || idle >= self.settings.away_threshold;
        let mut effects = Vec::new();

        if away {
            // Credit breaks only on a trusted idle reading (never on a suspend gap),
            // and at most once per idle span per rule.
            if self.settings.idle_policy == IdlePolicy::Credit && !suspended {
                for rs in &mut self.rules {
                    if rs.rule.enabled && !rs.credited && idle >= rs.rule.break_duration {
                        rs.work = Duration::ZERO;
                        rs.credited = true;
                        effects.push(Effect::RuleReset {
                            rule_id: rs.rule.id.clone(),
                        });
                    }
                }
            }
        } else {
            // Active: accumulate work and clear the per-span credit flags.
            for rs in &mut self.rules {
                rs.credited = false;
                if rs.rule.enabled {
                    rs.work += delta;
                }
            }
        }

        if let Some(idx) = self.pick_firing_rule() {
            effects.extend(self.fire_break(idx));
            return effects;
        }

        // Pre-break warning: emit when the imminent rule enters its warning window,
        // and cancel if it leaves that window without firing (e.g. idle credit).
        let imminent = self.pick_imminent_warning();
        let imminent_id = imminent.map(|i| self.rules[i].rule.id.clone());
        if imminent_id != self.warned {
            if let Some(i) = imminent {
                let lead = self.rules[i].remaining();
                let rule = &self.rules[i].rule;
                effects.push(Effect::BreakWarning {
                    rule_id: rule.id.clone(),
                    name: rule.name.clone(),
                    enforcement: rule.enforcement,
                    lead_secs: lead.as_secs(),
                });
            } else {
                effects.push(Effect::BreakWarningCancelled);
            }
            self.warned = imminent_id;
        }
        effects
    }

    /// Enabled rule whose break is imminent: in its warning window
    /// (`work` in `[interval - effective_warn, interval)`) and closest to firing.
    /// Picks the smallest remaining time, tie-broken by priority then list order, so
    /// the toast names the break that will actually fire next. `None` if warnings are
    /// disabled. The effective warn is capped at `interval - 1s` so a warning never
    /// fires at zero work (and `warn >= interval` simply warns from ~1s into the cycle).
    fn pick_imminent_warning(&self) -> Option<usize> {
        if self.settings.warn.is_zero() {
            return None;
        }
        let one = Duration::from_secs(1);
        let mut best: Option<usize> = None;
        for (i, rs) in self.rules.iter().enumerate() {
            if !rs.rule.enabled {
                continue;
            }
            let effective_warn = self.settings.warn.min(rs.rule.interval.saturating_sub(one));
            if effective_warn.is_zero() {
                continue; // interval too short to warn meaningfully
            }
            let threshold = rs.rule.interval - effective_warn;
            if rs.work < threshold || rs.work >= rs.rule.interval {
                continue;
            }
            let remaining = rs.remaining();
            best = Some(match best {
                None => i,
                Some(b) => {
                    let best_remaining = self.rules[b].remaining();
                    if remaining < best_remaining
                        || (remaining == best_remaining
                            && higher_priority(&rs.rule, &self.rules[b].rule))
                    {
                        i
                    } else {
                        b
                    }
                }
            });
        }
        best
    }

    fn tick_break(&mut self, delta: Duration) -> Vec<Effect> {
        let Some(active) = self.active.as_mut() else {
            // Defensive: no active break but state says InBreak. Recover to Running.
            self.state = RunState::Running;
            return vec![Effect::StateChanged(RunState::Running)];
        };
        active.remaining = active.remaining.saturating_sub(delta);
        if active.remaining.is_zero() {
            let rule_id = active.rule_id.clone();
            self.active = None;
            self.state = RunState::Running;
            return vec![
                Effect::EndBreak {
                    rule_id,
                    completed: true, // ran the full break duration
                },
                Effect::StateChanged(RunState::Running),
            ];
        }
        vec![Effect::BreakTick {
            rule_id: active.rule_id.clone(),
            remaining: active.remaining,
        }]
    }

    /// Highest-priority enabled rule also satisfying `extra`. Priority: strict > soft,
    /// then longer break wins, then list order.
    fn best_enabled_by_priority(&self, extra: impl Fn(&RuleState) -> bool) -> Option<usize> {
        let mut best: Option<usize> = None;
        for (i, rs) in self.rules.iter().enumerate() {
            if rs.rule.enabled && extra(rs) {
                match best {
                    None => best = Some(i),
                    Some(b) if higher_priority(&rs.rule, &self.rules[b].rule) => best = Some(i),
                    _ => {}
                }
            }
        }
        best
    }

    /// Enabled rule whose work has reached its interval (ready to fire).
    fn pick_firing_rule(&self) -> Option<usize> {
        self.best_enabled_by_priority(|rs| rs.work >= rs.rule.interval)
    }

    /// Highest-priority enabled rule regardless of accumulated work.
    fn pick_highest_priority_enabled(&self) -> Option<usize> {
        self.best_enabled_by_priority(|_| true)
    }

    /// Start the break for `idx`. Resets the firing rule, every rule with a shorter interval
    /// (so a longer break restarts the shorter cycles), and every rule that is itself currently
    /// due — a "collapsed" co-due break is covered by this one, so it must not fire right after.
    fn fire_break(&mut self, idx: usize) -> Vec<Effect> {
        let rule = self.rules[idx].rule.clone();
        let fired_interval = rule.interval;
        let mut effects = Vec::new();
        self.warned = None;

        for rs in &mut self.rules {
            // Reset the firing rule, every rule with a shorter interval (a longer break restarts
            // the shorter cycles), and every rule that is itself currently due — a "collapsed"
            // co-due break is covered by this one and must not fire back-to-back right after.
            if rs.rule.id == rule.id
                || rs.rule.interval < fired_interval
                || (rs.rule.enabled && rs.work >= rs.rule.interval)
            {
                rs.work = Duration::ZERO;
                rs.credited = false;
                effects.push(Effect::RuleReset {
                    rule_id: rs.rule.id.clone(),
                });
            }
        }

        self.active = Some(ActiveBreak {
            rule_id: rule.id.clone(),
            remaining: rule.break_duration,
        });
        self.state = RunState::InBreak;
        effects.push(Effect::StartBreak {
            rule_id: rule.id.clone(),
            name: rule.name.clone(),
            enforcement: rule.enforcement,
            duration: rule.break_duration,
            escape_mode: self.settings.escape_mode,
        });
        effects.push(Effect::StateChanged(RunState::InBreak));

        // A "once" rule fires a single break, then disables itself so it can't fire again
        // (the host persists this). Pushed last so the overlay shows before the host's disk
        // write; `enabled = false` is already respected by pick_firing_rule next tick.
        if !rule.repeat {
            self.rules[idx].rule.enabled = false;
            effects.push(Effect::RuleDisabled {
                rule_id: rule.id.clone(),
            });
        }
        effects
    }
}

/// Whether rule `a` should win over rule `b` when both are due.
/// Strict beats soft; within the same enforcement, the longer break wins; a true
/// tie returns `false` so the earlier-listed rule is kept.
fn higher_priority(a: &Rule, b: &Rule) -> bool {
    use Enforcement::{Soft, Strict};
    match (a.enforcement, b.enforcement) {
        (Strict, Soft) => true,
        (Soft, Strict) => false,
        _ => a.break_duration > b.break_duration,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule::Enforcement;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    fn rule(id: &str, interval: u64, brk: u64, enforcement: Enforcement) -> Rule {
        Rule {
            id: id.into(),
            name: id.into(),
            interval: secs(interval),
            break_duration: secs(brk),
            enforcement,
            enabled: true,
            repeat: true,
        }
    }

    #[test]
    fn rule_fires_break_after_interval_of_active_work() {
        let mut engine = Engine::new(
            vec![rule("eye", 30, 5, Enforcement::Soft)],
            Settings::default(),
        );
        engine.start();
        assert_eq!(engine.state(), RunState::Running);

        // 29 seconds of active work: no break yet.
        for _ in 0..29 {
            let effects = engine.tick(secs(1), secs(0));
            assert!(
                effects.is_empty(),
                "should not fire before the interval elapses"
            );
        }

        // The 30th second crosses the interval and fires the break.
        let effects = engine.tick(secs(1), secs(0));
        assert!(
            effects.iter().any(|e| matches!(
                e,
                Effect::StartBreak { rule_id, .. } if rule_id == "eye"
            )),
            "expected a StartBreak for 'eye', got {effects:?}"
        );
        assert_eq!(engine.state(), RunState::InBreak);
    }

    #[test]
    fn break_ends_after_its_duration_and_resumes_running() {
        let mut engine = Engine::new(
            vec![rule("eye", 2, 3, Enforcement::Soft)],
            Settings::default(),
        );
        engine.start();
        engine.tick(secs(1), secs(0)); // work = 1
        let fire = engine.tick(secs(1), secs(0)); // work = 2 -> fire
        assert!(fire.iter().any(|e| matches!(e, Effect::StartBreak { .. })));
        assert_eq!(engine.state(), RunState::InBreak);

        // The break is 3s long.
        engine.tick(secs(1), secs(0)); // remaining 2
        engine.tick(secs(1), secs(0)); // remaining 1
        let end = engine.tick(secs(1), secs(0)); // remaining 0 -> end

        assert!(
            end.iter().any(|e| matches!(
                e,
                Effect::EndBreak { rule_id, completed } if rule_id == "eye" && *completed
            )),
            "expected a completed EndBreak for 'eye', got {end:?}"
        );
        assert_eq!(engine.state(), RunState::Running);
    }

    #[test]
    fn idle_credits_the_break_once_per_span_under_credit_policy() {
        let settings = Settings {
            idle_policy: IdlePolicy::Credit,
            away_threshold: secs(2),
            ..Settings::default()
        };
        // interval=100 so accumulation never fires during this test.
        let mut engine =
            Engine::new(vec![rule("eye", 100, 10, Enforcement::Soft)], settings);
        engine.start();
        for _ in 0..20 {
            engine.tick(secs(1), secs(0)); // 20s of active work
        }

        // The user walks away; idle climbs from 3s to 13s. Break length is 10s,
        // so the break should be credited exactly once (when idle first hits 10).
        let mut resets = 0;
        for idle in 3..=13u64 {
            let e = engine.tick(secs(1), secs(idle));
            resets += e
                .iter()
                .filter(|e| matches!(e, Effect::RuleReset { rule_id } if rule_id == "eye"))
                .count();
        }
        assert_eq!(resets, 1, "break should be credited exactly once per idle span");
        assert_eq!(engine.state(), RunState::Running);
    }

    #[test]
    fn skip_ends_the_current_break_immediately() {
        let mut engine = Engine::new(
            vec![rule("eye", 2, 10, Enforcement::Soft)],
            Settings::default(),
        );
        engine.start();
        engine.tick(secs(1), secs(0));
        engine.tick(secs(1), secs(0)); // fire a 10s break
        assert_eq!(engine.state(), RunState::InBreak);

        let e = engine.skip();
        assert!(
            e.iter().any(
                |x| matches!(x, Effect::EndBreak { rule_id, completed } if rule_id == "eye" && !*completed)
            ),
            "skip should emit a non-completed EndBreak, got {e:?}"
        );
        assert_eq!(engine.state(), RunState::Running);
    }

    fn once_rule(id: &str, interval: u64, brk: u64, enforcement: Enforcement) -> Rule {
        let mut r = rule(id, interval, brk, enforcement);
        r.repeat = false;
        r
    }

    #[test]
    fn a_once_rule_fires_a_single_break_then_never_again() {
        let mut engine = Engine::new(
            vec![once_rule("eye", 2, 1, Enforcement::Soft)],
            Settings::default(),
        );
        engine.start();

        let mut fired = false;
        for _ in 0..2 {
            let fx = engine.tick(secs(1), secs(0));
            fired |= fx.iter().any(|e| matches!(e, Effect::StartBreak { .. }));
        }
        assert!(fired, "a once rule should fire after its interval");
        assert_eq!(engine.state(), RunState::InBreak);

        engine.tick(secs(1), secs(0)); // 1s break elapses -> back to Running
        assert_eq!(engine.state(), RunState::Running);

        let mut refired = false;
        for _ in 0..5 {
            let fx = engine.tick(secs(1), secs(0));
            refired |= fx.iter().any(|e| matches!(e, Effect::StartBreak { .. }));
        }
        assert!(!refired, "a once rule must not fire a second time");
    }

    #[test]
    fn fire_break_emits_rule_disabled_only_for_once_rules() {
        // Repeating rule: fires, but no RuleDisabled.
        let mut engine = Engine::new(
            vec![rule("rep", 1, 1, Enforcement::Soft)],
            Settings::default(),
        );
        engine.start();
        let fx = engine.tick(secs(1), secs(0));
        assert!(fx.iter().any(|e| matches!(e, Effect::StartBreak { .. })));
        assert!(!fx.iter().any(|e| matches!(e, Effect::RuleDisabled { .. })));

        // Once rule: fires and emits RuleDisabled for that id.
        let mut engine = Engine::new(
            vec![once_rule("once", 1, 1, Enforcement::Soft)],
            Settings::default(),
        );
        engine.start();
        let fx = engine.tick(secs(1), secs(0));
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::RuleDisabled { rule_id } if rule_id == "once")),
            "once rule should emit RuleDisabled, got {fx:?}"
        );
    }

    #[test]
    fn break_now_consumes_a_once_rule() {
        let mut engine = Engine::new(
            vec![once_rule("once", 100, 5, Enforcement::Strict)],
            Settings::default(),
        );
        engine.start();
        let fx = engine.break_now();
        assert!(fx.iter().any(|e| matches!(e, Effect::StartBreak { .. })));
        assert!(fx.iter().any(|e| matches!(e, Effect::RuleDisabled { rule_id } if rule_id == "once")));

        engine.skip(); // end the break
        // The once rule is now disabled, so a manual break has nothing to fire.
        let again = engine.break_now();
        assert!(
            !again.iter().any(|e| matches!(e, Effect::StartBreak { .. })),
            "a consumed once rule must not fire again, got {again:?}"
        );
    }

    #[test]
    fn pause_stops_accumulation_until_resumed() {
        let mut engine = Engine::new(
            vec![rule("eye", 5, 2, Enforcement::Soft)],
            Settings::default(),
        );
        engine.start();
        engine.tick(secs(1), secs(0));
        engine.tick(secs(1), secs(0)); // work = 2

        engine.pause();
        assert_eq!(engine.state(), RunState::Paused);
        for _ in 0..10 {
            let e = engine.tick(secs(1), secs(0));
            assert!(e.is_empty(), "paused ticks must do nothing, got {e:?}");
        }

        engine.start(); // resume
        engine.tick(secs(1), secs(0)); // work = 3
        engine.tick(secs(1), secs(0)); // work = 4
        let fire = engine.tick(secs(1), secs(0)); // work = 5 -> fire
        assert!(
            fire.iter().any(|x| matches!(x, Effect::StartBreak { .. })),
            "should fire after resuming and reaching the interval, got {fire:?}"
        );
    }

    #[test]
    fn pause_cancels_a_pending_break_warning() {
        let settings = Settings {
            warn: secs(5),
            ..Settings::default()
        };
        let mut engine = Engine::new(vec![rule("eye", 10, 3, Enforcement::Soft)], settings);
        engine.start();
        for _ in 0..5 {
            engine.tick(secs(1), secs(0)); // work = 5 -> warning emitted (countdown toast up)
        }

        // Pausing cancels the pending warning so the host closes the countdown toast.
        let e = engine.pause();
        assert!(
            e.iter().any(|x| matches!(x, Effect::BreakWarningCancelled)),
            "pause should cancel the pending warning, got {e:?}"
        );
        assert_eq!(engine.state(), RunState::Paused);

        // Resuming still inside the warning window re-emits the warning (toast comes back).
        engine.start();
        let e = engine.tick(secs(1), secs(0)); // work = 6, still in [5, 10)
        assert!(
            e.iter().any(|x| matches!(x, Effect::BreakWarning { .. })),
            "resuming in the warning window should re-emit the warning, got {e:?}"
        );
    }

    #[test]
    fn break_now_immediately_starts_the_highest_priority_break() {
        let rules = vec![
            rule("eye", 1800, 60, Enforcement::Soft),
            rule("long", 2700, 600, Enforcement::Strict),
        ];
        let mut engine = Engine::new(rules, Settings::default());

        let fire = engine.break_now();
        assert_eq!(
            started_rule(&fire).as_deref(),
            Some("long"),
            "break_now should pick the highest-priority (strict) rule"
        );
        assert_eq!(engine.state(), RunState::InBreak);
    }

    #[test]
    fn reconfigure_applies_new_interval_and_preserves_progress() {
        let mut engine = Engine::new(
            vec![rule("eye", 100, 5, Enforcement::Soft)],
            Settings::default(),
        );
        engine.start();
        for _ in 0..10 {
            engine.tick(secs(1), secs(0)); // work = 10
        }

        // Shorten the interval to 10s (same id). Work (10) is preserved, so the
        // next tick should cross the new interval and fire.
        engine.reconfigure(
            vec![rule("eye", 10, 5, Enforcement::Soft)],
            Settings::default(),
        );
        let fire = engine.tick(secs(1), secs(0));
        assert!(
            fire.iter().any(|e| matches!(e, Effect::StartBreak { .. })),
            "preserved progress + shorter interval should fire, got {fire:?}"
        );
    }

    #[test]
    fn warns_lead_seconds_before_a_break_fires() {
        let settings = Settings {
            warn: secs(5),
            ..Settings::default()
        };
        let mut engine = Engine::new(vec![rule("eye", 10, 3, Enforcement::Soft)], settings);
        engine.start();

        // Below the warning threshold (interval - warn = 5): no warning.
        for _ in 0..4 {
            let e = engine.tick(secs(1), secs(0));
            assert!(!e.iter().any(|x| matches!(x, Effect::BreakWarning { .. })));
        }
        // Crossing the threshold (work = 5) emits exactly one warning, no break yet.
        let e = engine.tick(secs(1), secs(0));
        assert!(
            e.iter().any(|x| matches!(
                x,
                Effect::BreakWarning { rule_id, lead_secs, .. } if rule_id == "eye" && *lead_secs == 5
            )),
            "expected a 5s warning for 'eye', got {e:?}"
        );
        assert!(!e.iter().any(|x| matches!(x, Effect::StartBreak { .. })));

        // Inside the window: no repeated warning, no fire yet.
        for _ in 0..4 {
            let e = engine.tick(secs(1), secs(0));
            assert!(!e.iter().any(|x| matches!(x, Effect::BreakWarning { .. })), "no repeat");
            assert!(!e.iter().any(|x| matches!(x, Effect::StartBreak { .. })));
        }
        // Reaching the interval fires the break.
        let e = engine.tick(secs(1), secs(0));
        assert!(e.iter().any(|x| matches!(x, Effect::StartBreak { .. })));
    }

    #[test]
    fn delay_break_pushes_the_break_back_and_cancels_the_warning() {
        let settings = Settings {
            warn: secs(5),
            ..Settings::default()
        };
        let mut engine = Engine::new(vec![rule("eye", 10, 3, Enforcement::Soft)], settings);
        engine.start();
        for _ in 0..5 {
            engine.tick(secs(1), secs(0)); // work = 5 -> warning emitted (5s to fire)
        }

        // Snooze 3s: work 5 -> 2, and the pending warning is cancelled.
        let e = engine.delay_break("eye", secs(3));
        assert!(e.iter().any(|x| matches!(x, Effect::BreakWarningCancelled)));

        // Was 5s from firing, now 8s: the next 5 ticks (work -> 7) must not fire.
        for _ in 0..5 {
            let e = engine.tick(secs(1), secs(0));
            assert!(!e.iter().any(|x| matches!(x, Effect::StartBreak { .. })));
        }
        // Three more reach the interval (work = 10) and fire — ~3s later than without the delay.
        let mut fired = false;
        for _ in 0..3 {
            let e = engine.tick(secs(1), secs(0));
            fired |= e
                .iter()
                .any(|x| matches!(x, Effect::StartBreak { rule_id, .. } if rule_id == "eye"));
        }
        assert!(fired, "the break should fire after the delayed countdown");
    }

    #[test]
    fn delay_break_unknown_rule_is_a_noop() {
        let mut engine = Engine::new(vec![rule("eye", 10, 3, Enforcement::Soft)], Settings::default());
        engine.start();
        assert!(engine.delay_break("nope", secs(60)).is_empty());
    }

    #[test]
    fn warning_is_cancelled_if_idle_credits_the_break() {
        let settings = Settings {
            warn: secs(5),
            idle_policy: IdlePolicy::Credit,
            away_threshold: secs(2),
            ..Settings::default()
        };
        let mut engine = Engine::new(vec![rule("eye", 10, 3, Enforcement::Soft)], settings);
        engine.start();
        for _ in 0..5 {
            engine.tick(secs(1), secs(0)); // work = 5 -> warning emitted
        }

        // Go idle long enough to credit the break (idle >= break_duration 3): the
        // rule resets, so the pending warning must be cancelled.
        let mut cancelled = false;
        for idle in 3..=4u64 {
            let e = engine.tick(secs(1), secs(idle));
            if e.iter().any(|x| matches!(x, Effect::BreakWarningCancelled)) {
                cancelled = true;
            }
        }
        assert!(cancelled, "an idle-credited break should cancel its pending warning");
    }

    #[test]
    fn status_reports_the_soonest_next_break() {
        let rules = vec![
            rule("eye", 30, 5, Enforcement::Soft),
            rule("long", 45, 10, Enforcement::Strict),
        ];
        let mut engine = Engine::new(rules, Settings::default());
        engine.start();
        for _ in 0..10 {
            engine.tick(secs(1), secs(0)); // each rule's work = 10
        }

        let s = engine.status();
        assert_eq!(s.state, RunState::Running);
        let next = s.next.expect("a next break");
        // eye remaining 20 < long remaining 35 -> eye is soonest.
        assert_eq!(next.rule_name, "eye");
        assert_eq!(next.remaining_secs, 20);
    }

    #[test]
    fn status_all_lists_every_enabled_rule_soonest_first() {
        let mut rules = vec![
            rule("eye", 30, 5, Enforcement::Soft),
            rule("long", 45, 10, Enforcement::Strict),
            rule("off", 20, 5, Enforcement::Soft),
        ];
        rules[2].enabled = false; // disabled -> must be excluded from `all`
        let mut engine = Engine::new(rules, Settings::default());
        engine.start();
        for _ in 0..10 {
            engine.tick(secs(1), secs(0)); // each enabled rule's work = 10
        }

        let s = engine.status();
        // Disabled "off" excluded; remaining eye 20 < long 35 -> soonest-first.
        let names: Vec<&str> = s.all.iter().map(|b| b.rule_name.as_str()).collect();
        assert_eq!(names, vec!["eye", "long"]);
        assert_eq!(s.all[0].rule_id, "eye");
        assert_eq!(s.all[0].remaining_secs, 20);
        assert_eq!(s.all[1].remaining_secs, 35);
        // `next` mirrors the soonest entry.
        assert_eq!(s.next.as_ref(), s.all.first());
    }

    #[test]
    fn reset_timers_pushes_next_break_back_to_full_interval() {
        let mut engine = Engine::new(
            vec![rule("eye", 30, 5, Enforcement::Soft)],
            Settings::default(),
        );
        engine.start();
        for _ in 0..20 {
            engine.tick(secs(1), secs(0)); // work 20 -> 10 remaining
        }
        assert_eq!(engine.status().next.unwrap().remaining_secs, 10);

        engine.reset_timers();
        assert_eq!(engine.status().next.unwrap().remaining_secs, 30);
    }

    #[test]
    fn reset_timer_restarts_only_the_named_rule() {
        let mut engine = Engine::new(
            vec![
                rule("eye", 30, 5, Enforcement::Soft),
                rule("long", 50, 10, Enforcement::Strict),
            ],
            Settings::default(),
        );
        engine.start();
        for _ in 0..10 {
            engine.tick(secs(1), secs(0)); // each rule's work = 10
        }
        engine.reset_timer("eye"); // eye -> full 30; long stays at 40 remaining

        let all = engine.status().all;
        let remaining = |name: &str| {
            all.iter()
                .find(|b| b.rule_name == name)
                .unwrap()
                .remaining_secs
        };
        assert_eq!(remaining("eye"), 30);
        assert_eq!(remaining("long"), 40);
    }

    #[test]
    fn reset_timers_cancels_a_pending_warning() {
        let settings = Settings {
            warn: secs(5),
            ..Settings::default()
        };
        let mut engine = Engine::new(vec![rule("eye", 10, 3, Enforcement::Soft)], settings);
        engine.start();
        for _ in 0..6 {
            engine.tick(secs(1), secs(0)); // work 6 -> inside the warning window
        }
        let e = engine.reset_timers();
        assert!(
            e.iter().any(|x| matches!(x, Effect::BreakWarningCancelled)),
            "reset should cancel a pending warning, got {e:?}"
        );
    }

    fn warned_ids(effects: &[Effect]) -> Vec<String> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::BreakWarning { rule_id, .. } => Some(rule_id.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn warning_targets_soonest_to_fire_not_highest_priority() {
        let settings = Settings {
            warn: secs(5),
            ..Settings::default()
        };
        // "eye" fires sooner (interval 10) than the higher-priority "long" (interval 12).
        let rules = vec![
            rule("eye", 10, 3, Enforcement::Soft),
            rule("long", 12, 5, Enforcement::Strict),
        ];
        let mut engine = Engine::new(rules, settings);
        engine.start();

        let mut warned = Vec::new();
        for _ in 0..8 {
            warned.extend(warned_ids(&engine.tick(secs(1), secs(0))));
        }
        assert!(warned.contains(&"eye".to_string()), "expected eye warned, got {warned:?}");
        assert!(
            !warned.contains(&"long".to_string()),
            "the later-but-higher-priority 'long' must not be warned, got {warned:?}"
        );
    }

    #[test]
    fn warning_tie_breaks_to_strict() {
        let settings = Settings {
            warn: secs(5),
            ..Settings::default()
        };
        // Equal interval -> equal remaining at every tick; strict should win the tie.
        let rules = vec![
            rule("soft1", 10, 3, Enforcement::Soft),
            rule("strict1", 10, 5, Enforcement::Strict),
        ];
        let mut engine = Engine::new(rules, settings);
        engine.start();

        let mut first = None;
        for _ in 0..6 {
            for id in warned_ids(&engine.tick(secs(1), secs(0))) {
                first.get_or_insert(id);
            }
        }
        assert_eq!(first.as_deref(), Some("strict1"));
    }

    #[test]
    fn does_not_warn_before_any_work_even_when_warn_exceeds_interval() {
        let settings = Settings {
            warn: secs(100), // far longer than the interval
            ..Settings::default()
        };
        let mut engine = Engine::new(vec![rule("eye", 5, 2, Enforcement::Soft)], settings);
        engine.start();
        // Idle immediately (work stays 0): there must be no warning at zero work.
        let e = engine.tick(secs(1), secs(999));
        assert!(
            !e.iter().any(|x| matches!(x, Effect::BreakWarning { .. })),
            "should not warn before any work is accumulated, got {e:?}"
        );
    }

    fn started_rule(effects: &[Effect]) -> Option<String> {
        effects.iter().find_map(|e| match e {
            Effect::StartBreak { rule_id, .. } => Some(rule_id.clone()),
            _ => None,
        })
    }

    #[test]
    fn strict_break_takes_priority_over_soft_when_both_are_due() {
        // Soft is listed first to prove priority is not just list order.
        let rules = vec![
            rule("eye", 3, 2, Enforcement::Soft),
            rule("long", 3, 5, Enforcement::Strict),
        ];
        let mut engine = Engine::new(rules, Settings::default());
        engine.start();
        engine.tick(secs(1), secs(0));
        engine.tick(secs(1), secs(0));
        let fire = engine.tick(secs(1), secs(0)); // both reach interval 3

        assert_eq!(
            started_rule(&fire).as_deref(),
            Some("long"),
            "the strict rule should win over the soft rule"
        );
    }

    #[test]
    fn firing_a_longer_rule_resets_shorter_interval_rules() {
        let rules = vec![
            rule("eye", 3, 2, Enforcement::Soft),
            rule("long", 5, 2, Enforcement::Strict),
        ];
        let mut engine = Engine::new(rules, Settings::default());
        engine.start();

        let mut long_fire = None;
        for _ in 0..30 {
            let e = engine.tick(secs(1), secs(0));
            if e
                .iter()
                .any(|x| matches!(x, Effect::StartBreak { rule_id, .. } if rule_id == "long"))
            {
                long_fire = Some(e);
                break;
            }
        }
        let e = long_fire.expect("the 'long' break should fire within 30s");
        assert!(
            e.iter()
                .any(|x| matches!(x, Effect::RuleReset { rule_id } if rule_id == "eye")),
            "firing 'long' should reset the shorter-interval 'eye', got {e:?}"
        );
    }

    #[test]
    fn firing_the_shorter_rule_does_not_reset_a_longer_rule() {
        // User report: when the 30-min "eye" break finishes, does the 45-min "sit" break
        // also reset to 45? It must NOT — fire_break resets the firing rule, rules with a
        // SHORTER interval, and any rule that is itself due; a longer rule that isn't due
        // (here "sit" is at 30/45) keeps its progress.
        let rules = vec![
            rule("eye", 30, 5, Enforcement::Soft), // shorter -> fires first
            rule("sit", 45, 10, Enforcement::Strict), // longer
        ];
        let mut engine = Engine::new(rules, Settings::default());
        engine.start();

        let remaining = |e: &Engine, name: &str| {
            e.status()
                .all
                .iter()
                .find(|b| b.rule_name == name)
                .map(|b| b.remaining_secs)
        };

        // 30s of active work: "eye" reaches its interval and fires.
        let mut eye_fired = false;
        for _ in 0..30 {
            let fx = engine.tick(secs(1), secs(0));
            eye_fired |= fx
                .iter()
                .any(|e| matches!(e, Effect::StartBreak { rule_id, .. } if rule_id == "eye"));
        }
        assert!(eye_fired, "eye should fire at its 30s interval");

        // At that moment "sit" had 30s of work -> 15s remaining, and must NOT be reset to 45.
        assert_eq!(remaining(&engine, "eye"), Some(30), "the firing rule resets to full");
        assert_eq!(
            remaining(&engine, "sit"),
            Some(15),
            "the longer 'sit' rule must keep its progress (45 - 30), not reset to 45"
        );

        // Play out eye's 5s break; work is frozen during the break, so "sit" is still 15.
        for _ in 0..5 {
            engine.tick(secs(1), secs(0));
        }
        assert_eq!(engine.state(), RunState::Running);
        assert_eq!(
            remaining(&engine, "sit"),
            Some(15),
            "'sit' is still not reset after the eye break ends"
        );
    }

    #[test]
    fn co_due_collapse_resets_the_losing_rule() {
        // Two rules with the SAME interval both come due on one tick ("collapse"). The strict
        // 20-unit break wins; the soft rule must be reset (covered), not left armed to fire a
        // back-to-back break the instant the strict break ends.
        let rules = vec![
            rule("eye", 3, 2, Enforcement::Soft),
            rule("posture", 3, 20, Enforcement::Strict),
        ];
        let mut engine = Engine::new(rules, Settings::default());
        engine.start();
        engine.tick(secs(1), secs(0));
        engine.tick(secs(1), secs(0));
        let fire = engine.tick(secs(1), secs(0)); // both reach interval 3

        // (a) the strict rule fires, (b) the co-due soft rule is reset on the same tick.
        assert_eq!(started_rule(&fire).as_deref(), Some("posture"));
        assert!(
            fire.iter().any(|e| matches!(e, Effect::RuleReset { rule_id } if rule_id == "eye")),
            "the co-due soft rule should be reset when the strict break fires, got {fire:?}"
        );

        // (c) play out the 20-unit strict break; on the tick right after it ends the soft rule
        // (now reset) must NOT fire — without the fix it would still be armed and refire.
        let mut after_end = None;
        for _ in 0..21 {
            let fx = engine.tick(secs(1), secs(0));
            if fx.iter().any(|e| matches!(e, Effect::EndBreak { .. })) {
                after_end = Some(engine.tick(secs(1), secs(0)));
                break;
            }
        }
        let after = after_end.expect("the strict break should have ended");
        assert!(
            !after.iter().any(|e| matches!(e, Effect::StartBreak { .. })),
            "the covered soft rule must not fire on the tick after the break ends, got {after:?}"
        );
    }

    #[test]
    fn co_due_collapse_resets_a_longer_interval_loser() {
        // A strict short-interval rule and a soft long-interval rule align on a tick. The strict
        // rule wins; the LONGER-interval soft rule must still be reset — something the old
        // "shorter interval only" reset never did (this isolates the new co-due clause).
        let rules = vec![
            rule("blink", 30, 5, Enforcement::Strict), // shorter interval, wins on priority
            rule("walk", 60, 5, Enforcement::Soft),    // longer interval, the co-due loser
        ];
        let mut engine = Engine::new(rules, Settings::default());
        engine.start();

        let remaining = |e: &Engine, name: &str| {
            e.status()
                .all
                .iter()
                .find(|b| b.rule_name == name)
                .map(|b| b.remaining_secs)
        };

        // t=30: "blink" fires alone ("walk" at 30/60, not due) and resets only itself.
        for _ in 0..30 {
            engine.tick(secs(1), secs(0));
        }
        // Play out blink's 5s break (work frozen), so "walk" keeps its 30/60 progress.
        for _ in 0..5 {
            engine.tick(secs(1), secs(0));
        }
        assert_eq!(remaining(&engine, "walk"), Some(30), "walk keeps its 30/60 progress");

        // 30 more units: re-armed "blink" (interval 30) and "walk" (now 60/60) come due together.
        let mut collide = None;
        for _ in 0..30 {
            let fx = engine.tick(secs(1), secs(0));
            if fx.iter().any(|e| matches!(e, Effect::StartBreak { .. })) {
                collide = Some(fx);
                break;
            }
        }
        let fx = collide.expect("blink and walk should come due together");
        assert_eq!(started_rule(&fx).as_deref(), Some("blink"), "strict blink wins the collision");
        assert!(
            fx.iter().any(|e| matches!(e, Effect::RuleReset { rule_id } if rule_id == "walk")),
            "the LONGER-interval co-due 'walk' must be reset, got {fx:?}"
        );
    }

    #[test]
    fn break_now_resets_a_coincidentally_due_longer_rule() {
        // A reconfigure leaves two rules due at once, the coincidentally-due one ("eye") having a
        // LONGER interval than the strict rule break_now will fire. break_now fires the strict rule
        // and, via the new co-due clause (not the shorter-interval clause), resets the longer rule.
        let mut engine = Engine::new(
            vec![
                rule("manual", 100, 5, Enforcement::Strict),
                rule("eye", 100, 2, Enforcement::Soft),
            ],
            Settings::default(),
        );
        engine.start();
        for _ in 0..60 {
            engine.tick(secs(1), secs(0)); // both banked to work = 60, neither due (interval 100)
        }
        // Shorten both intervals below the banked work so both are now due; "eye" (50) is LONGER
        // than the strict "manual" (5) that break_now will fire.
        engine.reconfigure(
            vec![
                rule("manual", 5, 5, Enforcement::Strict),
                rule("eye", 50, 2, Enforcement::Soft),
            ],
            Settings::default(),
        );

        let fire = engine.break_now();
        assert_eq!(started_rule(&fire).as_deref(), Some("manual"));
        assert!(
            fire.iter().any(|e| matches!(e, Effect::RuleReset { rule_id } if rule_id == "eye")),
            "break_now should reset the coincidentally-due longer 'eye' rule, got {fire:?}"
        );
    }

    #[test]
    fn co_due_once_loser_is_rearmed_not_consumed() {
        // Two "once" rules come due on the same tick. The strict one fires and is consumed
        // (RuleDisabled); the soft loser is reset/re-armed — it emits RuleReset, must NOT emit
        // RuleDisabled, and stays enabled so it gets its turn on the next cycle.
        let rules = vec![
            once_rule("eye", 3, 2, Enforcement::Soft),
            once_rule("posture", 3, 5, Enforcement::Strict),
        ];
        let mut engine = Engine::new(rules, Settings::default());
        engine.start();
        engine.tick(secs(1), secs(0));
        engine.tick(secs(1), secs(0));
        let fire = engine.tick(secs(1), secs(0)); // both due at interval 3

        assert_eq!(started_rule(&fire).as_deref(), Some("posture"));
        // Winner consumed:
        assert!(
            fire.iter().any(|e| matches!(e, Effect::RuleDisabled { rule_id } if rule_id == "posture")),
            "the firing once rule should be disabled, got {fire:?}"
        );
        // Loser reset but NOT consumed:
        assert!(
            fire.iter().any(|e| matches!(e, Effect::RuleReset { rule_id } if rule_id == "eye")),
            "the co-due once loser should be reset, got {fire:?}"
        );
        assert!(
            !fire.iter().any(|e| matches!(e, Effect::RuleDisabled { rule_id } if rule_id == "eye")),
            "the co-due once loser must NOT be disabled, got {fire:?}"
        );

        // It stays enabled: after the winner's break ends, "eye" still fires on its next cycle.
        let mut eye_refired = false;
        for _ in 0..20 {
            let fx = engine.tick(secs(1), secs(0));
            eye_refired |= fx
                .iter()
                .any(|e| matches!(e, Effect::StartBreak { rule_id, .. } if rule_id == "eye"));
        }
        assert!(eye_refired, "the re-armed once 'eye' rule should fire on its next cycle");
    }

    #[test]
    fn among_same_enforcement_the_longer_break_wins() {
        let rules = vec![
            rule("short", 3, 2, Enforcement::Soft),
            rule("longer", 3, 8, Enforcement::Soft),
        ];
        let mut engine = Engine::new(rules, Settings::default());
        engine.start();
        engine.tick(secs(1), secs(0));
        engine.tick(secs(1), secs(0));
        let fire = engine.tick(secs(1), secs(0));

        assert_eq!(started_rule(&fire).as_deref(), Some("longer"));
    }

    #[test]
    fn idle_pauses_accumulation_under_pause_policy() {
        let settings = Settings {
            away_threshold: secs(2),
            ..Settings::default() // idle_policy = Pause by default
        };
        let mut engine = Engine::new(vec![rule("eye", 5, 2, Enforcement::Soft)], settings);
        engine.start();
        engine.tick(secs(1), secs(0)); // work = 1
        engine.tick(secs(1), secs(0)); // work = 2

        // Idle for a long span: no accumulation, no fire.
        for idle in 2..=20u64 {
            let e = engine.tick(secs(1), secs(idle));
            assert!(e.is_empty(), "idle must not accumulate or fire, got {e:?}");
        }

        // Back to active: 3 more active seconds reaches the interval of 5.
        engine.tick(secs(1), secs(0)); // 3
        engine.tick(secs(1), secs(0)); // 4
        let fire = engine.tick(secs(1), secs(0)); // 5 -> fire
        assert!(fire.iter().any(|x| matches!(x, Effect::StartBreak { .. })));
    }

    #[test]
    fn a_suspend_gap_does_not_count_as_work() {
        let mut engine = Engine::new(
            vec![rule("eye", 30, 5, Enforcement::Soft)],
            Settings::default(),
        );
        engine.start();
        engine.tick(secs(20), secs(0)); // 20s active (< 60s gap) -> work = 20

        // A 10-minute gap (machine suspended) must not be counted as work.
        let e = engine.tick(secs(600), secs(0));
        assert!(e.is_empty(), "a suspend gap must not fire a break");

        for _ in 0..9 {
            assert!(engine.tick(secs(1), secs(0)).is_empty()); // work 21..29
        }
        let fire = engine.tick(secs(1), secs(0)); // work = 30 -> fire
        assert!(
            fire.iter().any(|x| matches!(x, Effect::StartBreak { .. })),
            "the gap should have added zero work (fires only at 30s of real activity)"
        );
    }
}
