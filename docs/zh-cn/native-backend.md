[English](../en/native-backend.md) · [简体中文](native-backend.md)

# 原生计数器后端

首个原生垂直切片通过真实 winit 窗口与 wgpu Surface 驱动共享计数器。它建立的窗口模型会被后续多窗口能力扩展，而不是被推倒重写。

## 结构

```text
Application
├── AppOwner：counter/count 等跨窗口 Signal
├── WindowManager
│   └── WindowContext
│       ├── WindowId
│       ├── NativeDom：带 generational NodeId 的 slotmap arena
│       ├── 窗口 Owner、焦点、IME、无障碍
│       ├── 布局、显示列表、Damage
│       └── Surface、帧请求、输入路由
└── SharedRenderContext
    ├── wgpu Device／Queue／Pipeline 缓存
    ├── 已光栅化 Glyph atlas
    └── 基于 Parley + Fontique 的 TextEngine
```

`fino::text` 持有公开的 `TextSpec → ShapedText → TextLayout` 模型。Shaping 按文本、字体、区域与空白设置缓存。仅宽度约束变化时克隆 shaping 结果并重跑行布局，不会重新 shaping。

`fino::native::dom::NativeDom` 同时实现 `DomBackend` 与 `HeadlessBackend`。每次变更都携带所属 `WindowId`。布局、绘制、命中测试与已提交几何读取也要求 `WindowId`，不存在 `current_window()`。

## 更新路径

```text
PointerInput { window, position }
→ WindowManager::route_input
→ WindowContext::dispatch_input
→ NativeDom::hit_test(window, position)
→ count.update(+1)
→ counter/text Effect 执行一次
→ NativeDom::set_text 只更新一个动态文本节点
→ 该节点的 ShapedText 与 TextLayout 失效
→ 只重新 shape 该文本
→ 受影响 WindowId 的布局与绘制
→ 一个合并后的帧请求
```

空白或仅无障碍写入不请求帧。写入相同文本是空操作。一帧呈现前重复请求会合并为一次。

`NativeDom::set_button_color` 是纯绘制变化：它请求绘制，但不触发 shaping 或布局。`WindowContext::runtime_stats()` 暴露单个窗口的共享 `RuntimeStats`，包含真实的 `shaped_texts`、`layout_nodes` 与合并后的 `frames_requested`。仅宽度变化会为可见文本重跑行布局，但不会重新 shaping 未变化内容。

## 生命周期

- `resize` 更新逻辑视口、使布局失效，并保留 DOM 与 count。
- `remove` 只分离选中节点。后代、generational 句柄与监听保持有效，因此同一子树可以重新插入。
- `SurfaceLost` 只释放 GPU 状态。DOM、已 shaping 文本、布局、焦点与信号都保留。
- 重建 Surface 时从共享 adapter 与 device 重新配置。
- 关闭窗口只释放对应 `WindowContext`、窗口 Owner、监听与 Surface。

## NativeOffscreen

`fino::native::offscreen_backend(viewport, dpr)` 创建同一条原生 DOM、文本、布局与显示列表路径，但不创建窗口、Surface 或显示服务器依赖。它支持 `dom`、`a11y`、`reactive`、`trace`、`layout` 与 `display_list` 快照。

示例在两个后端运行同一场景文件：

```bash
cargo run -p fino_counter -- scenario run examples/counter/scenarios/counter.json --backend mock --format json
cargo run -p fino_counter -- scenario run examples/counter/scenarios/counter.json --backend native-offscreen --format json
```

## 真实窗口

```bash
cargo run -p fino_counter -- native
cargo run -p fino_counter -- native --smoke
```

`--smoke` 通过同一条带窗口身份的输入路径点击两次后退出，并报告 `surface_presented`。显示器休眠或无头环境可能让 Surface 处于 occluded；此时运行仍验证 count、DOM、命中和帧请求，但会明确报告没有帧被呈现，不会把逻辑结果伪装成视觉通过。macOS 的 smoke 运行器会在首次 `about_to_wait` 再尝试一次前台激活。若 `surface_presented` 为 false，先把应用切到前台再重跑。若仍未呈现，继续检查 `Occluded`／`Timeout` 和渲染日志，不能直接归因于显示器休眠或渲染故障。

原生后端随仓库分发 SIL Open Font License 1.1 的 Roboto 可变字体，见 `crates/fino/assets/fonts/OFL.txt`。

## 边界

本切片只实现一个按钮、一种字体、一个窗口与最小 wgpu Pipeline。它不包含多窗口 UI、FFI、MCP、完整字体或 Unicode 覆盖、完整样式系统或自定义 WGSL Material／Effect。
