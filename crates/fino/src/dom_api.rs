//! Backend-neutral DOM semantics required by minimal vertical slices.
//! 最小垂直切片所需的后端无关 DOM 语义。

use std::{cell::RefCell, fmt, rc::Rc};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Closed set of element tags supported in PR1.
/// PR1 支持的封闭元素标签集合。
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Tag {
    /// A native button semantic.
    /// 原生按钮语义。
    Button,
}

/// Closed set of attributes supported by the minimal backend.
/// 最小后端支持的封闭属性集合。
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum Attribute {
    /// Accessible name for assistive technology and semantic selectors.
    /// 无障碍名称，同时用于语义选择器。
    AriaLabel,
}

/// Events supported by the minimal backend contract.
/// 最小后端契约支持的事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// A semantic click activation.
    /// 语义点击激活。
    Click,
}

/// Minimal event data delivered to listeners.
/// 传递给监听器的最小事件数据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    /// Kind of the dispatched event.
    /// 被分发事件的种类。
    pub kind: EventKind,
}

/// Framework context supplied during event dispatch.
/// 事件分发期间由框架提供的上下文。
#[derive(Debug, Default)]
pub struct EventContext;

type EventCallback = Rc<RefCell<Box<dyn FnMut(Event, &mut EventContext)>>>;

/// Cloneable local event callback stored by a backend.
/// 后端保存的可克隆本地事件回调。
#[derive(Clone)]
pub struct EventListener {
    callback: EventCallback,
}

impl EventListener {
    /// Creates an event listener without imposing platform threading types.
    /// 创建事件监听器，不引入平台线程类型。
    pub fn new(callback: impl FnMut(Event, &mut EventContext) + 'static) -> Self {
        Self {
            callback: Rc::new(RefCell::new(Box::new(callback))),
        }
    }

    /// Dispatches an event outside a backend borrow so reactive updates can safely mutate it.
    /// 在后端借用之外分发事件，使响应式更新可以安全修改后端。
    pub fn call(&self, event: Event, context: &mut EventContext) {
        (self.callback.borrow_mut())(event, context);
    }
}

impl fmt::Debug for EventListener {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EventListener")
            .finish_non_exhaustive()
    }
}

/// Accessibility role used by stable semantic selectors.
/// 稳定语义选择器使用的无障碍角色。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Button role.
    /// 按钮角色。
    Button,
}

/// A role plus accessible-name selector; internal node IDs are intentionally excluded.
/// 角色与无障碍名称选择器，刻意不包含内部节点 ID。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Selector {
    /// Required semantic role.
    /// 必需的语义角色。
    pub role: Role,
    /// Required accessible name.
    /// 必需的无障碍名称。
    pub name: String,
}

/// Platform-independent operations implemented by a DOM-like backend.
/// 由 DOM-like 后端实现的平台无关操作。
pub trait DomBackend {
    /// Opaque node handle owned by the backend.
    /// 由后端持有的不透明节点句柄。
    type Node: Clone + Eq + 'static;
    /// Opaque listener registration handle.
    /// 不透明监听注册句柄。
    type Listener: 'static;

    /// Creates an element with closed tag semantics.
    /// 创建具有封闭标签语义的元素。
    fn create_element(&mut self, tag: Tag) -> Self::Node;
    /// Creates a text node with initial static content.
    /// 创建带初始静态内容的文本节点。
    fn create_text(&mut self, value: &str) -> Self::Node;
    /// Creates an invisible structural anchor for a dynamic range.
    /// 为动态范围创建不可见的范围锚点。
    ///
    /// Backends without a dedicated anchor node fall back to an empty text
    /// node, which keeps older implementations source-compatible and still
    /// occupies a stable sibling position.
    /// 没有专用锚点节点的后端回退到空文本节点；这保持旧实现的源码兼容，
    /// 同时仍占据稳定的兄弟位置。
    fn create_anchor(&mut self) -> Self::Node {
        self.create_text("")
    }
    /// Inserts a child before an optional anchor without clearing siblings.
    /// 在可选锚点前插入子节点，不清空兄弟节点。
    ///
    /// Backends must validate the parent and reference before detaching or
    /// mutating the child, so an invalid reference fails atomically.
    /// 后端必须在分离或修改子节点前验证父节点与参考节点，使非法引用原子失败。
    fn insert_before(
        &mut self,
        parent: &Self::Node,
        child: &Self::Node,
        anchor: Option<&Self::Node>,
    );
    /// Detaches a node while leaving reactive cleanup to the owner.
    /// 分离节点，响应式清理由 Owner 负责。
    ///
    /// Detached nodes keep their handles so callers can reinsert them.
    /// 已分离节点保留句柄，调用方可以重新插入。
    fn remove(&mut self, node: &Self::Node);
    /// Detaches and releases a node subtree whose handles will not be reused.
    /// 分离并释放一个不会再复用句柄的节点子树。
    ///
    /// This is the terminal counterpart to [`Self::remove`]; backends should
    /// free storage and invalidate handles. The default only detaches because a
    /// generic backend cannot discover descendants, so backends that retain
    /// storage must override it.
    /// 这是 [`Self::remove`] 的终止对应操作；后端应释放存储并使句柄失效。
    /// 通用后端无法发现后代，因此默认只做分离；会保留存储的后端必须覆盖它。
    fn release(&mut self, node: &Self::Node) {
        self.remove(node);
    }
    /// Returns a stable backend identity for O(1) ownership indexes.
    /// 返回用于 O(1) 所有权索引的稳定后端标识。
    ///
    /// The default returns `None`, so legacy and custom backends keep using
    /// `Eq`-based fallback comparisons. Built-in backends return their stable
    /// node record identity rather than an address or formatted string.
    /// 默认返回 `None`，旧版与自定义后端继续使用基于 `Eq` 的回退比较。内建
    /// 后端返回稳定节点记录标识，而不是地址或格式化字符串。
    fn node_identity(&self, node: &Self::Node) -> Option<u64> {
        let _ = node;
        None
    }
    /// Replaces only the content of an existing text node.
    /// 仅替换现有文本节点的内容。
    fn set_text(&mut self, node: &Self::Node, value: &str);
    /// Sets or removes a supported attribute.
    /// 设置或移除受支持的属性。
    fn set_attribute(&mut self, node: &Self::Node, name: Attribute, value: Option<&str>);
    /// Registers a semantic event listener.
    /// 注册语义事件监听器。
    fn listen(
        &mut self,
        node: &Self::Node,
        event: EventKind,
        listener: EventListener,
    ) -> Self::Listener;
    /// Releases a listener registration.
    /// 释放监听注册。
    fn unlisten(&mut self, listener: Self::Listener);
}
