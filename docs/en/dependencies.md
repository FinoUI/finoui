[English](dependencies.md) · [简体中文](../zh-cn/dependencies.md)

# Dependency policy

## Mature crates first

### Custom implementation boundary

Only these differentiating capabilities require custom implementation:

- Component mounting and dynamic binding across DOM and native backends.
- Scheduling and invalidation protocols from reactive transactions to Style, Text, Layout, and Paint.
- Unified DOM semantics, backend conformance contracts, and the platform capability model.
- Integration of Material, Effect, GpuSurface, and reactive uniforms.
- AI scenario protocols, structured inspection, headless hosts, and development tooling.

The project does not reinvent reactive-graph algorithms, Unicode and shaping, Flex/Grid algorithms, vector rasterization, GPU abstractions, window systems, accessibility platform bridges, RSX parsing, image decoding, MCP protocols, or general-purpose testing infrastructure by default.

A proposed custom implementation must answer: does the existing library lack functionality, semantics, or performance? Can adaptation, composition, or a small upstream patch solve the problem? Only a reproducible blocker justifies custom implementation or a temporary fork.

### Recommended dependency baseline

| Layer | Preferred candidates | Integration and rationale |
|---|---|---|
| Reactivity | `reactive_graph` | Provides signals, derived computations, effects, and automatic dependency tracking; adapt framework scheduling semantics through `fino::reactive` |
| RSX | `rstml` + `syn` + `quote` | Reuse recoverable RSX parsing and source spans instead of hand-writing an HTML parser over TokenStream |
| Layout | `taffy` | Provides Block, Flexbox, Grid, and external measurement; hide it behind `fino::layout` |
| Text | `parley` + `fontique` | Reuse font fallback, HarfRust shaping, ICU4X analysis, and layout behind the ShapedText API |
| 2D rendering | `vello_gpu` / `vello_cpu`, or direct `wgpu` when needed | Evaluate the existing Sparse Strips renderer first; use the CPU path for software and headless validation |
| GPU / window | `wgpu` + `winit` | Retain the established cross-platform base instead of adding another abstraction |
| Geometry and color | `kurbo` + `peniko` | Share basic types with the Linebender graphics stack and reduce conversions |
| Accessibility | `accesskit` + `accesskit_winit` | Reuse the semantics schema, platform adapters, and consumer test capabilities |
| Node storage | `slotmap` | Provides generational keys from the counter stage instead of requiring a hand-written arena safety layer |
| Web | `web-sys` + `wasm-bindgen` + `wasm-bindgen-futures` | Use the real DOM and standard wasm boundary |
| Serialization and schema | `serde` + `schemars` + `serde_json` | Share type definitions across scenarios, inspection snapshots, errors, and MCP parameters |
| Diagnostics and observability | `thiserror` + `anyhow` + `tracing` | Use structured errors in libraries and contextual errors and traces in applications and tools |
| MCP | `rmcp` | Serve only as a thin protocol adapter for `fino::cli`; restrict Tokio to tool processes |
| Testing | `proptest` + `insta` + `trybuild` + `wasm-bindgen-test` + Criterion | Cover properties, snapshots, macro diagnostics, wasm, and performance regressions |

Versions are managed centrally in the workspace. The verified combination of wgpu, Vello, and Parley is locked. Dependency updates land in separate PRs rather than being mixed with feature work.

## Wrapping and direct use

A wrapper is not required for every dependency. Adapt a library only when at least one condition applies:

- It is a replaceable algorithm backend, such as Taffy, Parley, or Vello.
- Platform semantics must be unified, as with windows, text input, and accessibility.
- Invalidation tracking, virtual clocks, debugging, or deterministic control must be inserted.
- Third-party types must be kept out of public application APIs.

Simple, stable utility libraries can be used directly. Wrappers only map semantics and add observability; they do not duplicate the entire upstream API.

## Dependency admission and exit

Before adding a core dependency, validate it with one vertical slice:

```text
Can it build for the target platforms?
→ Does it support headless testing?
→ Do its license and MSRV meet the baseline?
→ Does it permit our scheduling and invalidation model?
→ Does it introduce conflicting async/GPU versions?
→ Do the counter and performance scenarios pass?
```

Prefer submitting fixes upstream. A temporary fork must record its patches, upstream PR, maintenance owner, and exit conditions. An implicit long-term fork must not become a second framework.
