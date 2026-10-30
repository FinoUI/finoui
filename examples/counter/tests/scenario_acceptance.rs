use std::{fs, path::Path};

use fino::{
    headless::SnapshotKind,
    inspect::inspect_artifact,
    scenario::{Scenario, Status, Step},
};
use fino_counter::runner::run_mock_scenario;
use serde_json::Value;

fn counter_scenario() -> Scenario {
    serde_json::from_str(include_str!("../scenarios/counter.json")).expect("valid counter scenario")
}

#[test]
fn mock_scenario_writes_deterministic_snapshots_and_trace() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let first_dir = temporary.path().join("first");
    let second_dir = temporary.path().join("second");
    let scenario = counter_scenario();

    let first = run_mock_scenario(&scenario, &first_dir).expect("first scenario run");
    let second = run_mock_scenario(&scenario, &second_dir).expect("second scenario run");

    assert_eq!(first.status, Status::Passed);
    assert_eq!(first.run_id, second.run_id);
    assert_eq!(first.run_dir, first_dir.to_string_lossy());
    assert_eq!(first.counters.component_executions, 1);
    assert_eq!(first.counters.effect_executions, 2);
    assert_eq!(first.counters.frame, 1);
    assert!(first.steps.iter().all(|step| step.status == Status::Passed));
    assert!(first.errors.is_empty());

    for artifact in ["dom.json", "a11y.json", "reactive.json", "trace.json"] {
        let first_json = fs::read_to_string(first_dir.join(artifact)).expect("first artifact");
        let second_json = fs::read_to_string(second_dir.join(artifact)).expect("second artifact");
        assert_eq!(first_json, second_json, "deterministic {artifact}");
    }

    let dom = inspect_artifact(&first_dir, SnapshotKind::Dom).expect("DOM inspection");
    let dom_text = serde_json::to_string(&dom).expect("DOM JSON");
    assert_eq!(dom["nodes"][1]["text"], "Count: ");
    assert_eq!(dom["nodes"][2]["text"], "1");
    assert!(dom_text.contains("aria-label"));
    assert!(!dom_text.contains("0x"));

    let a11y = inspect_artifact(&first_dir, SnapshotKind::A11y).expect("a11y inspection");
    assert_eq!(a11y["nodes"][0]["role"], "button");
    assert_eq!(a11y["nodes"][0]["name"], "Increment counter");
    assert_eq!(a11y["nodes"][0]["text"], "Count: 1");

    let reactive =
        inspect_artifact(&first_dir, SnapshotKind::Reactive).expect("reactive inspection");
    assert_eq!(reactive["component_executions"], 1);
    assert_eq!(reactive["effects"][0]["executions"], 2);
    assert_eq!(reactive["effects"][0]["dependencies"][0], "signal-1");

    let trace = inspect_artifact(&first_dir, SnapshotKind::Trace).expect("trace inspection");
    let events = trace["events"].as_array().expect("trace events");
    assert!(events.iter().any(|event| {
        event["kind"] == "backend"
            && event["details"]["operation"] == "set_text"
            && event["details"]["details"]["set_text"]["value"] == "1"
    }));
    assert!(events.iter().any(|event| event["kind"] == "capture"));
    assert!(!trace.to_string().contains("timestamp"));

    let saved_result: Value = serde_json::from_str(
        &fs::read_to_string(first_dir.join("result.json")).expect("saved result"),
    )
    .expect("result JSON");
    assert_eq!(saved_result["run_id"], first.run_id);
    assert_eq!(saved_result["snapshots"]["dom"], "dom.json");
}

#[test]
fn unsupported_snapshot_is_an_explicit_structured_failure() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let run_dir = temporary.path().join("unsupported");
    let mut scenario = counter_scenario();
    scenario.steps[4] = Step::Capture(vec![SnapshotKind::Layout]);

    let result = run_mock_scenario(&scenario, &run_dir).expect("completed failed run");

    assert_eq!(result.status, Status::Failed);
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.errors[0].code.as_ref(), "UI_SNAPSHOT_UNSUPPORTED");
    assert_eq!(result.errors[0].step, Some(4));
    assert_eq!(result.errors[0].capability.as_deref(), Some("layout"));
    assert!(!run_dir.join("layout.json").exists());
    assert!(run_dir.join("dom.json").exists());
    assert!(run_dir.join("trace.json").exists());
}

#[test]
fn failed_text_assertion_preserves_structured_artifacts() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let run_dir = temporary.path().join("assertion");
    let mut scenario = counter_scenario();
    let Step::AssertText(assertion) = &mut scenario.steps[0] else {
        panic!("first step must be an assertion");
    };
    assertion.value = "Count: wrong".to_owned();

    let result = run_mock_scenario(&scenario, &run_dir).expect("completed failed run");

    assert_eq!(result.status, Status::Failed);
    assert_eq!(result.steps.len(), 1);
    assert_eq!(result.errors[0].code.as_ref(), "UI_ASSERT_TEXT");
    assert_eq!(result.errors[0].step, Some(0));
    assert!(run_dir.join("result.json").exists());
    assert!(run_dir.join("reactive.json").exists());
}

#[test]
fn mount_identifier_is_metadata_not_a_core_registry_lookup() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let run_dir = temporary.path().join("explicit-mount");
    let mut scenario = counter_scenario();
    scenario.mount = "tests/explicit-mount".to_owned();

    let result = run_mock_scenario(&scenario, &run_dir).expect("explicit mount run");

    assert_eq!(result.status, Status::Passed);
    let trace = inspect_artifact(&run_dir, SnapshotKind::Trace).expect("trace inspection");
    assert_eq!(
        trace["events"][0]["details"]["component"],
        "tests/explicit-mount"
    );
}

#[test]
fn scenario_types_generate_a_versioned_json_schema() {
    let schema = schemars::schema_for!(Scenario);
    let schema = serde_json::to_value(schema).expect("schema JSON");
    assert!(schema["properties"]["schema"].is_object());
    assert!(schema["properties"]["steps"].is_object());
    assert!(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("scenarios/counter.json")
            .exists()
    );
}
