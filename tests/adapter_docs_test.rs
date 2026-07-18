use std::fs;
use std::path::{Path, PathBuf};

use tui_tetris::adapter::protocol::PROTOCOL_VERSION;
use tui_tetris::adapter::server::{CLIENT_RELIABLE_QUEUE_CAPACITY, WIRE_LOG_QUEUE_CAPACITY};

const PROTOCOL_ROOT: &str = "protocol/adapter";

fn project_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn read(relative: &str) -> String {
    fs::read_to_string(project_path(relative))
        .unwrap_or_else(|error| panic!("failed to read {relative}: {error}"))
}

#[test]
fn current_protocol_package_matches_runtime_version() {
    let version = read(&format!("{PROTOCOL_ROOT}/VERSION"));
    let spec = read(&format!("{PROTOCOL_ROOT}/SPEC.md"));
    let readme = read(&format!("{PROTOCOL_ROOT}/README.md"));

    assert_eq!(version.trim(), PROTOCOL_VERSION);
    assert!(spec.contains(&format!("Protocol {PROTOCOL_VERSION}")));
    assert!(readme.contains("single current protocol package"));
    assert!(readme.contains("notify dependent projects"));
    assert!(!project_path("protocol/adapter/v2.1.1").exists());
}

#[test]
fn protocol_schema_is_standalone_and_matches_core_contract() {
    let schema: serde_json::Value =
        serde_json::from_str(&read(&format!("{PROTOCOL_ROOT}/schema.json")))
            .expect("protocol schema must be valid JSON");

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
    assert_eq!(
        schema["definitions"]["board"]["properties"]["width"]["const"],
        10
    );
    assert_eq!(
        schema["definitions"]["board"]["properties"]["height"]["const"],
        20
    );

    let promotion_order = &schema["definitions"]["capabilities"]["properties"]["control_policy"]
        ["properties"]["promotion_order"];
    assert_eq!(promotion_order["type"], "string");
    assert_eq!(promotion_order["minLength"], 1);
    assert!(
        promotion_order.get("enum").is_none(),
        "the portable schema must not prescribe a local promotion policy"
    );

    let strict_v3_semver = concat!(
        "^3\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)",
        "(?:-(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)",
        "(?:\\.(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*))*)?",
        "(?:\\+[0-9A-Za-z-]+(?:\\.[0-9A-Za-z-]+)*)?$"
    );
    assert_eq!(
        schema["definitions"]["hello"]["properties"]["protocol_version"]["pattern"],
        strict_v3_semver
    );
    assert_eq!(
        schema["definitions"]["welcome"]["properties"]["protocol_version"]["pattern"],
        strict_v3_semver
    );

    for command in schema["definitions"]["command"]["oneOf"]
        .as_array()
        .expect("command.oneOf")
    {
        assert_eq!(command["properties"]["seq"]["minimum"], 0);
    }
    for message in ["ack", "error"] {
        assert_eq!(
            schema["definitions"][message]["properties"]["seq"]["minimum"],
            0
        );
    }
}

#[test]
fn shared_release_excludes_tui_tetris_implementation_details() {
    let spec = read(&format!("{PROTOCOL_ROOT}/SPEC.md"));
    let tcp_profile = read(&format!("{PROTOCOL_ROOT}/profiles/tcp-json-lines.md"));
    let shared = format!("{spec}\n{tcp_profile}");

    for forbidden in [
        "tui-tetris",
        "Tokio",
        "Arc fanout",
        "phase accumulator",
        "TETRIS_AI_LOG_PATH",
        "bounded reliable queue of 32",
        "1,024-record queue",
    ] {
        assert!(
            !shared.contains(forbidden),
            "shared release leaked implementation detail: {forbidden}"
        );
    }

    assert!(spec.contains("Place application MUST be atomic"));
    assert!(tcp_profile.contains("65,536 payload bytes"));
    for variable in ["TETRIS_AI_HOST", "TETRIS_AI_PORT", "TETRIS_AI_DISABLED"] {
        assert!(tcp_profile.contains(variable));
    }
}

#[test]
fn tui_tetris_index_and_profile_are_separate_from_shared_spec() {
    let index = read("docs/adapter.md");
    let profile = read("docs/adapter-tui-tetris.md");

    assert!(index.contains("protocol/adapter/SPEC.md"));
    assert!(index.contains("docs/adapter-tui-tetris.md"));
    assert!(profile.contains("fixed-step phase accumulator"));
    assert!(profile.contains(&format!(
        "reliable queue capacity: `{CLIENT_RELIABLE_QUEUE_CAPACITY}`"
    )));
    assert_eq!(WIRE_LOG_QUEUE_CAPACITY, 1024);
    assert!(profile.contains("wire-log queue capacity: `1,024`"));
}

#[test]
fn current_conformance_client_and_local_wrapper_exist() {
    let conformance = project_path(&format!("{PROTOCOL_ROOT}/conformance/adapter_verify.py"));
    assert!(conformance.is_file());
    assert!(project_path("scripts/adapter_verify.py").is_file());

    let source = read(&format!("{PROTOCOL_ROOT}/conformance/adapter_verify.py"));
    assert!(source.contains(&format!("PROTOCOL_VERSION = \"{PROTOCOL_VERSION}\"")));
}

#[test]
fn protocol_v3_has_a_dependent_client_migration_notice() {
    let migration = fs::read_to_string(project_path("docs/protocol-v3-migration.md"))
        .expect("protocol v3 migration notice");
    for required in [
        "2.1.1",
        "3.0.0",
        "last_event",
        "events",
        "logical_step",
        "correlation_seq",
        "applied_step",
        "state_hash",
    ] {
        assert!(
            migration.contains(required),
            "missing migration item {required}"
        );
    }
}

#[test]
fn protocol_package_contains_upgrade_and_notification_guidance() {
    let changelog = read(&format!("{PROTOCOL_ROOT}/CHANGELOG.md"));
    let readme = read(&format!("{PROTOCOL_ROOT}/README.md"));

    assert!(changelog.contains("## 2.1.1"));
    assert!(changelog.contains("single current package"));
    assert!(readme.contains("update the existing files in place"));
    assert!(readme.contains("notify dependent projects"));
    assert!(readme.contains("conformance/adapter_verify.py"));
    assert!(readme.contains("implementation profile"));
    assert!(readme.contains("https://github.com/lgn21st/tui-tetris"));
    assert!(readme.contains("does not constitute full protocol conformance"));
}
