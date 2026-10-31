//! Non-rendering in-memory backend with a deterministic tree and operation log.
//! 具有确定性树与操作日志的无渲染内存后端。

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    dom_api::{Attribute, DomBackend, EventKind, EventListener, Role, Selector, Tag},
    headless::{
        A11yNodeSnapshot, A11ySnapshot, BackendOperation, DomNodeKind, DomNodeSnapshot,
        DomSnapshot, HeadlessBackend, LogicalNodeSnapshot, LogicalTreeSnapshot, SnapshotKind,
    },
};

/// Stable mock node handle; it is never used as a scenario selector.
/// 稳定 Mock 节点句柄，绝不作为场景选择器。
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct MockNode(pub u64);

/// Stable mock listener registration handle.
/// 稳定 Mock 监听注册句柄。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MockListener(u64);

/// Observable backend mutations used by correctness counters and traces.
/// 用于正确性计数与 trace 的可观测后端变更。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendOp {
    /// Creates one element.
    /// 创建一个元素。
    CreateElement(Tag),
    /// Creates one text node.
    /// 创建一个文本节点。
    CreateText,
    /// Creates one invisible range anchor.
    /// 创建一个不可见的范围锚点。
    CreateAnchor,
    /// Inserts without clearing a parent.
    /// 插入节点且不清空父节点。
    Insert,
    /// Updates one existing text node.
    /// 更新一个现有文本节点。
    SetText {
        /// Target text node.
        /// 目标文本节点。
        node: MockNode,
        /// New text value.
        /// 新文本值。
        value: String,
    },
    /// Sets or removes one attribute.
    /// 设置或移除一个属性。
    SetAttribute {
        /// Target element.
        /// 目标元素。
        node: MockNode,
        /// Attribute name.
        /// 属性名。
        attribute: Attribute,
        /// New value or removal.
        /// 新值或移除操作。
        value: Option<String>,
    },
    /// Registers an event listener.
    /// 注册事件监听器。
    Listen(EventKind),
    /// Releases an event listener.
    /// 释放事件监听器。
    Unlisten(EventKind),
    /// Detaches one node.
    /// 分离一个节点。
    Remove,
}

impl BackendOp {
    /// Maps each mutation to the stable verb used by normalized trace events.
    /// 将每种变更映射为规范化 trace 事件使用的稳定操作名。
    fn operation(&self) -> &'static str {
        match self {
            Self::CreateElement(_) => "create_element",
            Self::CreateText => "create_text",
            Self::CreateAnchor => "create_anchor",
            Self::Insert => "insert",
            Self::SetText { .. } => "set_text",
            Self::SetAttribute { .. } => "set_attribute",
            Self::Listen(_) => "listen",
            Self::Unlisten(_) => "unlisten",
            Self::Remove => "remove",
        }
    }
}

#[derive(Clone, Debug)]
enum MockNodeData {
    Element {
        tag: Tag,
        attributes: BTreeMap<Attribute, String>,
        parent: Option<MockNode>,
        children: Vec<MockNode>,
    },
    Text {
        value: String,
        parent: Option<MockNode>,
    },
    Anchor {
        parent: Option<MockNode>,
    },
}

impl MockNodeData {
    /// Reads the shared parent field without exposing the node-kind representation.
    /// 读取统一父节点字段，不暴露节点种类的内部表示。
    fn parent(&self) -> Option<MockNode> {
        match self {
            Self::Element { parent, .. } | Self::Text { parent, .. } | Self::Anchor { parent } => {
                *parent
            }
        }
    }

    /// Updates parentage for either node kind during insertion and detachment.
    /// 在插入或分离时为任意节点种类更新父节点关系。
    fn set_parent(&mut self, new_parent: Option<MockNode>) {
        match self {
            Self::Element { parent, .. } | Self::Text { parent, .. } | Self::Anchor { parent } => {
                *parent = new_parent;
            }
        }
    }
}

