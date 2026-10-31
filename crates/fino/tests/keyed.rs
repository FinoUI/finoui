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
    show::{RangeAnchor, Show},
};

fn text(backend: &mut MockBackend, parent: &MockNode, value: &str) -> MockNode {
    let node = backend.create_text(value);
    backend.insert_before(parent, &node, None);
    node
}

fn strict_text(runtime: &Runtime<StrictBackend>, parent: &MockNode, value: &str) -> MockNode {
    let mut backend = runtime.backend_mut();
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

type ForSlot = Rc<RefCell<Option<Rc<For<MockBackend, String>>>>>;

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
        before_dispose + 1 + 4 + 4 + 2,
        "one item unlistens, detaches and releases four nodes, then the list releases two anchors"
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
        before + 8,
        "removing the item detaches and releases text, element, and two anchors exactly once"
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
    insert_count: std::cell::Cell<usize>,
    panic_insert_at: std::cell::Cell<Option<usize>>,
    panic_insert_budget: std::cell::Cell<usize>,
    anchor_count: std::cell::Cell<usize>,
    panic_anchor_at: std::cell::Cell<Option<usize>>,
}

impl StrictBackend {
    fn new() -> Self {
        Self::default()
    }

    fn inner(&self) -> &MockBackend {
        &self.inner
    }

    fn contains(&self, node: MockNode) -> bool {
        self.live.contains(&node)
    }

    fn panic_inserts_after(&self, delta: usize, budget: usize) {
        self.panic_insert_at
            .set(Some(self.insert_count.get() + delta));
        self.panic_insert_budget.set(budget);
    }

    fn panic_anchor_after(&self, delta: usize) {
        self.panic_anchor_at
            .set(Some(self.anchor_count.get() + delta));
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
        let count = self.anchor_count.get() + 1;
        self.anchor_count.set(count);
        if self
            .panic_anchor_at
            .get()
            .is_some_and(|panic_at| count >= panic_at)
        {
            self.panic_anchor_at.set(None);
            panic!("injected anchor create panic");
        }
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
        let count = self.insert_count.get() + 1;
        self.insert_count.set(count);
        if self
            .panic_insert_at
            .get()
            .is_some_and(|panic_at| count >= panic_at)
            && self.panic_insert_budget.get() > 0
        {
            let remaining = self.panic_insert_budget.get() - 1;
            self.panic_insert_budget.set(remaining);
            if remaining == 0 {
                self.panic_insert_at.set(None);
            }
            panic!("injected move panic");
        }
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

/// A builder may return the same external root twice; terminal disposal still
/// releases that live node exactly once.
/// 构建器可以重复返回同一个外部根节点；终止释放仍只释放该存活节点一次。
#[test]
fn keyed_for_releases_duplicate_external_roots_once() {
    let runtime = Runtime::new(StrictBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a"]));
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
            "keyed/strict-duplicate-root",
            move || read_items.get(),
            move |_context: &mut ForContext<StrictBackend>, _key| vec![external, external],
        )
    };
    assert!(!list.is_empty());
    list.dispose();
    list.dispose();
    assert!(
        !runtime.backend().contains(external),
        "the duplicate external root is released exactly once"
    );
    assert_eq!(
        runtime.backend().inner().dom_snapshot().nodes.len(),
        2,
        "only the static root and tail remain stored"
    );
    owner.dispose();
}

/// Panic rollback detaches parent and child before release, preserving the
/// original builder panic for both creation orders.
/// panic 回滚在释放前分离父子节点，并在两种创建顺序下都保留原始构建器 panic。
#[test]
fn keyed_for_panic_rollback_preserves_payload_for_both_creation_orders() {
    for parent_first in [true, false] {
        let runtime = Runtime::new(StrictBackend::new());
        let owner = runtime.create_owner();
        let root = runtime.backend_mut().create_element(Tag::Button);
        let tail = strict_text(&runtime, &root, "Tail");
        let items = keys(&["a"]);
        let message = format!("builder order parent_first={parent_first}");
        let expected = message.clone();
        let failure = catch_unwind(AssertUnwindSafe(|| {
            let _ = For::mount(
                &runtime,
                &owner,
                &root,
                Some(&tail),
                "keyed/strict-panic",
                move || items.clone(),
                move |context: &mut ForContext<StrictBackend>, _key| {
                    let (parent, child) = if parent_first {
                        let parent = context.create_element(Tag::Button);
                        let child = context.create_text("child");
                        (parent, child)
                    } else {
                        let child = context.create_text("child");
                        let parent = context.create_element(Tag::Button);
                        (parent, child)
                    };
                    context.insert_before(&parent, &child, None);
                    panic!("{message}");
                },
            );
        }));
        let payload = failure.expect_err("builder panic must propagate");
        let actual = payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| {
                payload
                    .downcast_ref::<&str>()
                    .map(|value| (*value).to_owned())
            })
            .unwrap_or_default();
        assert_eq!(actual, expected, "the original builder panic is preserved");
        assert_eq!(
            runtime.backend().inner().dom_snapshot().nodes.len(),
            2,
            "no tracked node survives rollback"
        );
        assert_eq!(
            runtime
                .backend()
                .inner()
                .dom_snapshot()
                .nodes
                .iter()
                .filter(|node| node.kind == DomNodeKind::Anchor)
                .count(),
            0
        );
    }
}

/// Terminal For disposal also detaches child-before-parent creation order before
/// releasing each node once.
/// 终止 For 释放同样会先分离“子先于父创建”的节点，再逐节点释放一次。
#[test]
fn keyed_for_child_before_parent_dispose_is_single_release() {
    let runtime = Runtime::new(StrictBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a"]));
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = strict_text(&runtime, &root, "Tail");
    let list = {
        let read_items = items.clone();
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/strict-terminal",
            move || read_items.get(),
            |context: &mut ForContext<StrictBackend>, _key| {
                let child = context.create_text("child");
                let parent = context.create_element(Tag::Button);
                context.insert_before(&parent, &child, None);
                vec![parent]
            },
        )
    };
    list.dispose();
    list.dispose();
    assert_eq!(
        runtime.backend().inner().dom_snapshot().nodes.len(),
        2,
        "only the static root and tail remain stored"
    );
    owner.dispose();
}

