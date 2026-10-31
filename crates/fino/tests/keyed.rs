//! Keyed range reconciliation, identity, lifecycle, and panic regressions.
//! Keyed 范围协调、身份、生命周期与 panic 回归。

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

use fino::{
    backend_mock::{BackendOp, MockBackend, MockListener, MockNode},
    dom_api::{Attribute, DomBackend, EventKind, Role, Selector, Tag},
    headless::{DomNodeKind, HeadlessBackend},
    keyed::{For, ForContext},
    reactive::Signal,
    runtime::Runtime,
    show::Show,
};

fn text(backend: &mut MockBackend, parent: &MockNode, value: &str) -> MockNode {
    let node = backend.create_text(value);
    backend.insert_before(parent, &node, None);
    node
}

fn keys(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn node_id(node: MockNode) -> String {
    format!("node-{}", node.0)
}

struct Probe {
    runs: Rc<RefCell<BTreeMap<String, u32>>>,
    handles: Rc<RefCell<BTreeMap<String, MockNode>>>,
    values: Rc<RefCell<BTreeMap<String, MockNode>>>,
    locals: Rc<RefCell<BTreeMap<String, Signal<i32>>>>,
}

impl Probe {
    fn new() -> Self {
        Self {
            runs: Rc::new(RefCell::new(BTreeMap::new())),
            handles: Rc::new(RefCell::new(BTreeMap::new())),
            values: Rc::new(RefCell::new(BTreeMap::new())),
            locals: Rc::new(RefCell::new(BTreeMap::new())),
        }
    }

    fn run_count(&self, key: &str) -> u32 {
        self.runs.borrow().get(key).copied().unwrap_or(0)
    }

    fn handle(&self, key: &str) -> MockNode {
        *self
            .handles
            .borrow()
            .get(key)
            .unwrap_or_else(|| panic!("missing handle for {key}"))
    }

    fn value(&self, key: &str) -> MockNode {
        *self
            .values
            .borrow()
            .get(key)
            .unwrap_or_else(|| panic!("missing text handle for {key}"))
    }

    fn local(&self, key: &str) -> Signal<i32> {
        self.locals
            .borrow()
            .get(key)
            .unwrap_or_else(|| panic!("missing local signal for {key}"))
            .clone()
    }
}

fn row(
    context: &mut ForContext<MockBackend>,
    key: &str,
    probe: &Probe,
    fail_on: Option<&str>,
) -> Vec<MockNode> {
    *probe.runs.borrow_mut().entry(key.to_owned()).or_insert(0) += 1;
    let button = context.create_element(Tag::Button);
    context.set_attribute(&button, Attribute::AriaLabel, Some(&format!("Row {key}")));
    let value = context.create_text("");
    context.insert_before(&button, &value, None);
    let local = context
        .owner()
        .signal_named(&format!("keyed/{key}/count"), 0_i32);
    let read = local.clone();
    let label = key.to_owned();
    context.effect(&format!("keyed/{key}/text"), move |effect| {
        effect.set_text(&value, &format!("{label}={}", read.get()));
    });
    let click_local = local.clone();
    context.listen(&button, EventKind::Click, move |_event, _context| {
        click_local.update(|count| *count += 1);
    });
    probe.handles.borrow_mut().insert(key.to_owned(), button);
    probe.values.borrow_mut().insert(key.to_owned(), value);
    probe.locals.borrow_mut().insert(key.to_owned(), local);
    if fail_on == Some(key) {
        panic!("keyed item builder panic for {key}");
    }
    vec![button]
}

impl Clone for Probe {
    fn clone(&self) -> Self {
        Self {
            runs: Rc::clone(&self.runs),
            handles: Rc::clone(&self.handles),
            values: Rc::clone(&self.values),
            locals: Rc::clone(&self.locals),
        }
    }
}

/// A key keeps its node handles, text node, local signal, effect, and listener
/// through swaps, reversals, and rotations; the builder runs once per key.
/// key 在交换、反转与旋转后保留节点句柄、文本节点、局部 Signal、Effect 与
/// 监听；每个 key 的构建器只运行一次。
#[test]
fn keyed_for_preserves_identity_across_reorders() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a", "b", "c"]));
    let probe = Probe::new();
    let (root, list) = {
        let root = runtime.backend_mut().create_element(Tag::Button);
        runtime
            .backend_mut()
            .set_attribute(&root, Attribute::AriaLabel, Some("Keyed host"));
        let _prefix = text(&mut runtime.backend_mut(), &root, "Host");
        let tail = text(&mut runtime.backend_mut(), &root, "Tail");
        let probe_copy = probe.clone();
        let read_items = items.clone();
        let list = For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/rows",
            move || read_items.get(),
            move |context, key| row(context, key, &probe_copy, None),
        );
        (root, list)
    };
    assert_eq!(list.keys(), keys(&["a", "b", "c"]));
    let initial =
        ["a", "b", "c"].map(|key| (probe.handle(key), probe.value(key), probe.run_count(key)));
    assert_eq!(runtime.backend().listener_count(), 3);
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("Hosta=0b=0c=0Tail")
    );

    let before_reverse = runtime.backend().operations().len();
    items.set(keys(&["c", "b", "a"]));
    assert_eq!(list.keys(), keys(&["c", "b", "a"]));
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("Hostc=0b=0a=0Tail")
    );
    for (index, key) in ["a", "b", "c"].iter().enumerate() {
        assert_eq!(
            (probe.handle(key), probe.value(key), probe.run_count(key)),
            initial[index],
            "{key} keeps its range and builder run"
        );
    }
    let reverse_ops = runtime.backend().operations()[before_reverse..].to_vec();
    assert!(
        reverse_ops
            .iter()
            .all(|operation| matches!(operation, BackendOp::Insert)),
        "a reversal only moves existing complete ranges"
    );
    assert_eq!(
        reverse_ops.len(),
        6,
        "a three-item reverse keeps one stable range and moves two complete ranges"
    );
    assert_eq!(runtime.backend().listener_count(), 3);

    let before_rotate = runtime.backend().operations().len();
    items.set(keys(&["b", "a", "c"]));
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("Hostb=0a=0c=0Tail")
    );
    for (index, key) in ["a", "b", "c"].iter().enumerate() {
        assert_eq!(
            (probe.handle(key), probe.value(key), probe.run_count(key)),
            initial[index],
            "{key} keeps its range through rotation"
        );
    }
    assert_eq!(
        runtime
            .backend()
            .operations()
            .iter()
            .skip(before_rotate)
            .filter(|operation| matches!(operation, BackendOp::Insert))
            .count(),
        3
    );

    let state = runtime.backend().dom_snapshot();
    let host = state
        .nodes
        .iter()
        .find(|node| node.attributes.get("aria-label") == Some(&"Keyed host".to_owned()))
        .expect("host remains mounted");
    assert!(host.id == "node-1", "the static host keeps its id");
    assert!(
        state
            .nodes
            .iter()
            .any(|node| node.kind == DomNodeKind::Anchor),
        "anchors stay live while the list is mounted"
    );
}

