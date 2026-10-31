#![cfg(not(target_arch = "wasm32"))]

use std::{path::PathBuf, process::Command};

use fino::{headless::SnapshotKind, inspect::inspect_artifact, scenario::Status};
use fino_counter::runner::{
    run_mock_show_conformance, run_mock_show_scenario, run_native_offscreen_show_conformance,
    run_native_offscreen_show_scenario,
};
use serde_json::Value;

fn show_scenario() -> fino::scenario::Scenario {
    serde_json::from_str(include_str!("../scenarios/show.json")).expect("valid show scenario")
}

#[test]
fn show_reports_shared_structure_events_and_counters() {
    let mock = run_mock_show_conformance().expect("mock show conformance");
    assert_eq!(mock.backend, "mock");
    assert!(mock.validate().is_empty(), "{:?}", mock.validate());

    let native = run_native_offscreen_show_conformance().expect("native show conformance");
    assert_eq!(native.backend, "native-offscreen");
    assert!(native.validate().is_empty(), "{:?}", native.validate());

    assert_eq!(mock.initial_tree, native.initial_tree);
    assert_eq!(mock.visible_tree, native.visible_tree);
    assert_eq!(mock.hidden_tree, native.hidden_tree);
    assert_eq!(mock.restored_tree, native.restored_tree);
    assert_eq!(mock.final_tree, native.final_tree);
    assert_eq!(mock.clicked_text, native.clicked_text);
    assert_eq!(mock.operations, native.operations);
    assert_eq!(mock.static_ids_stable, native.static_ids_stable);
    assert_eq!(mock.stale_write_is_noop, native.stale_write_is_noop);
    assert!(mock.storage_stable);
    assert!(native.storage_stable);
    assert_eq!(mock.storage_loops, 10);
    assert_eq!(
        mock.final_stats.component_executions,
        native.final_stats.component_executions
    );
    assert_eq!(
        mock.final_stats.effect_executions,
        native.final_stats.effect_executions
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

#[test]
fn show_scenario_runs_on_mock_and_native_offscreen() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let scenario = show_scenario();

    let mock_dir = temporary.path().join("show-mock");
    let mock = run_mock_show_scenario(&scenario, &mock_dir).expect("mock show scenario");
    assert_eq!(mock.status, Status::Passed);
    assert_eq!(mock.backend, "mock");
    assert_eq!(mock.runtime_stats.component_executions, 1);
    assert!(mock.errors.is_empty());
    let dom = inspect_artifact(&mock_dir, SnapshotKind::Dom).expect("mock DOM");
    assert_eq!(
        dom["nodes"]
            .as_array()
            .expect("mock DOM nodes")
            .iter()
            .filter(|node| node["kind"] == "anchor")
            .count(),
        2,
        "the scenario leaves exactly one anchor pair"
    );
    let mock_a11y = inspect_artifact(&mock_dir, SnapshotKind::A11y).expect("mock a11y");
    assert!(
        mock_a11y["nodes"]
            .as_array()
            .expect("mock a11y nodes")
            .iter()
            .any(|node| node["name"] == "Show host" && node["text"] == "HostToggleTail")
    );

    let native_dir = temporary.path().join("show-native");
    let native =
        run_native_offscreen_show_scenario(&scenario, &native_dir).expect("native show scenario");
    assert_eq!(native.status, Status::Passed);
    assert_eq!(native.backend, "native-offscreen");
    assert_eq!(native.runtime_stats.component_executions, 1);
    assert!(native.errors.is_empty());
    let native_a11y = inspect_artifact(&native_dir, SnapshotKind::A11y).expect("native a11y");
    assert!(
        native_a11y["nodes"]
            .as_array()
            .expect("native a11y nodes")
            .iter()
            .any(|node| node["name"] == "Show host" && node["text"] == "HostToggleTail")
    );
}

#[test]
fn show_scenario_capture_excludes_removed_action() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let run_dir = temporary.path().join("show-capture");
    let result = run_mock_show_scenario(&show_scenario(), &run_dir).expect("show capture");
    assert_eq!(result.status, Status::Passed);

    let dom = inspect_artifact(&run_dir, SnapshotKind::Dom).expect("final DOM");
    let host = dom["nodes"]
        .as_array()
        .expect("DOM nodes")
        .iter()
        .find(|node| node["attributes"]["aria-label"] == "Show host")
        .expect("Show host");
    let children: Vec<_> = host["children"].as_array().expect("children").clone();
    assert!(!children.is_empty());
    let a11y = inspect_artifact(&run_dir, SnapshotKind::A11y).expect("a11y");
    let names: Vec<_> = a11y["nodes"]
        .as_array()
        .expect("a11y nodes")
        .iter()
        .filter_map(|node| node["name"].as_str())
        .collect();
    assert!(names.contains(&"Show host"));
    assert!(names.contains(&"Show toggle"));
    assert!(!names.contains(&"Show action"));

    let reactive = inspect_artifact(&run_dir, SnapshotKind::Reactive).expect("reactive");
    let value: Value = reactive;
    assert_eq!(value["component_executions"], 1);
}

