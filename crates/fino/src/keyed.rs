//! Keyed range reconciliation without virtual-DOM rebuilds.
//! 不通过虚拟 DOM 重建的 keyed 范围协调。
//!
//! The reconciliation shape follows the public algorithm of SolidJS `mapArray`
//! (MIT, `solidjs/solid` commit `bc50fe5649ba4fb1ef33f26643dba56c1c002b56`,
//! `packages/solid/src/reactive/array.ts`): common prefix and suffix, a key to
//! position map, and one disposer per item. Placement keeps the longest
//! increasing subsequence of retained old positions in place, following Vue 3's
//! keyed patch (MIT, `vuejs/core` commit
//! `ff3501429d4970c37194708b055492a56d9e5be5`,
//! `packages/runtime-core/src/renderer.ts`). No source code is copied here.
//! Fino rejects duplicate keys instead of silently matching one occurrence,
//! because a duplicate can only produce a mismatched or half-committed range.
//!
//! 协调结构参考 SolidJS `mapArray` 的公开算法（MIT 许可，`solidjs/solid`
//! 提交 `bc50fe5649ba4fb1ef33f26643dba56c1c002b56`，
//! `packages/solid/src/reactive/array.ts`）：公共前缀与后缀、key 到位置映射，
//! 以及每项一个 disposer。放置阶段保留旧位置的最长递增子序列，沿用 Vue 3
//! keyed patch（MIT，`vuejs/core` 提交
//! `ff3501429d4970c37194708b055492a56d9e5be5`，
//! `packages/runtime-core/src/renderer.ts`）。此处未复制其源码。Fino 选择拒绝
//! 重复 key，而不是静默匹配其中一个出现位置，因为重复 key 只会产生错配或
//! 半提交的范围。

use std::{
    collections::{HashMap, HashSet},
    fmt,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    rc::Rc,
};

use send_wrapper::SendWrapper;

use crate::{
    dom_api::{Attribute, DomBackend, Event, EventContext, EventKind, Tag},
    reactive::Owner,
    runtime::Runtime,
    show::{RangeAnchor, Show, ShowContext, release_after_detach},
    view::with_owner,
};

/// Deterministic duplicate-key rejection reported by a keyed [`For`].
/// 由 keyed [`For`] 报告的确定性重复 key 拒绝结果。
///
/// The code is stable for machine checks. `index` is the position of the
/// second occurrence in the requested key list.
/// `code` 供机器检查使用且保持稳定；`index` 是请求 key 列表中第二次出现的
/// 位置。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForError {
    /// Stable machine-readable code.
    /// 稳定的机器可读错误码。
    pub code: String,
    /// Human-readable explanation.
    /// 供人阅读的错误说明。
    pub message: String,
    /// Position of the duplicate key.
    /// 重复 key 的位置。
    pub index: usize,
}

impl ForError {
    /// Builds the single duplicate-key error shape used by every backend.
    /// 构造每个后端共用的重复 key 错误形状。
    fn duplicate_key(index: usize) -> Self {
        Self {
            code: "UI_FOR_DUPLICATE_KEY".to_owned(),
            message: format!("duplicate key at index {index}"),
            index,
        }
    }
}

impl fmt::Display for ForError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ForError {}

/// Transactional construction context for one keyed item range.
/// 一个 keyed 条目范围使用的事务性构建上下文。
///
/// Nodes created through this context are tracked so a panic in the item
/// builder can release them after disposing the item owner. Effects and
/// listeners registered through it belong to that item owner.
/// 通过该上下文创建的节点会被跟踪，因此条目构建器 panic 时可以在释放条目
/// Owner 后回收它们。通过它注册的 Effect 与监听属于该条目 Owner。
pub struct ForContext<B: DomBackend + 'static> {
    inner: ShowContext<B>,
    owned: Rc<std::cell::RefCell<Vec<B::Node>>>,
}

/// Restricted effect context for one keyed item.
/// 一个 keyed 条目使用的受限 Effect 上下文。
///
/// Only nodes created by the same item may be mutated. The context intentionally
/// exposes no shared runtime or raw backend borrow.
/// 只能修改同一条目创建的节点。该上下文刻意不暴露共享 Runtime 或原始后端借用。
pub struct ForEffectContext<B: DomBackend + 'static> {
    runtime: Runtime<B>,
    owned: Rc<std::cell::RefCell<Vec<B::Node>>>,
}

