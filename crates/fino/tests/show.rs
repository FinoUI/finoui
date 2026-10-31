//! Range anchor and conditional Show lifecycle regressions.
//! 范围锚点与条件 Show 生命周期回归。

use std::{cell::RefCell, rc::Rc};

use fino::{
    backend_mock::{BackendOp, MockBackend, MockNode},
    dom_api::{Attribute, DomBackend, EventKind, Tag},
    headless::HeadlessBackend,
    runtime::Runtime,
    show::{Show, ShowContext},
};

fn text(backend: &mut MockBackend, parent: &MockNode, value: &str) -> MockNode {
    let node = backend.create_text(value);
    backend.insert_before(parent, &node, None);
    node
}

fn action_button(
    context: &mut ShowContext<MockBackend>,
    count: fino::reactive::Signal<i32>,
) -> MockNode {
    let button = context.create_element(Tag::Button);
    context.set_attribute(&button, Attribute::AriaLabel, Some("Show action"));
    let value = context.create_text("");
    context.insert_before(&button, &value, None);
    let read_count = count.clone();
    context.effect("show/action/text", move |effect| {
        effect
            .backend_mut()
            .set_text(&value, &format!("Value: {}", read_count.get()));
    });
    context.listen(&button, EventKind::Click, move |_event, _context| {
        count.update(|number| *number += 1);
    });
    button
}

fn has_action(backend: &MockBackend) -> bool {
    backend
        .find_all(&fino::dom_api::Selector {
            role: fino::dom_api::Role::Button,
            name: "Show action".to_owned(),
        })
        .len()
        == 1
}

