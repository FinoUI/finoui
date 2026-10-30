//! Explicit browser acceptance for the wasm counter and the real DOM backend.
//! wasm 计数器与真实 DOM 后端的显式浏览器验收。

use std::{fs, path::PathBuf, process::Command};

use fino::{headless::SnapshotKind, inspect::inspect_artifact};
use serde_json::Value;

fn counter() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fino_counter"))
}

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/counter.json")
}

/// Runs the unchanged scenario in headless Chromium and checks the browser evidence.
/// 在无头 Chromium 中运行未修改的场景，并检查浏览器证据。
#[test]
#[ignore = "launches headless Chromium; run with cargo test -p fino_counter --test web_browser -- --ignored"]
fn web_headless_runs_the_same_scenario_with_real_dom_probes() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let run_dir = temporary.path().join("counter-web");
    let output = counter()
        .arg("scenario")
        .arg("run")
        .arg(scenario_path())
        .arg("--backend")
        .arg("web-headless")
        .arg("--out")
        .arg(&run_dir)
        .args(["--format", "json"])
        .output()
        .expect("browser scenario output");
    assert!(
        output.status.success(),
        "web scenario failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let result: Value = serde_json::from_slice(&output.stdout).expect("scenario JSON");
    assert_eq!(result["status"], "passed");
    assert_eq!(result["backend"], "web-headless");
    assert_eq!(result["counters"]["component_executions"], 1);
    assert_eq!(result["counters"]["effect_executions"], 2);
    assert_eq!(result["steps"].as_array().map(Vec::len), Some(5));

    let dom = inspect_artifact(&run_dir, SnapshotKind::Dom).expect("DOM inspection");
    let nodes = dom["nodes"].as_array().expect("dom nodes");
    assert!(nodes.iter().any(|node| node["text"] == "Count: "));
    assert!(nodes.iter().any(|node| node["text"] == "1"));
    assert!(
        nodes.iter().any(|node| node["tag"] == "button"
            && node["attributes"]["aria-label"] == "Increment counter")
    );

    let a11y = inspect_artifact(&run_dir, SnapshotKind::A11y).expect("a11y inspection");
    assert_eq!(a11y["nodes"][0]["name"], "Increment counter");
    assert_eq!(a11y["nodes"][0]["text"], "Count: 1");

    let browser: Value = serde_json::from_str(
        &fs::read_to_string(run_dir.join("browser.json")).expect("browser checks"),
    )
    .expect("browser JSON");
    assert_eq!(browser["status"], "passed");
    let checks = browser["checks"].as_array().expect("browser checks");
    for expected in [
        "real_button_and_initial_text",
        "first_real_click_updates_node_value_once",
        "second_real_click_updates_node_value_once",
        "focus_and_keyboard_activation",
        "batched_increments_commit_only_final_text",
        "tab_order_reaches_button",
        "scenario_executes_in_chromium",
        "scenario_dom_matches_live_dom",
    ] {
        assert!(
            checks
                .iter()
                .any(|check| check["name"] == expected && check["status"] == "passed"),
            "missing passing browser check {expected}"
        );
    }
}