impl<B: DomBackend + 'static> ForEffectContext<B> {
    fn assert_owned(&self, node: &B::Node) {
        assert!(
            self.owned.borrow().iter().any(|owned| owned == node),
            "ForEffectContext mutations must target nodes created by this item"
        );
    }

    /// Replaces one text node created by this item.
    /// 替换本条目创建的一个文本节点。
    pub fn set_text(&mut self, node: &B::Node, value: &str) {
        self.assert_owned(node);
        self.runtime.backend_mut().set_text(node, value);
    }

    /// Sets or removes an attribute on an item-created element.
    /// 在条目创建的元素上设置或移除属性。
    pub fn set_attribute(&mut self, node: &B::Node, attribute: Attribute, value: Option<&str>) {
        self.assert_owned(node);
        self.runtime
            .backend_mut()
            .set_attribute(node, attribute, value);
    }

    /// Moves an item-created node inside the same item range.
    /// 在同一条目范围内移动一个条目创建节点。
    pub fn insert_before(&mut self, parent: &B::Node, child: &B::Node, before: Option<&B::Node>) {
        self.assert_owned(parent);
        self.assert_owned(child);
        if let Some(before) = before {
            self.assert_owned(before);
        }
        self.runtime
            .backend_mut()
            .insert_before(parent, child, before);
    }
}

impl<B: DomBackend + 'static> fmt::Debug for ForEffectContext<B> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ForEffectContext")
            .finish_non_exhaustive()
    }
}

impl<B: DomBackend + 'static> ForContext<B> {
    fn new(runtime: Runtime<B>, owner: Owner) -> Self {
        Self {
            inner: ShowContext::new(runtime, owner),
            owned: Rc::new(std::cell::RefCell::new(Vec::new())),
        }
    }

    /// Returns the child owner for this item range.
    /// 返回该条目范围的子 Owner。
    pub fn owner(&self) -> &Owner {
        self.inner.owner()
    }

    /// Registers cleanup that runs when this item owner is disposed.
    /// 注册在该条目 Owner 释放时执行的清理。
    ///
    /// The callback can remove probe state or other per-item bookkeeping. It
    /// runs during item owner disposal.
    /// 回调可以清除探针状态或其它按条目维护的数据；它在条目 Owner 释放期间
    /// 运行。
    pub fn on_dispose(&self, cleanup: impl FnOnce() + 'static) {
        self.inner.owner().on_dispose(cleanup);
    }

    /// Rejects a mutation that targets a node outside this item transaction.
    /// 拒绝目标不在本条事务内的变更。
    fn assert_owned(&self, node: &B::Node) {
        assert!(
            self.owned.borrow().iter().any(|owned| owned == node),
            "ForContext mutations must target nodes created by this item"
        );
    }

    /// Creates a tracked element node.
    /// 创建一个受跟踪的元素节点。
    pub fn create_element(&mut self, tag: Tag) -> B::Node {
        let node = self.inner.create_element(tag);
        self.owned.borrow_mut().push(node.clone());
        node
    }

    /// Creates a tracked text node.
    /// 创建一个受跟踪的文本节点。
    pub fn create_text(&mut self, value: &str) -> B::Node {
        let node = self.inner.create_text(value);
        self.owned.borrow_mut().push(node.clone());
        node
    }

    /// Creates a tracked anchor node.
    /// 创建一个受跟踪的锚点节点。
    pub fn create_anchor(&mut self) -> B::Node {
        let node = self.inner.create_anchor();
        self.owned.borrow_mut().push(node.clone());
        node
    }

    /// Inserts one node relative to an optional sibling created by this item.
    /// 把一个节点插入到本条目创建的某个可选兄弟节点之前。
    ///
    /// Both the parent and the child must have been created through this
    /// context; foreign mounted nodes are rejected before mutation.
    /// 父节点与子节点都必须通过本上下文创建；外部已挂载节点会在变更前被拒绝。
    pub fn insert_before(&mut self, parent: &B::Node, child: &B::Node, before: Option<&B::Node>) {
        self.assert_owned(parent);
        self.assert_owned(child);
        if let Some(before) = before {
            self.assert_owned(before);
        }
        self.inner.insert_before(parent, child, before);
    }

    /// Sets or removes a supported attribute on an item-created element.
    /// 在条目创建的元素上设置或移除受支持的属性。
    pub fn set_attribute(&mut self, node: &B::Node, attribute: Attribute, value: Option<&str>) {
        self.assert_owned(node);
        self.inner.set_attribute(node, attribute, value);
    }

    /// Replaces one text node created by this item.
    /// 替换本条目创建的一个文本节点。
    pub fn set_text(&mut self, node: &B::Node, value: &str) {
        self.assert_owned(node);
        self.inner.set_text(node, value);
    }

    /// Registers an event listener on an item-created node.
    /// 在条目创建的节点上注册监听。
    pub fn listen(
        &mut self,
        node: &B::Node,
        event: EventKind,
        callback: impl FnMut(Event, &mut EventContext) + 'static,
    ) {
        self.assert_owned(node);
        self.inner.listen(node, event, callback);
    }

    /// Registers an effect owned by this item.
    /// 注册一个由该条目持有的 Effect。
    ///
    /// The callback receives [`ForEffectContext`], which only mutates nodes
    /// created by the same item. The owner stops the effect before the item
    /// range is released.
    /// 回调接收 [`ForEffectContext`]，它只能修改同一条目创建的节点。范围释放前
    /// Owner 会先停止 Effect。
    pub fn effect(
        &mut self,
        name: &str,
        mut callback: impl FnMut(&mut ForEffectContext<B>) + 'static,
    ) {
        let runtime = self.inner.runtime().clone();
        let owned = Rc::clone(&self.owned);
        self.inner.effect(name, move |_context| {
            let mut effect = ForEffectContext {
                runtime: runtime.clone(),
                owned: Rc::clone(&owned),
            };
            callback(&mut effect);
        });
    }

    /// Mounts a nested Show inside this item range.
    /// 在该条目范围内挂载一个嵌套 Show。
    pub fn show<C, F>(
        &mut self,
        parent: &B::Node,
        before: Option<&B::Node>,
        name: &str,
        condition: C,
        content: F,
    ) -> Show<B>
    where
        C: Fn() -> bool + 'static,
        F: FnMut(&mut ShowContext<B>) -> B::Node + 'static,
    {
        self.assert_owned(parent);
        if let Some(before) = before {
            self.assert_owned(before);
        }
        self.inner.show(parent, before, name, condition, content)
    }
}