/// Show rollback detaches tracked content before releasing it and preserves the
/// original builder panic.
/// Show 回滚在释放前分离受跟踪内容，并保留原始构建器 panic。
#[test]
fn show_panic_rollback_preserves_payload_with_child_before_parent() {
    let runtime = Runtime::new(StrictBackend::new());
    let owner = runtime.create_owner();
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = strict_text(&runtime, &root, "Tail");
    let failure = catch_unwind(AssertUnwindSafe(|| {
        Show::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "show/strict-panic",
            || true,
            |context: &mut fino::show::ShowContext<StrictBackend>| {
                let child = context.create_text("child");
                let parent = context.create_element(Tag::Button);
                context.insert_before(&parent, &child, None);
                panic!("show builder child first");
            },
        );
    }));
    let payload = failure.expect_err("Show builder panic must propagate");
    let actual = payload
        .downcast_ref::<&str>()
        .map(|value| (*value).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert_eq!(actual, "show builder child first");
    assert_eq!(
        runtime.backend().inner().dom_snapshot().nodes.len(),
        2,
        "Show anchors and content are released after the failed mount"
    );
}

/// Show terminal disposal detaches child-before-parent content before release.
/// Show 终止释放会先分离“子先于父创建”的内容，再释放。
#[test]
fn show_child_before_parent_dispose_is_single_release() {
    let runtime = Runtime::new(StrictBackend::new());
    let owner = runtime.create_owner();
    let visible = owner.signal_named("show/strict-visible", true);
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = strict_text(&runtime, &root, "Tail");
    let show = {
        let visible = visible.clone();
        Show::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "show/strict-terminal",
            move || visible.get(),
            |context: &mut fino::show::ShowContext<StrictBackend>| {
                let child = context.create_text("child");
                let parent = context.create_element(Tag::Button);
                context.insert_before(&parent, &child, None);
                parent
            },
        )
    };
    visible.set(false);
    assert!(!show.is_mounted());
    assert_eq!(
        runtime.backend().inner().dom_snapshot().nodes.len(),
        4,
        "the static root, tail, and Show anchors remain"
    );
    show.dispose();
    assert_eq!(
        runtime.backend().inner().dom_snapshot().nodes.len(),
        2,
        "Show disposal releases its anchors after the content"
    );
    owner.dispose();
}

/// A dispose call from a new item builder records a terminal request; the outer
/// reconcile must not commit the new item or leave old entries active.
/// 新条目构建器内调用 dispose 会记录终止请求；外层协调不得提交新条目或留下
/// 活跃旧条目。
#[test]
fn keyed_for_dispose_from_builder_is_terminal() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a"]));
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let slot: ForSlot = Rc::new(RefCell::new(None));
    let local: Rc<RefCell<Option<Signal<i32>>>> = Rc::new(RefCell::new(None));
    let list = {
        let read_items = items.clone();
        let builder_slot = Rc::clone(&slot);
        let local_slot = Rc::clone(&local);
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/reentrant-builder",
            move || read_items.get(),
            move |context: &mut ForContext<MockBackend>, key| {
                if key == "b"
                    && let Some(list) = builder_slot.borrow().as_ref()
                {
                    list.dispose();
                }
                let item = context.create_element(Tag::Button);
                let value = context.create_text("");
                context.insert_before(&item, &value, None);
                let count = context
                    .owner()
                    .signal_named(&format!("keyed/{key}/count"), 0_i32);
                let read = count.clone();
                context.effect("keyed/reentrant/text", move |effect| {
                    effect.set_text(&value, &read.get().to_string());
                });
                let click = count.clone();
                context.listen(&item, EventKind::Click, move |_event, _context| {
                    click.update(|value| *value += 1);
                });
                *local_slot.borrow_mut() = Some(count);
                vec![item]
            },
        )
    };
    let list = Rc::new(list);
    *slot.borrow_mut() = Some(Rc::clone(&list));
    let before = runtime.backend().operations().len();

    items.set(keys(&["a", "b"]));

    assert!(list.is_empty(), "the list is terminally disposed");
    assert_eq!(list.keys(), Vec::<String>::new());
    assert_eq!(runtime.backend().listener_count(), 0);
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        2,
        "only the static root and tail remain"
    );
    assert!(owner.snapshot().effects.iter().all(|effect| !effect.active));
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("Tail")
    );
    let after_dispose = runtime.backend().operations().len();
    if let Some(signal) = local.borrow().as_ref() {
        signal.set(9);
    }
    assert_eq!(
        runtime.backend().operations().len(),
        after_dispose,
        "writes to disposed item signals do not reach the backend"
    );
    items.set(keys(&["c"]));
    items.set(keys(&["c", "c"]));
    assert!(list.is_empty(), "later updates cannot revive the list");
    assert_eq!(runtime.backend().operations().len(), after_dispose);
    list.dispose();
    list.dispose();
    assert_eq!(runtime.backend().operations().len(), after_dispose);
    let _ = before;
}

/// A dispose call from a removed item's on_dispose cleanup is not lost; the
/// outer reconcile terminates the remaining and newly built ranges.
/// 被删除条目的 on_dispose 清理内调用 dispose 不会被丢弃；外层协调会终止剩余
/// 与新建范围。
#[test]
fn keyed_for_dispose_from_removed_item_cleanup_is_terminal() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a"]));
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let slot: ForSlot = Rc::new(RefCell::new(None));
    let local_a: Rc<RefCell<Option<Signal<i32>>>> = Rc::new(RefCell::new(None));
    let list = {
        let read_items = items.clone();
        let builder_slot = Rc::clone(&slot);
        let local_slot = Rc::clone(&local_a);
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/reentrant-cleanup",
            move || read_items.get(),
            move |context: &mut ForContext<MockBackend>, key| {
                let item = context.create_element(Tag::Button);
                let value = context.create_text("");
                context.insert_before(&item, &value, None);
                let count = context
                    .owner()
                    .signal_named(&format!("keyed/{key}/count"), 0_i32);
                let read = count.clone();
                context.effect("keyed/reentrant/text", move |effect| {
                    effect.set_text(&value, &read.get().to_string());
                });
                let click = count.clone();
                context.listen(&item, EventKind::Click, move |_event, _context| {
                    click.update(|value| *value += 1);
                });
                if key == "a" {
                    *local_slot.borrow_mut() = Some(count.clone());
                    let cleanup_slot = Rc::clone(&builder_slot);
                    context.on_dispose(move || {
                        if let Some(list) = cleanup_slot.borrow().as_ref() {
                            list.dispose();
                        }
                    });
                }
                vec![item]
            },
        )
    };
    let list = Rc::new(list);
    *slot.borrow_mut() = Some(Rc::clone(&list));

    items.set(keys(&["b"]));

    assert!(
        list.is_empty(),
        "removal cleanup completes terminal disposal"
    );
    assert_eq!(list.keys(), Vec::<String>::new());
    assert_eq!(runtime.backend().listener_count(), 0);
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        2,
        "the removed item and newly built item are both released"
    );
    assert!(owner.snapshot().effects.iter().all(|effect| !effect.active));
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("Tail")
    );
    let after_dispose = runtime.backend().operations().len();
    if let Some(signal) = local_a.borrow().as_ref() {
        signal.set(7);
    }
    assert_eq!(runtime.backend().operations().len(), after_dispose);
    items.set(keys(&["b", "b"]));
    items.set(keys(&["c"]));
    assert!(list.is_empty(), "later updates cannot revive the list");
    assert_eq!(runtime.backend().operations().len(), after_dispose);
    list.dispose();
    assert_eq!(runtime.backend().operations().len(), after_dispose);
}

