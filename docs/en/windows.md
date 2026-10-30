[English](windows.md) · [简体中文](../zh-cn/windows.md)

# Multi-window architecture

Desktop multi-window support is a first-class capability. The native counter PR introduces `WindowId` and `WindowContext` even though it creates only one window.

The existing reactivity, DOM, and rendering design remains intact. The host must not implicitly assume one window, surface, focus target, or DOM root per application.

## Runtime structure

```text
Application
├── AppOwner
│   └── Signals, business state, and services shared across windows
├── WindowManager
│   ├── WindowContext A
│   │   ├── WindowOwner
│   │   ├── Independent DOM root and NodeId space
│   │   ├── Focus / IME / Accessibility
│   │   ├── Layout / Paint / Damage
│   │   ├── Surface / frame scheduling
│   │   └── Independent input routing
│   └── WindowContext B
│       └── ...
└── SharedRenderContext
    ├── wgpu Device / Queue
    ├── Pipeline and glyph caches
    └── Shareable image, font, and material resources
```

Each window owns its surface, DOM, focus, accessibility, and frame requests. GPU devices, fonts, text shaping, and other shareable resources are not recreated per window.

## Cross-window signals

```text
AppOwner::count
├── Window A::TextBinding
└── Window B::TextBinding
```

Calling `count.update()` in any window produces:

```text
One reactive transaction
→ Update the corresponding bindings in A and B
→ Mark Paint / Layout separately in A and B
→ Request one frame for each affected window
→ Do not redraw unrelated windows
```

Closing a window releases only its `WindowOwner`, DOM, listeners, and surface. Signals in `AppOwner` and all other windows remain alive.

## Lifecycle distinctions

- `SurfaceLost`: recreate the GPU surface while preserving the window's UI and state.
- `hide_window`: stop unnecessary painting while preserving window state.
- `close_window`: release that window's owner and UI resources.
- `close_application`: release `AppOwner` and shared rendering resources last.

## API and platform boundaries

The conceptual API is:

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

All input, geometry reads, and paint contexts carry window identity:

```rust
WindowId
WindowHandle        // External, generational handle.
                    // 对外、带 generation。
WindowEvent
WindowConstraints
committed_bounds(window, node)
request_current_bounds(window, node)
```

A global mutable `current_window()` must not determine event ownership.

| Platform | Multi-window strategy |
|---|---|
| Windows / macOS / Linux | Real native windows with first-class support |
| Web | Multiple views within one Document; additional browser windows are independent runtimes synchronized through messages or shared state |
| Android | Follow Activity, system split-screen, and other host capabilities; no promise of arbitrary desktop-style windows |
| iOS | Follow the Scene lifecycle; do not disguise a Scene as an ordinary desktop window |

Menus, tooltips, and popovers remain in-window layers by default. Only independent panels, inspectors, detachable tool windows, and similar UI create real native windows.

## Headless acceptance scenarios

The later multi-window vertical slice must prove:

1. Two windows share one counter; clicking in either window updates both precisely.
2. Closing one window releases its effects, events, and frame requests without affecting the other window.
3. Resize, DPR changes, and SurfaceLost in one window do not touch another window's layout or state.
4. Focus, IME, accessibility, and input events always route to the correct window.
5. Static windows request no frames, and local updates never redraw unrelated windows.

Full multi-window support is not required immediately. The first native host still adopts `Application → WindowManager → WindowContext`, avoiding global singletons that would require disruptive redesign later.
