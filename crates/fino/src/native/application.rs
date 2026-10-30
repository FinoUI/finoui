//! Application owner, shared render context, and the per-window registry.
//! 应用 Owner、共享渲染上下文与每窗口注册表。

use std::sync::Arc;

use winit::window::Window;

use super::{
    dom::NodeId,
    id::{WindowDesc, WindowId},
    render::{RenderError, RenderStatus, SharedRenderContext},
    window::{InputEvent, InputOutcome, WindowContext, WindowManager},
};
use crate::reactive::Owner;

/// Application-level state above [`WindowManager`].
/// 位于 [`WindowManager`] 之上的应用级状态。
pub struct Application {
    owner: Owner,
    windows: WindowManager,
    shared: SharedRenderContext,
}

impl Application {
    /// Creates the app owner, shared GPU/text resources, and an empty window manager.
    /// 创建应用 Owner、共享 GPU/文本资源与空窗口管理器。
    pub fn new() -> Result<Self, RenderError> {
        Self::with_instance(wgpu::Instance::default())
    }

    /// Creates the application around a host-owned wgpu instance.
    /// 围绕 Host 持有的 wgpu Instance 创建应用。
    pub fn with_instance(instance: wgpu::Instance) -> Result<Self, RenderError> {
        let shared = SharedRenderContext::with_instance(instance)?;
        let windows = WindowManager::new(shared.text_engine());
        Ok(Self {
            owner: Owner::new(),
            windows,
            shared,
        })
    }

    /// Returns the application owner that owns cross-window signals.
    /// 返回持有跨窗口 Signal 的应用 Owner。
    pub fn owner(&self) -> &Owner {
        &self.owner
    }

    /// Returns the shared rendering context.
    /// 返回共享渲染上下文。
    pub fn shared(&self) -> &SharedRenderContext {
        &self.shared
    }

    /// Returns the shared rendering context mutably.
    /// 可变返回共享渲染上下文。
    pub fn shared_mut(&mut self) -> &mut SharedRenderContext {
        &mut self.shared
    }

    /// Creates one window context with its own DOM and owner.
    /// 创建一个拥有独立 DOM 与 Owner 的窗口上下文。
    pub fn create_window(&mut self, desc: WindowDesc) -> WindowId {
        self.windows.create_window(desc)
    }

    /// Attaches a winit surface to an existing window context.
    /// 为现有窗口上下文挂载 winit Surface。
    pub fn attach_surface(&mut self, id: WindowId, window: Arc<Window>) -> Result<(), RenderError> {
        let context = self
            .windows
            .window_mut(id)
            .ok_or(RenderError::MissingWindow(id.value()))?;
        self.shared.attach_surface(context, window)
    }

    /// Mounts the caller-owned component into one window.
    /// 将调用方组件挂载到一个窗口。
    pub fn mount_window(
        &mut self,
        id: WindowId,
        mount: impl FnOnce(&mut crate::runtime::Runtime<super::dom::NativeDom>, &Owner) -> NodeId,
    ) -> Option<NodeId> {
        self.windows
            .window_mut(id)
            .map(|context| context.mount(mount))
    }

    /// Looks up one window context.
    /// 查找一个窗口上下文。
    pub fn window(&self, id: WindowId) -> Option<&WindowContext> {
        self.windows.window(id)
    }

    /// Looks up one window context mutably.
    /// 可变查找一个窗口上下文。
    pub fn window_mut(&mut self, id: WindowId) -> Option<&mut WindowContext> {
        self.windows.window_mut(id)
    }

    /// Routes window-tagged input; unknown windows never fall back to a global current window.
    /// 路由携带窗口身份的输入；未知窗口绝不回退到全局当前窗口。
    pub fn route_input(&mut self, event: InputEvent) -> Option<InputOutcome> {
        self.windows.route_input(event)
    }

    /// Renders one window with the shared device and its own surface.
    /// 使用共享 Device 与窗口自身 Surface 渲染一个窗口。
    pub fn render(&mut self, id: WindowId) -> Result<RenderStatus, RenderError> {
        let context = self
            .windows
            .window_mut(id)
            .ok_or(RenderError::MissingWindow(id.value()))?;
        self.shared.render(context)
    }

    /// Reconfigures one window surface after a resize.
    /// 缩放后重新配置一个窗口 Surface。
    pub fn reconfigure(&mut self, id: WindowId) -> Result<(), RenderError> {
        let context = self
            .windows
            .window_mut(id)
            .ok_or(RenderError::MissingWindow(id.value()))?;
        self.shared.reconfigure_surface(context)
    }

    /// Closes one window and drops only its owner, DOM, listeners, and surface.
    /// 关闭一个窗口，只释放其 Owner、DOM、监听与 Surface。
    pub fn close_window(&mut self, id: WindowId) -> Option<WindowContext> {
        self.windows.remove_window(id)
    }

    /// Returns live window identities.
    /// 返回存活窗口身份。
    pub fn window_ids(&self) -> Vec<WindowId> {
        self.windows.window_ids()
    }
}
