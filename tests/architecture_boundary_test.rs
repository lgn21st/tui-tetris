use std::fs;
use std::path::Path;
use std::process::Command;

fn rust_sources(root: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(rust_sources(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    files
}

#[test]
fn core_has_no_platform_or_adapter_dependencies() {
    for path in rust_sources(Path::new("crates/tetris-core/src")) {
        let source = fs::read_to_string(&path).unwrap();
        for forbidden in [
            "crossterm",
            "tokio",
            "serde",
            "crate::adapter",
            "crate::input",
            "crate::term",
        ] {
            assert!(
                !source.contains(forbidden),
                "{} contains forbidden dependency {forbidden}",
                path.display()
            );
        }
    }
}

#[test]
fn composition_root_does_not_mutate_game_state_directly() {
    let main = fs::read_to_string("src/main.rs").unwrap();
    assert!(!main.contains("game_state.apply_action"));
    assert!(!main.contains("game_state.tick"));
    assert!(main.contains("step_session("));
}

#[test]
fn production_adapter_has_no_unbounded_outbound_channel() {
    for path in rust_sources(Path::new("crates/tetris-adapter/src")) {
        let source = fs::read_to_string(&path).unwrap();
        assert!(
            !source.contains("unbounded_channel::<OutboundMessage>"),
            "{} creates an unbounded outbound channel",
            path.display()
        );
    }
}

#[test]
fn outbound_bridge_has_no_duplicate_owned_or_typed_variants() {
    let runtime = fs::read_to_string("crates/tetris-adapter/src/adapter/runtime.rs").unwrap();
    for forbidden in [
        "ToClient {",
        "Broadcast {",
        "ToClientObservation {",
        "BroadcastObservation {",
        "ToClientObservationArc {",
        "ToClientError {",
    ] {
        assert!(!runtime.contains(forbidden), "legacy variant {forbidden}");
    }
}

#[test]
fn workspace_exposes_compiler_checked_core_session_and_protocol_packages() {
    let metadata = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .expect("cargo metadata");
    assert!(metadata.status.success());
    let value: serde_json::Value = serde_json::from_slice(&metadata.stdout).unwrap();
    let packages = value["packages"].as_array().unwrap();
    let names = packages
        .iter()
        .filter_map(|package| package["name"].as_str())
        .collect::<Vec<_>>();

    assert!(names.contains(&"tetris-core"));
    assert!(names.contains(&"tetris-session"));
    assert!(names.contains(&"tetris-adapter-protocol"));
    assert!(names.contains(&"tetris-adapter"));
    assert!(names.contains(&"tetris-terminal"));
}

#[test]
fn workspace_crates_own_their_sources_without_cross_tree_path_indirection() {
    for manifest_root in [
        "crates/tetris-core/src",
        "crates/tetris-session/src",
        "crates/tetris-adapter-protocol/src",
        "crates/tetris-adapter/src",
        "crates/tetris-terminal/src",
    ] {
        for path in rust_sources(Path::new(manifest_root)) {
            let source = fs::read_to_string(&path).unwrap();
            assert!(
                !source.contains("../../../src/"),
                "{} borrows source from the app tree",
                path.display()
            );
        }
    }
}

#[test]
fn development_workflow_preserves_dependency_order() {
    let workflow = fs::read_to_string("docs/development-workflow.md").unwrap();
    for stage in ["Rules", "Core", "Session and replay", "Adapter", "Terminal"] {
        assert!(workflow.contains(stage), "missing workflow stage {stage}");
    }
    assert!(workflow.contains("cargo test --workspace"));
}

#[test]
fn product_contract_separates_goals_from_replaceable_policies() {
    let contract = fs::read_to_string("docs/product-contract.md").unwrap();
    for invariant in [
        "Deterministic",
        "Replayable",
        "Resource-bounded",
        "Externally controllable",
        "Causally observable",
    ] {
        assert!(
            contract.contains(invariant),
            "missing invariant {invariant}"
        );
    }
    assert!(contract.contains("Replaceable policies"));
    assert!(contract.contains("16 ms"));
    assert!(contract.contains("protocol version"));
}

#[test]
fn maintained_roadmap_does_not_claim_known_hot_path_allocations() {
    let roadmap = fs::read_to_string("docs/roadmap.md").unwrap();
    assert!(!roadmap.contains("remove remaining per-frame allocations"));
}

#[test]
fn protocol_v3_rust_surface_has_no_v2_last_event_name() {
    let protocol = fs::read_to_string("crates/tetris-adapter-protocol/src/protocol.rs").unwrap();
    assert!(!protocol.contains("pub struct LastEvent"));
}
