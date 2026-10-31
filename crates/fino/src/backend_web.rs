//! Real browser DOM backend built directly on `web-sys`.
//! 直接基于 `web-sys` 构建的真实浏览器 DOM 后端。

use std::{collections::BTreeMap, fmt};

use serde::Serialize;
use wasm_bindgen::{JsCast, prelude::Closure};
use web_sys::{Document, Element, Event as WebEvent, HtmlElement, Node};

use crate::{
    dom_api::{Attribute, Event, EventContext, EventKind, EventListener, Role, Selector, Tag},
    headless::{
        A11yNodeSnapshot, A11ySnapshot, BackendOperation, DomNodeKind, DomNodeSnapshot,
        DomSnapshot, HeadlessBackend, LogicalNodeSnapshot, LogicalTreeSnapshot, SnapshotKind,
    },
};

/// Stable browser node handle; the browser node itself is the storage.
/// 稳定的浏览器节点句柄；浏览器节点本身就是存储。
///
/// A handle never carries a mirror tree: equality compares the real DOM node identity.
/// 句柄不携带镜像树；相等性比较真实 DOM 节点身份。
#[derive(Clone)]
pub struct WebNode {
    node: Node,
    tag: Option<Tag>,
}

impl PartialEq for WebNode {
    fn eq(&self, other: &Self) -> bool {
        self.node.is_same_node(Some(&other.node))
    }
}

impl Eq for WebNode {}

impl fmt::Debug for WebNode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebNode")
            .field("tag", &self.tag)
            .finish_non_exhaustive()
    }
}

/// Stable listener registration handle.
/// 稳定的监听注册句柄。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WebListener(u64);

/// Observable backend mutations kept in the same shape as the mock operation log.
/// 与 Mock 操作日志形状一致的可观测后端变更。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum WebBackendOp {
    CreateElement(Tag),
    CreateText,
    Insert,
    SetText {
        node: u64,
        value: String,
    },
    SetAttribute {
        node: u64,
        attribute: Attribute,
        value: Option<String>,
    },
    Listen(EventKind),
    Unlisten(EventKind),
    Remove,
}

impl WebBackendOp {
    /// Maps each mutation to the stable verb used by normalized trace events.
    /// 将每种变更映射为规范化 trace 事件使用的稳定操作名。
    fn operation(&self) -> &'static str {
        match self {
            Self::CreateElement(_) => "create_element",
            Self::CreateText => "create_text",
            Self::Insert => "insert",
            Self::SetText { .. } => "set_text",
            Self::SetAttribute { .. } => "set_attribute",
            Self::Listen(_) => "listen",
            Self::Unlisten(_) => "unlisten",
            Self::Remove => "remove",
        }
    }
}

struct NodeRecord {
    id: u64,
    node: WebNode,
    /// Detached nodes stay alive but leave the reachable logical tree.
    /// 已分离节点仍然存活，但离开可达逻辑树。
    removed: bool,
}

struct ListenerRegistration {
    node: Node,
    event: EventKind,
    closure: Closure<dyn FnMut(WebEvent)>,
}

/// Browser DOM backend that maps every operation to a real node mutation.
/// 将每个操作映射为真实节点变更的浏览器 DOM 后端。
pub struct DomBackend {
    document: Document,
    body: HtmlElement,
    next_node: u64,
    next_listener: u64,
    nodes: Vec<NodeRecord>,
    listeners: BTreeMap<u64, ListenerRegistration>,
    operations: Vec<WebBackendOp>,
}

impl Default for DomBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl DomBackend {
    /// Creates a backend bound to the current document and its body.
    /// 创建绑定到当前文档及其 body 的后端。
    pub fn new() -> Self {
        let window = web_sys::window().expect("the browser backend requires a Window");
        let document = window
            .document()
            .expect("the browser backend requires a Document");
        let body = document
            .body()
            .expect("the browser backend requires a document body");
        Self {
            document,
            body,
            next_node: 0,
            next_listener: 0,
            nodes: Vec::new(),
            listeners: BTreeMap::new(),
            operations: Vec::new(),
        }
    }

