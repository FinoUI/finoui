[English](../en/runtime.md) · [简体中文](runtime.md)

# 运行时与 DOM API

## 响应式核心

### 最小语义

| 能力 | 约束 |
|---|---|
| `signal` | 创建可读、可写状态；读取时默认建立依赖 |
| `get_untracked` | 显式读取但不建立依赖 |
| `memo` | 缓存派生值，仅在结果变化时通知下游 |
| `effect` | 建立副作用依赖；拥有明确执行与释放时机 |
| `batch` | 同一事务内合并通知，避免中间状态与重复更新 |
| `Owner` | 管理 Signal、Effect、监听与子组件生命周期 |
| `dispose` | 释放订阅、事件、定时器及后端资源，可幂等执行 |

### 更新顺序

默认事务流程：

```text
Signal write
→ 标记依赖 Effect
→ 当前 batch 结束
→ 按拓扑与创建顺序调度 Effect
→ 执行后端节点更新
→ 标记布局或绘制需求
→ 请求下一渲染帧
```

要求：

- 同一 Effect 在一个批次中最多执行一次。
- 写 Effect 不直接触发无限同步重入；循环必须可检测并报告。
- 组件卸载后，不再允许其 Effect 更新后端。
- 响应式刷新与 GPU 帧调度分离；没有视觉变化时不请求帧。

### 组件语义

组件函数负责创建结构与绑定，不承担逐帧渲染：

```rust
fn counter() -> View {
    let count = signal(0);

    view! {
        <button on:click={move || count.update(|value| *value += 1)}>
            "Count: " {move || count.get()}
        </button>
    }
}
```

上述目标 API 应在概念上展开为：

```text
静态模板：button + 静态文本 + 动态文本槽 + click 槽
动态绑定：count → 动态文本槽
组件执行：仅挂载时一次
点击更新：不重新调用 counter()
```

### 计数器阶段的 `view!` 宏

计数器现在使用刻意收窄的 Rust RSX 子集。`rstml` 以可恢复方式解析 Token 并保留源码 Span，`syn` 校验支持的节点，`quote` 输出可读的模板与绑定构建代码。

```rust
fn counter<B: DomBackend + 'static>() -> View<B> {
    View::new(|runtime, owner| {
        let count = signal(0);
        view! {
            <button
                aria-label="Increment counter"
                on:click={move || count.update(|value| *value += 1)}
            >
                "Count: " {move || count.get()}
            </button>
        }
    })
}
```

| 作者写法 | 下沉结果 |
|---|---|
| 一个静态 `<button>` 与带引号文本 | `Template` |
| `{move || value}` 子节点 | `TextBinding` |
| `on:click={move || action()}` | `EventBinding` |
| 动态 `aria-label={move || value}` | `AttributeBinding` |
| 静态 `aria-label="..."` | 模板属性 |

`View::mount` 安装组件 Owner，只调用一次构建闭包，挂载静态模板，然后把事件与 Effect 绑定到已有节点。闭包内的 `signal()` 通过该 Owner 解析。被多个生成闭包捕获的 Signal 会分别克隆进各闭包，因此组件不会重新执行，点击只更新目标绑定。

宏要求作用域内存在 `runtime` 与 `owner`。不支持的标签、事件、属性以及非闭包动态值会在对应源码 Span 上产生编译错误；`trybuild` 固定这些诊断。完整 Tailwind-like class 子集、`Show`、`For`、完整 RSX 与热重载仍不在范围内。

## 统一 DOM API

### 后端操作

```rust
pub trait DomBackend {
    type Node: Clone + Eq;
    type Listener;

    fn create_element(&mut self, tag: Tag) -> Self::Node;
    fn create_text(&mut self, value: &str) -> Self::Node;
    fn create_anchor(&mut self) -> Self::Node;

    fn insert_before(
        &mut self,
        parent: &Self::Node,
        child: &Self::Node,
        anchor: Option<&Self::Node>,
    );
    fn remove(&mut self, node: &Self::Node);

    fn set_text(&mut self, node: &Self::Node, value: &str);
    fn set_attribute(
        &mut self,
        node: &Self::Node,
        name: Attribute,
        value: Option<&str>,
    );
    fn set_style(
        &mut self,
        node: &Self::Node,
        property: StyleProperty,
    );
    fn listen(
        &mut self,
        node: &Self::Node,
        event: EventKind,
        listener: EventListener,
    ) -> Self::Listener;
    fn unlisten(&mut self, listener: Self::Listener);
}
```

这是语义接口而非完整 DOM 标准。首版 `Tag`、`Attribute` 与 `StyleProperty` 应是封闭枚举；字符串扩展只用于明确允许的自定义属性。

### 两种节点存储

```text
Web：DomBackend::Node = 浏览器节点句柄
Native：DomBackend::Node = NodeId { index, generation }
```

Web 不额外维护一棵镜像 DOM。原生端每个 `WindowContext` 拥有独立 DOM 根节点与 `NodeId` 空间，由 `fino::native_dom` 保存：

```rust
pub struct NodeData {
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub kind: NodeKind,
    pub style: StyleId,
    pub semantics: Semantics,
    pub backend: BackendData,
}
```

推荐首版使用 `parent + children Vec` 以简化实现；当 profiling 证明 sibling 操作成为热点，再切换存储。NodeId 带 generation，避免节点释放后旧句柄误指向新节点。

### 结构与所有权

后端 `remove` 只负责结构移除；响应式资源释放由 Runtime 的 Owner 负责。卸载顺序固定为：

```text
停止事件分发
→ dispose 子 Owner
→ 解绑 Effect 与监听
→ 释放后端资源
→ 从父节点移除
```

`Show` 使用锚点表达动态范围；`For` 使用 key 与节点范围移动已有节点，不能通过清空父节点再重建实现 keyed update。

## 调度与几何读取

### 几何读取契约

核心 DOM API 不提供会隐式触发布局的 `width()`、`offset_height()` 或 `get_bounding_client_rect()`。

几何访问分为：

```rust
committed_bounds(window, node) // Reads a committed snapshot without computation.
                               // 读取已提交快照，不触发计算。
request_current_bounds(window, node) // Explicit request completed after a layout scheduling point.
                                     // 显式请求，在布局调度点后完成。
measure_text(shaped, constraints) // Pure text computation, independent of DOM mounting.
                                  // 纯文本计算，与 DOM 挂载无关。
```

每次几何请求都携带 `WindowId`，不读取隐含当前窗口。布局引擎是原生几何的事实来源，DOM 只保存布局结果引用。

### 帧管线

```text
Platform events
→ Signal transactions
→ Reactive flush
→ DOM mutations
→ Style invalidation
→ Text shape（仅受影响文本）
→ Layout（仅受影响子树）
→ Paint / Damage
→ GPU render 或 DOM commit
→ Accessibility commit
→ Committed snapshot
```

同一帧中业务代码不能反复交叉写样式和读取当前几何。确需当前几何时，必须显式进入测量阶段，并在 DevTools 中可观测。