/// Proves only real transitions create or remove the dynamic range while static
/// siblings and their node handles stay stable.
/// 验证只有真实切换才创建或移除动态范围，静态兄弟与其节点句柄保持稳定。
#[test]
fn show_only_rebuilds_its_dynamic_range() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let visible = owner.signal_named("show/visible", false);
    let count = owner.signal_named("show/count", 0);

    let root = runtime.backend_mut().create_element(Tag::Button);
    runtime
        .backend_mut()
        .set_attribute(&root, Attribute::AriaLabel, Some("Show host"));
    let _prefix = text(&mut runtime.backend_mut(), &root, "Before");
    let tail = text(&mut runtime.backend_mut(), &root, "After");

    let show = {
        let visible = visible.clone();
        let count = count.clone();
        Show::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "show/details",
            move || visible.get(),
            move |context| action_button(context, count.clone()),
        )
    };

    let initial_ids = runtime.backend().dom_snapshot().nodes.len();
    let prefix_id = runtime.backend().dom_snapshot().nodes[1].id.clone();
    let tail_id = runtime.backend().dom_snapshot().nodes[2].id.clone();
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("BeforeAfter")
    );
    assert!(!show.is_mounted());
    assert!(!has_action(&runtime.backend()));
    assert_eq!(runtime.backend().listener_count(), 0);
    let initial_effects = owner.snapshot().effects.len();
    assert_eq!(initial_effects, 1, "only the condition effect exists");

    visible.set(true);
    assert!(show.is_mounted());
    assert!(has_action(&runtime.backend()));
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("BeforeValue: 0After")
    );
    assert_eq!(runtime.backend().listener_count(), 1);
    assert_eq!(owner.snapshot().effects.len(), 2);
    let after_show = runtime.backend().dom_snapshot();
    let ids: Vec<_> = after_show
        .nodes
        .iter()
        .map(|node| node.id.clone())
        .collect();
    assert_eq!(ids[1], prefix_id, "prefix node id stays stable");
    assert_eq!(ids[2], tail_id, "tail node id stays stable");

    let before_update = runtime.backend().operations().len();
    count.set(5);
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("BeforeValue: 5After")
    );
    let backend = runtime.backend();
    let updates: Vec<String> = backend.operations()[before_update..]
        .iter()
        .filter_map(|operation| match operation {
            BackendOp::SetText { value, .. } => Some(value.clone()),
            _ => None,
        })
        .collect();
    drop(backend);
    assert_eq!(updates, ["Value: 5"], "one text node is updated");
    assert_eq!(runtime.component_executions().get(), 0);

    let before_hide = runtime.backend().operations().len();
    visible.set(false);
    assert!(!show.is_mounted());
    assert!(!has_action(&runtime.backend()));
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("BeforeAfter")
    );
    assert_eq!(runtime.backend().listener_count(), 0);
    let hidden_snapshot = runtime.backend().dom_snapshot();
    assert_eq!(
        hidden_snapshot.roots,
        vec!["node-1".to_owned()],
        "released Show content does not become a live snapshot root"
    );
    assert!(
        hidden_snapshot
            .nodes
            .iter()
            .all(|node| node.attributes.get("aria-label") != Some(&"Show action".to_owned())),
        "released Show content leaves the stored snapshot"
    );
    assert_eq!(owner.snapshot().effects.len(), 2);
    assert!(
        !owner.snapshot().effects[1].active,
        "the removed child effect is inactive"
    );
    let hide_ops = runtime.backend().operations()[before_hide..].to_vec();
    let unlisten = hide_ops
        .iter()
        .position(|operation| matches!(operation, BackendOp::Unlisten(EventKind::Click)))
        .expect("listener is released");
    let remove = hide_ops
        .iter()
        .position(|operation| matches!(operation, BackendOp::Release))
        .expect("content is removed");
    assert!(
        unlisten < remove,
        "listeners must stop before backend nodes are detached"
    );
    assert!(
        !hide_ops
            .iter()
            .any(|operation| matches!(operation, BackendOp::Release))
            || remove > unlisten
    );

    let after_hide = runtime.backend().operations().len();
    count.set(9);
    assert_eq!(
        runtime.backend().operations().len(),
        after_hide,
        "writing a removed dependency must not touch the backend"
    );

    visible.set(true);
    assert!(show.is_mounted());
    assert!(has_action(&runtime.backend()));
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("BeforeValue: 9After")
    );
    assert!(
        has_action(&runtime.backend()),
        "remounting reaches one action"
    );
    assert_eq!(runtime.backend().listener_count(), 1);
    assert_eq!(
        runtime
            .backend()
            .dom_snapshot()
            .nodes
            .iter()
            .filter(|node| node.kind == fino::headless::DomNodeKind::Anchor)
            .count(),
        2,
        "the anchor pair is created once"
    );

    let remounted_ops = runtime.backend().operations().len();
    visible.set(true);
    assert_eq!(
        runtime.backend().listener_count(),
        1,
        "an equal condition write is a no-op"
    );
    assert_eq!(
        runtime.backend().operations().len(),
        remounted_ops,
        "an equal condition write never touches the backend"
    );
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        initial_ids + 2,
        "released range storage does not accumulate across toggles"
    );
}

/// Proves nested Show owners dispose recursively, stale conditions stop updating,
/// and repeated disposal stays idempotent.
/// 验证嵌套 Show Owner 递归释放，旧条件不再更新，重复释放保持幂等。
#[test]
fn nested_show_owners_dispose_idempotently() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let outer_visible = owner.signal_named("show/outer", false);
    let inner_visible = owner.signal_named("show/inner", false);

    let root = runtime.backend_mut().create_element(Tag::Button);
    runtime
        .backend_mut()
        .set_attribute(&root, Attribute::AriaLabel, Some("Nested host"));
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let inner_slot: Rc<RefCell<Option<Show<MockBackend>>>> = Rc::new(RefCell::new(None));

    let outer = {
        let outer_visible = outer_visible.clone();
        let inner_visible = inner_visible.clone();
        let inner_slot = Rc::clone(&inner_slot);
        Show::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "show/outer-range",
            move || outer_visible.get(),
            move |context| {
                let panel = context.create_element(Tag::Button);
                context.set_attribute(&panel, Attribute::AriaLabel, Some("Outer panel"));
                let label = context.create_text("Outer");
                context.insert_before(&panel, &label, None);
                let inner_visible = inner_visible.clone();
                let slot = Rc::clone(&inner_slot);
                let inner = context.show(
                    &panel,
                    None,
                    "show/inner-range",
                    move || inner_visible.get(),
                    |context| context.create_text("Inner"),
                );
                *slot.borrow_mut() = Some(inner);
                panel
            },
        )
    };

    outer_visible.set(true);
    assert!(outer.is_mounted());
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("OuterTail")
    );

    inner_visible.set(true);
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("OuterInnerTail")
    );
    let inner_effects = owner
        .snapshot()
        .effects
        .iter()
        .filter(|effect| effect.name.starts_with("show/inner-range"))
        .count();
    assert_eq!(inner_effects, 1);

    outer_visible.set(false);
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("Tail")
    );
    let effects_after_outer_off = owner.snapshot().effects;
    assert!(
        effects_after_outer_off
            .iter()
            .filter(|effect| effect.name.starts_with("show/"))
            .all(|effect| effect.name == "show/outer-range/condition" || !effect.active),
        "{effects_after_outer_off:?}"
    );

    let backend_ops = runtime.backend().operations().len();
    inner_visible.set(true);
    inner_visible.set(false);
    assert_eq!(
        runtime.backend().operations().len(),
        backend_ops,
        "inner condition writes cannot reach a disposed nested owner"
    );

    outer.dispose();
    outer.dispose();
    owner.dispose();
    owner.dispose();
    assert_eq!(runtime.backend().listener_count(), 0);
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("Tail"),
        "the static sibling survives nested cleanup"
    );
}