impl<B: DomBackend + 'static> fmt::Debug for ForContext<B> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("ForContext").finish_non_exhaustive()
    }
}

/// One mounted keyed list item with a stable range and child owner.
/// 一个已挂载的 keyed 列表条目，拥有稳定范围与子 Owner。
struct ForEntry<B: DomBackend + 'static, K> {
    key: K,
    owner: Owner,
    anchors: RangeAnchor<B>,
    nodes: Vec<B::Node>,
    created: Vec<B::Node>,
}

impl<B: DomBackend + 'static, K> ForEntry<B, K> {
    /// Stops the item owner, then releases every created node and external node.
    /// 先停止条目 Owner，再释放全部已创建节点与外部节点。
    fn dispose(self, runtime: &Runtime<B>) {
        self.owner.dispose();
        let mut nodes = self.created;
        nodes.extend(self.nodes);
        release_after_detach(runtime, &nodes);
    }

    /// Moves the complete item range before one reference node.
    /// 把完整条目范围移动到单个参考节点之前。
    fn move_before(&self, runtime: &Runtime<B>, parent: &B::Node, before: &B::Node) {
        self.anchors
            .move_before(runtime, parent, &self.nodes, Some(before));
    }
}

type ItemsReader<K> = Box<dyn Fn() -> Vec<K>>;
type ContentBuilder<B, K> = Box<dyn FnMut(&mut ForContext<B>, &K) -> Vec<<B as DomBackend>::Node>>;