#[derive(Clone)]
struct ListenerRegistration {
    node: MockNode,
    event: EventKind,
    callback: EventListener,
}

/// Minimal tree-only backend; it performs no layout, painting, or platform I/O.
/// 只保存树的最小后端，不执行布局、绘制或平台 I/O。
#[derive(Default)]
pub struct MockBackend {
    next_node: u64,
    next_listener: u64,
    nodes: BTreeMap<MockNode, MockNodeData>,
    /// Attached forest roots; removed or nested nodes leave this set.
    /// 当前作为森林根挂载的节点；被移除或嵌套的节点会离开该集合。
    roots: BTreeSet<MockNode>,
    listeners: BTreeMap<u64, ListenerRegistration>,
    operations: Vec<BackendOp>,
    unsupported_snapshots: BTreeSet<SnapshotKind>,
}

impl MockBackend {
    /// Creates an empty mock backend.
    /// 创建空 Mock 后端。
    pub fn new() -> Self {
        Self::default()
    }

    /// Configures a snapshot capability for backend-contract regression tests.
    /// 配置快照能力，用于后端契约回归测试。
    pub fn set_snapshot_supported(&mut self, kind: SnapshotKind, supported: bool) {
        if supported {
            self.unsupported_snapshots.remove(&kind);
        } else {
            self.unsupported_snapshots.insert(kind);
        }
    }

    /// Returns the complete ordered operation log.
    /// 返回完整有序操作日志。
    pub fn operations(&self) -> &[BackendOp] {
        &self.operations
    }

    /// Returns the number of live listener registrations.
    /// 返回仍存活的监听注册数量。
    pub fn listener_count(&self) -> usize {
        self.listeners.len()
    }

    fn allocate_node(&mut self) -> MockNode {
        self.next_node += 1;
        MockNode(self.next_node)
    }

    fn parent_children(children: &mut Vec<MockNode>, child: MockNode) {
        children.retain(|candidate| *candidate != child);
    }

    /// Concatenates descendant text in child order and reports missing nodes.
    /// 按子节点顺序拼接后代文本，并报告节点是否存在。
    fn append_text(&self, node: MockNode, output: &mut String) -> bool {
        match self.nodes.get(&node) {
            Some(MockNodeData::Text { value, .. }) => {
                output.push_str(value);
                true
            }
            Some(MockNodeData::Element { children, .. }) => {
                for child in children {
                    self.append_text(*child, output);
                }
                true
            }
            Some(MockNodeData::Anchor { .. }) => true,
            None => false,
        }
    }

    /// Appends one attached subtree in document order, never entering detached storage.
    /// 按文档顺序追加单个已挂载子树，不进入已分离的存储。
    fn collect_reachable(&self, node: MockNode, nodes: &mut Vec<MockNode>) {
        let Some(data) = self.nodes.get(&node) else {
            return;
        };
        nodes.push(node);
        if let MockNodeData::Element { children, .. } = data {
            for child in children {
                self.collect_reachable(*child, nodes);
            }
        }
    }

    /// Collects every node reachable from attached roots in document order.
    /// 按文档顺序收集从已挂载根可达的全部节点。
    fn reachable_nodes(&self) -> Vec<MockNode> {
        let mut nodes = Vec::new();
        for root in &self.roots {
            self.collect_reachable(*root, &mut nodes);
        }
        nodes
    }

