use std::{fs, process::Command};

use serde_json::{Value, json};

fn fino() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fino"))
}

#[test]
fn generic_check_reports_tool_prerequisites() {
    let output = fino()
        .args(["check", "--format", "json"])
        .output()
        .expect("generic check output");

    assert!(
        output.status.success(),
        "generic check failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("check JSON");
    assert_eq!(report["status"], "passed");
    assert_eq!(report["schema"], 1);
    assert!(
        report["checks"]
            .as_array()
            .expect("check list")
            .iter()
            .all(|check| check["status"] == "passed")
    );
    assert!(report.get("counters").is_none());
}

#[test]
fn inspect_reads_only_the_requested_existing_artifact() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let run_dir = temporary.path().join("run");

    for kind in ["dom", "a11y", "reactive", "trace"] {
        fs::create_dir_all(&run_dir).expect("run directory");
        let artifact = json!({ "schema": 1, "kind": kind });
        fs::write(
            run_dir.join(format!("{kind}.json")),
            serde_json::to_vec(&artifact).expect("artifact JSON"),
        )
        .expect("artifact file");

        let output = fino()
            .args(["inspect", kind, "--run"])
            .arg(&run_dir)
            .args(["--format", "json"])
            .output()
            .expect("inspect output");
        assert!(
            output.status.success(),
            "inspect {kind} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let inspected: Value = serde_json::from_slice(&output.stdout).expect("inspect JSON");
        assert_eq!(inspected, artifact);
    }
}

#[test]
fn human_inspect_prints_a_summary_instead_of_raw_json() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let run_dir = temporary.path().join("run");
    fs::create_dir_all(&run_dir).expect("run directory");
    fs::write(
        run_dir.join("dom.json"),
        serde_json::to_vec(&json!({
            "schema": 1,
            "roots": ["root"],
            "nodes": [{"text": "do-not-print-the-payload"}]
        }))
        .expect("artifact JSON"),
    )
    .expect("DOM artifact");

    let output = fino()
        .args(["inspect", "dom", "--run"])
        .arg(&run_dir)
        .output()
        .expect("human inspect output");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("human output UTF-8");
    assert!(stdout.contains("dom"));
    assert!(stdout.contains("nodes: 1"));
    assert!(!stdout.contains("do-not-print-the-payload"));
    assert!(!stdout.contains("\"schema\": 1"));
}

#[test]
fn generic_cli_does_not_expose_a_scenario_runner() {
    let output = fino()
        .arg("scenario")
        .output()
        .expect("unknown command output");
    assert!(!output.status.success());
}
