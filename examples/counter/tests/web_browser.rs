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
    keyed::KeyedConformanceReport,
    runner::{run_mock_conformance, run_mock_keyed_conformance, run_mock_show_conformance},
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
        "anchor_nodes_reject_text_writes",
        "web_remount_restores_reachability",
        "keyed_for_matches_shared_semantics",
        "keyed_rows_keep_dom_identity_and_insert_before_moves",
        "web_node_identity_is_stable_and_scan_free",
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

    let mut web_keyed: KeyedConformanceReport = serde_json::from_str(
        &fs::read_to_string(run_dir.join("keyed_conformance.json"))
            .expect("keyed conformance file"),
    )
    .expect("keyed conformance JSON");
    let mock_keyed = run_mock_keyed_conformance().expect("mock keyed conformance");
    assert_eq!(web_keyed.backend, "web-headless");
    web_keyed.backend = mock_keyed.backend.clone();
    assert_eq!(web_keyed.initial_tree, mock_keyed.initial_tree);
    assert_eq!(web_keyed.reordered_tree, mock_keyed.reordered_tree);
    assert_eq!(web_keyed.disposed_tree, mock_keyed.disposed_tree);
    assert_eq!(web_keyed.final_tree, mock_keyed.final_tree);
    assert_eq!(web_keyed.initial_text, mock_keyed.initial_text);
    assert_eq!(web_keyed.phases, mock_keyed.phases);
    assert_eq!(web_keyed.local_update, mock_keyed.local_update);
    assert_eq!(web_keyed.operations, mock_keyed.operations);
    assert_eq!(web_keyed.builder_runs, mock_keyed.builder_runs);
    assert_eq!(
        web_keyed.stale_write_is_noop,
        mock_keyed.stale_write_is_noop
    );
    assert_eq!(
        web_keyed.duplicate_key_rejected,
        mock_keyed.duplicate_key_rejected
    );
    assert_eq!(
        web_keyed.duplicate_recovered,
        mock_keyed.duplicate_recovered
    );
    assert_eq!(web_keyed.dispose_idempotent, mock_keyed.dispose_idempotent);
    assert_eq!(web_keyed.static_ids_stable, mock_keyed.static_ids_stable);
    assert_eq!(
        web_keyed.item_identity_stable,
        mock_keyed.item_identity_stable
    );
    assert_eq!(
        web_keyed.probe_storage_bounded,
        mock_keyed.probe_storage_bounded
    );
    assert!(web_keyed.probe_storage_bounded);
    assert_eq!(
        web_keyed.final_stats.component_executions,
        mock_keyed.final_stats.component_executions
    );
    assert_eq!(
        web_keyed.final_stats.effect_executions,
        mock_keyed.final_stats.effect_executions
    );
    assert_eq!(
        web_keyed.final_stats.nodes_created,
        mock_keyed.final_stats.nodes_created
    );
    assert_eq!(
        web_keyed.final_stats.nodes_removed,
        mock_keyed.final_stats.nodes_removed
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

/// Runs the keyed scenario through the same browser pipeline with its explicit mount.
/// 通过同一浏览器流水线及其显式 mount 运行 keyed 场景。
#[test]
#[ignore = "launches headless Chromium; run with cargo test -p fino_counter --test web_browser -- --ignored"]
fn web_headless_runs_the_keyed_scenario() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let run_dir = temporary.path().join("keyed-web");
    let scenario = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/keyed.json");
    let output = run_web_scenario_with_mount(&scenario, &run_dir, Some("keyed"));
    assert!(
        output.status.success(),
        "keyed scenario failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let result: Value = serde_json::from_slice(&output.stdout).expect("keyed scenario JSON");
    assert_eq!(result["status"], "passed");
    assert_eq!(result["backend"], "web-headless");
    assert_eq!(result["counters"]["component_executions"], 1);
    assert_eq!(result["steps"].as_array().map(Vec::len), Some(28));

    let dom = inspect_artifact(&run_dir, SnapshotKind::Dom).expect("keyed DOM");
    assert_eq!(dom["roots"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        dom["nodes"]
            .as_array()
            .expect("keyed DOM nodes")
            .iter()
            .filter(|node| node["kind"] == "anchor")
            .count(),
        6
    );
    let a11y = inspect_artifact(&run_dir, SnapshotKind::A11y).expect("keyed a11y");
    let host = a11y["nodes"]
        .as_array()
        .expect("keyed a11y nodes")
        .iter()
        .find(|node| node["name"] == "Keyed host")
        .expect("keyed host");
    assert_eq!(host["text"], "HostNextD=1E=0Tail");

    let browser: Value = serde_json::from_str(
        &fs::read_to_string(run_dir.join("browser.json")).expect("browser checks"),
    )
    .expect("browser JSON");
    assert_eq!(browser["status"], "passed");
    for expected in [
        "scenario_executes_in_chromium",
        "scenario_dom_matches_live_dom",
        "keyed_rows_keep_dom_identity_and_insert_before_moves",
    ] {
        assert!(
            browser["checks"]
                .as_array()
                .expect("browser checks")
                .iter()
                .any(|check| check["name"] == expected && check["status"] == "passed"),
            "missing passing browser check {expected}"
        );
    }
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
    let keyed_report: Value = serde_json::from_str(
        &fs::read_to_string(run_dir.join("keyed_conformance.json")).expect("keyed report"),
    )
    .expect("keyed report JSON");
    assert_eq!(keyed_report["status"], "passed");
    fs::write(run_dir.join("keyed_conformance.json"), "stale keyed report")
        .expect("stale keyed report");
    fs::write(run_dir.join("show_conformance.json"), "stale show report")
        .expect("stale show report");

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
    for name in [
        "keyed_conformance.json",
        "show_conformance.json",
        "conformance.json",
        "detach_reinsert.json",
        "remount.json",
        "anchor_misuse.json",
    ] {
        assert!(
            !run_dir.join(name).exists(),
            "stale managed report removed: {name}"
        );
    }
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

/// A user-supplied keyed scenario may legitimately end with only the list
/// anchors (clear state); the generic runner must not require a bundled anchor count.
/// 用户 keyed 场景可以合法地只保留列表锚点（清空状态）；通用运行器不得要求
/// bundled 场景的固定锚点数。
#[test]
#[ignore = "launches headless Chromium; run with cargo test -p fino_counter --test web_browser -- --ignored"]
fn web_headless_accepts_keyed_scenario_with_clear_final_state() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let scenario_path = temporary.path().join("keyed-clear.json");
    let run_dir = temporary.path().join("keyed-clear-web");
    let scenario = serde_json::json!({
        "schema": 1,
        "name": "keyed/clear-final",
        "mount": "keyed",
        "environment": {
            "viewport": [800, 600],
            "dpr": 1.0,
            "locale": "zh-CN",
            "color_scheme": "dark",
            "seed": 42,
        },
        "steps": [
            {
                "assert_text": {
                    "role": "button",
                    "name": "Keyed host",
                    "value": "HostNextTail",
                }
            },
            { "advance_frames": 1 },
            { "capture": ["dom", "a11y", "reactive"] },
        ],
    });
    fs::write(
        &scenario_path,
        serde_json::to_vec_pretty(&scenario).expect("scenario JSON"),
    )
    .expect("scenario file");

    let output = run_web_scenario_with_mount(&scenario_path, &run_dir, Some("keyed"));
    assert!(
        output.status.success(),
        "clear keyed scenario failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).expect("scenario JSON");
    assert_eq!(result["status"], "passed");
    assert_eq!(result["backend"], "web-headless");
    assert_eq!(result["steps"].as_array().map(Vec::len), Some(3));

    for name in [
        "result.json",
        "dom.json",
        "a11y.json",
        "reactive.json",
        "trace.json",
        "browser.json",
        "keyed_conformance.json",
    ] {
        assert!(
            run_dir.join(name).is_file(),
            "missing successful artifact {name}"
        );
    }
    let dom = inspect_artifact(&run_dir, SnapshotKind::Dom).expect("clear DOM");
    assert_eq!(dom["roots"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        dom["nodes"]
            .as_array()
            .expect("DOM nodes")
            .iter()
            .filter(|node| node["kind"] == "anchor")
            .count(),
        2,
        "clear state keeps only the list anchor pair"
    );
    let a11y = inspect_artifact(&run_dir, SnapshotKind::A11y).expect("clear a11y");
    let host = a11y["nodes"]
        .as_array()
        .expect("a11y nodes")
        .iter()
        .find(|node| node["name"] == "Keyed host")
        .expect("keyed host");
    assert_eq!(host["text"], "HostNextTail");

    let browser: Value = serde_json::from_str(
        &fs::read_to_string(run_dir.join("browser.json")).expect("browser checks"),
    )
    .expect("browser JSON");
    assert_eq!(browser["status"], "passed");
    for expected in [
        "scenario_executes_in_chromium",
        "scenario_dom_matches_live_dom",
    ] {
        assert!(
            browser["checks"]
                .as_array()
                .expect("browser checks")
                .iter()
                .any(|check| check["name"] == expected && check["status"] == "passed"),
            "missing passing browser check {expected}"
        );
    }
}