    /// Projects one reachable node into a handle-free logical tree.
    /// 把一个可达节点投影为不含句柄的逻辑树节点。
    fn logical_node(&self, node: MockNode) -> Option<LogicalNodeSnapshot> {
        match self.nodes.get(&node)? {
            MockNodeData::Element {
                tag,
                attributes,
                children,
                ..
            } => Some(LogicalNodeSnapshot {
                kind: DomNodeKind::Element,
                tag: Some(
                    match tag {
                        Tag::Button => "button",
                    }
                    .to_owned(),
                ),
                text: None,
                attributes: attributes
                    .iter()
                    .map(|(name, value)| {
                        let name = match name {
                            Attribute::AriaLabel => "aria-label",
                        };
                        (name.to_owned(), value.clone())
                    })
                    .collect(),
                children: children
                    .iter()
                    .filter_map(|child| self.logical_node(*child))
                    .collect(),
            }),
            MockNodeData::Text { value, .. } => Some(LogicalNodeSnapshot {
                kind: DomNodeKind::Text,
                tag: None,
                text: Some(value.clone()),
                attributes: BTreeMap::new(),
                children: Vec::new(),
            }),
            MockNodeData::Anchor { .. } => None,
        }
    }
}

impl DomBackend for MockBackend {
    type Node = MockNode;
    type Listener = MockListener;

    fn create_element(&mut self, tag: Tag) -> Self::Node {
        let node = self.allocate_node();
        self.nodes.insert(
            node,
            MockNodeData::Element {
                tag,
                attributes: BTreeMap::new(),
                parent: None,
                children: Vec::new(),
            },
        );
        self.roots.insert(node);
        self.operations.push(BackendOp::CreateElement(tag));
        node
    }

    fn create_text(&mut self, value: &str) -> Self::Node {
        let node = self.allocate_node();
        self.nodes.insert(
            node,
            MockNodeData::Text {
                value: value.to_owned(),
                parent: None,
            },
        );
        self.roots.insert(node);
        self.operations.push(BackendOp::CreateText);
        node
    }

    /// Creates an invisible anchor that participates only in sibling order.
    /// 创建一个只参与兄弟顺序的不可见范围锚点。
    fn create_anchor(&mut self) -> Self::Node {
        let node = self.allocate_node();
        self.nodes
            .insert(node, MockNodeData::Anchor { parent: None });
        self.roots.insert(node);
        self.operations.push(BackendOp::CreateAnchor);
        node
    }

    /// Moves a node to the requested position while preserving all other siblings.
    /// 将节点移动到指定位置，同时保留其他兄弟节点，绝不清空父节点。
    fn insert_before(
        &mut self,
        parent: &Self::Node,
        child: &Self::Node,
        anchor: Option<&Self::Node>,
    ) {
        if anchor == Some(child) {
            return;
        }
        // Inserting a node into itself or its descendant would make every later
        // traversal recurse forever, so reject it before any mutation.
        // 把节点插入自身或其后代会形成环，使后续遍历无限递归，因此在任何变更前拒绝。
        let mut ancestor = Some(*parent);
        while let Some(node) = ancestor {
            assert!(
                node != *child,
                "insert parent must not be the inserted child or its descendant"
            );
            ancestor = self.nodes.get(&node).and_then(MockNodeData::parent);
        }
        if let Some(old_parent) = self.nodes.get(child).and_then(MockNodeData::parent)
            && let Some(MockNodeData::Element { children, .. }) = self.nodes.get_mut(&old_parent)
        {
            Self::parent_children(children, *child);
        }

        let Some(MockNodeData::Element { children, .. }) = self.nodes.get_mut(parent) else {
            panic!("insert parent must be an existing element");
        };
        Self::parent_children(children, *child);
        let index = anchor.map_or(children.len(), |anchor| {
            children
                .iter()
                .position(|candidate| candidate == anchor)
                .expect("insert anchor must be a child of the parent")
        });
        children.insert(index, *child);
        self.nodes
            .get_mut(child)
            .expect("inserted child must exist")
            .set_parent(Some(*parent));
        self.roots.remove(child);
        self.operations.push(BackendOp::Insert);
    }

