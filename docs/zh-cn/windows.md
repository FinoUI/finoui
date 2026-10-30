[English](../en/windows.md) · [简体中文](windows.md)

# 多窗口架构

桌面多窗口是一等能力。原生计数器 PR 就引入 `WindowId／WindowContext`，即使当时只创建一个窗口。

现有响应式、DOM 和渲染设计无需推翻；Host 不能继续隐含“一个应用只有一个 Window、Surface、焦点和 DOM 根节点”。

## 运行时结构

```text
Application
├── AppOwner
│   └── 跨窗口共享的 Signal、业务状态与服务
├── WindowManager
│   ├── WindowContext A
│   │   ├── WindowOwner
│   │   ├── 独立 DOM 根节点与 NodeId 空间
│   │   ├── Focus／IME／Accessibility
│   │   ├── Layout／Paint／Damage
│   │   ├── Surface／帧调度
│   │   └── 独立输入路由
│   └── WindowContext B
│       └── ...
└── SharedRenderContext
    ├── wgpu Device／Queue
    ├── Pipeline 与 Glyph 缓存
    └── 可共享的图片、字体和材质资源
```

每个窗口有自己的 Surface、DOM、焦点、无障碍和帧请求；GPU Device、字体、文本 shaping 等可共享资源不重复创建。

## 跨窗口 Signal

```text
AppOwner::count
├── Window A::TextBinding
└── Window B::TextBinding
```

在任意窗口执行 `count.update()`：

```text
一次响应式事务
→ 更新 A、B 中对应绑定
→ 分别标记 A、B 的 Paint／Layout
→ 每个受影响窗口各请求一帧
→ 不相关窗口不重绘
```

关闭窗口时只释放对应的 `WindowOwner`、DOM、监听和 Surface；`AppOwner` 中的 Signal 与其他窗口继续存在。

## 生命周期区别

- `SurfaceLost`：重建 GPU Surface，保留窗口 UI 和状态。
- `hide_window`：停止不必要绘制，保留窗口状态。
- `close_window`：释放该窗口的 Owner 和 UI 资源。
- `close_application`：最后释放 AppOwner 与共享渲染资源。

## API 与平台边界

概念接口：

```rust
let count = app.signal(0);

let main = app.open_window(WindowDesc::default(), {
    let count = count.clone();
    move |cx| counter(cx, count)
});

let inspector = app.open_window(WindowDesc::default(), {
    move |cx| counter(cx, count)
});
```

所有输入、布局读取和绘制上下文都携带窗口身份：

```rust
WindowId
WindowHandle        // External, generational handle.
                    // 对外、带 generation。
WindowEvent
WindowConstraints
committed_bounds(window, node)
request_current_bounds(window, node)
```

不要用全局可变的 `current_window()` 决定事件归属。

| 平台 | 多窗口策略 |
|---|---|
| Windows／macOS／Linux | 真正的多个原生窗口，一等支持 |
| Web | 单个 Document 内支持多视图；额外浏览器窗口是独立运行时，通过消息或共享状态同步 |
| Android | 以 Activity／系统分屏等 Host 能力为准，不承诺任意桌面式窗口 |
| iOS | 以 Scene 生命周期为准，不把 Scene 伪装成普通桌面窗口 |

普通菜单、Tooltip 和 Popover 默认仍是窗口内 Layer，只有独立面板、检查器、可分离工具窗等才创建真正的原生窗口。

## 无头验收场景

后续多窗口垂直切片至少证明：

1. 两个窗口共享一个计数器，在任意窗口点击，两边都精确更新。
2. 关闭一个窗口后，其 Effect、事件和帧请求全部释放，另一个窗口不受影响。
3. 单独 resize、DPR 变化或 SurfaceLost，不触碰其他窗口的布局与状态。
4. 焦点、IME、无障碍和输入事件始终路由到正确窗口。
5. 静态窗口不请求帧；局部更新不重绘无关窗口。

实施上不需要现在完成多窗口，但原生 Host 初版就采用 `Application → WindowManager → WindowContext`，避免先写全局单例、之后再做伤筋动骨的改造。