/// A reentrant dispose followed by a builder panic keeps the original payload
/// and still completes terminal cleanup.
/// 重入 dispose 后构建器 panic 仍保留原始 payload，并完成终止清理。
#[test]
fn keyed_for_dispose_then_builder_panic_keeps_payload_and_terminal_state() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a"]));
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let slot: ForSlot = Rc::new(RefCell::new(None));
    let list = {
        let read_items = items.clone();
        let builder_slot = Rc::clone(&slot);
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/reentrant-panic",
            move || read_items.get(),
            move |context: &mut ForContext<MockBackend>, key| {
                if key == "b"
                    && let Some(list) = builder_slot.borrow().as_ref()
                {
                    list.dispose();
                    panic!("reentrant builder panic");
                }
                let item = context.create_element(Tag::Button);
                vec![item]
            },
        )
    };
    let list = Rc::new(list);
    *slot.borrow_mut() = Some(Rc::clone(&list));

    let failure = catch_unwind(AssertUnwindSafe(|| items.set(keys(&["a", "b"]))));
    let payload = failure.expect_err("builder panic must propagate");
    let message = payload
        .downcast_ref::<&str>()
        .map(|value| (*value).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert_eq!(message, "reentrant builder panic");
    assert!(list.is_empty());
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        2,
        "the old and new items are both released"
    );
    assert!(owner.snapshot().effects.iter().all(|effect| !effect.active));
    list.dispose();
}

/// A panic in the first removed item's on_dispose still releases that range,
/// cleans the remaining removed entries, commits surviving/new state, and
/// keeps the original payload.
/// 第一个被删除条目的 on_dispose panic 仍释放该范围、清理其余删除条目、提交
/// 存活/新建状态，并保留原始 payload。
#[test]
fn keyed_for_removed_cleanup_panic_restores_state() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a", "b"]));
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let b_cleanup = Rc::new(std::cell::Cell::new(0_u32));
    let list = {
        let read_items = items.clone();
        let b_cleanup = Rc::clone(&b_cleanup);
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/removal-panic",
            move || read_items.get(),
            move |context: &mut ForContext<MockBackend>, key| {
                let item = context.create_element(Tag::Button);
                let value = context.create_text(key);
                context.insert_before(&item, &value, None);
                if key == "a" {
                    context.on_dispose(|| panic!("a cleanup"));
                }
                if key == "b" {
                    let b_cleanup = Rc::clone(&b_cleanup);
                    context.on_dispose(move || b_cleanup.set(b_cleanup.get() + 1));
                }
                vec![item]
            },
        )
    };

    let failure = catch_unwind(AssertUnwindSafe(|| items.set(keys(&["c"]))));
    let payload = failure.expect_err("a cleanup panic must propagate");
    let message = payload
        .downcast_ref::<&str>()
        .map(|value| (*value).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert_eq!(message, "a cleanup", "the first payload is preserved");
    assert_eq!(b_cleanup.get(), 1, "later removed entries still clean up");
    assert_eq!(list.keys(), keys(&["c"]));
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("cTail")
    );
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        8,
        "static root/tail, list anchors, and one committed item range"
    );

    items.set(keys(&["d"]));
    assert_eq!(list.keys(), keys(&["d"]));
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("dTail")
    );
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        8,
        "a changed-key update does not duplicate ranges"
    );

    list.dispose();
    assert!(list.is_empty());
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        2,
        "whole-list disposal releases every surviving range and anchors"
    );
}

/// Multiple removed entries panic; every cleanup runs, the first payload wins,
/// and the committed state remains consistent.
/// 多个被删除条目 panic 时每个清理都会运行，首个 payload 优先，已提交状态保持一致。
#[test]
fn keyed_for_multiple_removed_cleanup_panics_keep_first_payload() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a", "b", "c"]));
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let b_cleanup = Rc::new(std::cell::Cell::new(0_u32));
    let c_cleanup = Rc::new(std::cell::Cell::new(0_u32));
    let list = {
        let read_items = items.clone();
        let b_cleanup = Rc::clone(&b_cleanup);
        let c_cleanup = Rc::clone(&c_cleanup);
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/multi-removal-panic",
            move || read_items.get(),
            move |context: &mut ForContext<MockBackend>, key| {
                let item = context.create_element(Tag::Button);
                let value = context.create_text(key);
                context.insert_before(&item, &value, None);
                match key.as_str() {
                    "a" => context.on_dispose(|| panic!("a cleanup first")),
                    "b" => {
                        let b_cleanup = Rc::clone(&b_cleanup);
                        context.on_dispose(move || b_cleanup.set(b_cleanup.get() + 1));
                    }
                    "c" => {
                        let c_cleanup = Rc::clone(&c_cleanup);
                        context.on_dispose(move || c_cleanup.set(c_cleanup.get() + 1));
                    }
                    _ => {}
                }
                vec![item]
            },
        )
    };

    let failure = catch_unwind(AssertUnwindSafe(|| items.set(keys(&["d"]))));
    let payload = failure.expect_err("a cleanup panic must propagate");
    let message = payload
        .downcast_ref::<&str>()
        .map(|value| (*value).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert_eq!(message, "a cleanup first");
    assert_eq!(b_cleanup.get(), 1);
    assert_eq!(c_cleanup.get(), 1);
    assert_eq!(list.keys(), keys(&["d"]));
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("dTail")
    );
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        8,
        "only the surviving committed range remains"
    );

    list.dispose();
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        2,
        "terminal disposal still releases the surviving range and anchors"
    );
}

