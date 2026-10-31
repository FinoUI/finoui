use std::{cell::Cell, fs, path::Path, rc::Rc};

use fino::{
    backend_mock::MockBackend,
    dom_api::{Attribute, DomBackend, EventKind, Role, Selector, Tag},
    headless::{Environment, HeadlessBackend, HeadlessHost, SnapshotKind},
    inspect::inspect_artifact,
    metrics::RuntimeStats,
    scenario::{Scenario, ScenarioResult, Status, Step, run_on_host},
};
use fino_counter::runner::{mount_counter, run_mock_scenario};
use serde_json::Value;

fn counter_scenario() -> Scenario {
    serde_json::from_str(include_str!("../scenarios/counter.json")).expect("valid counter scenario")
}

fn reachability_probe_selector() -> Selector {
    Selector {
        role: Role::Button,
        name: "Reachability probe".to_owned(),
    }
}

/// Verifies stable run identity, virtual time, semantic snapshots, and deterministic artifacts.
/// 验证稳定运行标识、虚拟时间、语义快照与确定性产物。
#[test]
fn mock_scenario_writes_deterministic_snapshots_and_trace() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let first_dir = temporary.path().join("first");
    let second_dir = temporary.path().join("second");
    let scenario = counter_scenario();

    let first = run_mock_scenario(&scenario, &first_dir).expect("first scenario run");
    let second = run_mock_scenario(&scenario, &second_dir).expect("second scenario run");

    assert_eq!(first.status, Status::Passed);
    assert_eq!(first.run_id, second.run_id);
    assert_eq!(first.run_dir, first_dir.to_string_lossy());
    assert_eq!(first.counters.component_executions, 1);
    assert_eq!(first.counters.effect_executions, 2);
    assert_eq!(first.counters.frame, 1);
    assert_eq!(first.counters.virtual_time_ms, 16);
    assert_eq!(first.runtime_stats.component_executions, 1);
    assert_eq!(first.runtime_stats.effect_executions, 2);
    assert_eq!(first.runtime_stats.backend_set_text, 2);
    assert_eq!(first.runtime_stats.nodes_created, 3);
    assert_eq!(first.runtime_stats.nodes_removed, 0);
    assert_eq!(first.runtime_stats.shaped_texts, 0);
    assert_eq!(first.runtime_stats.layout_nodes, 0);
    assert_eq!(first.runtime_stats.frames_requested, 0);
    assert_eq!(first.captures.len(), 1);
    assert_eq!(first.captures[0].step, 4);
    assert!(first.steps.iter().all(|step| step.status == Status::Passed));
    assert!(first.errors.is_empty());

    for artifact in ["dom.json", "a11y.json", "reactive.json", "trace.json"] {
        let first_json = fs::read_to_string(first_dir.join(artifact)).expect("first artifact");
        let second_json = fs::read_to_string(second_dir.join(artifact)).expect("second artifact");
        assert_eq!(first_json, second_json, "deterministic {artifact}");
    }

    let dom = inspect_artifact(&first_dir, SnapshotKind::Dom).expect("DOM inspection");
    let dom_text = serde_json::to_string(&dom).expect("DOM JSON");
    let dom_nodes = dom["nodes"].as_array().expect("DOM nodes");
    assert!(dom_nodes.iter().any(|node| node["text"] == "Count: "));
    assert!(dom_nodes.iter().any(|node| node["text"] == "1"));
    assert!(dom_text.contains("aria-label"));
    assert!(!dom_text.contains("0x"));

    let a11y = inspect_artifact(&first_dir, SnapshotKind::A11y).expect("a11y inspection");
    let button = a11y["nodes"]
        .as_array()
        .expect("a11y nodes")
        .iter()
        .find(|node| node["role"] == "button" && node["name"] == "Increment counter")
        .expect("counter button semantics");
    assert_eq!(button["text"], "Count: 1");

    let reactive =
        inspect_artifact(&first_dir, SnapshotKind::Reactive).expect("reactive inspection");
    assert_eq!(reactive["component_executions"], 1);
    assert_eq!(reactive["signals"][0]["name"], "counter/count");
    assert_eq!(reactive["effects"][0]["name"], "counter/text");
    assert_eq!(reactive["effects"][0]["executions"], 2);
    assert_eq!(reactive["effects"][0]["dependencies"][0], "signal-1");

    let trace = inspect_artifact(&first_dir, SnapshotKind::Trace).expect("trace inspection");
    let events = trace["events"].as_array().expect("trace events");
    assert!(events.iter().any(|event| {
        event["kind"] == "backend"
            && event["details"]["operation"] == "set_text"
            && event["details"]["details"]["set_text"]["value"] == "1"
    }));
    assert!(events.iter().any(|event| event["kind"] == "capture"));
    assert!(events.iter().any(|event| {
        event["kind"] == "advance_frames" && event["details"]["virtual_time_ms"] == 16
    }));
    assert!(!trace.to_string().contains("timestamp"));

    let saved_result: Value = serde_json::from_str(
        &fs::read_to_string(first_dir.join("result.json")).expect("saved result"),
    )
    .expect("result JSON");
    assert_eq!(saved_result["run_id"], first.run_id);
    assert_eq!(saved_result["snapshots"]["dom"], "dom.json");
}

