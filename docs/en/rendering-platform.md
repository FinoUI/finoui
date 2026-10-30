[English](rendering-platform.md) · [简体中文](../zh-cn/rendering-platform.md)

# Rendering and platform hosts

## Native rendering and GPU extensions

### Display list

```rust
pub enum DrawCommand {
    Quad(DrawQuad),
    Text(DrawText),
    Path(DrawPath),
    Material(DrawMaterial),
    Effect(ApplyEffect),
    GpuSurface(DrawGpuSurface),
    PushLayer(LayerSpec),
    PopLayer,
}
```

Layout produces geometry and paint descriptions without calling the GPU directly. The renderer handles ordering, batching, caching, offscreen textures, and final submission.

### Shader capabilities

| Type | Purpose | Main cost |
|---|---|---|
| `Material` | Replaces rectangle or control-surface painting | Pipeline switches and instance data |
| `Effect` | Applies blur, distortion, and other post-processing to a painted subtree | Offscreen textures and additional full-screen passes |
| `GpuSurface` | Provides self-rendered regions for particles, charts, video, 3D, and similar content | Independent rendering state and synchronization |

WGSL is the only custom shader language. The framework provides shared conventions for coordinates, color spaces, premultiplied alpha, antialiasing, and uniforms. Developers do not start from a complete pipeline.

### Signal-driven uniforms

```text
Signal change
→ Update the corresponding MaterialInstance uniform
→ Mark paint
→ Request a frame
```

This path must not re-execute components, recreate nodes, reshape text, or rerun layout. Animations request continuous frames only while active; rendering returns to an on-demand model when they stop.

### Web capability mapping

| Native effect | Web backend strategy |
|---|---|
| Gradients, rounded corners, shadows, simple filters | Translate to CSS |
| Effects with partial SVG representations | Use SVG |
| Arbitrary WGSL Material or Effect | Use a local WebGPU canvas inside the node, or a declarative fallback |
| GpuSurface | WebGPU or WebGL canvas |

Ordinary UI always uses real DOM. A local GPU canvas is an explicit capability and must never silently turn the entire application into a canvas.

## Platform hosts and services

### Host responsibilities

A host only:

- Creates and drives the application lifecycle.
- Provides windows, surfaces, the event loop, and frame requests.
- Converts platform input.
- Connects services such as fonts, IME, clipboard, files, and accessibility.

The UI core has no direct knowledge of Activity, UIKit, DOM Window, or winit Window.

Desktop hosts use `Application → WindowManager → WindowContext` from the first native counter. Each `WindowContext` owns its DOM root, focus, accessibility, input routing, layout/paint state, surface, and frame requests. `SharedRenderContext` retains the GPU device, queue, pipelines, glyph caches, and other shareable resources. See [Multi-window architecture](windows.md).

### Unified lifecycle

```text
HostCreated
SurfaceReady
SurfaceLost
Suspended
Resumed
HostDestroyed
```

UI state, the DOM, and ShapedText must survive surface loss. Surface lifecycle events and frame requests are scoped by `WindowId`; rebuilding one window's surface must not invalidate another window. The host and renderer recreate GPU devices, surfaces, and swapchain resources according to their distinct application, window, and surface lifetimes.

### Input and semantics

The shared input model supports mouse, touch, stylus, keyboard, IME, and focus while preserving platform differences. Mobile soft keyboards, system back actions, safe areas, and text selection are coordinated by hosts and platform services. The framework does not bypass system capabilities with a custom-drawn keyboard.

The native semantics tree is generated from the DOM-like structure and mapped to AccessKit. Web uses real DOM semantics by default. SharedLayout Web mode still requires separate validation of DOM order, focus, and text selection.
