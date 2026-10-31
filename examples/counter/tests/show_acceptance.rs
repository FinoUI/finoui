#![cfg(not(target_arch = "wasm32"))]

use std::{path::PathBuf, process::Command};

use fino::{headless::SnapshotKind, inspect::inspect_artifact, scenario::Status};
use fino_counter::runner::{
    run_mock_show_conformance, run_mock_show_scenario, run_native_offscreen_show_conformance,
    run_native_offscreen_show_scenario,
};
use serde_json::Value;

fn show_scenario() -> fino::scenario::Scenario {
    serde_json::from_str(include_str!("../scenarios/show.json")).expect("valid show scenario")
}

#[test]
fn show_reports_shared_structure_events_and_counters() {
    let mock = run_mock_show_conformance().expect("mock show conformance");
    assert_eq!(mock.backend, "mock");
    assert!(mock.validate().is_empty(), "{:?}", mock.validate());

    let native = run_native_offscreen_show_conformance().expect("native show conformance");
    assert_eq!(native.backend, "native-offscreen");
    assert!(native.validate().is_empty(), "{:?}", native.validate());

    assert_eq!(mock.initial_tree, native.initial_tree);
    assert_eq!(mock.visible_tree, native.visible_tree);
    assert_eq!(mock.hidden_tree, native.hidden_tree);
    assert_eq!(mock.restored_tree, native.restored_tree);
    assert_eq!(mock.final_tree, native.final_tree);
    assert_eq!(mock.clicked_text, native.clicked_text);
    assert_eq!(mock.operations, native.operations);
    assert_eq!(mock.static_ids_stable, native.static_ids_stable);
    assert_eq!(mock.stale_write_is_noop, native.stale_write_is_noop);
    assert_eq!(
        mock.final_stats.component_executions,
        native.final_stats.component_executions
    );
    assert_eq!(
        mock.final_stats.effect_executions,
        native.final_stats.effect_executions
    );
    assert_eq!(
        mock.final_stats.nodes_created,
        native.final_stats.nodes_created
    );
    assert_eq!(
        mock.final_stats.nodes_removed,
        native.final_stats.nodes_removed
    );
}

#[test]
fn show_scenario_runs_on_mock_and_native_offscreen() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let scenario = show_scenario();

    let mock_dir = temporary.path().join("show-mock");
    let mock = run_mock_show_scenario(&scenario, &mock_dir).expect("mock show scenario");
    assert_eq!(mock.status, Status::Passed);
    assert_eq!(mock.backend, "mock");
    assert_eq!(mock.runtime_stats.component_executions, 1);
    assert!(mock.errors.is_empty());
    let dom = inspect_artifact(&mock_dir, SnapshotKind::Dom).expect("mock DOM");
    assert_eq!(
        dom["nodes"]
            .as_array()
            .expect("mock DOM nodes")
            .iter()
            .filter(|node| node["kind"] == "anchor")
            .count(),
        2,
        "the scenario leaves exactly one anchor pair"
    );
    let mock_a11y = inspect_artifact(&mock_dir, SnapshotKind::A11y).expect("mock a11y");
    assert!(
        mock_a11y["nodes"]
            .as_array()
            .expect("mock a11y nodes")
            .iter()
            .any(|node| node["name"] == "Show host" && node["text"] == "HostToggleTail")
    );

    let native_dir = temporary.path().join("show-native");
    let native =
        run_native_offscreen_show_scenario(&scenario, &native_dir).expect("native show scenario");
    assert_eq!(native.status, Status::Passed);
    assert_eq!(native.backend, "native-offscreen");
    assert_eq!(native.runtime_stats.component_executions, 1);
    assert!(native.errors.is_empty());
    let native_a11y = inspect_artifact(&native_dir, SnapshotKind::A11y).expect("native a11y");
    assert!(
        native_a11y["nodes"]
            .as_array()
            .expect("native a11y nodes")
            .iter()
            .any(|node| node["name"] == "Show host" && node["text"] == "HostToggleTail")
    );
}

#[test]
fn show_scenario_capture_excludes_removed_action() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let run_dir = temporary.path().join("show-capture");
    let result = run_mock_show_scenario(&show_scenario(), &run_dir).expect("show capture");
    assert_eq!(result.status, Status::Passed);

    let dom = inspect_artifact(&run_dir, SnapshotKind::Dom).expect("final DOM");
    let host = dom["nodes"]
        .as_array()
        .expect("DOM nodes")
        .iter()
        .find(|node| node["attributes"]["aria-label"] == "Show host")
        .expect("Show host");
    let children: Vec<_> = host["children"].as_array().expect("children").clone();
    assert!(!children.is_empty());
    let a11y = inspect_artifact(&run_dir, SnapshotKind::A11y).expect("a11y");
    let names: Vec<_> = a11y["nodes"]
        .as_array()
        .expect("a11y nodes")
        .iter()
        .filter_map(|node| node["name"].as_str())
        .collect();
    assert!(names.contains(&"Show host"));
    assert!(names.contains(&"Show toggle"));
    assert!(!names.contains(&"Show action"));

    let reactive = inspect_artifact(&run_dir, SnapshotKind::Reactive).expect("reactive");
    let value: Value = reactive;
    assert_eq!(value["component_executions"], 1);
}

#[test]
fn show_scenario_cli_uses_the_explicit_mount() {
    let scenario = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/show.json");
    for backend in ["mock", "native-offscreen"] {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let run_dir = temporary.path().join(format!("show-cli-{backend}"));
        let output = Command::new(env!("CARGO_BIN_EXE_fino_counter"))
            .arg("scenario")
            .arg("run")
            .arg(&scenario)
            .arg("--backend")
            .arg(backend)
            .arg("--mount")
            .arg("show")
            .arg("--out")
            .arg(&run_dir)
            .args(["--format", "json"])
            .output()
            .expect("show CLI output");
        assert!(
            output.status.success(),
            "{backend} show CLI failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value = serde_json::from_slice(&output.stdout).expect("show CLI JSON");
        assert_eq!(result["status"], "passed");
        assert_eq!(result["backend"], backend);
        assert_eq!(result["counters"]["component_executions"], 1);
        assert_eq!(result["steps"].as_array().map(Vec::len), Some(10));
    }
}
