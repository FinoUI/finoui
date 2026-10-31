#![cfg(not(target_arch = "wasm32"))]

use std::{path::PathBuf, process::Command};

use fino::{headless::SnapshotKind, inspect::inspect_artifact, scenario::Status};
use fino_counter::runner::{
    run_mock_keyed_conformance, run_mock_keyed_scenario, run_native_offscreen_keyed_conformance,
    run_native_offscreen_keyed_scenario,
};
use serde_json::Value;

fn keyed_scenario() -> fino::scenario::Scenario {
    serde_json::from_str(include_str!("../scenarios/keyed.json")).expect("valid keyed scenario")
}

/// Mock and Native agree on trees, moves, identity, duplicate rejection, and
/// operation counts for the shared keyed sequence.
/// Mock 与 Native 在共享 keyed 序列的树、移动、身份、重复 key 拒绝与操作计数
/// 上保持一致。
#[test]
fn keyed_reports_shared_structure_moves_and_counters() {
    let mock = run_mock_keyed_conformance().expect("mock keyed conformance");
    assert_eq!(mock.backend, "mock");
    assert!(mock.validate().is_empty(), "{:?}", mock.validate());

    let native = run_native_offscreen_keyed_conformance().expect("native keyed conformance");
    assert_eq!(native.backend, "native-offscreen");
    assert!(native.validate().is_empty(), "{:?}", native.validate());

    assert_eq!(mock.initial_tree, native.initial_tree);
    assert_eq!(mock.reordered_tree, native.reordered_tree);
    assert_eq!(mock.disposed_tree, native.disposed_tree);
    assert_eq!(mock.final_tree, native.final_tree);
    assert_eq!(mock.initial_text, native.initial_text);
    assert_eq!(mock.phases, native.phases);
    assert_eq!(mock.local_update, native.local_update);
    assert_eq!(mock.operations, native.operations);
    assert_eq!(mock.builder_runs, native.builder_runs);
    assert_eq!(mock.stale_write_is_noop, native.stale_write_is_noop);
    assert_eq!(mock.duplicate_key_rejected, native.duplicate_key_rejected);
    assert_eq!(mock.duplicate_recovered, native.duplicate_recovered);
    assert_eq!(mock.dispose_idempotent, native.dispose_idempotent);
    assert_eq!(mock.static_ids_stable, native.static_ids_stable);
    assert_eq!(mock.item_identity_stable, native.item_identity_stable);
    assert_eq!(mock.reconcile_executions, native.reconcile_executions);
    assert_eq!(
        mock.final_stats.component_executions,
        native.final_stats.component_executions
    );
    assert_eq!(
        mock.final_stats.effect_executions,
        native.final_stats.effect_executions
    );
    assert_eq!(
        mock.final_stats.backend_set_text,
        native.final_stats.backend_set_text
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

/// The JSON keyed scenario runs on Mock and NativeOffscreen through explicit mounts.
/// JSON keyed 场景通过显式 mount 在 Mock 与 NativeOffscreen 上运行。
#[test]
fn keyed_scenario_runs_on_mock_and_native_offscreen() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let scenario = keyed_scenario();

    let mock_dir = temporary.path().join("keyed-mock");
    let mock = run_mock_keyed_scenario(&scenario, &mock_dir).expect("mock keyed scenario");
    assert_eq!(mock.status, Status::Passed);
    assert_eq!(mock.backend, "mock");
    assert_eq!(mock.steps.len(), 28);
    assert_eq!(mock.runtime_stats.component_executions, 1);
    assert!(mock.errors.is_empty());
    let dom = inspect_artifact(&mock_dir, SnapshotKind::Dom).expect("mock keyed DOM");
    assert_eq!(
        dom["nodes"]
            .as_array()
            .expect("keyed DOM nodes")
            .iter()
            .filter(|node| node["kind"] == "anchor")
            .count(),
        6,
        "one list anchor pair and two item anchor pairs remain"
    );
    let a11y = inspect_artifact(&mock_dir, SnapshotKind::A11y).expect("mock keyed a11y");
    let host = a11y["nodes"]
        .as_array()
        .expect("keyed a11y nodes")
        .iter()
        .find(|node| node["name"] == "Keyed host")
        .expect("keyed host");
    assert_eq!(host["text"], "HostNextD=1E=0Tail");

    let native_dir = temporary.path().join("keyed-native");
    let native =
        run_native_offscreen_keyed_scenario(&scenario, &native_dir).expect("native keyed scenario");
    assert_eq!(native.status, Status::Passed);
    assert_eq!(native.backend, "native-offscreen");
    assert_eq!(native.steps.len(), 28);
    assert_eq!(native.runtime_stats.component_executions, 1);
    assert!(native.errors.is_empty());
    let a11y = inspect_artifact(&native_dir, SnapshotKind::A11y).expect("native keyed a11y");
    let host = a11y["nodes"]
        .as_array()
        .expect("keyed a11y nodes")
        .iter()
        .find(|node| node["name"] == "Keyed host")
        .expect("keyed host");
    assert_eq!(host["text"], "HostNextD=1E=0Tail");
}

/// The example CLI dispatches the keyed mount explicitly on both local backends.
/// 示例 CLI 在两个本地后端上显式分发 keyed mount。
#[test]
fn keyed_scenario_cli_uses_the_explicit_mount() {
    let scenario = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/keyed.json");
    for backend in ["mock", "native-offscreen"] {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let run_dir = temporary.path().join(format!("keyed-cli-{backend}"));
        let output = Command::new(env!("CARGO_BIN_EXE_fino_counter"))
            .arg("scenario")
            .arg("run")
            .arg(&scenario)
            .arg("--backend")
            .arg(backend)
            .arg("--mount")
            .arg("keyed")
            .arg("--out")
            .arg(&run_dir)
            .args(["--format", "json"])
            .output()
            .expect("keyed CLI output");
        assert!(
            output.status.success(),
            "{backend} keyed CLI failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value = serde_json::from_slice(&output.stdout).expect("keyed CLI JSON");
        assert_eq!(result["status"], "passed");
        assert_eq!(result["backend"], backend);
        assert_eq!(result["counters"]["component_executions"], 1);
        assert_eq!(result["steps"].as_array().map(Vec::len), Some(28));
    }
}

/// NativeOffscreen releases removed item arenas, invalidates handles, and never
/// paints a released row after disposal.
/// NativeOffscreen 释放已移除条目的 arena，使句柄失效，并且释放后绝不绘制
/// 已释放行。
#[test]
fn native_keyed_dispose_releases_arena_and_display_list() {
    use fino::{
        dom_api::DomBackend as _,
        keyed::For,
        native::{Size, WindowConstraints, WindowId, offscreen_backend},
        runtime::Runtime,
    };
    use fino_counter::keyed::row_selector;

    let backend = offscreen_backend(Size::new(800.0, 600.0), 1.0);
    let runtime = Runtime::new(backend);
    let owner = runtime.create_owner();
    let keys = owner.signal_named(
        "native/keyed",
        vec!["A".to_owned(), "B".to_owned(), "C".to_owned()],
    );
    let mount = {
        let read_keys = keys.clone();
        let root = runtime
            .backend_mut()
            .create_element(fino::dom_api::Tag::Button);
        let tail = runtime.backend_mut().create_text("Tail");
        runtime.backend_mut().insert_before(&root, &tail, None);
        let list = For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/native",
            move || read_keys.get(),
            |context, key| {
                let button = context.create_element(fino::dom_api::Tag::Button);
                context.set_attribute(
                    &button,
                    fino::dom_api::Attribute::AriaLabel,
                    Some(&format!("Row {key}")),
                );
                let value = context.create_text(key);
                context.insert_before(&button, &value, None);
                vec![button]
            },
        );
        (root, list)
    };
    let root = mount.0;
    let list = mount.1;
    let row_a = runtime
        .backend()
        .find_all(&row_selector("A"))
        .first()
        .copied()
        .expect("row A handle");
    assert!(runtime.backend().contains(row_a));
    keys.set(vec!["B".to_owned(), "C".to_owned()]);
    assert!(
        !runtime.backend().contains(row_a),
        "a removed key releases its NodeId storage"
    );
    assert!(runtime.backend().find_all(&row_selector("A")).is_empty());
    let window = WindowId::new(1);
    let viewport = Size::new(800.0, 600.0);
    let _ = runtime
        .backend_mut()
        .layout(WindowConstraints::new(window, viewport));
    let list_snapshot = runtime.backend_mut().paint(window, viewport);
    let painted = serde_json::to_string(&list_snapshot.to_snapshot()).expect("display list JSON");
    assert!(
        !painted.contains("\"A\""),
        "the display list never reads a released row"
    );
    assert!(
        runtime
            .backend()
            .text_content(root)
            .is_some_and(|text| !text.contains("A"))
    );

    let row_b = runtime
        .backend()
        .find_all(&row_selector("B"))
        .first()
        .copied()
        .expect("row B handle");
    list.dispose();
    list.dispose();
    assert!(
        !runtime.backend().contains(row_b),
        "whole-list disposal releases every remaining row"
    );
    assert_eq!(
        runtime.backend().text_content(root).as_deref(),
        Some("Tail")
    );
}
