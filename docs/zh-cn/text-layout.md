[English](../en/text-layout.md) · [简体中文](text-layout.md)

# 文本与布局

## 文本系统

### 公开模型

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

命名固定为 `shape / layout / paint`，公开 API 不使用 `prepare / Prepared*`。

### 阶段职责

| 阶段 | 输入变化 | 主要工作 |
|---|---|---|
| `shape` | 文本、字体、字号、locale、文字特性 | 规范化、script/BiDi 分析、字体回退、shaping、断行机会 |
| `layout` | 可用宽度、最大行数、对齐等约束 | 复用 shaping 缓存，生成行与几何 |
| `paint` | 颜色、透明度、绘制位置 | 绘制已有 glyph runs，不重新 shaping |

长文档按段落或逻辑块缓存。流式文本只重新 `shape` 被编辑的块，不能每追加一个 token 就处理整篇文档。

### 正确性边界

首版底层可复用成熟文本栈，但必须隐藏其具体类型。复杂脚本、阿拉伯文、组合字符、emoji、字体回退和 BiDi 不能简化为“把分段宽度相加”。缓存键至少覆盖文本、字体、locale、特性与必要的量化信息。

## 样式与布局

### Utility 作者接口

默认作者接口使用受控的 Tailwind 风格 `class` 子集：

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

- `h-9`、`px-4`、`rounded-lg` 是普通跨端 Utility。
- `active:`、`disabled:` 映射到组件交互状态。
- `glass-primary` 是 Fino 自定义的 Material Utility，无需展开成一长串阴影参数。
- `class:glass-chosen={selected}` 是预编译的动态绑定，只切换目标样式，不重新执行组件。

`fino_glass` 通过语义 Token 与 Material 映射提供 `glass-panel`、`glass-card`、`glass-popover`、`glass-primary`、`glass-chosen` 等 Material Utility。核心负责解析与 Style IR，不持有这些具体视觉值。

### 编译模型与动态样式

```text
Tailwind-like class
→ 宏在编译期解析、校验、去重与合并
→ StyleId＋强类型 Style IR
→ Web：生成静态 CSS／切换真实 DOM class
→ Native：映射布局、Paint、Material 与 Compositing
```

原生端无需实现 CSS，运行时也不引入 Tailwind。开发者看到 Utility 语法，框架内部保留可精确失效的强类型数据。Web CSS 只是生成物，不是跨端事实来源。

| 需求 | 推荐写法 | 实现 |
| --- | --- | --- |
| 静态样式 | `class="flex px-4"` | 编译期完全解析 |
| 有限状态切换 | `class:glass-chosen={selected}` | 切换预编译 StyleId |
| 连续动态数值 | `style:width={width}` | 直接更新布局属性、CSS 变量或 uniform |

运行时拼接任意 class 字符串不属于受支持的公共路径：

```rust
// Unsupported: the framework cannot determine the style set in advance.
// 不作为受支持模式：框架无法提前确定样式集合。
class={move || format!("w-[{}px]", width.get())}
```

这条限制保留了 Utility 的作者效率，同时避免运行时解析、无限样式组合和不可控的样式失效。

### 支持边界与冲突规则

初版支持：

- 布局、少量明确的 `display` 取值、间距、尺寸、字体、颜色、圆角、透明度、transform 和 transition。
- `hover`、`focus-visible`、`active`、`disabled`、选中状态。
- `dark:`、响应式断点、`motion-reduce:` 和减少透明度变体。
- `glass-panel`、`glass-card`、`glass-popover` 等 Material Utility。
- 同组 Utility 冲突时后者覆盖前者，不复制 CSS 选择器优先级体系。

Grid、复杂继承、伪元素、复杂选择器、`group-*`／`peer-*`、任意 CSS、JavaScript 插件和完整 Tailwind 兼容暂缓。公共语义以框架规范为准，不能由某个浏览器行为隐式决定。

### 布局树独立

DOM 树与布局树不是同一棵树：

```text
DOM Node
→ Style Resolution
→ Layout Box（0、1 或多个）
→ Fragment / Display Item
```

首版限制 `display: contents`、伪元素等复杂映射，但数据模型不要把 NodeId 永久等同于 LayoutId。

### 失效传播

| 变化 | 最小失效范围 |
|---|---|
| 文本颜色 | Paint |
| 背景或圆角 | Paint |
| transform | Compositing，必要时更新命中测试 |
| aria-label | Accessibility |
| 文本内容 | Shape → intrinsic size → 按需 Layout |
| padding / font-size | Layout → Paint |

子节点只有在 intrinsic size 真正变化时才向父级传播布局失效。初版可以接入 Taffy 等布局实现，但公共 API 不得暴露其类型，以便后续替换为更细粒度的自有实现。
