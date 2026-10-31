//! Application-to-window context structure, input routing, and lifecycle state.
//! Application 到窗口上下文的结构、输入路由与生命周期状态。

use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use super::{
    dom::{NativeDom, NodeId},
    geom::{Point, Size},
    id::{WindowDesc, WindowId},
    layout::LayoutSnapshot,
    paint::DisplayList,
    signals::WindowSignals,
};
use crate::{
    dom_api::{Event, EventContext, EventKind, Role, Selector},
    headless::HeadlessBackend,
    metrics::RuntimeStats,
    reactive::Owner,
    runtime::Runtime,
    text::TextEngine,
};

/// Focus state owned by one window.
/// 单个窗口持有的焦点状态。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FocusState {
    /// Whether the window itself is focused.
    /// 窗口本身是否获得焦点。
    pub window_focused: bool,
    /// Currently focused node.
    /// 当前聚焦节点。
    pub node: Option<NodeId>,
}

/// IME state owned by one window.
/// 单个窗口持有的 IME 状态。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImeState {
    /// Whether IME input is enabled.
    /// 是否启用 IME 输入。
    pub enabled: bool,
    /// Current composition text.
    /// 当前组合文本。
    pub composition: Option<String>,
    /// Last committed text.
    /// 最近一次提交的文本。
    pub last_commit: Option<String>,
}

/// Accessibility commit state owned by one window.
/// 单个窗口持有的无障碍提交状态。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccessibilityState {
    /// Number of committed semantic changes.
    /// 已提交语义变更数。
    pub commits: u64,
    /// Last committed focused node.
    /// 最近提交的聚焦节点。
    pub focused: Option<NodeId>,
}

/// Pointer action converted by a platform host.
/// 由平台 Host 转换的指针动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerAction {
    /// Pointer moved.
    /// 指针移动。
    Moved,
    /// Primary button pressed.
    /// 主按钮按下。
    Pressed,
    /// Primary button released.
    /// 主按钮释放。
    Released,
    /// Pointer interaction cancelled by the platform.
    /// 平台取消指针交互。
    Cancelled,
}

/// Window-tagged pointer input.
/// 携带窗口身份的指针输入。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerInput {
    /// Target window.
    /// 目标窗口。
    pub window: WindowId,
    /// Logical position inside the window.
    /// 窗口内的逻辑位置。
    pub position: Point,
    /// Pointer action.
    /// 指针动作。
    pub action: PointerAction,
}

/// IME events routed to one window.
/// 路由到单个窗口的 IME 事件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImeEvent {
    /// Enables or disables IME.
    /// 启用或停用 IME。
    Enabled(bool),
    /// Updates composition text.
    /// 更新组合文本。
    Composition(String),
    /// Commits text.
    /// 提交文本。
    Commit(String),
}

/// Keyboard input routed to one window.
/// 路由到单个窗口的键盘输入。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyInput {
    /// Target window.
    /// 目标窗口。
    pub window: WindowId,
    /// Logical key name.
    /// 逻辑键名。
    pub key: String,
    /// Whether the key was pressed.
    /// 按键是否按下。
    pub pressed: bool,
    /// Whether this is an OS key-repeat event.
    /// 是否为操作系统按键重复事件。
    pub repeat: bool,
}

/// Platform-neutral input carrying its target window.
/// 携带目标窗口的平台无关输入。
#[derive(Clone, Debug, PartialEq)]
pub enum InputEvent {
    /// Pointer input.
    /// 指针输入。
    Pointer(PointerInput),
    /// Keyboard input.
    /// 键盘输入。
    Key(KeyInput),
    /// IME input.
    /// IME 输入。
    Ime {
        /// Target window.
        /// 目标窗口。
        window: WindowId,
        /// IME event.
        /// IME 事件。
        event: ImeEvent,
    },
    /// Window focus change.
    /// 窗口焦点变化。
    Focus {
        /// Target window.
        /// 目标窗口。
        window: WindowId,
        /// Whether the window is focused.
        /// 窗口是否获得焦点。
        focused: bool,
    },
    /// Logical resize.
    /// 逻辑尺寸变化。
    Resize {
        /// Target window.
        /// 目标窗口。
        window: WindowId,
        /// New logical viewport.
        /// 新逻辑视口。
        viewport: Size,
        /// New device pixel ratio.
        /// 新设备像素比。
        dpr: f32,
    },
    /// Surface loss; UI state and DOM must survive.
    /// Surface 丢失；UI 状态与 DOM 必须保留。
    SurfaceLost {
        /// Target window.
        /// 目标窗口。
        window: WindowId,
    },
}

