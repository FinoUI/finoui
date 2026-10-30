//! Minimal Parley and Fontique text engine with an explicit shape/layout split.
//! 基于 Parley 与 Fontique 的最小文本引擎，明确区分 shape 与 layout。
//!
//! The bundled Roboto variable font is licensed under the SIL Open Font License 1.1;
//! see `assets/fonts/OFL.txt`.
//! 随仓库分发的 Roboto 可变字体使用 SIL Open Font License 1.1，见
//! `assets/fonts/OFL.txt`。

use std::borrow::Cow;

use fontique::{
    Blob, Collection, CollectionOptions, FontInfoOverride, Language, SourceCache,
    SourceCacheOptions,
};
use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontWeight, LayoutContext, LineHeight,
    PositionedLayoutItem, StyleProperty, TextWrapMode,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Font family bundled with the native backend.
/// 原生后端随仓库分发的字体系列。
pub const BUNDLED_FONT_FAMILY: &str = "Roboto";

const BUNDLED_FONT: &[u8] = include_bytes!("../assets/fonts/Roboto.ttf");

/// Text direction requested by an author.
/// 作者声明的文本方向。
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum TextDirection {
    /// Let the Unicode bidi algorithm decide.
    /// 交由 Unicode BiDi 算法决定。
    #[default]
    Auto,
    /// Force a left-to-right base direction.
    /// 强制从左到右的基础方向。
    Ltr,
    /// Force a right-to-left base direction.
    /// 强制从右到左的基础方向。
    Rtl,
}

/// White-space behavior required by the counter.
/// 计数器所需的空白处理方式。
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum WhiteSpace {
    /// Allow ordinary soft wrapping.
    /// 允许普通软换行。
    #[default]
    Normal,
    /// Keep the text on one line.
    /// 保持单行。
    NoWrap,
}

/// Author-facing text style accepted by the minimal text engine.
/// 最小文本引擎接受的作者侧文本样式。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextStyle {
    /// Font family name. The native backend bundles [`BUNDLED_FONT_FAMILY`].
    /// 字体系列名。原生后端随仓库分发 [`BUNDLED_FONT_FAMILY`]。
    pub font_family: String,
    /// Font size in logical pixels.
    /// 以逻辑像素表示的字号。
    pub font_size: f32,
    /// CSS-style font weight from 1 to 1000.
    /// 1 到 1000 的 CSS 风格字重。
    pub font_weight: u16,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_family: BUNDLED_FONT_FAMILY.to_owned(),
            font_size: 18.0,
            font_weight: 500,
        }
    }
}

/// A complete text request; shaping and layout consume this value without touching a DOM.
/// 完整的文本请求；shaping 与 layout 消费该值且无需访问 DOM。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextSpec {
    /// Source text.
    /// 源文本。
    pub text: String,
    /// Font and decoration-independent text style.
    /// 与字体和装饰无关的文本样式。
    pub style: TextStyle,
    /// Locale identifier used by line breaking and font fallback.
    /// 用于断行与字体回退的区域标识。
    pub locale: String,
    /// Requested base direction.
    /// 请求的基础方向。
    pub direction: TextDirection,
    /// White-space handling.
    /// 空白处理方式。
    pub white_space: WhiteSpace,
}

impl TextSpec {
    /// Creates a counter-sized single-line specification.
    /// 创建计数器尺寸的单行文本规格。
    pub fn counter(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: TextStyle::default(),
            locale: "en".to_owned(),
            direction: TextDirection::Ltr,
            white_space: WhiteSpace::NoWrap,
        }
    }

    /// Creates a specification with explicit style fields.
    /// 使用完整样式字段创建文本规格。
    pub fn new(
        text: impl Into<String>,
        style: TextStyle,
        locale: impl Into<String>,
        direction: TextDirection,
        white_space: WhiteSpace,
    ) -> Self {
        Self {
            text: text.into(),
            style,
            locale: locale.into(),
            direction,
            white_space,
        }
    }
}

/// Constraints applied only during layout; changing them must not reshape the text.
/// 仅在布局阶段应用的约束；修改它们不得重新 shaping 文本。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextConstraints {
    /// Maximum advance width in logical pixels.
    /// 以逻辑像素表示的最大推进宽度。
    pub max_width: Option<f32>,
    /// Maximum line height budget in logical pixels.
    /// 以逻辑像素表示的最大行高预算。
    pub max_height: Option<f32>,
}

impl TextConstraints {
    /// Creates width and height constraints.
    /// 创建宽高约束。
    pub fn new(max_width: Option<f32>, max_height: Option<f32>) -> Self {
        Self {
            max_width,
            max_height,
        }
    }

