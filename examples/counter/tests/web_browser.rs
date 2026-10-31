//! Explicit browser acceptance for the wasm counter and the real DOM backend.
//! wasm 计数器与真实 DOM 后端的显式浏览器验收。

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use fino::{
    backend_mock::MockBackend,
    headless::{Environment, HeadlessHost, SnapshotKind},
    inspect::inspect_artifact,
};
use fino_counter::{
    conformance::{ConformanceReport, DetachReinsertResult, run_detach_reinsert},
    runner::{run_mock_conformance, run_mock_show_conformance},
    show::ShowConformanceReport,
};
use serde_json::Value;

/// Builds the example binary command used by browser acceptance tests.
/// 构造浏览器验收测试使用的示例二进制命令。
fn counter() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fino_counter"))
}

/// Returns the unchanged counter scenario shipped with the example.
/// 返回示例中随附的未修改计数器场景。
fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/counter.json")
}

/// Runs one scenario through the web-headless backend into a chosen directory.
/// 通过 web-headless 后端把单个场景运行到指定目录。
fn run_web_scenario(scenario: &Path, run_dir: &Path) -> Output {
    run_web_scenario_with_mount(scenario, run_dir, None)
}

fn run_web_scenario_with_mount(scenario: &Path, run_dir: &Path, mount: Option<&str>) -> Output {
    let mut command = counter();
    command
        .arg("scenario")
        .arg("run")
        .arg(scenario)
        .arg("--backend")
        .arg("web-headless")
        .arg("--out")
        .arg(run_dir)
        .args(["--format", "json"]);
    if let Some(mount) = mount {
        command.arg("--mount").arg(mount);
    }
    command.output().expect("browser scenario output")
}

/// Runs the unchanged scenario in headless Chromium and checks the browser evidence.
/// 在无头 Chromium 中运行未修改的场景，并检查浏览器证据。
#[test]
#[ignore = "launches headless Chromium; run with cargo test -p fino_counter --test web_browser -- --ignored"]
fn web_headless_runs_the_same_scenario_with_real_dom_probes() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let run_dir = temporary.path().join("counter-web");
    let output = run_web_scenario(&scenario_path(), &run_dir);
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
        "conformance_reports_shared_runtime_stats",
        "detach_reinsert_matches_shared_semantics",
        "web_remount_restores_reachability",
    ] {
        assert!(
            checks
                .iter()
                .any(|check| check["name"] == expected && check["status"] == "passed"),
            "missing passing browser check {expected}"
        );
    }

    let mut web: ConformanceReport = serde_json::from_str(
        &fs::read_to_string(run_dir.join("conformance.json")).expect("conformance file"),
    )
    .expect("conformance JSON");
    let mock = run_mock_conformance().expect("mock conformance");
    assert_eq!(web.backend, "web-headless");
    web.backend = mock.backend.clone();
    assert_eq!(
        web, mock,
        "Web and Mock must agree on trees, events, operations, and shared counters"
    );

    let mut web_detach: DetachReinsertResult = serde_json::from_str(
        &fs::read_to_string(run_dir.join("detach_reinsert.json")).expect("detach file"),
    )
    .expect("detach JSON");
    let mock_host = HeadlessHost::new(MockBackend::new(), Environment::default());
    let mock_detach = run_detach_reinsert(mock_host).expect("mock detach probe");
    web_detach.backend = mock_detach.backend.clone();
    assert_eq!(
        web_detach, mock_detach,
        "Web and Mock must retain and reinsert the same child node"
    );

    let remount: Value = serde_json::from_str(
        &fs::read_to_string(run_dir.join("remount.json")).expect("remount file"),
    )
    .expect("remount JSON");
    assert_eq!(remount["removed_matches"], 0);
    assert_eq!(remount["remounted_matches"], 1);
    assert_eq!(remount["remounted_tree_roots"], 1);

    let mut web_show: ShowConformanceReport = serde_json::from_str(
        &fs::read_to_string(run_dir.join("show_conformance.json")).expect("show conformance file"),
    )
    .expect("show conformance JSON");
    let mock_show = run_mock_show_conformance().expect("mock show conformance");
    assert_eq!(web_show.backend, "web-headless");
    web_show.backend = mock_show.backend.clone();
    assert_eq!(web_show.initial_tree, mock_show.initial_tree);
    assert_eq!(web_show.visible_tree, mock_show.visible_tree);
    assert_eq!(web_show.hidden_tree, mock_show.hidden_tree);
    assert_eq!(web_show.restored_tree, mock_show.restored_tree);
    assert_eq!(web_show.final_tree, mock_show.final_tree);
    assert_eq!(web_show.clicked_text, mock_show.clicked_text);
    assert_eq!(web_show.operations, mock_show.operations);
    assert_eq!(web_show.static_ids_stable, mock_show.static_ids_stable);
    assert_eq!(web_show.stale_write_is_noop, mock_show.stale_write_is_noop);
    assert_eq!(
        web_show.final_stats.component_executions,
        mock_show.final_stats.component_executions
    );
    assert_eq!(
        web_show.final_stats.effect_executions,
        mock_show.final_stats.effect_executions
    );
    assert_eq!(
        web_show.final_stats.nodes_created,
        mock_show.final_stats.nodes_created
    );
    assert_eq!(
        web_show.final_stats.nodes_removed,
        mock_show.final_stats.nodes_removed
    );
}

