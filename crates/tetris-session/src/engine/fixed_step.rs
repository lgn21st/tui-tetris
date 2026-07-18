//! Pure fixed-step backlog accounting.

use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct FixedStepClock {
    tick: Duration,
    max_steps_per_advance: u32,
    backlog: Duration,
}

impl FixedStepClock {
    pub fn new(tick: Duration, max_steps_per_advance: u32) -> Self {
        assert!(!tick.is_zero(), "fixed-step duration must be non-zero");
        assert!(
            max_steps_per_advance > 0,
            "fixed-step burst limit must be non-zero"
        );
        Self {
            tick,
            max_steps_per_advance,
            backlog: Duration::ZERO,
        }
    }

    /// Add elapsed wall time and consume one bounded burst of whole steps.
    pub fn advance(&mut self, elapsed: Duration) -> u32 {
        self.backlog = self.backlog.saturating_add(elapsed);
        let due = (self.backlog.as_nanos() / self.tick.as_nanos())
            .min(self.max_steps_per_advance as u128) as u32;
        self.backlog = self.backlog.saturating_sub(self.tick * due);
        due
    }

    pub fn until_next_step(&self) -> Duration {
        self.tick.saturating_sub(self.backlog)
    }

    pub fn backlog(&self) -> Duration {
        self.backlog
    }
}
