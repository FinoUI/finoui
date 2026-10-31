//! Native backend: generational DOM, text, layout, paint, windows, and wgpu host.
//! 原生后端：generational DOM、文本、布局、绘制、窗口与 wgpu Host。

use std::{cell::RefCell, rc::Rc};

use crate::text::TextEngine;

pub mod application;
pub mod dom;
pub mod geom;
pub mod id;
pub mod layout;
pub mod paint;
pub mod render;
pub mod signals;
pub mod window;

pub use application::Application;
pub use dom::{NativeDom, NativeListener, NodeId};
pub use geom::{Point, Rect, Size};
pub use id::{WindowDesc, WindowId};
pub use layout::{LayoutBoxSnapshot, LayoutSnapshot, WindowConstraints};
pub use paint::{Color, Damage, DisplayList, PaintCommand, PaintContext};
pub use render::{RenderError, RenderStats, RenderStatus, SharedRenderContext};
pub use signals::{WindowSignals, WindowSignalsSnapshot};
pub use window::{
    AccessibilityState, FocusState, ImeEvent, ImeState, InputEvent, InputOutcome, KeyInput,
    PointerAction, PointerInput, SurfaceState, WindowContext, WindowManager,
};

/// Creates a deterministic offscreen native backend with no GPU or window.
/// 创建不依赖 GPU 或窗口的确定性离屏原生后端。
pub fn offscreen_backend(viewport: Size, dpr: f32) -> NativeDom {
    NativeDom::new_offscreen(
        WindowId::new(1),
        Rc::new(RefCell::new(TextEngine::new())),
        viewport,
        dpr,
    )
}
