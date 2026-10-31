//! Range-anchored conditional mounting without virtual-DOM reruns.
//! 使用范围锚点的条件挂载，不触发虚拟 DOM 式的整树重跑。

use std::{
    cell::{Cell, RefCell},
    fmt,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    rc::Rc,
};

use send_wrapper::SendWrapper;

use crate::{
    dom_api::{Attribute, DomBackend, Event, EventContext, EventKind, Tag},
    reactive::Owner,
    runtime::{EffectContext, Runtime},
    view::with_owner,
};

/// A stable start/end pair that marks one dynamic child range.
/// 标记一个动态子范围的一对稳定起止锚点。
///
/// The anchors are ordinary backend nodes, so their handles survive every
/// condition change while the content between them does not.
/// 锚点是普通后端节点，因此它们的句柄在每次条件切换后保持不变，而两者之间的内容不会。
pub struct RangeAnchor<B: DomBackend> {
    start: B::Node,
    end: B::Node,
}

impl<B: DomBackend> Clone for RangeAnchor<B> {
    fn clone(&self) -> Self {
        Self {
            start: self.start.clone(),
            end: self.end.clone(),
        }
    }
}

impl<B: DomBackend + 'static> RangeAnchor<B> {
    /// Creates the anchor pair and inserts it before an optional stable sibling.
    /// 创建锚点对，并把它插入到可选稳定兄弟节点之前。
    pub fn create(runtime: &Runtime<B>, parent: &B::Node, before: Option<&B::Node>) -> Self {
        let start = runtime.backend_mut().create_anchor();
        let end = runtime.backend_mut().create_anchor();
        runtime.backend_mut().insert_before(parent, &start, before);
        runtime.backend_mut().insert_before(parent, &end, before);
        Self { start, end }
    }

    /// Returns the first anchor that starts the dynamic range.
    /// 返回动态范围的起始锚点。
    pub fn start(&self) -> &B::Node {
        &self.start
    }

    /// Returns the last anchor that ends the dynamic range.
    /// 返回动态范围的结束锚点。
    pub fn end(&self) -> &B::Node {
        &self.end
    }

    /// Builds a detached anchor pair from two created anchors.
    /// 用两个已创建锚点构造一个尚未挂载的锚点对。
    pub(crate) fn from_parts(start: B::Node, end: B::Node) -> Self {
        Self { start, end }
    }

    /// Inserts one node inside the range, immediately before the end anchor.
    /// 把一个节点插入范围内部，紧邻结束锚点之前。
    pub fn insert(&self, runtime: &Runtime<B>, parent: &B::Node, node: &B::Node) {
        runtime
            .backend_mut()
            .insert_before(parent, node, Some(&self.end));
    }

    /// Moves the complete range, anchors included, before one reference node.
    /// 把包含锚点的完整范围移动到单个参考节点之前。
    ///
    /// Moving every external node in order keeps the range contiguous without
    /// clearing the parent or recreating children.
    /// 按顺序移动每个外部节点，使范围保持连续，同时不清空父节点或重建子节点。
    pub fn move_before(
        &self,
        runtime: &Runtime<B>,
        parent: &B::Node,
        nodes: &[B::Node],
        before: Option<&B::Node>,
    ) {
        runtime
            .backend_mut()
            .insert_before(parent, &self.start, before);
        for node in nodes {
            runtime.backend_mut().insert_before(parent, node, before);
        }
        runtime
            .backend_mut()
            .insert_before(parent, &self.end, before);
    }

    /// Removes one node from the range without touching either anchor.
    /// 从范围内移除一个节点，不触碰两个锚点。
    pub fn remove(&self, runtime: &Runtime<B>, node: &B::Node) {
        runtime.backend_mut().remove(node);
    }

    /// Removes both anchors after the range has been released.
    /// 在范围释放后移除两个锚点。
    pub fn remove_anchors(&self, runtime: &Runtime<B>) {
        runtime.backend_mut().release(&self.end);
        runtime.backend_mut().release(&self.start);
    }
}

impl<B: DomBackend> fmt::Debug for RangeAnchor<B> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RangeAnchor")
            .finish_non_exhaustive()
    }
}