/// Adding a key creates exactly one item range; removing a key releases exactly
/// one; unchanged keys incur neither creation nor release.
/// 新增 key 只创建一个条目范围，删除 key 只释放一个；未变化 key 不产生创建
/// 或释放。
#[test]
fn keyed_for_creates_and_releases_only_required_ranges() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", Vec::<String>::new());
    let probe = Probe::new();
    let (root, list) = {
        let root = runtime.backend_mut().create_element(Tag::Button);
        runtime
            .backend_mut()
            .set_attribute(&root, Attribute::AriaLabel, Some("Keyed host"));
        let _prefix = text(&mut runtime.backend_mut(), &root, "Host");
        let tail = text(&mut runtime.backend_mut(), &root, "Tail");
        let probe_copy = probe.clone();
        let read_items = items.clone();
        let list = For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/rows",
            move || read_items.get(),
            move |context, key| row(context, key, &probe_copy, None),
        );
        (root, list)
    };
    assert!(list.is_empty());
    let before_append = runtime.backend().operations().len();

    items.set(keys(&["a", "b", "c"]));
    let append = runtime.backend().operations()[before_append..].to_vec();
    assert_eq!(
        append
            .iter()
            .filter(|op| matches!(op, BackendOp::CreateElement(_)))
            .count(),
        3
    );
    assert_eq!(
        append
            .iter()
            .filter(|op| matches!(op, BackendOp::CreateText))
            .count(),
        3
    );
    assert_eq!(
        append
            .iter()
            .filter(|op| matches!(op, BackendOp::CreateAnchor))
            .count(),
        6
    );
    let handles = ["a", "b", "c"].map(|key| probe.handle(key));

    let before_prepend = runtime.backend().operations().len();
    items.set(keys(&["x", "a", "b", "c"]));
    let prepend = runtime.backend().operations()[before_prepend..].to_vec();
    assert_eq!(
        prepend
            .iter()
            .filter(|op| matches!(op, BackendOp::CreateElement(_)))
            .count(),
        1
    );
    assert_eq!(
        prepend
            .iter()
            .filter(|op| matches!(op, BackendOp::Release))
            .count(),
        0
    );
    assert!(
        handles
            .iter()
            .enumerate()
            .all(|(index, handle)| probe.handle(["a", "b", "c"][index]) == *handle)
    );

    let before_delete = runtime.backend().operations().len();
    items.set(keys(&["x", "a", "c"]));
    let delete = runtime.backend().operations()[before_delete..].to_vec();
    assert_eq!(
        delete
            .iter()
            .filter(|op| matches!(op, BackendOp::CreateElement(_)))
            .count(),
        0
    );
    assert_eq!(
        delete
            .iter()
            .filter(|op| matches!(op, BackendOp::Release))
            .count(),
        4,
        "one item releases text, element, and two anchors"
    );
    assert_eq!(
        delete
            .iter()
            .filter(|op| matches!(op, BackendOp::Unlisten(EventKind::Click)))
            .count(),
        1
    );
    assert_eq!(runtime.backend().listener_count(), 3);

    let before_replace = runtime.backend().operations().len();
    items.set(keys(&["x", "a", "q"]));
    let replace = runtime.backend().operations()[before_replace..].to_vec();
    assert_eq!(
        replace
            .iter()
            .filter(|op| matches!(op, BackendOp::CreateElement(_)))
            .count(),
        1
    );
    assert_eq!(
        replace
            .iter()
            .filter(|op| matches!(op, BackendOp::Release))
            .count(),
        4
    );

    let before_clear = runtime.backend().operations().len();
    items.set(Vec::new());
    let clear = runtime.backend().operations()[before_clear..].to_vec();
    assert_eq!(
        clear
            .iter()
            .filter(|op| matches!(op, BackendOp::Release))
            .count(),
        12,
        "three items release text, element, and two anchors"
    );
    assert_eq!(runtime.backend().listener_count(), 0);
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("HostTail")
    );
    assert!(list.is_empty());

    let before_empty = runtime.backend().operations().len();
    items.set(Vec::new());
    assert_eq!(
        runtime.backend().operations().len(),
        before_empty,
        "an equal empty write never touches the backend"
    );
}