impl InputEvent {
    /// Returns the target window carried by the event.
    /// 返回事件携带的目标窗口。
    pub fn window(&self) -> WindowId {
        match self {
            Self::Pointer(input) => input.window,
            Self::Key(input) => input.window,
            Self::Ime { window, .. }
            | Self::Focus { window, .. }
            | Self::Resize { window, .. }
            | Self::SurfaceLost { window } => *window,
        }
    }
}

/// Outcome of routing one input event.
/// 路由单个输入事件的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputOutcome {
    /// Target window.
    /// 目标窗口。
    pub window: WindowId,
    /// Whether a listener handled the input.
    /// 是否有监听器处理输入。
    pub handled: bool,
    /// Semantic target, when hit.
    /// 命中时的语义目标。
    pub target: Option<NodeId>,
}

/// GPU surface and configuration owned by one window.
/// 单个窗口持有的 GPU Surface 与配置。
pub struct SurfaceState {
    pub(crate) surface: wgpu::Surface<'static>,
    pub(crate) config: wgpu::SurfaceConfiguration,
}

impl std::fmt::Debug for SurfaceState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SurfaceState")
            .field("width", &self.config.width)
            .field("height", &self.config.height)
            .field("format", &self.config.format)
            .finish_non_exhaustive()
    }
}

/// One window's independent DOM, focus, IME, accessibility, layout, paint, and surface state.
/// 单个窗口独立的 DOM、焦点、IME、无障碍、布局、绘制与 Surface 状态。
pub struct WindowContext {
    id: WindowId,
    desc: WindowDesc,
    text_engine: Rc<RefCell<TextEngine>>,
    runtime: Runtime<NativeDom>,
    owner: Owner,
    root: Option<NodeId>,
    focus: FocusState,
    ime: ImeState,
    accessibility: AccessibilityState,
    viewport: Size,
    dpr: f32,
    surface: Option<SurfaceState>,
    surface_lost_count: u64,
    surface_created_count: u64,
    last_layout: Option<LayoutSnapshot>,
    last_display_list: Option<DisplayList>,
    last_input: Option<Point>,
    pressed_target: Option<NodeId>,
}

impl WindowContext {
    /// Creates a window context with its own DOM and owner.
    /// 创建拥有独立 DOM 与 Owner 的窗口上下文。
    pub fn new(id: WindowId, desc: WindowDesc, text_engine: Rc<RefCell<TextEngine>>) -> Self {
        let viewport = Size::new(desc.width as f32, desc.height as f32);
        let dom = NativeDom::new_window(id, Rc::clone(&text_engine), viewport, 1.0);
        let runtime = Runtime::new(dom);
        let owner = runtime.create_owner();
        Self {
            id,
            desc,
            text_engine,
            runtime,
            owner,
            root: None,
            focus: FocusState::default(),
            ime: ImeState::default(),
            accessibility: AccessibilityState::default(),
            viewport,
            dpr: 1.0,
            surface: None,
            surface_lost_count: 0,
            surface_created_count: 0,
            last_layout: None,
            last_display_list: None,
            last_input: None,
            pressed_target: None,
        }
    }

    /// Returns the window identity.
    /// 返回窗口身份。
    pub fn id(&self) -> WindowId {
        self.id
    }

    /// Returns the creation descriptor.
    /// 返回创建描述。
    pub fn desc(&self) -> &WindowDesc {
        &self.desc
    }

    /// Returns the per-window runtime.
    /// 返回每窗口 Runtime。
    pub fn runtime(&self) -> &Runtime<NativeDom> {
        &self.runtime
    }

    /// Returns the per-window runtime mutably.
    /// 可变返回每窗口 Runtime。
    pub fn runtime_mut(&mut self) -> &mut Runtime<NativeDom> {
        &mut self.runtime
    }

    /// Returns the window owner.
    /// 返回窗口 Owner。
    pub fn owner(&self) -> &Owner {
        &self.owner
    }

    /// Returns the shared text engine handle used by this window.
    /// 返回该窗口使用的共享文本引擎句柄。
    pub fn text_engine(&self) -> Rc<RefCell<TextEngine>> {
        Rc::clone(&self.text_engine)
    }

    /// Returns the shared invalidation signals.
    /// 返回共享失效信号。
    pub fn signals(&self) -> Rc<WindowSignals> {
        self.runtime.backend().signals()
    }