/// Removal cleanup combines a reentrant dispose request with a later owner panic;
/// terminal cleanup wins and the first panic payload is preserved.
/// 删除清理同时出现重入 dispose 与后续 Owner panic 时，终止清理优先并保留首个
/// panic payload。
#[test]
fn keyed_for_removal_panic_with_reentrant_dispose_is_terminal() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a", "b"]));
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let slot: ForSlot = Rc::new(RefCell::new(None));
    let list = {
        let read_items = items.clone();
        let builder_slot = Rc::clone(&slot);
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/panic-terminal",
            move || read_items.get(),
            move |context: &mut ForContext<MockBackend>, key| {
                let item = context.create_element(Tag::Button);
                let value = context.create_text(key);
                context.insert_before(&item, &value, None);
                match key.as_str() {
                    "a" => {
                        let cleanup_slot = Rc::clone(&builder_slot);
                        context.on_dispose(move || {
                            if let Some(list) = cleanup_slot.borrow().as_ref() {
                                list.dispose();
                            }
                        });
                    }
                    "b" => context.on_dispose(|| panic!("b cleanup")),
                    _ => {}
                }
                vec![item]
            },
        )
    };
    let list = Rc::new(list);
    *slot.borrow_mut() = Some(Rc::clone(&list));

    let failure = catch_unwind(AssertUnwindSafe(|| items.set(keys(&["c"]))));
    let payload = failure.expect_err("owner cleanup panic must propagate");
    let message = payload
        .downcast_ref::<&str>()
        .map(|value| (*value).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert_eq!(message, "b cleanup");
    assert!(list.is_empty());
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        2,
        "the new range, removed ranges, and anchors are all released"
    );
    assert!(owner.snapshot().effects.iter().all(|effect| !effect.active));
    let after_dispose = runtime.backend().operations().len();
    items.set(keys(&["d"]));
    items.set(keys(&["d", "d"]));
    list.dispose();
    assert_eq!(runtime.backend().operations().len(), after_dispose);
    assert!(list.is_empty());
}

/// A failed new item builder keeps its own panic even when the item owner's
/// on_dispose panics, and still releases nodes, listeners, and effects.
/// 新条目构建器失败时，即使条目 Owner 的 on_dispose 也 panic，仍保留构建器
/// panic，并释放节点、监听与 Effect。
#[test]
fn keyed_for_builder_panic_wins_over_owner_cleanup_panic() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a"]));
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let a_handle: Rc<RefCell<Option<MockNode>>> = Rc::new(RefCell::new(None));
    let list = {
        let read_items = items.clone();
        let a_handle = Rc::clone(&a_handle);
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/builder-owner-panic",
            move || read_items.get(),
            move |context: &mut ForContext<MockBackend>, key| {
                if key == "b" {
                    let item = context.create_element(Tag::Button);
                    let value = context.create_text("b");
                    context.insert_before(&item, &value, None);
                    context.on_dispose(|| panic!("owner cleanup boom"));
                    let bound = value;
                    context.effect("keyed/failed/text", move |effect| {
                        effect.set_text(&bound, "failed");
                    });
                    context.listen(&item, EventKind::Click, |_event, _context| {});
                    panic!("builder boom");
                }
                let item = context.create_element(Tag::Button);
                let value = context.create_text(key);
                context.insert_before(&item, &value, None);
                if key == "a" {
                    *a_handle.borrow_mut() = Some(item);
                }
                vec![item]
            },
        )
    };
    let old_handle = a_handle.borrow().expect("a handle");
    let baseline = runtime.backend().dom_snapshot().nodes.len();

    let failure = catch_unwind(AssertUnwindSafe(|| items.set(keys(&["a", "b"]))));
    let payload = failure.expect_err("builder panic must propagate");
    let message = payload
        .downcast_ref::<&str>()
        .map(|value| (*value).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert_eq!(message, "builder boom", "the builder payload wins");
    assert_eq!(list.keys(), keys(&["a"]));
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        baseline,
        "the failed item leaves no nodes behind"
    );
    assert_eq!(runtime.backend().listener_count(), 0);
    assert!(
        owner
            .snapshot()
            .effects
            .iter()
            .any(|effect| effect.name == "keyed/failed/text" && !effect.active),
        "the failed item effect is inactive"
    );
    assert_eq!(
        a_handle.borrow().expect("a handle still live"),
        old_handle,
        "the committed item keeps its NodeId"
    );

    items.set(keys(&["a", "c"]));
    assert_eq!(list.keys(), keys(&["a", "c"]));
    assert_eq!(a_handle.borrow().expect("a handle"), old_handle);
    list.dispose();
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        2,
        "subsequent updates and whole-list disposal remain healthy"
    );
}

/// Without a builder panic, a terminal owner cleanup panic still releases every
/// node and anchor and propagates the owner payload.
/// 没有构建器 panic 时，终止 Owner 清理 panic 仍释放全部节点与锚点，并传播
/// Owner payload。
#[test]
fn keyed_for_terminal_owner_panic_still_releases_range() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a"]));
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let list = {
        let read_items = items.clone();
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/terminal-owner-panic",
            move || read_items.get(),
            move |context: &mut ForContext<MockBackend>, key| {
                let item = context.create_element(Tag::Button);
                let value = context.create_text(key);
                context.insert_before(&item, &value, None);
                context.listen(&item, EventKind::Click, |_event, _context| {});
                context.on_dispose(|| panic!("terminal owner cleanup"));
                vec![item]
            },
        )
    };

    let failure = catch_unwind(AssertUnwindSafe(|| list.dispose()));
    let payload = failure.expect_err("owner cleanup panic must propagate");
    let message = payload
        .downcast_ref::<&str>()
        .map(|value| (*value).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert_eq!(message, "terminal owner cleanup");
    assert!(list.is_empty());
    assert_eq!(runtime.backend().listener_count(), 0);
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        2,
        "item nodes and list anchors are released"
    );
    let after_dispose = runtime.backend().operations().len();
    list.dispose();
    assert_eq!(runtime.backend().operations().len(), after_dispose);
}

