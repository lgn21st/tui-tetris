use std::path::PathBuf;

use tui_tetris::app_cli::Budget;
use tui_tetris::replay_cli::{ReplayCommand, parse_replay_args, run_replay_command};

fn temp_replay(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "tui-tetris-replay-{}-{name}.ttr",
        std::process::id()
    ))
}

#[test]
fn replay_subcommands_have_an_explicit_stable_surface() {
    let path = temp_replay("surface");
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
    let path = temp_replay("loop");
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

#[test]
fn replay_eval_parses_a_budget_and_scores_a_complete_run() {
    let path = temp_replay("eval-complete");
    let _ = std::fs::remove_file(&path);

    assert_eq!(
        parse_replay_args(&[
            "replay".into(),
            "eval".into(),
            path.display().to_string(),
            "--budget-lines".into(),
            "2".into(),
        ])
        .unwrap(),
        Some(ReplayCommand::Eval {
            path: path.clone(),
            budget: Budget {
                max_lines: Some(2),
                max_pieces: None,
            },
        })
    );

    run_replay_command(ReplayCommand::Record {
        path: path.clone(),
        seed: 17,
        steps: 40,
    })
    .unwrap();
    let evaluated = run_replay_command(ReplayCommand::Eval {
        path: path.clone(),
        budget: Budget {
            max_lines: Some(2),
            max_pieces: None,
        },
    })
    .unwrap();

    // Nothing clears a line in 40 no-input steps, so the whole tape is scored.
    assert!(evaluated.contains("budget: lines:2"));
    assert!(evaluated.contains("steps: 40"));
    assert!(evaluated.contains("budget_reached: 0"));
    assert!(evaluated.contains("replay_check: ok"));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn replay_eval_stops_early_when_the_budget_is_consumed() {
    let path = temp_replay("eval-bounded");
    let _ = std::fs::remove_file(&path);

    run_replay_command(ReplayCommand::Record {
        path: path.clone(),
        seed: 17,
        steps: 6000,
    })
    .unwrap();
    let evaluated = run_replay_command(ReplayCommand::Eval {
        path: path.clone(),
        budget: Budget {
            max_lines: None,
            max_pieces: Some(3),
        },
    })
    .unwrap();

    // A full replay would report 6000 steps; stopping at three pieces shows the
    // budget bound the run.
    assert!(evaluated.contains("budget: pieces:3"));
    assert!(evaluated.contains("budget_reached: 1"));
    assert!(evaluated.contains("pieces: 3"));
    assert!(evaluated.contains("replay_check: ok"));
    std::fs::remove_file(path).unwrap();
}
