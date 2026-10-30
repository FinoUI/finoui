[English](architecture.md) · [简体中文](../zh-cn/architecture.md)

# Architecture

> Status: implementation baseline v0.2, prioritizing mature crates, AI-native workflows, and headless development.
> Target platforms: Windows, macOS, Linux, Android, iOS, and Web.
> First vertical slice: one counter component shared by a real DOM backend and a native wgpu backend.
> Tooling: the `fino_cli` crate produces the `fino` command. It contains no example-specific behavior; examples run through Cargo. Third-party dependencies keep their actual names.

## Project definition

Fino is a cross-platform UI framework written in Rust. It follows SolidJS's fine-grained reactive model: components run once, signals establish precise dependencies on individual UI properties, and state changes update their target nodes directly. There is no virtual DOM and no re-execution of the component tree.

The framework unifies:

- Components, ownership, disposal, and reactive semantics.
- A DOM-like logical UI structure, events, and keyed children.
- Explicit text, style, layout, and paint phases.
- Cross-platform component source code and shared style contracts.

The framework does not force these implementations to be identical:

- Browser DOM storage on Web and arena node storage on native platforms.
- Browser CSS layout and the native layout engine.
- Browser painting and native wgpu rendering.
- Desktop windows, mobile lifecycle, and Web page hosts.

In short:

> Fine-grained Reactive DOM + ShapedText + Incremental Layout/Paint + DOM/wgpu Renderers.

The default style-authoring interface is a controlled, Tailwind-like `class` subset compiled into strongly typed Style IR. Web CSS is generated from that IR, while native backends consume it directly. Fino is Tailwind-like rather than Tailwind-compatible: Utility is the authoring syntax, and Style IR is the cross-platform source of truth.

All cross-language capabilities use a versioned, handle-based API. Rust RSX is a first-class authoring interface, not the cross-language ABI; other languages never bind internal Rust types. See [Multi-language bindings](bindings.md).

Desktop multi-window support is first-class. Native hosts use `Application → WindowManager → WindowContext` from the first native counter, including window identity in input, geometry, paint, and lifecycle contracts. See [Multi-window architecture](windows.md).

## Goals and non-goals

### Core goals

| Goal | Architectural implication |
|---|---|
| Components run once | RSX produces static templates and dynamic bindings instead of treating component functions as per-frame render functions |
| Precise updates | Signals connect directly to text, attributes, styles, uniforms, or child-structure anchors |
| Real DOM on Web | Ordinary elements, text, forms, and semantic nodes are real browser nodes |
| High-performance native DOM | An arena, stable node IDs, and phased invalidation avoid the browser's implicit synchronous layout |
| Predictable text | `shape` and `layout` are separate, and text measurement does not require mounted nodes |
| Extensible GPU effects | WGSL materials, effects, and GPU surfaces are first-class painting capabilities |
| Evolution on every platform | The core contains no concrete window, GPU, DOM, or async-runtime types |
| Multi-language bindings | Versioned handles and non-generic semantics isolate other languages from Rust layouts, generics, closures, and lifetimes |
| Desktop multi-window | Every window has independent UI and frame state, while application owners and shareable rendering resources outlive individual windows |
| Verifiability | Every backend runs the same conformance tests, and performance metrics are observable |
| Mature crates first | Custom implementation is reserved for differentiating layers rather than repeating existing text, layout, GPU, and protocol algorithms |
| AI-native | Framework and application development both support a headless feedback loop through structured CLI commands, scenarios, and snapshots |

### Non-goals

The first major version does not aim to provide:

- Complete W3C DOM, CSS, HTML, or browser behavior.
- Pixel-identical output across all six platforms.
- Arbitrary WGSL execution inside ordinary Web CSS.
- Multiple authoring interfaces such as XAML, MVVM, CSS, and JSX at the same time.
- A complete control library, Shadow DOM, full selectors, or rich-text editing in the first release.
- Identical platform behavior merely to maximize shared code.

### Core tradeoffs

| Decision | Choice | Rejected alternative |
|---|---|---|
| Reactive updates | Fine-grained dependencies and direct node updates | VDOM and per-frame component-tree reconstruction |
| Web rendering | Real DOM by default | Drawing the entire UI into one canvas |
| Native structure | DOM-like retained tree | Unstructured immediate-mode widget calls |
| Native rendering | wgpu and WGSL | A custom translation layer for multiple GPU APIs |
| Style authoring | A controlled Tailwind-like `class` subset compiled to Style IR | Hand-written CSS as the primary API, a Tailwind runtime, or arbitrary runtime class construction |
| Text | `TextSpec → ShapedText → TextLayout` | Touching the DOM or reshaping for every measurement |
| Layout reads | Explicit, phased, and cacheable | Implicitly flushing the whole tree during geometry reads |
| Custom GPU work | Material, Effect, and GpuSurface interfaces | Routing every requirement through one ShaderNode |
| Backend selection | One backend at compile time with generic static dispatch | Dynamic dispatch for every node operation |
| Cross-language ABI | Versioned, generational handles over non-generic semantics | Binding Rust generics, references, closures, `Result`, `Vec`, or reactive-graph objects directly |
| Native window model | `Application → WindowManager → WindowContext` from the first native host | Global single-window state or event routing through `current_window()` |

## System layers

