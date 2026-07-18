use tui_tetris::engine::session::{GameCommand, SessionRuntime, StepInput};
use tui_tetris::types::GameAction;

#[test]
fn one_step_input_produces_one_transition_with_an_event_collection() {
    let mut session = SessionRuntime::new(1);
    let input = StepInput::default()
        .with_remote(GameCommand::action(GameAction::MoveLeft))
        .with_local(GameAction::HardDrop);

    let transition = session.transition(&input);

    assert_eq!(transition.command_outcomes.len(), 1);
    assert_eq!(transition.events.len(), 1);
    assert!(transition.events[0].locked);
    assert_eq!(session.logical_step(), 1);
}

#[test]
fn empty_step_input_still_advances_one_logical_tick() {
    let mut session = SessionRuntime::new(1);
    let before = session.snapshot().timers.drop_ms;

    let transition = session.transition(&StepInput::default());

    assert!(transition.changed);
    assert_eq!(session.logical_step(), 1);
    assert!(session.snapshot().timers.drop_ms > before);
}