/// Proves a multi-node range is inserted and removed as one anchored unit.
/// 验证多节点范围作为一个锚点单元插入与移除。
#[test]
fn show_range_removes_every_node_between_anchors() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let visible = owner.signal_named("show/visible", false);
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let show = {
        let visible = visible.clone();
        Show::mount_many(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "show/many",
            move || visible.get(),
            |context| {
                let one = context.create_text("one");
                let two = context.create_text("two");
                vec![one, two]
            },
        )
    };

    visible.set(true);
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("onetwoTail")
    );
    visible.set(false);
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("Tail")
    );
    assert!(!show.is_mounted());
}

/// Proves a condition effect drops dependencies it no longer reads.
/// 验证条件 Effect 会丢弃不再读取的依赖。
#[test]
fn condition_effect_drops_stale_dependencies() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let choose_primary = owner.signal_named("show/choose-primary", false);
    let primary = owner.signal_named("show/primary", true);
    let fallback = owner.signal_named("show/fallback", false);

    let root = runtime.backend_mut().create_element(Tag::Button);
    let show = {
        let choose_primary = choose_primary.clone();
        let primary = primary.clone();
        let fallback = fallback.clone();
        Show::mount(
            &runtime,
            &owner,
            &root,
            None,
            "show/dependencies",
            move || {
                if choose_primary.get() {
                    primary.get()
                } else {
                    fallback.get()
                }
            },
            |context| context.create_text("visible"),
        )
    };

    assert!(!show.is_mounted());
    assert_eq!(owner.snapshot().effects[0].executions, 1);
    assert_eq!(
        owner.snapshot().effects[0].dependencies,
        ["signal-1", "signal-3"],
        "the false branch depends on the selector and fallback"
    );

    choose_primary.set(true);
    assert!(show.is_mounted());
    assert_eq!(owner.snapshot().effects[0].executions, 2);
    assert_eq!(
        owner.snapshot().effects[0].dependencies,
        ["signal-1", "signal-2"],
        "the true branch drops the old fallback dependency"
    );

    fallback.set(true);
    assert_eq!(
        owner.snapshot().effects[0].executions,
        2,
        "the old branch no longer triggers the condition effect"
    );
}

/// Proves the parent owner, not the returned handle, owns the dynamic range.
/// 验证动态范围由父 Owner 而非返回句柄持有。
#[test]
fn dropping_the_show_handle_keeps_the_parent_owned_range() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let visible = owner.signal_named("show/visible", false);
    let root = runtime.backend_mut().create_element(Tag::Button);

    {
        let visible = visible.clone();
        Show::mount(
            &runtime,
            &owner,
            &root,
            None,
            "show/handle",
            move || visible.get(),
            |context| context.create_text("visible"),
        );
    }

    visible.set(true);
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("visible")
    );
    owner.dispose();
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some(""),
        "parent disposal removes the range and anchors"
    );
}

