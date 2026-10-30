#![cfg(not(target_arch = "wasm32"))]

use std::{cell::RefCell, rc::Rc};

use fino::{
    headless::{Environment, HeadlessBackend, HeadlessHost, SnapshotKind},
    inspect::inspect_artifact,
    native::Size,
    scenario::{Scenario, Status, Step},
    text::TextEngine,
};
use fino_counter::{mount_counter, runner::run_native_offscreen_scenario};
use serde_json::Value;

fn counter_scenario() -> Scenario {
    serde_json::from_str(include_str!("../scenarios/counter.json")).expect("valid counter scenario")
}

/// Runs the unchanged shared counter scenario on the native offscreen backend.
/// 在原生离屏后端运行未改动的共享计数器场景。
#[test]
fn native_offscreen_runs_the_same_counter_scenario() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let run_dir = temporary.path().join("native");
    let scenario = counter_scenario();

    let result = run_native_offscreen_scenario(&scenario, &run_dir).expect("native scenario run");

    assert_eq!(result.status, Status::Passed);
    assert_eq!(result.backend, "native-offscreen");
    assert_eq!(result.counters.component_executions, 1);
    assert_eq!(result.counters.effect_executions, 2);
    assert_eq!(result.counters.frame, 1);
    assert!(result.errors.is_empty());

    let dom = inspect_artifact(&run_dir, SnapshotKind::Dom).expect("DOM inspection");
    assert!(
        dom["nodes"]
            .as_array()
            .expect("DOM nodes")
            .iter()
            .any(|node| node["text"] == "Count: ")
    );
    assert!(
        dom["nodes"]
            .as_array()
            .expect("DOM nodes")
            .iter()
            .any(|node| node["text"] == "1")
    );
    let a11y = inspect_artifact(&run_dir, SnapshotKind::A11y).expect("a11y inspection");
    let button = a11y["nodes"]
        .as_array()
        .expect("a11y nodes")
        .iter()
        .find(|node| node["role"] == "button")
        .expect("counter button");
    assert_eq!(button["text"], "Count: 1");
    let reactive = inspect_artifact(&run_dir, SnapshotKind::Reactive).expect("reactive inspection");
    assert_eq!(reactive["component_executions"], 1);
}

/// Captures real native layout and display-list artifacts through the scenario protocol.
/// 通过场景协议捕获真实原生布局与显示列表产物。
#[test]
fn native_offscreen_captures_layout_and_display_list() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let run_dir = temporary.path().join("native-artifacts");
    let mut scenario = counter_scenario();
    scenario.steps.push(Step::Capture(vec![
        SnapshotKind::Layout,
        SnapshotKind::DisplayList,
    ]));

    let result = run_native_offscreen_scenario(&scenario, &run_dir).expect("native capture run");

    assert_eq!(result.status, Status::Passed);
    let capture = result.captures.last().expect("layout capture");
    assert_eq!(capture.artifacts["layout"], "captures/step-5/layout.json");
    assert_eq!(
        capture.artifacts["display_list"],
        "captures/step-5/display_list.json"
    );
    let layout = inspect_artifact(&run_dir.join("captures/step-5"), SnapshotKind::Layout)
        .expect("layout artifact");
    assert_eq!(layout["schema"], 1);
    assert_eq!(layout["window"], 1);
    assert_eq!(layout["boxes"].as_array().map(Vec::len), Some(3));
    assert_eq!(layout["boxes"][0]["tag"], "button");
    assert!(layout["text"]["shape_calls"].as_u64().unwrap_or(0) >= 2);

    let display = inspect_artifact(&run_dir.join("captures/step-5"), SnapshotKind::DisplayList)
        .expect("display list artifact");
    assert_eq!(display["schema"], 1);
    assert_eq!(display["window"], 1);
    assert!(
        display["commands"]
            .as_array()
            .expect("commands")
            .iter()
            .any(|command| command["kind"] == "text" && command["text"] == "1")
    );
}

/// Keeps node identity and component execution stable across repeated clicks.
/// 在连续点击中保持节点身份与组件执行计数稳定。
#[test]
fn native_clicks_update_one_text_node_without_remounting() {
    let engine = Rc::new(RefCell::new(TextEngine::new()));
    let backend = fino::native::NativeDom::new_offscreen(
        fino::native::WindowId::new(1),
        engine,
        Size::new(800.0, 600.0),
        1.0,
    );
    let mut host = HeadlessHost::new(backend, Environment::default());
    let root = host.mount("counter", mount_counter);
    let selector = fino::dom_api::Selector {
        role: fino::dom_api::Role::Button,
        name: "Increment counter".to_owned(),
    };

    let before = host.dom_snapshot();
    let ids: Vec<_> = before.nodes.iter().map(|node| node.id.clone()).collect();
    assert_eq!(ids, ["node-1", "node-2", "node-3"]);
    host.click(&selector).expect("first click");
    host.click(&selector).expect("second click");
    host.assert_text(&selector, "Count: 2")
        .expect("second click text");

    let after = host.dom_snapshot();
    let after_ids: Vec<_> = after.nodes.iter().map(|node| node.id.clone()).collect();
    assert_eq!(after_ids, ids, "node identity stays stable");
    assert_eq!(host.runtime().component_executions().get(), 1);
    let reactive = host.reactive_snapshot();
    assert_eq!(reactive.component_executions, 1);
    assert_eq!(reactive.effects[0].executions, 3);
    assert_eq!(
        host.runtime().backend().text_content(root).as_deref(),
        Some("Count: 2")
    );

    let text_updates: Vec<_> = host
        .runtime()
        .backend()
        .operation_log()
        .into_iter()
        .filter(|operation| operation.operation == "set_text")
        .map(|operation| operation.details)
        .collect();
    assert_eq!(text_updates.len(), 3, "initial 0, then 1, then 2");
    let values: Vec<Value> = text_updates;
    assert_eq!(values[0]["set_text"]["value"], "0");
    assert_eq!(values[1]["set_text"]["value"], "1");
    assert_eq!(values[2]["set_text"]["value"], "2");
}
