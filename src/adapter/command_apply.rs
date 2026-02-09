use crate::adapter::protocol::ErrorCode;
use crate::adapter::runtime::ClientCommand;
use crate::core::GameState;
use crate::engine::place::{apply_place, PlaceError};
use crate::types::GameAction;

pub fn apply_client_command(
    game_state: &mut GameState,
    cmd: ClientCommand,
) -> Result<(), PlaceError> {
    match cmd {
        ClientCommand::Actions {
            actions,
            mut restart_seed,
        } => {
            for action in actions {
                if action == GameAction::Restart {
                    if let Some(seed) = restart_seed.take() {
                        let _ = game_state.restart_with_seed(seed);
                        continue;
                    }
                }
                let _ = game_state.apply_action(action);
            }
            Ok(())
        }
        ClientCommand::Place {
            x,
            rotation,
            use_hold,
        } => apply_place(game_state, x, rotation, use_hold),
    }
}

pub fn map_place_error_code(err: PlaceError) -> ErrorCode {
    match err {
        PlaceError::HoldUnavailable => ErrorCode::HoldUnavailable,
        PlaceError::RotationBlocked
        | PlaceError::XOutOfBounds
        | PlaceError::XBlocked
        | PlaceError::NotPlayable
        | PlaceError::NoActive => ErrorCode::InvalidPlace,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrayvec::ArrayVec;

    #[test]
    fn apply_actions_with_restart_seed_restarts_episode() {
        let mut game_state = GameState::new(1);
        game_state.start();
        let previous_episode = game_state.episode_id();

        let mut actions = ArrayVec::<GameAction, 32>::new();
        actions.push(GameAction::Restart);
        let cmd = ClientCommand::Actions {
            actions,
            restart_seed: Some(42),
        };

        let result = apply_client_command(&mut game_state, cmd);
        assert!(result.is_ok());
        assert!(game_state.episode_id() > previous_episode);
        assert_eq!(game_state.snapshot().seed, 42);
    }

    #[test]
    fn apply_place_returns_not_playable_when_paused() {
        let mut game_state = GameState::new(1);
        game_state.start();
        assert!(game_state.apply_action(GameAction::Pause));
        let active = game_state.active().expect("active piece");
        let cmd = ClientCommand::Place {
            x: active.x,
            rotation: active.rotation,
            use_hold: false,
        };

        let err = apply_client_command(&mut game_state, cmd).unwrap_err();
        assert_eq!(err, PlaceError::NotPlayable);
    }

    #[test]
    fn map_place_error_code_maps_hold_unavailable() {
        assert_eq!(
            map_place_error_code(PlaceError::HoldUnavailable),
            ErrorCode::HoldUnavailable
        );
        assert_eq!(
            map_place_error_code(PlaceError::RotationBlocked),
            ErrorCode::InvalidPlace
        );
    }
}
