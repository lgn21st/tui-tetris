use std::path::Path;

use tui_tetris::adapter::protocol::PROTOCOL_VERSION;

const ADAPTER_DOC: &str = include_str!("../docs/adapter.md");

fn json_blocks() -> Vec<serde_json::Value> {
    ADAPTER_DOC
        .split("```json\n")
        .skip(1)
        .map(|rest| {
            let block = rest
                .split_once("\n```")
                .expect("unterminated JSON code block")
                .0;
            serde_json::from_str(block).expect("adapter documentation contains invalid JSON")
        })
        .collect()
}

#[test]
fn adapter_doc_version_matches_protocol_constant() {
    assert!(ADAPTER_DOC.contains(&format!("Standard (v{PROTOCOL_VERSION})")));
    assert!(ADAPTER_DOC.contains(&format!("Protocol version: `{PROTOCOL_VERSION}`")));
}

#[test]
fn adapter_doc_json_examples_and_schema_are_valid_json() {
    let blocks = json_blocks();
    assert!(blocks.len() >= 8, "expected message examples plus schema");
    assert!(blocks.iter().any(|value| value.get("$schema").is_some()));
}

#[test]
fn documented_observation_example_is_a_full_snapshot() {
    let observation = json_blocks()
        .into_iter()
        .find(|value| value.get("type").and_then(|kind| kind.as_str()) == Some("observation"))
        .expect("missing observation example");
    let board = observation["board"].as_object().expect("board object");
    assert_eq!(board["width"], 10);
    assert_eq!(board["height"], 20);
    let rows = board["cells"].as_array().expect("board rows");
    assert_eq!(rows.len(), 20);
    assert!(rows
        .iter()
        .all(|row| row.as_array().is_some_and(|cells| cells.len() == 10)));
    assert_eq!(observation["next"], observation["next_queue"][0]);
}

#[test]
fn schema_requires_v21_capability_partition_and_hash_shape() {
    let schema = json_blocks()
        .into_iter()
        .find(|value| value.get("$schema").is_some())
        .expect("missing JSON Schema appendix");
    let required = schema["definitions"]["capabilities"]["required"]
        .as_array()
        .expect("capabilities.required");
    for field in [
        "features",
        "features_always",
        "features_optional",
        "control_policy",
    ] {
        assert!(required.iter().any(|value| value.as_str() == Some(field)));
    }
    assert_eq!(
        schema["definitions"]["observation"]["properties"]["state_hash"]["pattern"],
        "^[0-9a-f]{16}$"
    );
}

#[test]
fn maintained_verification_script_exists() {
    assert!(ADAPTER_DOC.contains("scripts/adapter_verify.py"));
    assert!(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scripts/adapter_verify.py")
        .is_file());
}
