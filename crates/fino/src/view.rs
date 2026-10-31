//! Backend-neutral RSX authoring support used by the `view!` macro.
//! `view!` 宏使用的后端无关 RSX 作者支持。

use std::{any::Any, cell::RefCell, fmt, rc::Rc};

use crate::{
    dom_api::{Attribute, DomBackend, Event, EventContext, EventKind, Tag},
    reactive::{Owner, Signal},
    runtime::Runtime,
};

thread_local! {
    // Component-local signals resolve against the owner of the view currently mounting.
    // 组件内 Signal 通过当前正在挂载的 View 所属 Owner 解析。
    static CURRENT_OWNER: RefCell<Option<Owner>> = const { RefCell::new(None) };
}

/// Creates a Signal on the owner of the View currently mounting.
/// 在正在挂载的 View 所属 Owner 上创建 Signal。
///
/// ```
/// use fino::{backend_mock::MockBackend, reactive::Owner, runtime::Runtime, signal};
///
/// let runtime = Runtime::new(MockBackend::new());
/// let owner = runtime.create_owner();
/// let count = fino::view::with_owner(&owner, || signal(0));
/// assert_eq!(count.get(), 0);
/// ```
pub fn signal<T: 'static>(value: T) -> Signal<T> {
    current_owner().signal(value)
}

/// Creates a named Signal on the owner of the View currently mounting.
/// 在正在挂载的 View 所属 Owner 上创建命名 Signal。
pub fn signal_named<T: 'static>(name: &str, value: T) -> Signal<T> {
    current_owner().signal_named(name, value)
}

/// Returns the owner installed by the view mount context.
/// 返回 View 挂载上下文安装的 Owner。
fn current_owner() -> Owner {
    CURRENT_OWNER
        .with(|owner| owner.borrow().clone())
        .expect("signal() must be called while a Fino View is mounting")
}

/// Restores the previous component owner even when a component panics.
/// 即使组件 panic 也恢复之前的组件 Owner。
struct OwnerGuard(Option<Owner>);

impl Drop for OwnerGuard {
    fn drop(&mut self) {
        let previous = self.0.take();
        CURRENT_OWNER.with(|owner| owner.replace(previous));
    }
}

/// Installs `owner` for component-local `signal` calls during one mount.
/// 在一次挂载期间为组件内 `signal` 调用安装 `owner`。
fn with_current_owner<T>(owner: &Owner, build: impl FnOnce() -> T) -> T {
    let previous = CURRENT_OWNER.with(|current| current.replace(Some(owner.clone())));
    let _guard = OwnerGuard(previous);
    build()
}

/// Runs a component factory with an owner available to `signal()` calls.
/// 在组件工厂执行期间提供可供 `signal()` 使用的 Owner。
pub fn with_owner<T>(owner: &Owner, component: impl FnOnce() -> T) -> T {
    with_current_owner(owner, component)
}

/// Constructs and mounts a component factory inside one mount context.
/// 在一个挂载上下文中构建并挂载组件工厂。
pub fn mount_component<B, C>(runtime: &mut Runtime<B>, owner: &Owner, component: C) -> B::Node
where
    B: DomBackend + 'static,
    C: FnOnce() -> View,
{
    with_current_owner(owner, component).mount(runtime, owner)
}

/// Static template node description.
/// 静态模板节点描述。
#[derive(Clone, Debug)]
enum TemplateNode {
    Element {
        tag: Tag,
        attributes: Vec<(Attribute, String)>,
    },
    Text(String),
    TextSlot,
}

/// Static tag, static attributes, and static text lowered by `view!`.
/// 由 `view!` 下沉的静态标签、静态属性与静态文本。
///
/// Dynamic text nodes are represented by an empty [`TextSlot`](Self::text_slot)
/// whose node handle is bound by [`TextBinding`] after mounting.
/// 动态文本节点表示为空的 [`TextSlot`](Self::text_slot)，挂载后由
/// [`TextBinding`] 绑定其节点句柄。
#[derive(Clone, Debug, Default)]
pub struct Template {
    nodes: Vec<(Option<usize>, TemplateNode)>,
}

