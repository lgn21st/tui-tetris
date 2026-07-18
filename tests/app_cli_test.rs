use tui_tetris::app_cli::{parse_app_args, run_batch_headless, AppCommand, HeadlessConfig};

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
    };
    let first = run_batch_headless(config).unwrap();
    let second = run_batch_headless(config).unwrap();
    assert_eq!(first, second);
    assert!(first.contains("steps=100"));
    assert!(first.contains("state_hash="));
}