/// A keyed dynamic range mounted between stable [`RangeAnchor`] nodes.
/// 挂载在稳定 [`RangeAnchor`] 之间的一段 keyed 动态范围。
///
/// Each key owns one range and one child owner. Reordering moves the existing
/// ranges with `insert_before`; it never clears the parent, assigns
/// `innerHTML`, or reruns the content builder for a retained key.
/// 每个 key 拥有一个范围与子 Owner。重排使用 `insert_before` 移动既有范围，
/// 绝不清空父节点、写入 `innerHTML`，也不会为保留下来的 key 重跑内容构建器。
///
/// The key list is read in a `{name}/reconcile` effect. Invalid lists are
/// rejected atomically: a duplicate key leaves every mounted range untouched
/// and exposes [`For::last_error`]. The initial [`For::try_mount`] returns the
/// same error before creating the For anchors when the first read is duplicate.
/// key 列表在 `{name}/reconcile` Effect 中读取。非法列表会被原子拒绝：出现
/// 重复 key 时所有已挂载范围保持不变，并通过 [`For::last_error`] 暴露错误。
/// 首次读取即重复时，[`For::try_mount`] 会在创建 For 锚点前返回同一错误。
///
/// Dropping the handle does not unmount the list; the parent owner keeps it
/// alive. Call [`For::dispose`] or dispose the parent owner to release it.
/// 丢弃句柄不会卸载列表；父 Owner 会继续保活。调用 [`For::dispose`] 或释放
/// 父 Owner 可提前释放。
pub struct For<B: DomBackend + 'static, K: Clone + Eq + std::hash::Hash + 'static> {
    state: Rc<std::cell::RefCell<ForState<B, K>>>,
}

impl<B: DomBackend + 'static, K: Clone + Eq + std::hash::Hash + 'static> fmt::Debug for For<B, K> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("For").finish_non_exhaustive()
    }
}

impl<B: DomBackend + 'static, K: Clone + Eq + std::hash::Hash + 'static> For<B, K> {
    /// Mounts a keyed list, panicking on an initial duplicate key.
    /// 挂载 keyed 列表；初始 key 重复时 panic。
    ///
    /// The panic happens before the For anchors are created when the first
    /// read already contains a duplicate. Use [`For::try_mount`] to receive the
    /// structured error instead.
    /// 首次读取已包含重复时，panic 发生在创建 For 锚点之前。需要结构化错误
    /// 时请使用 [`For::try_mount`]。
    pub fn mount<C, F>(
        runtime: &Runtime<B>,
        owner: &Owner,
        parent: &B::Node,
        before: Option<&B::Node>,
        name: &str,
        items: C,
        content: F,
    ) -> Self
    where
        C: Fn() -> Vec<K> + 'static,
        F: FnMut(&mut ForContext<B>, &K) -> Vec<B::Node> + 'static,
    {
        match Self::try_mount(runtime, owner, parent, before, name, items, content) {
            Ok(list) => list,
            Err(error) => panic!("{error}"),
        }
    }

