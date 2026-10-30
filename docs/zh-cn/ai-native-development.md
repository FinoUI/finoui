[English](../en/ai-native-development.md) · [简体中文](ai-native-development.md)

# AI 原生开发

## AI 辅助实施协议

AI 适合加速实现，但每个任务必须包含可执行验收条件。推荐工作单元：

```text
一份小范围规格
→ 先写失败测试
→ 实现最小代码
→ 运行单元与一致性测试
→ 检查公共类型是否泄漏平台依赖
→ 运行性能计数
→ 提交
```

核心不变量由架构维护者明确掌握：

- Signal 依赖与 Effect 调度。
- Owner/dispose 与 Rust 所有权。
- DOM 节点生命周期。
- 布局失效传播。
- 文本缓存键。
- GPU 资源与 Surface 生命周期。

不要让 AI 一次性生成所有平台、完整 CSS、完整文本引擎和控件库。每个 PR 都应能独立回退，并且不破坏计数器一致性测试。

## 目标

AI 对框架或应用的开发闭环应当全部可以在无显示服务器的环境中运行：

```text
读取机器可读能力
→ 生成或修改 Rust/RSX
→ fmt / check / clippy / test
→ 运行无头场景
→ 获取 DOM、A11y、Reactive、Layout、Trace 与图像快照
→ 根据结构化失败修复
→ 重跑并生成验证报告
```

目标是让每次生成都伴随可执行反馈，而非绕过验证。CLI 是事实来源，Agent Skill 只把同一套工作流教给 Agent，不复制业务逻辑。只有外部集成、鉴权或跨客户端需求出现时，才考虑增加 MCP 薄适配层。

## 工具分层

```text
fino::runtime / fino::dom_api / fino::backends::*
        ↓
fino::headless：虚拟时钟、确定性输入、离屏目标
        ↓
fino::scenario：Action / Assert / Capture
        ↓
fino::inspect + fino::test：结构化状态与验证
        ↓
fino_cli：通用命令、JSON 输出、artifacts
        ↓
.agents/skills/fino：只提供工作流指引
        ↓
任意 AI Agent / CI / 人类开发者
```

`fino::headless` 不模拟响应式或 DOM；它运行真实框架代码，只替换时间、窗口、帧呈现和输入来源。

## 无头后端矩阵

| 后端 | 用途 | 是否需要显示器 |
|---|---|---|
| Mock | 响应式、DOM 操作、dispose 与事件逻辑 | 否 |
| NativeOffscreen | 真实文本、布局、Display List、离屏图像 | 否 |
| NativeSoftware | 固定软件渲染器下的确定性快照 | 否 |
| WebHeadless | Headless Chromium 中的真实 DOM、事件与截图 | 否 |
| HostIntegration | winit、窗口和平台服务集成，可在 Xvfb 等 CI 环境运行 | 通常否 |
| MobileEmulator | Android/iOS 构建、生命周期与有限集成测试 | 否，但需要对应 CI 工具链 |

NativeOffscreen 直接创建纹理目标，不要求 winit Surface。确定性图像基线优先使用固定版本的软件渲染路径；硬件 GPU 测试作为独立性能与兼容性任务，不与逻辑快照混为一谈。

## 场景协议与结构化检查

### 场景即应用测试作者接口

场景使用版本化 JSON，并由同一组 Rust 类型生成 JSON Schema：

```json
{
  "schema": 1,
  "name": "counter/increment",
  "mount": "counter",
  "environment": {
    "viewport": [800, 600],
    "dpr": 1.0,
    "locale": "zh-CN",
    "color_scheme": "dark",
    "seed": 42
  },
  "steps": [
    { "assert_text": { "role": "button", "name": "Increment counter", "value": "Count: 0" } },
    { "click": { "role": "button", "name": "Increment counter" } },
    { "advance_frames": 1 },
    { "assert_text": { "role": "button", "name": "Increment counter", "value": "Count: 1" } },
    { "capture": ["dom", "a11y", "reactive"] }
  ]
}
```

跨后端共享场景只捕获各后端都支持的逻辑快照。`layout`、`display_list` 与 `screenshot` 通过 NativeOffscreen／WebHeadless 的后端特定运行参数追加，不让 Mock 因没有布局或图像能力而跳过核心断言；不支持的能力必须显式报告，禁止静默生成空快照。`capture` 动作立即把快照写入 `captures/step-{index}/`，后续步骤不能覆盖更早的时点状态。

选择器优先级固定为 `role + accessible name`、稳定 `test_id`、语义查询。禁止 AI 依赖易变的内部 NodeId、屏幕绝对坐标或自动生成 class 作为默认定位方式。

场景必须支持虚拟时间和显式帧推进。动画、debounce、异步任务和随机数据均通过可注入时钟与种子控制，而不是让测试 `sleep` 猜测完成时间。

### 检查快照

统一提供以下只读快照：

| 快照 | 内容 |
|---|---|
| `dom` | 元素、文本、属性、父子关系、稳定语义 ID |
| `a11y` | Role、name、focus、actions 与状态 |
| `reactive` | Signal/Effect 调试名、依赖、执行次数、最近变更原因 |
| `layout` | 约束、尺寸、位置、缓存命中与失效原因 |
| `display_list` | 绘制命令、Layer、Material、Damage 与离屏边界 |
| `resources` | 字体、纹理、GPU buffer、缓存大小与 Surface 状态 |
| `trace` | 事务、后端操作、帧阶段、错误与源码位置 |