/// Removing an item disposes its owner first, releases its range, and makes old
/// signal writes backend no-ops; whole-list disposal is idempotent.
/// 删除条目时先释放其 Owner 再释放范围，旧 Signal 写入不再触达后端；整体
/// 释放可重复调用。
#[test]
fn keyed_for_disposes_removed_items_and_whole_list_once() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a", "b"]));
    let probe = Probe::new();
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let list = {
        let probe_copy = probe.clone();
        let read_items = items.clone();
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/rows",
            move || read_items.get(),
            move |context, key| row(context, key, &probe_copy, None),
        )
    };
    let anchor_ids = [node_id(list.start()), node_id(list.end())];
    let removed_local = probe.local("a");
    let removed_handle = probe.handle("a");
    let before_remove = runtime.backend().operations().len();

    items.set(keys(&["b"]));
    let remove = runtime.backend().operations()[before_remove..].to_vec();
    let unlisten = remove
        .iter()
        .position(|op| matches!(op, BackendOp::Unlisten(EventKind::Click)))
        .expect("the removed listener stops");
    let release = remove
        .iter()
        .position(|op| matches!(op, BackendOp::Release))
        .expect("the removed range is released");
    assert!(
        unlisten < release,
        "the item owner stops before its backend range is released"
    );
    assert_eq!(runtime.backend().listener_count(), 1);
    assert!(
        !runtime
            .backend()
            .dom_snapshot()
            .nodes
            .iter()
            .any(|node| node.id == node_id(removed_handle)),
        "the removed element leaves stored backend state"
    );
    let inactive = owner
        .snapshot()
        .effects
        .iter()
        .any(|effect| effect.name == "keyed/a/text" && !effect.active);
    assert!(inactive, "the removed item effect is inactive");

    let after_remove = runtime.backend().operations().len();
    removed_local.set(7);
    assert_eq!(
        runtime.backend().operations().len(),
        after_remove,
        "writing a removed item signal never touches the backend"
    );

    let before_dispose = runtime.backend().operations().len();
    list.dispose();
    list.dispose();
    assert!(list.is_empty());
    assert_eq!(
        runtime.backend().listener_count(),
        0,
        "all item listeners are gone"
    );
    assert_eq!(
        runtime.backend().operations().len(),
        before_dispose + 4 + 2 + 1,
        "one item unlistens and releases four nodes, then the list releases two anchors"
    );
    let state = runtime.backend().dom_snapshot();
    assert!(!state.roots.is_empty());
    for id in anchor_ids {
        assert!(
            !state.nodes.iter().any(|node| node.id == id),
            "the list anchors are released"
        );
    }

    let before_second_dispose = runtime.backend().operations().len();
    list.dispose();
    assert_eq!(
        runtime.backend().operations().len(),
        before_second_dispose,
        "a repeated whole-list dispose is a no-op"
    );
    owner.dispose();
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("Tail")
    );
}