    /// Mounts exactly one component root and returns its stable handle.
    /// 只挂载一个组件根并返回其稳定句柄。
    pub fn mount(
        &mut self,
        mount: impl FnOnce(&mut Runtime<NativeDom>, &Owner) -> NodeId,
    ) -> NodeId {
        assert!(self.root.is_none(), "one window hosts one component root");
        let root = mount(&mut self.runtime, &self.owner);
        self.root = Some(root);
        self.focus.node = Some(root);
        root
    }

    /// Returns the mounted root.
    /// 返回已挂载根节点。
    pub fn root(&self) -> Option<NodeId> {
        self.root
    }

    /// Returns the logical viewport.
    /// 返回逻辑视口。
    pub fn viewport(&self) -> Size {
        self.viewport
    }

    /// Returns the device pixel ratio.
    /// 返回设备像素比。
    pub fn dpr(&self) -> f32 {
        self.dpr
    }

    /// Returns focus state.
    /// 返回焦点状态。
    pub fn focus(&self) -> &FocusState {
        &self.focus
    }

    /// Returns IME state.
    /// 返回 IME 状态。
    pub fn ime(&self) -> &ImeState {
        &self.ime
    }

    /// Returns accessibility state.
    /// 返回无障碍状态。
    pub fn accessibility(&self) -> &AccessibilityState {
        &self.accessibility
    }

    /// Returns the committed layout snapshot, if layout has run.
    /// 返回已提交布局快照（若已运行布局）。
    pub fn last_layout(&self) -> Option<&LayoutSnapshot> {
        self.last_layout.as_ref()
    }

    /// Returns the last painted display list.
    /// 返回最近绘制的显示列表。
    pub fn last_display_list(&self) -> Option<&DisplayList> {
        self.last_display_list.as_ref()
    }

    /// Returns whether a frame request is pending.
    /// 返回是否有待处理帧请求。
    pub fn frame_request_pending(&self) -> bool {
        self.runtime.backend().frame_request_pending()
    }

    /// Takes one coalesced frame request.
    /// 取走一个合并帧请求。
    pub fn take_frame_request(&self) -> bool {
        self.runtime.backend().take_frame_request()
    }

    /// Returns the number of coalesced frame requests.
    /// 返回合并帧请求数。
    pub fn frame_requests(&self) -> u64 {
        self.signals().frame_requests()
    }

    /// Returns unified runtime counters for this window only.
    /// 仅返回该窗口的统一运行时计数。
    pub fn runtime_stats(&self) -> RuntimeStats {
        let reactive = self
            .owner
            .snapshot_with_component_executions(self.runtime.component_executions().get());
        let effect_executions = reactive
            .effects
            .iter()
            .map(|effect| effect.executions)
            .sum();
        RuntimeStats::from_parts(
            reactive.component_executions,
            effect_executions,
            self.runtime.backend().backend_stats(),
        )
    }

    /// Runs layout in this window's own coordinate space.
    /// 在该窗口自身的坐标空间中运行布局。
    pub fn layout(&mut self) -> LayoutSnapshot {
        let snapshot = self
            .runtime
            .backend_mut()
            .layout(super::layout::WindowConstraints::new(
                self.id,
                self.viewport,
            ));
        self.last_layout = Some(snapshot.clone());
        snapshot
    }

    /// Paints this window's display list.
    /// 绘制该窗口的显示列表。
    pub fn paint(&mut self) -> DisplayList {
        let list = self.runtime.backend_mut().paint(self.id, self.viewport);
        self.last_display_list = Some(list.clone());
        list
    }

    /// Reads committed bounds tagged with this window.
    /// 读取带当前窗口身份的已提交边界。
    pub fn committed_bounds(&self, window: WindowId, node: NodeId) -> Option<super::geom::Rect> {
        self.runtime.backend().committed_bounds(window, node)
    }

    /// Explicitly requests current bounds, running layout first.
    /// 显式请求最新边界，先运行布局。
    pub fn request_current_bounds(
        &mut self,
        window: WindowId,
        node: NodeId,
    ) -> Option<super::geom::Rect> {
        let viewport = self.viewport;
        self.runtime
            .backend_mut()
            .request_current_bounds(window, node, viewport)
    }

    /// Returns concatenated textual content for one node.
    /// 返回单个节点的拼接文本内容。
    pub fn text_content(&self, node: NodeId) -> Option<String> {
        self.runtime.backend().text_content(node)
    }