/// Reads an older schema-1 result that predates `runtime_stats`.
/// 读取缺少 `runtime_stats` 的旧 schema-1 结果。
#[test]
fn legacy_result_without_runtime_stats_deserializes_with_defaults() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let run_dir = temporary.path().join("legacy-result");
    let result = run_mock_scenario(&counter_scenario(), &run_dir).expect("scenario run");
    let mut value = serde_json::to_value(&result).expect("result JSON");
    value
        .as_object_mut()
        .expect("result object")
        .remove("runtime_stats");

    let legacy: ScenarioResult = serde_json::from_value(value).expect("legacy result");
    assert_eq!(legacy.schema, 1);
    assert_eq!(legacy.runtime_stats, RuntimeStats::default());
    assert_eq!(legacy.run_id, result.run_id);
    assert_eq!(legacy.counters, result.counters);
    assert_eq!(legacy.status, result.status);
}

/// Locks capture files to their step state so later clicks cannot replace history.
/// 将捕获文件固定到步骤时点，防止后续点击覆盖历史状态。
#[test]
fn capture_steps_preserve_before_and_after_click_states() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let run_dir = temporary.path().join("captures");
    let mut scenario = counter_scenario();
    scenario
        .steps
        .insert(0, Step::Capture(vec![SnapshotKind::Dom]));

    let result = run_mock_scenario(&scenario, &run_dir).expect("capture scenario run");

    assert_eq!(result.status, Status::Passed);
    assert_eq!(result.captures.len(), 2);
    assert_eq!(
        result.captures[0].artifacts["dom"],
        "captures/step-0/dom.json"
    );
    assert_eq!(
        result.captures[1].artifacts["dom"],
        "captures/step-5/dom.json"
    );
    let before = inspect_artifact(&run_dir.join("captures/step-0"), SnapshotKind::Dom)
        .expect("before-click capture");
    let after = inspect_artifact(&run_dir.join("captures/step-5"), SnapshotKind::Dom)
        .expect("after-click capture");
    assert!(
        before["nodes"]
            .as_array()
            .expect("before nodes")
            .iter()
            .any(|node| { node["kind"] == "text" && node["text"] == "0" })
    );
    assert!(
        after["nodes"]
            .as_array()
            .expect("after nodes")
            .iter()
            .any(|node| { node["kind"] == "text" && node["text"] == "1" })
    );
    let final_dom = inspect_artifact(&run_dir, SnapshotKind::Dom).expect("final DOM");
    assert!(
        final_dom["nodes"]
            .as_array()
            .expect("final nodes")
            .iter()
            .any(|node| { node["kind"] == "text" && node["text"] == "1" })
    );
}