/// Duplicate keys are rejected atomically: initial mount creates no anchors and
/// updates keep the committed order, with a deterministic public error.
/// 重复 key 会被原子拒绝：初始挂载不创建锚点，更新保留已提交顺序，并提供
/// 确定性公开错误。
#[test]
fn keyed_for_rejects_duplicate_keys_before_backend_changes() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let probe = Probe::new();
    let before_mount = runtime.backend().operations().len();
    let duplicate_items = keys(&["a", "a"]);
    let probe_copy = probe.clone();
    let error = match For::try_mount(
        &runtime,
        &owner,
        &root,
        Some(&tail),
        "keyed/duplicate",
        move || duplicate_items.clone(),
        move |context, key| row(context, key, &probe_copy, None),
    ) {
        Ok(_) => panic!("duplicate initial keys must fail"),
        Err(error) => error,
    };
    assert_eq!(error.code, "UI_FOR_DUPLICATE_KEY");
    assert_eq!(error.index, 1);
    assert_eq!(
        runtime.backend().operations().len(),
        before_mount,
        "initial duplicate rejection creates no anchors or nodes"
    );

    let items = owner.signal_named("keyed/items", keys(&["a", "b"]));
    let probe_copy = probe.clone();
    let read_items = items.clone();
    let list = For::mount(
        &runtime,
        &owner,
        &root,
        Some(&tail),
        "keyed/rows",
        move || read_items.get(),
        move |context, key| row(context, key, &probe_copy, None),
    );
    let initial_handles = [probe.handle("a"), probe.handle("b")];
    let before_duplicate = runtime.backend().operations().len();
    items.set(keys(&["c", "c"]));
    assert_eq!(list.last_error().as_ref().map(|error| error.index), Some(1));
    assert_eq!(list.keys(), keys(&["a", "b"]));
    assert_eq!(
        runtime.backend().operations().len(),
        before_duplicate,
        "a duplicate update is rejected before any backend mutation"
    );
    assert_eq!(probe.run_count("c"), 0, "rejected keys are never built");

    items.set(keys(&["b", "a"]));
    assert!(list.last_error().is_none());
    assert_eq!(list.keys(), keys(&["b", "a"]));
    assert_eq!(probe.handle("a"), initial_handles[0]);
    assert_eq!(probe.handle("b"), initial_handles[1]);
    assert_eq!(probe.run_count("a"), 1);
    assert_eq!(probe.run_count("b"), 1);

    items.set(keys(&["a", "a"]));
    assert!(list.last_error().is_some());
    items.set(keys(&["a"]));
    assert!(list.last_error().is_none());
    assert_eq!(list.keys(), keys(&["a"]));
}

/// A panicking item builder releases its owner, nodes, effects, and listeners
/// and leaves the previous reconciliation committed; the list stays usable.
/// 条目构建器 panic 会释放其 Owner、节点、Effect 与监听，并保留上一次协调
/// 结果；列表之后仍可继续使用。
#[test]
fn keyed_for_builder_panic_is_transactional() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a"]));
    let probe = Probe::new();
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let list = {
        let probe_copy = probe.clone();
        let read_items = items.clone();
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/rows",
            move || read_items.get(),
            move |context, key| row(context, key, &probe_copy, Some("boom")),
        )
    };
    let before_nodes = runtime.backend().dom_snapshot().nodes.len();
    let before_ops = runtime.backend().operations().len();
    let outcome = catch_unwind(AssertUnwindSafe(|| items.set(keys(&["a", "boom"]))));
    assert!(outcome.is_err(), "the builder panic must escape the update");
    assert_eq!(list.keys(), keys(&["a"]));
    assert_eq!(probe.run_count("a"), 1, "retained items never rebuild");
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        before_nodes,
        "the failed range leaves no backend storage behind"
    );
    assert_eq!(
        runtime.backend().listener_count(),
        1,
        "the failed item listener is released"
    );
    assert!(
        runtime
            .backend()
            .operations()
            .iter()
            .skip(before_ops)
            .any(|op| matches!(op, BackendOp::Unlisten(EventKind::Click))),
        "the failed item unlistens before release"
    );
    assert!(
        owner
            .snapshot()
            .effects
            .iter()
            .any(|effect| effect.name == "keyed/boom/text" && !effect.active),
        "the failed item effect is inactive"
    );

    items.set(keys(&["a", "b"]));
    assert_eq!(list.keys(), keys(&["a", "b"]));
    assert_eq!(runtime.backend().listener_count(), 2);
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("a=0b=0Tail")
    );
}

/// A panic during the initial item build releases the For anchors and owner.
/// 初始条目构建期间 panic 会释放 For 锚点与 Owner。
#[test]
fn keyed_for_initial_build_panic_leaks_nothing() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let probe = Probe::new();
    let before = runtime.backend().dom_snapshot().nodes.len();
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let items = keys(&["boom"]);
        let probe_copy = probe.clone();
        let _ = For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/panic",
            move || items.clone(),
            move |context, key| row(context, key, &probe_copy, Some("boom")),
        );
    }));
    assert!(outcome.is_err());
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        before,
        "the failed initial mount releases its anchors and item nodes"
    );
    assert_eq!(
        runtime
            .backend()
            .dom_snapshot()
            .nodes
            .iter()
            .filter(|node| node.kind == DomNodeKind::Anchor)
            .count(),
        0
    );
    assert_eq!(runtime.backend().listener_count(), 0);
    assert!(
        owner.snapshot().effects.iter().all(|effect| !effect.active),
        "the failed mount leaves no active effect"
    );
}