/// Detaches tracked nodes from each other, then releases each unique node once.
/// 先把受跟踪节点彼此分离，再对每个唯一节点释放一次。
///
/// `remove` stays non-destructive; `release` stays terminal. Detaching first
/// prevents a recursive parent release from freeing a descendant that is still
/// in the tracked list and would otherwise be released a second time.
/// `remove` 仍只做非破坏性分离，`release` 仍是终止释放。先分离可避免父节点
/// 递归释放已经跟踪、随后又会被再次释放的后代。
pub(crate) fn release_after_detach<B: DomBackend + 'static>(
    runtime: &Runtime<B>,
    nodes: &[B::Node],
) {
    let mut unique: Vec<B::Node> = Vec::new();
    for node in nodes {
        if !unique.iter().any(|existing| existing == node) {
            unique.push(node.clone());
        }
    }
    for node in &unique {
        runtime.backend_mut().remove(node);
    }
    for node in &unique {
        runtime.backend_mut().release(node);
    }
}

/// Stops an owner, then detaches and releases every tracked node exactly once.
/// 停止一个 Owner，然后分离并逐个释放全部受跟踪节点。
///
/// If `first_panic` already exists, it always wins. Otherwise an owner cleanup
/// panic wins over a secondary release panic. Node release still runs for every
/// case, so a user `on_dispose` panic cannot orphan backend ranges.
/// 若已有 `first_panic`，始终保留它。否则 Owner 清理 panic 优先于二次 release
/// panic。所有情况下都会继续释放节点，因此用户 `on_dispose` panic 不会留下
/// 孤儿后端范围。
pub(crate) fn terminate_owner_and_nodes<B: DomBackend + 'static>(
    owner: Owner,
    nodes: Vec<B::Node>,
    runtime: &Runtime<B>,
    first_panic: Option<Box<dyn std::any::Any + Send>>,
) -> Option<Box<dyn std::any::Any + Send>> {
    let owner_panic = catch_unwind(AssertUnwindSafe(|| owner.dispose())).err();
    let release_panic =
        catch_unwind(AssertUnwindSafe(|| release_after_detach(runtime, &nodes))).err();
    first_panic.or(owner_panic).or(release_panic)
}

/// Transactional construction context for one [`Show`] content mount.
/// 一次 [`Show`] 内容挂载使用的事务性构建上下文。
///
/// Nodes created through this context are tracked so a panic in the content
/// builder can release them after disposing the child owner. Effects and
/// listeners registered through it belong to the child owner.
/// 通过该上下文创建的节点会被跟踪，因此内容构建器 panic 时可以在释放子
/// Owner 后回收它们。通过它注册的 Effect 与监听属于子 Owner。
pub struct ShowContext<B: DomBackend + 'static> {
    runtime: Runtime<B>,
    owner: Owner,
    created: Vec<B::Node>,
}

impl<B: DomBackend + 'static> ShowContext<B> {
    /// Creates the transactional context shared by Show and keyed ranges.
    /// 创建 Show 与 keyed 范围共用的挂载事务上下文。
    pub(crate) fn new(runtime: Runtime<B>, owner: Owner) -> Self {
        Self {
            runtime,
            owner,
            created: Vec::new(),
        }
    }

    /// Takes every node created through this context for terminal cleanup.
    /// 取走通过该上下文创建的全部节点，供终止清理使用。
    pub(crate) fn take_created(&mut self) -> Vec<B::Node> {
        std::mem::take(&mut self.created)
    }

    /// Returns the shared runtime for crate-internal range assembly.
    /// 返回 crate 内部范围组装使用的共享 Runtime。
    pub(crate) fn runtime(&self) -> &Runtime<B> {
        &self.runtime
    }

    /// Returns the child owner for this range.
    /// 返回该范围的子 Owner。
    pub fn owner(&self) -> &Owner {
        &self.owner
    }

    /// Creates a tracked element node.
    /// 创建一个受跟踪的元素节点。
    pub fn create_element(&mut self, tag: Tag) -> B::Node {
        let node = self.runtime.backend_mut().create_element(tag);
        self.created.push(node.clone());
        node
    }

    /// Creates a tracked text node.
    /// 创建一个受跟踪的文本节点。
    pub fn create_text(&mut self, value: &str) -> B::Node {
        let node = self.runtime.backend_mut().create_text(value);
        self.created.push(node.clone());
        node
    }

    /// Creates a tracked range anchor.
    /// 创建一个受跟踪的范围锚点。
    pub fn create_anchor(&mut self) -> B::Node {
        let node = self.runtime.backend_mut().create_anchor();
        self.created.push(node.clone());
        node
    }

    /// Inserts one node relative to an optional stable sibling.
    /// 把一个节点插入到可选稳定兄弟节点之前。
    pub fn insert_before(&mut self, parent: &B::Node, child: &B::Node, before: Option<&B::Node>) {
        self.runtime
            .backend_mut()
            .insert_before(parent, child, before);
    }

    /// Sets or removes a supported attribute.
    /// 设置或移除受支持的属性。
    pub fn set_attribute(&mut self, node: &B::Node, attribute: Attribute, value: Option<&str>) {
        self.runtime
            .backend_mut()
            .set_attribute(node, attribute, value);
    }

    /// Replaces one existing text node.
    /// 替换一个现有文本节点的内容。
    pub fn set_text(&mut self, node: &B::Node, value: &str) {
        self.runtime.backend_mut().set_text(node, value);
    }

    /// Registers an event listener owned by this range.
    /// 注册一个由该范围持有的监听。
    pub fn listen(
        &mut self,
        node: &B::Node,
        event: EventKind,
        callback: impl FnMut(Event, &mut EventContext) + 'static,
    ) {
        self.runtime.listen(&self.owner, node, event, callback);
    }

    /// Registers an effect owned by this range.
    /// 注册一个由该范围持有的 Effect。
    pub fn effect(&mut self, name: &str, callback: impl FnMut(&mut EffectContext<B>) + 'static) {
        self.runtime.effect(&self.owner, name, callback);
    }

    /// Mounts a nested Show using this range as its parent owner.
    /// 以该范围作为父 Owner 挂载一个嵌套 Show。
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
        Show::mount(
            &self.runtime,
            &self.owner,
            parent,
            before,
            name,
            condition,
            content,
        )
    }
}