impl Template {
    /// Creates an empty template builder.
    /// 创建空模板构建器。
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an element and returns its stable template index.
    /// 添加元素并返回其稳定模板索引。
    pub fn element(&mut self, parent: Option<usize>, tag: Tag) -> usize {
        self.push(
            parent,
            TemplateNode::Element {
                tag,
                attributes: Vec::new(),
            },
        )
    }

    /// Adds a static text node and returns its stable template index.
    /// 添加静态文本节点并返回其稳定模板索引。
    pub fn text(&mut self, parent: usize, value: impl Into<String>) -> usize {
        self.push(Some(parent), TemplateNode::Text(value.into()))
    }

    /// Adds an empty text node reserved for a dynamic text binding.
    /// 添加为动态文本绑定预留的空文本节点。
    pub fn text_slot(&mut self, parent: usize) -> usize {
        self.push(Some(parent), TemplateNode::TextSlot)
    }

    /// Adds one static attribute to an existing element.
    /// 为现有元素添加一个静态属性。
    pub fn static_attribute(
        &mut self,
        node: usize,
        attribute: Attribute,
        value: impl Into<String>,
    ) {
        if let Some((_, TemplateNode::Element { attributes, .. })) = self.nodes.get_mut(node) {
            attributes.push((attribute, value.into()));
            return;
        }
        panic!("static attribute target is not an element template");
    }

    /// Creates the template nodes in stable order and inserts children.
    /// 按稳定顺序创建模板节点并插入子节点。
    pub fn mount<B: DomBackend + 'static>(&self, runtime: &mut Runtime<B>) -> Vec<B::Node> {
        let mut nodes = Vec::with_capacity(self.nodes.len());
        for (_, node) in &self.nodes {
            let handle = match node {
                TemplateNode::Element { tag, .. } => runtime.backend_mut().create_element(*tag),
                TemplateNode::Text(value) => runtime.backend_mut().create_text(value),
                TemplateNode::TextSlot => runtime.backend_mut().create_text(""),
            };
            nodes.push(handle);
        }

        for (index, (_, node)) in self.nodes.iter().enumerate() {
            if let TemplateNode::Element { attributes, .. } = node {
                for (attribute, value) in attributes {
                    runtime
                        .backend_mut()
                        .set_attribute(&nodes[index], *attribute, Some(value));
                }
            }
        }

        for (index, (parent, _)) in self.nodes.iter().enumerate() {
            if let Some(parent) = parent {
                let parent = nodes[*parent].clone();
                let child = nodes[index].clone();
                runtime.backend_mut().insert_before(&parent, &child, None);
            }
        }

        nodes
    }

    fn push(&mut self, parent: Option<usize>, node: TemplateNode) -> usize {
        let index = self.nodes.len();
        self.nodes.push((parent, node));
        index
    }
}

/// Dynamic text lowered by `view!` into an owner-scoped effect.
/// 由 `view!` 下沉为 Owner 作用域 Effect 的动态文本。
pub struct TextBinding {
    name: String,
    read: Box<dyn Fn() -> String>,
}

impl TextBinding {
    /// Creates a text binding from a zero-argument reader.
    /// 从未接收参数的读取闭包创建文本绑定。
    pub fn new<F, T>(name: impl Into<String>, read: F) -> Self
    where
        F: Fn() -> T + 'static,
        T: ToString + 'static,
    {
        Self {
            name: name.into(),
            read: Box::new(move || read().to_string()),
        }
    }

    /// Binds the reader to one existing text node.
    /// 将读取闭包绑定到一个现有文本节点。
    pub fn mount<B: DomBackend + 'static>(
        self,
        runtime: &mut Runtime<B>,
        owner: &Owner,
        node: &B::Node,
    ) {
        let node = node.clone();
        let read = self.read;
        runtime.effect(owner, &self.name, move |context| {
            let value = read();
            context.backend_mut().set_text(&node, &value);
        });
    }
}

impl fmt::Debug for TextBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TextBinding")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// Dynamic attribute lowered by `view!` into an owner-scoped effect.
/// 由 `view!` 下沉为 Owner 作用域 Effect 的动态属性。
pub struct AttributeBinding {
    name: String,
    attribute: Attribute,
    read: Box<dyn Fn() -> String>,
}

