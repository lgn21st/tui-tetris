use tui_tetris::engine::session::{GameCommand, SessionRuntime, StepInput};
use tui_tetris::types::{GameAction, TICK_MS};

#[test]
fn session_applies_remote_commands_before_local_actions_and_ticks_once() {
    let mut session = SessionRuntime::new(1);
    let input = StepInput::default()
        .with_remote(GameCommand::action(GameAction::HardDrop))
        .with_local(GameAction::MoveLeft);

    let first_piece_id = session.game().piece_id();
    let result = session.transition(&input);

    assert_eq!(result.command_outcomes.len(), 1);
    assert!(result.command_outcomes[0].is_ok());
    assert_eq!(session.game().piece_id(), first_piece_id + 1);
    assert_eq!(session.game().step_in_piece(), 1);

    let active = session.game().active().expect("new active piece");
    assert_eq!(active.x, 2, "local movement must target the new piece");
}

#[test]
fn session_returns_each_core_event_once() {
    let mut session = SessionRuntime::new(1);
    let input = StepInput::default().with_remote(GameCommand::action(GameAction::HardDrop));

    let locked = session.transition(&input);
    assert_eq!(locked.events.len(), 1);

    let next = session.transition(&StepInput::default());
    assert!(next.events.is_empty());
}

#[test]
fn session_reports_metadata_only_tick_changes() {
    let mut session = SessionRuntime::new(1);
    let before = session.snapshot().step_in_piece;

    let result = session.transition(&StepInput::default());

    assert!(result.changed);
    assert_eq!(session.snapshot().step_in_piece, before + 1);
}

#[test]
fn session_snapshot_store_keeps_board_and_metadata_coherent() {
    let mut session = SessionRuntime::new(1);
    let initial_board_id = session.snapshot().board_id;
    let initial_piece_id = session.snapshot().piece_id;

    let input = StepInput::default().with_remote(GameCommand::action(GameAction::HardDrop));
    let result = session.transition(&input);

    assert!(result.changed);
    assert!(session.snapshot().board_id > initial_board_id);
    assert!(session.snapshot().piece_id > initial_piece_id);
    assert_eq!(session.snapshot().board_id, session.game().board_id());
    assert_eq!(session.snapshot().piece_id, session.game().piece_id());
    assert!(session
        .snapshot()
        .board
        .iter()
        .flatten()
        .any(|cell| *cell != 0));
}

#[test]
fn equal_session_inputs_produce_equal_snapshots() {
    let mut a = SessionRuntime::new(42);
    let mut b = SessionRuntime::new(42);
    let commands = [
        GameCommand::action(GameAction::MoveLeft),
        GameCommand::action(GameAction::RotateCw),
    ];
    let mut input = StepInput::default();
    input.remote.extend(commands);

    for _ in 0..100 {
        let a_result = a.transition(&input);
        let b_result = b.transition(&input);
        assert_eq!(a_result.events, b_result.events);
        assert_eq!(a.snapshot(), b.snapshot());
        assert_eq!(a.game().step_in_piece(), b.game().step_in_piece());
        assert_eq!(TICK_MS, 16);
    }
}
