#![cfg(not(target_arch = "wasm32"))]

use std::sync::{Mutex, MutexGuard};

use fino::{
    headless::{Environment, HeadlessHost},
    metrics::RuntimeStats,
    native::{Size, WindowConstraints, WindowId, offscreen_backend},
    reactive::batch,
};
use fino_counter::{
    conformance::{ConformanceReport, expected_tree, run_conformance},
    mount_counter_with_count,
    runner::{run_mock_conformance, run_native_offscreen_conformance},
};

static TEST_LOCK: Mutex<()> = Mutex::new(());

/// Serializes tests that use `reactive_graph` on the process-global runtime.
/// 串行化使用进程级 `reactive_graph` Runtime 的测试。
fn lock() -> MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Asserts one cumulative checkpoint has the expected shared counts.
/// 断言单个累计检查点具有预期的共享计数。
fn assert_checkpoint(
    report: &ConformanceReport,
    name: &str,
    component: u64,
    effect: u64,
    set_text: u64,
    created: u64,
    removed: u64,
) {
    let stats = report
        .checkpoints
        .get(name)
        .unwrap_or_else(|| panic!("missing {name} checkpoint"));
    assert_eq!(stats.component_executions, component, "{name}");
    assert_eq!(stats.effect_executions, effect, "{name}");
    assert_eq!(stats.backend_set_text, set_text, "{name}");
    assert_eq!(stats.nodes_created, created, "{name}");
    assert_eq!(stats.nodes_removed, removed, "{name}");
}

/// Runs the same operation sequence on Mock and Native and compares shared semantics.
/// 在 Mock 与 Native 上运行同一操作序列，并比较共享语义。
#[test]
fn mock_and_native_conformance_share_trees_events_and_operation_counts() {
    let _guard = lock();
    let mock = run_mock_conformance().expect("mock conformance");
    let native = run_native_offscreen_conformance().expect("native conformance");

    assert_eq!(mock.schema, 1);
    assert_eq!(native.schema, 1);
    assert_eq!(mock.backend, "mock");
    assert_eq!(native.backend, "native-offscreen");
    assert_eq!(mock.tree, expected_tree());
    assert_eq!(native.tree, expected_tree());
    assert_eq!(mock.tree, native.tree);
    assert_eq!(mock.a11y, native.a11y);
    assert_eq!(mock.a11y.len(), 1);
    assert_eq!(mock.a11y[0].role, "button");
    assert_eq!(mock.a11y[0].name, "Increment counter");
    assert_eq!(mock.a11y[0].text, "Count: !102");
    assert_eq!(mock.event, native.event);
    assert_eq!(mock.event.text_after, "Count: !2");
    assert!(mock.event.handled);
    assert_eq!(mock.event.selector_matches, 1);
    assert_eq!(mock.operations, native.operations);
    assert_eq!(
        mock.operations,
        [
            ("create_element".to_owned(), 1),
            ("create_text".to_owned(), 3),
            ("insert".to_owned(), 4),
            ("listen".to_owned(), 1),
            ("remove".to_owned(), 1),
            ("set_attribute".to_owned(), 3),
            ("set_text".to_owned(), 4),
            ("unlisten".to_owned(), 1),
        ]
        .into_iter()
        .collect()
    );

    for report in [&mock, &native] {
        assert_checkpoint(report, "mounted", 1, 1, 1, 3, 0);
        assert_checkpoint(report, "text_changed", 1, 2, 2, 3, 0);
        assert_checkpoint(report, "attribute_changed", 1, 2, 2, 3, 0);
        assert_checkpoint(report, "inserted", 1, 2, 2, 4, 0);
        assert_checkpoint(report, "clicked", 1, 3, 3, 4, 0);
        assert_checkpoint(report, "batched", 1, 4, 4, 4, 0);
        assert_eq!(report.batch.increments, 100);
        assert_eq!(report.batch.final_text, "Count: !102");
        assert_eq!(report.batch.set_text_delta, 1);
        assert_eq!(report.batch.nodes_created_delta, 0);
        assert_eq!(report.batch.nodes_removed_delta, 0);
        assert_eq!(report.batch.frames_requested_delta, 0);
        assert_eq!(report.unmount.selector_matches, 0);
        assert_eq!(report.unmount.a11y_nodes, 0);
        assert_eq!(report.unmount.nodes_removed_delta, 1);
        assert!(report.final_tree.roots.is_empty());
        assert_eq!(report.final_stats.backend_set_text, 4);
        assert_eq!(report.final_stats.nodes_created, 4);
        assert_eq!(report.final_stats.nodes_removed, 1);
    }

    assert_eq!(mock.final_stats.frames_requested, 0);
    assert_eq!(native.final_stats.frames_requested, 1);
}