impl AttributeBinding {
    /// Creates an attribute binding from a zero-argument reader.
    /// 从未接收参数的读取闭包创建属性绑定。
    pub fn new<F, T>(attribute: Attribute, read: F) -> Self
    where
        F: Fn() -> T + 'static,
        T: ToString + 'static,
    {
        Self {
            name: format!("view/attribute/{}", attribute_name(attribute)),
            attribute,
            read: Box::new(move || read().to_string()),
        }
    }

    /// Binds the reader to one existing element.
    /// 将读取闭包绑定到一个现有元素。
    pub fn mount<B: DomBackend + 'static>(
        self,
        runtime: &mut Runtime<B>,
        owner: &Owner,
        node: &B::Node,
    ) {
        let node = node.clone();
        let read = self.read;
        let attribute = self.attribute;
        runtime.effect(owner, &self.name, move |context| {
            let value = read();
            context
                .backend_mut()
                .set_attribute(&node, attribute, Some(&value));
        });
    }
}

impl fmt::Debug for AttributeBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AttributeBinding")
            .field("name", &self.name)
            .field("attribute", &self.attribute)
            .finish_non_exhaustive()
    }
}

type EventHandler = Box<dyn FnMut(Event, &mut EventContext)>;

/// Event lowered by `view!` into an owner-scoped listener.
/// 由 `view!` 下沉为 Owner 作用域监听器的事件。
pub struct EventBinding {
    kind: EventKind,
    handler: EventHandler,
}

impl EventBinding {
    /// Creates a click binding from a zero-argument handler.
    /// 从未接收参数的处理闭包创建点击绑定。
    pub fn click<F>(mut handler: F) -> Self
    where
        F: FnMut() + 'static,
    {
        Self {
            kind: EventKind::Click,
            handler: Box::new(move |_event, _context| handler()),
        }
    }

    /// Registers the binding on one existing element.
    /// 将一个绑定注册到一个现有元素。
    pub fn mount<B: DomBackend + 'static>(
        self,
        runtime: &mut Runtime<B>,
        owner: &Owner,
        node: &B::Node,
    ) {
        runtime.listen(owner, node, self.kind, self.handler);
    }
}

impl fmt::Debug for EventBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EventBinding")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

/// Mounted node handles returned by one `view!` expansion.
/// 一次 `view!` 展开返回的已挂载节点句柄。
pub struct ViewOutput<B: DomBackend> {
    root: B::Node,
    text_slots: Vec<B::Node>,
}

impl<B: DomBackend> ViewOutput<B> {
    /// Creates a mounted view output for macro-generated code.
    /// 为宏生成代码创建已挂载视图输出。
    pub fn new(root: B::Node, text_slots: Vec<B::Node>) -> Self {
        Self { root, text_slots }
    }

    /// Returns the root node handle.
    /// 返回根节点句柄。
    pub fn root(&self) -> &B::Node {
        &self.root
    }

    /// Returns dynamic text node handles in source order.
    /// 按源码顺序返回动态文本节点句柄。
    pub fn text_slots(&self) -> &[B::Node] {
        &self.text_slots
    }

    /// Splits the output into its root and text slots.
    /// 把输出拆分为根节点与动态文本槽。
    pub fn into_parts(self) -> (B::Node, Vec<B::Node>) {
        (self.root, self.text_slots)
    }
}

impl<B: DomBackend> fmt::Debug for ViewOutput<B> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ViewOutput")
            .field("text_slots", &self.text_slots.len())
            .finish_non_exhaustive()
    }
}

/// Backend-erased node handle used while a `view!` closure is being mounted.
/// `view!` 闭包挂载期间使用的后端擦除节点句柄。
pub type ErasedNode = Rc<dyn Any>;