    /// Attaches a backend-created root to `document.body`.
    /// 把后端创建的根节点挂载到 `document.body`。
    pub fn mount_to_body(&self, node: &WebNode) {
        self.body
            .append_child(&node.node)
            .expect("mounting the counter on document.body must succeed");
    }

    /// Returns the number of live listener registrations.
    /// 返回仍存活的监听注册数量。
    pub fn listener_count(&self) -> usize {
        self.listeners.len()
    }

    /// Assigns a stable creation-order identifier to one real browser node.
    /// 为一个真实浏览器节点分配稳定的创建顺序标识。
    fn allocate_node(&mut self, node: Node, tag: Option<Tag>) -> WebNode {
        self.next_node += 1;
        let handle = WebNode { node, tag };
        self.nodes.push(NodeRecord {
            id: self.next_node,
            node: handle.clone(),
            removed: false,
        });
        handle
    }

    /// Records whether a tracked node is detached from the reachable tree.
    /// 记录受跟踪节点是否已从可达树中分离。
    fn set_removed(&mut self, node: &Node, removed: bool) {
        for record in &mut self.nodes {
            if record.node.node.is_same_node(Some(node)) {
                record.removed = removed;
                return;
            }
        }
    }

    /// Looks up the stable identifier by comparing real DOM node identity.
    /// 通过比较真实 DOM 节点身份查找稳定标识。
    fn record_id(&self, node: &Node) -> Option<u64> {
        self.nodes
            .iter()
            .find(|record| record.node.node.is_same_node(Some(node)))
            .map(|record| record.id)
    }

    /// Returns the tracked handle for a real node without copying its subtree.
    /// 返回真实节点的受跟踪句柄，不复制其子树。
    fn record(&self, node: &Node) -> Option<WebNode> {
        self.nodes
            .iter()
            .find(|record| record.node.node.is_same_node(Some(node)))
            .map(|record| record.node.clone())
    }

    /// Materializes the current real child list so it can be traversed safely.
    /// 具象化当前真实子节点列表，以便安全遍历。
    fn child_nodes(node: &Node) -> Vec<Node> {
        let children = node.child_nodes();
        (0..children.length())
            .filter_map(|index| children.item(index))
            .collect()
    }

    /// Collects tracked roots: attached nodes whose real parent is not tracked.
    /// 收集受跟踪的根：已挂载且真实父节点不是受跟踪节点的节点。
    fn roots(&self) -> Vec<WebNode> {
        self.nodes
            .iter()
            .filter(|record| {
                !record.removed
                    && record
                        .node
                        .node
                        .parent_node()
                        .is_none_or(|parent| self.record_id(&parent).is_none())
            })
            .map(|record| record.node.clone())
            .collect()
    }

    fn collect_reachable(&self, node: &WebNode, output: &mut Vec<WebNode>) {
        output.push(node.clone());
        for child in Self::child_nodes(&node.node) {
            if let Some(child) = self.record(&child) {
                self.collect_reachable(&child, output);
            }
        }
    }

    /// Collects every reachable tracked node in real document order.
    /// 按真实文档顺序收集所有可达的受跟踪节点。
    fn reachable_nodes(&self) -> Vec<WebNode> {
        let mut nodes = Vec::new();
        for root in self.roots() {
            self.collect_reachable(&root, &mut nodes);
        }
        nodes
    }

    /// Borrows a tracked node as an element without changing its identity.
    /// 把受跟踪节点借用为元素，且不改变其身份。
    fn element(node: &Node) -> &Element {
        node.unchecked_ref::<Element>()
    }

