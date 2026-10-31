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
fn counter() -> View {
    let count = signal_named("counter/count", 0);
    view! {
        <button
            fino:name="counter"
            aria-label="Increment counter"
            on:click={move || count.update(|value| *value += 1)}
        >
            "Count: " {move || count.get()}
        </button>
    }
}
```

| 作者写法 | 下沉结果 |
|---|---|
| 一个静态 `<button>` 与带引号文本 | `Template` |
| `{move \|\| value}` 子节点 | `TextBinding` |
| `on:click={move \|\| action()}` | `EventBinding` |
| 动态 `aria-label={move \|\| value}` | `AttributeBinding` |
| 静态 `aria-label="..."` | 模板属性 |

根元素上的可选 `fino:name` 是编译期元数据，不会写入 DOM。它把计数器的文本 Effect 命名为 `counter/text`；普通视图仍保留默认诊断名 `view/text/{node}`。

`View::mount` 安装组件 Owner，只调用一次构建闭包，挂载静态模板，然后把事件与 Effect 绑定到已有节点。闭包内的 `signal()` 通过该 Owner 解析。被多个生成闭包捕获的 Signal 会分别克隆进各闭包，因此组件不会重新执行，点击只更新目标绑定。

`view!` 直接下沉为 `View`，调用点不需要 `runtime` 或 `owner`。Host 使用 `fino::view::mount_component`（构造前工作可用 `with_owner`），使 `signal()` 能取得组件 Owner。不支持的标签、事件、属性以及非闭包动态值会在对应源码 Span 上产生编译错误；`trybuild` 固定这些诊断。完整 Tailwind-like class 子集、完整 RSX `Show`／`For` 语法与热重载仍不在范围内。

### 手工 Show 范围

M1 通过手工 API 增加结构响应式；完整 RSX `Show` 语法仍不在范围内。`fino::show::Show` 挂载一对锚点，并在两者之间持有一个条件范围：

```rust
let details = Show::mount(
    runtime,
    owner,
    &button,
    Some(&tail),
    "show/details",
    move || visible.get(),
    move |context| details_button(context, count.clone()),
);
```

- `ShowContext` 跟踪一次挂载中创建的每个节点、Effect 与监听。内容构建器 panic 时会释放子 Owner、回收已跟踪节点并重新抛出，且不推进条件，后续真实切换可以重试。
- 锚点只创建一次并保持稳定；静态兄弟及其 `NodeId` 绝不替换。
- 条件 Effect 只在真正的 `false → true` 切换时创建子 Owner 与内容；写入相同值不执行操作。
- `true → false` 时先释放子 Owner，停止事件分发并解绑 Effect 与监听，再从父节点释放范围。
- `remove` 仍只做分离，供重插探针使用。`release` 是 `Show` 使用的终止路径：释放子树、保留句柄与监听，因此反复切换不会增长后端存储。
- 已释放的 `NodeId` 句柄失效且不会复用；Native 继续保证 generational 安全。
- 窗口帧请求按窗口合并，而不是按范围关联。范围释放后仍可能保留一个待处理窗口请求，但绘制已释放范围不会触达其节点；之后写入被移除 Signal 也不会产生新请求。
- 嵌套 `Show` Owner 递归释放，重复调用 `Show::dispose` 或 Owner 释放均幂等。
- 动态范围由父 Owner 持有；丢弃返回句柄不会卸载它。调用 `Show::dispose` 可提前释放。
- `RangeAnchor` 暴露起止锚点句柄，供 keyed `For` 与整段移动复用。

早于锚点与释放能力的后端仍可编译：`create_anchor` 默认创建空文本节点，`release` 默认回退到只分离的 `remove`，`HeadlessBackend::listener_count` 从操作日志推导存活数。会保留节点存储的后端应覆盖 `release` 以真正回收。Mock 快照只暴露已挂载根；显式分离的节点保留存储句柄以便重插。

### 手工 keyed For 范围

M1 还提供手工 keyed `For`；完整 RSX `For` 语法仍不在范围内。`fino::keyed::For` 持有整体锚点对，并为每个 key 持有一个范围与子 Owner：

```rust
let rows = For::mount(
    runtime,
    owner,
    &list,
    Some(&tail),
    "for/rows",
    move || keys.get(),
    move |context, key| row(context, key),
);
```

- `items` 读取器在 `{name}/reconcile` Effect 中执行。key 必须唯一，并实现 `Clone + Eq + Hash`。
- 重复 key 会在任何后端变更前被原子拒绝。`For::try_mount` 在创建 For 锚点前返回 `ForError`；`For::mount` 则 panic。更新时已提交列表保持不变，`For::last_error` 报告拒绝；之后的合法写入会重试并清除错误。
- `ForContext` 跟踪一个条目的每个节点、Effect 与监听。每个条目拥有自己的子 Owner 与锚点对。
- 协调结构参考 SolidJS `mapArray`（MIT，`solidjs/solid` 提交 `bc50fe5649ba4fb1ef33f26643dba56c1c002b56`，`packages/solid/src/reactive/array.ts`）：公共前缀与后缀、key 到位置映射，以及每项一个 disposer。Fino 拒绝重复 key，而不是匹配其中一个出现位置。未复制其源码。
- 只有新增 key 创建范围，只有删除 key 释放范围。保留的 key 保留 `NodeId`、DOM 身份、文本节点、局部 Signal、Effect 与监听；内容构建器绝不重跑。
- 重排使用 `insert_before` 移动每个保留范围（含锚点）。前缀与后缀范围原地不动；父节点、静态兄弟与 For 锚点绝不清空或重建，也绝不写入 `innerHTML`。
- 每个新条目都在改动既有范围前完整构建。构建器 panic 时先释放该条目 Owner 及其跟踪的节点、Effect 与监听，再继续展开；已构建条目会被释放，上一次列表保持已提交。
- 删除 key 时先释放条目 Owner，再释放其范围。写入已移除条目的 Signal 不再触达后端或请求帧。
- 条目内通过条目 Effect 完成的 Signal 更新不会运行协调 Effect。
- `For::dispose` 释放全部条目与列表锚点；重复释放与父 Owner 释放均幂等。
- Mock、真实 Web DOM 与 Native 复用同一场景。Web 探针检查真实节点身份与 `insertBefore` 移动；Native 检查 arena 释放与释放后的绘制列表。

`RangeAnchor::move_before` 是整段移动的共享操作：起始锚点、外部节点、结束锚点，全部移动到同一个参考节点之前。

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