/// Conditional dynamic range mounted between two [`RangeAnchor`] nodes.
/// 挂载在两个 [`RangeAnchor`] 之间的条件动态范围。
///
/// `Show` reads its condition in an owner-scoped effect. Only a real `false`
/// to `true` or `true` to `false` transition creates or removes the range;
/// equal writes remain backend no-ops. Static siblings and existing text nodes
/// are never recreated.
/// `Show` 在 Owner 作用域 Effect 中读取条件。只有真正的 `false` 到 `true`
/// 或 `true` 到 `false` 切换才会创建或移除范围；写入相同值不会触达后端。
/// 静态兄弟节点与既有文本节点绝不重建。
///
/// Unmounting disposes the child owner (which stops listeners and effects)
/// before removing content nodes from the parent.
/// 卸载时先释放子 Owner（停止监听与 Effect），再从父节点移除内容节点。
///
/// Dropping the returned handle does not unmount the range; the parent owner
/// keeps it alive. Call [`Show::dispose`] or dispose the parent owner to
/// release it early.
/// 丢弃返回句柄不会卸载范围；父 Owner 会继续保活。调用 [`Show::dispose`]
/// 或释放父 Owner 可提前释放。
pub struct Show<B: DomBackend + 'static> {
    state: Rc<RefCell<ShowState<B>>>,
    dispose_requested: Rc<Cell<bool>>,
}

impl<B: DomBackend> fmt::Debug for Show<B> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Show").finish_non_exhaustive()
    }
}