/// Mount context supplied to the macro-generated closure.
/// 提供给宏生成闭包的挂载上下文。
pub trait ViewContext {
    /// Mounts static template nodes and returns erased handles.
    /// 挂载静态模板节点并返回擦除句柄。
    fn mount_template(&mut self, template: &Template) -> Vec<ErasedNode>;
    /// Binds a dynamic text reader to an existing node.
    /// 将动态文本读取闭包绑定到现有节点。
    fn mount_text(&mut self, node: &ErasedNode, binding: TextBinding);
    /// Binds a dynamic attribute reader to an existing element.
    /// 将动态属性读取闭包绑定到现有元素。
    fn mount_attribute(&mut self, node: &ErasedNode, binding: AttributeBinding);
    /// Binds an event listener to an existing element.
    /// 将事件监听器绑定到现有元素。
    fn mount_event(&mut self, node: &ErasedNode, binding: EventBinding);
}

struct TypedViewContext<'a, B: DomBackend> {
    runtime: &'a mut Runtime<B>,
    owner: &'a Owner,
}

impl<B: DomBackend + 'static> ViewContext for TypedViewContext<'_, B> {
    fn mount_template(&mut self, template: &Template) -> Vec<ErasedNode> {
        template
            .mount(self.runtime)
            .into_iter()
            .map(|node| Rc::new(node) as ErasedNode)
            .collect()
    }

    fn mount_text(&mut self, node: &ErasedNode, binding: TextBinding) {
        let node = downcast_node::<B>(node, "text");
        binding.mount(self.runtime, self.owner, &node);
    }

    fn mount_attribute(&mut self, node: &ErasedNode, binding: AttributeBinding) {
        let node = downcast_node::<B>(node, "attribute");
        binding.mount(self.runtime, self.owner, &node);
    }

    fn mount_event(&mut self, node: &ErasedNode, binding: EventBinding) {
        let node = downcast_node::<B>(node, "event");
        binding.mount(self.runtime, self.owner, &node);
    }
}

fn downcast_node<B: DomBackend>(node: &ErasedNode, kind: &str) -> B::Node {
    node.downcast_ref::<B::Node>()
        .unwrap_or_else(|| panic!("view {kind} node has the wrong backend type"))
        .clone()
}

/// Backend-erased output returned by one `view!` build closure.
/// 一次 `view!` 构建闭包返回的后端擦除输出。
pub struct BuildOutput {
    root: ErasedNode,
    text_slots: Vec<ErasedNode>,
}

impl BuildOutput {
    /// Creates a build output for macro-generated code.
    /// 为宏生成代码创建构建输出。
    pub fn new(root: ErasedNode, text_slots: Vec<ErasedNode>) -> Self {
        Self { root, text_slots }
    }
}

type ViewBuilder = Box<dyn FnOnce(&mut dyn ViewContext) -> BuildOutput>;

/// A component that executes once and builds a mounted view.
/// 只执行一次并构建已挂载视图的组件。
pub struct View {
    build: ViewBuilder,
}

impl View {
    /// Creates a component from a one-shot build closure.
    /// 从一次性构建闭包创建组件。
    pub fn new(build: impl FnOnce(&mut dyn ViewContext) -> BuildOutput + 'static) -> Self {
        Self {
            build: Box::new(build),
        }
    }

    /// Mounts the component once and returns its root node.
    /// 只挂载组件一次并返回根节点。
    pub fn mount<B: DomBackend + 'static>(
        self,
        runtime: &mut Runtime<B>,
        owner: &Owner,
    ) -> B::Node {
        self.mount_with_slots(runtime, owner).root
    }

    /// Mounts the component once and returns root and dynamic text slots.
    /// 只挂载组件一次并返回根节点与动态文本槽。
    pub fn mount_with_slots<B: DomBackend + 'static>(
        self,
        runtime: &mut Runtime<B>,
        owner: &Owner,
    ) -> ViewOutput<B> {
        let View { build } = self;
        runtime.component_executions().increment();
        let output = with_current_owner(owner, || {
            let mut context = TypedViewContext { runtime, owner };
            build(&mut context)
        });
        let root = downcast_node::<B>(&output.root, "root");
        let text_slots = output
            .text_slots
            .iter()
            .map(|node| downcast_node::<B>(node, "text slot"))
            .collect();
        ViewOutput::new(root, text_slots)
    }
}

impl fmt::Debug for View {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("View").finish_non_exhaustive()
    }
}

fn attribute_name(attribute: Attribute) -> &'static str {
    match attribute {
        Attribute::AriaLabel => "aria-label",
    }
}
