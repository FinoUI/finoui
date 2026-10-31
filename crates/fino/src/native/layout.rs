//! Minimal native layout contract for the counter button.
//! 计数器按钮的最小原生布局契约。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    geom::{Rect, Size},
    id::WindowId,
};
use crate::text::TextEngineStats;

/// Layout request; the window identity is part of the request, never implicit.
/// 布局请求；窗口身份属于请求本身，绝不隐含。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowConstraints {
    /// Target window.
    /// 目标窗口。
    pub window: WindowId,
    /// Logical viewport size.
    /// 逻辑视口尺寸。
    pub viewport: Size,
}

impl WindowConstraints {
    /// Creates constraints for one window.
    /// 为一个窗口创建约束。
    pub const fn new(window: WindowId, viewport: Size) -> Self {
        Self { window, viewport }
    }
}

/// Fixed box metrics for the first native button.
/// 首个原生按钮的固定盒模型尺寸。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ButtonStyle {
    /// Outer margin.
    /// 外边距。
    pub margin: f32,
    /// Horizontal padding.
    /// 水平内边距。
    pub padding_x: f32,
    /// Vertical padding.
    /// 垂直内边距。
    pub padding_y: f32,
    /// Minimum control width.
    /// 控件最小宽度。
    pub min_width: f32,
    /// Minimum control height.
    /// 控件最小高度。
    pub min_height: f32,
}

impl Default for ButtonStyle {
    fn default() -> Self {
        Self {
            margin: 24.0,
            padding_x: 20.0,
            padding_y: 12.0,
            min_width: 180.0,
            min_height: 44.0,
        }
    }
}

/// One committed layout box.
/// 一个已提交布局盒。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LayoutBoxSnapshot {
    /// Stable semantic node identifier.
    /// 稳定语义节点标识。
    pub id: String,
    /// Element tag, when applicable.
    /// 适用时的元素标签。
    pub tag: Option<String>,
    /// Text content, when applicable.
    /// 适用时的文本内容。
    pub text: Option<String>,
    /// Committed logical bounds.
    /// 已提交逻辑边界。
    pub rect: Rect,
}

/// Versioned layout snapshot with real text work counters.
/// 带真实文本工作量计数的版本化布局快照。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LayoutSnapshot {
    /// Snapshot schema version.
    /// 快照 schema 版本。
    pub schema: u32,
    /// Owning window.
    /// 所属窗口。
    pub window: WindowId,
    /// Logical viewport.
    /// 逻辑视口。
    pub viewport: Size,
    /// Boxes in paint order.
    /// 按绘制顺序排列的布局盒。
    pub boxes: Vec<LayoutBoxSnapshot>,
    /// Text shaping and layout counters at capture time.
    /// 捕获时的文本 shaping 与布局计数。
    pub text: TextEngineStats,
}
