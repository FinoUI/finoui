[English](../en/dependencies.md) · [简体中文](dependencies.md)

# 依赖策略

## 成熟 crate 优先

### 自研边界

本项目只把以下部分视为必须自研的差异化能力：

- 跨 DOM／Native Backend 的组件挂载与动态绑定。
- 响应式事务到 Style／Text／Layout／Paint 的调度与失效协议。
- 统一 DOM 语义、后端一致性契约与平台能力模型。
- Material、Effect、GpuSurface 与响应式 uniform 的集成。
- AI 场景协议、结构化检查、无头 Host 与开发工具链。

以下能力默认不重复造轮子：响应式图算法、Unicode 与 shaping、Flex/Grid 算法、向量光栅化、GPU 抽象、窗口系统、无障碍平台桥接、RSX 解析、图像解码、CLI 解析与通用测试设施。

“自研”必须回答：现有库缺少的是功能、语义还是性能？能否通过适配、组合或向上游提交小补丁解决？只有可复现的阻断问题才进入自研或临时 fork。

### 推荐依赖基线

| 层 | 首选候选 | 接入方式与理由 |
|---|---|---|
| 响应式 | `reactive_graph` | 已提供 Signal、派生计算、Effect 与自动依赖追踪；通过 `fino::reactive` 适配框架调度语义 |
| RSX | `rstml` + `syn` + `quote` | 复用可恢复 RSX 解析和源码 Span，不从 TokenStream 手写 HTML 解析器 |
| 布局 | `taffy` | 已有 Block、Flexbox、Grid 与外部测量能力；隐藏在 `fino::layout` 后 |
| 文本 | `parley` + `fontique` | 复用字体回退、HarfRust shaping、ICU4X 分析与布局；包装成 ShapedText API |
| 2D 渲染 | `vello_gpu`／`vello_cpu`，必要时直接 `wgpu` | 先评估现成 Sparse Strips 渲染；CPU 路径用于软件与无头验证 |
| GPU／窗口 | `wgpu` + `winit` | 保持既定跨平台底座，不另造抽象 |
| 几何与颜色 | `kurbo` + `peniko` | 与 Linebender 图形栈共享基础类型，减少重复转换 |
| 无障碍 | `accesskit` + `accesskit_winit` | 复用语义 schema、平台 adapter 与 consumer 测试能力 |
| 节点存储 | `slotmap` | 计数器阶段即可提供 generational key，不先手写 arena 安全性 |
| Web | `web-sys` + `wasm-bindgen` + `wasm-bindgen-futures` | 使用真实 DOM 与标准 wasm 边界 |
| 序列化与 Schema | `serde` + `schemars` + `serde_json` | 场景、检查快照、错误和 CLI 参数共用类型来源 |
| 诊断与观测 | `thiserror` + `anyhow` + `tracing` | 库使用结构化错误，应用与工具使用上下文错误和 trace |
| CLI | `clap` | 命令解析用于 `fino_cli` 和 Cargo 示例运行器，不进入核心 Runtime |
| 测试 | `proptest` + `insta` + `trybuild` + `wasm-bindgen-test` + Criterion | 覆盖属性、快照、宏诊断、wasm 与性能回归 |

版本统一在 Workspace 管理，并锁定 `wgpu`、Vello、Parley 之间经过验证的组合。依赖更新通过独立 PR 完成，不与功能开发混合。

## 包装与直接使用的边界

不为每个依赖都写 wrapper。只有满足以下至少一项时才适配：

- 该库属于可能替换的算法后端，例如 Taffy、Parley、Vello。
- 需要统一平台语义，例如窗口、文本输入与无障碍。
- 需要插入失效追踪、虚拟时钟、调试或确定性控制。
- 需要防止第三方类型进入应用公共 API。

简单且稳定的工具库可以直接使用。包装层只做语义映射与观测，不复制上游全部 API。

## 依赖准入与退出

新增核心依赖前用一个垂直切片验证：

```text
目标平台能否构建
→ 是否支持无头测试
→ 许可证与 MSRV 是否符合基线
→ 是否允许我们的调度与失效模型
→ 是否引入冲突的 async/GPU 版本
→ 计数器与性能场景是否通过
```

优先向上游提交修复。临时 fork 必须记录补丁、上游 PR、维护责任人与退出条件；不能让隐式长期 fork 成为第二套框架。