/// Requires reactive cycles to fail the run while retaining diagnostic evidence.
/// 要求响应式循环使运行失败，同时保留诊断证据。
#[test]
fn reactive_cycles_fail_the_scenario_and_preserve_artifacts() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let run_dir = temporary.path().join("cycle");
    let scenario = counter_scenario();
    let host = HeadlessHost::new(MockBackend::new(), Environment::default());

    let result = run_on_host(&scenario, &run_dir, host, |runtime, owner| {
        let root = mount_counter(runtime, owner);
        let cyclic = owner.signal_named("test/scenario-cycle", 0_i32);
        let effect_cyclic = cyclic.clone();
        owner.effect_named("test/scenario-cycle-effect", move || {
            let value = effect_cyclic.get();
            if value == 0 {
                effect_cyclic.set(1);
            }
        });
        root
    })
    .expect("cycle run artifacts");

    assert_eq!(result.status, Status::Failed);
    assert!(
        result
            .errors
            .iter()
            .any(|error| error.code.as_ref() == "UI_REACTIVE_CYCLE")
    );
    assert!(run_dir.join("result.json").exists());
    assert!(run_dir.join("reactive.json").exists());
    assert!(run_dir.join("trace.json").exists());
}

/// Ensures unsupported capture capabilities fail explicitly without empty artifacts.
/// 确保不支持的捕获能力显式失败，且不生成空产物。
#[test]
fn unsupported_snapshot_is_an_explicit_structured_failure() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let run_dir = temporary.path().join("unsupported");
    let mut scenario = counter_scenario();
    scenario.steps[4] = Step::Capture(vec![SnapshotKind::Layout]);

    let result = run_mock_scenario(&scenario, &run_dir).expect("completed failed run");

    assert_eq!(result.status, Status::Failed);
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.errors[0].code.as_ref(), "UI_SNAPSHOT_UNSUPPORTED");
    assert_eq!(result.errors[0].step, Some(4));
    assert_eq!(result.errors[0].capability.as_deref(), Some("layout"));
    assert!(!run_dir.join("layout.json").exists());
    assert!(!run_dir.join("captures/step-4").exists());
    assert!(result.captures.is_empty());
    assert!(run_dir.join("dom.json").exists());
    assert!(run_dir.join("trace.json").exists());
}

/// Excludes a detached button from semantic actions while retaining its listener.
/// 将已分离的按钮排除在语义动作之外，同时保留其监听器。
#[test]
fn detached_button_is_unreachable_and_keeps_its_listener() {
    let mut host = HeadlessHost::new(MockBackend::new(), Environment::default());
    let button = host.mount("test/detach", |runtime, owner| {
        let button = runtime.backend_mut().create_element(Tag::Button);
        runtime.backend_mut().set_attribute(
            &button,
            Attribute::AriaLabel,
            Some("Reachability probe"),
        );
        let label = runtime.backend_mut().create_text("Count: 1");
        runtime.backend_mut().insert_before(&button, &label, None);
        runtime.listen(owner, &button, EventKind::Click, |_event, _context| {});
        runtime.backend_mut().remove(&button);
        button
    });
    let selector = reachability_probe_selector();

    assert!(host.runtime().backend().find_all(&selector).is_empty());
    assert_eq!(
        host.assert_text(&selector, "Count: 1")
            .expect_err("detached text is unreachable")
            .code,
        "UI_SELECTOR_NOT_FOUND"
    );
    assert_eq!(
        host.click(&selector)
            .expect_err("detached clicks are unreachable")
            .code,
        "UI_SELECTOR_NOT_FOUND"
    );
    assert!(host.a11y_snapshot().nodes.is_empty());
    let dom = host.dom_snapshot();
    assert!(
        dom.roots.is_empty(),
        "an explicitly detached node is stored but is not a live root"
    );
    assert!(
        dom.nodes
            .iter()
            .any(|node| node.id == "node-1" && node.tag.as_deref() == Some("button")),
        "the detached node keeps its storage and handle for reinsertion"
    );
    assert_eq!(host.runtime().backend().listener_count(), 1);
    assert!(
        host.runtime()
            .backend()
            .listener(&button, EventKind::Click)
            .is_some()
    );

    host.dispose();
    assert_eq!(host.runtime().backend().listener_count(), 0);
}

