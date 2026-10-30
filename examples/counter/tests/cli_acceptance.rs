use std::{path::PathBuf, process::Command};

use fino::{headless::SnapshotKind, inspect::inspect_artifact};
use serde_json::Value;

fn counter() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fino_counter"))
}

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/counter.json")
}

#[test]
fn cargo_check_and_scenario_commands_form_a_json_loop() {
    let check = counter()
        .args(["check", "--format", "json"])
        .output()
        .expect("counter check output");
    assert!(
        check.status.success(),
        "counter check failed: {}",
        String::from_utf8_lossy(&check.stderr)
    );
    let check: Value = serde_json::from_slice(&check.stdout).expect("check JSON");
    assert_eq!(check["status"], "passed");
    assert_eq!(check["counters"]["component_executions"], 1);
    assert_eq!(check["counters"]["effect_executions"], 3);

    let temporary = tempfile::tempdir().expect("temporary directory");
    let run_dir = temporary.path().join("counter-mock");
    let run = counter()
        .arg("scenario")
        .arg("run")
        .arg(scenario_path())
        .arg("--backend")
        .arg("mock")
        .arg("--out")
        .arg(&run_dir)
        .args(["--format", "json"])
        .output()
        .expect("counter scenario output");
    assert!(
        run.status.success(),
        "counter scenario failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let result: Value = serde_json::from_slice(&run.stdout).expect("scenario JSON");
    assert_eq!(result["status"], "passed");
    assert_eq!(result["backend"], "mock");
    assert!(result["run_id"].as_str().is_some_and(|id| !id.is_empty()));

    let dom = inspect_artifact(&run_dir, SnapshotKind::Dom).expect("DOM inspection");
    assert_eq!(dom["schema"], 1);
    assert!(
        dom["nodes"]
            .as_array()
            .expect("dom nodes")
            .iter()
            .any(|node| node["text"] == "1")
    );

    let native_dir = temporary.path().join("counter-native");
    let native = counter()
        .arg("scenario")
        .arg("run")
        .arg(scenario_path())
        .arg("--backend")
        .arg("native-offscreen")
        .arg("--out")
        .arg(&native_dir)
        .args(["--format", "json"])
        .output()
        .expect("native counter scenario output");
    assert!(
        native.status.success(),
        "native counter scenario failed: {}",
        String::from_utf8_lossy(&native.stderr)
    );
    let native: Value = serde_json::from_slice(&native.stdout).expect("native scenario JSON");
    assert_eq!(native["status"], "passed");
    assert_eq!(native["backend"], "native-offscreen");
    assert_eq!(native["counters"]["component_executions"], 1);
    let native_dom = inspect_artifact(&native_dir, SnapshotKind::Dom).expect("native DOM");
    assert!(
        native_dom["nodes"]
            .as_array()
            .expect("native dom nodes")
            .iter()
            .any(|node| node["text"] == "1")
    );
}
