use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tetris_core::core::GameState;
use tetris_core::types::GameAction;
use tetris_terminal::input::{InputCommand, map_input_command};
use tetris_terminal::term::{AdapterStatusView, GameViewModel};

#[test]
fn terminal_projection_owns_an_immutable_render_model() {
    let mut game = GameState::new(1);
    game.start();
    let snapshot = game.snapshot();
    let adapter = AdapterStatusView {
        enabled: true,
        client_count: 2,
        controller_id: Some(1),
        streaming_count: 1,
        pid: 7,
        listen_addr: None,
    };
    let model = GameViewModel::new(snapshot, Some(adapter));

    assert_eq!(model.snapshot().seed, 1);
    assert_eq!(model.adapter(), Some(&adapter));
}

#[test]
fn platform_key_events_map_to_platform_neutral_commands() {
    let left = KeyEvent::new(KeyCode::Left, KeyModifiers::NONE);
    let quit = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);

    assert_eq!(
        map_input_command(left),
        Some(InputCommand::Action(GameAction::MoveLeft))
    );
    assert_eq!(map_input_command(quit), Some(InputCommand::Quit));
}