    /// Resolves semantic selectors inside this window only.
    /// 仅在该窗口内解析语义选择器。
    pub fn find_all(&self, selector: &Selector) -> Vec<NodeId> {
        self.runtime.backend().find_all(selector)
    }

    /// Updates logical viewport and device pixel ratio, preserving DOM and state.
    /// 更新逻辑视口与设备像素比，保留 DOM 与状态。
    pub fn resize(&mut self, viewport: Size, dpr: f32) {
        if viewport == self.viewport && dpr == self.dpr {
            return;
        }
        self.viewport = viewport;
        self.dpr = dpr;
        self.runtime.backend_mut().set_viewport(viewport, dpr);
    }

    /// Installs a freshly created surface without remounting the component.
    /// 安装新建 Surface，不重新挂载组件。
    pub fn install_surface(&mut self, surface: SurfaceState) {
        self.surface = Some(surface);
        self.surface_created_count += 1;
        self.signals().mark_paint_dirty();
    }

    /// Returns the current surface state.
    /// 返回当前 Surface 状态。
    pub fn surface(&self) -> Option<&SurfaceState> {
        self.surface.as_ref()
    }

    /// Returns the current surface state mutably.
    /// 可变返回当前 Surface 状态。
    pub fn surface_mut(&mut self) -> Option<&mut SurfaceState> {
        self.surface.as_mut()
    }

    /// Drops GPU resources while preserving DOM, text, layout, and signals.
    /// 释放 GPU 资源，同时保留 DOM、文本、布局与信号。
    pub fn surface_lost(&mut self) {
        self.surface = None;
        self.surface_lost_count += 1;
    }

    /// Returns how many times the surface was lost.
    /// 返回 Surface 丢失次数。
    pub fn surface_lost_count(&self) -> u64 {
        self.surface_lost_count
    }

    /// Returns how many surfaces were installed.
    /// 返回已安装 Surface 次数。
    pub fn surface_created_count(&self) -> u64 {
        self.surface_created_count
    }

    /// Routes one already window-tagged input event to this context.
    /// 将一条携带窗口身份的输入事件路由到该上下文。
    pub fn dispatch_input(&mut self, event: InputEvent) -> InputOutcome {
        assert_eq!(
            event.window(),
            self.id,
            "input must be routed to its window"
        );
        match event {
            InputEvent::Pointer(input) => self.dispatch_pointer(input),
            InputEvent::Key(input) => self.dispatch_key(input),
            InputEvent::Ime { event, .. } => {
                match event {
                    ImeEvent::Enabled(enabled) => self.ime.enabled = enabled,
                    ImeEvent::Composition(text) => self.ime.composition = Some(text),
                    ImeEvent::Commit(text) => {
                        self.ime.composition = None;
                        self.ime.last_commit = Some(text);
                    }
                }
                InputOutcome {
                    window: self.id,
                    handled: true,
                    target: self.focus.node,
                }
            }
            InputEvent::Focus { focused, .. } => {
                self.focus.window_focused = focused;
                if focused {
                    if self.focus.node.is_none() {
                        self.focus.node = self.root;
                    }
                } else {
                    self.pressed_target = None;
                }
                self.accessibility.commits += 1;
                self.sync_accessibility_focus();
                InputOutcome {
                    window: self.id,
                    handled: true,
                    target: self.focus.node,
                }
            }
            InputEvent::Resize { viewport, dpr, .. } => {
                self.resize(viewport, dpr);
                InputOutcome {
                    window: self.id,
                    handled: true,
                    target: None,
                }
            }
            InputEvent::SurfaceLost { .. } => {
                self.surface_lost();
                InputOutcome {
                    window: self.id,
                    handled: true,
                    target: None,
                }
            }
        }
    }

    /// Reports whether a frame is needed for this window.
    /// 报告该窗口是否需要新帧。
    pub fn needs_frame(&self) -> bool {
        self.frame_request_pending()
    }

    /// Records a presented frame without clearing windows that were not painted.
    /// 记录已呈现帧，且不影响未绘制窗口。
    pub fn finish_frame(&mut self) {
        self.signals().commit_frame();
    }