/// Resolves only the replacement button when a detached twin shares its label.
/// 当已分离的同名按钮存在时，只解析替换后的按钮。
#[test]
fn replacement_button_is_not_ambiguous_with_a_detached_twin() {
    let clicks = Rc::new(Cell::new(0));
    let new_clicks = Rc::clone(&clicks);
    let mut host = HeadlessHost::new(MockBackend::new(), Environment::default());
    host.mount("test/replace", move |runtime, owner| {
        let old = runtime.backend_mut().create_element(Tag::Button);
        runtime
            .backend_mut()
            .set_attribute(&old, Attribute::AriaLabel, Some("Reachability probe"));
        let old_label = runtime.backend_mut().create_text("Count: old");
        runtime.backend_mut().insert_before(&old, &old_label, None);
        runtime.listen(owner, &old, EventKind::Click, |_event, _context| {});
        runtime.backend_mut().remove(&old);

        let new = runtime.backend_mut().create_element(Tag::Button);
        runtime
            .backend_mut()
            .set_attribute(&new, Attribute::AriaLabel, Some("Reachability probe"));
        let new_label = runtime.backend_mut().create_text("Count: new");
        runtime.backend_mut().insert_before(&new, &new_label, None);
        runtime.listen(owner, &new, EventKind::Click, move |_event, _context| {
            new_clicks.set(new_clicks.get() + 1);
        });
        new
    });
    let selector = reachability_probe_selector();

    let matches = host.runtime().backend().find_all(&selector);
    assert_eq!(matches.len(), 1);
    let a11y = host.a11y_snapshot();
    assert_eq!(a11y.nodes.len(), 1);
    assert_eq!(a11y.nodes[0].id, format!("node-{}", matches[0].0));
    assert_eq!(a11y.nodes[0].name, "Reachability probe");
    assert_eq!(a11y.nodes[0].text, "Count: new");
    assert_eq!(a11y.nodes[0].actions, vec!["click".to_owned()]);
    let dom = host.dom_snapshot();
    let buttons: Vec<_> = dom
        .nodes
        .iter()
        .filter(|node| node.tag.as_deref() == Some("button"))
        .map(|node| node.id.as_str())
        .collect();
    assert_eq!(buttons, ["node-1", "node-3"]);
    host.assert_text(&selector, "Count: new")
        .expect("replacement text is unambiguous");
    host.click(&selector).expect("replacement click");
    assert_eq!(clicks.get(), 1);
    assert_eq!(host.runtime().backend().listener_count(), 2);

    host.dispose();
    assert_eq!(host.runtime().backend().listener_count(), 0);
}

/// Exercises every required final capability and checks independent artifact preservation.
/// 逐项检查最终必需能力，并验证其他真实产物仍被保留。
#[test]
fn final_artifacts_check_every_required_snapshot_capability() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut scenario = counter_scenario();
    scenario.steps.clear();

    for kind in [
        SnapshotKind::Dom,
        SnapshotKind::A11y,
        SnapshotKind::Reactive,
        SnapshotKind::Trace,
    ] {
        let run_dir = temporary.path().join(kind.name());
        let mut backend = MockBackend::new();
        backend.set_snapshot_supported(kind, false);
        let host = HeadlessHost::new(backend, scenario.environment.clone());
        let result = run_on_host(&scenario, &run_dir, host, mount_counter)
            .expect("final capability failure artifacts");

        assert_eq!(result.status, Status::Failed, "{kind:?}");
        assert!(
            result.errors.iter().any(|error| {
                error.code.as_ref() == "UI_SNAPSHOT_UNSUPPORTED"
                    && error.capability.as_deref() == Some(kind.name())
                    && error.step.is_none()
            }),
            "{kind:?}"
        );
        assert!(!run_dir.join(kind.artifact_name()).exists(), "{kind:?}");
        assert!(run_dir.join("result.json").exists(), "{kind:?}");
    }
}