快照使用稳定排序和版本化 schema。内部指针、随机地址与无意义的时间戳不得进入快照，否则无法做 AI 差分。

### 结构化错误

所有工具错误尽量统一为：

```json
{
  "code": "UI_LAYOUT_INVALID_CONSTRAINT",
  "message": "layout produced a non-finite width",
  "severity": "error",
  "source": {
    "file": "examples/counter/src/lib.rs",
    "line": 18,
    "column": 9
  },
  "backend": "native-offscreen",
  "node": { "test_id": "counter-button" },
  "artifacts": ["trace.json", "dom.json", "layout.json"],
  "hint": "Inspect the last width constraint before frame 3"
}
```

`hint` 只是调查入口，不能替代 trace 证据。Rust panic、wgpu validation、浏览器异常和宏错误都应转换为带原始输出的工具结果，同时保留退出码。

## CLI、Skill 与应用工具链

### 共用 CLI

`fino_cli` 产生 `fino` 命令，只提供项目与工具自身的通用操作，不负责发现、挂载、检查或依赖任何示例。最小命令集：

```bash
fino doctor --format json
fino check --format json
fino inspect dom --run artifacts/runs/counter-web --format json
fino inspect a11y --run artifacts/runs/counter-web --format json
fino shader check --format json
fino bench --baseline main --format json
```

默认输出适合人类，`--format json` 返回 Agent 使用的同一结构化结果。大快照写进 artifacts，命令只返回摘要与路径，避免把数兆 DOM 或 Base64 图片塞进上下文。每次场景运行必须返回稳定的 `run_id` 与 `run_dir`；`fino inspect --run` 读取已完成的运行产物，`fino inspect --session` 读取显式保活的调试会话，不依赖隐藏的全局应用状态。

### 示例通过 Cargo 运行

每个示例拥有自己的组件、挂载函数、冒烟检查和场景运行器。开发者通过 Cargo 调用运行器，`fino_cli` 不承担任何示例专属工作：

```bash
cargo run -p fino_counter -- check --format json
cargo run -p fino_counter -- scenario run examples/counter/scenarios/counter.json --backend mock --out artifacts/runs/counter-mock --format json
cargo run -p fino_counter -- scenario run examples/counter/scenarios/counter.json --backend web-headless --out artifacts/runs/counter-web --format json
cargo run -p fino_counter -- scenario run examples/counter/scenarios/counter.json --backend native-offscreen --out artifacts/runs/counter-native --format json
```

示例调用共享的 `fino::scenario` 与 `fino::headless` library 接口。`fino inspect` 只读取产物，不需要知道产物来自哪个示例。

`web-headless` 运行器在示例内完成 wasm 构建、`wasm-bindgen` 绑定生成、无头 Chromium 启动和同一场景执行。它写入 `browser.json`，记录真实点击、焦点、键盘激活、节点身份、`nodeValue` 更新次数、`innerHTML` 赋值次数和批量提交次数。环境前置条件：

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129
npm ci --prefix examples/counter/web
npx --prefix examples/counter/web playwright install chromium
```

缺少 Node.js、Playwright、Chromium 或版本匹配的 `wasm-bindgen` 时，运行器返回结构化失败，不跳过浏览器步骤。`WASM_BINDGEN`、`NODE` 与 `FINO_CHROMIUM` 可覆盖对应可执行文件路径。

### Agent Skill

仓库提供 `.agents/skills/fino/SKILL.md`，记录命令选择、基于 Cargo 的示例工作流、JSON 输出约定和产物导航。Skill 不包含场景实现、挂载注册表或业务逻辑；Agent 直接调用 CLI 与 Cargo 命令，不解析人类终端文本。

初始工具链不包含 MCP。以后出现外部集成、鉴权或跨客户端访问需求时，可以增加适配层并复用同一结构化 CLI 结果，但不能形成第二套实现。

### 应用模板

应用模板默认包含：

```text
src/                  # 应用组件与业务代码
scenarios/            # AI 可执行的用户行为与断言
assets/fonts/         # 许可明确、可复现的测试字体
tests/                # Rust、后端一致性与宏测试
fino.toml             # 后端、环境与快照配置
AGENTS.md             # 项目命令、边界与验证要求
artifacts/            # 可忽略生成的报告、图像与 trace
```

业务应用拥有自己的挂载与场景二进制，并通过 Cargo 运行，不需要单独发明测试 Host。只要使用框架的组件、语义和平台服务，就自动获得 headless mount、输入场景、快照和性能计数，同时不把应用依赖带入 `fino_cli`。

### AI 友好的 Rust／RSX 设计

- 使用 `rstml` 的可恢复解析与 Span，宏错误指向准确源码。
- 用 `trybuild` 固定编译错误，避免 AI 面对“宏内部 panic”。
- 公共类型和函数使用一致命名、rustdoc 示例与可运行 doctest。
- Signal、Effect、Material 等调试对象支持开发态名称和 `#[track_caller]` 来源。
- 提供能力清单和版本化 schema，不让 AI 从源码猜测命令是否支持。
- 组件元数据、场景和检查快照共享语义 ID，减少多工具之间的推理断点。
- 格式化、展开和诊断优先组合 rustfmt、cargo expand、leptosfmt／rstml 生态经验，不重复实现完整 Rust 分析器。
