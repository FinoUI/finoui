//! Acceptance tests for the counter-stage `view!` authoring surface.
//! 计数器阶段 `view!` 作者接口的验收测试。

use std::sync::{Mutex, MutexGuard};

use fino::{
    backend_mock::{BackendOp, MockBackend, MockNode},
    dom_api::{DomBackend, Event, EventContext, EventKind, Role, Selector},
    headless::HeadlessBackend,
    reactive::{Owner, batch},
    runtime::Runtime,
    view::{View, signal},
};

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn counter() -> View<MockBackend> {
    View::<MockBackend>::new(|runtime, owner| {
        let count = signal(0);
        fino::view! {
            <button
                aria-label="Increment counter"
                on:click={move || count.update(|value| *value += 1)}
            >
                "Count: " {move || count.get()}
            </button>
        }
    })
}

fn manual_counter(runtime: &mut Runtime<MockBackend>, owner: &Owner) -> MockNode {
    runtime.component_executions().increment();
    let count = owner.signal_named("counter/count", 0_i32);
    let button = runtime
        .backend_mut()
        .create_element(fino::dom_api::Tag::Button);
    let prefix = runtime.backend_mut().create_text("Count: ");
    let value = runtime.backend_mut().create_text("");
    runtime.backend_mut().set_attribute(
        &button,
        fino::dom_api::Attribute::AriaLabel,
        Some("Increment counter"),
    );
    runtime.backend_mut().insert_before(&button, &prefix, None);
    runtime.backend_mut().insert_before(&button, &value, None);

    let click_count = count.clone();
    runtime.listen(owner, &button, EventKind::Click, move |_event, _context| {
        click_count.update(|number| *number += 1);
    });
    let text_node = value;
    runtime.effect(owner, "counter/text", move |context| {
        context
            .backend_mut()
            .set_text(&text_node, &count.get().to_string());
    });
    button
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

/// The macro-built component runs once and updates one existing text node per click.
/// 宏构建的组件只执行一次，每次点击只更新一个现有文本节点。
#[test]
fn view_counter_matches_manual_identity_and_component_execution() {
    let _guard = lock();
    let mut runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let button = counter().mount(&mut runtime, &owner);

    assert_eq!(runtime.component_executions().get(), 1);
    assert_eq!(owner.snapshot().effects[0].name, "view/text/2");
    assert_eq!(
        runtime.backend().text_content(&button).as_deref(),
        Some("Count: 0")
    );
    let initial_operations = runtime.backend().operations().len();
    let ids = runtime
        .backend()
        .dom_snapshot()
        .nodes
        .into_iter()
        .map(|node| node.id)
        .collect::<Vec<_>>();
    assert_eq!(ids, ["node-1", "node-2", "node-3"]);

    click(&runtime, &button);
    click(&runtime, &button);

    assert_eq!(
        runtime.backend().text_content(&button).as_deref(),
        Some("Count: 2")
    );
    assert_eq!(runtime.component_executions().get(), 1);
    let after = runtime.backend().dom_snapshot();
    let after_ids = after
        .nodes
        .into_iter()
        .map(|node| node.id)
        .collect::<Vec<_>>();
    assert_eq!(after_ids, ids, "node identity stays stable");
    let backend = runtime.backend();
    let operations = backend.operations();
    assert!(
        operations[initial_operations..]
            .iter()
            .all(|operation| !matches!(
                operation,
                BackendOp::CreateElement(_) | BackendOp::CreateText | BackendOp::Insert
            ))
    );
    assert_eq!(runtime.backend().find_all(&selector()).len(), 1);
}

/// A batch still commits only the final bound text value.
/// 批处理仍只提交最终绑定的文本值。
#[test]
fn view_counter_batches_one_text_commit() {
    let _guard = lock();
    let mut runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let button = counter().mount(&mut runtime, &owner);
    let initial_operations = runtime.backend().operations().len();

    batch(|| {
        click(&runtime, &button);
        for _ in 0..3 {
            click(&runtime, &button);
        }
    });

    assert_eq!(
        runtime.backend().text_content(&button).as_deref(),
        Some("Count: 4")
    );
    assert_eq!(
        runtime.backend().operations()[initial_operations..]
            .iter()
            .filter(|operation| matches!(operation, BackendOp::SetText { .. }))
            .count(),
        1
    );
}

/// Disposal releases listeners and keeps later writes out of the backend.
/// 释放监听并阻止释放后的写入进入后端。
#[test]
fn view_counter_disposal_is_idempotent() {
    let _guard = lock();
    let mut runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let count = owner.signal_named("counter/count", 0_i32);
    let mount_count = count.clone();
    let view = View::<MockBackend>::new(move |runtime, owner| {
        let _owner = owner;
        fino::view! {
            <button
                aria-label="Increment counter"
                on:click={move || mount_count.update(|value| *value += 1)}
            >
                "Count: " {move || mount_count.get()}
            </button>
        }
    });
    let button = view.mount(&mut runtime, &owner);

    owner.dispose();
    owner.dispose();

    assert_eq!(runtime.backend().listener_count(), 0);
    assert!(
        runtime
            .backend()
            .listener(&button, EventKind::Click)
            .is_none()
    );
    let operations = runtime.backend().operations().len();
    count.update(|value| *value += 1);
    assert_eq!(runtime.backend().operations().len(), operations);
}

/// The macro-built component matches the previous hand-built counter.
/// 宏构建的组件与此前手工构建的计数器一致。
#[test]
fn view_counter_matches_manual_construction() {
    let _guard = lock();
    let mut macro_runtime = Runtime::new(MockBackend::new());
    let macro_owner = macro_runtime.create_owner();
    let macro_button = counter().mount(&mut macro_runtime, &macro_owner);

    let mut manual_runtime = Runtime::new(MockBackend::new());
    let manual_owner = manual_runtime.create_owner();
    let manual_button = manual_counter(&mut manual_runtime, &manual_owner);

    assert_eq!(
        macro_runtime.backend().dom_snapshot().nodes,
        manual_runtime.backend().dom_snapshot().nodes
    );
    assert_eq!(
        macro_runtime.backend().text_content(&macro_button),
        manual_runtime.backend().text_content(&manual_button)
    );
    assert_eq!(
        macro_runtime.component_executions().get(),
        manual_runtime.component_executions().get()
    );

    click(&macro_runtime, &macro_button);
    click(&manual_runtime, &manual_button);
    click(&macro_runtime, &macro_button);
    click(&manual_runtime, &manual_button);

    assert_eq!(
        macro_runtime.backend().text_content(&macro_button),
        manual_runtime.backend().text_content(&manual_button)
    );
    assert_eq!(
        macro_runtime.backend().operations().len(),
        manual_runtime.backend().operations().len()
    );
    assert_eq!(
        macro_runtime
            .backend()
            .operations()
            .iter()
            .filter(|operation| matches!(operation, BackendOp::SetText { .. }))
            .count(),
        manual_runtime
            .backend()
            .operations()
            .iter()
            .filter(|operation| matches!(operation, BackendOp::SetText { .. }))
            .count()
    );
}

/// Generated locals keep macro hygiene and do not shadow caller bindings.
/// 生成的局部变量保持宏卫生，不遮蔽调用者的同名绑定。
#[test]
fn generated_locals_do_not_shadow_caller_bindings() {
    let _guard = lock();
    let mut runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let count = owner.signal_named("test/hygiene", 0_i32);
    let click_count = count.clone();
    let __fino_template = 40_i32;
    let __fino_nodes = 2_i32;
    let __fino_node_0 = 1_i32;
    let view = View::<MockBackend>::new(move |runtime, owner| {
        let _owner = owner;
        fino::view! {
            <button
                fino:name="hygiene"
                on:click={move || click_count.update(|value| *value += __fino_nodes)}
            >
                "Total: " {move || __fino_template + __fino_node_0 + click_count.get()}
            </button>
        }
    });
    let button = view.mount(&mut runtime, &owner);

    assert_eq!(owner.snapshot().effects[0].name, "hygiene/text");
    assert_eq!(
        runtime.backend().text_content(&button).as_deref(),
        Some("Total: 41")
    );
    click(&runtime, &button);
    assert_eq!(
        runtime.backend().text_content(&button).as_deref(),
        Some("Total: 43")
    );
}

/// A let initializer captures the outer signal before its own binding starts.
/// let 初始化表达式在自身绑定生效前捕获外层 Signal。
// The deliberate shadowing exercises capture analysis; clippy would simplify it away.
// 这里的刻意遮蔽用于验证捕获分析，clippy 的建议会移除测试目标。
#[allow(clippy::let_and_return)]
#[test]
fn let_initializer_captures_outer_signal_before_shadowing() {
    let _guard = lock();
    let mut runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let count = owner.signal_named("test/let-shadow", 0_i32);
    let click_count = count.clone();
    let view = View::<MockBackend>::new(move |runtime, owner| {
        let _owner = owner;
        fino::view! {
            <button on:click={move || click_count.update(|value| *value += 1)}>
                "Value: " {move || {
                    let count = click_count.get();
                    count
                }}
            </button>
        }
    });
    let button = view.mount(&mut runtime, &owner);

    assert_eq!(
        runtime.backend().text_content(&button).as_deref(),
        Some("Value: 0")
    );
    click(&runtime, &button);
    assert_eq!(
        runtime.backend().text_content(&button).as_deref(),
        Some("Value: 1")
    );
}

/// `format!` arguments participate in capture sharing with event closures.
/// `format!` 参数与事件闭包共享捕获集合。
#[test]
fn format_macro_arguments_are_captured() {
    let _guard = lock();
    let mut runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let count = owner.signal_named("test/format", 0_i32);
    let click_count = count.clone();
    let view = View::<MockBackend>::new(move |runtime, owner| {
        let _owner = owner;
        fino::view! {
            <button on:click={move || click_count.update(|value| *value += 1)}>
                "Count: " {move || format!("{}", click_count.get())}
            </button>
        }
    });
    let button = view.mount(&mut runtime, &owner);

    assert_eq!(
        runtime.backend().text_content(&button).as_deref(),
        Some("Count: 0")
    );
    click(&runtime, &button);
    assert_eq!(
        runtime.backend().text_content(&button).as_deref(),
        Some("Count: 1")
    );
}

/// Dynamic attributes lower to an owner-scoped AttributeBinding.
/// 动态属性下沉为 Owner 作用域内的 AttributeBinding。
#[test]
fn view_dynamic_attribute_updates_the_existing_element() {
    let _guard = lock();
    let mut runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let label = owner.signal_named("test/label", "First");
    let build_label = label.clone();
    let view = View::<MockBackend>::new(move |runtime, owner| {
        let _owner = owner;
        fino::view! {
            <button aria-label={move || build_label.get()}>
                "Label"
            </button>
        }
    });
    let button = view.mount(&mut runtime, &owner);

    let attribute = |runtime: &Runtime<MockBackend>| {
        runtime
            .backend()
            .dom_snapshot()
            .nodes
            .into_iter()
            .find(|node| node.id == format!("node-{}", button.0))
            .and_then(|node| node.attributes.get("aria-label").cloned())
    };
    assert_eq!(attribute(&runtime), Some("First".to_owned()));
    label.set("Second");
    assert_eq!(attribute(&runtime), Some("Second".to_owned()));
    assert_eq!(runtime.component_executions().get(), 1);
}