/// Removes the stale managed snapshot when a rerun loses a required capability.
/// 重跑失去某项必需能力时，删除上一次留下的受管快照。
#[test]
fn failed_rerun_removes_only_the_stale_required_snapshot() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let scenario = counter_scenario();
    let required = [
        SnapshotKind::Dom,
        SnapshotKind::A11y,
        SnapshotKind::Reactive,
        SnapshotKind::Trace,
    ];

    for kind in required {
        let run_dir = temporary.path().join(kind.name());
        let first = run_mock_scenario(&scenario, &run_dir).expect("successful first run");
        assert_eq!(first.status, Status::Passed, "{kind:?}");
        assert!(run_dir.join(kind.artifact_name()).exists(), "{kind:?}");

        let mut backend = MockBackend::new();
        backend.set_snapshot_supported(kind, false);
        let host = HeadlessHost::new(backend, scenario.environment.clone());
        let second =
            run_on_host(&scenario, &run_dir, host, mount_counter).expect("completed failed rerun");

        assert_eq!(second.status, Status::Failed, "{kind:?}");
        assert!(
            second.errors.iter().any(|error| {
                error.code.as_ref() == "UI_SNAPSHOT_UNSUPPORTED"
                    && error.capability.as_deref() == Some(kind.name())
            }),
            "{kind:?}"
        );
        assert!(!run_dir.join(kind.artifact_name()).exists(), "{kind:?}");
        assert_eq!(
            inspect_artifact(&run_dir, kind)
                .expect_err("stale snapshot is gone")
                .code
                .as_ref(),
            "UI_ARTIFACT_NOT_FOUND",
            "{kind:?}"
        );
        for other in required.into_iter().filter(|other| *other != kind) {
            assert!(
                run_dir.join(other.artifact_name()).exists(),
                "{kind:?} keeps {other:?}"
            );
        }
        assert!(
            run_dir.join("captures/step-4/dom.json").exists(),
            "{kind:?}"
        );
        assert!(run_dir.join("result.json").exists(), "{kind:?}");
    }
}

/// Treats an already absent stale snapshot as success during a failed rerun.
/// 失败重跑时，把本就不存在的旧快照视为删除成功。
#[test]
fn absent_stale_snapshot_does_not_fail_the_rerun() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let run_dir = temporary.path().join("already-absent");
    let scenario = counter_scenario();
    run_mock_scenario(&scenario, &run_dir).expect("successful first run");
    fs::remove_file(run_dir.join(SnapshotKind::A11y.artifact_name())).expect("remove artifact");

    let mut backend = MockBackend::new();
    backend.set_snapshot_supported(SnapshotKind::A11y, false);
    let host = HeadlessHost::new(backend, scenario.environment.clone());
    let result =
        run_on_host(&scenario, &run_dir, host, mount_counter).expect("completed failed rerun");

    assert_eq!(result.status, Status::Failed);
    assert!(
        result
            .errors
            .iter()
            .any(|error| error.code.as_ref() == "UI_SNAPSHOT_UNSUPPORTED")
    );
    assert!(
        result
            .errors
            .iter()
            .all(|error| error.code.as_ref() != "UI_ARTIFACT_IO")
    );
}

/// Reports a stale snapshot removal failure instead of leaving it readable.
/// 旧快照删除失败时返回结构化错误，而不是让它继续可读。
#[test]
fn stale_snapshot_removal_failure_is_reported_as_artifact_io() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let run_dir = temporary.path().join("blocked-removal");
    let scenario = counter_scenario();
    run_mock_scenario(&scenario, &run_dir).expect("successful first run");
    let stale = run_dir.join(SnapshotKind::A11y.artifact_name());
    fs::remove_file(&stale).expect("remove stale artifact");
    fs::create_dir(&stale).expect("block managed artifact removal");
    fs::write(stale.join("keep.txt"), b"keep").expect("blocking content");

    let mut backend = MockBackend::new();
    backend.set_snapshot_supported(SnapshotKind::A11y, false);
    let host = HeadlessHost::new(backend, scenario.environment.clone());
    let error = run_on_host(&scenario, &run_dir, host, mount_counter).expect_err("removal failure");

    assert_eq!(error.code.as_ref(), "UI_ARTIFACT_IO");
    assert!(
        error.message.contains("failed to remove"),
        "{}",
        error.message
    );
}

