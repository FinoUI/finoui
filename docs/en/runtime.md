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
    move |context| details_button(context, count.clone()),
);
```

- `ShowContext` tracks every node, effect, and listener created during one mount. A panic in the content builder disposes the child owner, releases tracked nodes, and rethrows without advancing the condition, so a later real transition can retry.
- A first-run condition or content panic during `Show::mount` releases the Show anchors and owner before rethrowing; a later transition panic keeps the anchors and active condition effect for retry.
- `Show::dispose` also records a request while the condition effect or content cleanup owns the state, then releases the range and anchors at the next safe point and disposes the control owner after the borrow. Parent-owner cleanup follows the same pending-request path.
- `Show` content-owner cleanup panics do not prevent node release or terminal anchor cleanup. The first payload is preserved, and repeated disposal remains idempotent.
- The anchors are created once and stay stable. Static siblings and their `NodeId`s are never replaced.
- The condition effect creates a child owner and content only on a real `false → true` transition; equal writes are no-ops.
- On `true → false`, the child owner is disposed first, which stops event dispatch and releases effects and listeners, then the range is released from the parent.
- `remove` remains detach-only for reinsertion probes. `release` is the terminal path used by `Show`: it frees the subtree, retained handles, and listeners so repeated toggles do not grow backend storage.
- Released `NodeId` handles are invalid and are never reused; Native keeps generational safety.
- A window frame request is coalesced per window, not per range. A pending request may survive a range release, but painting the released range cannot touch its nodes. Later writes to removed signals do not issue new requests.
- Nested `Show` owners dispose recursively, and repeated `Show::dispose` or owner disposal is idempotent.
- The parent owner owns the range; dropping the returned handle does not unmount it. Call `Show::dispose` to release it early.
- `RangeAnchor` exposes the start and end anchor handles reused by keyed `For` and whole-range moves.

Backends that predate anchors and release keep compiling: `create_anchor` defaults to an empty text node, `release` defaults to detach-only `remove`, and `HeadlessBackend::listener_count` derives the live count from the operation log. Backends that retain node storage should override `release` to reclaim it. Mock snapshots expose only attached roots, while explicitly detached nodes keep their stored handle for reinsertion.

### Manual keyed For ranges

M1 also provides a manual keyed `For`; the full RSX `For` syntax stays out of scope. `fino::keyed::For` owns one anchor pair plus one range and child owner per key:

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

- The `items` reader runs in a `{name}/reconcile` effect. Keys must be unique and implement `Clone + Eq + Hash`.
- Duplicate keys are rejected atomically before any backend mutation. `For::try_mount` returns `ForError` before creating the For anchors; `For::mount` panics instead. On update, the committed list stays mounted and `For::last_error` reports the rejection; a later valid write retries and clears it.
- `ForContext` tracks every node, effect, and listener of one item. Each item has its own child owner and anchor pair.
- Reconciliation follows the shape of SolidJS `mapArray` (MIT, `solidjs/solid` commit `bc50fe5649ba4fb1ef33f26643dba56c1c002b56`, `packages/solid/src/reactive/array.ts`): common prefix and suffix, a key-to-position map, and one disposer per item. Fino rejects duplicates instead of matching one occurrence. No source code is copied.
- Placement keeps the longest increasing subsequence of retained old positions in place, following the stable-subsequence idea in Vue 3's keyed patch (MIT, `vuejs/core` commit `ff3501429d4970c37194708b055492a56d9e5be5`, `packages/runtime-core/src/renderer.ts`). New and unstable ranges move; stable retained ranges do not issue `insertBefore`.
- Only new keys create ranges and only removed keys release them. Retained keys keep their `NodeId`, DOM identity, text nodes, local signals, effects, and listeners; their content builder never reruns.
- Reordering moves each affected range, anchors included, with `insert_before`. Prefix, suffix, and LIS-stable ranges stay in place; the parent, static siblings, and For anchors are never cleared or rebuilt, and `innerHTML` is never assigned.
- `ForContext` mutations may only target nodes created by the current item. `insert_before`, `set_text`, `set_attribute`, and `listen` reject foreign mounted nodes before mutation, and the context does not expose the shared runtime. Item effects receive a restricted `ForEffectContext` with only item-owned `set_text`, `set_attribute`, and `insert_before`; it has no raw backend borrow.
- Nested `Show` mounts are transactional too. A first-run content or condition panic releases the Show anchors and owner before rethrowing; a later content panic leaves the anchors and active effect in place so a retry can succeed.
- Panic rollback and terminal release first detach every tracked node with `remove`, then release each unique node once. `remove` stays detach-only and `release` stays terminal, so parent-before-child and child-before-parent creation orders cannot double-release tracked descendants.
- `For::dispose` records a terminal request even when reconciliation or item cleanup already borrows the state. The effect completes range and anchor release at the next safe point, then disposes the control owner after releasing the borrow; the outer reconcile never commits active items afterward, and later key updates cannot revive the list. Parent-owner cleanup shares the same pending request instead of silently skipping a busy state.
- Cleanup registration precedes the initial reconcile for For and Show, so disposing the parent owner from the first builder or content mount still terminates anchors, ranges, effects, and listeners.
- Removal cleanup catches each item owner/on_dispose panic, keeps the first payload, continues releasing every removed range, restores surviving and new entries into the committed state, and only then rethrows. Terminal disposal likewise completes all node, anchor, and control cleanup before rethrowing.
- For item dispose, builder rollback, removed entries, and terminal cleanup share one owner-and-node termination path: owner cleanup panics are captured, nodes are detached and released exactly once, and an earlier builder/user payload always wins over cleanup or release panics. Show mount rollback and active-range disposal use the same rule.
- `ForContext::on_dispose` is deferred until item construction ends, so listener and effect cleanup runs before user on_dispose callbacks even when the callback was registered earlier.
- Different keys must not return the same external node. Conflicts are rejected atomically with `UI_FOR_SHARED_NODE`, leaving retained owners, NodeIds, and DOM unchanged; rejected staged entries release only their unique resources. `ForError::index` is the rejected key's position in the requested new list for both duplicate-key and shared-node errors.
- Item ownership includes live nodes created by nested keyed Shows. Nested-range release removes those records, so a released node cannot remain a permanent ownership entry. Shared-node rejection still skips nodes owned by retained entries.
- Ownership checks use a stable backend node identity when available; pure reorder skips ownership collection entirely, and a staged update uses indexed lookup bounded by committed plus staged nodes. Backends without identity fall back to `Eq` comparisons.
- Item owners and item/list anchors are acquired inside guarded transactions. A failed anchor create or insert disposes the owner and releases partial anchors while preserving the backend panic; initial list/Show anchor failures do the same for the control owner. Placement uses a retained key-to-old-position map rather than scanning by key.
- Placement moves are rollback-safe. A backend panic restores the old middle order, entries, and committed keys before rethrowing; if rollback also panics, the list enters terminal cleanup and keeps the first backend payload.
- `ForContext::show` passes a scoped `ForShowContext` that only mutates nodes created by that nested range and exposes no raw runtime. Standalone `ShowContext::runtime()` remains public for downstream Show content.
- `RangeAnchor::create` releases partial anchors if creation or insertion panics. `RangeAnchor::move_before` treats `start` as a no-op and rejects `end` or any top-level range node as the reference before the first backend operation.
- Every new item is fully built before existing ranges change. A builder panic disposes that item owner and releases its tracked nodes, effects, and listeners before rethrowing; already-built items are released and the previous list stays committed.
- Removing a key disposes the item owner first, then releases its range. Writing a removed item's signal no longer touches the backend or requests a frame.
- Item-local signal updates through item effects do not run the reconciliation effect.
- `For::dispose` releases every item and the list anchors; repeated disposal and parent-owner disposal are idempotent.
- Mock, real Web DOM, and Native reuse the same scenario. The Web probe checks real node identity and `insertBefore` moves; Native checks arena release and display lists after disposal.

`RangeAnchor::move_before` is the shared operation for a complete range: start anchor, external nodes, end anchor, all before one reference node.

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

This is a semantic interface, not an implementation of the full DOM standard. In the first version, `Tag`, `Attribute`, and `StyleProperty` are closed enums. String extensions are reserved for explicitly permitted custom attributes. `node_identity` is an optional stable backend identity for O(1) ownership indexes; its default `None` keeps legacy backends on `Eq` fallback.

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
