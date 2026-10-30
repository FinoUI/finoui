//! Native display list, damage tracking, and window-tagged paint context.
//! 原生绘制列表、Damage 跟踪与携带窗口身份的绘制上下文。

use std::rc::Rc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    dom::NodeId,
    geom::{Rect, Size},
    id::WindowId,
};
use crate::text::TextLayout;

/// Straight-alpha color used by the minimal native renderer.
/// 最小原生渲染器使用的直通 alpha 颜色。
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Color {
    /// Red channel from 0 to 1.
    /// 0 到 1 的红色通道。
    pub r: f32,
    /// Green channel from 0 to 1.
    /// 0 到 1 的绿色通道。
    pub g: f32,
    /// Blue channel from 0 to 1.
    /// 0 到 1 的蓝色通道。
    pub b: f32,
    /// Alpha channel from 0 to 1.
    /// 0 到 1 的 alpha 通道。
    pub a: f32,
}

impl Color {
    /// Creates a color from straight-alpha components.
    /// 使用直通 alpha 分量创建颜色。
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Converts the color into a renderer-friendly array.
    /// 把颜色转换为渲染器友好的数组。
    pub const fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

/// Minimal damage set for one window.
/// 单个窗口的最小 Damage 集合。
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Damage {
    /// Whether the whole surface must be repainted.
    /// 是否必须重绘整个 Surface。
    pub full: bool,
    /// Additional damaged rectangle.
    /// 额外受损矩形。
    pub rect: Option<Rect>,
}

impl Damage {
    /// Creates an empty damage set.
    /// 创建空 Damage。
    pub const fn empty() -> Self {
        Self {
            full: false,
            rect: None,
        }
    }

    /// Creates full-surface damage.
    /// 创建全 Surface Damage。
    pub const fn full() -> Self {
        Self {
            full: true,
            rect: None,
        }
    }

    /// Creates damage for one rectangle.
    /// 为一个矩形创建 Damage。
    pub const fn rect(rect: Rect) -> Self {
        Self {
            full: false,
            rect: Some(rect),
        }
    }

    /// Returns whether no area needs repainting.
    /// 返回是否没有区域需要重绘。
    pub fn is_empty(self) -> bool {
        !self.full && self.rect.is_none()
    }

    /// Adds one rectangle to the damage set.
    /// 向 Damage 添加一个矩形。
    pub fn add(&mut self, rect: Rect) {
        if self.full {
            return;
        }
        self.rect = Some(match self.rect {
            Some(existing) => existing.union(rect),
            None => rect,
        });
    }
}

/// One native paint command; every command carries its owning window.
/// 一条原生绘制命令；每条命令都携带所属窗口。
#[derive(Clone, Debug)]
pub enum PaintCommand {
    /// Solid rectangle.
    /// 实心矩形。
    Rect {
        /// Owning window.
        /// 所属窗口。
        window: WindowId,
        /// Source node.
        /// 源节点。
        node: NodeId,
        /// Stable semantic node identifier.
        /// 稳定语义节点标识。
        stable_id: u64,
        /// Logical bounds.
        /// 逻辑边界。
        bounds: Rect,
        /// Fill color.
        /// 填充颜色。
        color: Color,
    },
    /// Text run with pre-shaped glyphs.
    /// 带预 shaping glyph 的文本 run。
    Text {
        /// Owning window.
        /// 所属窗口。
        window: WindowId,
        /// Source node.
        /// 源节点。
        node: NodeId,
        /// Stable semantic node identifier.
        /// 稳定语义节点标识。
        stable_id: u64,
        /// Logical text origin.
        /// 逻辑文本原点。
        bounds: Rect,
        /// Source text used by diagnostics.
        /// 诊断使用的源文本。
        text: String,
        /// Shaped and laid-out text.
        /// 已 shaping 并布局的文本。
        layout: Rc<TextLayout>,
        /// Fill color.
        /// 填充颜色。
        color: Color,
    },
}

impl PaintCommand {
    /// Returns the owning window.
    /// 返回所属窗口。
    pub fn window(&self) -> WindowId {
        match self {
            Self::Rect { window, .. } | Self::Text { window, .. } => *window,
        }
    }

    /// Returns the source node.
    /// 返回源节点。
    pub fn node(&self) -> NodeId {
        match self {
            Self::Rect { node, .. } | Self::Text { node, .. } => *node,
        }
    }