/// Two keys cannot return the same external node; initial mount rejects it
/// deterministically and releases each staged resource once.
/// 两个 key 不能返回同一个外部节点；初始挂载会确定性拒绝，并只释放每个暂存
/// 资源一次。
#[test]
fn keyed_for_initial_shared_external_node_is_rejected() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let external = runtime.backend_mut().create_element(Tag::Button);
    let items = keys(&["a", "b"]);

    let error = match For::try_mount(
        &runtime,
        &owner,
        &root,
        Some(&tail),
        "keyed/shared-initial",
        move || items.clone(),
        move |_context: &mut ForContext<MockBackend>, _key| vec![external],
    ) {
        Ok(_) => panic!("shared external node must be rejected"),
        Err(error) => error,
    };
    assert_eq!(error.code, "UI_FOR_SHARED_NODE");
    assert_eq!(
        error.index, 1,
        "the second key owns the conflicting staged node"
    );
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        2,
        "only the static root and tail remain"
    );
    assert_eq!(runtime.backend().listener_count(), 0);
}

/// A shared external node in an update is rejected atomically without releasing
/// the retained entry's node; a later valid update recovers.
/// 更新中的共享外部节点会被原子拒绝，不释放保留条目的节点；后续合法更新可恢复。
#[test]
fn keyed_for_update_shared_external_node_preserves_retained_owner() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a"]));
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let external = runtime.backend_mut().create_element(Tag::Button);
    let external_id = format!("node-{}", external.0);
    let list = {
        let read_items = items.clone();
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/shared-update",
            move || read_items.get(),
            move |context: &mut ForContext<MockBackend>, key| {
                if key == "a" {
                    return vec![external];
                }
                if key == "b" {
                    return vec![external];
                }
                let item = context.create_element(Tag::Button);
                let value = context.create_text(key);
                context.insert_before(&item, &value, None);
                vec![item]
            },
        )
    };
    let baseline = runtime.backend().dom_snapshot().nodes.len();
    assert!(
        runtime
            .backend()
            .dom_snapshot()
            .nodes
            .iter()
            .any(|node| node.id == external_id)
    );

    items.set(keys(&["a", "b"]));

    let error = list.last_error().expect("shared-node rejection");
    assert_eq!(error.code, "UI_FOR_SHARED_NODE");
    assert_eq!(error.index, 1, "the rejected key position is reported");
    assert_eq!(list.keys(), keys(&["a"]));
    assert_eq!(runtime.backend().dom_snapshot().nodes.len(), baseline);
    assert!(
        runtime
            .backend()
            .dom_snapshot()
            .nodes
            .iter()
            .any(|node| node.id == external_id),
        "the retained entry still owns the shared external node"
    );

    items.set(keys(&["a", "c"]));
    assert!(list.last_error().is_none());
    assert_eq!(list.keys(), keys(&["a", "c"]));
    list.dispose();
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        2,
        "the external node is released exactly once at terminal disposal"
    );
}

/// Multiple staged entries with panicking cleanups do not interrupt rollback or
/// override the later builder payload.
/// 多个暂存条目的清理 panic 不会中断回滚，也不会覆盖后续构建器 payload。
#[test]
fn keyed_for_staged_cleanup_panics_keep_builder_payload() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a"]));
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let b_cleanup = Rc::new(std::cell::Cell::new(0_u32));
    let c_cleanup = Rc::new(std::cell::Cell::new(0_u32));
    let list = {
        let read_items = items.clone();
        let b_cleanup = Rc::clone(&b_cleanup);
        let c_cleanup = Rc::clone(&c_cleanup);
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/staged-panics",
            move || read_items.get(),
            move |context: &mut ForContext<MockBackend>, key| {
                let item = context.create_element(Tag::Button);
                let value = context.create_text(key);
                context.insert_before(&item, &value, None);
                match key.as_str() {
                    "b" => {
                        let cleanup = Rc::clone(&b_cleanup);
                        context.on_dispose(move || {
                            cleanup.set(cleanup.get() + 1);
                            panic!("b cleanup");
                        });
                    }
                    "c" => {
                        let cleanup = Rc::clone(&c_cleanup);
                        context.on_dispose(move || {
                            cleanup.set(cleanup.get() + 1);
                            panic!("c cleanup");
                        });
                    }
                    "d" => panic!("d builder panic"),
                    _ => {}
                }
                vec![item]
            },
        )
    };
    let baseline = runtime.backend().dom_snapshot().nodes.len();

    let failure = catch_unwind(AssertUnwindSafe(|| items.set(keys(&["a", "b", "c", "d"]))));
    let payload = failure.expect_err("d builder panic must propagate");
    let message = payload
        .downcast_ref::<&str>()
        .map(|value| (*value).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert_eq!(message, "d builder panic");
    assert_eq!(b_cleanup.get(), 1, "b staged cleanup still ran");
    assert_eq!(c_cleanup.get(), 1, "c staged cleanup still ran");
    assert_eq!(list.keys(), keys(&["a"]));
    assert_eq!(runtime.backend().dom_snapshot().nodes.len(), baseline);
    assert_eq!(runtime.backend().listener_count(), 0);
    assert!(
        owner
            .snapshot()
            .effects
            .iter()
            .all(|effect| { effect.name != "keyed/staged-panics/reconcile" || effect.active })
    );

    items.set(keys(&["a", "e"]));
    assert_eq!(list.keys(), keys(&["a", "e"]));
    list.dispose();
    assert_eq!(runtime.backend().dom_snapshot().nodes.len(), 2);
}