    /// Reads the caller-visible name the same way a semantic selector needs it.
    /// 按语义选择器所需的方式读取调用方可见名称。
    fn accessible_name(&self, node: &WebNode) -> String {
        let element = Self::element(&node.node);
        element
            .get_attribute("aria-label")
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| node.node.text_content().unwrap_or_default())
            .trim()
            .to_owned()
    }

    /// Formats one stable snapshot identifier from a real node.
    /// 为一个真实节点格式化稳定快照标识。
    fn node_id_string(&self, node: &Node) -> Option<String> {
        self.record_id(node).map(|id| format!("node-{id}"))
    }

    /// Appends one normalized operation to the inspection log.
    /// 向检查日志追加一条规范化操作。
    fn emit(&mut self, operation: WebBackendOp) {
        self.operations.push(operation);
    }

    /// Projects one reachable real node into a handle-free logical tree.
    /// 把一个可达真实节点投影为不含句柄的逻辑树节点。
    fn logical_node(&self, node: &WebNode) -> Option<LogicalNodeSnapshot> {
        self.record_id(&node.node)?;
        match node.tag {
            Some(Tag::Button) => {
                let element = Self::element(&node.node);
                let mut attributes = BTreeMap::new();
                if let Some(value) = element.get_attribute("aria-label") {
                    attributes.insert("aria-label".to_owned(), value);
                }
                Some(LogicalNodeSnapshot {
                    kind: DomNodeKind::Element,
                    tag: Some("button".to_owned()),
                    text: None,
                    attributes,
                    children: Self::child_nodes(&node.node)
                        .iter()
                        .filter_map(|child| self.record(child))
                        .filter_map(|child| self.logical_node(&child))
                        .collect(),
                })
            }
            None => Some(LogicalNodeSnapshot {
                kind: DomNodeKind::Text,
                tag: None,
                text: Some(node.node.node_value().unwrap_or_default()),
                attributes: BTreeMap::new(),
                children: Vec::new(),
            }),
        }
    }
}

impl crate::dom_api::DomBackend for DomBackend {
    type Node = WebNode;
    type Listener = WebListener;

    /// Creates a real button element for the closed tag set.
    /// 为封闭标签集合创建真实按钮元素。
    fn create_element(&mut self, tag: Tag) -> Self::Node {
        let name = match tag {
            Tag::Button => "button",
        };
        let element = self
            .document
            .create_element(name)
            .expect("creating a supported element must succeed");
        let handle = self.allocate_node(element.unchecked_into(), Some(tag));
        self.emit(WebBackendOp::CreateElement(tag));
        handle
    }

    /// Creates a real text node with its initial content.
    /// 创建带初始内容的真实文本节点。
    fn create_text(&mut self, value: &str) -> Self::Node {
        let text = self.document.create_text_node(value);
        let handle = self.allocate_node(text.unchecked_into(), None);
        self.emit(WebBackendOp::CreateText);
        handle
    }

    /// Inserts a real child at the requested position without clearing siblings.
    /// 在请求位置插入真实子节点，且不清空兄弟节点。
    fn insert_before(
        &mut self,
        parent: &Self::Node,
        child: &Self::Node,
        anchor: Option<&Self::Node>,
    ) {
        assert!(
            self.record_id(&parent.node).is_some() && self.record_id(&child.node).is_some(),
            "insert requires tracked parent and child nodes"
        );
        parent
            .node
            .insert_before(&child.node, anchor.map(|anchor| &anchor.node))
            .expect("inserting a tracked node must not violate the DOM hierarchy");
        self.set_removed(&child.node, false);
        self.emit(WebBackendOp::Insert);
    }

    /// Detaches only the selected real node and removes it from the reachable tree.
    /// 仅分离选定的真实节点，并把它移出可达树。
    fn remove(&mut self, node: &Self::Node) {
        if let Some(parent) = node.node.parent_node() {
            parent
                .remove_child(&node.node)
                .expect("removing a tracked child must succeed");
        }
        self.set_removed(&node.node, true);
        self.emit(WebBackendOp::Remove);
    }

    /// Replaces only the existing text node's `nodeValue`.
    /// 仅替换现有文本节点的 `nodeValue`。
    fn set_text(&mut self, node: &Self::Node, value: &str) {
        assert!(
            node.tag.is_none() && self.record_id(&node.node).is_some(),
            "set_text requires a tracked text node"
        );
        if node.node.node_value().as_deref() == Some(value) {
            return;
        }
        node.node.set_node_value(Some(value));
        self.emit(WebBackendOp::SetText {
            node: self.record_id(&node.node).expect("tracked text node"),
            value: value.to_owned(),
        });
    }