    /// Creates unconstrained layout.
    /// 创建无约束布局。
    pub fn unbounded() -> Self {
        Self::default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct TextKey {
    text: String,
    family: String,
    size_bits: u32,
    weight: u16,
    locale: String,
    direction: TextDirection,
    white_space: WhiteSpace,
}

impl TextKey {
    fn from_spec(spec: &TextSpec) -> Self {
        Self {
            text: spec.text.clone(),
            family: spec.style.font_family.clone(),
            size_bits: spec.style.font_size.to_bits(),
            weight: spec.style.font_weight,
            locale: spec.locale.clone(),
            direction: spec.direction,
            white_space: spec.white_space,
        }
    }
}

/// Opaque shaped text with no line breaking applied yet.
/// 尚未应用断行的不透明 shaping 结果。
#[derive(Clone, Debug)]
pub struct ShapedText {
    key: TextKey,
    layout: parley::Layout<[u8; 4]>,
}

impl ShapedText {
    /// Returns the source text carried by this shaping result.
    /// 返回该 shaping 结果携带的源文本。
    pub fn text(&self) -> &str {
        &self.key.text
    }

    /// Returns the intrinsic shaping width with no line wrapping applied.
    /// 返回未应用换行的内在 shaping 宽度。
    pub fn width(&self) -> f32 {
        self.layout.calculate_content_widths().max
    }

    /// Returns the shaping height with no width constraint applied.
    /// 返回未应用宽度约束的 shaping 高度。
    pub fn height(&self) -> f32 {
        self.layout.height()
    }
}

/// One positioned glyph referencing its font data and baseline.
/// 一个携带字体数据与基线的定位 glyph。
#[derive(Clone, Debug)]
pub struct PositionedGlyph {
    /// Glyph identifier inside its font.
    /// 字体内的 glyph 标识。
    pub glyph_id: u32,
    /// Advance origin in line-local logical pixels.
    /// 行内逻辑像素的推进原点。
    pub x: f32,
    /// Baseline position in line-local logical pixels.
    /// 行内逻辑像素的基线位置。
    pub y: f32,
    /// Advance width.
    /// 推进宽度。
    pub advance: f32,
    /// Font data needed by the rasterizer.
    /// 光栅化所需的字体数据。
    pub font: parley::FontData,
    /// Font size in logical pixels.
    /// 以逻辑像素表示的字号。
    pub font_size: f32,
    /// Face index inside a font collection.
    /// 字体集合中的字形索引。
    pub font_index: u32,
}

/// Layout result for one shaped text and one set of constraints.
/// 单个 shaping 结果在给定约束下的布局结果。
#[derive(Clone, Debug)]
pub struct TextLayout {
    constraints: TextConstraints,
    width: f32,
    height: f32,
    line_count: usize,
    is_rtl: bool,
    glyphs: Vec<PositionedGlyph>,
}

impl TextLayout {
    /// Returns the width used by the line boxes.
    /// 返回行框使用的宽度。
    pub fn width(&self) -> f32 {
        self.width
    }

    /// Returns the height used by all line boxes.
    /// 返回全部行框使用的高度。
    pub fn height(&self) -> f32 {
        self.height
    }

    /// Returns the number of laid-out lines.
    /// 返回已布局的行数。
    pub fn line_count(&self) -> usize {
        self.line_count
    }

    /// Returns whether the paragraph base direction is right-to-left.
    /// 返回段落基础方向是否为从右到左。
    pub fn is_rtl(&self) -> bool {
        self.is_rtl
    }

    /// Returns positioned glyphs in visual order.
    /// 按视觉顺序返回定位 glyph。
    pub fn glyphs(&self) -> &[PositionedGlyph] {
        &self.glyphs
    }

    /// Returns the constraints used for this layout.
    /// 返回该布局使用的约束。
    pub fn constraints(&self) -> TextConstraints {
        self.constraints
    }
}

/// Observable text work used by acceptance counters and diagnostics.
/// 用于验收计数与诊断的可观测文本工作量。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TextEngineStats {
    /// Actual shaping operations.
    /// 实际 shaping 操作数。
    pub shape_calls: u64,
    /// Shaping results reused from the engine cache.
    /// 从引擎缓存复用的 shaping 结果数。
    pub shape_cache_hits: u64,
    /// Actual layout operations.
    /// 实际布局操作数。
    pub layout_calls: u64,
}

/// Minimal text engine owning font discovery, shaping, and line layout.
/// 持有字体发现、shaping 与行布局的最小文本引擎。
pub struct TextEngine {
    font_context: FontContext,
    layout_context: LayoutContext<[u8; 4]>,
    cache: Vec<(TextKey, ShapedText)>,
    stats: TextEngineStats,
}

impl std::fmt::Debug for TextEngine {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TextEngine")
            .field("stats", &self.stats)
            .field("cached", &self.cache.len())
            .finish_non_exhaustive()
    }
}

