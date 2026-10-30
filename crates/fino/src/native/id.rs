//! Stable window identity used by every native input, geometry, and paint path.
//! 原生输入、几何与绘制路径统一使用的稳定窗口身份。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Stable generational-style window handle allocated by [`super::window::WindowManager`].
/// 由 [`super::window::WindowManager`] 分配的稳定窗口句柄。
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(transparent)]
pub struct WindowId(pub u64);

impl WindowId {
    /// Creates a window identity.
    /// 创建窗口身份。
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the raw allocation-order value used in artifacts.
    /// 返回产物中使用的分配顺序值。
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for WindowId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "window-{}", self.0)
    }
}

/// Platform-neutral window creation request.
/// 平台无关的窗口创建请求。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WindowDesc {
    /// Window title.
    /// 窗口标题。
    pub title: String,
    /// Initial logical width.
    /// 初始逻辑宽度。
    pub width: u32,
    /// Initial logical height.
    /// 初始逻辑高度。
    pub height: u32,
}

impl Default for WindowDesc {
    fn default() -> Self {
        Self {
            title: "Fino".to_owned(),
            width: 640,
            height: 360,
        }
    }
}
