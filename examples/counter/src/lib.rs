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

/// Mounts one button with static prefix text and one signal-bound text node.
/// 挂载一个按钮，包含静态前缀文本与一个 Signal 绑定的文本节点。
pub fn mount_counter<B: DomBackend + 'static>(runtime: &mut Runtime<B>, owner: &Owner) -> B::Node {
    mount_counter_with_state(runtime, owner).0
}

pub(crate) fn mount_counter_with_state<B: DomBackend + 'static>(
    runtime: &mut Runtime<B>,
    owner: &Owner,
) -> (B::Node, Signal<i32>) {
    runtime.component_executions().increment();

    let count = owner.signal_named("counter/count", 0_i32);
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
    let text_count = count.clone();
    runtime.effect(owner, "counter/text", move |cx| {
        let next = text_count.get().to_string();
        cx.backend_mut().set_text(&text_node, &next);
    });

    (button, count)
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        rc::Rc,
        sync::{Mutex, MutexGuard},
    };

    use fino::{
        backend_mock::{BackendOp, MockBackend, MockNode},
        dom_api::{Event, EventContext, EventKind, Role, Selector},
        headless::HeadlessBackend,
        reactive::{Owner, batch},
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
