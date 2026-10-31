//! Native Host and Backend initialization for the shared counter.
//! 共享计数器的原生 Host 与 Backend 初始化。

use fino::native::{Application, NodeId, Size, WindowContext, WindowDesc, WindowId};
use winit::dpi::PhysicalSize;

use crate::runner::mount_counter;

/// Creates the application around the configured native surface.
/// 围绕已配置的原生 Surface 创建 Application。
pub fn new_application(
    instance: wgpu::Instance,
    surface: &wgpu::Surface,
) -> Result<Application, String> {
    Application::with_surface(instance, surface).map_err(|error| error.to_string())
}

/// Creates one native window through the application window manager.
/// 通过 Application 的 WindowManager 创建一个原生窗口。
pub fn create_window(app: &mut Application, window_desc: WindowDesc) -> WindowId {
    app.create_window(window_desc)
}

/// Installs the platform surface for an initialized window.
/// 为已初始化的窗口安装平台 Surface。
pub fn install_surface(
    app: &mut Application,
    window_id: WindowId,
    surface: wgpu::Surface<'static>,
) -> Result<(), String> {
    app.install_surface(window_id, surface)
        .map_err(|error| error.to_string())
}

/// Mounts the shared counter component into one initialized window.
/// 把共享计数器组件挂载到一个已初始化的窗口。
pub fn mount_counter_window(app: &mut Application, window_id: WindowId) -> Option<NodeId> {
    app.mount_window(window_id, mount_counter)
}

/// Applies the real window geometry and DPR before the first surface configuration.
/// 在首次配置 Surface 前应用真实窗口几何与 DPR。
pub fn apply_initial_viewport(
    context: &mut WindowContext,
    physical: PhysicalSize<u32>,
    scale_factor: f64,
) {
    let logical = physical.to_logical::<f32>(scale_factor);
    context.resize(
        Size::new(logical.width.max(1.0), logical.height.max(1.0)),
        scale_factor as f32,
    );
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use fino::{
        native::{Size, WindowContext, WindowDesc, WindowId},
        text::TextEngine,
    };

    use super::{PhysicalSize, apply_initial_viewport};

    /// The first surface configuration must use the real physical size and scale factor.
    /// 首次 Surface 配置必须使用真实物理尺寸与缩放因子。
    #[test]
    fn initial_viewport_uses_physical_size_and_scale_factor() {
        let mut context = WindowContext::new(
            WindowId::new(1),
            WindowDesc::default(),
            Rc::new(RefCell::new(TextEngine::new())),
        );
        apply_initial_viewport(&mut context, PhysicalSize::new(1000, 600), 2.0);
        assert_eq!(context.viewport(), Size::new(500.0, 300.0));
        assert_eq!(context.dpr(), 2.0);
    }
}