/// Proves a panic in the content builder leaves no child effect, listener, or node,
/// and a later real transition retries the mount.
/// 验证内容构建器 panic 后不残留子 Effect、监听或节点，并且后续真实切换可重试挂载。
#[test]
fn failed_content_mount_cleans_up_and_can_retry() {
    use std::{
        cell::Cell,
        panic::{AssertUnwindSafe, catch_unwind},
    };

    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let visible = owner.signal_named("show/visible", false);
    let panic_once = Rc::new(Cell::new(true));
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");

    let show = {
        let visible = visible.clone();
        let panic_once = Rc::clone(&panic_once);
        Show::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "show/panic",
            move || visible.get(),
            move |context| {
                let button = context.create_element(Tag::Button);
                context.set_attribute(&button, Attribute::AriaLabel, Some("Panic action"));
                let value = context.create_text("value");
                context.insert_before(&button, &value, None);
                let observed = context.owner().signal_named("show/panic/observed", 0);
                context.effect("show/panic/effect", move |effect| {
                    let _ = observed.get();
                    let _ = effect.backend_mut();
                });
                context.listen(&button, EventKind::Click, |_event, _context| {});
                if panic_once.replace(false) {
                    panic!("content builder failed");
                }
                button
            },
        )
    };

    let nodes_before_failure = runtime.backend().dom_snapshot().nodes.len();
    let operations_before_failure = runtime.backend().operations().len();
    let failure = catch_unwind(AssertUnwindSafe(|| visible.set(true)));
    assert!(failure.is_err(), "the builder panic must propagate");
    assert!(!show.is_mounted());
    assert_eq!(runtime.backend().listener_count(), 0);
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        nodes_before_failure,
        "tracked nodes created before the panic are released"
    );
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("Tail")
    );
    assert!(
        owner
            .snapshot()
            .effects
            .iter()
            .all(|effect| effect.name != "show/panic/effect" || !effect.active),
        "the failed child effect is inactive"
    );
    assert!(
        runtime.backend().operations()[operations_before_failure..]
            .iter()
            .any(|operation| matches!(operation, BackendOp::Release)),
        "the failed transaction releases its created nodes"
    );

    visible.set(false);
    visible.set(true);
    assert!(show.is_mounted());
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("valueTail")
    );
    assert_eq!(runtime.backend().listener_count(), 1);
}

/// Proves Mock anchors are never valid text targets.
/// 验证 Mock 锚点绝不是合法的文本写入目标。
#[test]
fn mock_anchor_rejects_text_writes() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let mut backend = MockBackend::new();
    let anchor = backend.create_anchor();
    let text = backend.create_text("text");
    let rejected = catch_unwind(AssertUnwindSafe(|| backend.set_text(&anchor, "leak")));
    assert!(rejected.is_err(), "writing an anchor must fail");
    backend.set_text(&text, "changed");
    assert_eq!(backend.text_content(&text).as_deref(), Some("changed"));
}

/// Proves an initially-true failed mount also releases its anchors.
/// 验证初始为 true 的失败挂载同样释放锚点。
#[test]
fn initial_failed_mount_releases_anchors() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let baseline = runtime.backend().dom_snapshot().nodes.len();

    let failure = catch_unwind(AssertUnwindSafe(|| {
        Show::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "show/initial-panic",
            || true,
            |context| {
                let _ = context.create_element(Tag::Button);
                let _ = context.create_text("value");
                context.effect("show/initial-panic/effect", |_effect| {});
                panic!("initial content failed");
            },
        );
    }));

    assert!(failure.is_err());
    assert_eq!(runtime.backend().dom_snapshot().nodes.len(), baseline);
    assert_eq!(runtime.backend().listener_count(), 0);
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("Tail"),
        "the static sibling survives a failed initial mount"
    );
}

