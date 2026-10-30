[English](bindings.md) · [简体中文](../zh-cn/bindings.md)

# Multi-language bindings

All cross-language capabilities pass through a versioned, handle-based API. Other languages must never bind directly to internal Rust types.

Fino's backend-independent runtime, stable node handles, phased updates, scenario protocol, and structured inspection support multi-language bindings. RSX macros, the `Runtime<B>` generic, Rust closures, owner lifetimes, and `reactive_graph` internals do not cross the language boundary.

## Binding facade

```text
Rust RSX / component API
        │
        ▼
Fino Runtime: Signal, Binding, DOM, Style, Material
        │
        ▼
fino_api: versioned, handle-based, non-generic public semantics
        │
        ├── fino_ffi: stable C ABI
        │     ├── C / C++
        │     ├── Python
        │     ├── C# / .NET
        │     ├── Swift
        │     └── Kotlin / JNI
        │
        └── fino_wasm: JavaScript / TypeScript
```

`fino_bindgen` can generate declarations for each language from one IDL/schema, preventing hand-written bindings from diverging. The IDL describes capabilities and types; the C ABI and WASM are different transports.

These facade packages are introduced when the corresponding validation stage starts. The Rust counter does not initialize the full FFI layer or binding generator in advance.

## Components in other languages

Other languages do not call Rust `view!` directly. They use the same underlying semantics:

```text
Create static templates
→ Create signals
→ Establish text / attribute / class bindings
→ Register event callbacks
→ Create owners
→ Release everything through dispose
```

The model retains:

- Components that execute once.
- Precise signal updates to target bindings.
- No VDOM or whole-tree reconstruction.
- Tailwind-like classes compiled to StyleId.
- The same headless scenarios for applications written in different languages.

TypeScript may later add a JSX/TSX authoring interface. Python, Kotlin, and Swift may provide idiomatic DSLs, but all of them lower to the same template and binding API.

## ABI boundaries

### Expose handles, never internal layout

The public surface uses `RuntimeHandle`, `NodeHandle`, `SignalHandle`, `OwnerHandle`, and `StyleId`. Handles contain generations. Using a stale handle returns an error. The API exposes no `slotmap`, pointer, or Rust reference.

### No Rust types across the C ABI

The boundary permits only POD values, UTF-8 strings, fixed-length buffers, explicit callbacks, and error codes. Generics, `Result<T, E>`, `Vec<T>`, trait objects, lifetimes, and panics must not cross the FFI boundary.

### Fixed threading and callback rules

The runtime is confined to the UI thread by default. Other threads may only post tasks. Callbacks run at defined safe points and never while an internal mutable borrow is held. Owners keep listeners and callbacks alive; disposal prevents further invocation.

### No JSON on hot paths

Scenario, Inspect, and MCP continue to use JSON. Actual UI updates use native function calls or WASM calls. Batch changes use mutation buffers, and static templates are registered once, avoiding repeated cross-language calls for every node and property.

### Common bindings stay in the core

`Signal<i32> → Text`, `Signal<bool> → Class`, attribute bindings, and uniform bindings execute directly in the core without calling back into the external language for every update. Only complex computations and platform business logic cross into language callbacks.

## Validation sequence

1. Keep the counter stage Rust-only; do not implement the full FFI layer early.
2. After M1, add a minimal `fino_ffi` and validate signals, text binding, click, batching, and disposal with a C counter.
3. Implement the TypeScript/WASM package and run the same counter and scenarios.
4. Add Python, C#, Swift, and Kotlin according to real demand. Defer custom `GpuSurface` and complex GPU callbacks.

Rust RSX is a first-class authoring interface, but it is not the cross-language ABI. The cross-language ABI describes only templates, handles, bindings, events, and lifetimes.
