[English](../en/quality.md) · [简体中文](quality.md)

# 质量、性能与 CI

## 性能契约与可观测性

“高性能”必须转化为可以测试的契约：

| 场景 | 不允许发生 |
|---|---|
| 修改一个文本 Signal | 全组件重跑、全树 diff、全树布局 |
| 连续批量写入 | 每次写入都提交 DOM 或请求独立帧 |
| 仅改变颜色 | Text shape 或 Layout |
| 仅改变宽度约束 | 对未变化文本重新 Shape |
| 静态 Material uniform 不变 | 永久保持 60/120 FPS 空转 |
| 长列表滚动 | 为全部数据创建 DOM/原生节点 |

DevTools 至少记录：

```text
组件执行次数
Effect 执行次数与原因
后端操作次数
Shape 的节点/段落数量
Layout 的节点数量
Paint 与 Damage 范围
Draw calls / Pipeline switches
CPU frame time / GPU frame time
节点、文本缓存与离屏纹理内存
```

优化必须依据这些计数和真实帧时间，不能只依据文本测量微基准。

### 运行时统计

`fino::metrics::RuntimeStats` 是一致性测试与 CI 产物共用的跨后端结构：

```text
component_executions
effect_executions
backend_set_text
nodes_created
nodes_removed
shaped_texts
layout_nodes
frames_requested
```

`HeadlessHost::runtime_stats()` 与 `WindowContext::runtime_stats()` 把响应式执行计数与后端计数合并。Mock 与 Web 不拥有原生文本或帧调度，因此其 `shaped_texts`、`layout_nodes` 与 `frames_requested` 为零；Native 报告真实的 shaping、布局盒与合并帧请求工作量。`BackendStats` 从规范化操作日志推导 `backend_set_text`、`nodes_created` 与 `nodes_removed`，所以每个后端统计的是同一组操作名。

## 测试体系

### 响应式单元测试

覆盖：

- 依赖建立、更新、删除与重新建立。
- batch、嵌套 batch、Memo 与重复通知。
- Owner 嵌套 dispose 与幂等释放。
- Effect 重入、循环依赖和错误报告。
- 条件依赖变化后，旧依赖不再触发更新。

### 后端一致性测试

同一组操作分别运行在 Mock、Web 与 Native：

```text
创建结构
→ 修改文本
→ 修改属性
→ 插入与移动
→ 触发事件
→ 卸载
```

比较规范化后的逻辑树、事件结果与操作次数，不比较平台特有节点句柄。

计数器示例拥有共享序列与稳定报告。Mock 与 Native 通过 Cargo 输出报告；浏览器驱动为 Web 写入同一份报告：

```bash
cargo run -p fino_counter -- conformance --backend mock --format json
cargo run -p fino_counter -- conformance --backend native-offscreen --format json
```

报告包含 `tree` 与 `final_tree`（`LogicalTreeSnapshot`）、无障碍投影、点击结果、`checkpoints` 与 `final_stats`（`RuntimeStats`）、规范化 `operations`，以及 100 次增量的 `batch` 结果。所有映射使用稳定键顺序，schema 中不出现平台句柄。

### 文本与布局测试

| 类型 | 方法 |
|---|---|
| 单元测试 | 固定字体下的宽度、换行、行数、光标与命中测试 |
| 属性测试 | 任意约束不产生负尺寸、NaN、越界或无限循环 |
| 差分测试 | 对支持的语义与浏览器结果比较，记录首个差异 |
| 多语言语料 | 中文、阿拉伯文、组合字符、emoji、缺字与字体回退 |
| 增量测试 | 局部编辑只使对应文本块与布局子树失效 |

浏览器是对照实现之一，不是所有平台必须逐像素复制的规范。跨端一致性应由框架公共语义定义。

### 渲染与平台测试

- WGSL 在所有目标后端编译验证。
- 关键 Material 使用容差明确的 golden image。
- SurfaceLost、DeviceLost、Suspend、Resume 与 DPR 变化测试。
- Android/iOS 真机验证触摸、软键盘、安全区域与文本输入。
- Web 验证焦点、键盘、文本选择、DOM 顺序与无障碍树。

### 性能回归场景

至少维护以下固定场景：

```text
单文本高频更新
流式长文档追加
一万行 keyed list 更新
大列表滚动
窗口连续缩放
复杂静态页面中的局部动画
```

每个场景同时记录正确性计数与帧时间，防止通过“少做必要工作”获得虚假的性能提升。

## 确定性、CI 与真实设备边界

### 可复现环境

无头运行固定：

```text
viewport / DPR / locale / direction
字体文件与字体版本
随机种子
虚拟时钟与帧号
颜色主题
软件渲染器版本
场景 schema 与快照 schema
```

截图基线允许按渲染器分开管理，并定义少量容差；逻辑断言必须严格，不能用图像容差掩盖文本或结构错误。

### CI 分层

| 层级 | 每次提交运行内容 |
|---|---|
| 快速 | fmt、check、clippy、单元、Mock 场景、宏测试 |
| 核心 | NativeOffscreen、软件快照、WebHeadless、后端一致性 |
| 扩展 | 多浏览器、多 DPR、多语言、属性与性能回归 |
| 平台 | 桌面 Host、Android/iOS 模拟器或真机任务 |
| 发布 | 真实 GPU、IME、触摸、屏幕阅读器、功耗与帧稳定性 |

### 无头验证的边界

无头环境可以覆盖绝大多数编码、调试、结构断言、视觉回归和性能分析，但不能证明：

- 所有真实 GPU／驱动都没有兼容性问题。
- 移动软键盘、系统手势和 IME 体验正确。
- 屏幕阅读器实际播报符合预期。
- 高刷新率、热降频和真实触摸下性能合格。

因此“全 AI、全无头开发”作为日常闭环成立；发布前仍保留可自动执行但需要真实设备与人工体验确认的验收层。
