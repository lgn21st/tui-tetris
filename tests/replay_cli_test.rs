use std::path::PathBuf;

use tui_tetris::replay_cli::{ReplayCommand, parse_replay_args, run_replay_command};

fn temp_replay() -> PathBuf {
    std::env::temp_dir().join(format!("tui-tetris-replay-{}.ttr", std::process::id()))
}

#[test]
fn replay_subcommands_have_an_explicit_stable_surface() {
    let path = temp_replay();
    let record = parse_replay_args(&[
        "replay".into(),
        "record".into(),
        path.display().to_string(),
        "--seed".into(),
        "9".into(),
        "--steps".into(),
        "3".into(),
    ])
    .unwrap();
    assert_eq!(
        record,
        Some(ReplayCommand::Record {
            path: path.clone(),
            seed: 9,
            steps: 3,
        })
    );
    assert_eq!(
        parse_replay_args(&["replay".into(), "verify".into(), path.display().to_string()]).unwrap(),
        Some(ReplayCommand::Verify { path: path.clone() })
    );
}

#[test]
fn record_verify_and_inspect_form_a_complete_cli_loop() {
    let path = temp_replay();
    let _ = std::fs::remove_file(&path);
    let recorded = run_replay_command(ReplayCommand::Record {
        path: path.clone(),
        seed: 17,
        steps: 4,
    })
    .unwrap();
    assert!(recorded.contains("recorded 4 steps"));

    let verified = run_replay_command(ReplayCommand::Verify { path: path.clone() }).unwrap();
    assert!(verified.contains("verified 4 steps"));

    let inspected = run_replay_command(ReplayCommand::Inspect { path: path.clone() }).unwrap();
    assert!(inspected.contains("seed: 17"));
    assert!(inspected.contains("steps: 4"));
    assert!(inspected.contains("ruleset:"));
    std::fs::remove_file(path).unwrap();
}
