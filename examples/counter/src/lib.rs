//! Shared manually constructed counter used by tests and backend-neutral scenarios.
//! 测试与后端无关场景共用的手工构建计数器。

use fino::{
    dom_api::{Attribute, DomBackend, EventKind, Tag},
    reactive::{Owner, Signal},
    runtime::Runtime,
};

/// Counter-owned smoke checks and scenario invocation.
/// 计数器示例拥有的冒烟检查与场景调用。
pub mod runner;

/// Native browser orchestration for the wasm counter.
/// wasm 计数器的原生浏览器编排。
#[cfg(not(target_arch = "wasm32"))]
pub mod browser;

/// Native winit/wgpu host for the same counter.
/// 同一计数器的原生 winit/wgpu Host。
#[cfg(not(target_arch = "wasm32"))]
pub mod native;

/// wasm browser entry points that mount the shared counter on `document.body`.
/// 将共享计数器挂载到 `document.body` 的 wasm 浏览器入口。
#[cfg(target_arch = "wasm32")]
pub mod web;

/// Mounts one button with static prefix text and one signal-bound text node.
/// 挂载一个按钮，包含静态前缀文本与一个 Signal 绑定的文本节点。
pub fn mount_counter<B: DomBackend + 'static>(runtime: &mut Runtime<B>, owner: &Owner) -> B::Node {
    mount_counter_with_state(runtime, owner).0
}

/// Mounts the counter against an application-owned shared count signal.
/// 使用应用持有的共享计数 Signal 挂载计数器。
pub fn mount_counter_with_count<B: DomBackend + 'static>(
    runtime: &mut Runtime<B>,
    owner: &Owner,
    count: Signal<i32>,
) -> B::Node {
    runtime.component_executions().increment();
    build_counter(runtime, owner, count)
}

pub(crate) fn mount_counter_with_state<B: DomBackend + 'static>(
    runtime: &mut Runtime<B>,
    owner: &Owner,
) -> (B::Node, Signal<i32>) {
    runtime.component_executions().increment();

    let count = owner.signal_named("counter/count", 0_i32);
    let button = build_counter(runtime, owner, count.clone());
    (button, count)
}

