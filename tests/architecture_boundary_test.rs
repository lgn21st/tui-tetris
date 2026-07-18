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
fn composition_root_has_one_terminal_lifecycle_boundary() {
    let main = fs::read_to_string("src/main.rs").unwrap();
    assert_eq!(main.matches("term.enter()?").count(), 1);
    assert_eq!(main.matches("term.exit()").count(), 1);
    assert!(main.contains("with_terminal("));
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
fn workspace_uses_edition_2024_with_matching_dependency_resolver() {
    let root_manifest = fs::read_to_string("Cargo.toml").unwrap();
    assert!(
        root_manifest.contains("resolver = \"3\""),
        "Edition 2024 workspace must use Cargo resolver 3"
    );

    let metadata = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .expect("cargo metadata");
    assert!(metadata.status.success());
    let value: serde_json::Value = serde_json::from_slice(&metadata.stdout).unwrap();
    let packages = value["packages"].as_array().unwrap();
    assert!(!packages.is_empty());
    for package in packages {
        assert_eq!(
            package["edition"].as_str(),
            Some("2024"),
            "package {} is not on Edition 2024",
            package["name"].as_str().unwrap_or("<unknown>")
        );
    }
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
fn workspace_members_do_not_reexport_dependency_layers() {
    for manifest_root in [
        "crates/tetris-session/src",
        "crates/tetris-adapter-protocol/src",
        "crates/tetris-adapter/src",
        "crates/tetris-terminal/src",
    ] {
        for path in rust_sources(Path::new(manifest_root)) {
            let source = fs::read_to_string(&path).unwrap();
            for forbidden in [
                "pub use tetris_core::{core, types};",
                "pub use tetris_session::engine;",
            ] {
                assert!(
                    !source.lines().any(|line| line.trim() == forbidden),
                    "{} reexports dependency layer {forbidden}",
                    path.display()
                );
            }
        }
    }

    let app = fs::read_to_string("src/lib.rs").unwrap();
    for forbidden in [
        "pub use tetris_adapter::adapter;",
        "pub use tetris_core::{core, types};",
        "pub use tetris_session::engine;",
        "pub use tetris_terminal::{input, term};",
    ] {
        assert!(!app.lines().any(|line| line.trim() == forbidden));
    }
}

#[test]
fn app_manifest_keeps_test_only_and_unused_dependencies_out_of_production() {
    let manifest = fs::read_to_string("Cargo.toml").unwrap();
    let dependencies = manifest
        .split("[dependencies]")
        .nth(1)
        .and_then(|tail| tail.split("[dev-dependencies]").next())
        .expect("root dependency section");

    assert!(!dependencies.lines().any(|line| line.starts_with("tokio ")));
    assert!(!dependencies.lines().any(|line| line.starts_with("serde ")));
    assert!(!manifest.lines().any(|line| line.starts_with("tokio-test ")));
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
