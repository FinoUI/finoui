[English](../en/architecture.md) · [简体中文](architecture.md)

# 基础架构

> 文档状态：实施基线 v0.2，优先使用成熟 crate，支持 AI 原生与无头开发。
> 目标平台：Windows、macOS、Linux、Android、iOS、Web。
> 首个垂直切片：同一份计数器组件，由真实 DOM 后端与原生 wgpu 后端共同驱动。
> 命名：文中的 `ui` 命令为工作名称，可在实现前替换；第三方依赖保留其实际名称。

## 项目定义

Fino 是一套以 Rust 实现的跨端 UI 框架。它采用 SolidJS 哲学的细粒度响应式模型：组件只执行一次，Signal 与具体 UI 属性建立精确依赖，状态变化直接更新目标节点，不通过虚拟 DOM，也不重新执行整个组件树。

框架统一以下能力：

- 组件、ownership、dispose 与响应式语义。
- DOM-like 的逻辑 UI 结构、事件与 keyed children。
- 文本、样式、布局和绘制的显式分阶段处理。
- 跨端组件源码与公共样式契约。

框架不强行统一以下实现：

- Web 的浏览器 DOM 存储与原生的 arena 节点存储。
- 浏览器 CSS 布局与原生布局引擎。
- 浏览器绘制与原生 wgpu 绘制。
- 桌面窗口、移动生命周期与 Web 页面宿主。

一句话概括：

> Fine-grained Reactive DOM + ShapedText + Incremental Layout/Paint + DOM/wgpu Renderers。

默认样式作者接口采用受控的 Tailwind 风格 `class` 子集，并编译为强类型 Style IR。Web CSS 由 Style IR 生成，原生后端直接消费 Style IR。Fino 的定位是 Tailwind-like，而非 Tailwind-compatible；Utility 是作者接口，Style IR 才是跨端事实来源。

## 设计目标与非目标

### 核心目标

| 目标 | 架构含义 |
|---|---|
| 组件只运行一次 | RSX 产生静态模板与动态绑定，不把组件函数当作每帧渲染函数 |
| 精确更新 | Signal 直接连接文本、属性、样式、uniform 或子结构锚点 |
| Web 使用实体 DOM | 普通元素、文本、表单与语义节点均为真实浏览器节点 |
| 原生高性能 DOM | arena、稳定 NodeId、分阶段失效，不复制浏览器的隐式同步布局 |
| 文本可预测 | `shape` 与 `layout` 分离，文本测量不要求节点挂载 |
| GPU 效果可扩展 | WGSL Material、Effect 与 GpuSurface 是一等绘制能力 |
| 全平台可演进 | 核心不包含具体窗口、GPU、DOM 或异步运行时类型 |
| 可验证 | 每个后端运行同一套一致性测试，性能指标可观测 |
| 成熟 crate 优先 | 自研只用于差异化层，不重复实现已有文本、布局、GPU 与协议算法 |
| AI 原生 | 框架与应用都能通过结构化 CLI、场景和快照完成无头开发闭环 |

### 明确非目标

首个大版本不追求：

- 完整实现 W3C DOM、CSS、HTML 或浏览器行为。
- 六端像素级一致。
- 在 Web 普通 CSS 中执行任意 WGSL。
- 同时提供 XAML、MVVM、CSS、JSX 等多套作者接口。
- 第一版就覆盖完整控件库、Shadow DOM、完整选择器与富文本编辑。
- 以“所有端共用最多代码”为理由抹平平台特有行为。

### 核心取舍

| 决策 | 选择 | 放弃的方案 |
|---|---|---|
| 响应式更新 | 细粒度依赖与直接节点更新 | VDOM、每帧重建组件树 |
| Web 渲染 | 实体 DOM 为默认 | 全部 UI 绘制进单个 Canvas |
| 原生结构 | DOM-like retained tree | 无结构的立即模式控件调用 |
| 原生渲染 | wgpu + WGSL | 自研多 GPU API 转译层 |
| 样式作者接口 | 受控的 Tailwind 风格 `class` 子集，编译为 Style IR | 以手写 CSS 为主要接口、Tailwind 运行时或任意运行时 class 拼接 |
| 文本 | `TextSpec → ShapedText → TextLayout` | 每次测量都触达 DOM 或重新 shaping |
| 布局读取 | 显式、分阶段、可缓存 | 读取几何时隐式 flush 全树 |
| 自定义 GPU | Material / Effect / GpuSurface 三类接口 | 所有需求都塞进一个 ShaderNode |
| 后端选择 | 编译期单后端，泛型静态分发 | 每个节点操作都经过动态分发 |

## 总体分层

