use std::fs;
use std::path::{Path, PathBuf};

use tetris_core::core::pieces::get_kick_table;
use tetris_core::types::{LINE_SCORES, LOCK_DELAY_MS, LOCK_RESET_LIMIT, PieceKind};
use tetris_session::engine::replay::RULESET_VERSION;

const RULES_ROOT: &str = "protocol/rules";

fn project_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn read(relative: &str) -> String {
    fs::read_to_string(project_path(relative))
        .unwrap_or_else(|error| panic!("failed to read {relative}: {error}"))
}

#[test]
fn current_rules_package_matches_runtime_version() {
    let version = read(&format!("{RULES_ROOT}/VERSION"));
    let spec = read(&format!("{RULES_ROOT}/SPEC.md"));
    let readme = read(&format!("{RULES_ROOT}/README.md"));
    let changelog = read(&format!("{RULES_ROOT}/CHANGELOG.md"));

    assert_eq!(version.trim(), "1.1.0");
    assert_eq!(RULESET_VERSION, "guideline-ds-1.1.0");
    assert!(spec.contains("Ruleset 1.1.0"));
    assert!(spec.contains("guideline-ds-1.1.0"));
    assert!(readme.contains("portable rules"));
    assert!(readme.contains("notify dependent projects"));
    assert!(changelog.contains("## 1.1.0"));
    assert!(changelog.contains("## 1.0.0"));
    assert!(!project_path("protocol/rules/v1.1.0").exists());
}

#[test]
fn constants_json_matches_core_tables() {
    let constants: serde_json::Value =
        serde_json::from_str(&read(&format!("{RULES_ROOT}/constants.json")))
            .expect("constants.json must be valid JSON");

    assert_eq!(constants["version"], "1.1.0");
    assert_eq!(constants["replay_id"], RULESET_VERSION);
    assert_eq!(constants["b2b_zero_line_neutral"], true);
    assert_eq!(constants["tspin_mini_promotes_on_final_kick"], true);
    assert_eq!(constants["lock_delay_ms"], u64::from(LOCK_DELAY_MS));
    assert_eq!(constants["lock_reset_limit"], u64::from(LOCK_RESET_LIMIT));
    assert_eq!(constants["kick_y_up"], true);

    let line_scores = constants["line_scores"]
        .as_array()
        .expect("line_scores")
        .iter()
        .map(|value| value.as_u64().expect("line score") as u32)
        .collect::<Vec<_>>();
    assert_eq!(line_scores, LINE_SCORES);

    assert_eq!(LOCK_DELAY_MS, 500);
    assert_eq!(LINE_SCORES, [0, 100, 300, 500, 800]);
}

#[test]
fn y_down_kick_tables_negate_wiki_dy() {
    let constants: serde_json::Value =
        serde_json::from_str(&read(&format!("{RULES_ROOT}/constants.json")))
            .expect("constants.json must be valid JSON");

    let jlstz = get_kick_table(PieceKind::T);
    let wiki = constants["jlstz_kicks_y_up"]
        .as_array()
        .expect("jlstz kicks");
    for (row_index, row) in wiki.iter().enumerate() {
        for (kick_index, pair) in row.as_array().expect("kick row").iter().enumerate() {
            let dx = pair[0].as_i64().expect("dx") as i8;
            let dy_up = pair[1].as_i64().expect("dy") as i8;
            assert_eq!(jlstz[row_index][kick_index], (dx, -dy_up));
        }
    }

    let i_kicks = get_kick_table(PieceKind::I);
    let wiki_i = constants["i_kicks_y_up"].as_array().expect("i kicks");
    for (row_index, row) in wiki_i.iter().enumerate() {
        for (kick_index, pair) in row.as_array().expect("kick row").iter().enumerate() {
            let dx = pair[0].as_i64().expect("dx") as i8;
            let dy_up = pair[1].as_i64().expect("dy") as i8;
            assert_eq!(i_kicks[row_index][kick_index], (dx, -dy_up));
        }
    }
}

#[test]
fn shared_rules_exclude_local_timing_profile() {
    let spec = read(&format!("{RULES_ROOT}/SPEC.md"));
    let constants = read(&format!("{RULES_ROOT}/constants.json"));
    let shared = format!("{spec}\n{constants}");

    for forbidden in [
        "tui-tetris",
        "TICK_MS",
        "DEFAULT_DAS_MS",
        "LINE_CLEAR_PAUSE_MS",
        "DROP_INTERVALS",
        "TUI_TETRIS_KEY_RELEASE_TIMEOUT_MS",
    ] {
        assert!(
            !shared.contains(forbidden),
            "shared rules leaked implementation detail: {forbidden}"
        );
    }

    assert!(spec.contains("negate kick `dy`"));
    assert!(spec.contains("out of scope"));
}

#[test]
fn local_rules_profile_points_at_shared_spec() {
    let profile = read("docs/rules-spec.md");
    assert!(profile.contains("protocol/rules/SPEC.md"));
    assert!(profile.contains("guideline-ds-1.1.0"));
    assert!(profile.contains("LOCK_DELAY_MS"));
    assert!(profile.contains("DROP_INTERVALS"));
    assert!(profile.contains("DAS"));
}

#[test]
fn spec_states_zero_line_neutrality_and_kick_promotion() {
    let spec = read(&format!("{RULES_ROOT}/SPEC.md"));

    // A lock that clears nothing is neutral for back-to-back, T-Spin or not.
    assert!(spec.contains("clears 0 lines MUST leave the back-to-back chain unchanged"));
    assert!(!spec.contains("A lock with 0 lines and no T-Spin MUST reset"));

    // Mini promotes to Full when the final SRS 1x2 kick reached the slot.
    assert!(spec.contains("Kick promotion"));
    assert!(spec.contains("final offset of the SRS kick table"));
}