/// Leaves prior captures unreported and untouched when a capture capability disappears.
/// 捕获能力消失时，不报告也不改写之前的捕获产物。
#[test]
fn failed_capture_rerun_does_not_report_or_rewrite_stale_capture_artifacts() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let run_dir = temporary.path().join("capture-rerun");
    let mut scenario = counter_scenario();
    scenario
        .steps
        .insert(0, Step::Capture(vec![SnapshotKind::Dom]));

    let first = run_mock_scenario(&scenario, &run_dir).expect("successful capture run");
    assert_eq!(first.captures.len(), 2);
    let capture_path = run_dir.join("captures/step-0/dom.json");
    let first_capture = fs::read_to_string(&capture_path).expect("first capture");

    scenario.steps[0] = Step::Capture(vec![SnapshotKind::Layout]);
    let result = run_mock_scenario(&scenario, &run_dir).expect("completed failed rerun");

    assert_eq!(result.status, Status::Failed);
    assert!(result.captures.is_empty());
    assert_eq!(result.errors[0].step, Some(0));
    assert_eq!(result.errors[0].capability.as_deref(), Some("layout"));
    assert!(!run_dir.join("captures/step-0/layout.json").exists());
    assert_eq!(
        fs::read_to_string(&capture_path).expect("retained capture"),
        first_capture
    );
    let saved: Value = serde_json::from_str(
        &fs::read_to_string(run_dir.join("result.json")).expect("saved result"),
    )
    .expect("result JSON");
    assert_eq!(saved["captures"].as_array().map(Vec::len), Some(0));
}

/// Confirms failed assertions stop execution but still emit useful run artifacts.
/// 确认断言失败会停止执行，同时仍输出有用的运行产物。
#[test]
fn failed_text_assertion_preserves_structured_artifacts() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let run_dir = temporary.path().join("assertion");
    let mut scenario = counter_scenario();
    let Step::AssertText(assertion) = &mut scenario.steps[0] else {
        panic!("first step must be an assertion");
    };
    assertion.value = "Count: wrong".to_owned();

    let result = run_mock_scenario(&scenario, &run_dir).expect("completed failed run");

    assert_eq!(result.status, Status::Failed);
    assert_eq!(result.steps.len(), 1);
    assert_eq!(result.errors[0].code.as_ref(), "UI_ASSERT_TEXT");
    assert_eq!(result.errors[0].step, Some(0));
    assert!(run_dir.join("result.json").exists());
    assert!(run_dir.join("reactive.json").exists());
}

#[test]
fn mount_identifier_is_metadata_not_a_core_registry_lookup() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let run_dir = temporary.path().join("explicit-mount");
    let mut scenario = counter_scenario();
    scenario.mount = "tests/explicit-mount".to_owned();

    let result = run_mock_scenario(&scenario, &run_dir).expect("explicit mount run");

    assert_eq!(result.status, Status::Passed);
    let trace = inspect_artifact(&run_dir, SnapshotKind::Trace).expect("trace inspection");
    assert_eq!(
        trace["events"][0]["details"]["component"],
        "tests/explicit-mount"
    );
}

/// Exposes the same run artifacts in memory for hosts without a filesystem.
/// 为无文件系统 Host 暴露同一运行产物的内存版本。
#[test]
fn collected_run_returns_artifacts_without_a_filesystem() {
    let scenario = counter_scenario();
    let host = HeadlessHost::new(MockBackend::new(), Environment::default());
    let run = fino::scenario::run_on_host_collect(
        &scenario,
        Path::new("artifacts/runs/counter-memory"),
        host,
        mount_counter,
    )
    .expect("collected run");

    assert_eq!(run.result.status, Status::Passed);
    assert_eq!(run.result.run_dir, "artifacts/runs/counter-memory");
    for artifact in [
        "dom.json",
        "a11y.json",
        "reactive.json",
        "trace.json",
        "captures/step-4/dom.json",
        "result.json",
    ] {
        assert!(run.artifacts.contains_key(artifact), "missing {artifact}");
    }
    let dom: Value =
        serde_json::from_slice(&run.artifacts["dom.json"]).expect("collected DOM JSON");
    assert!(
        dom["nodes"]
            .as_array()
            .expect("dom nodes")
            .iter()
            .any(|node| node["text"] == "1")
    );
}

#[test]
fn scenario_types_generate_a_versioned_json_schema() {
    let schema = schemars::schema_for!(Scenario);
    let schema = serde_json::to_value(schema).expect("schema JSON");
    assert!(schema["properties"]["schema"].is_object());
    assert!(schema["properties"]["steps"].is_object());
    assert!(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("scenarios/counter.json")
            .exists()
    );
}