#[test]
fn show_scenario_cli_uses_the_explicit_mount() {
    let scenario = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/show.json");
    for backend in ["mock", "native-offscreen"] {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let run_dir = temporary.path().join(format!("show-cli-{backend}"));
        let output = Command::new(env!("CARGO_BIN_EXE_fino_counter"))
            .arg("scenario")
            .arg("run")
            .arg(&scenario)
            .arg("--backend")
            .arg(backend)
            .arg("--mount")
            .arg("show")
            .arg("--out")
            .arg(&run_dir)
            .args(["--format", "json"])
            .output()
            .expect("show CLI output");
        assert!(
            output.status.success(),
            "{backend} show CLI failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value = serde_json::from_slice(&output.stdout).expect("show CLI JSON");
        assert_eq!(result["status"], "passed");
        assert_eq!(result["backend"], backend);
        assert_eq!(result["counters"]["component_executions"], 1);
        assert_eq!(result["steps"].as_array().map(Vec::len), Some(10));
    }
}

/// Proves repeated NativeOffscreen toggles release arena storage and invalidate
/// generational handles instead of accumulating detached subtrees.
/// 验证 NativeOffscreen 反复切换会释放 arena 存储并使 generational 句柄失效，
/// 而不是累积分离子树。
#[test]
fn native_show_toggles_release_arena_storage() {
    use std::{cell::RefCell, rc::Rc};

    use fino::{
        dom_api::{Attribute, DomBackend, Tag},
        headless::HeadlessBackend as _,
        native::{NodeId, Size, offscreen_backend},
        runtime::Runtime,
        show::{Show, ShowContext},
    };

    let backend = offscreen_backend(Size::new(800.0, 600.0), 1.0);
    let runtime = Runtime::new(backend);
    let owner = runtime.create_owner();
    let visible = owner.signal_named("show/storage", false);
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = runtime.backend_mut().create_text("Tail");
    runtime.backend_mut().insert_before(&root, &tail, None);
    let action_slot: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let slot = Rc::clone(&action_slot);
    let show = {
        let visible = visible.clone();
        Show::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "show/storage",
            move || visible.get(),
            move |context: &mut ShowContext<_>| {
                let action = context.create_element(Tag::Button);
                context.set_attribute(&action, Attribute::AriaLabel, Some("Storage action"));
                let value = context.create_text("value");
                context.insert_before(&action, &value, None);
                *slot.borrow_mut() = Some(action);
                action
            },
        )
    };

    let baseline = runtime.backend().dom_snapshot().nodes.len();
    for _ in 0..50 {
        visible.set(true);
        visible.set(false);
    }
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        baseline,
        "50 hide/show loops must not grow native arena storage"
    );
    let old = action_slot.borrow_mut().take().expect("last action handle");
    assert!(
        !runtime.backend().contains(old),
        "released NodeId handles must not stay live"
    );

    visible.set(true);
    let new = action_slot.borrow_mut().take().expect("new action handle");
    assert_ne!(old, new, "a released generational NodeId is never reused");
    assert!(runtime.backend().contains(new));
    assert_eq!(runtime.backend().dom_snapshot().nodes.len(), baseline + 2);
    assert!(show.is_mounted());
}