```text
┌──────────────────────────────────────────────────────────┐
│ App: business components, signals, routing, app state     │
├──────────────────────────────────────────────────────────┤
│ Authoring: view!/rsx!, Show, For, components, templates   │
├──────────────────────────────────────────────────────────┤
│ Runtime: Owner, effect scheduling, bindings, mount/dispose│
├──────────────────────────────────────────────────────────┤
│ DOM API: nodes, events, styles, semantics, backend contract│
├───────────────────────┬──────────────────────────────────┤
│ Web Backend           │ Native Backend                   │
│ - Browser DOM         │ - Native DOM arena               │
│ - CSS / SharedLayout  │ - Style / Text / Layout          │
│ - DOM events          │ - Display List / Damage          │
│ - DOM accessibility   │ - wgpu / WGSL                    │
├───────────────────────┴──────────────────────────────────┤
│ Host: Desktop / Android / iOS / Web                      │
│ Services: IME, clipboard, files, fonts, a11y, lifecycle  │
└──────────────────────────────────────────────────────────┘
```

### Dependency direction

Dependencies point downward. Platform capabilities must never flow back into the reactive core:

```text
fino::reactive
    ↓
fino::component
    ↓
fino::runtime ──────→ fino::dom_api
    ↓                       ↑
    ├────────────→ fino::web
    └────────────→ fino::native
                           ↓
         fino::text / fino::layout / fino::render_wgpu
                           ↓
                    fino::platform
```

`fino::reactive` does not depend on the DOM. `fino::dom_api` depends on neither wgpu nor web-sys. `fino::text` does not depend on windows, and `fino::layout` does not depend on a concrete rendering backend. `fino_primitives`, `fino_components`, and `fino_blocks` build on these public contracts; `fino_glass` supplies their default semantic-token and material values.

## Package and theme boundaries

```text
fino
└── Reactivity, DOM, layout, rendering, and hosts; no concrete visual style

fino_primitives
└── Unstyled component behavior: focus, keyboard, state machines, a11y, popovers

fino_components
└── Official Button, Input, Card, Tabs, Dialog, and other components
    Uses semantic tokens and Material only; no hard-coded glass colors

fino_glass
└── Default Solid Glass theme
    Light/dark themes, three thicknesses, edge highlights, shadows, blur, and motion

fino_blocks
└── AI application blocks such as Composer, ToolSteps, DiffView, and AskCard
```

`fino_glass` is the default theme and the only package that owns Solid Glass's concrete visual values. `fino_components` and `fino_blocks` consume semantic tokens and material capabilities so their behavior and structure remain independent of the theme.

The MIT-licensed [Solid Glass](https://github.com/deeplethe/solid-glass) project separates its stylesheet from shadcn/ui component behavior. Fino preserves that boundary without bringing React, Tailwind, or shadcn types into the core. The visual reference is pinned to commit `6e8903e494a15491ceda38fafea5056c95a74d44` during the architecture baseline; any copied source or assets require their upstream copyright and license notices.

## Workspace structure

```text
crates/
  fino/              # Reactive core, DOM, layout, rendering, hosts, headless tooling
  fino_cli/          # Generic fino command; no example dependencies or behavior
  fino_primitives/   # Unstyled behavior and accessibility
  fino_components/   # Official theme-independent components
  fino_glass/        # Default Solid Glass theme
  fino_blocks/       # AI application blocks
examples/
  counter/
    src/lib.rs       # Counter component shared by both backends
    src/main.rs      # Example-owned checks and scenario runner invoked through Cargo
    src/native.rs
    src/web.rs
```

Within `fino`, the reactive, component, DOM API, runtime, backend, text, layout, rendering, platform, host, headless, scenario, inspection, DevTools, and test modules retain the boundaries described by this architecture. `fino_cli` depends on these public library interfaces but never on examples. The `.agents/skills/fino` Skill contains workflow guidance only. In particular, web-sys, wgpu, and winit types must not leak into shared component APIs.

## Initial technical decisions

Implementation starts with these constraints:

```text
Language: Rust
Reactivity: prefer reactive_graph behind a thin adapter for Owner, Signal, Effect, and batch semantics
First component: manually constructed counter
First backend: MockBackend
Second backend: Web DOM
Third backend: Native wgpu
Web layout: BrowserFlow
Native structure: arena + generational NodeId
Native host: Application → WindowManager → WindowContext from the first native counter
Cross-language ABI: versioned fino_api handles; C ABI and WASM are transports; no internal Rust types
Tooling: fino_cli + Agent Skill; examples run through Cargo; MCP deferred
Styles: controlled Tailwind-like class syntax → strongly typed Style IR; Web CSS is generated
Text API: TextSpec → ShapedText → TextLayout
Layout reads: no implicit synchronous flush
Shaders: WGSL later; none during the counter stage
Macros: implement view! only after the low-level vertical slice works
```

## References

- [SolidJS](https://www.solidjs.com/): reference for fine-grained reactivity and components that run once.
- [Solid Glass](https://github.com/deeplethe/solid-glass/tree/6e8903e494a15491ceda38fafea5056c95a74d44): MIT-licensed reference for the default theme's light/dark palettes, three thicknesses, edge treatment, shadows, blur, and motion.
- [Pretext](https://github.com/chenglou/pretext): reference for separating text precomputation from low-cost layout; this project does not adopt its naming.
- [Makepad](https://github.com/makepad/makepad): reference for shaders, SDF, instanced drawing, and developer experience.
- [wgpu](https://github.com/gfx-rs/wgpu): GPU infrastructure for native and Web backends.
- [winit](https://github.com/rust-windowing/winit): native window, input, and lifecycle infrastructure.
- [AccessKit](https://github.com/AccessKit/accesskit): native accessibility infrastructure.