/// A first-run condition panic releases the anchors created before the effect.
/// 首次条件读取 panic 会释放 Effect 之前创建的锚点。
#[test]
fn initial_condition_panic_releases_anchors() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let baseline = runtime.backend().dom_snapshot().nodes.len();

    let failure = catch_unwind(AssertUnwindSafe(|| {
        Show::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "show/condition-panic",
            || panic!("initial condition"),
            |context: &mut ShowContext<MockBackend>| context.create_text("never"),
        );
    }));

    assert!(failure.is_err());
    assert_eq!(runtime.backend().dom_snapshot().nodes.len(), baseline);
    assert_eq!(
        runtime
            .backend()
            .dom_snapshot()
            .nodes
            .iter()
            .filter(|node| node.kind == fino::headless::DomNodeKind::Anchor)
            .count(),
        0
    );
    assert_eq!(runtime.backend().listener_count(), 0);
    assert!(owner.snapshot().effects.iter().all(|effect| !effect.active));
}

/// A content panic on a real transition leaves the anchors and effect active so
/// a later transition can retry, matching the PR16 contract.
/// 真实切换中的内容 panic 会保留锚点与活跃 Effect，使后续切换可以重试，
/// 符合 PR16 契约。
#[test]
fn content_panic_on_transition_keeps_show_retryable() {
    use std::{
        cell::Cell,
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };

    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let visible = owner.signal_named("show/retry", false);
    let attempts = Rc::new(Cell::new(0_u32));
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let show = {
        let visible = visible.clone();
        let attempts = Rc::clone(&attempts);
        Show::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "show/retry",
            move || visible.get(),
            move |context| {
                let attempt = attempts.get() + 1;
                attempts.set(attempt);
                let action = context.create_element(Tag::Button);
                context.set_attribute(&action, Attribute::AriaLabel, Some("Retry action"));
                let value = context.create_text("value");
                context.insert_before(&action, &value, None);
                if attempt == 1 {
                    panic!("first transition failed");
                }
                action
            },
        )
    };

    let failure = catch_unwind(AssertUnwindSafe(|| visible.set(true)));
    assert!(failure.is_err());
    assert!(!show.is_mounted());
    assert_eq!(
        runtime
            .backend()
            .dom_snapshot()
            .nodes
            .iter()
            .filter(|node| node.kind == fino::headless::DomNodeKind::Anchor)
            .count(),
        2,
        "the anchors survive a failed transition"
    );
    assert!(
        owner
            .snapshot()
            .effects
            .iter()
            .any(|effect| effect.name == "show/retry/condition" && effect.active),
        "the condition effect stays active for retry"
    );
    assert_eq!(runtime.backend().listener_count(), 0);

    visible.set(true);
    assert!(show.is_mounted());
    assert_eq!(attempts.get(), 2);
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("valueTail")
    );
    assert_eq!(runtime.backend().listener_count(), 0);
}

/// A dispose call from a Show content builder is recorded and completes terminal
/// cleanup after the condition effect releases its state borrow.
/// Show 内容构建器内调用 dispose 会被记录，并在条件 Effect 释放状态借用后完成
/// 终止清理。
#[test]
fn show_dispose_from_content_builder_is_terminal() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let visible = owner.signal_named("show/reentrant", false);
    let count = owner.signal_named("show/reentrant-count", 0);
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let slot: Rc<RefCell<Option<Rc<Show<MockBackend>>>>> = Rc::new(RefCell::new(None));
    let show = {
        let visible = visible.clone();
        let count = count.clone();
        let builder_slot = Rc::clone(&slot);
        Show::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "show/reentrant",
            move || visible.get(),
            move |context| {
                if let Some(show) = builder_slot.borrow().as_ref() {
                    show.dispose();
                }
                let action = context.create_element(Tag::Button);
                context.set_attribute(&action, Attribute::AriaLabel, Some("Reentrant action"));
                let value = context.create_text("value");
                context.insert_before(&action, &value, None);
                let read = count.clone();
                context.effect("show/reentrant/text", move |effect| {
                    effect
                        .backend_mut()
                        .set_text(&value, &read.get().to_string());
                });
                let click = count.clone();
                context.listen(&action, EventKind::Click, move |_event, _context| {
                    click.update(|value| *value += 1);
                });
                action
            },
        )
    };
    let show = Rc::new(show);
    *slot.borrow_mut() = Some(Rc::clone(&show));

    visible.set(true);

    assert!(!show.is_mounted(), "the reentrant request is terminal");
    assert_eq!(runtime.backend().listener_count(), 0);
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        2,
        "content and anchors are released"
    );
    assert!(owner.snapshot().effects.iter().all(|effect| !effect.active));
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("Tail")
    );
    let after_dispose = runtime.backend().operations().len();
    visible.set(false);
    visible.set(true);
    assert_eq!(
        runtime.backend().operations().len(),
        after_dispose,
        "later condition writes cannot revive the list"
    );
    show.dispose();
    show.dispose();
    assert_eq!(runtime.backend().operations().len(), after_dispose);
}