    /// Mounts a keyed list and reports an initial duplicate key as an error.
    /// 挂载 keyed 列表，并把初始重复 key 作为错误报告。
    pub fn try_mount<C, F>(
        runtime: &Runtime<B>,
        owner: &Owner,
        parent: &B::Node,
        before: Option<&B::Node>,
        name: &str,
        items: C,
        content: F,
    ) -> Result<Self, ForError>
    where
        C: Fn() -> Vec<K> + 'static,
        F: FnMut(&mut ForContext<B>, &K) -> Vec<B::Node> + 'static,
    {
        // Validate the first read before creating anchors or owners. The effect
        // below performs the tracked read; this preflight stays untracked.
        // 在创建锚点或 Owner 前校验首次读取。下方 Effect 才执行受跟踪读取，
        // 这次预检保持不跟踪。
        validate_keys(&crate::reactive::untracked(&items))?;

        let control = owner.child();
        let anchors = RangeAnchor::create(runtime, parent, before);
        let state = Rc::new(std::cell::RefCell::new(ForState {
            runtime: runtime.clone(),
            control: control.clone(),
            parent: parent.clone(),
            anchors,
            items: Box::new(items),
            content: Box::new(content),
            entries: Vec::new(),
            last_keys: None,
            last_error: None,
            cleaned: false,
        }));

        let effect_state = Rc::clone(&state);
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            runtime.effect_with_runtime(
                &control,
                &format!("{name}/reconcile"),
                move |_context, runtime| {
                    effect_state.borrow_mut().update(runtime);
                },
            );
        }));
        if let Err(payload) = outcome {
            // A failed initial build must not leak the For owner or anchors.
            // 初始构建失败不得泄漏 For Owner 或锚点。
            if let Ok(mut state) = state.try_borrow_mut() {
                state.cleanup(false);
            }
            control.dispose();
            resume_unwind(payload);
        }

        let initial_error = state.borrow().last_error.clone();
        if let Some(error) = initial_error {
            if let Ok(mut state) = state.try_borrow_mut() {
                state.cleanup(false);
            }
            control.dispose();
            return Err(error);
        }

        let cleanup_state = SendWrapper::new(Rc::downgrade(&state));
        control.register_cleanup(move || {
            if let Some(state) = cleanup_state.upgrade()
                && let Ok(mut state) = state.try_borrow_mut()
            {
                state.cleanup(false);
            }
        });

        Ok(Self { state })
    }

    /// Returns the stable start anchor of the whole list.
    /// 返回整个列表的稳定起始锚点。
    pub fn start(&self) -> B::Node {
        self.state.borrow().anchors.start().clone()
    }

    /// Returns the stable end anchor of the whole list.
    /// 返回整个列表的稳定结束锚点。
    pub fn end(&self) -> B::Node {
        self.state.borrow().anchors.end().clone()
    }

    /// Returns a copy of the stable anchor pair for the whole list.
    /// 返回整个列表的稳定锚点对副本。
    pub fn anchors(&self) -> RangeAnchor<B> {
        self.state.borrow().anchors.clone()
    }

    /// Returns the number of mounted keys.
    /// 返回已挂载 key 的数量。
    pub fn len(&self) -> usize {
        self.state.borrow().entries.len()
    }

    /// Returns whether no key is currently mounted.
    /// 返回当前是否没有挂载任何 key。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the mounted keys in committed order.
    /// 按已提交顺序返回挂载的 key。
    pub fn keys(&self) -> Vec<K> {
        self.state
            .borrow()
            .entries
            .iter()
            .map(|entry| entry.key.clone())
            .collect()
    }

    /// Returns the last duplicate-key rejection, cleared by the next success.
    /// 返回最近一次重复 key 拒绝；下一次成功后清除。
    pub fn last_error(&self) -> Option<ForError> {
        self.state.borrow().last_error.clone()
    }

    /// Releases every item and the list anchors; repeated calls are safe.
    /// 释放所有条目与列表锚点；可安全重复调用。
    pub fn dispose(&self) {
        let control = {
            let Ok(mut state) = self.state.try_borrow_mut() else {
                return;
            };
            state.cleanup(false);
            state.control.clone()
        };
        control.dispose();
    }
}

/// One reconciliation state with its own transaction guard and error slot.
/// 一次协调使用的状态，包含事务保护与错误槽。
struct ForState<B: DomBackend + 'static, K: Clone + Eq + std::hash::Hash + 'static> {
    runtime: Runtime<B>,
    control: Owner,
    parent: B::Node,
    anchors: RangeAnchor<B>,
    items: ItemsReader<K>,
    content: ContentBuilder<B, K>,
    entries: Vec<ForEntry<B, K>>,
    last_keys: Option<Vec<K>>,
    last_error: Option<ForError>,
    cleaned: bool,
}

impl<B: DomBackend + 'static, K: Clone + Eq + std::hash::Hash + 'static> ForState<B, K> {
    /// Reads the key list and applies only a valid structural change.
    /// 读取 key 列表，并只应用合法的结构变更。
    fn update(&mut self, runtime: &mut Runtime<B>) {
        if self.cleaned {
            return;
        }
        let next = (self.items)();
        if let Err(error) = validate_keys(&next) {
            // Reject before any backend mutation, keeping the committed list.
            // 在任何后端变更前拒绝，保留已提交列表。
            self.last_error = Some(error);
            return;
        }
        if self.last_keys.as_ref() == Some(&next) {
            self.last_error = None;
            return;
        }
        self.reconcile(next, runtime);
    }

