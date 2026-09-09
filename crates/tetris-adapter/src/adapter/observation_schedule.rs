//! Deterministic observation cadence shared by interactive and headless runners.

use arrayvec::ArrayVec;
use tetris_adapter_protocol::protocol::TransitionEvent;
use tetris_core::core::GameSnapshot;
use tetris_core::types::CoreLastEvent;
use tetris_core::types::TICK_MS;

#[derive(Debug)]
pub struct ObservationSchedule {
    frequency_hz: u32,
    accumulated_frequency_units: u32,
    seq: u64,
    last_episode_id: u32,
    last_piece_id: u32,
    last_active_id: u32,
    last_paused: bool,
    last_game_over: bool,
    pending_events: ArrayVec<TransitionEvent, 4>,
}

impl ObservationSchedule {
    pub fn new(snapshot: &GameSnapshot, frequency_hz: u32) -> Self {
        Self {
            frequency_hz: frequency_hz.clamp(1, 60),
            accumulated_frequency_units: 0,
            seq: 0,
            last_episode_id: snapshot.episode_id,
            last_piece_id: snapshot.piece_id,
            last_active_id: snapshot.active_id,
            last_paused: snapshot.paused,
            last_game_over: snapshot.game_over,
            pending_events: ArrayVec::new(),
        }
    }

    pub fn from_env(snapshot: &GameSnapshot) -> Self {
        let frequency_hz = std::env::var("TETRIS_AI_OBS_HZ")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(20);
        Self::new(snapshot, frequency_hz)
    }

    pub fn capture_event(&mut self, event: CoreLastEvent) {
        if self.pending_events.is_full() {
            self.pending_events.remove(0);
        }
        self.pending_events.push(event.into());
    }

    pub fn immediate(&mut self) -> (u64, ArrayVec<TransitionEvent, 4>) {
        self.seq = self.seq.wrapping_add(1);
        (self.seq, std::mem::take(&mut self.pending_events))
    }

    pub fn after_tick(
        &mut self,
        snapshot: &GameSnapshot,
    ) -> Option<(u64, ArrayVec<TransitionEvent, 4>)> {
        let mut critical = false;
        critical |= update_changed(&mut self.last_piece_id, snapshot.piece_id);
        critical |= update_changed(&mut self.last_active_id, snapshot.active_id);
        critical |= update_changed(&mut self.last_episode_id, snapshot.episode_id);
        critical |= update_changed(&mut self.last_paused, snapshot.paused);
        critical |= update_changed(&mut self.last_game_over, snapshot.game_over);

        critical |= !self.pending_events.is_empty();

        self.accumulated_frequency_units = self
            .accumulated_frequency_units
            .saturating_add(TICK_MS.saturating_mul(self.frequency_hz));
        if !critical && self.accumulated_frequency_units < 1000 {
            return None;
        }

        if critical {
            self.accumulated_frequency_units = 0;
        } else {
            self.accumulated_frequency_units -= 1000;
        }
        Some(self.immediate())
    }
}

fn update_changed<T: Copy + PartialEq>(previous: &mut T, current: T) -> bool {
    if *previous == current {
        false
    } else {
        *previous = current;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tetris_core::core::GameState;
    use tetris_core::types::GameAction;

    #[test]
    fn emits_at_configured_fixed_step_interval() {
        let mut game = GameState::new(1);
        game.start();
        let mut schedule = ObservationSchedule::new(&game.snapshot(), 20);

        assert!(schedule.after_tick(&game.snapshot()).is_none());
        assert!(schedule.after_tick(&game.snapshot()).is_none());
        assert!(schedule.after_tick(&game.snapshot()).is_none());
        assert_eq!(schedule.after_tick(&game.snapshot()).unwrap().0, 1);
    }

    #[test]
    fn preserves_requested_frequency_without_integer_period_drift() {
        let mut game = GameState::new(1);
        game.start();
        let mut schedule = ObservationSchedule::new(&game.snapshot(), 20);

        // 63 fixed 16ms steps cover 1008ms, so a 20Hz schedule should emit 20
        // observations and retain the fractional phase for the next second.
        let emissions = (0..63)
            .filter(|_| schedule.after_tick(&game.snapshot()).is_some())
            .count();

        assert_eq!(emissions, 20);
    }

    #[test]
    fn emits_immediately_when_pause_state_changes() {
        let mut game = GameState::new(1);
        game.start();
        let mut schedule = ObservationSchedule::new(&game.snapshot(), 1);

        game.apply_action(GameAction::Pause);
        assert_eq!(schedule.after_tick(&game.snapshot()).unwrap().0, 1);
    }

    #[test]
    fn immediate_snapshots_share_the_monotonic_sequence() {
        let mut game = GameState::new(1);
        game.start();
        let mut schedule = ObservationSchedule::new(&game.snapshot(), 20);

        assert_eq!(schedule.immediate().0, 1);
        assert_eq!(schedule.immediate().0, 2);
    }

    #[test]
    fn captures_an_explicit_core_event_without_mutating_game() {
        let mut game = GameState::new(1);
        game.start();
        let mut schedule = ObservationSchedule::new(&game.snapshot(), 1);
        assert!(game.apply_action(GameAction::HardDrop));
        let event = game.take_last_event().expect("lock event");

        schedule.capture_event(event);
        let (_, observed) = schedule
            .after_tick(&game.snapshot())
            .expect("critical observation");

        assert!(observed.first().expect("event").locked);
    }
}
