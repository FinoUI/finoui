//! Platform-neutral geometry values shared by native layout, paint, and input.
//! 原生布局、绘制与输入共享的平台无关几何值。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Logical point in window coordinates.
/// 窗口坐标中的逻辑点。
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Point {
    /// Horizontal coordinate.
    /// 水平坐标。
    pub x: f32,
    /// Vertical coordinate.
    /// 垂直坐标。
    pub y: f32,
}

impl Point {
    /// Creates a point.
    /// 创建点。
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Logical window size.
/// 逻辑窗口尺寸。
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Size {
    /// Width in logical pixels.
    /// 以逻辑像素表示的宽度。
    pub width: f32,
    /// Height in logical pixels.
    /// 以逻辑像素表示的高度。
    pub height: f32,
}

impl Size {
    /// Creates a size.
    /// 创建尺寸。
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    /// Returns whether either dimension is non-positive.
    /// 返回任一维度是否非正。
    pub fn is_empty(self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }
}

/// Axis-aligned logical rectangle.
/// 轴对齐逻辑矩形。
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rect {
    /// Left edge.
    /// 左边缘。
    pub x: f32,
    /// Top edge.
    /// 上边缘。
    pub y: f32,
    /// Width.
    /// 宽度。
    pub width: f32,
    /// Height.
    /// 高度。
    pub height: f32,
}

impl Rect {
    /// Creates a rectangle.
    /// 创建矩形。
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Returns the right edge.
    /// 返回右边缘。
    pub fn right(self) -> f32 {
        self.x + self.width
    }

    /// Returns the bottom edge.
    /// 返回下边缘。
    pub fn bottom(self) -> f32 {
        self.y + self.height
    }

    /// Returns the center point.
    /// 返回中心点。
    pub fn center(self) -> Point {
        Point::new(self.x + self.width * 0.5, self.y + self.height * 0.5)
    }

    /// Tests point containment with an inclusive top-left and exclusive bottom-right edge.
    /// 以包含左上、排除右下的规则测试点是否在矩形内。
    pub fn contains(self, point: Point) -> bool {
        point.x >= self.x && point.x < self.right() && point.y >= self.y && point.y < self.bottom()
    }

    /// Returns the union of two rectangles.
    /// 返回两个矩形的并集。
    pub fn union(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());
        Self::new(x, y, right - x, bottom - y)
    }
}
