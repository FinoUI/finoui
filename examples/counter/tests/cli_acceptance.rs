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
    assert_eq!(check["runtime_stats"]["component_executions"], 1);
    assert_eq!(check["runtime_stats"]["effect_executions"], 3);
    assert_eq!(check["runtime_stats"]["backend_set_text"], 3);
    assert_eq!(check["runtime_stats"]["nodes_created"], 3);
    assert_eq!(check["runtime_stats"]["nodes_removed"], 0);

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
    assert_eq!(result["runtime_stats"]["component_executions"], 1);
    assert_eq!(result["runtime_stats"]["effect_executions"], 2);
    assert_eq!(result["runtime_stats"]["backend_set_text"], 2);
    assert_eq!(result["runtime_stats"]["nodes_created"], 3);
    assert_eq!(result["runtime_stats"]["nodes_removed"], 0);
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

#[test]
fn conformance_command_prints_a_stable_shared_report() {
    for backend in ["mock", "native-offscreen"] {
        let output = counter()
            .args(["conformance", "--backend", backend, "--format", "json"])
            .output()
            .expect("conformance output");
        assert!(
            output.status.success(),
            "{backend} conformance failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&output.stdout).expect("conformance JSON");
        assert_eq!(report["schema"], 1);
        assert_eq!(report["backend"], backend);
        assert_eq!(report["tree"]["schema"], 1);
        assert_eq!(report["tree"]["roots"][0]["tag"], "button");
        assert_eq!(report["tree"]["roots"][0]["children"][2]["text"], "102");
        assert_eq!(report["event"]["text_after"], "Count: !2");
        assert_eq!(report["batch"]["set_text_delta"], 1);
        assert_eq!(report["batch"]["nodes_created_delta"], 0);
        assert_eq!(report["batch"]["nodes_removed_delta"], 0);
        assert_eq!(report["unmount"]["selector_matches"], 0);
        assert_eq!(report["unmount"]["nodes_removed_delta"], 1);
        assert_eq!(report["final_stats"]["component_executions"], 1);
        assert_eq!(report["final_stats"]["effect_executions"], 4);
        assert_eq!(report["final_stats"]["backend_set_text"], 4);
        assert_eq!(report["final_stats"]["nodes_created"], 4);
        assert_eq!(report["final_stats"]["nodes_removed"], 1);
        assert_eq!(report["operations"]["set_text"], 4);
        assert_eq!(report["operations"]["insert"], 4);
        assert_eq!(report["operations"]["remove"], 1);
    }
}
