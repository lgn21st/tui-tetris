//! Deterministic observation cadence shared by interactive and headless runners.

use crate::adapter::protocol::LastEvent;
use crate::core::GameState;
use crate::types::TICK_MS;

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
    pending_last_event: Option<LastEvent>,
}

impl ObservationSchedule {
    pub fn new(game: &GameState, frequency_hz: u32) -> Self {
        Self {
            frequency_hz: frequency_hz.clamp(1, 60),
            accumulated_frequency_units: 0,
            seq: 0,
            last_episode_id: game.episode_id(),
            last_piece_id: game.piece_id(),
            last_active_id: game.active_id(),
            last_paused: game.paused(),
            last_game_over: game.game_over(),
            pending_last_event: None,
        }
    }

    pub fn from_env(game: &GameState) -> Self {
        let frequency_hz = std::env::var("TETRIS_AI_OBS_HZ")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(20);
        Self::new(game, frequency_hz)
    }

    pub fn capture_core_event(&mut self, game: &mut GameState) {
        if let Some(event) = game.take_last_event() {
            self.pending_last_event = Some(event.into());
        }
    }

    pub fn immediate(&mut self) -> (u64, Option<LastEvent>) {
        self.seq = self.seq.wrapping_add(1);
        (self.seq, self.pending_last_event.take())
    }

    pub fn after_tick(&mut self, game: &mut GameState) -> Option<(u64, Option<LastEvent>)> {
        let mut critical = false;
        critical |= update_changed(&mut self.last_piece_id, game.piece_id());
        critical |= update_changed(&mut self.last_active_id, game.active_id());
        critical |= update_changed(&mut self.last_episode_id, game.episode_id());
        critical |= update_changed(&mut self.last_paused, game.paused());
        critical |= update_changed(&mut self.last_game_over, game.game_over());

        if let Some(event) = game.take_last_event() {
            self.pending_last_event = Some(event.into());
            critical = true;
        }

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
    use crate::types::GameAction;

    #[test]
    fn emits_at_configured_fixed_step_interval() {
        let mut game = GameState::new(1);
        game.start();
        let mut schedule = ObservationSchedule::new(&game, 20);

        assert!(schedule.after_tick(&mut game).is_none());
        assert!(schedule.after_tick(&mut game).is_none());
        assert!(schedule.after_tick(&mut game).is_none());
        assert_eq!(schedule.after_tick(&mut game).unwrap().0, 1);
    }

    #[test]
    fn preserves_requested_frequency_without_integer_period_drift() {
        let mut game = GameState::new(1);
        game.start();
        let mut schedule = ObservationSchedule::new(&game, 20);

        // 63 fixed 16ms steps cover 1008ms, so a 20Hz schedule should emit 20
        // observations and retain the fractional phase for the next second.
        let emissions = (0..63)
            .filter(|_| schedule.after_tick(&mut game).is_some())
            .count();

        assert_eq!(emissions, 20);
    }

    #[test]
    fn emits_immediately_when_pause_state_changes() {
        let mut game = GameState::new(1);
        game.start();
        let mut schedule = ObservationSchedule::new(&game, 1);

        game.apply_action(GameAction::Pause);
        assert_eq!(schedule.after_tick(&mut game).unwrap().0, 1);
    }

    #[test]
    fn immediate_snapshots_share_the_monotonic_sequence() {
        let mut game = GameState::new(1);
        game.start();
        let mut schedule = ObservationSchedule::new(&game, 20);

        assert_eq!(schedule.immediate().0, 1);
        assert_eq!(schedule.immediate().0, 2);
    }
}