impl Default for TextEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEngine {
    /// Creates an engine that uses only the bundled font, so shaping is deterministic.
    /// 创建仅使用随仓库字体、因而 shaping 确定性的引擎。
    pub fn new() -> Self {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        let families = collection.register_fonts(
            Blob::from(BUNDLED_FONT.to_vec()),
            Some(FontInfoOverride {
                family_name: Some(BUNDLED_FONT_FAMILY),
                ..FontInfoOverride::default()
            }),
        );
        assert!(
            !families.is_empty(),
            "the bundled counter font must register at least one family"
        );
        let font_context = FontContext {
            collection,
            source_cache: SourceCache::new(SourceCacheOptions::default()),
        };
        Self {
            font_context,
            layout_context: LayoutContext::new(),
            cache: Vec::new(),
            stats: TextEngineStats::default(),
        }
    }

    /// Returns the bundled family name accepted by [`TextStyle::font_family`].
    /// 返回 [`TextStyle::font_family`] 接受的随仓库字体系列名。
    pub fn family(&self) -> &'static str {
        BUNDLED_FONT_FAMILY
    }

    /// Returns cumulative shaping and layout counters.
    /// 返回累计 shaping 与 layout 计数。
    pub fn stats(&self) -> TextEngineStats {
        self.stats
    }

    /// Shapes text and caches the result by text, font, locale, and white-space settings.
    /// 按文本、字体、区域与空白设置 shaping 文本并缓存结果。
    pub fn shape(&mut self, spec: &TextSpec) -> ShapedText {
        let key = TextKey::from_spec(spec);
        if let Some((_, shaped)) = self.cache.iter().find(|(cached, _)| *cached == key) {
            self.stats.shape_cache_hits += 1;
            return shaped.clone();
        }

        self.stats.shape_calls += 1;
        // Parley exposes only first-strong paragraph resolution. A leading LRM/RLM sets the
        // explicit base level without adding an advance or glyph.
        // Parley 仅公开 first-strong 段落方向解析；前置 LRM/RLM 可在不增加推进或 glyph 的
        // 情况下设置显式基础方向。
        let layout_text = match spec.direction {
            TextDirection::Auto => spec.text.clone(),
            TextDirection::Ltr => format!("\u{200E}{}", spec.text),
            TextDirection::Rtl => format!("\u{200F}{}", spec.text),
        };
        let mut builder =
            self.layout_context
                .ranged_builder(&mut self.font_context, &layout_text, 1.0, true);
        builder.push_default(StyleProperty::FontFamily(FontFamily::Source(Cow::Owned(
            spec.style.font_family.clone(),
        ))));
        builder.push_default(StyleProperty::FontSize(spec.style.font_size));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(f32::from(
            spec.style.font_weight,
        ))));
        builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
            1.25,
        )));
        if spec.white_space == WhiteSpace::NoWrap {
            builder.push_default(StyleProperty::TextWrapMode(TextWrapMode::NoWrap));
        }
        if let Ok(language) = Language::parse(&spec.locale) {
            builder.push_default(StyleProperty::Locale(Some(language)));
        }
        let layout = builder.build(&layout_text);
        let shaped = ShapedText { key, layout };
        if self.cache.len() >= 32 {
            self.cache.remove(0);
        }
        self.cache.push((shaped.key.clone(), shaped.clone()));
        shaped
    }

    /// Applies line breaking and collects positioned glyphs without touching DOM state.
    /// 应用断行并收集定位 glyph，不触碰 DOM 状态。
    pub fn layout(&mut self, shaped: &ShapedText, constraints: TextConstraints) -> TextLayout {
        self.stats.layout_calls += 1;
        let mut layout = shaped.layout.clone();
        let max_advance = if shaped.key.white_space == WhiteSpace::NoWrap {
            None
        } else {
            constraints.max_width
        };
        layout.break_all_lines(max_advance);
        layout.align(Alignment::Start, AlignmentOptions::default());

        let mut glyphs = Vec::new();
        let mut line_count = 0;
        let mut width = 0.0_f32;
        let mut height = 0.0_f32;
        let max_height = constraints.max_height;
        let is_rtl = layout.is_rtl();
        for line in layout.lines() {
            let metrics = line.metrics();
            if max_height.is_some_and(|budget| height + metrics.line_height > budget) {
                break;
            }
            height += metrics.line_height;
            width = width.max(metrics.advance - metrics.trailing_whitespace);
            line_count += 1;
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(run) = item else {
                    continue;
                };
                let font = run.run().font().clone();
                let font_size = run.run().font_size();
                for glyph in run.positioned_glyphs() {
                    glyphs.push(PositionedGlyph {
                        glyph_id: glyph.id,
                        x: glyph.x,
                        y: glyph.y,
                        advance: glyph.advance,
                        font: font.clone(),
                        font_size,
                        font_index: font.index,
                    });
                }
            }
        }

        TextLayout {
            constraints,
            width,
            height,
            line_count,
            is_rtl,
            glyphs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Proves content changes reshape while width-only changes only rerun layout.
    /// 验证内容变化会重新 shaping，而仅宽度变化只重新布局。
    #[test]
    fn width_only_changes_reuse_shaping() {
        let mut engine = TextEngine::new();
        let spec = TextSpec::counter("Count: 0");
        let shaped = engine.shape(&spec);
        let first = engine.layout(&shaped, TextConstraints::new(Some(240.0), None));
        let second = engine.layout(&shaped, TextConstraints::new(Some(320.0), None));
        assert_eq!(engine.stats().shape_calls, 1);
        assert_eq!(engine.stats().layout_calls, 2);
        assert_eq!(first.line_count(), 1);
        assert_eq!(second.line_count(), 1);
        assert!(!first.glyphs().is_empty());
        assert!((first.width() - second.width()).abs() < f32::EPSILON);

        let changed = engine.shape(&TextSpec::counter("Count: 1"));
        assert_eq!(engine.stats().shape_calls, 2);
        assert!(changed.width() > 0.0);
    }

    /// Applies explicit paragraph direction, including mixed-direction text.
    /// 应用显式段落方向，包括混合方向文本。
    #[test]
    fn explicit_direction_controls_base_level() {
        let mut engine = TextEngine::new();
        let style = TextStyle::default();
        let mixed = "abc \u{05D0}\u{05D1}\u{05D2}";
        let rtl = TextSpec::new(
            mixed,
            style.clone(),
            "en",
            TextDirection::Rtl,
            WhiteSpace::NoWrap,
        );
        let ltr = TextSpec::new(
            mixed,
            style.clone(),
            "en",
            TextDirection::Ltr,
            WhiteSpace::NoWrap,
        );
        let rtl_shaped = engine.shape(&rtl);
        let ltr_shaped = engine.shape(&ltr);
        let rtl_layout = engine.layout(&rtl_shaped, TextConstraints::unbounded());
        let ltr_layout = engine.layout(&ltr_shaped, TextConstraints::unbounded());
        assert!(rtl_layout.is_rtl());
        assert!(!ltr_layout.is_rtl());
        assert!(!rtl_layout.glyphs().is_empty());
        assert!(!ltr_layout.glyphs().is_empty());

        let auto_rtl = TextSpec::new(
            "\u{05D0}\u{05D1}\u{05D2}",
            style.clone(),
            "he",
            TextDirection::Auto,
            WhiteSpace::NoWrap,
        );
        let auto_rtl_shaped = engine.shape(&auto_rtl);
        let auto_rtl_layout = engine.layout(&auto_rtl_shaped, TextConstraints::unbounded());
        assert!(auto_rtl_layout.is_rtl());
    }

    /// Height budgets drop later lines and their glyphs instead of overflowing.
    /// 高度预算会丢弃后续行及其 glyph，而不是溢出预算。
    #[test]
    fn max_height_truncates_lines_and_glyphs() {
        let mut engine = TextEngine::new();
        let spec = TextSpec::new(
            "alpha beta gamma delta",
            TextStyle::default(),
            "en",
            TextDirection::Ltr,
            WhiteSpace::Normal,
        );
        let shaped = engine.shape(&spec);
        let full = engine.layout(&shaped, TextConstraints::new(Some(60.0), None));
        assert!(full.line_count() > 1);
        let budget = full.height() * 0.5;
        let clipped = engine.layout(&shaped, TextConstraints::new(Some(60.0), Some(budget)));
        assert!(clipped.line_count() < full.line_count());
        assert!(clipped.height() <= budget + f32::EPSILON);
        assert!(clipped.glyphs().len() < full.glyphs().len());

        let none = engine.layout(&shaped, TextConstraints::new(Some(60.0), Some(0.0)));
        assert_eq!(none.line_count(), 0);
        assert!(none.glyphs().is_empty());
        assert_eq!(none.height(), 0.0);
    }

    /// Reuses a cached shaping result for identical specifications.
    /// 对相同规格复用缓存的 shaping 结果。
    #[test]
    fn identical_specs_hit_the_shaping_cache() {
        let mut engine = TextEngine::new();
        let spec = TextSpec::counter("Count: 0");
        let first = engine.shape(&spec);
        let second = engine.shape(&spec);
        assert_eq!(engine.stats().shape_calls, 1);
        assert_eq!(engine.stats().shape_cache_hits, 1);
        assert_eq!(first.text(), second.text());
    }
}
