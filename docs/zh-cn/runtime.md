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

`view!` 宏是后续作者接口，不是验证响应式模型的前置条件。计数器核心先用底层构建 API 完成，避免同时调试宏、运行时与后端。

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

Web 不额外维护一棵镜像 DOM。原生端由 `fino::native_dom` 保存：

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
committed_bounds(node)        // Reads a committed snapshot without computation.
                              // 读取已提交快照，不触发计算。
request_current_bounds(node) // Explicit request completed after a layout scheduling point.
                              // 显式请求，在布局调度点后完成。
measure_text(shaped, constraints) // Pure text computation, independent of DOM mounting.
                                  // 纯文本计算，与 DOM 挂载无关。
```

布局引擎是原生几何的事实来源，DOM 只保存布局结果引用。

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