impl<B: DomBackend + 'static> Show<B> {
    /// Mounts one conditional node between fresh anchors.
    /// 在新锚点之间挂载一个条件节点。
    pub fn mount<C, F>(
        runtime: &Runtime<B>,
        owner: &Owner,
        parent: &B::Node,
        before: Option<&B::Node>,
        name: &str,
        condition: C,
        mut content: F,
    ) -> Self
    where
        C: Fn() -> bool + 'static,
        F: FnMut(&mut ShowContext<B>) -> B::Node + 'static,
    {
        Self::mount_many(
            runtime,
            owner,
            parent,
            before,
            name,
            condition,
            move |context| vec![content(context)],
        )
    }

    /// Mounts one or more conditional nodes between fresh anchors.
    /// 在新锚点之间挂载一个或多个条件节点。
    pub fn mount_many<C, F>(
        runtime: &Runtime<B>,
        owner: &Owner,
        parent: &B::Node,
        before: Option<&B::Node>,
        name: &str,
        condition: C,
        content: F,
    ) -> Self
    where
        C: Fn() -> bool + 'static,
        F: FnMut(&mut ShowContext<B>) -> Vec<B::Node> + 'static,
    {
        let control = owner.child();
        let anchors = RangeAnchor::create(runtime, parent, before);
        let dispose_requested = Rc::new(Cell::new(false));
        let state = Rc::new(RefCell::new(ShowState {
            runtime: runtime.clone(),
            control: control.clone(),
            parent: parent.clone(),
            anchors,
            condition: Box::new(condition),
            content: Box::new(content),
            active: None,
            last: None,
            cleaned: false,
            dispose_requested: Rc::clone(&dispose_requested),
            control_dispose_pending: false,
        }));

        let effect_state = Rc::clone(&state);
        let effect_control = control.clone();
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            runtime.effect_with_runtime(
                &control,
                &format!("{name}/condition"),
                move |_context, runtime| {
                    let mut state = effect_state.borrow_mut();
                    let update = catch_unwind(AssertUnwindSafe(|| state.update(runtime)));
                    if state.dispose_requested.get() {
                        state.request_terminal();
                    }
                    let dispose_control = state.take_control_dispose_pending();
                    drop(state);
                    if dispose_control {
                        effect_control.dispose();
                    }
                    if let Err(payload) = update {
                        resume_unwind(payload);
                    }
                },
            );
        }));
        if let Err(payload) = outcome {
            // A first-run panic happens before cleanup registration. Release the
            // range and anchors before rethrowing so no orphan nodes survive.
            // 首次执行 panic 发生在清理注册之前。重新抛出前释放范围与锚点，
            // 避免留下孤儿节点。
            if let Ok(mut state) = state.try_borrow_mut() {
                state.cleanup(false);
            }
            control.dispose();
            resume_unwind(payload);
        }

        if state.borrow().cleaned {
            return Self {
                state,
                dispose_requested,
            };
        }

        let cleanup_state = SendWrapper::new(Rc::downgrade(&state));
        control.register_cleanup(move || {
            if let Some(state) = cleanup_state.upgrade()
                && let Ok(mut state) = state.try_borrow_mut()
            {
                state.cleanup(false);
            }
        });

        Self {
            state,
            dispose_requested,
        }
    }

    /// Returns the stable start anchor.
    /// 返回稳定的起始锚点。
    pub fn start(&self) -> B::Node {
        self.state.borrow().anchors.start.clone()
    }

    /// Returns the stable end anchor.
    /// 返回稳定的结束锚点。
    pub fn end(&self) -> B::Node {
        self.state.borrow().anchors.end.clone()
    }

    /// Returns a copy of the stable anchor pair.
    /// 返回稳定锚点对的副本。
    pub fn anchors(&self) -> RangeAnchor<B> {
        self.state.borrow().anchors.clone()
    }

    /// Returns whether the dynamic range currently owns content nodes.
    /// 返回动态范围当前是否持有内容节点。
    pub fn is_mounted(&self) -> bool {
        self.state.borrow().active.is_some()
    }

    /// Unmounts the current range and removes the anchors; repeated calls are safe.
    /// 卸载当前范围并移除锚点；可安全重复调用。
    pub fn dispose(&self) {
        self.dispose_requested.set(true);
        let Ok(mut state) = self.state.try_borrow_mut() else {
            // The condition effect owns the state. The pending flag completes
            // terminal cleanup after the effect releases its borrow.
            // 条件 Effect 正持有状态。pending 标记让 Effect 在释放借用后完成
            // 终止清理。
            return;
        };
        let terminal = catch_unwind(AssertUnwindSafe(|| state.request_terminal())).err();
        let _ = state.take_control_dispose_pending();
        let control = state.control.clone();
        drop(state);
        let control_panic = catch_unwind(AssertUnwindSafe(|| control.dispose())).err();
        if let Some(payload) = terminal {
            resume_unwind(payload);
        }
        if let Some(payload) = control_panic {
            resume_unwind(payload);
        }
    }
}

struct ActiveRange<B: DomBackend> {
    owner: Owner,
    nodes: Vec<B::Node>,
    created: Vec<B::Node>,
}

impl<B: DomBackend + 'static> ActiveRange<B> {
    /// Stops the child owner before releasing its nodes and retained handles.
    /// 在释放节点与保留句柄前先停止子 Owner。
    fn dispose(self, runtime: &Runtime<B>) {
        let ActiveRange {
            owner,
            nodes: returned_nodes,
            created,
        } = self;
        let mut nodes = created;
        nodes.extend(returned_nodes);
        if let Some(payload) = terminate_owner_and_nodes(owner, nodes, runtime, None) {
            resume_unwind(payload);
        }
    }
}