    /// Detaches only the selected node and records removal even for a root.
    /// 仅分离目标节点；即使目标是根节点也记录 Remove 操作。
    fn remove(&mut self, node: &Self::Node) {
        if let Some(parent) = self.nodes.get(node).and_then(MockNodeData::parent)
            && let Some(MockNodeData::Element { children, .. }) = self.nodes.get_mut(&parent)
        {
            Self::parent_children(children, *node);
        }
        self.nodes
            .get_mut(node)
            .expect("removed node must exist")
            .set_parent(None);
        self.roots.remove(node);
        self.operations.push(BackendOp::Remove);
    }

    /// Mutates existing text storage and records the exact target for update assertions.
    /// 修改现有文本存储，并记录精确目标供更新次数断言使用。
    fn set_text(&mut self, node: &Self::Node, value: &str) {
        let Some(MockNodeData::Text { value: current, .. }) = self.nodes.get_mut(node) else {
            panic!("set_text target must be an existing text node");
        };
        if current == value {
            return;
        }
        *current = value.to_owned();
        self.operations.push(BackendOp::SetText {
            node: *node,
            value: value.to_owned(),
        });
    }

    /// Applies insertion or removal semantics and preserves them in the operation log.
    /// 应用属性写入或移除语义，并将值变化完整保存到操作日志。
    fn set_attribute(&mut self, node: &Self::Node, name: Attribute, value: Option<&str>) {
        let Some(MockNodeData::Element { attributes, .. }) = self.nodes.get_mut(node) else {
            panic!("attribute target must be an existing element");
        };
        if let Some(value) = value {
            attributes.insert(name, value.to_owned());
        } else {
            attributes.remove(&name);
        }
        self.operations.push(BackendOp::SetAttribute {
            node: *node,
            attribute: name,
            value: value.map(str::to_owned),
        });
    }

    fn listen(
        &mut self,
        node: &Self::Node,
        event: EventKind,
        listener: EventListener,
    ) -> Self::Listener {
        assert!(self.nodes.contains_key(node), "listener node must exist");
        self.next_listener += 1;
        let handle = MockListener(self.next_listener);
        self.listeners.insert(
            handle.0,
            ListenerRegistration {
                node: *node,
                event,
                callback: listener,
            },
        );
        self.operations.push(BackendOp::Listen(event));
        handle
    }

    /// Releases a live registration and logs unlistening only once.
    /// 释放仍存活的监听注册，并且只记录一次解绑操作。
    fn unlisten(&mut self, listener: Self::Listener) {
        if let Some(registration) = self.listeners.remove(&listener.0) {
            self.operations
                .push(BackendOp::Unlisten(registration.event));
        }
    }
}

