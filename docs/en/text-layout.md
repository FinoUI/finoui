[English](text-layout.md) · [简体中文](../zh-cn/text-layout.md)

# Text and layout

## Text system

### Public model

```rust
pub struct TextSpec {
    pub text: TextContent,
    pub style: TextStyle,
    pub locale: Locale,
    pub direction: TextDirection,
    pub white_space: WhiteSpace,
}

pub struct ShapedText {
    // Opaque: normalized text, runs, glyphs, line-break opportunities, cache keys.
    // 不透明：规范文本、runs、glyphs、断行机会、缓存键。
}

pub struct TextLayout {
    // Lines, dimensions, glyph runs, cursor mapping, and hit-test information.
    // 行、尺寸、glyph runs、光标映射与命中测试信息。
}

impl TextEngine {
    pub fn shape(&mut self, spec: TextSpec) -> ShapedText;
    pub fn layout(
        &mut self,
        text: &ShapedText,
        constraints: LayoutConstraints,
    ) -> TextLayout;
}
```

The names are fixed as `shape / layout / paint`. Public APIs do not use `prepare / Prepared*`.

### Stage responsibilities

| Stage | Input changes | Main work |
|---|---|---|
| `shape` | Text, font, font size, locale, text features | Normalization, script/BiDi analysis, font fallback, shaping, line-break opportunities |
| `layout` | Constraints such as available width, maximum lines, and alignment | Reuses shaping caches and produces lines and geometry |
| `paint` | Color, opacity, paint position | Paints existing glyph runs without reshaping |

Long documents are cached by paragraph or logical block. Streaming text only reshapes edited blocks; appending a token must not process the entire document again.

### Correctness boundaries

The first version may reuse a mature text stack internally, but it must hide the concrete types. Complex scripts, Arabic, combining characters, emoji, font fallback, and BiDi cannot be reduced to adding segment widths. Cache keys cover at least text, font, locale, features, and necessary quantization information.

## Style and layout

### Utility authoring

The default author interface uses a controlled Tailwind-like `class` subset:

```rust
view! {
    <div class="glass-card rounded-2xl p-5">
        <button
            class="glass-primary h-9 px-4 rounded-lg text-sm font-medium
                   active:translate-y-px disabled:opacity-50"
            class:glass-chosen={selected}
        >
            "Create project"
        </button>
    </div>
}
```

- `h-9`, `px-4`, and `rounded-lg` are ordinary cross-platform utilities.
- `active:` and `disabled:` map to component interaction states.
- `glass-primary` is a Fino Material Utility rather than a long list of shadow parameters.
- `class:glass-chosen={selected}` is a precompiled dynamic binding that only switches the target style; it does not re-execute the component.

`fino_glass` supplies Material Utilities such as `glass-panel`, `glass-card`, `glass-popover`, `glass-primary`, and `glass-chosen` through semantic tokens and material mappings. The core owns parsing and Style IR, not these concrete visual values.

### Compile model and dynamic styles

```text
Tailwind-like class
→ Macro parses, validates, deduplicates, and merges at compile time
→ StyleId + strongly typed Style IR
→ Web: generate static CSS / toggle real DOM classes
→ Native: map to Layout, Paint, Material, and Compositing
```

Native does not implement CSS, and the runtime does not include Tailwind. Developers see Utility syntax while the framework retains strongly typed data with precise invalidation. Web CSS is a generated artifact, never the cross-platform source of truth.

| Requirement | Recommended syntax | Implementation |
|---|---|---|
| Static styles | `class="flex px-4"` | Fully resolved at compile time |
| Bounded state switching | `class:glass-chosen={selected}` | Switches a precompiled StyleId |
| Continuous dynamic values | `style:width={width}` | Directly updates a layout property, CSS variable, or uniform |

Arbitrary runtime class-string construction is not a supported public path:

```rust
// Unsupported: the framework cannot determine the style set in advance.
// 不作为受支持模式：框架无法提前确定样式集合。
class={move || format!("w-[{}px]", width.get())}
```

This restriction retains Utility authoring efficiency without runtime parsing, unbounded style combinations, or uncontrolled style invalidation.

### Supported subset and conflict rules

The initial subset supports:

- Layout, a small set of explicit `display` values, spacing, sizing, typography, color, border radius, opacity, transform, and transition.
- `hover`, `focus-visible`, `active`, `disabled`, and selected states.
- `dark:`, responsive breakpoints, `motion-reduce:`, and reduced-transparency variants.
- Material Utilities including `glass-panel`, `glass-card`, and `glass-popover`.
- Last-wins conflicts within a utility group, without reproducing CSS selector specificity.

Grid, complex inheritance, pseudo-elements, complex selectors, `group-*` / `peer-*`, arbitrary CSS, JavaScript plugins, and full Tailwind compatibility are deferred. Framework specifications define shared semantics; browser behavior does not implicitly determine them.

### Independent layout tree

The DOM tree and layout tree are distinct:

```text
DOM Node
→ Style Resolution
→ Layout Box (zero, one, or more)
→ Fragment / Display Item
```

The first version limits complex mappings such as `display: contents` and pseudo-elements, but the data model must not permanently equate `NodeId` with `LayoutId`.

### Invalidation propagation

| Change | Minimum invalidation scope |
|---|---|
| Text color | Paint |
| Background or border radius | Paint |
| Transform | Compositing, with hit testing updated when needed |
| `aria-label` | Accessibility |
| Text content | Shape → intrinsic size → layout as needed |
| Padding or font size | Layout → Paint |

A child propagates layout invalidation to its parent only when its intrinsic size actually changes. The first version may integrate an implementation such as Taffy, but public APIs must not expose its types. This preserves the option to replace it with a more fine-grained implementation later.
