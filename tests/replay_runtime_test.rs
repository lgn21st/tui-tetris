use tetris_core::types::{CoreLastEvent, GameAction, TSpinKind};
use tetris_session::engine::replay::{
    REPLAY_FORMAT_VERSION, RULESET_VERSION, ReplayMismatch, ReplayTape, StepRecord,
    replay_and_verify, transition_hash,
};
use tetris_session::engine::session::{GameCommand, SessionRuntime, StepInput};

fn sample_batches() -> Vec<StepInput> {
    vec![
        StepInput::default().with_local(GameAction::MoveLeft),
        StepInput::default().with_remote(GameCommand::action(GameAction::RotateCw)),
        StepInput::default().with_local(GameAction::HardDrop),
    ]
}

#[test]
fn recorded_tape_replays_to_the_same_step_hashes() {
    let tape = ReplayTape::record(42, sample_batches());
    let replayed = replay_and_verify(&tape).expect("valid replay");

    assert_eq!(replayed.snapshot(), tape.final_snapshot());
    assert_eq!(tape.records().len(), 3);
}

#[test]
fn replay_reports_the_first_mismatching_step_and_minimal_prefix() {
    let mut tape = ReplayTape::record(42, sample_batches());
    tape.replace_record_for_test(
        1,
        StepRecord {
            state_hash: 0,
            ..tape.records()[1].clone()
        },
    );

    let error = replay_and_verify(&tape).unwrap_err();
    assert_eq!(
        error,
        ReplayMismatch {
            step: 1,
            expected: 0,
            actual: error.actual,
        }
    );
    let prefix = tape.minimal_failure_prefix(&error);
    assert_eq!(prefix.records().len(), 2);
    assert_eq!(replay_and_verify(&prefix).unwrap_err(), error);
}

#[test]
fn equal_command_trajectories_are_byte_stable() {
    let first = ReplayTape::record(7, sample_batches()).encode();
    let second = ReplayTape::record(7, sample_batches()).encode();
    assert_eq!(first, second);

    let decoded = ReplayTape::decode(&first).expect("stable replay encoding");
    assert_eq!(decoded, ReplayTape::record(7, sample_batches()));
}

#[test]
fn replay_header_versions_the_container_and_ruleset() {
    let tape = ReplayTape::record(7, sample_batches());
    let encoded = String::from_utf8(tape.encode()).unwrap();
    assert!(encoded.starts_with(&format!(
        "TTR{REPLAY_FORMAT_VERSION}\t{RULESET_VERSION}\t7\n"
    )));
    assert_eq!(tape.ruleset_version(), RULESET_VERSION);

    let incompatible = encoded.replacen(RULESET_VERSION, "incompatible-rules", 1);
    assert!(
        ReplayTape::decode(incompatible.as_bytes())
            .unwrap_err()
            .contains("unsupported ruleset")
    );
}

#[test]
fn transition_hash_covers_logical_step_and_every_event() {
    let session = SessionRuntime::new(11);
    let event = CoreLastEvent {
        locked: true,
        lines_cleared: 1,
        line_clear_score: 40,
        tspin: Some(TSpinKind::Mini),
        combo: 2,
        back_to_back: false,
    };
    let second = CoreLastEvent {
        lines_cleared: 2,
        ..event
    };

    let one = transition_hash(session.snapshot(), 1, &[event], &[]);
    let two = transition_hash(session.snapshot(), 1, &[event, second], &[]);
    let next_step = transition_hash(session.snapshot(), 2, &[event], &[]);
    assert_ne!(one, two);
    assert_ne!(one, next_step);
}

#[test]
fn command_batch_obeys_session_capacity_without_heap_growth() {
    let mut session = SessionRuntime::new(1);
    let batch = StepInput::default()
        .with_remote(GameCommand::action(GameAction::MoveRight))
        .with_local(GameAction::SoftDrop);

    let result = session.transition(&batch);
    assert_eq!(result.command_outcomes.len(), 1);
}