    /// Sets or removes one supported real DOM attribute.
    /// 设置或移除一个受支持的真实 DOM 属性。
    fn set_attribute(&mut self, node: &Self::Node, name: Attribute, value: Option<&str>) {
        assert!(
            node.tag.is_some() && self.record_id(&node.node).is_some(),
            "set_attribute requires a tracked element"
        );
        let element = Self::element(&node.node);
        let attribute_name = match name {
            Attribute::AriaLabel => "aria-label",
        };
        match value {
            Some(value) => element
                .set_attribute(attribute_name, value)
                .expect("setting a supported attribute must succeed"),
            None => element
                .remove_attribute(attribute_name)
                .expect("removing a supported attribute must succeed"),
        }
        self.emit(WebBackendOp::SetAttribute {
            node: self.record_id(&node.node).expect("tracked element"),
            attribute: name,
            value: value.map(str::to_owned),
        });
    }

    /// Registers a real DOM event listener while retaining the Rust callback.
    /// 注册真实 DOM 事件监听器，同时保留 Rust 回调。
    fn listen(
        &mut self,
        node: &Self::Node,
        event: EventKind,
        listener: EventListener,
    ) -> Self::Listener {
        assert!(
            self.record_id(&node.node).is_some(),
            "listener node must be tracked"
        );
        self.next_listener += 1;
        let handle = WebListener(self.next_listener);
        let callback = listener.clone();
        let closure = Closure::<dyn FnMut(WebEvent)>::new(move |_event: WebEvent| {
            callback.call(Event { kind: event }, &mut EventContext);
        });
        node.node
            .add_event_listener_with_callback(event_name(event), closure.as_ref().unchecked_ref())
            .expect("registering a supported DOM event must succeed");
        self.listeners.insert(
            handle.0,
            ListenerRegistration {
                node: node.node.clone(),
                event,
                closure,
            },
        );
        self.emit(WebBackendOp::Listen(event));
        handle
    }

    /// Removes a live DOM listener and drops its retained Rust closure.
    /// 移除存活的 DOM 监听器并释放其保留的 Rust 闭包。
    fn unlisten(&mut self, listener: Self::Listener) {
        if let Some(registration) = self.listeners.remove(&listener.0) {
            registration
                .node
                .remove_event_listener_with_callback(
                    event_name(registration.event),
                    registration.closure.as_ref().unchecked_ref(),
                )
                .expect("removing a registered DOM event must succeed");
            drop(registration.closure);
            self.emit(WebBackendOp::Unlisten(registration.event));
        }
    }
}