```text
┌──────────────────────────────────────────────────────────┐
│ App：业务组件、Signals、路由、应用状态                    │
├──────────────────────────────────────────────────────────┤
│ Authoring：view!/rsx!、Show、For、组件与模板              │
├──────────────────────────────────────────────────────────┤
│ Runtime：Owner、Effect 调度、动态绑定、挂载与销毁          │
├──────────────────────────────────────────────────────────┤
│ DOM API：节点操作、事件、样式、语义与后端一致性契约         │
├───────────────────────┬──────────────────────────────────┤
│ Web Backend           │ Native Backend                   │
│ - 浏览器 DOM           │ - Native DOM arena               │
│ - CSS / SharedLayout   │ - Style / Text / Layout           │
│ - DOM events           │ - Display List / Damage           │
│ - DOM accessibility    │ - wgpu / WGSL                     │
├───────────────────────┴──────────────────────────────────┤
│ Host：Desktop / Android / iOS / Web                       │
│ Services：IME、剪贴板、文件、字体、无障碍、生命周期         │
└──────────────────────────────────────────────────────────┘
```

### 依赖方向

依赖只能向下，禁止平台能力反向进入响应式核心：

```text
fino::reactive
    ↓
fino::component
    ↓
fino::runtime ──────→ fino::dom_api
    ↓                       ↑
    ├────────────→ fino::web
    └────────────→ fino::native
                           ↓
         fino::text / fino::layout / fino::render_wgpu
                           ↓
                    fino::platform
```

`fino::reactive` 不依赖 DOM；`fino::dom_api` 不依赖 wgpu 或 web-sys；`fino::text` 不依赖窗口；`fino::layout` 不依赖具体绘制后端。`fino_primitives`、`fino_components` 与 `fino_blocks` 基于这些公共契约构建；`fino_glass` 提供默认语义 Token 与 Material 值。

## 分包与主题边界

```text
fino
└── 响应式、DOM、布局、渲染、Host，不包含具体视觉风格

fino_primitives
└── 无样式组件行为：焦点、键盘、状态机、A11y、弹出层

fino_components
└── Button、Input、Card、Tabs、Dialog 等正式组件
    只引用语义 Token 和 Material，不写死玻璃颜色

fino_glass
└── Solid Glass 官方默认主题
    明暗主题、三档厚度、边缘高光、阴影、模糊与动效

fino_blocks
└── Composer、ToolSteps、DiffView、AskCard 等 AI 应用区块
```

`fino_glass` 是默认主题，Solid Glass 的具体视觉值只由这个包持有。`fino_components` 与 `fino_blocks` 使用语义 Token 和 Material 能力，使组件行为与结构保持主题无关。

MIT 许可证的 [Solid Glass](https://github.com/deeplethe/solid-glass) 将样式表与 shadcn/ui 组件行为分开。Fino 保留这条边界，同时不把 React、Tailwind 或 shadcn 类型带入核心。架构基线以提交 `6e8903e494a15491ceda38fafea5056c95a74d44` 为视觉参考；复制源码或资源时必须保留对应上游版权与许可证声明。

## 建议工作区

```text
crates/
  fino/              # 响应式核心、DOM、布局、渲染、Host、无头工具
  fino_primitives/   # 无样式行为与无障碍
  fino_components/   # 主题无关的正式组件
  fino_glass/        # Solid Glass 默认主题
  fino_blocks/       # AI 应用区块
examples/
  counter/
    src/lib.rs       # 两端共享的计数器组件
    src/native.rs
    src/web.rs
```

`fino` 内部仍按响应式、组件、DOM API、运行时、后端、文本、布局、渲染、平台、Host、无头、场景、检查、CLI、MCP、DevTools 与测试模块保持本文所述边界。尤其不能把 web-sys、wgpu 或 winit 类型泄漏到共享组件 API。

## 第一组技术决策

开始编码时直接采用以下约束：

```text
语言：Rust
响应式：优先以 reactive_graph 实现 Owner + Signal + Effect + batch 语义，经薄适配层接入
首个组件：手工构建的 counter
首个后端：MockBackend
第二后端：Web DOM
第三后端：Native wgpu
Web 布局：BrowserFlow
Native 结构：arena + generational NodeId
样式：受控 Tailwind 风格 class → 强类型 Style IR；Web CSS 为生成物
文本接口：TextSpec → ShapedText → TextLayout
布局读取：禁止隐式同步 flush
Shader：后续使用 WGSL，不在计数器阶段实现
宏：底层垂直切片通过后再实现 view!
```

## 参考资料

- [SolidJS](https://www.solidjs.com/)：细粒度响应式与组件只执行一次的作者体验参考。
- [Solid Glass](https://github.com/deeplethe/solid-glass/tree/6e8903e494a15491ceda38fafea5056c95a74d44)：MIT 许可证项目，作为默认主题明暗配色、三档厚度、边缘处理、阴影、模糊与动效的参考。
- [Pretext](https://github.com/chenglou/pretext)：文本预计算与低成本布局的阶段分离参考；本项目不沿用其命名。
- [Makepad](https://github.com/makepad/makepad)：Shader、SDF、实例化绘制与开发体验参考。
- [wgpu](https://github.com/gfx-rs/wgpu)：原生与 Web GPU 后端基础设施。
- [winit](https://github.com/rust-windowing/winit)：原生窗口、输入与生命周期基础设施。
- [AccessKit](https://github.com/AccessKit/accesskit)：原生无障碍基础设施。
