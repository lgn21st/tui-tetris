//! Integration tests for main game loop

use tui_tetris::core::GameState;
use tui_tetris::types::{GameAction, PieceKind};
use tui_tetris::ui::InputHandler;

#[test]
fn test_game_lifecycle() {
    // Create and start game
    let mut state = GameState::new(12345);
    assert!(!state.started);

    state.start();
    assert!(state.started);
    assert!(state.active.is_some());
    assert!(!state.game_over);
    assert!(!state.paused);
}

#[test]
fn test_game_actions() {
    let mut state = GameState::new(12345);
    state.start();

    // Get initial position
    let initial_x = state.active.unwrap().x;
    let initial_y = state.active.unwrap().y;

    // Move left (may fail at left edge, but should try)
    let moved = state.try_move(-1, 0);
    if moved {
        assert_eq!(state.active.unwrap().x, initial_x - 1);
    }

    // Rotate (may or may not succeed depending on piece position)
    state.try_rotate(true);

    // Move down (soft drop)
    let dropped = state.try_move(0, 1);
    if dropped {
        assert!(state.active.unwrap().y > initial_y);
    }

    // Verify game state is still valid
    assert!(state.active.is_some());
    assert!(!state.game_over);
}

#[test]
fn test_input_handler_integration() {
    use crossterm::event::KeyCode;

    let mut input = InputHandler::new();

    // Simulate pressing left key
    input.handle_key_press(KeyCode::Left);

    // First update: 166ms (DAS not yet triggered)
    let actions = input.update(166);
    assert!(actions.is_empty(), "DAS should not trigger at 166ms");

    // Second update: 100ms more (DAS triggers at 167ms, 33ms ARR = 1 action)
    let actions = input.update(100);
    assert!(
        !actions.is_empty(),
        "DAS should trigger and generate ARR action"
    );
    assert_eq!(actions[0], GameAction::MoveLeft);

    // Third update: Another 100ms (should generate ~3 more ARR actions)
    let actions = input.update(100);
    assert!(actions.len() >= 2, "Should generate multiple ARR actions");
}

#[test]
fn test_game_pause() {
    let mut state = GameState::new(12345);
    state.start();

    // Pause game
    state.apply_action(GameAction::Pause);
    assert!(state.paused);

    // Resume
    state.apply_action(GameAction::Pause);
    assert!(!state.paused);
}

#[test]
fn test_hold_piece() {
    let mut state = GameState::new(12345);
    state.start();

    let initial_hold = state.hold;
    let initial_piece = state.active.unwrap().kind;

    // Hold piece
    state.apply_action(GameAction::Hold);

    // Hold should work (can_hold starts as true)
    if state.can_hold {
        assert!(state.hold.is_some());
        assert!(!state.can_hold); // Can only hold once per piece
    }
}

#[test]
fn test_line_clear() {
    let mut state = GameState::new(12345);
    state.start();

    // Fill a row to trigger line clear
    for x in 0..10 {
        state.board.set(x, 19, Some(PieceKind::I));
    }

    let initial_lines = state.lines;

    // Clear lines
    let cleared = state.board.clear_full_rows();

    // Should have cleared row 19
    assert!(!cleared.is_empty());
}

#[test]
fn test_game_restart() {
    let mut state = GameState::new(12345);
    state.start();

    // Play a bit
    state.hard_drop();

    // Restart
    state = GameState::new(12345);
    state.start();

    // Should be fresh state
    assert_eq!(state.score, 0);
    assert_eq!(state.lines, 0);
    assert_eq!(state.level, 0);
}