    fn dispatch_pointer(&mut self, input: PointerInput) -> InputOutcome {
        self.last_input = Some(input.position);
        match input.action {
            PointerAction::Moved => InputOutcome {
                window: self.id,
                handled: false,
                target: None,
            },
            PointerAction::Cancelled => {
                self.pressed_target = None;
                InputOutcome {
                    window: self.id,
                    handled: true,
                    target: None,
                }
            }
            PointerAction::Pressed => {
                // A press only records the candidate; activation waits for release on the same node.
                // 按下仅记录候选目标；只有在同一节点释放才激活。
                let _ = self.layout();
                let target = self.runtime.backend().hit_test(self.id, input.position);
                self.pressed_target = target;
                if let Some(node) = target {
                    self.focus.node = Some(node);
                    self.accessibility.commits += 1;
                    self.sync_accessibility_focus();
                }
                InputOutcome {
                    window: self.id,
                    handled: target.is_some(),
                    target,
                }
            }
            PointerAction::Released => {
                let pressed = self.pressed_target.take();
                let _ = self.layout();
                let released = self.runtime.backend().hit_test(self.id, input.position);
                let target = match (pressed, released) {
                    (Some(pressed), Some(released)) if pressed == released => Some(pressed),
                    _ => None,
                };
                let listener =
                    target.and_then(|node| self.runtime.backend().listener(node, EventKind::Click));
                let handled = listener.is_some();
                if let Some(listener) = listener {
                    listener.call(
                        Event {
                            kind: EventKind::Click,
                        },
                        &mut EventContext,
                    );
                }
                InputOutcome {
                    window: self.id,
                    handled,
                    target,
                }
            }
        }
    }

    fn dispatch_key(&mut self, input: KeyInput) -> InputOutcome {
        if !input.pressed || input.repeat || !self.focus.window_focused {
            return InputOutcome {
                window: self.id,
                handled: false,
                target: self.focus.node,
            };
        }
        if input.key == "Enter" || input.key == " " {
            let target = self.focus.node;
            let listener =
                target.and_then(|node| self.runtime.backend().listener(node, EventKind::Click));
            if let Some(listener) = listener {
                listener.call(
                    Event {
                        kind: EventKind::Click,
                    },
                    &mut EventContext,
                );
                return InputOutcome {
                    window: self.id,
                    handled: true,
                    target,
                };
            }
        }
        InputOutcome {
            window: self.id,
            handled: false,
            target: self.focus.node,
        }
    }

    fn sync_accessibility_focus(&mut self) {
        self.accessibility.focused = if self.focus.window_focused {
            self.focus.node
        } else {
            None
        };
    }
}

impl Drop for WindowContext {
    fn drop(&mut self) {
        // Closing a window releases only its own owner, effects, and listeners.
        // 关闭窗口只释放该窗口自己的 Owner、Effect 与监听。
        self.owner.dispose();
    }
}

/// Application-level window registry; no global "current window" exists.
/// 应用级窗口注册表；不存在全局“当前窗口”。
pub struct WindowManager {
    next_window: u64,
    text_engine: Rc<RefCell<TextEngine>>,
    windows: BTreeMap<WindowId, WindowContext>,
}

impl WindowManager {
    /// Creates an empty window manager sharing one text engine.
    /// 创建共享一个文本引擎的空窗口管理器。
    pub fn new(text_engine: Rc<RefCell<TextEngine>>) -> Self {
        Self {
            next_window: 0,
            text_engine,
            windows: BTreeMap::new(),
        }
    }

    /// Allocates one window context.
    /// 分配一个窗口上下文。
    pub fn create_window(&mut self, desc: WindowDesc) -> WindowId {
        self.next_window += 1;
        let id = WindowId::new(self.next_window);
        let context = WindowContext::new(id, desc, Rc::clone(&self.text_engine));
        self.windows.insert(id, context);
        id
    }

    /// Looks up one window context.
    /// 查找一个窗口上下文。
    pub fn window(&self, id: WindowId) -> Option<&WindowContext> {
        self.windows.get(&id)
    }

    /// Looks up one window context mutably.
    /// 可变查找一个窗口上下文。
    pub fn window_mut(&mut self, id: WindowId) -> Option<&mut WindowContext> {
        self.windows.get_mut(&id)
    }

    /// Removes and returns one closed window context.
    /// 移除并返回一个已关闭窗口上下文。
    pub fn remove_window(&mut self, id: WindowId) -> Option<WindowContext> {
        self.windows.remove(&id)
    }

    /// Returns all live window identities in allocation order.
    /// 按分配顺序返回全部存活窗口身份。
    pub fn window_ids(&self) -> Vec<WindowId> {
        self.windows.keys().copied().collect()
    }