/// An item-local signal update updates only that item and never runs whole-list
/// reconciliation.
/// 条目局部 Signal 更新只更新该条目，绝不运行整表协调。
#[test]
fn keyed_for_item_updates_skip_reconciliation() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a", "b"]));
    let probe = Probe::new();
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let list = {
        let probe_copy = probe.clone();
        let read_items = items.clone();
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/rows",
            move || read_items.get(),
            move |context, key| row(context, key, &probe_copy, None),
        )
    };
    assert_eq!(list.keys(), keys(&["a", "b"]));
    let before = runtime.backend().operations().len();
    let reconcile_before = owner
        .snapshot()
        .effects
        .iter()
        .find(|effect| effect.name == "keyed/rows/reconcile")
        .map(|effect| effect.executions)
        .expect("reconcile effect exists");

    let handle = probe.handle("a");
    let listener = runtime
        .backend()
        .listener(&handle, EventKind::Click)
        .expect("item click listener");
    listener.call(
        fino::dom_api::Event {
            kind: EventKind::Click,
        },
        &mut fino::dom_api::EventContext,
    );

    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("a=1b=0Tail")
    );
    let updates: Vec<String> = runtime
        .backend()
        .operations()
        .iter()
        .skip(before)
        .filter_map(|operation| match operation {
            BackendOp::SetText { value, .. } => Some(value.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(updates, ["a=1"], "only the clicked item text changes");
    let reconcile_after = owner
        .snapshot()
        .effects
        .iter()
        .find(|effect| effect.name == "keyed/rows/reconcile")
        .map(|effect| effect.executions)
        .expect("reconcile effect exists");
    assert_eq!(
        reconcile_after, reconcile_before,
        "item-local updates never run reconciliation"
    );
    assert_eq!(runtime.backend().listener_count(), 2);
}

/// The stable selectors used by the example conformance probe stay semantic.
/// 示例一致性探针使用的稳定选择器保持语义化。
#[test]
fn keyed_row_selector_is_addressable() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a"]));
    let probe = Probe::new();
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let _list = {
        let probe_copy = probe.clone();
        let read_items = items.clone();
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/rows",
            move || read_items.get(),
            move |context, key| row(context, key, &probe_copy, None),
        )
    };
    let selector = Selector {
        role: Role::Button,
        name: "Row a".to_owned(),
    };
    assert_eq!(runtime.backend().find_all(&selector).len(), 1);
}

/// A retained item keeps its local signal value through a reorder.
/// 保留条目在重排后保持局部 Signal 的值。
#[test]
fn keyed_for_preserves_item_local_state_across_reorders() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a", "b"]));
    let probe = Probe::new();
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let _list = {
        let probe_copy = probe.clone();
        let read_items = items.clone();
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/rows",
            move || read_items.get(),
            move |context, key| row(context, key, &probe_copy, None),
        )
    };
    let handle_a = probe.handle("a");
    let value_a = probe.value("a");
    probe.local("a").set(7);
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("a=7b=0Tail")
    );

    items.set(keys(&["b", "a"]));
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("b=0a=7Tail")
    );
    assert_eq!(probe.handle("a"), handle_a);
    assert_eq!(probe.value("a"), value_a);
    assert_eq!(probe.run_count("a"), 1);
}

/// A nested owner inside an item disposes with the item and ignores stale writes.
/// 条目内的嵌套 Owner 随条目释放，并忽略旧 Signal 写入。
#[test]
fn keyed_for_disposes_nested_item_owners() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a"]));
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let nested: Rc<RefCell<Option<Signal<i32>>>> = Rc::new(RefCell::new(None));
    let runs = Rc::new(std::cell::Cell::new(0_u32));
    let nested_runtime = runtime.clone();
    let list = {
        let read_items = items.clone();
        let nested_slot = Rc::clone(&nested);
        let nested_runs = Rc::clone(&runs);
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/nested",
            move || read_items.get(),
            move |context, key| {
                let button = context.create_element(Tag::Button);
                let value = context.create_text("");
                context.insert_before(&button, &value, None);
                let child = context.owner().child();
                let count = child.signal_named("nested/count", 0_i32);
                let read = count.clone();
                let binding = value;
                let label = key.to_owned();
                let runs = Rc::clone(&nested_runs);
                nested_runtime.effect(&child, "nested/text", move |effect| {
                    runs.set(runs.get() + 1);
                    effect
                        .backend_mut()
                        .set_text(&binding, &format!("{label}={}", read.get()));
                });
                *nested_slot.borrow_mut() = Some(count);
                vec![button]
            },
        )
    };
    assert_eq!(runs.get(), 1);
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("a=0Tail")
    );
    let stale = nested.borrow().clone().expect("nested signal");
    let before = runtime.backend().operations().len();
    items.set(Vec::new());
    assert_eq!(list.len(), 0);
    assert!(
        owner
            .snapshot()
            .effects
            .iter()
            .any(|effect| effect.name == "nested/text" && !effect.active),
        "the nested owner is disposed with the item"
    );
    stale.set(9);
    assert_eq!(
        runtime.backend().operations().len(),
        before + 4,
        "removing the item releases text, element, and two anchors exactly once"
    );
}