/// A single unbatched click commits one text update without structural work.
/// 一次未批处理点击只提交一次文本更新，不产生结构操作。
#[test]
fn one_unbatched_click_adds_one_text_commit() {
    let _guard = lock();
    let mock = run_mock_conformance().expect("mock conformance");
    let inserted = mock.checkpoints["inserted"];
    let clicked = mock.checkpoints["clicked"];
    let delta = clicked.delta_since(&inserted);
    assert_eq!(delta.backend_set_text, 1);
    assert_eq!(delta.effect_executions, 1);
    assert_eq!(delta.nodes_created, 0);
    assert_eq!(delta.nodes_removed, 0);
}

/// A 100-write batch submits only the final text and one coalesced frame request.
/// 100 次写入的批处理只提交最终文本和一个合并帧请求。
#[test]
fn batched_increments_commit_once_and_request_one_frame() {
    let _guard = lock();
    let backend = offscreen_backend(Size::new(800.0, 600.0), 1.0);
    let mut host = HeadlessHost::new(backend, Environment::default());
    let count = host.owner().signal_named("counter/count", 0_i32);
    let mount_count = count.clone();
    host.mount("counter", move |runtime, owner| {
        mount_counter_with_count(runtime, owner, mount_count)
    });
    host.runtime_mut()
        .backend_mut()
        .layout(WindowConstraints::new(
            WindowId::new(1),
            Size::new(800.0, 600.0),
        ));
    let _ = host.runtime().backend().take_frame_request();
    let before = host.runtime_stats();

    batch(|| {
        for _ in 0..100 {
            count.update(|number| *number += 1);
        }
    });

    let after = host.runtime_stats();
    let delta = after.delta_since(&before);
    assert_eq!(
        delta.backend_set_text, 1,
        "only the final text is committed"
    );
    assert_eq!(delta.nodes_created, 0);
    assert_eq!(delta.nodes_removed, 0);
    assert_eq!(
        delta.frames_requested, 1,
        "one coalesced frame request, not one per write"
    );
    let button = host
        .runtime()
        .backend()
        .find_all(&fino_counter::conformance::counter_selector());
    assert_eq!(button.len(), 1);
    assert_eq!(
        host.runtime().backend().text_content(button[0]).as_deref(),
        Some("Count: 100")
    );
}

/// Signal writes after disposal add no backend work and request no frame.
/// 释放后的 Signal 写入不增加后端工作，也不请求帧。
#[test]
fn writes_after_disposal_are_inert() {
    let _guard = lock();
    let backend = offscreen_backend(Size::new(800.0, 600.0), 1.0);
    let mut host = HeadlessHost::new(backend, Environment::default());
    let count = host.owner().signal_named("counter/count", 0_i32);
    let mount_count = count.clone();
    host.mount("counter", move |runtime, owner| {
        mount_counter_with_count(runtime, owner, mount_count)
    });
    host.dispose();
    let before: RuntimeStats = host.runtime_stats();

    count.update(|number| *number += 1);
    count.set(99);

    assert_eq!(host.runtime_stats(), before);
    assert_eq!(before.nodes_created, 3);
    assert_eq!(before.nodes_removed, 1);
}

/// Reuses the public mount helper so the conformance module stays example-owned.
/// 复用公开 mount 辅助函数，使一致性模块仍归示例所有。
#[test]
fn conformance_helper_runs_a_fresh_native_host() {
    let _guard = lock();
    let backend = offscreen_backend(Size::new(800.0, 600.0), 1.0);
    let host = HeadlessHost::new(backend, Environment::default());
    let report = run_conformance(host).expect("native conformance");
    assert_eq!(report.backend, "native-offscreen");
    assert_eq!(report.tree, expected_tree());
}