/// Proves a pending window frame survives a released range without retaining
/// its content, and later stale signal writes do not request another frame.
/// 验证已释放范围不会把内容留在待处理窗口帧中，且之后旧 Signal 写入不再请求帧。
#[test]
fn native_show_pending_frame_after_release_is_safe() {
    use fino::{
        dom_api::{Attribute, DomBackend, Tag},
        headless::HeadlessBackend as _,
        native::{Size, WindowConstraints, WindowId, offscreen_backend},
        runtime::Runtime,
        show::{Show, ShowContext},
    };

    let backend = offscreen_backend(Size::new(800.0, 600.0), 1.0);
    let runtime = Runtime::new(backend);
    let owner = runtime.create_owner();
    let visible = owner.signal_named("show/frame", false);
    let count = owner.signal_named("show/frame-count", 0);
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = runtime.backend_mut().create_text("Tail");
    runtime.backend_mut().insert_before(&root, &tail, None);
    let show = {
        let visible = visible.clone();
        let count = count.clone();
        Show::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "show/frame-range",
            move || visible.get(),
            move |context: &mut ShowContext<_>| {
                let action = context.create_element(Tag::Button);
                context.set_attribute(&action, Attribute::AriaLabel, Some("Frame action"));
                let value = context.create_text("stale");
                context.insert_before(&action, &value, None);
                let read_count = count.clone();
                context.effect("show/frame/text", move |effect| {
                    effect
                        .backend_mut()
                        .set_text(&value, &read_count.get().to_string());
                });
                action
            },
        )
    };

    visible.set(true);
    assert!(runtime.backend().frame_request_pending());
    visible.set(false);
    assert!(
        runtime.backend().take_frame_request(),
        "the coalesced window request remains valid after release"
    );
    assert!(!runtime.backend().frame_request_pending());

    let operations = runtime.backend().operation_log().len();
    count.set(1);
    assert_eq!(
        runtime.backend().operation_log().len(),
        operations,
        "the removed text effect no longer touches the backend"
    );
    assert!(!runtime.backend().frame_request_pending());

    let window = WindowId::new(1);
    let viewport = Size::new(800.0, 600.0);
    let _ = runtime
        .backend_mut()
        .layout(WindowConstraints::new(window, viewport));
    let list = runtime.backend_mut().paint(window, viewport);
    let painted = serde_json::to_string(&list.to_snapshot()).expect("display list JSON");
    assert!(
        !painted.contains("\"stale\"") && !painted.contains("\"1\""),
        "a pending frame cannot paint released range content"
    );
    assert_eq!(
        runtime.backend().text_content(root).as_deref(),
        Some("Tail")
    );
    assert!(!show.is_mounted());
}

/// Proves Native anchors are never valid text targets.
/// 验证 Native 锚点绝不是合法的文本写入目标。
#[test]
fn native_anchor_rejects_text_writes() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use fino::{
        dom_api::DomBackend,
        native::{Size, offscreen_backend},
    };

    let mut backend = offscreen_backend(Size::new(800.0, 600.0), 1.0);
    let anchor = backend.create_anchor();
    let text = backend.create_text("text");
    let rejected = catch_unwind(AssertUnwindSafe(|| backend.set_text(&anchor, "leak")));
    assert!(rejected.is_err(), "writing an anchor must fail");
    backend.set_text(&text, "changed");
    assert_eq!(backend.text_content(text).as_deref(), Some("changed"));
}
