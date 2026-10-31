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

### Counter-stage `view!` macro

The counter now uses a deliberately small Rust RSX subset. `rstml` parses the tokens with recovery and source spans, `syn` validates the supported nodes, and `quote` emits readable template and binding construction.

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

| Authoring form | Lowering |
|---|---|
| One static `<button>` with quoted text | `Template` |
| `{move \|\| value}` child | `TextBinding` |
| `on:click={move \|\| action()}` | `EventBinding` |
| Dynamic `aria-label={move \|\| value}` | `AttributeBinding` |
| Static `aria-label="..."` | Template attribute |

The optional root `fino:name` attribute is compile-time metadata rather than a DOM attribute. It names the counter's text effect `counter/text`; ordinary views keep the default `view/text/{node}` diagnostic name.

`View::mount` installs the component owner, calls the build closure exactly once, mounts the static template, and then binds events and effects to the existing nodes. `signal()` inside the closure resolves against that owner. A signal captured by multiple generated closures is cloned into each closure so the component is not re-executed and clicks update only the target binding.

`view!` lowers directly to a `View` and does not require `runtime` or `owner` at the call site. Hosts use `fino::view::mount_component` (or `with_owner` for pre-construction work) so `signal()` sees the component owner. Unsupported tags, events, attributes, and non-closure dynamic values produce a compile error at the offending source span; `trybuild` pins those diagnostics. The complete Tailwind-like class subset, full RSX `Show`/`For` syntax, and hot reload remain out of scope.

### Manual Show ranges

M1 adds structural reactivity through a manual API; the full RSX `Show` syntax remains out of scope. `fino::show::Show` mounts an anchor pair and owns one conditional range between them:

```rust
let details = Show::mount(
    runtime,
    owner,
    &button,
    Some(&tail),
    "show/details",
    move || visible.get(),
    move |runtime, child| details_button(runtime, child, count.clone()),
);
owner.keep_alive(details);
```

- The anchors are created once and stay stable. Static siblings and their `NodeId`s are never replaced.
- The condition effect creates a child owner and content only on a real `false → true` transition; equal writes are no-ops.
- On `true → false`, the child owner is disposed first, which stops event dispatch and releases effects and listeners, then the content nodes are detached from the parent.
- Removed scopes keep no live effects, listeners, or frame requests. Later writes to their signals do not reach the backend.
- Nested `Show` owners dispose recursively, and repeated `Show::dispose` or owner disposal is idempotent.
- The parent owner owns the range; dropping the returned handle does not unmount it. Call `Show::dispose` to release it early.
- `RangeAnchor` exposes the start and end anchor handles for future `For` and keyed range work.

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

Web does not maintain an additional mirror DOM. Each native `WindowContext` owns an independent DOM root and `NodeId` space, and `fino::native_dom` stores:

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
committed_bounds(window, node) // Reads a committed snapshot without computation.
                               // 读取已提交快照，不触发计算。
request_current_bounds(window, node) // Explicit request completed after a layout scheduling point.
                                     // 显式请求，在布局调度点后完成。
measure_text(shaped, constraints) // Pure text computation, independent of DOM mounting.
                                  // 纯文本计算，与 DOM 挂载无关。
```

Every geometry request carries a `WindowId`; no implicit current window is consulted. The layout engine is the native source of truth for geometry. The DOM only retains references to layout results.

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