type ContentBuilder<B> = Box<dyn FnMut(&mut ShowContext<B>) -> Vec<<B as DomBackend>::Node>>;

struct ShowState<B: DomBackend + 'static> {
    runtime: Runtime<B>,
    control: Owner,
    parent: B::Node,
    anchors: RangeAnchor<B>,
    condition: Box<dyn Fn() -> bool>,
    content: ContentBuilder<B>,
    active: Option<ActiveRange<B>>,
    last: Option<bool>,
    cleaned: bool,
    dispose_requested: Rc<Cell<bool>>,
    control_dispose_pending: bool,
}

impl<B: DomBackend + 'static> ShowState<B> {
    /// Applies a condition read, creating or removing only a real transition.
    /// 应用一次条件读取，只在真实切换时创建或移除范围。
    fn update(&mut self, runtime: &mut Runtime<B>) {
        if self.cleaned {
            return;
        }
        if self.dispose_requested.get() {
            self.request_terminal();
            return;
        }
        let next = (self.condition)();
        if self.dispose_requested.get() {
            self.request_terminal();
            return;
        }
        if self.last == Some(next) {
            return;
        }
        if next {
            self.mount_range(runtime);
            if self.dispose_requested.get() {
                self.request_terminal();
                return;
            }
            self.last = Some(true);
        } else {
            self.unmount_range(runtime);
            if self.dispose_requested.get() {
                self.request_terminal();
                return;
            }
            self.last = Some(false);
        }
    }

    /// Creates a child owner and mounts its content inside the anchor range.
    /// 创建子 Owner，并把其内容挂载到锚点范围内。
    ///
    /// A panic in the builder disposes the child owner and releases every
    /// tracked creation before resuming the unwind, so `last` is not advanced.
    /// 构建器 panic 时，在继续展开前释放子 Owner 与全部已跟踪创建节点，
    /// 因此 `last` 不会前进。
    fn mount_range(&mut self, runtime: &mut Runtime<B>) {
        if self.active.is_some() {
            return;
        }
        let owner = self.control.child();
        let mut context = ShowContext::new(runtime.clone(), owner.clone());
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let nodes = with_owner(&owner, || (self.content)(&mut context));
            for node in &nodes {
                self.anchors.insert(runtime, &self.parent, node);
            }
            nodes
        }));
        match outcome {
            Ok(nodes) => {
                let created = std::mem::take(&mut context.created);
                self.active = Some(ActiveRange {
                    owner,
                    nodes,
                    created,
                });
            }
            Err(payload) => {
                let created = std::mem::take(&mut context.created);
                if let Some(payload) =
                    terminate_owner_and_nodes(owner, created, runtime, Some(payload))
                {
                    resume_unwind(payload);
                }
            }
        }
    }

    /// Disposes the child owner and then removes its nodes.
    /// 释放子 Owner，然后移除其节点。
    fn unmount_range(&mut self, runtime: &Runtime<B>) {
        if let Some(active) = self.active.take() {
            active.dispose(runtime);
        }
    }

    /// Releases the range, anchors, and driver effect exactly once.
    /// 只释放一次范围、锚点与驱动 Effect。
    fn request_terminal(&mut self) {
        self.control_dispose_pending = true;
        self.release_terminal();
    }

    fn release_terminal(&mut self) {
        if self.cleaned {
            return;
        }
        self.cleaned = true;
        let runtime = self.runtime.clone();
        let unmount = catch_unwind(AssertUnwindSafe(|| self.unmount_range(&runtime))).err();
        let anchors =
            catch_unwind(AssertUnwindSafe(|| self.anchors.remove_anchors(&runtime))).err();
        if let Some(payload) = unmount {
            resume_unwind(payload);
        }
        if let Some(payload) = anchors {
            resume_unwind(payload);
        }
    }

    fn take_control_dispose_pending(&mut self) -> bool {
        std::mem::take(&mut self.control_dispose_pending)
    }

    fn cleanup(&mut self, dispose_control: bool) {
        let release = catch_unwind(AssertUnwindSafe(|| self.release_terminal())).err();
        let control = if dispose_control {
            catch_unwind(AssertUnwindSafe(|| self.control.dispose())).err()
        } else {
            None
        };
        if let Some(payload) = release {
            resume_unwind(payload);
        }
        if let Some(payload) = control {
            resume_unwind(payload);
        }
    }
}

impl<B: DomBackend + 'static> Drop for ShowState<B> {
    fn drop(&mut self) {
        self.cleanup(true);
    }
}