/// A staged dispose request still performs terminal cleanup even when a staged
/// cleanup panics, and that cleanup payload survives.
/// 暂存清理 panic 时，暂存 dispose 请求仍完成终止清理，并保留该清理 payload。
#[test]
fn keyed_for_staged_dispose_with_cleanup_panic_is_terminal() {
    let runtime = Runtime::new(MockBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a"]));
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let slot: ForSlot = Rc::new(RefCell::new(None));
    let list = {
        let read_items = items.clone();
        let builder_slot = Rc::clone(&slot);
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/staged-dispose",
            move || read_items.get(),
            move |context: &mut ForContext<MockBackend>, key| {
                let item = context.create_element(Tag::Button);
                let value = context.create_text(key);
                context.insert_before(&item, &value, None);
                if key == "b" {
                    context.on_dispose(|| panic!("b staged cleanup"));
                    if let Some(list) = builder_slot.borrow().as_ref() {
                        list.dispose();
                    }
                }
                vec![item]
            },
        )
    };
    let list = Rc::new(list);
    *slot.borrow_mut() = Some(Rc::clone(&list));

    let failure = catch_unwind(AssertUnwindSafe(|| items.set(keys(&["a", "b"]))));
    let payload = failure.expect_err("cleanup panic must propagate");
    let message = payload
        .downcast_ref::<&str>()
        .map(|value| (*value).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert_eq!(message, "b staged cleanup");
    assert!(list.is_empty());
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        2,
        "old entries, staged entries, and anchors are released"
    );
    assert!(owner.snapshot().effects.iter().all(|effect| !effect.active));
    let after_dispose = runtime.backend().operations().len();
    items.set(keys(&["c"]));
    list.dispose();
    assert_eq!(runtime.backend().operations().len(), after_dispose);
}

/// Disposing the parent owner from a live builder records a terminal request
/// even though reconciliation owns the state, then releases every range.
/// 在构建器内释放父 Owner 时，即使协调持有状态，也会记录终止请求并释放全部范围。
#[test]
fn keyed_for_parent_owner_dispose_in_builder_is_terminal() {
    let runtime = Runtime::new(MockBackend::new());
    let signals_owner = runtime.create_owner();
    let parent_owner = runtime.create_owner();
    let items = signals_owner.signal_named("keyed/items", keys(&["a"]));
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = text(&mut runtime.backend_mut(), &root, "Tail");
    let list = {
        let read_items = items.clone();
        let parent = parent_owner.clone();
        For::mount(
            &runtime,
            &parent_owner,
            &root,
            Some(&tail),
            "keyed/parent-dispose",
            move || read_items.get(),
            move |context: &mut ForContext<MockBackend>, key| {
                if key == "b" {
                    parent.dispose();
                }
                let item = context.create_element(Tag::Button);
                let value = context.create_text(key);
                context.insert_before(&item, &value, None);
                vec![item]
            },
        )
    };
    items.set(keys(&["a", "b"]));

    assert!(list.is_empty());
    assert_eq!(runtime.backend().listener_count(), 0);
    assert_eq!(
        runtime.backend().dom_snapshot().nodes.len(),
        2,
        "the live builder request releases old and new ranges"
    );
    assert!(
        signals_owner
            .snapshot()
            .effects
            .iter()
            .all(|effect| !effect.active)
    );
    let after_dispose = runtime.backend().operations().len();
    items.set(keys(&["c"]));
    list.dispose();
    assert_eq!(runtime.backend().operations().len(), after_dispose);
}