    /// Reuses retained ranges, creates only new keys, and moves whole ranges.
    /// 复用保留范围，只创建新 key，并整体移动范围。
    fn reconcile(&mut self, new_keys: Vec<K>, runtime: &mut Runtime<B>) {
        let old_len = self.entries.len();
        let new_len = new_keys.len();

        // Common prefix, then common suffix outside the prefix.
        // 先找公共前缀，再在前缀之外找公共后缀。
        let mut prefix = 0;
        while prefix < old_len.min(new_len) && self.entries[prefix].key == new_keys[prefix] {
            prefix += 1;
        }
        let mut suffix = 0;
        while suffix
            < old_len
                .saturating_sub(prefix)
                .min(new_len.saturating_sub(prefix))
            && self.entries[old_len - 1 - suffix].key == new_keys[new_len - 1 - suffix]
        {
            suffix += 1;
        }

        let old_mid = &self.entries[prefix..old_len - suffix];
        let new_mid = &new_keys[prefix..new_len - suffix];
        let old_mid_keys: HashSet<K> = old_mid.iter().map(|entry| entry.key.clone()).collect();
        let new_mid_keys: HashSet<K> = new_mid.iter().cloned().collect();

        // Build every new item before mutating any existing range. A panic here
        // disposes already built items and leaves the committed list untouched.
        // 在改动任何既有范围前构建全部新条目。此处 panic 会释放已构建条目，
        // 并保持已提交列表不变。
        let mut built: Vec<Option<ForEntry<B, K>>> = (0..new_mid.len()).map(|_| None).collect();
        let build = catch_unwind(AssertUnwindSafe(|| {
            for (index, key) in new_mid.iter().enumerate() {
                if !old_mid_keys.contains(key) {
                    built[index] = Some(self.build_entry(key.clone()));
                }
            }
        }));
        if let Err(payload) = build {
            for entry in built.into_iter().flatten() {
                entry.dispose(runtime);
            }
            resume_unwind(payload);
        }

        // Commit: release removed entries in old order, then reuse the rest.
        // 提交：按旧顺序释放被移除条目，然后复用其余条目。
        let old_entries = std::mem::take(&mut self.entries);
        let mut old_iter = old_entries.into_iter();
        let prefix_entries: Vec<_> = old_iter.by_ref().take(prefix).collect();
        let middle_entries: Vec<_> = old_iter.by_ref().take(old_len - prefix - suffix).collect();
        let suffix_entries: Vec<_> = old_iter.collect();

        let mut retained = HashMap::with_capacity(middle_entries.len());
        for (old_index, entry) in middle_entries.into_iter().enumerate() {
            if new_mid_keys.contains(&entry.key) {
                retained.insert(entry.key.clone(), (old_index, entry));
            } else {
                entry.dispose(runtime);
            }
        }

        let mut middle_final = Vec::with_capacity(new_mid.len());
        let mut old_positions = Vec::with_capacity(new_mid.len());
        let mut built_iter = built.into_iter();
        for key in new_mid {
            match built_iter.next().flatten() {
                Some(entry) => {
                    middle_final.push(entry);
                    old_positions.push(None);
                }
                None => {
                    let (old_index, entry) = retained
                        .remove(key)
                        .expect("a retained key must keep its range");
                    middle_final.push(entry);
                    old_positions.push(Some(old_index));
                }
            }
        }
        debug_assert!(retained.is_empty(), "all requested keys are resolved");

        // Keep the longest increasing subsequence of retained old positions in
        // place and move only the remaining ranges. This is the same stable
        // subsequence idea as Vue 3's keyed patch (MIT, vuejs/core commit
        // ff3501429d4970c37194708b055492a56d9e5be5,
        // packages/runtime-core/src/renderer.ts); new nodes use the 0 sentinel.
        // 保留旧位置最长递增子序列中的范围，只移动其余范围。这与 Vue 3 keyed
        // patch 的稳定子序列思路一致（MIT，vuejs/core 提交
        // ff3501429d4970c37194708b055492a56d9e5be5，
        // packages/runtime-core/src/renderer.ts）；新节点使用 0 哨兵。
        let sequence: Vec<usize> = old_positions
            .iter()
            .map(|position| position.map_or(0, |old_index| old_index + 1))
            .collect();
        let stable = stable_subsequence(&sequence);

        // Reposition from right to left. Stable retained ranges keep their DOM
        // position; new or unstable ranges move before the next final item.
        // Prefix and suffix ranges never move.
        // 从右向左重排。稳定的保留范围保持 DOM 位置；新范围或不稳定范围移动到
        // 下一个最终条目之前。前后两段绝不移动。
        let mut reference = suffix_entries
            .first()
            .map(|entry| entry.anchors.start().clone())
            .unwrap_or_else(|| self.anchors.end().clone());
        for index in (0..middle_final.len()).rev() {
            let entry = &middle_final[index];
            if old_positions[index].is_none() || !stable[index] {
                entry.move_before(runtime, &self.parent, &reference);
            }
            reference = entry.anchors.start().clone();
        }

        self.entries = prefix_entries;
        self.entries.extend(middle_final);
        self.entries.extend(suffix_entries);
        self.last_keys = Some(new_keys);
        self.last_error = None;
    }