/// The convenience mount panics with the stable duplicate-key code.
/// 便捷挂载会以稳定的重复 key 错误码 panic。
#[test]
#[should_panic(expected = "UI_FOR_DUPLICATE_KEY")]
fn keyed_for_mount_panics_on_duplicate_keys() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let items = keys(&["a", "a"]);
    let _ = For::mount(
        &runtime,
        &owner,
        &root,
        Some(&tail),
        "keyed/duplicate",
        move || items.clone(),
        |context: &mut ForContext<MockBackend>, _key: &String| {
            vec![context.create_element(Tag::Button)]
        },
    );
}

/// A nested Show panic during the initial For build releases every item, Show,
/// and For anchor instead of leaving orphan ranges.
/// 初始 For 构建期间嵌套 Show panic 会释放全部条目、Show 与 For 锚点，而不是
/// 留下孤儿范围。
#[test]
fn keyed_for_nested_show_initial_panic_releases_anchors() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let baseline = runtime.backend().dom_snapshot().nodes.len();

    let failure = catch_unwind(AssertUnwindSafe(|| {
        let items = keys(&["a"]);
        let _ = For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/nested-initial",
            move || items.clone(),
            |context: &mut ForContext<MockBackend>, _key| {
                let item = context.create_element(Tag::Button);
                context.show(
                    &item,
                    None,
                    "nested/panic",
                    || true,
                    |_show_context| {
                        panic!("nested show content");
                    },
                );
                vec![item]
            },
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
            .filter(|node| node.kind == DomNodeKind::Anchor)
            .count(),
        0
    );
    assert_eq!(runtime.backend().listener_count(), 0);
    assert!(owner.snapshot().effects.iter().all(|effect| !effect.active));
}

/// A nested Show mounted on the list parent that panics rejects only the new
/// item and keeps the committed list structure unchanged.
/// 挂在列表父节点上的嵌套 Show panic 只拒绝新条目，已提交列表结构保持不变。
#[test]
fn keyed_for_nested_show_on_list_parent_panic_keeps_committed_list() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a"]));
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let show_runtime = runtime.clone();
    let list = {
        let read_items = items.clone();
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/nested-update",
            move || read_items.get(),
            move |context: &mut ForContext<MockBackend>, key| {
                if key == "a" {
                    let item = context.create_element(Tag::Button);
                    let value = context.create_text("A");
                    context.insert_before(&item, &value, None);
                    return vec![item];
                }
                Show::mount(
                    &show_runtime,
                    context.owner(),
                    &root,
                    None,
                    "nested/list-parent",
                    || true,
                    |_show_context| panic!("nested list content"),
                );
                vec![context.create_element(Tag::Button)]
            },
        )
    };
    let baseline_nodes = runtime.backend().dom_snapshot().nodes.len();
    let baseline_anchors = runtime
        .backend()
        .dom_snapshot()
        .nodes
        .iter()
        .filter(|node| node.kind == DomNodeKind::Anchor)
        .count();
    assert_eq!(baseline_anchors, 4);

    let failure = catch_unwind(AssertUnwindSafe(|| items.set(keys(&["a", "b"]))));
    assert!(failure.is_err());
    assert_eq!(list.keys(), keys(&["a"]));
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("ATail")
    );
    assert_eq!(runtime.backend().dom_snapshot().nodes.len(), baseline_nodes);
    assert_eq!(
        runtime
            .backend()
            .dom_snapshot()
            .nodes
            .iter()
            .filter(|node| node.kind == DomNodeKind::Anchor)
            .count(),
        baseline_anchors,
        "the failed nested Show releases only its own anchors"
    );
    assert_eq!(runtime.backend().listener_count(), 0);
}

