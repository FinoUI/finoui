use std::{fs, process::Command};

use serde_json::{Value, json};

fn fino() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fino"))
}

#[test]
fn generic_check_reports_tool_prerequisites() {
    let output = fino()
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
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

/// Reads each core artifact by path without involving its producer or mount code.
/// 按路径读取各类核心产物，不涉及其生产者或挂载代码。
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

/// Locks Clap parsing to the protocol's underscore spelling for display lists.
/// 固定 Clap 对 display_list 协议下划线名称的解析。
#[test]
fn inspect_display_list_accepts_the_protocol_snapshot_name() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let run_dir = temporary.path().join("run");
    fs::create_dir_all(&run_dir).expect("run directory");
    let artifact = json!({ "schema": 1, "commands": [] });
    fs::write(
        run_dir.join("display_list.json"),
        serde_json::to_vec(&artifact).expect("display list JSON"),
    )
    .expect("display list artifact");

    let output = fino()
        .args(["inspect", "display_list", "--run"])
        .arg(&run_dir)
        .args(["--format", "json"])
        .output()
        .expect("display list inspect output");

    assert!(
        output.status.success(),
        "display_list was rejected: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let inspected: Value = serde_json::from_slice(&output.stdout).expect("inspect JSON");
    assert_eq!(inspected, artifact);
}

/// Rejects empty, syntactically invalid, and structurally incomplete workspace manifests.
/// 拒绝空、语法损坏与结构不完整的 workspace manifest。
#[test]
fn empty_invalid_and_memberless_manifests_fail_generic_check() {
    for manifest in ["", "[workspace\n", "[workspace]\nmembers = []\n"] {
        let temporary = tempfile::tempdir().expect("temporary directory");
        fs::write(temporary.path().join("Cargo.toml"), manifest).expect("manifest fixture");
        let output = fino()
            .current_dir(temporary.path())
            .args(["check", "--format", "json"])
            .output()
            .expect("manifest check output");

        assert!(
            !output.status.success(),
            "manifest unexpectedly passed: {manifest:?}"
        );
        let report: Value = serde_json::from_slice(&output.stdout).expect("check JSON");
        assert_eq!(report["status"], "failed");
        let workspace = report["checks"]
            .as_array()
            .expect("check list")
            .iter()
            .find(|check| check["name"] == "workspace_manifest")
            .expect("workspace check");
        assert_eq!(workspace["status"], "failed");
        assert!(
            workspace["message"]
                .as_str()
                .is_some_and(|message| !message.is_empty())
        );
    }
}

/// Prevents human inspection from leaking full JSON while preserving useful counts.
/// 防止 human inspect 输出完整 JSON，同时保留有用计数。
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