/// A backend move panic mid-placement restores the old keys, DOM order, and
/// identity before propagating, so retrying the same update succeeds.
/// 放置中途的后端 move panic 会先恢复旧 keys、DOM 顺序与身份，再传播；重试同一
/// 更新可以成功。
#[test]
fn keyed_for_move_panic_rolls_back_and_retries() {
    let runtime = Runtime::new(StrictBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a", "b", "c"]));
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = {
        let node = runtime.backend_mut().create_text("Tail");
        runtime.backend_mut().insert_before(&root, &node, None);
        node
    };
    let handles: Rc<RefCell<BTreeMap<String, MockNode>>> = Rc::new(RefCell::new(BTreeMap::new()));
    let list = {
        let read_items = items.clone();
        let handles = Rc::clone(&handles);
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/move-panic",
            move || read_items.get(),
            move |context: &mut ForContext<StrictBackend>, key| {
                let item = context.create_element(Tag::Button);
                let value = context.create_text(key);
                context.insert_before(&item, &value, None);
                handles.borrow_mut().insert(key.to_owned(), item);
                vec![item]
            },
        )
    };
    let baseline_nodes = runtime.backend().inner().dom_snapshot().nodes.len();
    let initial_handles = ["a", "b", "c"].map(|key| *handles.borrow().get(key).unwrap());
    runtime.backend().panic_inserts_after(4, 1);

    let failure = catch_unwind(AssertUnwindSafe(|| items.set(keys(&["c", "b", "a"]))));
    let payload = failure.expect_err("move panic must propagate");
    let message = payload
        .downcast_ref::<&str>()
        .map(|value| (*value).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert_eq!(message, "injected move panic");
    assert_eq!(list.keys(), keys(&["a", "b", "c"]));
    assert_eq!(
        runtime.backend().inner().text_content(&root).as_deref(),
        Some("abcTail")
    );
    assert_eq!(
        runtime.backend().inner().dom_snapshot().nodes.len(),
        baseline_nodes,
        "rollback restores the old DOM without leaking staged entries"
    );
    for (index, key) in ["a", "b", "c"].iter().enumerate() {
        assert_eq!(
            handles.borrow().get(*key).copied(),
            Some(initial_handles[index]),
            "{key} keeps its NodeId across rollback"
        );
    }

    items.set(keys(&["c", "b", "a"]));
    assert_eq!(list.keys(), keys(&["c", "b", "a"]));
    assert_eq!(
        runtime.backend().inner().text_content(&root).as_deref(),
        Some("cbaTail")
    );
    for (index, key) in ["a", "b", "c"].iter().enumerate() {
        assert_eq!(
            handles.borrow().get(*key).copied(),
            Some(initial_handles[index])
        );
    }
    list.dispose();
    assert_eq!(runtime.backend().inner().dom_snapshot().nodes.len(), 2);
}

/// If rollback itself panics, the list enters terminal cleanup, preserves the
/// first backend payload, and cannot be revived.
/// 若回滚自身也 panic，列表进入终止清理，保留首个后端 payload，且不可复活。
#[test]
fn keyed_for_move_panic_rollback_failure_is_terminal() {
    let runtime = Runtime::new(StrictBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", keys(&["a", "b", "c"]));
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = {
        let node = runtime.backend_mut().create_text("Tail");
        runtime.backend_mut().insert_before(&root, &node, None);
        node
    };
    let list = {
        let read_items = items.clone();
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/move-panic-terminal",
            move || read_items.get(),
            |context: &mut ForContext<StrictBackend>, key| {
                let item = context.create_element(Tag::Button);
                let value = context.create_text(key);
                context.insert_before(&item, &value, None);
                vec![item]
            },
        )
    };
    runtime.backend().panic_inserts_after(1, 2);

    let failure = catch_unwind(AssertUnwindSafe(|| items.set(keys(&["c", "b", "a"]))));
    let payload = failure.expect_err("first panic must propagate");
    let message = payload
        .downcast_ref::<&str>()
        .map(|value| (*value).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert_eq!(message, "injected move panic");
    assert!(list.is_empty());
    assert_eq!(
        runtime.backend().inner().dom_snapshot().nodes.len(),
        2,
        "terminal cleanup releases every range and anchor"
    );
    assert!(owner.snapshot().effects.iter().all(|effect| !effect.active));
    let after_dispose = runtime.backend().inner().operations().len();
    items.set(keys(&["d"]));
    list.dispose();
    assert_eq!(runtime.backend().inner().operations().len(), after_dispose);
}

/// RangeAnchor create and insert failures release partial anchors without
/// touching static siblings and preserve the backend payload.
/// RangeAnchor 创建或插入失败会释放部分锚点，不触碰静态兄弟，并保留后端 payload。
#[test]
fn range_anchor_create_failures_are_transactional() {
    for (label, arm) in [("create", 0_u8), ("first_insert", 1), ("second_insert", 2)] {
        let runtime = Runtime::new(StrictBackend::new());
        let root = runtime.backend_mut().create_element(Tag::Button);
        let tail = {
            let node = runtime.backend_mut().create_text("Tail");
            runtime.backend_mut().insert_before(&root, &node, None);
            node
        };
        match arm {
            0 => runtime.backend().panic_anchor_after(2),
            1 => runtime.backend().panic_inserts_after(1, 1),
            2 => runtime.backend().panic_inserts_after(2, 1),
            _ => unreachable!(),
        }
        let failure = catch_unwind(AssertUnwindSafe(|| {
            let _ = RangeAnchor::create(&runtime, &root, Some(&tail));
        }));
        let payload = failure.expect_err("anchor creation must fail");
        let message = payload
            .downcast_ref::<&str>()
            .map(|value| (*value).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        assert!(
            message.contains("injected"),
            "{label}: unexpected payload {message}"
        );
        assert_eq!(
            runtime.backend().inner().dom_snapshot().nodes.len(),
            2,
            "{label}: partial anchors are released and static nodes remain"
        );
        assert_eq!(
            runtime.backend().inner().text_content(&root).as_deref(),
            Some("Tail")
        );
    }
}

/// RangeAnchor move_before ignores a start self-reference, rejects end or
/// external-node references before mutation, and still moves normally.
/// RangeAnchor move_before 对 start 自引用 no-op，在变更前拒绝 end 或外部节点
/// 引用，并保持正常移动。
#[test]
fn range_anchor_move_before_reference_contract() {
    let runtime = Runtime::new(StrictBackend::new());
    let root = runtime.backend_mut().create_element(Tag::Button);
    let prefix = {
        let node = runtime.backend_mut().create_text("P");
        runtime.backend_mut().insert_before(&root, &node, None);
        node
    };
    let middle = {
        let node = runtime.backend_mut().create_text("M");
        runtime.backend_mut().insert_before(&root, &node, None);
        node
    };
    let anchor = RangeAnchor::create(&runtime, &root, Some(&middle));
    let external = runtime.backend_mut().create_text("X");
    anchor.insert(&runtime, &root, &external);
    let tail = {
        let node = runtime.backend_mut().create_text("T");
        runtime.backend_mut().insert_before(&root, &node, None);
        node
    };
    assert_eq!(
        runtime.backend().inner().text_content(&root).as_deref(),
        Some("PXMT")
    );
    let baseline = runtime.backend().inner().dom_snapshot().nodes.len();

    let before_start = runtime.backend().inner().operations().len();
    anchor.move_before(&runtime, &root, &[external], Some(anchor.start()));
    assert_eq!(
        runtime.backend().inner().operations().len(),
        before_start,
        "a start self-reference is a zero-operation no-op"
    );

    for (label, reference) in [("end", anchor.end()), ("external", &external)] {
        let before = runtime.backend().inner().operations().len();
        let failure = catch_unwind(AssertUnwindSafe(|| {
            anchor.move_before(&runtime, &root, &[external], Some(reference));
        }));
        let payload = failure.expect_err("inside-range reference must be rejected");
        let message = payload
            .downcast_ref::<&str>()
            .map(|value| (*value).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        assert!(
            message.contains("must not be inside the moving range"),
            "{label}: unexpected payload {message}"
        );
        assert_eq!(
            runtime.backend().inner().operations().len(),
            before,
            "{label}: rejection performs no backend operations"
        );
        assert_eq!(
            runtime.backend().inner().text_content(&root).as_deref(),
            Some("PXMT")
        );
    }

    let before_move = runtime.backend().inner().operations().len();
    anchor.move_before(&runtime, &root, &[external], Some(&tail));
    assert!(
        runtime.backend().inner().operations().len() > before_move,
        "normal movement still emits backend operations"
    );
    assert_eq!(
        runtime.backend().inner().text_content(&root).as_deref(),
        Some("PMXT")
    );
    assert_eq!(
        runtime.backend().inner().dom_snapshot().nodes.len(),
        baseline
    );
    let ids: Vec<_> = [prefix, middle, tail]
        .iter()
        .map(|node| format!("node-{}", node.0))
        .collect();
    let snapshot = runtime.backend().inner().dom_snapshot();
    for id in ids {
        assert!(snapshot.nodes.iter().any(|node| node.id == id));
    }
}

/// Nested Show inside a keyed item uses a scoped context and cannot mutate the
/// retained host or sibling nodes.
/// keyed 条目内的嵌套 Show 使用受限上下文，不能修改保留的宿主或兄弟节点。
#[test]
fn keyed_for_nested_show_scope_rejects_foreign_mutation() {
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
            "keyed/nested-scope",
            move || read_items.get(),
            move |context: &mut ForContext<MockBackend>, key| {
                if key.as_str() == "a" {
                    let item = context.create_element(Tag::Button);
                    let value = context.create_text("a");
                    context.insert_before(&item, &value, None);
                    return vec![item];
                }
                let item = context.create_element(Tag::Button);
                if key.as_str() == "b" {
                    context.show(
                        &item,
                        None,
                        "keyed/nested-scope/show",
                        || true,
                        move |nested| {
                            nested.set_text(&host_tail, "hacked");
                            nested.create_text("never")
                        },
                    );
                } else {
                    let value = context.create_text(key);
                    context.insert_before(&item, &value, None);
                }
                vec![item]
            },
        )
    };
    let baseline = runtime.backend().dom_snapshot().nodes.len();
    let failure = catch_unwind(AssertUnwindSafe(|| items.set(keys(&["a", "b"]))));
    let payload = failure.expect_err("foreign nested mutation must panic");
    let message = payload
        .downcast_ref::<&str>()
        .map(|value| (*value).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert!(
        message.contains("ForShowContext mutations must target nodes created by this nested range"),
        "unexpected payload: {message}"
    );
    assert_eq!(list.keys(), keys(&["a"]));
    assert_eq!(
        runtime.backend().text_content(&root).as_deref(),
        Some("aTail")
    );
    assert_eq!(runtime.backend().dom_snapshot().nodes.len(), baseline);
    assert_eq!(runtime.backend().listener_count(), 0);

    items.set(keys(&["a", "c"]));
    assert_eq!(list.keys(), keys(&["a", "c"]));
    list.dispose();
    assert_eq!(runtime.backend().dom_snapshot().nodes.len(), 2);
}

/// A second item-anchor create panic disposes the item owner and releases the
/// first anchor while keeping the committed list intact.
/// 第二个条目锚点创建 panic 会释放条目 Owner 与第一个锚点，并保持已提交列表。
#[test]
fn keyed_for_item_anchor_failure_keeps_committed_list() {
    let runtime = Runtime::new(StrictBackend::new());
    let owner = runtime.create_owner();
    let items = owner.signal_named("keyed/items", Vec::<String>::new());
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = {
        let node = runtime.backend_mut().create_text("Tail");
        runtime.backend_mut().insert_before(&root, &node, None);
        node
    };
    let list = {
        let read_items = items.clone();
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/item-anchor-panic",
            move || read_items.get(),
            |context: &mut ForContext<StrictBackend>, key| {
                let item = context.create_element(Tag::Button);
                let value = context.create_text(key);
                context.insert_before(&item, &value, None);
                vec![item]
            },
        )
    };
    let baseline = runtime.backend().inner().dom_snapshot().nodes.len();
    runtime.backend().panic_anchor_after(2);

    let failure = catch_unwind(AssertUnwindSafe(|| items.set(keys(&["a"]))));
    let payload = failure.expect_err("item anchor panic must propagate");
    let message = payload
        .downcast_ref::<&str>()
        .map(|value| (*value).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert_eq!(message, "injected anchor create panic");
    assert!(list.is_empty());
    assert_eq!(
        runtime.backend().inner().dom_snapshot().nodes.len(),
        baseline,
        "the item owner, first anchor, and list anchors leave no extra storage"
    );
    assert_eq!(runtime.backend().inner().listener_count(), 0);

    items.set(keys(&["b"]));
    assert_eq!(list.keys(), keys(&["b"]));
    list.dispose();
    assert_eq!(runtime.backend().inner().dom_snapshot().nodes.len(), 2);
}

/// Initial list-anchor failure releases the first anchor and disposes the
/// control owner, then a later mount succeeds.
/// 初始列表锚点失败会释放第一个锚点并处置 control Owner，之后仍可重新挂载。
#[test]
fn keyed_for_initial_list_anchor_failure_disposes_control() {
    let runtime = Runtime::new(StrictBackend::new());
    let owner = runtime.create_owner();
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = {
        let node = runtime.backend_mut().create_text("Tail");
        runtime.backend_mut().insert_before(&root, &node, None);
        node
    };
    runtime.backend().panic_anchor_after(2);

    let failure = catch_unwind(AssertUnwindSafe(|| {
        let items = Vec::<String>::new();
        For::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "keyed/list-anchor-panic",
            move || items.clone(),
            |context: &mut ForContext<StrictBackend>, _key| vec![context.create_text("never")],
        );
    }));
    let payload = failure.expect_err("list anchor panic must propagate");
    let message = payload
        .downcast_ref::<&str>()
        .map(|value| (*value).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert_eq!(message, "injected anchor create panic");
    assert_eq!(
        runtime.backend().inner().dom_snapshot().nodes.len(),
        2,
        "partial anchors are released and static nodes remain"
    );
    assert!(owner.snapshot().effects.iter().all(|effect| !effect.active));

    let items = Vec::<String>::new();
    let list = For::mount(
        &runtime,
        &owner,
        &root,
        Some(&tail),
        "keyed/list-anchor-retry",
        move || items.clone(),
        |context: &mut ForContext<StrictBackend>, _key| vec![context.create_text("never")],
    );
    list.dispose();
    assert_eq!(runtime.backend().inner().dom_snapshot().nodes.len(), 2);
}

/// Show initial anchor failure also disposes its control owner and leaves no
/// partial anchors.
/// Show 初始锚点失败同样处置 control Owner，不留下部分锚点。
#[test]
fn show_initial_list_anchor_failure_disposes_control() {
    let runtime = Runtime::new(StrictBackend::new());
    let owner = runtime.create_owner();
    let root = runtime.backend_mut().create_element(Tag::Button);
    let tail = {
        let node = runtime.backend_mut().create_text("Tail");
        runtime.backend_mut().insert_before(&root, &node, None);
        node
    };
    runtime.backend().panic_anchor_after(2);

    let failure = catch_unwind(AssertUnwindSafe(|| {
        Show::mount(
            &runtime,
            &owner,
            &root,
            Some(&tail),
            "show/anchor-panic",
            || false,
            |context: &mut fino::show::ShowContext<StrictBackend>| context.create_text("never"),
        );
    }));
    let payload = failure.expect_err("Show anchor panic must propagate");
    let message = payload
        .downcast_ref::<&str>()
        .map(|value| (*value).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert_eq!(message, "injected anchor create panic");
    assert_eq!(
        runtime.backend().inner().dom_snapshot().nodes.len(),
        2,
        "partial anchors are released and static nodes remain"
    );
    assert!(owner.snapshot().effects.iter().all(|effect| !effect.active));

    let show = Show::mount(
        &runtime,
        &owner,
        &root,
        Some(&tail),
        "show/anchor-retry",
        || false,
        |context: &mut fino::show::ShowContext<StrictBackend>| context.create_text("never"),
    );
    show.dispose();
    assert_eq!(runtime.backend().inner().dom_snapshot().nodes.len(), 2);
}