    /// Builds one child owner and item range inside a panic transaction.
    /// 在一个 panic 事务内构建一个子 Owner 与条目范围。
    ///
    /// Two tracked anchors bracket the item. When the builder panics, the child
    /// owner is disposed first and every tracked creation is released before
    /// the unwind resumes, so no listener, effect, node, or owner survives.
    /// 两个受跟踪锚点包围条目。构建器 panic 时，先释放子 Owner，再在继续
    /// 展开前回收全部受跟踪创建，因此监听、Effect、节点与 Owner 都不会残留。
    fn build_entry(&mut self, key: K) -> ForEntry<B, K> {
        let owner = self.control.child();
        let mut context = ForContext::new(self.runtime.clone(), owner.clone());
        let start = context.create_anchor();
        let end = context.create_anchor();
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            with_owner(&owner, || (self.content)(&mut context, &key))
        }));
        match outcome {
            Ok(nodes) => {
                let created = context.inner.take_created();
                ForEntry {
                    key,
                    owner,
                    anchors: RangeAnchor::from_parts(start, end),
                    nodes,
                    created,
                }
            }
            Err(payload) => {
                let created = context.inner.take_created();
                owner.dispose();
                release_after_detach(&self.runtime, &created);
                resume_unwind(payload);
            }
        }
    }

    /// Releases the committed ranges, the list anchors, and optionally the owner.
    /// 释放已提交范围、列表锚点，并按需释放 Owner。
    fn cleanup(&mut self, dispose_control: bool) {
        if self.cleaned {
            return;
        }
        self.cleaned = true;
        let runtime = self.runtime.clone();
        let entries = std::mem::take(&mut self.entries);
        for entry in entries.into_iter().rev() {
            entry.dispose(&runtime);
        }
        self.anchors.remove_anchors(&runtime);
        if dispose_control {
            self.control.dispose();
        }
    }
}

impl<B: DomBackend + 'static, K: Clone + Eq + std::hash::Hash + 'static> Drop for ForState<B, K> {
    fn drop(&mut self) {
        self.cleanup(true);
    }
}

/// Marks one longest increasing subsequence; zero entries are new nodes.
/// 标记一个最长递增子序列；零表示新节点。
///
/// The Rust implementation follows the stable-subsequence placement used by
/// Vue 3's `getSequence` (MIT, vuejs/core commit
/// `ff3501429d4970c37194708b055492a56d9e5be5`,
/// `packages/runtime-core/src/renderer.ts`). It returns a flag per element so
/// the caller can keep retained ranges in place and move only the rest.
/// Rust 实现沿用 Vue 3 `getSequence` 使用的稳定子序列放置策略（MIT，
/// vuejs/core 提交 `ff3501429d4970c37194708b055492a56d9e5be5`，
/// `packages/runtime-core/src/renderer.ts`）。它按元素返回标记，使调用方
/// 保留既有范围、只移动其余范围。
fn stable_subsequence(sequence: &[usize]) -> Vec<bool> {
    let mut predecessors: Vec<Option<usize>> = vec![None; sequence.len()];
    let mut tails: Vec<usize> = Vec::new();
    for (index, value) in sequence.iter().copied().enumerate() {
        if value == 0 {
            continue;
        }
        let mut low = 0;
        let mut high = tails.len();
        while low < high {
            let middle = (low + high) / 2;
            if sequence[tails[middle]] < value {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        if low > 0 {
            predecessors[index] = Some(tails[low - 1]);
        }
        if low == tails.len() {
            tails.push(index);
        } else {
            tails[low] = index;
        }
    }

    let mut stable = vec![false; sequence.len()];
    let mut current = tails.last().copied();
    while let Some(index) = current {
        stable[index] = true;
        current = predecessors[index];
    }
    stable
}

/// Rejects the first repeated key before any structural mutation.
/// 在任何结构变更前拒绝第一个重复 key。
fn validate_keys<K: Clone + Eq + std::hash::Hash>(keys: &[K]) -> Result<(), ForError> {
    let mut seen = HashSet::with_capacity(keys.len());
    for (index, key) in keys.iter().enumerate() {
        if !seen.insert(key.clone()) {
            return Err(ForError::duplicate_key(index));
        }
    }
    Ok(())
}
