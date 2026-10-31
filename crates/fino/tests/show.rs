//! Range anchor and conditional Show lifecycle regressions.
//! 范围锚点与条件 Show 生命周期回归。

use std::{cell::RefCell, rc::Rc};

use fino::{
    backend_mock::{BackendOp, MockBackend, MockNode},
    dom_api::{Attribute, DomBackend, EventKind, Tag},
    headless::HeadlessBackend,
    reactive::Owner,
    runtime::Runtime,
    show::Show,
};

fn text(backend: &mut MockBackend, parent: &MockNode, value: &str) -> MockNode {
    let node = backend.create_text(value);
    backend.insert_before(parent, &node, None);
    node
}

fn action_button(
    runtime: &Runtime<MockBackend>,
    owner: &Owner,
    count: fino::reactive::Signal<i32>,
) -> MockNode {
    let button = runtime.backend_mut().create_element(Tag::Button);
    runtime
        .backend_mut()
        .set_attribute(&button, Attribute::AriaLabel, Some("Show action"));
    let value = runtime.backend_mut().create_text("");
    runtime.backend_mut().insert_before(&button, &value, None);
    let read_count = count.clone();
    runtime.effect(owner, "show/action/text", move |context| {
        context
            .backend_mut()
            .set_text(&value, &format!("Value: {}", read_count.get()));
    });
    runtime.listen(owner, &button, EventKind::Click, move |_event, _context| {
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
            move |runtime, child| action_button(runtime, child, count.clone()),
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
        .position(|operation| matches!(operation, BackendOp::Remove))
        .expect("content is removed");
    assert!(
        unlisten < remove,
        "listeners must stop before backend nodes are detached"
    );
    assert!(
        !hide_ops
            .iter()
            .any(|operation| matches!(operation, BackendOp::Remove))
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
        initial_ids + 4,
        "detached old range nodes stay historical while only one range is live"
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
            move |runtime, child| {
                let panel = runtime.backend_mut().create_element(Tag::Button);
                runtime.backend_mut().set_attribute(
                    &panel,
                    Attribute::AriaLabel,
                    Some("Outer panel"),
                );
                let label = runtime.backend_mut().create_text("Outer");
                runtime.backend_mut().insert_before(&panel, &label, None);
                let inner_visible = inner_visible.clone();
                let slot = Rc::clone(&inner_slot);
                let inner = Show::mount(
                    runtime,
                    child,
                    &panel,
                    None,
                    "show/inner-range",
                    move || inner_visible.get(),
                    |runtime, _owner| runtime.backend_mut().create_text("Inner"),
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
            |runtime, _owner| {
                let one = runtime.backend_mut().create_text("one");
                let two = runtime.backend_mut().create_text("two");
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
            |runtime, _owner| runtime.backend_mut().create_text("visible"),
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
            |runtime, _owner| runtime.backend_mut().create_text("visible"),
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