/// A dispose call from content owner cleanup during hide is recorded and still
/// releases anchors and the control owner.
/// 隐藏期间内容 Owner 清理内调用 dispose 会被记录，并仍释放锚点与 control Owner。
#[test]
fn show_dispose_from_range_cleanup_is_terminal() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let visible = owner.signal_named("show/reentrant-cleanup", true);
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let slot: Rc<RefCell<Option<Rc<Show<MockBackend>>>>> = Rc::new(RefCell::new(None));
    let show = {
        let visible = visible.clone();
        let builder_slot = Rc::clone(&slot);
        Show::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "show/reentrant-cleanup",
            move || visible.get(),
            move |context| {
                let action = context.create_element(Tag::Button);
                let cleanup_slot = Rc::clone(&builder_slot);
                context.owner().on_dispose(move || {
                    if let Some(show) = cleanup_slot.borrow().as_ref() {
                        show.dispose();
                    }
                });
                action
            },
        )
    };
    let show = Rc::new(show);
    *slot.borrow_mut() = Some(Rc::clone(&show));

    visible.set(false);

    assert!(!show.is_mounted());
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        2,
        "content and anchors are released"
    );
    assert!(owner.snapshot().effects.iter().all(|effect| !effect.active));
    let after_dispose = runtime.backend().operations().len();
    visible.set(true);
    assert_eq!(runtime.backend().operations().len(), after_dispose);
    show.dispose();
    assert_eq!(runtime.backend().operations().len(), after_dispose);
}

/// A content-owner on_dispose panic during terminal Show disposal still releases
/// content nodes and anchors, then preserves the original payload.
/// 终止 Show 释放期间内容 Owner 的 on_dispose panic 仍释放内容节点与锚点，并保留
/// 原始 payload。
#[test]
fn show_terminal_dispose_owner_panic_still_releases_anchors() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let show = Show::mount(
        &runtime,
        &owner,
        &root,
        Some(&tail),
        "show/terminal-panic",
        || true,
        |context: &mut ShowContext<MockBackend>| {
            let action = context.create_element(Tag::Button);
            context.owner().on_dispose(|| panic!("show cleanup"));
            action
        },
    );

    let failure = catch_unwind(AssertUnwindSafe(|| show.dispose()));
    let payload = failure.expect_err("owner cleanup panic must propagate");
    let message = payload
        .downcast_ref::<&str>()
        .map(|value| (*value).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert_eq!(message, "show cleanup");
    assert!(!show.is_mounted());
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        2,
        "content nodes and Show anchors are released"
    );
    let after_dispose = runtime.backend().operations().len();
    show.dispose();
    show.dispose();
    assert_eq!(
        runtime.backend().operations().len(),
        after_dispose,
        "repeated disposal is idempotent"
    );
}

/// A content-owner on_dispose panic during hide releases content but keeps the
/// Show anchors, which terminal disposal can still release.
/// 隐藏期间内容 Owner 的 on_dispose panic 释放内容但保留 Show 锚点，终止释放仍
/// 可清除它们。
#[test]
fn show_hide_owner_panic_releases_content_and_keeps_anchors() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let visible = owner.signal_named("show/hide-panic", true);
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let show = {
        let visible = visible.clone();
        Show::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "show/hide-panic",
            move || visible.get(),
            |context: &mut ShowContext<MockBackend>| {
                let action = context.create_element(Tag::Button);
                context.owner().on_dispose(|| panic!("hide cleanup"));
                action
            },
        )
    };

    let failure = catch_unwind(AssertUnwindSafe(|| visible.set(false)));
    let payload = failure.expect_err("owner cleanup panic must propagate");
    let message = payload
        .downcast_ref::<&str>()
        .map(|value| (*value).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert_eq!(message, "hide cleanup");
    assert!(!show.is_mounted());
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        4,
        "content is released while the static root, tail, and anchors remain"
    );
    assert_eq!(
        runtime
            .backend()
            .dom_snapshot()
            .nodes
            .iter()
            .filter(|node| node.kind == fino::headless::DomNodeKind::Anchor)
            .count(),
        2
    );

    show.dispose();
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        2,
        "terminal disposal releases the remaining anchors"
    );
}

