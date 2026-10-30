[English](runtime.md) · [简体中文](../zh-cn/runtime.md)

# Runtime and DOM API

## Reactive core

### Minimal semantics

| Capability | Constraint |
|---|---|
| `signal` | Creates readable and writable state; reads establish dependencies by default |
| `get_untracked` | Reads explicitly without establishing a dependency |
| `memo` | Caches a derived value and notifies downstream consumers only when its result changes |
| `effect` | Establishes side-effect dependencies with defined execution and disposal timing |
| `batch` | Coalesces notifications within one transaction to avoid intermediate states and duplicate updates |
| `Owner` | Owns the lifecycle of signals, effects, listeners, and child components |
| `dispose` | Releases subscriptions, events, timers, and backend resources; safe to call more than once |

### Update order

The default transaction flow is:

```text
Signal write
→ Mark dependent effects
→ Finish the current batch
→ Schedule effects by topology and creation order
→ Apply backend node updates
→ Mark layout or paint requirements
→ Request the next rendering frame
```

Requirements:

- An effect runs at most once in a batch.
- Writing from an effect must not directly cause unbounded synchronous re-entry; cycles must be detectable and reported.
- After a component unmounts, its effects cannot update the backend.
- Reactive flushing is separate from GPU frame scheduling. No frame is requested without a visual change.

### Component semantics

A component function creates structure and bindings. It is not a per-frame render function:

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

The target API conceptually expands to:

```text
Static template: button + static text + dynamic text slot + click slot
Dynamic binding: count → dynamic text slot
Component execution: once at mount
Click update: does not call counter() again
```

The `view!` macro is a later authoring interface, not a prerequisite for validating the reactive model. The counter core starts with low-level construction APIs so that macro, runtime, and backend problems are not debugged at the same time.

## Unified DOM API

### Backend operations

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

This is a semantic interface, not an implementation of the full DOM standard. In the first version, `Tag`, `Attribute`, and `StyleProperty` are closed enums. String extensions are reserved for explicitly permitted custom attributes.

### Node storage

```text
Web: DomBackend::Node = browser node handle
Native: DomBackend::Node = NodeId { index, generation }
```

Web does not maintain an additional mirror DOM. On native platforms, `fino::native_dom` stores:

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

The first version uses `parent + children Vec` to simplify implementation. Storage can change if profiling shows sibling operations are a hotspot. Generational node IDs prevent stale handles from referring to newly allocated nodes after disposal.

### Structure and ownership

Backend `remove` only removes structure. The runtime's owner releases reactive resources. Unmount order is fixed:

```text
Stop event dispatch
→ Dispose child owners
→ Unbind effects and listeners
→ Release backend resources
→ Remove from the parent
```

`Show` uses anchors for dynamic ranges. `For` uses keys and node-range moves for existing nodes; keyed updates must not clear the parent and rebuild its children.

## Scheduling and geometry

### Geometry read contract

The core DOM API has no `width()`, `offset_height()`, or `get_bounding_client_rect()` operation that implicitly triggers layout.

Geometry access is divided into:

```rust
committed_bounds(node)        // Reads a committed snapshot without computation.
                              // 读取已提交快照，不触发计算。
request_current_bounds(node) // Explicit request completed after a layout scheduling point.
                              // 显式请求，在布局调度点后完成。
measure_text(shaped, constraints) // Pure text computation, independent of DOM mounting.
                                  // 纯文本计算，与 DOM 挂载无关。
```

The layout engine is the native source of truth for geometry. The DOM only retains references to layout results.

### Frame pipeline

```text
Platform events
→ Signal transactions
→ Reactive flush
→ DOM mutations
→ Style invalidation
→ Text shape (affected text only)
→ Layout (affected subtrees only)
→ Paint / Damage
→ GPU render or DOM commit
→ Accessibility commit
→ Committed snapshot
```

Business code must not repeatedly alternate style writes with current-geometry reads within one frame. When current geometry is required, code must enter the measurement phase explicitly, and the operation must be observable in DevTools.
