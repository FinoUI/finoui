[English](../en/bindings.md) · [简体中文](bindings.md)

# 多语言绑定

所有跨语言能力统一经过版本化、句柄化的 API，禁止其他语言直接绑定内部 Rust 类型。

Fino 的后端无关 Runtime、稳定节点句柄、分阶段更新、场景协议和结构化检查都很适合多语言绑定。RSX 宏、`Runtime<B>` 泛型、Rust 闭包、Owner 生命周期和 `reactive_graph` 内部对象不能直接跨语言。

## 绑定外观层

```text
Rust RSX／组件 API
        │
        ▼
Fino Runtime：Signal、Binding、DOM、Style、Material
        │
        ▼
fino_api：版本化、句柄化、无泛型的公共语义
        │
        ├── fino_ffi：稳定 C ABI
        │     ├── C／C++
        │     ├── Python
        │     ├── C#／.NET
        │     ├── Swift
        │     └── Kotlin／JNI
        │
        └── fino_wasm：JavaScript／TypeScript
```

`fino_bindgen` 可以从同一份 IDL／Schema 生成各语言声明，避免手写绑定逐渐分叉。IDL 描述能力和类型，C ABI 与 WASM 只是不同传输方式。

这些外观层包在对应验证阶段开始时再引入。Rust 计数器阶段不提前初始化完整 FFI 或绑定生成器。

## 其他语言的组件如何运行

其他语言不直接使用 Rust `view!`，而是调用同一套底层语义：

```text
创建静态模板
→ 创建 Signal
→ 建立文本／属性／class 绑定
→ 注册事件回调
→ 创建 Owner
→ dispose 时统一释放
```

因此仍然可以保持：

- 组件只执行一次。
- Signal 精确更新目标绑定。
- 无 VDOM、无整树重建。
- Tailwind-like class 编译为 StyleId。
- 同一套无头场景检查不同语言编写的应用。

以后 TypeScript 可以增加 JSX／TSX 作者接口，Python、Kotlin、Swift 也可以有各自惯用 DSL，但都必须降低到相同的模板与绑定 API。

## ABI 边界

### 只暴露句柄，不暴露内部布局

对外使用 `RuntimeHandle`、`NodeHandle`、`SignalHandle`、`OwnerHandle`、`StyleId`。句柄包含 generation，失效后使用必须返回错误，不能暴露 `slotmap`、指针或 Rust 引用。

### C ABI 不传 Rust 类型

边界只允许 POD、UTF-8 字符串、定长 buffer、显式回调和错误码。禁止泛型、`Result<T, E>`、`Vec<T>`、trait object、生命周期和 panic 穿越 FFI。

### 固定线程与回调规则

Runtime 默认限定 UI 线程。其他线程只能投递任务；回调在明确的安全点执行，不能在持有内部可变借用时调用外部语言。监听和回调由 Owner 保活，dispose 后不再触发。

### 热路径不用 JSON

Scenario、Inspect、MCP 继续使用 JSON；实际 UI 更新走本地函数调用或 WASM 调用。批量修改使用 mutation buffer，静态模板一次注册，避免每个节点和属性都产生多次跨语言调用。

### 常见绑定留在核心内完成

`Signal<i32> → Text`、`Signal<bool> → Class`、属性与 uniform 绑定由核心直接执行，不必每次更新都回调外部语言。只有复杂计算和平台业务逻辑才进入语言回调。

## 推荐验证顺序

1. 计数器阶段继续只用 Rust，不提前实现完整 FFI。
2. M1 后增加最小 `fino_ffi`，用一个 C 计数器验证 Signal、文本绑定、click、batch 和 dispose。
3. 再实现 TypeScript／WASM 包，运行同一个计数器和同一组 Scenario。
4. 最后按真实需求增加 Python、C#、Swift、Kotlin；自定义 `GpuSurface` 和复杂 GPU 回调后置。

Rust RSX 是一等作者接口，但不是跨语言 ABI；跨语言 ABI 只描述模板、句柄、绑定、事件与生命周期。