/// Runs the Show scenario through the same browser pipeline with its explicit mount.
/// 通过同一浏览器流水线及其显式 mount 运行 Show 场景。
#[test]
#[ignore = "launches headless Chromium; run with cargo test -p fino_counter --test web_browser -- --ignored"]
fn web_headless_runs_the_show_scenario() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let run_dir = temporary.path().join("show-web");
    let scenario = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/show.json");
    let output = run_web_scenario_with_mount(&scenario, &run_dir, Some("show"));
    assert!(
        output.status.success(),
        "show scenario failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let result: Value = serde_json::from_slice(&output.stdout).expect("show scenario JSON");
    assert_eq!(result["status"], "passed");
    assert_eq!(result["backend"], "web-headless");
    assert_eq!(result["counters"]["component_executions"], 1);
    assert_eq!(result["steps"].as_array().map(Vec::len), Some(10));

    let dom = inspect_artifact(&run_dir, SnapshotKind::Dom).expect("Show DOM");
    assert_eq!(dom["roots"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        dom["nodes"]
            .as_array()
            .expect("Show DOM nodes")
            .iter()
            .filter(|node| node["kind"] == "anchor")
            .count(),
        2
    );
    let a11y = inspect_artifact(&run_dir, SnapshotKind::A11y).expect("Show a11y");
    let host = a11y["nodes"]
        .as_array()
        .expect("Show a11y nodes")
        .iter()
        .find(|node| node["name"] == "Show host")
        .expect("Show host");
    assert_eq!(host["text"], "HostToggleTail");
    assert!(
        !a11y["nodes"]
            .as_array()
            .expect("Show a11y nodes")
            .iter()
            .any(|node| node["name"] == "Show action"),
        "the removed Show action must be unreachable"
    );

    let browser: Value = serde_json::from_str(
        &fs::read_to_string(run_dir.join("browser.json")).expect("browser checks"),
    )
    .expect("browser JSON");
    assert_eq!(browser["status"], "passed");
    assert!(
        browser["checks"]
            .as_array()
            .expect("browser checks")
            .iter()
            .any(|check| check["name"] == "scenario_executes_in_chromium"
                && check["status"] == "passed")
    );
    assert!(
        browser["checks"]
            .as_array()
            .expect("browser checks")
            .iter()
            .any(|check| check["name"] == "scenario_dom_matches_live_dom"
                && check["status"] == "passed")
    );
}

/// Proves a failed rerun removes stale captures without deleting unrelated files.
/// 验证失败重跑会移除旧捕获，同时不删除无关文件。
#[test]
#[ignore = "launches headless Chromium; run with cargo test -p fino_counter --test web_browser -- --ignored"]
fn failed_web_rerun_clears_stale_captures_and_keeps_unrelated_files() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let run_dir = temporary.path().join("counter-web-rerun");
    let first = run_web_scenario(&scenario_path(), &run_dir);
    assert!(
        first.status.success(),
        "first web scenario failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(run_dir.join("captures/step-4/dom.json").is_file());

    fs::write(run_dir.join("keep.txt"), "unrelated").expect("unrelated file");
    fs::create_dir_all(run_dir.join("notes")).expect("notes directory");
    fs::write(run_dir.join("notes/dom.json"), "nested unrelated").expect("nested file");

    let mut failing: Value =
        serde_json::from_str(&fs::read_to_string(scenario_path()).expect("counter scenario"))
            .expect("scenario JSON");
    failing["steps"][0]["assert_text"]["value"] = Value::String("Count: wrong".to_owned());
    let failing_path = temporary.path().join("counter-failing.json");
    fs::write(
        &failing_path,
        serde_json::to_vec_pretty(&failing).expect("failing scenario JSON"),
    )
    .expect("failing scenario file");

    let second = run_web_scenario(&failing_path, &run_dir);
    assert_eq!(second.status.code(), Some(1), "failed scenario exit code");
    let result: Value = serde_json::from_slice(&second.stdout).expect("failed scenario JSON");
    assert_eq!(result["status"], "failed");
    assert_eq!(result["errors"][0]["code"], "UI_ASSERT_TEXT");

    assert!(!run_dir.join("captures").exists(), "stale captures removed");
    assert_eq!(
        fs::read_to_string(run_dir.join("keep.txt")).expect("unrelated file kept"),
        "unrelated"
    );
    assert_eq!(
        fs::read_to_string(run_dir.join("notes/dom.json")).expect("nested file kept"),
        "nested unrelated"
    );

    let saved: Value = serde_json::from_str(
        &fs::read_to_string(run_dir.join("result.json")).expect("failed result"),
    )
    .expect("failed result JSON");
    assert_eq!(saved["status"], "failed");
    let dom = inspect_artifact(&run_dir, SnapshotKind::Dom).expect("failed DOM inspection");
    let nodes = dom["nodes"].as_array().expect("dom nodes");
    assert!(nodes.iter().any(|node| node["text"] == "0"));
    assert!(!nodes.iter().any(|node| node["text"] == "1"));
}
