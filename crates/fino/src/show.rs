//! Range-anchored conditional mounting without virtual-DOM reruns.
//! 使用范围锚点的条件挂载，不触发虚拟 DOM 式的整树重跑。

use std::{cell::RefCell, fmt, rc::Rc};

use send_wrapper::SendWrapper;

use crate::{dom_api::DomBackend, reactive::Owner, runtime::Runtime, view::with_owner};

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

    /// Inserts one node inside the range, immediately before the end anchor.
    /// 把一个节点插入范围内部，紧邻结束锚点之前。
    pub fn insert(&self, runtime: &Runtime<B>, parent: &B::Node, node: &B::Node) {
        runtime
            .backend_mut()
            .insert_before(parent, node, Some(&self.end));
    }

    /// Removes one node from the range without touching either anchor.
    /// 从范围内移除一个节点，不触碰两个锚点。
    pub fn remove(&self, runtime: &Runtime<B>, node: &B::Node) {
        runtime.backend_mut().remove(node);
    }

    /// Removes both anchors after the range has been released.
    /// 在范围释放后移除两个锚点。
    pub fn remove_anchors(&self, runtime: &Runtime<B>) {
        runtime.backend_mut().remove(&self.end);
        runtime.backend_mut().remove(&self.start);
    }
}

impl<B: DomBackend> fmt::Debug for RangeAnchor<B> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RangeAnchor")
            .finish_non_exhaustive()
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
        F: FnMut(&mut Runtime<B>, &Owner) -> B::Node + 'static,
    {
        Self::mount_many(
            runtime,
            owner,
            parent,
            before,
            name,
            condition,
            move |runtime, owner| vec![content(runtime, owner)],
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
        F: FnMut(&mut Runtime<B>, &Owner) -> Vec<B::Node> + 'static,
    {
        let control = owner.child();
        let anchors = RangeAnchor::create(runtime, parent, before);
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
        }));

        let effect_state = Rc::clone(&state);
        runtime.effect_with_runtime(
            &control,
            &format!("{name}/condition"),
            move |_context, runtime| {
                effect_state.borrow_mut().update(runtime);
            },
        );

        let cleanup_state = SendWrapper::new(Rc::downgrade(&state));
        control.register_cleanup(move || {
            if let Some(state) = cleanup_state.upgrade()
                && let Ok(mut state) = state.try_borrow_mut()
            {
                state.cleanup(false);
            }
        });

        Self { state }
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

struct ActiveRange<B: DomBackend> {
    owner: Owner,
    nodes: Vec<B::Node>,
}

impl<B: DomBackend + 'static> ActiveRange<B> {
    /// Stops the child owner before detaching its nodes from the parent.
    /// 在从父节点分离内容前先释放子 Owner。
    fn dispose(self, runtime: &Runtime<B>) {
        self.owner.dispose();
        for node in &self.nodes {
            runtime.backend_mut().remove(node);
        }
    }
}

type ContentBuilder<B> = Box<dyn FnMut(&mut Runtime<B>, &Owner) -> Vec<<B as DomBackend>::Node>>;

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
}

impl<B: DomBackend + 'static> ShowState<B> {
    /// Applies a condition read, creating or removing only a real transition.
    /// 应用一次条件读取，只在真实切换时创建或移除范围。
    fn update(&mut self, runtime: &mut Runtime<B>) {
        if self.cleaned {
            return;
        }
        let next = (self.condition)();
        if self.last == Some(next) {
            return;
        }
        self.last = Some(next);
        if next {
            self.mount_range(runtime);
        } else {
            self.unmount_range(runtime);
        }
    }

    /// Creates a child owner and mounts its content inside the anchor range.
    /// 创建子 Owner，并把其内容挂载到锚点范围内。
    fn mount_range(&mut self, runtime: &mut Runtime<B>) {
        if self.active.is_some() {
            return;
        }
        let owner = self.control.child();
        let mut child_runtime = runtime.clone();
        let nodes = with_owner(&owner, || (self.content)(&mut child_runtime, &owner));
        for node in &nodes {
            self.anchors.insert(runtime, &self.parent, node);
        }
        self.active = Some(ActiveRange { owner, nodes });
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
    fn cleanup(&mut self, dispose_control: bool) {
        if self.cleaned {
            return;
        }
        self.cleaned = true;
        let runtime = self.runtime.clone();
        self.unmount_range(&runtime);
        self.anchors.remove_anchors(&runtime);
        if dispose_control {
            self.control.dispose();
        }
    }
}

impl<B: DomBackend + 'static> Drop for ShowState<B> {
    fn drop(&mut self) {
        self.cleanup(true);
    }
}