    /// Returns the command bounds.
    /// 返回命令边界。
    pub fn bounds(&self) -> Rect {
        match self {
            Self::Rect { bounds, .. } | Self::Text { bounds, .. } => *bounds,
        }
    }
}

/// Complete display list for one window and one frame.
/// 单个窗口单帧的完整绘制列表。
#[derive(Clone, Debug)]
pub struct DisplayList {
    window: WindowId,
    viewport: Size,
    commands: Vec<PaintCommand>,
    damage: Damage,
}

impl DisplayList {
    /// Creates an empty display list for one window.
    /// 为一个窗口创建空绘制列表。
    pub fn new(window: WindowId, viewport: Size, damage: Damage) -> Self {
        Self {
            window,
            viewport,
            commands: Vec::new(),
            damage,
        }
    }

    /// Returns the owning window.
    /// 返回所属窗口。
    pub fn window(&self) -> WindowId {
        self.window
    }

    /// Returns the viewport used to build this list.
    /// 返回构建该列表时使用的视口。
    pub fn viewport(&self) -> Size {
        self.viewport
    }

    /// Returns the commands in paint order.
    /// 按绘制顺序返回命令。
    pub fn commands(&self) -> &[PaintCommand] {
        &self.commands
    }

    /// Returns the damage consumed by this frame.
    /// 返回该帧消费的 Damage。
    pub fn damage(&self) -> Damage {
        self.damage
    }

    /// Appends one command after verifying that it belongs to this window.
    /// 校验命令属于当前窗口后追加命令。
    pub fn push(&mut self, command: PaintCommand) {
        assert_eq!(
            command.window(),
            self.window,
            "paint commands must carry the display list window"
        );
        self.commands.push(command);
    }

    /// Serializes the list into the versioned display-list artifact.
    /// 将列表序列化为版本化 display-list 产物。
    pub fn to_snapshot(&self) -> Value {
        let commands: Vec<Value> = self
            .commands
            .iter()
            .map(|command| match command {
                PaintCommand::Rect {
                    window,
                    stable_id,
                    bounds,
                    color,
                    ..
                } => json!({
                    "kind": "rect",
                    "window": window,
                    "node": format!("node-{stable_id}"),
                    "rect": bounds,
                    "color": color,
                }),
                PaintCommand::Text {
                    window,
                    stable_id,
                    bounds,
                    text,
                    layout,
                    color,
                    ..
                } => json!({
                    "kind": "text",
                    "window": window,
                    "node": format!("node-{stable_id}"),
                    "rect": bounds,
                    "text": text,
                    "glyphs": layout.glyphs().len(),
                    "lines": layout.line_count(),
                    "color": color,
                }),
            })
            .collect();
        json!({
            "schema": 1,
            "window": self.window,
            "viewport": self.viewport,
            "damage": self.damage,
            "commands": commands,
        })
    }
}

/// Paint context that forces every emitted command to carry [`WindowId`].
/// 强制每条绘制命令携带 [`WindowId`] 的绘制上下文。
#[derive(Clone, Copy, Debug)]
pub struct PaintContext {
    window: WindowId,
}

impl PaintContext {
    /// Creates a paint context for one window.
    /// 为一个窗口创建绘制上下文。
    pub const fn new(window: WindowId) -> Self {
        Self { window }
    }

    /// Returns the owning window.
    /// 返回所属窗口。
    pub fn window(&self) -> WindowId {
        self.window
    }

    /// Emits a solid rectangle for one node.
    /// 为一个节点输出实心矩形。
    pub fn rect(&self, node: NodeId, stable_id: u64, bounds: Rect, color: Color) -> PaintCommand {
        PaintCommand::Rect {
            window: self.window,
            node,
            stable_id,
            bounds,
            color,
        }
    }

    /// Emits a text run for one node.
    /// 为一个节点输出文本 run。
    pub fn text(
        &self,
        node: NodeId,
        stable_id: u64,
        bounds: Rect,
        text: String,
        layout: Rc<TextLayout>,
        color: Color,
    ) -> PaintCommand {
        PaintCommand::Text {
            window: self.window,
            node,
            stable_id,
            bounds,
            text,
            layout,
            color,
        }
    }
}
