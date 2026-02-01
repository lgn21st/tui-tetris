//! Input module - Keyboard handling for game controls

use crate::types::GameAction;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Map keyboard input to game actions
pub fn handle_key_event(key: KeyEvent) -> Option<GameAction> {
    match key.code {
        // Movement
        KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('a') => Some(GameAction::MoveLeft),
        KeyCode::Right | KeyCode::Char('l') | KeyCode::Char('d') => Some(GameAction::MoveRight),
        KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('s') => Some(GameAction::SoftDrop),

        // Rotation
        KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('w') => Some(GameAction::RotateCw),
        KeyCode::Char('z') | KeyCode::Char('y') => Some(GameAction::RotateCcw),

        // Actions
        KeyCode::Char(' ') => Some(GameAction::HardDrop),
        KeyCode::Char('c') | KeyCode::Char('C') => Some(GameAction::Hold),
        KeyCode::Char('p') | KeyCode::Char('P') => Some(GameAction::Pause),

        // Restart
        KeyCode::Char('r') | KeyCode::Char('R') => Some(GameAction::Restart),

        _ => None,
    }
}

/// Check if key should quit the game
pub fn should_quit(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q'))
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn test_movement_keys() {
        assert_eq!(
            handle_key_event(KeyEvent::from(KeyCode::Left)),
            Some(GameAction::MoveLeft)
        );
        assert_eq!(
            handle_key_event(KeyEvent::from(KeyCode::Right)),
            Some(GameAction::MoveRight)
        );
        assert_eq!(
            handle_key_event(KeyEvent::from(KeyCode::Down)),
            Some(GameAction::SoftDrop)
        );
    }

    #[test]
    fn test_rotation_keys() {
        assert_eq!(
            handle_key_event(KeyEvent::from(KeyCode::Up)),
            Some(GameAction::RotateCw)
        );
        assert_eq!(
            handle_key_event(KeyEvent::from(KeyCode::Char('z'))),
            Some(GameAction::RotateCcw)
        );
    }

    #[test]
    fn test_action_keys() {
        assert_eq!(
            handle_key_event(KeyEvent::from(KeyCode::Char(' '))),
            Some(GameAction::HardDrop)
        );
        assert_eq!(
            handle_key_event(KeyEvent::from(KeyCode::Char('c'))),
            Some(GameAction::Hold)
        );
        assert_eq!(
            handle_key_event(KeyEvent::from(KeyCode::Char('p'))),
            Some(GameAction::Pause)
        );
    }

    #[test]
    fn test_quit_keys() {
        assert!(should_quit(KeyEvent::from(KeyCode::Char('q'))));
        assert!(should_quit(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL
        )));
        assert!(!should_quit(KeyEvent::from(KeyCode::Char('x'))));
    }
}
