//! Native DOM arena with generational node IDs, text state, layout, and paint.
//! 带 generational 节点 ID、文本状态、布局与绘制的原生 DOM arena。

use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use serde::Serialize;
use serde_json::{Value, json};
use slotmap::{SlotMap, new_key_type};

use super::{
    geom::{Point, Rect, Size},
    id::WindowId,
    layout::{ButtonStyle, LayoutBoxSnapshot, LayoutSnapshot, WindowConstraints},
    paint::{Color, Damage, DisplayList, PaintContext},
    signals::WindowSignals,
};
use crate::{
    dom_api::{Attribute, DomBackend, EventKind, EventListener, Role, Selector, Tag},
    headless::{
        A11yNodeSnapshot, A11ySnapshot, BackendOperation, DomNodeKind, DomNodeSnapshot,
        DomSnapshot, HeadlessBackend, SnapshotKind,
    },
    text::{ShapedText, TextConstraints, TextEngine, TextLayout, TextSpec},
};

new_key_type! {
    /// Generational native node handle; a released handle never aliases a replacement.
    /// Generational 原生节点句柄；已释放句柄绝不会指向替代节点。
    pub struct NodeId;
}

/// Stable listener registration handle.
/// 稳定的监听注册句柄。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeListener(u64);

/// Observable native mutation used by trace artifacts.
/// 用于 trace 产物的可观测原生变更。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum NativeBackendOp {
    CreateElement(Tag),
    CreateText {
        node: u64,
    },
    Insert {
        parent: u64,
        child: u64,
        anchor: Option<u64>,
    },
    Remove {
        node: u64,
    },
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
}