/// Show content builder panic wins over a panicking content-owner on_dispose,
/// while nodes, listeners, effects, and anchors are still released.
/// Show 内容构建器 panic 优先于内容 Owner 的 on_dispose panic，同时仍释放节点、
/// 监听、Effect 与锚点。
#[test]
fn show_builder_panic_wins_over_owner_cleanup_panic() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let baseline = runtime.backend().dom_snapshot().nodes.len();

    let failure = catch_unwind(AssertUnwindSafe(|| {
        Show::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "show/builder-owner-panic",
            || true,
            |context: &mut ShowContext<MockBackend>| {
                let action = context.create_element(Tag::Button);
                let value = context.create_text("value");
                context.insert_before(&action, &value, None);
                let bound = value;
                context.effect("show/failed/text", move |effect| {
                    effect.backend_mut().set_text(&bound, "failed");
                });
                context.listen(&action, EventKind::Click, |_event, _context| {});
                context
                    .owner()
                    .on_dispose(|| panic!("content owner cleanup"));
                panic!("show builder boom");
            },
        );
    }));

    let payload = failure.expect_err("builder panic must propagate");
    let message = payload
        .downcast_ref::<&str>()
        .map(|value| (*value).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert_eq!(message, "show builder boom");
    assert_eq!(runtime.backend().listener_count(), 0);
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        baseline,
        "failed content and anchors leave no storage"
    );
    assert!(
        owner
            .snapshot()
            .effects
            .iter()
            .any(|effect| effect.name == "show/failed/text" && !effect.active),
        "the failed content effect is inactive"
    );
}

/// Disposing the parent owner from a Show content builder is recorded and
/// completes terminal range and anchor cleanup.
/// 在 Show 内容构建器内释放父 Owner 会被记录，并完成范围与锚点终止清理。
#[test]
fn show_parent_owner_dispose_in_content_builder_is_terminal() {
    let runtime = Runtime::new(MockBackend::new());
    let signals_owner = runtime.create_owner();
    let parent_owner = runtime.create_owner();
    let visible = signals_owner.signal_named("show/parent-dispose", false);
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let show = {
        let visible = visible.clone();
        let parent = parent_owner.clone();
        Show::mount(
            &runtime,
            &parent_owner,
            &root,
            Some(&tail),
            "show/parent-dispose",
            move || visible.get(),
            move |context: &mut ShowContext<MockBackend>| {
                parent.dispose();
                context.create_element(Tag::Button)
            },
        )
    };
    visible.set(true);

    assert!(!show.is_mounted());
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        2,
        "content and anchors are released after the parent request"
    );
    assert!(
        parent_owner
            .snapshot()
            .effects
            .iter()
            .any(|effect| effect.name == "show/parent-dispose/condition" && !effect.active),
        "the parent-owned condition effect is terminated"
    );
    let after_dispose = runtime.backend().operations().len();
    visible.set(false);
    show.dispose();
    assert_eq!(runtime.backend().operations().len(), after_dispose);
}

/// Standalone ShowContext keeps its public runtime accessor for downstream code.
/// 独立 ShowContext 保留公开 runtime 访问器，供下游代码使用。
#[test]
fn standalone_show_context_runtime_remains_public() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let show = Show::mount(
        &runtime,
        &owner,
        &root,
        Some(&tail),
        "show/public-runtime",
        || true,
        |context: &mut ShowContext<MockBackend>| {
            assert_eq!(context.runtime().backend().listener_count(), 0);
            context.create_element(Tag::Button)
        },
    );
    assert!(show.is_mounted());
}
