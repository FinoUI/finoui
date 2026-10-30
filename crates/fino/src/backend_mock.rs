//! Non-rendering in-memory backend with a deterministic tree and operation log.
//! 具有确定性树与操作日志的无渲染内存后端。

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    dom_api::{Attribute, DomBackend, EventKind, EventListener, Role, Selector, Tag},
    headless::{
        A11yNodeSnapshot, A11ySnapshot, BackendOperation, DomNodeKind, DomNodeSnapshot,
        DomSnapshot, HeadlessBackend, SnapshotKind,
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
}

impl MockNodeData {
    fn parent(&self) -> Option<MockNode> {
        match self {
            Self::Element { parent, .. } | Self::Text { parent, .. } => *parent,
        }
    }

    fn set_parent(&mut self, new_parent: Option<MockNode>) {
        match self {
            Self::Element { parent, .. } | Self::Text { parent, .. } => *parent = new_parent,
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
    listeners: BTreeMap<u64, ListenerRegistration>,
    operations: Vec<BackendOp>,
}

impl MockBackend {
    /// Creates an empty mock backend.
    /// 创建空 Mock 后端。
    pub fn new() -> Self {
        Self::default()
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
            None => false,
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
        self.operations.push(BackendOp::CreateText);
        node
    }

    fn insert_before(
        &mut self,
        parent: &Self::Node,
        child: &Self::Node,
        anchor: Option<&Self::Node>,
    ) {
        if anchor == Some(child) {
            return;
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
        self.operations.push(BackendOp::Insert);
    }

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
        self.operations.push(BackendOp::Remove);
    }

    fn set_text(&mut self, node: &Self::Node, value: &str) {
        let Some(MockNodeData::Text { value: current, .. }) = self.nodes.get_mut(node) else {
            panic!("set_text target must be an existing text node");
        };
        *current = value.to_owned();
        self.operations.push(BackendOp::SetText {
            node: *node,
            value: value.to_owned(),
        });
    }

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

    fn supports_snapshot(&self, kind: SnapshotKind) -> bool {
        matches!(
            kind,
            SnapshotKind::Dom | SnapshotKind::A11y | SnapshotKind::Reactive | SnapshotKind::Trace
        )
    }

    fn find_all(&self, selector: &Selector) -> Vec<Self::Node> {
        self.nodes
            .iter()
            .filter_map(|(node, data)| match data {
                MockNodeData::Element {
                    tag: Tag::Button,
                    attributes,
                    ..
                } if selector.role == Role::Button
                    && attributes.get(&Attribute::AriaLabel) == Some(&selector.name) =>
                {
                    Some(*node)
                }
                _ => None,
            })
            .collect()
    }

    fn text_content(&self, node: &Self::Node) -> Option<String> {
        let mut output = String::new();
        self.append_text(*node, &mut output).then_some(output)
    }

    fn listener(&self, node: &Self::Node, event: EventKind) -> Option<EventListener> {
        self.listeners
            .values()
            .find(|registration| registration.node == *node && registration.event == event)
            .map(|registration| registration.callback.clone())
    }

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
                }
            })
            .collect();
        DomSnapshot {
            schema: 1,
            roots,
            nodes,
        }
    }

    fn a11y_snapshot(&self) -> A11ySnapshot {
        let nodes = self
            .nodes
            .iter()
            .filter_map(|(node, data)| match data {
                MockNodeData::Element {
                    tag: Tag::Button,
                    attributes,
                    ..
                } => Some(A11yNodeSnapshot {
                    id: format!("node-{}", node.0),
                    role: Role::Button,
                    name: attributes
                        .get(&Attribute::AriaLabel)
                        .cloned()
                        .unwrap_or_default(),
                    focusable: true,
                    actions: vec!["click".to_owned()],
                    states: BTreeMap::new(),
                    text: self.text_content(node).unwrap_or_default(),
                }),
                _ => None,
            })
            .collect();
        A11ySnapshot { schema: 1, nodes }
    }

    fn operation_log(&self) -> Vec<BackendOperation> {
        self.operations
            .iter()
            .map(|operation| BackendOperation {
                operation: operation.operation().to_owned(),
                details: serde_json::to_value(operation).unwrap_or(Value::Null),
            })
            .collect()
    }
}
