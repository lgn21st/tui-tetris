use tui_tetris::app_cli::{AppCommand, Budget, HeadlessConfig, parse_app_args, run_batch_headless};

#[test]
fn headless_and_diagnostic_commands_are_explicit() {
    assert_eq!(
        parse_app_args(&[
            "headless".into(),
            "--seed".into(),
            "5".into(),
            "--steps".into(),
            "12".into(),
        ])
        .unwrap(),
        Some(AppCommand::Headless(HeadlessConfig {
            seed: 5,
            steps: Some(12),
            budget: Budget::default(),
        }))
    );
    assert_eq!(
        parse_app_args(&["diagnostic".into()]).unwrap(),
        Some(AppCommand::Diagnostic)
    );
}

#[test]
fn finite_headless_mode_is_deterministic_and_terminates() {
    let config = HeadlessConfig {
        seed: 8,
        steps: Some(100),
        budget: Budget::default(),
    };
    let first = run_batch_headless(config).unwrap();
    let second = run_batch_headless(config).unwrap();
    assert_eq!(first, second);
    assert!(first.contains("steps=100"));
    assert!(first.contains("state_hash="));
    assert!(first.contains("budget=none"));
}

#[test]
fn budget_flags_parse_into_a_bounded_run() {
    assert_eq!(
        parse_app_args(&[
            "headless".into(),
            "--seed".into(),
            "3".into(),
            "--steps".into(),
            "5000".into(),
            "--budget-lines".into(),
            "12".into(),
        ])
        .unwrap(),
        Some(AppCommand::Headless(HeadlessConfig {
            seed: 3,
            steps: Some(5000),
            budget: Budget {
                max_lines: Some(12),
                max_pieces: None,
            },
        }))
    );

    let both = parse_app_args(&[
        "headless".into(),
        "--budget-lines".into(),
        "150".into(),
        "--budget-pieces".into(),
        "500".into(),
    ])
    .unwrap();
    let Some(AppCommand::Headless(config)) = both else {
        panic!("expected a headless command");
    };
    assert_eq!(
        config.budget,
        Budget {
            max_lines: Some(150),
            max_pieces: Some(500),
        }
    );
    assert_eq!(config.budget.describe(), "lines:150,pieces:500");

    assert!(parse_app_args(&["headless".into(), "--budget-lines".into(), "x".into()]).is_err());
}

#[test]
fn a_piece_budget_stops_the_run_and_is_reported() {
    // With no input the run would otherwise continue to its step limit, so
    // stopping at exactly three pieces shows the budget bound the run.
    let report = run_batch_headless(HeadlessConfig {
        seed: 8,
        steps: Some(50_000),
        budget: Budget {
            max_lines: None,
            max_pieces: Some(3),
        },
    })
    .unwrap();

    assert!(report.contains("budget=pieces:3"));
    assert!(report.contains("budget_reached=1"));
    assert!(report.contains("pieces=3"));
    assert!(report.contains("requested_steps=50000"));
    assert!(report.contains("state_hash="));
}