impl NativeBackendOp {
    /// Maps each mutation to the stable verb used by normalized trace events.
    /// 将每种变更映射为规范化 trace 事件使用的稳定操作名。
    fn operation(&self) -> &'static str {
        match self {
            Self::CreateElement(_) => "create_element",
            Self::CreateText { .. } => "create_text",
            Self::Insert { .. } => "insert",
            Self::Remove { .. } => "remove",
            Self::SetText { .. } => "set_text",
            Self::SetAttribute { .. } => "set_attribute",
            Self::Listen(_) => "listen",
            Self::Unlisten(_) => "unlisten",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NodeKind {
    Element(Tag),
    Text,
}

struct TextBinding {
    spec: TextSpec,
    shaped: Option<ShapedText>,
    layout: Option<Rc<TextLayout>>,
    layout_width: Option<u32>,
}

impl TextBinding {
    fn new(text: &str) -> Self {
        Self {
            spec: TextSpec::counter(text),
            shaped: None,
            layout: None,
            layout_width: None,
        }
    }
}

struct NativeNode {
    stable_id: u64,
    kind: NodeKind,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    attributes: BTreeMap<Attribute, String>,
    text: Option<TextBinding>,
}

impl NativeNode {
    fn is_element(&self, tag: Tag) -> bool {
        self.kind == NodeKind::Element(tag)
    }
}

struct ListenerRegistration {
    node: NodeId,
    event: EventKind,
    callback: EventListener,
}

/// Native DOM arena bound to one window and its invalidation signals.
/// 绑定单个窗口及其失效信号的原生 DOM arena。
pub struct NativeDom {
    window: WindowId,
    backend_name: &'static str,
    text_engine: Rc<RefCell<TextEngine>>,
    signals: Rc<WindowSignals>,
    nodes: SlotMap<NodeId, NativeNode>,
    roots: Vec<NodeId>,
    listeners: BTreeMap<u64, ListenerRegistration>,
    next_listener: u64,
    next_stable_id: u64,
    operations: Vec<BackendOperation>,
    viewport: Size,
    dpr: f32,
    bounds: BTreeMap<NodeId, Rect>,
    dirty_nodes: Vec<NodeId>,
    damage: Damage,
    layout_dirty: bool,
    last_layout: Option<LayoutSnapshot>,
    last_display_list: Option<DisplayList>,
    style: ButtonStyle,
}

impl NativeDom {
    /// Creates a native DOM for a real window.
    /// 为真实窗口创建原生 DOM。
    pub fn new_window(
        window: WindowId,
        text_engine: Rc<RefCell<TextEngine>>,
        viewport: Size,
        dpr: f32,
    ) -> Self {
        Self::new(
            window,
            "native",
            text_engine,
            Rc::new(WindowSignals::new(window)),
            viewport,
            dpr,
        )
    }

    /// Creates a native DOM for the deterministic offscreen backend.
    /// 为确定性离屏后端创建原生 DOM。
    pub fn new_offscreen(
        window: WindowId,
        text_engine: Rc<RefCell<TextEngine>>,
        viewport: Size,
        dpr: f32,
    ) -> Self {
        Self::new(
            window,
            "native-offscreen",
            text_engine,
            Rc::new(WindowSignals::new(window)),
            viewport,
            dpr,
        )
    }

    fn new(
        window: WindowId,
        backend_name: &'static str,
        text_engine: Rc<RefCell<TextEngine>>,
        signals: Rc<WindowSignals>,
        viewport: Size,
        dpr: f32,
    ) -> Self {
        Self {
            window,
            backend_name,
            text_engine,
            signals,
            nodes: SlotMap::with_key(),
            roots: Vec::new(),
            listeners: BTreeMap::new(),
            next_listener: 0,
            next_stable_id: 0,
            operations: Vec::new(),
            viewport,
            dpr,
            bounds: BTreeMap::new(),
            dirty_nodes: Vec::new(),
            damage: Damage::full(),
            layout_dirty: true,
            last_layout: None,
            last_display_list: None,
            style: ButtonStyle::default(),
        }
    }

    /// Returns the owning window.
    /// 返回所属窗口。
    pub fn window(&self) -> WindowId {
        self.window
    }

    /// Returns the shared invalidation state.
    /// 返回共享失效状态。
    pub fn signals(&self) -> Rc<WindowSignals> {
        Rc::clone(&self.signals)
    }

    /// Returns the logical viewport.
    /// 返回逻辑视口。
    pub fn viewport(&self) -> Size {
        self.viewport
    }

    /// Returns the device pixel ratio used by the renderer.
    /// 返回渲染器使用的设备像素比。
    pub fn dpr(&self) -> f32 {
        self.dpr
    }

    /// Updates the logical viewport and requests a coalesced frame when it changed.
    /// 更新逻辑视口，并在变化时请求合并帧。
    pub fn set_viewport(&mut self, viewport: Size, dpr: f32) {
        if viewport == self.viewport && dpr == self.dpr {
            return;
        }
        self.viewport = viewport;
        self.dpr = dpr;
        self.layout_dirty = true;
        self.damage = Damage::full();
        self.signals.mark_layout_dirty();
    }

    /// Returns the stable creation-order identifier for a live node.
    /// 返回仍存活节点的稳定创建顺序标识。
    pub fn stable_id(&self, node: NodeId) -> Option<u64> {
        self.nodes.get(node).map(|data| data.stable_id)
    }

    /// Returns whether a node handle still points at live storage.
    /// 返回节点句柄是否仍指向存活存储。
    pub fn contains(&self, node: NodeId) -> bool {
        self.nodes.contains_key(node)
    }

    /// Returns committed bounds without running layout.
    /// 读取已提交边界，不运行布局。
    pub fn committed_bounds(&self, window: WindowId, node: NodeId) -> Option<Rect> {
        assert_eq!(
            window, self.window,
            "geometry reads must target their window"
        );
        self.bounds.get(&node).copied()
    }

    /// Runs layout if needed and returns fresh bounds for one node.
    /// 按需运行布局并返回单个节点的最新边界。
    pub fn request_current_bounds(
        &mut self,
        window: WindowId,
        node: NodeId,
        viewport: Size,
    ) -> Option<Rect> {
        self.layout(WindowConstraints::new(window, viewport));
        self.bounds.get(&node).copied()
    }

    /// Returns concatenated descendant text for one node.
    /// 返回单个节点的拼接后代文本。
    pub fn text_content(&self, node: NodeId) -> Option<String> {
        let mut output = String::new();
        self.append_text(node, &mut output).then_some(output)
    }

    /// Runs the minimal button layout if invalidated and returns a committed snapshot.
    /// 在失效时运行最小按钮布局并返回已提交快照。
    pub fn layout(&mut self, constraints: WindowConstraints) -> LayoutSnapshot {
        assert_eq!(
            constraints.window, self.window,
            "layout must target its window"
        );
        if constraints.viewport != self.viewport {
            self.set_viewport(constraints.viewport, self.dpr);
        }
        if !self.layout_dirty
            && let Some(snapshot) = &self.last_layout
            && snapshot.viewport == self.viewport
        {
            return snapshot.clone();
        }

        self.bounds.clear();
        let roots = self.roots.clone();
        let mut boxes = Vec::new();
        let mut y = self.style.margin;
        let available_text_width =
            (self.viewport.width - self.style.margin * 2.0 - self.style.padding_x * 2.0).max(0.0);

        for root in roots {
            let Some(node) = self.nodes.get(root) else {
                continue;
            };
            match node.kind {
                NodeKind::Element(Tag::Button) => {
                    let children = node.children.clone();
                    let mut text_layouts = Vec::new();
                    let mut text_width = 0.0_f32;
                    let mut text_height = 0.0_f32;
                    for child in &children {
                        if let Some((layout, text)) =
                            self.ensure_text_layout(*child, Some(available_text_width))
                        {
                            text_width += layout.width();
                            text_height = text_height.max(layout.height());
                            text_layouts.push((*child, layout, text));
                        }
                    }
                    let width = (text_width + self.style.padding_x * 2.0)
                        .max(self.style.min_width)
                        .min(self.viewport.width.max(self.style.min_width));
                    let height =
                        (text_height + self.style.padding_y * 2.0).max(self.style.min_height);
                    let button_rect = Rect::new(self.style.margin, y, width, height);
                    self.bounds.insert(root, button_rect);
                    boxes.push(self.layout_box(root, button_rect));

                    let mut text_x = button_rect.x + self.style.padding_x;
                    let text_y = button_rect.y + (button_rect.height - text_height) * 0.5;
                    for (child, layout, _) in text_layouts {
                        let rect = Rect::new(text_x, text_y, layout.width(), layout.height());
                        self.bounds.insert(child, rect);
                        boxes.push(self.layout_box(child, rect));
                        text_x += layout.width();
                    }
                    y = button_rect.bottom() + self.style.margin;
                }
                NodeKind::Text => {
                    if let Some((layout, _)) = self.ensure_text_layout(root, None) {
                        let rect = Rect::new(self.style.margin, y, layout.width(), layout.height());
                        self.bounds.insert(root, rect);
                        boxes.push(self.layout_box(root, rect));
                        y = rect.bottom() + self.style.margin;
                    }
                }
            }
        }

        let dirty_nodes = std::mem::take(&mut self.dirty_nodes);
        for node in dirty_nodes {
            if let Some(rect) = self.bounds.get(&node).copied() {
                self.damage.add(rect);
            }
        }
        self.layout_dirty = false;
        let snapshot = LayoutSnapshot {
            schema: 1,
            window: self.window,
            viewport: self.viewport,
            boxes,
            text: self.text_engine.borrow().stats(),
        };
        self.last_layout = Some(snapshot.clone());
        snapshot
    }

    /// Builds the display list for the current window state.
    /// 为当前窗口状态构建绘制列表。
    pub fn paint(&mut self, window: WindowId, viewport: Size) -> DisplayList {
        assert_eq!(window, self.window, "paint must target its window");
        self.layout(WindowConstraints::new(window, viewport));

        let damage = if self.damage.is_empty() {
            Damage::full()
        } else {
            self.damage
        };
        self.damage = Damage::empty();
        let context = PaintContext::new(window);
        let mut list = DisplayList::new(window, viewport, damage);
        let roots = self.roots.clone();

        for root in roots {
            let Some(node) = self.nodes.get(root) else {
                continue;
            };
            if !node.is_element(Tag::Button) {
                continue;
            }
            let Some(bounds) = self.bounds.get(&root).copied() else {
                continue;
            };
            list.push(context.rect(
                root,
                node.stable_id,
                bounds,
                Color::rgba(0.14, 0.16, 0.21, 1.0),
            ));
            for child in &node.children {
                let Some(child_node) = self.nodes.get(*child) else {
                    continue;
                };
                let (Some(text), Some(layout), Some(bounds)) = (
                    child_node.text.as_ref().map(|text| text.spec.text.clone()),
                    child_node
                        .text
                        .as_ref()
                        .and_then(|text| text.layout.clone()),
                    self.bounds.get(child).copied(),
                ) else {
                    continue;
                };
                list.push(context.text(
                    *child,
                    child_node.stable_id,
                    bounds,
                    text,
                    layout,
                    Color::rgba(0.95, 0.96, 0.99, 1.0),
                ));
            }
        }

        self.signals.commit_paint();
        self.last_display_list = Some(list.clone());
        list
    }

    /// Returns the most recently painted display list.
    /// 返回最近一次绘制的显示列表。
    pub fn last_display_list(&self) -> Option<&DisplayList> {
        self.last_display_list.as_ref()
    }

    /// Hit tests a window-tagged point against the topmost interactive control.
    /// 使用带窗口身份的点测试最上层交互控件。
    pub fn hit_test(&self, window: WindowId, point: Point) -> Option<NodeId> {
        assert_eq!(window, self.window, "hit testing must target its window");
        for root in self.roots.iter().rev() {
            let Some(node) = self.nodes.get(*root) else {
                continue;
            };
            if !node.is_element(Tag::Button) {
                continue;
            }
            if self
                .bounds
                .get(root)
                .is_some_and(|bounds| bounds.contains(point))
            {
                return Some(*root);
            }
        }
        None
    }

    /// Resolves a semantic selector against reachable native nodes.
    /// 在可达原生节点中解析语义选择器。
    pub fn find_all(&self, selector: &Selector) -> Vec<NodeId> {
        self.reachable_nodes()
            .into_iter()
            .filter(|node| {
                self.nodes.get(*node).is_some_and(|data| {
                    data.is_element(Tag::Button)
                        && selector.role == Role::Button
                        && data.attributes.get(&Attribute::AriaLabel) == Some(&selector.name)
                })
            })
            .collect()
    }

    /// Finds a registered callback without retaining a DOM borrow during dispatch.
    /// 查找已注册回调，分发期间不保持 DOM 借用。
    pub fn listener(&self, node: NodeId, event: EventKind) -> Option<EventListener> {
        self.listeners
            .values()
            .find(|registration| registration.node == node && registration.event == event)
            .map(|registration| registration.callback.clone())
    }

    /// Returns the live listener count.
    /// 返回仍存活的监听数量。
    pub fn listener_count(&self) -> usize {
        self.listeners.len()
    }

    /// Returns whether a non-empty frame request is pending.
    /// 返回是否存在待处理帧请求。
    pub fn frame_request_pending(&self) -> bool {
        self.signals.frame_request_pending()
    }

    /// Takes one coalesced frame request.
    /// 取走一个合并帧请求。
    pub fn take_frame_request(&self) -> bool {
        self.signals.take_frame_request()
    }

    fn ensure_text_layout(
        &mut self,
        node: NodeId,
        max_width: Option<f32>,
    ) -> Option<(Rc<TextLayout>, String)> {
        let engine = Rc::clone(&self.text_engine);
        let binding = self.nodes.get_mut(node)?.text.as_mut()?;
        let width_bits = max_width.map(f32::to_bits);
        if binding.layout_width == width_bits
            && let Some(layout) = &binding.layout
        {
            return Some((Rc::clone(layout), binding.spec.text.clone()));
        }

        if binding.shaped.is_none() {
            binding.shaped = Some(engine.borrow_mut().shape(&binding.spec));
        }
        let shaped = binding
            .shaped
            .as_ref()
            .expect("shaped text was just created");
        let layout = Rc::new(
            engine
                .borrow_mut()
                .layout(shaped, TextConstraints::new(max_width, None)),
        );
        binding.layout = Some(Rc::clone(&layout));
        binding.layout_width = width_bits;
        Some((layout, binding.spec.text.clone()))
    }

    fn layout_box(&self, node: NodeId, rect: Rect) -> LayoutBoxSnapshot {
        let data = self.nodes.get(node).expect("layout box node must exist");
        let (tag, text) = match data.kind {
            NodeKind::Element(Tag::Button) => (Some("button".to_owned()), None),
            NodeKind::Text => (None, data.text.as_ref().map(|text| text.spec.text.clone())),
        };
        LayoutBoxSnapshot {
            id: format!("node-{}", data.stable_id),
            tag,
            text,
            rect,
        }
    }

    fn allocate(&mut self, kind: NodeKind, text: Option<TextBinding>) -> NodeId {
        self.next_stable_id += 1;
        let stable_id = self.next_stable_id;
        let node = self.nodes.insert(NativeNode {
            stable_id,
            kind,
            parent: None,
            children: Vec::new(),
            attributes: BTreeMap::new(),
            text,
        });
        self.roots.push(node);
        node
    }

    fn mark_structural_change(&mut self) {
        self.layout_dirty = true;
        self.damage = Damage::full();
        self.signals.mark_layout_dirty();
    }

    fn mark_node_damage(&mut self, node: NodeId) {
        if let Some(bounds) = self.bounds.get(&node).copied() {
            self.damage.add(bounds);
        }
        if !self.dirty_nodes.contains(&node) {
            self.dirty_nodes.push(node);
        }
    }

    fn append_text(&self, node: NodeId, output: &mut String) -> bool {
        let Some(data) = self.nodes.get(node) else {
            return false;
        };
        match data.kind {
            NodeKind::Text => {
                if let Some(text) = &data.text {
                    output.push_str(&text.spec.text);
                }
                true
            }
            NodeKind::Element(_) => {
                for child in &data.children {
                    if !self.append_text(*child, output) {
                        return false;
                    }
                }
                true
            }
        }
    }

    fn collect_reachable(&self, node: NodeId, output: &mut Vec<NodeId>) {
        if !self.nodes.contains_key(node) {
            return;
        }
        output.push(node);
        if let Some(data) = self.nodes.get(node) {
            for child in &data.children {
                self.collect_reachable(*child, output);
            }
        }
    }

    fn reachable_nodes(&self) -> Vec<NodeId> {
        let mut roots = self.roots.clone();
        roots.sort_by_key(|node| self.nodes.get(*node).map(|data| data.stable_id));
        let mut nodes = Vec::new();
        for root in roots {
            self.collect_reachable(root, &mut nodes);
        }
        nodes
    }

    /// Removes a subtree and invalidates every released generational handle.
    /// 删除子树，并让所有已释放的 generational 句柄失效。
    fn erase_subtree(&mut self, node: NodeId) {
        let Some(data) = self.nodes.remove(node) else {
            return;
        };
        for child in data.children {
            self.erase_subtree(child);
        }
    }

    fn emit(&mut self, operation: NativeBackendOp) {
        self.operations.push(BackendOperation {
            operation: operation.operation().to_owned(),
            details: serde_json::to_value(operation).unwrap_or(Value::Null),
        });
    }

    fn stable(&self, node: NodeId) -> u64 {
        self.nodes
            .get(node)
            .map(|data| data.stable_id)
            .expect("node must exist")
    }
}

impl DomBackend for NativeDom {
    type Node = NodeId;
    type Listener = NativeListener;

    fn create_element(&mut self, tag: Tag) -> Self::Node {
        let node = self.allocate(NodeKind::Element(tag), None);
        self.emit(NativeBackendOp::CreateElement(tag));
        self.mark_structural_change();
        node
    }

    fn create_text(&mut self, value: &str) -> Self::Node {
        let node = self.allocate(NodeKind::Text, Some(TextBinding::new(value)));
        let stable = self.stable(node);
        self.emit(NativeBackendOp::CreateText { node: stable });
        self.mark_structural_change();
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
        let mut ancestor = Some(*parent);
        while let Some(node) = ancestor {
            assert!(
                node != *child,
                "native insert parent must not be the inserted child or its descendant"
            );
            ancestor = self.nodes.get(node).and_then(|data| data.parent);
        }
        if let Some(old_parent) = self.nodes.get(*child).and_then(|data| data.parent)
            && let Some(data) = self.nodes.get_mut(old_parent)
        {
            data.children.retain(|candidate| candidate != child);
        }
        let Some(parent_data) = self.nodes.get_mut(*parent) else {
            panic!("native insert parent must exist");
        };
        if !matches!(parent_data.kind, NodeKind::Element(_)) {
            panic!("native insert parent must be an element");
        }
        parent_data.children.retain(|candidate| candidate != child);
        let index = anchor.map_or(parent_data.children.len(), |anchor| {
            parent_data
                .children
                .iter()
                .position(|candidate| candidate == anchor)
                .expect("native insert anchor must be a child of the parent")
        });
        parent_data.children.insert(index, *child);
        self.nodes
            .get_mut(*child)
            .expect("inserted native child must exist")
            .parent = Some(*parent);
        self.roots.retain(|root| root != child);
        let (parent_id, child_id, anchor_id) = (
            self.stable(*parent),
            self.stable(*child),
            anchor.map(|a| self.stable(*a)),
        );
        self.emit(NativeBackendOp::Insert {
            parent: parent_id,
            child: child_id,
            anchor: anchor_id,
        });
        self.mark_structural_change();
    }

    fn remove(&mut self, node: &Self::Node) {
        let Some(data) = self.nodes.get(*node) else {
            return;
        };
        let stable = data.stable_id;
        if let Some(parent) = data.parent
            && let Some(parent_data) = self.nodes.get_mut(parent)
        {
            parent_data.children.retain(|candidate| candidate != node);
        }
        self.roots.retain(|root| root != node);
        self.erase_subtree(*node);
        self.listeners
            .retain(|_, registration| self.nodes.contains_key(registration.node));
        self.emit(NativeBackendOp::Remove { node: stable });
        self.mark_structural_change();
    }

    fn set_text(&mut self, node: &Self::Node, value: &str) {
        let stable = self.stable(*node);
        let Some(binding) = self
            .nodes
            .get_mut(*node)
            .and_then(|data| data.text.as_mut())
        else {
            panic!("native set_text target must be a text node");
        };
        if binding.spec.text == value {
            return;
        }
        binding.spec.text.clear();
        binding.spec.text.push_str(value);
        binding.shaped = None;
        binding.layout = None;
        binding.layout_width = None;
        self.mark_node_damage(*node);
        self.layout_dirty = true;
        self.emit(NativeBackendOp::SetText {
            node: stable,
            value: value.to_owned(),
        });
        self.signals.mark_layout_dirty();
    }

    fn set_attribute(&mut self, node: &Self::Node, name: Attribute, value: Option<&str>) {
        let stable = self.stable(*node);
        let Some(data) = self.nodes.get_mut(*node) else {
            panic!("native attribute target must exist");
        };
        if !matches!(data.kind, NodeKind::Element(_)) {
            panic!("native attribute target must be an element");
        }
        let previous = match value {
            Some(value) => data.attributes.insert(name, value.to_owned()),
            None => data.attributes.remove(&name),
        };
        if previous.as_deref() != value {
            self.signals.mark_accessibility_dirty();
        }
        self.emit(NativeBackendOp::SetAttribute {
            node: stable,
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
        assert!(self.nodes.contains_key(*node), "listener node must exist");
        self.next_listener += 1;
        let handle = NativeListener(self.next_listener);
        self.listeners.insert(
            handle.0,
            ListenerRegistration {
                node: *node,
                event,
                callback: listener,
            },
        );
        self.emit(NativeBackendOp::Listen(event));
        handle
    }

    fn unlisten(&mut self, listener: Self::Listener) {
        if let Some(registration) = self.listeners.remove(&listener.0) {
            self.emit(NativeBackendOp::Unlisten(registration.event));
        }
    }
}

impl HeadlessBackend for NativeDom {
    fn backend_name(&self) -> &'static str {
        self.backend_name
    }

    fn supports_snapshot(&self, kind: SnapshotKind) -> bool {
        matches!(
            kind,
            SnapshotKind::Dom
                | SnapshotKind::A11y
                | SnapshotKind::Reactive
                | SnapshotKind::Trace
                | SnapshotKind::Layout
                | SnapshotKind::DisplayList
        )
    }

    fn find_all(&self, selector: &Selector) -> Vec<Self::Node> {
        NativeDom::find_all(self, selector)
    }

    fn text_content(&self, node: &Self::Node) -> Option<String> {
        NativeDom::text_content(self, *node)
    }

    fn listener(&self, node: &Self::Node, event: EventKind) -> Option<EventListener> {
        NativeDom::listener(self, *node, event)
    }

    fn dom_snapshot(&self) -> DomSnapshot {
        let mut nodes: Vec<&NativeNode> = self.nodes.values().collect();
        nodes.sort_by_key(|node| node.stable_id);
        let roots = {
            let mut roots = self.roots.clone();
            roots.sort_by_key(|node| self.nodes.get(*node).map(|data| data.stable_id));
            roots
                .into_iter()
                .filter_map(|node| {
                    self.nodes
                        .get(node)
                        .map(|data| format!("node-{}", data.stable_id))
                })
                .collect()
        };
        let nodes = nodes
            .into_iter()
            .map(|node| {
                let (kind, tag, text, attributes) = match node.kind {
                    NodeKind::Element(Tag::Button) => {
                        let attributes = node
                            .attributes
                            .iter()
                            .map(|(name, value)| {
                                let name = match name {
                                    Attribute::AriaLabel => "aria-label".to_owned(),
                                };
                                (name, value.clone())
                            })
                            .collect();
                        (
                            DomNodeKind::Element,
                            Some("button".to_owned()),
                            None,
                            attributes,
                        )
                    }
                    NodeKind::Text => (
                        DomNodeKind::Text,
                        None,
                        node.text.as_ref().map(|text| text.spec.text.clone()),
                        BTreeMap::new(),
                    ),
                };
                DomNodeSnapshot {
                    id: format!("node-{}", node.stable_id),
                    kind,
                    parent: node
                        .parent
                        .and_then(|parent| self.nodes.get(parent))
                        .map(|parent| format!("node-{}", parent.stable_id)),
                    children: node
                        .children
                        .iter()
                        .filter_map(|child| {
                            self.nodes
                                .get(*child)
                                .map(|child| format!("node-{}", child.stable_id))
                        })
                        .collect(),
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

    fn a11y_snapshot(&self) -> A11ySnapshot {
        let nodes = self
            .reachable_nodes()
            .into_iter()
            .filter_map(|node| {
                let data = self.nodes.get(node)?;
                if !data.is_element(Tag::Button) {
                    return None;
                }
                Some(A11yNodeSnapshot {
                    id: format!("node-{}", data.stable_id),
                    role: Role::Button,
                    name: data
                        .attributes
                        .get(&Attribute::AriaLabel)
                        .cloned()
                        .unwrap_or_default(),
                    focusable: true,
                    actions: vec!["click".to_owned()],
                    states: BTreeMap::new(),
                    text: self.text_content(node).unwrap_or_default(),
                })
            })
            .collect();
        A11ySnapshot { schema: 1, nodes }
    }

    fn operation_log(&self) -> Vec<BackendOperation> {
        self.operations.clone()
    }

    fn layout_snapshot(&mut self) -> Option<Value> {
        let snapshot = self.layout(WindowConstraints::new(self.window, self.viewport));
        Some(json!(snapshot))
    }

    fn display_list_snapshot(&mut self) -> Option<Value> {
        let list = self.paint(self.window, self.viewport);
        Some(list.to_snapshot())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::geom::Point;

    fn dom() -> NativeDom {
        NativeDom::new_offscreen(
            WindowId::new(1),
            Rc::new(RefCell::new(TextEngine::new())),
            Size::new(800.0, 600.0),
            1.0,
        )
    }

    fn button_with_text(dom: &mut NativeDom) -> (NodeId, NodeId, NodeId) {
        let button = dom.create_element(Tag::Button);
        dom.set_attribute(&button, Attribute::AriaLabel, Some("Increment counter"));
        let prefix = dom.create_text("Count: ");
        let value = dom.create_text("0");
        dom.insert_before(&button, &prefix, None);
        dom.insert_before(&button, &value, None);
        (button, prefix, value)
    }

    /// A released generational handle never resolves to a replacement node.
    /// 已释放的 generational 句柄绝不会解析到替代节点。
    #[test]
    fn released_node_ids_do_not_alias_replacements() {
        let mut dom = dom();
        let old = dom.create_text("old");
        let old_stable = dom.stable_id(old).expect("stable id");
        dom.remove(&old);
        assert!(!dom.contains(old));
        let replacement = dom.create_text("new");
        assert_ne!(old, replacement);
        assert_ne!(some_stable(&dom, replacement), old_stable);
        assert_eq!(dom.text_content(old), None);
    }

    fn some_stable(dom: &NativeDom, node: NodeId) -> u64 {
        dom.stable_id(node).expect("stable id")
    }

    /// Text content changes reshape one node; width-only changes reuse shaping.
    /// 文本内容变化只重新 shaping 一个节点；仅宽度变化复用 shaping。
    #[test]
    fn text_changes_shape_only_the_changed_text() {
        let mut dom = dom();
        let (button, _prefix, value) = button_with_text(&mut dom);
        let first = dom.layout(WindowConstraints::new(dom.window(), dom.viewport()));
        assert_eq!(first.text.shape_calls, 2);
        let first_layouts = first.text.layout_calls;

        dom.set_text(&value, "1");
        let second = dom.layout(WindowConstraints::new(dom.window(), dom.viewport()));
        assert_eq!(second.text.shape_calls, 3);
        assert_eq!(second.text.layout_calls, first_layouts + 1);

        dom.set_viewport(Size::new(900.0, 600.0), 1.0);
        let third = dom.layout(WindowConstraints::new(dom.window(), dom.viewport()));
        assert_eq!(
            third.text.shape_calls, 3,
            "width-only change reuses shaping"
        );
        assert_eq!(
            third.text.layout_calls,
            second.text.layout_calls + 2,
            "both visible text nodes re-layout for the new width"
        );
        assert_eq!(dom.text_content(button).as_deref(), Some("Count: 1"));
    }

    /// Unchanged content and accessibility-only writes do not request frames.
    /// 未变化内容与仅无障碍写入不请求帧。
    #[test]
    fn non_visual_changes_do_not_request_frames() {
        let mut dom = dom();
        let (button, _prefix, value) = button_with_text(&mut dom);
        dom.layout(WindowConstraints::new(dom.window(), dom.viewport()));
        dom.take_frame_request();

        dom.set_text(&value, "0");
        assert!(!dom.frame_request_pending(), "same text is a no-op");

        dom.set_attribute(&button, Attribute::AriaLabel, Some("Renamed"));
        assert!(!dom.frame_request_pending(), "aria-label is not visual");
        assert!(dom.signals().snapshot().accessibility_dirty);

        dom.set_text(&value, "1");
        assert!(
            dom.take_frame_request(),
            "visual text change requests a frame"
        );
        assert!(!dom.take_frame_request(), "requests are coalesced");
    }

    /// Layout and display-list snapshots contain real window-tagged geometry.
    /// 布局与显示列表快照包含真实的带窗口身份几何。
    #[test]
    fn layout_and_display_list_capture_real_geometry() {
        let mut dom = dom();
        let (button, _prefix, _value) = button_with_text(&mut dom);
        let layout = dom.layout(WindowConstraints::new(dom.window(), dom.viewport()));
        assert_eq!(layout.boxes.len(), 3);
        assert_eq!(layout.boxes[0].tag.as_deref(), Some("button"));
        let bounds = dom
            .committed_bounds(dom.window(), button)
            .expect("button bounds");
        assert_eq!(dom.hit_test(dom.window(), bounds.center()), Some(button));
        assert_eq!(
            dom.hit_test(
                dom.window(),
                Point::new(bounds.right() + 1.0, bounds.center().y)
            ),
            None
        );

        let list = dom.paint(dom.window(), dom.viewport());
        assert_eq!(list.window(), dom.window());
        assert_eq!(list.commands().len(), 3);
        assert!(
            list.commands()
                .iter()
                .all(|command| command.window() == dom.window())
        );
        let snapshot = list.to_snapshot();
        assert_eq!(snapshot["schema"], 1);
        assert_eq!(snapshot["commands"].as_array().map(Vec::len), Some(3));
    }
}