    /// Routes an event strictly by its carried window identity.
    /// 严格按事件携带的窗口身份路由。
    pub fn route_input(&mut self, event: InputEvent) -> Option<InputOutcome> {
        let window = event.window();
        self.windows
            .get_mut(&window)
            .map(|context| context.dispatch_input(event))
    }

    /// Returns the number of live windows.
    /// 返回存活窗口数量。
    pub fn len(&self) -> usize {
        self.windows.len()
    }

    /// Returns whether no windows are live.
    /// 返回是否没有存活窗口。
    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }
}

/// Finds all buttons matching a semantic selector in one window.
/// 在一个窗口中查找符合语义选择器的所有按钮。
pub fn find_buttons(context: &WindowContext, name: &str) -> Vec<NodeId> {
    context.find_all(&Selector {
        role: Role::Button,
        name: name.to_owned(),
    })
}

/// Builds a counter-shaped button for focused native tests.
/// 为聚焦的原生测试构建计数器形状按钮。
#[cfg(test)]
pub(crate) fn build_probe_button(
    runtime: &mut Runtime<NativeDom>,
    owner: &Owner,
    count: crate::reactive::Signal<i32>,
) -> NodeId {
    use crate::dom_api::{Attribute, DomBackend, Tag};

    runtime.component_executions().increment();
    let button = runtime.backend_mut().create_element(Tag::Button);
    let prefix = runtime.backend_mut().create_text("Count: ");
    let value = runtime.backend_mut().create_text("");
    runtime
        .backend_mut()
        .set_attribute(&button, Attribute::AriaLabel, Some("Increment counter"));
    runtime.backend_mut().insert_before(&button, &prefix, None);
    runtime.backend_mut().insert_before(&button, &value, None);
    let click_count = count.clone();
    runtime.listen(owner, &button, EventKind::Click, move |_event, _context| {
        click_count.update(|number| *number += 1);
    });
    let text_count = count;
    runtime.effect(owner, "probe/text", move |context| {
        let next = text_count.get().to_string();
        context.backend_mut().set_text(&value, &next);
    });
    button
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::Signal;

    fn mount_window(
        engine: Rc<RefCell<TextEngine>>,
        window: WindowId,
        count: Signal<i32>,
    ) -> (WindowContext, NodeId) {
        let mut context = WindowContext::new(
            window,
            WindowDesc {
                title: format!("window-{}", window.value()),
                ..WindowDesc::default()
            },
            engine,
        );
        let root = context.mount(move |runtime, owner| build_probe_button(runtime, owner, count));
        (context, root)
    }

    fn pointer(
        context: &mut WindowContext,
        position: Point,
        action: PointerAction,
    ) -> InputOutcome {
        context.dispatch_input(InputEvent::Pointer(PointerInput {
            window: context.id(),
            position,
            action,
        }))
    }

    fn click(context: &mut WindowContext, position: Point) -> InputOutcome {
        pointer(context, position, PointerAction::Pressed);
        pointer(context, position, PointerAction::Released)
    }

    /// Proves hit testing and click dispatch update one window's text only.
    /// 验证命中测试与点击分发只更新一个窗口的文本。
    #[test]
    fn a_click_carries_its_window_and_updates_only_the_target() {
        let engine = Rc::new(RefCell::new(TextEngine::new()));
        let first_app = Owner::new();
        let second_app = Owner::new();
        let first_count = first_app.signal_named("app/count", 0);
        let second_count = second_app.signal_named("app/count", 0);
        let (mut first, first_root) =
            mount_window(Rc::clone(&engine), WindowId::new(1), first_count);
        let (mut second, second_root) =
            mount_window(Rc::clone(&engine), WindowId::new(2), second_count);
        first.layout();
        second.layout();
        first.take_frame_request();
        second.take_frame_request();

        let bounds = first
            .committed_bounds(first.id(), first_root)
            .expect("first button bounds");
        let pressed = pointer(&mut first, bounds.center(), PointerAction::Pressed);
        assert_eq!(pressed.target, Some(first_root));
        assert_eq!(
            first.text_content(first_root).as_deref(),
            Some("Count: 0"),
            "a press alone must not activate the button"
        );
        let released = pointer(&mut first, bounds.center(), PointerAction::Released);
        assert!(released.handled);
        assert_eq!(released.target, Some(first_root));
        assert_eq!(first.text_content(first_root).as_deref(), Some("Count: 1"));
        assert_eq!(
            second.text_content(second_root).as_deref(),
            Some("Count: 0")
        );
        assert!(first.take_frame_request(), "one click requests one frame");
        assert!(!first.take_frame_request(), "the request is coalesced");
        assert!(
            !second.frame_request_pending(),
            "unrelated window stays idle"
        );
        let stats = first.runtime_stats();
        assert_eq!(stats.component_executions, 1);
        assert_eq!(stats.backend_set_text, 2, "initial 0 and clicked 1");
        assert_eq!(stats.nodes_created, 3);
        assert_eq!(stats.nodes_removed, 0);
        assert_eq!(
            stats.frames_requested, 2,
            "initial mount plus one click frame for the affected window"
        );
    }

    /// Verifies hit testing matches the button bounds exactly.
    /// 验证命中测试与按钮边界完全一致。
    #[test]
    fn hit_testing_matches_the_button_bounds() {
        let engine = Rc::new(RefCell::new(TextEngine::new()));
        let app = Owner::new();
        let count = app.signal_named("app/count", 0);
        let (mut context, root) = mount_window(engine, WindowId::new(1), count);
        context.layout();
        let bounds = context
            .committed_bounds(context.id(), root)
            .expect("bounds");
        assert_eq!(
            context
                .runtime()
                .backend()
                .hit_test(context.id(), bounds.center()),
            Some(root)
        );
        let outside = Point::new(bounds.right() + 1.0, bounds.center().y);
        assert_eq!(
            context.runtime().backend().hit_test(context.id(), outside),
            None
        );
        assert_eq!(
            context
                .runtime()
                .backend()
                .hit_test(context.id(), Point::new(bounds.x, bounds.y)),
            Some(root)
        );
    }

    /// Resizing and surface loss preserve the shared count and DOM identity.
    /// 缩放与 Surface 丢失保留共享计数与 DOM 身份。
    #[test]
    fn resize_and_surface_loss_preserve_count_and_dom() {
        let engine = Rc::new(RefCell::new(TextEngine::new()));
        let app = Owner::new();
        let count = app.signal_named("app/count", 0);
        let (mut context, root) = mount_window(engine, WindowId::new(1), count.clone());
        context.layout();
        let bounds = context
            .committed_bounds(context.id(), root)
            .expect("bounds");
        click(&mut context, bounds.center());
        assert_eq!(context.text_content(root).as_deref(), Some("Count: 1"));
        count.set(7);
        assert_eq!(context.text_content(root).as_deref(), Some("Count: 7"));
        let stable = context.runtime().backend().stable_id(root);
        let before = context.text_content(root);

        context.resize(Size::new(900.0, 600.0), 2.0);
        context.dispatch_input(InputEvent::SurfaceLost {
            window: context.id(),
        });
        assert_eq!(context.text_content(root), before);
        assert_eq!(context.runtime().backend().stable_id(root), stable);
        assert_eq!(context.runtime().backend().window(), context.id());
        assert_eq!(count.get_untracked(), 7);
        assert_eq!(context.surface_lost_count(), 1);
        context.layout();
        assert!(context.last_layout().is_some());
    }

    /// Activation requires press and release on the same button; drag-out and cancel do nothing.
    /// 激活要求在同一按钮上按下并释放；拖出与取消不触发。
    #[test]
    fn pointer_activation_requires_press_and_release_on_same_button() {
        let engine = Rc::new(RefCell::new(TextEngine::new()));
        let app = Owner::new();
        let count = app.signal_named("app/count", 0);
        let (mut context, root) = mount_window(engine, WindowId::new(1), count.clone());
        context.layout();
        context.dispatch_input(InputEvent::Focus {
            window: context.id(),
            focused: true,
        });
        let bounds = context
            .committed_bounds(context.id(), root)
            .expect("button bounds");
        let inside = bounds.center();
        let outside = Point::new(bounds.right() + 50.0, bounds.center().y);

        pointer(&mut context, inside, PointerAction::Pressed);
        pointer(&mut context, outside, PointerAction::Released);
        assert_eq!(count.get_untracked(), 0, "drag out must not click");

        pointer(&mut context, outside, PointerAction::Pressed);
        pointer(&mut context, inside, PointerAction::Released);
        assert_eq!(count.get_untracked(), 0, "press outside must not click");

        pointer(&mut context, inside, PointerAction::Pressed);
        pointer(&mut context, inside, PointerAction::Cancelled);
        assert_eq!(count.get_untracked(), 0, "cancel must not click");

        pointer(&mut context, inside, PointerAction::Pressed);
        pointer(&mut context, inside, PointerAction::Released);
        assert_eq!(count.get_untracked(), 1, "press and release must click");

        pointer(&mut context, inside, PointerAction::Pressed);
        pointer(&mut context, outside, PointerAction::Moved);
        pointer(&mut context, inside, PointerAction::Moved);
        pointer(&mut context, inside, PointerAction::Released);
        assert_eq!(
            count.get_untracked(),
            2,
            "returning to the pressed button still activates"
        );
        assert_eq!(context.text_content(root).as_deref(), Some("Count: 2"));
    }

    /// Keyboard activation ignores OS repeat and remains available after focus blur/regain.
    /// 键盘激活忽略系统重复，并在失焦/重新聚焦后仍可用。
    #[test]
    fn keyboard_activation_ignores_repeat_and_survives_focus_blur() {
        let engine = Rc::new(RefCell::new(TextEngine::new()));
        let app = Owner::new();
        let count = app.signal_named("app/count", 0);
        let (mut context, root) = mount_window(engine, WindowId::new(1), count.clone());
        context.layout();
        context.dispatch_input(InputEvent::Focus {
            window: context.id(),
            focused: true,
        });
        assert_eq!(context.accessibility().focused, Some(root));

        let window = context.id();
        let key = |key: &str, repeat: bool| KeyInput {
            window,
            key: key.to_owned(),
            pressed: true,
            repeat,
        };
        assert!(
            context
                .dispatch_input(InputEvent::Key(key("Enter", false)))
                .handled
        );
        assert_eq!(count.get_untracked(), 1);
        assert!(
            !context
                .dispatch_input(InputEvent::Key(key("Enter", true)))
                .handled
        );
        assert_eq!(count.get_untracked(), 1, "OS repeat must not click again");

        context.dispatch_input(InputEvent::Focus {
            window: context.id(),
            focused: false,
        });
        assert_eq!(
            context.focus().node,
            Some(root),
            "blur keeps the focus node"
        );
        assert_eq!(context.accessibility().focused, None);
        assert!(
            !context
                .dispatch_input(InputEvent::Key(key("Enter", false)))
                .handled
        );
        assert_eq!(count.get_untracked(), 1);

        context.dispatch_input(InputEvent::Focus {
            window: context.id(),
            focused: true,
        });
        assert_eq!(context.accessibility().focused, Some(root));
        assert!(
            context
                .dispatch_input(InputEvent::Key(key(" ", false)))
                .handled
        );
        assert_eq!(count.get_untracked(), 2);
        assert_eq!(context.text_content(root).as_deref(), Some("Count: 2"));
    }

    /// Routes events by carried identity and ignores unknown windows.
    /// 按携带身份路由事件，并忽略未知窗口。
    #[test]
    fn window_manager_routes_by_window_identity() {
        let engine = Rc::new(RefCell::new(TextEngine::new()));
        let mut manager = WindowManager::new(engine);
        let app = Owner::new();
        let count = app.signal_named("app/count", 0);
        let first = manager.create_window(WindowDesc::default());
        let second = manager.create_window(WindowDesc::default());
        let first_root = {
            let count = count.clone();
            manager
                .window_mut(first)
                .unwrap()
                .mount(move |runtime, owner| build_probe_button(runtime, owner, count))
        };
        let second_root = {
            let count = count.clone();
            manager
                .window_mut(second)
                .unwrap()
                .mount(move |runtime, owner| build_probe_button(runtime, owner, count))
        };
        manager.window_mut(first).unwrap().layout();
        manager.window_mut(second).unwrap().layout();
        let bounds = manager
            .window(first)
            .unwrap()
            .committed_bounds(first, first_root)
            .expect("bounds");
        manager
            .route_input(InputEvent::Pointer(PointerInput {
                window: first,
                position: bounds.center(),
                action: PointerAction::Pressed,
            }))
            .expect("press routed");
        let outcome = manager
            .route_input(InputEvent::Pointer(PointerInput {
                window: first,
                position: bounds.center(),
                action: PointerAction::Released,
            }))
            .expect("release routed");
        assert!(outcome.handled);
        assert_eq!(
            manager
                .window(first)
                .unwrap()
                .text_content(first_root)
                .as_deref(),
            Some("Count: 1")
        );
        assert_eq!(
            manager
                .window(second)
                .unwrap()
                .text_content(second_root)
                .as_deref(),
            Some("Count: 1")
        );
        assert!(
            manager
                .route_input(InputEvent::Focus {
                    window: WindowId::new(99),
                    focused: true,
                })
                .is_none()
        );
    }
}