/// Builder mutations that target mounted nodes outside the item are rejected
/// before they can modify the committed list.
/// 目标为条目外部已挂载节点的构建器变更会在修改已提交列表前被拒绝。
#[test]
fn keyed_for_rejects_foreign_context_mutations_without_state_change() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a"]));
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let host_root = root;
    let host_tail = tail;
    let list = {
        let read_items = items.clone();
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/foreign",
            move || read_items.get(),
            move |context: &mut ForContext<MockBackend>, key| {
                if key == "a" {
                    let item = context.create_element(Tag::Button);
                    let value = context.create_text("A");
                    context.insert_before(&item, &value, None);
                    return vec![item];
                }
                let created = context.create_element(Tag::Button);
                if key == "b" {
                    context.insert_before(&host_root, &created, None);
                } else {
                    context.set_text(&host_tail, "hacked");
                }
                vec![created]
            },
        )
    };
    let baseline_nodes = runtime.backend().dom_snapshot().nodes.len();
    let baseline_text = runtime
        .backend()
        .text_content(&root)
        .expect("committed list text");
    assert_eq!(baseline_text, "ATail");

    for (key, expected) in [("b", "ForContext mutations"), ("c", "ForContext mutations")] {
        let before = runtime.backend().operations().len();
        let failure = catch_unwind(AssertUnwindSafe(|| {
            items.set(keys(&["a", key]));
        }));
        let payload = failure.expect_err("foreign mutation must panic");
        let message = payload
            .downcast_ref::<&str>()
            .map(|message| (*message).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        assert!(
            message.contains(expected),
            "unexpected panic payload: {message}"
        );
        assert_eq!(list.keys(), keys(&["a"]));
        assert_eq!(
            runtime.backend().text_content(&root).as_deref(),
            Some("ATail")
        );
        assert_eq!(runtime.backend().dom_snapshot().nodes.len(), baseline_nodes);
        let operations = runtime.backend().operations()[before..].to_vec();
        assert!(
            !operations
                .iter()
                .any(|operation| matches!(operation, BackendOp::Insert)),
            "the rejected mutation never inserts or moves an existing node"
        );
        assert_eq!(runtime.backend().listener_count(), 0);
    }
}

/// Test backend that rejects any second release of the same live node.
/// 测试后端：拒绝同一存活节点的第二次释放。
#[derive(Default)]
struct StrictBackend {
    inner: MockBackend,
    live: BTreeSet<MockNode>,
}

impl StrictBackend {
    fn new() -> Self {
        Self::default()
    }

    fn inner(&self) -> &MockBackend {
        &self.inner
    }

    fn release_subtree(&mut self, node: MockNode) {
        assert!(
            self.live.contains(&node),
            "strict backend rejected a duplicate release of {node:?}"
        );
        let snapshot = self.inner.dom_snapshot();
        let mut pending = vec![node];
        while let Some(current) = pending.pop() {
            assert!(
                self.live.remove(&current),
                "strict backend rejected a duplicate descendant release of {current:?}"
            );
            let id = format!("node-{}", current.0);
            if let Some(record) = snapshot.nodes.iter().find(|record| record.id == id) {
                for child in &record.children {
                    let child = child
                        .strip_prefix("node-")
                        .and_then(|value| value.parse::<u64>().ok())
                        .map(MockNode)
                        .expect("strict backend child id");
                    pending.push(child);
                }
            }
        }
        self.inner.release(&node);
    }
}

impl DomBackend for StrictBackend {
    type Node = MockNode;
    type Listener = MockListener;

    fn create_element(&mut self, tag: Tag) -> Self::Node {
        let node = self.inner.create_element(tag);
        self.live.insert(node);
        node
    }

    fn create_text(&mut self, value: &str) -> Self::Node {
        let node = self.inner.create_text(value);
        self.live.insert(node);
        node
    }

    fn create_anchor(&mut self) -> Self::Node {
        let node = self.inner.create_anchor();
        self.live.insert(node);
        node
    }

    fn insert_before(
        &mut self,
        parent: &Self::Node,
        child: &Self::Node,
        anchor: Option<&Self::Node>,
    ) {
        self.inner.insert_before(parent, child, anchor);
    }

    fn remove(&mut self, node: &Self::Node) {
        self.inner.remove(node);
    }

    fn release(&mut self, node: &Self::Node) {
        self.release_subtree(*node);
    }

    fn set_text(&mut self, node: &Self::Node, value: &str) {
        self.inner.set_text(node, value);
    }

    fn set_attribute(&mut self, node: &Self::Node, name: Attribute, value: Option<&str>) {
        self.inner.set_attribute(node, name, value);
    }

    fn listen(
        &mut self,
        node: &Self::Node,
        event: EventKind,
        listener: fino::dom_api::EventListener,
    ) -> Self::Listener {
        self.inner.listen(node, event, listener)
    }

    fn unlisten(&mut self, listener: Self::Listener) {
        self.inner.unlisten(listener);
    }
}