impl HeadlessBackend for DomBackend {
    /// Returns the stable backend name used in results and traces.
    /// 返回结果与 trace 使用的稳定后端名。
    fn backend_name(&self) -> &'static str {
        "web-headless"
    }

    /// Reports the four logical snapshot capabilities backed by the real DOM.
    /// 报告由真实 DOM 支撑的四种逻辑快照能力。
    fn supports_snapshot(&self, kind: SnapshotKind) -> bool {
        matches!(
            kind,
            SnapshotKind::Dom | SnapshotKind::A11y | SnapshotKind::Reactive | SnapshotKind::Trace
        )
    }

    /// Resolves selectors from reachable real nodes and their accessible names.
    /// 从可达真实节点及其无障碍名称解析选择器。
    fn find_all(&self, selector: &Selector) -> Vec<Self::Node> {
        self.reachable_nodes()
            .into_iter()
            .filter(|node| {
                selector.role == Role::Button
                    && node.tag == Some(Tag::Button)
                    && self.accessible_name(node) == selector.name
            })
            .collect()
    }

    /// Reads descendant text only from a tracked real node.
    /// 仅从受跟踪的真实节点读取后代文本。
    fn text_content(&self, node: &Self::Node) -> Option<String> {
        self.record_id(&node.node)?;
        node.node.text_content()
    }

    /// Dispatches the scenario's semantic click through the browser event system.
    /// 通过浏览器事件系统分发场景的语义点击。
    fn listener(&self, node: &Self::Node, event: EventKind) -> Option<EventListener> {
        let registered = self.listeners.values().any(|registration| {
            registration.node.is_same_node(Some(&node.node)) && registration.event == event
        });
        if !registered {
            return None;
        }
        let target = node.node.clone();
        Some(EventListener::new(move |_event, _context| {
            target.unchecked_ref::<HtmlElement>().click();
        }))
    }

    /// Serializes the real tree with stable creation-order identifiers.
    /// 使用稳定创建顺序标识序列化真实树。
    fn dom_snapshot(&self) -> DomSnapshot {
        let roots = self
            .roots()
            .iter()
            .filter_map(|node| self.node_id_string(&node.node))
            .collect();
        let nodes = self
            .nodes
            .iter()
            .map(|record| {
                let node = &record.node;
                let parent = node
                    .node
                    .parent_node()
                    .and_then(|parent| self.node_id_string(&parent));
                let children = Self::child_nodes(&node.node)
                    .iter()
                    .filter_map(|child| self.node_id_string(child))
                    .collect();
                let (kind, tag, text, attributes) = match node.tag {
                    Some(Tag::Button) => {
                        let element = Self::element(&node.node);
                        let mut attributes = BTreeMap::new();
                        if let Some(value) = element.get_attribute("aria-label") {
                            attributes.insert("aria-label".to_owned(), value);
                        }
                        (
                            DomNodeKind::Element,
                            Some("button".to_owned()),
                            None,
                            attributes,
                        )
                    }
                    None => (
                        DomNodeKind::Text,
                        None,
                        Some(node.node.node_value().unwrap_or_default()),
                        BTreeMap::new(),
                    ),
                };
                DomNodeSnapshot {
                    id: format!("node-{}", record.id),
                    kind,
                    parent,
                    children,
                    tag,
                    text,
                    attributes,
                }
            })
            .collect();
        DomSnapshot {
            schema: 1,
            roots,
            nodes,
        }
    }

    /// Projects reachable real buttons into semantic accessibility snapshots.
    /// 将可达的真实按钮投影为语义无障碍快照。
    fn a11y_snapshot(&self) -> A11ySnapshot {
        let nodes = self
            .reachable_nodes()
            .into_iter()
            .filter(|node| node.tag == Some(Tag::Button))
            .map(|node| A11yNodeSnapshot {
                id: format!(
                    "node-{}",
                    self.record_id(&node.node)
                        .expect("reachable node is tracked")
                ),
                role: Role::Button,
                name: self.accessible_name(&node),
                focusable: true,
                actions: vec!["click".to_owned()],
                states: BTreeMap::new(),
                text: node.node.text_content().unwrap_or_default(),
            })
            .collect();
        A11ySnapshot { schema: 1, nodes }
    }

    /// Normalizes real DOM mutations into backend-neutral trace records.
    /// 把真实 DOM 变更规范化为后端无关的 trace 记录。
    fn operation_log(&self) -> Vec<BackendOperation> {
        self.operations
            .iter()
            .map(|operation| BackendOperation {
                operation: operation.operation().to_owned(),
                details: serde_json::to_value(operation).unwrap_or(serde_json::Value::Null),
            })
            .collect()
    }

    /// Serializes only reachable real nodes and never exposes browser handles.
    /// 仅序列化可达真实节点，绝不暴露浏览器句柄。
    fn logical_tree(&self) -> LogicalTreeSnapshot {
        LogicalTreeSnapshot {
            schema: 1,
            roots: self
                .roots()
                .iter()
                .filter_map(|root| self.logical_node(root))
                .collect(),
        }
    }
}

/// Maps the closed event contract to the browser's DOM event name.
/// 把封闭事件契约映射为浏览器的 DOM 事件名。
fn event_name(event: EventKind) -> &'static str {
    match event {
        EventKind::Click => "click",
    }
}