impl HeadlessBackend for MockBackend {
    fn backend_name(&self) -> &'static str {
        "mock"
    }

    /// Combines built-in logical capabilities with explicit test-only capability exclusions.
    /// 结合内建逻辑快照能力与显式测试能力排除项。
    fn supports_snapshot(&self, kind: SnapshotKind) -> bool {
        !self.unsupported_snapshots.contains(&kind)
            && matches!(
                kind,
                SnapshotKind::Dom
                    | SnapshotKind::A11y
                    | SnapshotKind::Reactive
                    | SnapshotKind::Trace
            )
    }

    /// Resolves semantic selectors only from nodes reachable from attached roots.
    /// 仅从已挂载根可达的节点解析语义选择器，绝不使用节点 ID。
    fn find_all(&self, selector: &Selector) -> Vec<Self::Node> {
        self.reachable_nodes()
            .into_iter()
            .filter(|node| {
                matches!(
                    self.nodes.get(node),
                    Some(MockNodeData::Element {
                        tag: Tag::Button,
                        attributes,
                        ..
                    }) if selector.role == Role::Button
                        && attributes.get(&Attribute::AriaLabel) == Some(&selector.name)
                )
            })
            .collect()
    }

    /// Returns descendant text only when the requested node still exists.
    /// 仅在请求节点仍存在时返回其后代文本。
    fn text_content(&self, node: &Self::Node) -> Option<String> {
        let mut output = String::new();
        self.append_text(*node, &mut output).then_some(output)
    }

    /// Returns the number of live listener registrations.
    /// 返回仍存活的监听注册数量。
    fn listener_count(&self) -> usize {
        self.listeners.len()
    }

    /// Finds a matching callback without retaining a backend borrow during dispatch.
    /// 查找匹配回调，使事件分发期间无需保持后端借用。
    fn listener(&self, node: &Self::Node, event: EventKind) -> Option<EventListener> {
        self.listeners
            .values()
            .find(|registration| registration.node == *node && registration.event == event)
            .map(|registration| registration.callback.clone())
    }

    /// Serializes the complete tree with stable semantic IDs and creation-order nodes.
    /// 使用稳定语义 ID 与创建顺序节点序列化完整树。
    fn dom_snapshot(&self) -> DomSnapshot {
        let roots = self
            .nodes
            .iter()
            .filter(|(_, data)| data.parent().is_none())
            .map(|(node, _)| format!("node-{}", node.0))
            .collect();
        let nodes = self
            .nodes
            .iter()
            .map(|(node, data)| {
                let common =
                    |kind: DomNodeKind,
                     parent: Option<MockNode>,
                     children: Vec<String>,
                     tag: Option<String>,
                     text: Option<String>,
                     attributes: BTreeMap<String, String>| DomNodeSnapshot {
                        id: format!("node-{}", node.0),
                        kind,
                        parent: parent.map(|parent| format!("node-{}", parent.0)),
                        children,
                        tag,
                        text,
                        attributes,
                    };
                match data {
                    MockNodeData::Element {
                        tag,
                        attributes,
                        parent,
                        children,
                    } => common(
                        DomNodeKind::Element,
                        *parent,
                        children
                            .iter()
                            .map(|child| format!("node-{}", child.0))
                            .collect(),
                        Some(match tag {
                            Tag::Button => "button".to_owned(),
                        }),
                        None,
                        attributes
                            .iter()
                            .map(|(name, value)| {
                                let name = match name {
                                    Attribute::AriaLabel => "aria-label".to_owned(),
                                };
                                (name, value.clone())
                            })
                            .collect(),
                    ),
                    MockNodeData::Text { value, parent } => common(
                        DomNodeKind::Text,
                        *parent,
                        Vec::new(),
                        None,
                        Some(value.clone()),
                        BTreeMap::new(),
                    ),
                    MockNodeData::Anchor { parent } => common(
                        DomNodeKind::Anchor,
                        *parent,
                        Vec::new(),
                        None,
                        None,
                        BTreeMap::new(),
                    ),
                }
            })
            .collect();
        DomSnapshot {
            schema: 1,
            roots,
            nodes,
        }
    }

    /// Projects only reachable semantic elements into roles, names, actions, and text.
    /// 仅将可达的语义元素映射为角色、名称、动作与当前文本。
    fn a11y_snapshot(&self) -> A11ySnapshot {
        let nodes = self
            .reachable_nodes()
            .into_iter()
            .filter_map(|node| match self.nodes.get(&node) {
                Some(MockNodeData::Element {
                    tag: Tag::Button,
                    attributes,
                    ..
                }) => Some(A11yNodeSnapshot {
                    id: format!("node-{}", node.0),
                    role: Role::Button,
                    name: attributes
                        .get(&Attribute::AriaLabel)
                        .cloned()
                        .unwrap_or_default(),
                    focusable: true,
                    actions: vec!["click".to_owned()],
                    states: BTreeMap::new(),
                    text: self.text_content(&node).unwrap_or_default(),
                }),
                _ => None,
            })
            .collect();
        A11ySnapshot { schema: 1, nodes }
    }

    /// Normalizes backend-specific variants into backend-neutral trace records.
    /// 将后端特定操作枚举规范化为后端无关 trace 记录。
    fn operation_log(&self) -> Vec<BackendOperation> {
        self.operations
            .iter()
            .map(|operation| BackendOperation {
                operation: operation.operation().to_owned(),
                details: serde_json::to_value(operation).unwrap_or(Value::Null),
            })
            .collect()
    }

    /// Serializes only attached nodes and never exposes mock handles.
    /// 仅序列化已挂载节点，绝不暴露 Mock 句柄。
    fn logical_tree(&self) -> LogicalTreeSnapshot {
        LogicalTreeSnapshot {
            schema: 1,
            roots: self
                .roots
                .iter()
                .filter_map(|root| self.logical_node(*root))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::*;

    /// Builds a labeled button so semantic queries can address it by name.
    /// 构建带标签的按钮，便于用名称进行语义查询。
    fn button(backend: &mut MockBackend, label: &str) -> MockNode {
        let node = backend.create_element(Tag::Button);
        backend.set_attribute(&node, Attribute::AriaLabel, Some(label));
        node
    }

    fn selector(name: &str) -> Selector {
        Selector {
            role: Role::Button,
            name: name.to_owned(),
        }
    }

    fn panic_message(outcome: std::thread::Result<()>) -> String {
        let payload = outcome.expect_err("cycle insertion must panic");
        if let Some(message) = payload.downcast_ref::<&str>() {
            (*message).to_owned()
        } else if let Some(message) = payload.downcast_ref::<String>() {
            message.clone()
        } else {
            String::new()
        }
    }

    /// Rejects inserting a node into itself before any tree mutation.
    /// 在任何树变更前拒绝把节点插入自身。
    #[test]
    fn inserting_a_node_into_itself_is_rejected() {
        let mut backend = MockBackend::new();
        let root = button(&mut backend, "Root");
        let label = backend.create_text("ready");
        backend.insert_before(&root, &label, None);

        let message = panic_message(catch_unwind(AssertUnwindSafe(|| {
            backend.insert_before(&root, &root, None);
        })));
        assert!(
            message.contains("must not be the inserted child"),
            "{message}"
        );

        // The anchor no-op stays a no-op, and traversals still terminate.
        backend.insert_before(&root, &label, Some(&label));
        assert_eq!(backend.text_content(&root).as_deref(), Some("ready"));
        assert_eq!(backend.find_all(&selector("Root")).len(), 1);
        assert_eq!(backend.a11y_snapshot().nodes.len(), 1);
        assert_eq!(backend.dom_snapshot().nodes.len(), 2);
    }

    /// Rejects inserting an ancestor under its own descendant and keeps legal moves.
    /// 拒绝把祖先插入其后代，并保持合法移动可用。
    #[test]
    fn inserting_an_ancestor_into_its_descendant_is_rejected() {
        let mut backend = MockBackend::new();
        let root = button(&mut backend, "Root");
        let middle = button(&mut backend, "Middle");
        let leaf = button(&mut backend, "Leaf");
        backend.insert_before(&root, &middle, None);
        backend.insert_before(&middle, &leaf, None);
        let text = backend.create_text("ready");
        backend.insert_before(&leaf, &text, None);

        let message = panic_message(catch_unwind(AssertUnwindSafe(|| {
            backend.insert_before(&leaf, &root, None);
        })));
        assert!(
            message.contains("must not be the inserted child"),
            "{message}"
        );

        assert_eq!(backend.text_content(&root).as_deref(), Some("ready"));
        assert_eq!(backend.find_all(&selector("Root")).len(), 1);
        assert_eq!(backend.find_all(&selector("Leaf")).len(), 1);
        assert_eq!(backend.a11y_snapshot().nodes.len(), 3);
        assert_eq!(backend.dom_snapshot().nodes.len(), 4);

        backend.insert_before(&root, &leaf, Some(&middle));
        assert_eq!(backend.text_content(&root).as_deref(), Some("ready"));
        assert_eq!(backend.find_all(&selector("Leaf")).len(), 1);
    }
}