fn build_counter<B: DomBackend + 'static>(
    runtime: &mut Runtime<B>,
    owner: &Owner,
    count: Signal<i32>,
) -> B::Node {
    let button = runtime.backend_mut().create_element(Tag::Button);
    let prefix = runtime.backend_mut().create_text("Count: ");
    let value = runtime.backend_mut().create_text("");

    runtime
        .backend_mut()
        .set_attribute(&button, Attribute::AriaLabel, Some("Increment counter"));
    runtime.backend_mut().insert_before(&button, &prefix, None);
    runtime.backend_mut().insert_before(&button, &value, None);

    let click_count = count.clone();
    runtime.listen(owner, &button, EventKind::Click, move |_event, _cx| {
        click_count.update(|number| *number += 1);
    });

    let text_node = value.clone();
    let text_count = count;
    runtime.effect(owner, "counter/text", move |cx| {
        let next = text_count.get().to_string();
        cx.backend_mut().set_text(&text_node, &next);
    });

    button
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
        sync::{Mutex, MutexGuard},
    };

    use fino::{
        backend_mock::{BackendOp, MockBackend, MockNode},
        dom_api::{Event, EventContext, EventKind, Role, Selector},
        headless::HeadlessBackend,
        reactive::{Owner, batch, untracked},
        runtime::Runtime,
    };

    use super::mount_counter_with_state;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn lock() -> MutexGuard<'static, ()> {
        TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn click(runtime: &Runtime<MockBackend>, button: &MockNode) {
        let listener = runtime
            .backend()
            .listener(button, EventKind::Click)
            .expect("counter click listener");
        listener.call(
            Event {
                kind: EventKind::Click,
            },
            &mut EventContext,
        );
    }

    fn selector() -> Selector {
        Selector {
            role: Role::Button,
            name: "Increment counter".to_owned(),
        }
    }

    /// Derives the dynamic target from the public initial write instead of a fixed node ID.
    /// 从公共初始写入推导动态文本目标，避免依赖固定节点 ID。
    fn initial_text_target(operations: &[BackendOp]) -> MockNode {
        let targets: Vec<_> = operations
            .iter()
            .filter_map(|operation| match operation {
                BackendOp::SetText { node, value } if value == "0" => Some(*node),
                _ => None,
            })
            .collect();
        assert_eq!(targets.len(), 1, "one initial text binding target");
        targets[0]
    }

    /// Proves consecutive clicks mutate one existing text target without structural work.
    /// 验证连续点击只修改一个现有文本目标，不执行结构重建。
    #[test]
    fn clicks_update_only_the_bound_text_once() {
        let _guard = lock();
        let mut runtime = Runtime::new(MockBackend::new());
        let owner = runtime.create_owner();
        let (button, _count) = mount_counter_with_state(&mut runtime, &owner);

        assert_eq!(runtime.component_executions().get(), 1);
        assert_eq!(
            runtime.backend().text_content(&button).as_deref(),
            Some("Count: 0")
        );
        let text_target = initial_text_target(runtime.backend().operations());
        let initial_operations = runtime.backend().operations().len();

        click(&runtime, &button);
        assert_eq!(
            runtime.backend().text_content(&button).as_deref(),
            Some("Count: 1")
        );
        click(&runtime, &button);
        assert_eq!(
            runtime.backend().text_content(&button).as_deref(),
            Some("Count: 2")
        );
        assert_eq!(runtime.component_executions().get(), 1);

        let backend = runtime.backend();
        let operations = backend.operations();
        let updates: Vec<_> = operations[initial_operations..]
            .iter()
            .filter_map(|operation| match operation {
                BackendOp::SetText { node, value } => Some((*node, value.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(updates, [(text_target, "1"), (text_target, "2")]);
        assert!(
            operations[initial_operations..]
                .iter()
                .all(|operation| !matches!(
                    operation,
                    BackendOp::CreateElement(_) | BackendOp::CreateText | BackendOp::Insert
                ))
        );
        assert_eq!(owner.snapshot().effects[0].executions, 3);
        assert_eq!(runtime.backend().find_all(&selector()).len(), 1);
    }

    /// Verifies that a 100-write transaction produces one final backend text mutation.
    /// 验证包含 100 次写入的事务只产生一次最终文本变更。
    #[test]
    fn one_hundred_batched_increments_commit_only_the_final_text() {
        let _guard = lock();
        let mut runtime = Runtime::new(MockBackend::new());
        let owner = runtime.create_owner();
        let (button, count) = mount_counter_with_state(&mut runtime, &owner);
        let text_target = initial_text_target(runtime.backend().operations());
        let initial_operations = runtime.backend().operations().len();

        batch(|| {
            for _ in 0..100 {
                count.update(|number| *number += 1);
            }
        });

        assert_eq!(
            runtime.backend().text_content(&button).as_deref(),
            Some("Count: 100")
        );
        let backend = runtime.backend();
        let updates: Vec<_> = backend.operations()[initial_operations..]
            .iter()
            .filter_map(|operation| match operation {
                BackendOp::SetText { node, value } => Some((*node, value.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(updates, [(text_target, "100")]);
        assert_eq!(owner.snapshot().effects[0].executions, 2);
        assert_eq!(runtime.component_executions().get(), 1);
    }

    /// Checks idempotent cleanup, listener release, and no post-disposal backend writes.
    /// 检查幂等清理、监听释放以及释放后不再写入后端。
    #[test]
    fn disposal_is_idempotent_and_blocks_backend_access() {
        let _guard = lock();
        let mut runtime = Runtime::new(MockBackend::new());
        let owner = runtime.create_owner();
        let (button, count) = mount_counter_with_state(&mut runtime, &owner);

        owner.dispose();
        owner.dispose();

        assert_eq!(runtime.backend().listener_count(), 0);
        assert!(
            runtime
                .backend()
                .listener(&button, EventKind::Click)
                .is_none()
        );
        assert!(owner.snapshot().disposed);
        assert!(!owner.snapshot().effects[0].active);
        let operations = runtime.backend().operations().len();
        count.update(|number| *number += 1);
        assert_eq!(runtime.backend().operations().len(), operations);
        assert_eq!(
            runtime.backend().text_content(&button).as_deref(),
            Some("Count: 0")
        );
    }

    /// Separates causes when one batch writes two signals with distinct dependents.
    /// 在同一批次写入两个 Signal 时，按各自依赖区分 Effect 原因。
    #[test]
    fn effect_reasons_respect_each_dependency_in_the_same_batch() {
        let _guard = lock();
        let owner = Owner::new();
        let first = owner.signal_named("test/first-cause", 0_i32);
        let second = owner.signal_named("test/second-cause", 0_i32);
        let first_observed = first.clone();
        owner.effect_named("test/first-dependent", move || {
            first_observed.get();
        });
        let second_observed = second.clone();
        owner.effect_named("test/second-dependent", move || {
            second_observed.get();
        });

        batch(|| {
            first.set(1);
            second.set(1);
        });

        let snapshot = owner.snapshot();
        assert_eq!(snapshot.effects[0].last_reason, "signal-1 write");
        assert_eq!(snapshot.effects[1].last_reason, "signal-2 write");
        owner.dispose();
    }

    /// Ensures unrelated writes and passive snapshots cannot rewrite an effect's last cause.
    /// 确保无关写入与被动快照不会改写 Effect 最近原因。
    #[test]
    fn effect_reason_does_not_drift_without_a_new_dependency_write() {
        let _guard = lock();
        let owner = Owner::new();
        let dependency = owner.signal_named("test/dependency", 0_i32);
        let unrelated = owner.signal_named("test/unrelated", 0_i32);
        let observed = dependency.clone();
        owner.effect_named("test/dependent-only", move || {
            observed.get();
        });
        dependency.set(1);
        let after_dependency_write = owner.snapshot();
        assert_eq!(
            after_dependency_write.effects[0].last_reason,
            "signal-1 write"
        );
        assert_eq!(after_dependency_write.effects[0].executions, 2);

        unrelated.set(1);
        let after_unrelated_write = owner.snapshot();
        assert_eq!(after_unrelated_write.effects[0].executions, 2);
        assert_eq!(
            after_unrelated_write.effects[0].last_reason,
            "signal-1 write"
        );
        assert_eq!(after_unrelated_write, owner.snapshot());
        owner.dispose();
    }

    /// Aligns diagnostic dependencies with graph behavior for untracked reads.
    /// 使 untracked 读取的诊断依赖与真实图订阅行为保持一致。
    #[test]
    fn untracked_reads_leave_diagnostic_dependencies_empty() {
        let _guard = lock();
        let owner = Owner::new();
        let signal = owner.signal_named("test/untracked", 0_i32);
        let observed = signal.clone();
        let runs = Rc::new(Cell::new(0));
        let effect_runs = Rc::clone(&runs);
        owner.effect_named("test/untracked-read", move || {
            untracked(|| observed.get());
            effect_runs.set(effect_runs.get() + 1);
        });

        assert!(owner.snapshot().effects[0].dependencies.is_empty());
        signal.set(1);
        assert_eq!(runs.get(), 1);
        assert_eq!(owner.snapshot().effects[0].executions, 1);
        owner.dispose();
    }

    /// Verifies guard-based restoration within the same effect run after unwinding.
    /// 验证 panic 展开后 guard 在同一次 Effect 运行内恢复状态。
    #[test]
    fn untracked_panic_restores_diagnostic_tracking_state() {
        let _guard = lock();
        let owner = Owner::new();
        let inside = owner.signal_named("test/panic-inside", 0_i32);
        let outside = owner.signal_named("test/panic-outside", 0_i32);
        let observed_inside = inside.clone();
        let observed_outside = outside.clone();
        owner.effect_named("test/panicking-untracked", move || {
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                untracked(|| {
                    observed_inside.get();
                    panic!("untracked restoration regression");
                });
            }));
            assert!(outcome.is_err());

            // Still inside the effect run, so this read proves `untracked` restored tracking.
            // 仍处于该 Effect 运行内，因此这次读取能证明 `untracked` 已恢复追踪。
            observed_outside.get();
        });

        // `outside` is the second signal, so the effect depends on it and not on `inside`.
        // `outside` 是第二个 Signal，因此该 Effect 只依赖它而不依赖 `inside`。
        let effects = owner.snapshot().effects;
        assert_eq!(effects[0].dependencies, vec!["signal-2".to_owned()]);
        owner.dispose();
    }

    /// Ensures every dependent effect records the shared signal's write reason.
    /// 确保每个依赖 Effect 都记录共享 Signal 的写入原因。
    #[test]
    fn every_effect_keeps_the_latest_batch_reason() {
        let _guard = lock();
        let owner = Owner::new();
        let signal = owner.signal_named("test/shared-reason", 0_i32);
        let first_signal = signal.clone();
        owner.effect_named("test/first-reason", move || {
            first_signal.get();
        });
        let second_signal = signal.clone();
        owner.effect_named("test/second-reason", move || {
            second_signal.get();
        });

        batch(|| {
            for value in 1..=100 {
                signal.set(value);
            }
        });

        let snapshot = owner.snapshot();
        assert_eq!(snapshot.effects.len(), 2);
        for effect in snapshot.effects {
            assert_eq!(effect.executions, 2);
            assert_eq!(effect.last_reason, "signal-1 write");
        }
        owner.dispose();
    }

    /// Covers batch de-duplication and structured rejection of recursive effect entry.
    /// 覆盖批内 Effect 去重与递归重入的结构化报告。
    #[test]
    fn effects_are_deduplicated_and_cycles_are_reported() {
        let _guard = lock();
        let owner = Owner::new();
        let signal = owner.signal_named("test/value", 0_i32);
        let runs = Rc::new(Cell::new(0));
        let effect_runs = Rc::clone(&runs);
        let effect_signal = signal.clone();
        owner.effect_named("test/batched", move || {
            effect_signal.get();
            effect_runs.set(effect_runs.get() + 1);
        });
        batch(|| {
            for value in 1..=100 {
                signal.set(value);
            }
        });
        assert_eq!(runs.get(), 2);
        assert_eq!(owner.snapshot().effects[0].dependencies, ["signal-1"]);
        owner.dispose();

        let cyclic_owner = Owner::new();
        let cyclic = cyclic_owner.signal_named("test/cycle", 0_i32);
        let effect_cyclic = cyclic.clone();
        cyclic_owner.effect_named("test/cycle-effect", move || {
            let value = effect_cyclic.get();
            if value == 0 {
                effect_cyclic.set(1);
            }
        });
        let snapshot = cyclic_owner.snapshot();
        assert_eq!(snapshot.effects[0].executions, 1);
        assert_eq!(snapshot.errors.len(), 1);
        assert_eq!(snapshot.errors[0].code, "UI_REACTIVE_CYCLE");
        assert_eq!(snapshot.errors[0].effect.as_deref(), Some("effect-1"));
        cyclic_owner.dispose();
    }
}
