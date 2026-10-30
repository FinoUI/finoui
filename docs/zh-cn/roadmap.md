[English](../en/roadmap.md) · [简体中文](roadmap.md)

# 演进路线与风险

## 计数器之后的演进路线

| 阶段 | 新增能力 | 必须证明的事项 |
|---|---|---|
| M1 结构响应式 | Show、For、锚点、keyed move | 不重排重建、dispose 完整 |
| M2 基础应用 | Style IR、受控 `class` 子集、Focus、输入框、滚动 | 真实表单与 IME 可用 |
| M3 性能核心 | ShapedText 缓存、增量布局、虚拟列表 | 长文档与大列表满足性能契约 |
| M4 原生视觉 | Display List、Damage、Layer、Material | 局部更新与 GPU 合批有效 |
| M5 平台扩展 | Android、iOS、Web SharedLayout 评估 | 生命周期与平台输入不破坏核心 |
| M6 开发生态 | RSX 完整化、Devtools、热重载、控件库 | 真实应用驱动 API 稳定 |

每个阶段都继续运行计数器。计数器是框架最小一致性测试，不应在功能增加后被删除。

## 主要风险与控制方式

| 风险 | 早期信号 | 控制方式 |
|---|---|---|
| 抽象被 DOM 带偏 | Native API 出现浏览器专有概念 | Mock 与 Native 从早期参与一致性测试 |
| 抽象被 wgpu 带偏 | Web 组件必须理解 GPU 资源 | Shader 能力独立，普通 DOM 不依赖 Renderer |
| 文本范围失控 | 首版承诺全部 Unicode/CSS 行为 | 明确支持子集，复用成熟文本栈，建立语料 |
| 布局频繁全树失效 | 单次文本更新布局节点数等于全树 | 记录失效原因与布局计数，补充缓存 |
| Web 与原生行为分叉 | 同一 RSX 需要大量条件代码 | 固定公共语义，平台扩展显式命名 |
| Shader 破坏性能 | 每个控件独立 pipeline/离屏纹理 | Material 实例化、Effect 显式边界、纹理复用 |
| AI 代码量超过验证能力 | 大量未测试模块同时出现 | 小 PR、测试先行、持续运行垂直切片 |
| 过早公开 API | 为兼容示例阻碍核心重构 | 真实应用验证前保持实验状态 |