/// Item deletion, external returned roots, and whole-list disposal release each
/// node exactly once on a backend that rejects duplicate release.
/// 在拒绝重复释放的后端上，条目删除、外部返回根节点与整体释放都只释放每个
/// 节点一次。
#[test]
fn keyed_for_releases_each_node_once_on_strict_backend() {
    let runtime = Runtime::new(StrictBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a", "b", "external"]));
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = {
        let node = runtime.backend_mut().create_text("Tail");
        runtime.backend_mut().insert_before(&root, &node, None);
        node
    };
    let external = runtime.backend_mut().create_element(Tag::Button);
    let list = {
        let read_items = items.clone();
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/strict",
            move || read_items.get(),
            move |context: &mut ForContext<StrictBackend>, key| {
                if key == "external" {
                    return vec![external];
                }
                let item = context.create_element(Tag::Button);
                let value = context.create_text(key);
                context.insert_before(&item, &value, None);
                vec![item]
            },
        )
    };
    assert_eq!(list.len(), 3);

    items.set(keys(&["b", "external"]));
    assert_eq!(list.keys(), keys(&["b", "external"]));
    items.set(keys(&["b"]));
    assert_eq!(list.keys(), keys(&["b"]));
    list.dispose();
    list.dispose();
    assert!(list.is_empty());
    assert_eq!(
        runtime.backend().inner().dom_snapshot().nodes.len(),
        2,
        "only the static root and tail remain stored"
    );
    owner.dispose();
}

/// Common prefix/suffix and LIS placement keep stable ranges in place while a
/// swap or a mixed insert/delete moves only the affected ranges.
/// 公共前后缀与 LIS 放置保持稳定范围不动；交换或插入删除混合更新只移动受影响
/// 范围。
#[test]
fn keyed_for_lis_keeps_prefix_suffix_and_mixed_ranges_stable() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a", "b", "c", "d"]));
    let probe = Probe::new();
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let list = {
        let read_items = items.clone();
        let probe_copy = probe.clone();
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/lis",
            move || read_items.get(),
            move |context, key| row(context, key, &probe_copy, None),
        )
    };
    let handles = ["a", "b", "c", "d"].map(|key| probe.handle(key));

    let before_swap = runtime.backend().operations().len();
    items.set(keys(&["a", "c", "b", "d"]));
    let swap = runtime.backend().operations()[before_swap..].to_vec();
    assert_eq!(
        swap.iter()
            .filter(|operation| matches!(operation, BackendOp::Insert))
            .count(),
        3,
        "only one of the two swapped middle ranges moves"
    );
    assert_eq!(probe.handle("a"), handles[0]);
    assert_eq!(probe.handle("b"), handles[1]);
    assert_eq!(probe.handle("c"), handles[2]);
    assert_eq!(probe.handle("d"), handles[3]);
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("a=0c=0b=0d=0Tail")
    );

    let before_mixed = runtime.backend().operations().len();
    items.set(keys(&["a", "x", "c"]));
    let mixed = runtime.backend().operations()[before_mixed..].to_vec();
    assert_eq!(
        mixed
            .iter()
            .filter(|operation| matches!(operation, BackendOp::CreateElement(_)))
            .count(),
        1
    );
    assert_eq!(
        mixed
            .iter()
            .filter(|operation| matches!(operation, BackendOp::Release))
            .count(),
        8,
        "removed b and d release text, element, and two anchors each"
    );
    assert_eq!(
        mixed
            .iter()
            .filter(|operation| matches!(operation, BackendOp::Insert))
            .count(),
        4,
        "the new item inserts its text and its complete range once"
    );
    assert_eq!(probe.handle("a"), handles[0]);
    assert_eq!(probe.handle("c"), handles[2]);
    assert_eq!(probe.run_count("a"), 1);
    assert_eq!(probe.run_count("c"), 1);
    assert_eq!(probe.run_count("x"), 1);
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("a=0x=0c=0Tail")
    );
    assert_eq!(list.keys(), keys(&["a", "x", "c"]));
}

/// Item effects use a restricted context and cannot mutate mounted siblings.
/// 条目 Effect 使用受限上下文，不能修改已挂载的兄弟节点。
#[test]
fn keyed_for_effect_rejects_foreign_targets() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a"]));
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let host_tail = tail;
    let list = {
        let read_items = items.clone();
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/effect-foreign",
            move || read_items.get(),
            move |context: &mut ForContext<MockBackend>, key| {
                if key == "a" {
                    let item = context.create_element(Tag::Button);
                    let value = context.create_text("A");
                    context.insert_before(&item, &value, None);
                    return vec![item];
                }
                let item = context.create_element(Tag::Button);
                context.effect("keyed/effect-foreign/bad", move |effect| {
                    effect.set_text(&host_tail, "hacked");
                });
                vec![item]
            },
        )
    };
    let failure = catch_unwind(AssertUnwindSafe(|| items.set(keys(&["a", "b"]))));
    assert!(failure.is_err());
    assert_eq!(list.keys(), keys(&["a"]));
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("ATail")
    );
}
