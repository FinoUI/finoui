[English](native-backend.md) · [简体中文](../zh-cn/native-backend.md)

# Native counter backend

The first native vertical slice drives the shared counter through a real winit window and wgpu surface. It establishes the window model that later multi-window work extends rather than replaces.

## Structure

```text
Application
├── AppOwner: cross-window signals such as counter/count
├── WindowManager
│   └── WindowContext
│       ├── WindowId
│       ├── NativeDom: slotmap arena with generational NodeId
│       ├── window Owner, focus, IME, accessibility
│       ├── layout, display list, damage
│       └── surface, frame requests, input routing
└── SharedRenderContext
    ├── wgpu Device / Queue / pipeline cache
    ├── Rasterized glyph atlas
    └── TextEngine with Parley + Fontique
```

`fino::text` owns the public `TextSpec → ShapedText → TextLayout` model. Shaping is cached by text, font, locale, and white-space settings. Width-only constraint changes clone the shaped result and rerun line layout without reshaping.

`fino::native::dom::NativeDom` implements both `DomBackend` and `HeadlessBackend`. Every mutation carries its owning `WindowId`. Layout, paint, hit testing, and committed geometry reads also take `WindowId`; no `current_window()` exists.

## Update path

```text
PointerInput { window, position }
→ WindowManager::route_input
→ WindowContext::dispatch_input
→ NativeDom::hit_test(window, position)
→ count.update(+1)
→ counter/text effect runs once
→ NativeDom::set_text on the one dynamic text node
→ ShapedText and TextLayout invalidated for that node
→ only that text reshapes
→ layout and paint for the affected WindowId
→ one coalesced frame request
```

A white-space or accessibility-only write does not request a frame. Writing the same text is a no-op. Repeated requests before a presented frame coalesce into one.

`NativeDom::set_button_color` is a paint-only change: it requests paint without shaping or layout. `WindowContext::runtime_stats()` exposes the shared `RuntimeStats` for one window, including actual `shaped_texts`, `layout_nodes`, and coalesced `frames_requested`. Width-only changes rerun line layout for visible text without reshaping unchanged content.

## Lifecycle

- `resize` updates logical viewport, invalidates layout, and keeps the DOM and count.
- `remove` detaches only the selected node. Descendants, generational handles, and listeners stay valid, so the same subtree can be reinserted.
- `SurfaceLost` drops only GPU state. DOM, shaped text, layout, focus, and signals remain.
- A recreated surface reconfigures from the shared adapter and device.
- Closing a window removes only its `WindowContext`, window Owner, listeners, and surface.

## NativeOffscreen

`fino::native::offscreen_backend(viewport, dpr)` creates the same native DOM, text, layout, and display-list path without a window, surface, or display server. It supports `dom`, `a11y`, `reactive`, `trace`, `layout`, and `display_list` snapshots.

The example runs the same scenario file in both backends:

```bash
cargo run -p fino_counter -- scenario run examples/counter/scenarios/counter.json --backend mock --format json
cargo run -p fino_counter -- scenario run examples/counter/scenarios/counter.json --backend native-offscreen --format json
```

## Real window

```bash
cargo run -p fino_counter -- native
cargo run -p fino_counter -- native --smoke
```

`--smoke` clicks twice through the same window-tagged input path and then exits. It reports `surface_presented`. A sleeping or headless display can leave the surface occluded; the run then still verifies count, DOM, hit testing, and frame requests, but reports that no frame was presented instead of claiming a visual pass.

The native backend bundles the Roboto variable font under the SIL Open Font License 1.1; see `crates/fino/assets/fonts/OFL.txt`.

## Boundaries

This slice implements one button, one font, one window, and a minimal wgpu pipeline. It does not add multi-window UI, FFI, MCP, full font or Unicode coverage, a complete style system, or custom WGSL Material/Effect support.
