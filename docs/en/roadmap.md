[English](roadmap.md) · [简体中文](../zh-cn/roadmap.md)

# Evolution and risks

## Post-counter roadmap

| Stage | New capabilities | Required proof |
|---|---|---|
| M1: Structural reactivity | Show, For, anchors, keyed move | No rebuild during reordering; complete disposal |
| M2: Basic applications | Style IR, controlled `class` subset, focus, input fields, scrolling | Real forms and IME work |
| M3: Performance core | ShapedText caches, incremental layout, virtual lists | Long documents and large lists meet the performance contract |
| M4: Native visuals | Display lists, damage, layers, materials | Local updates and GPU batching are effective |
| M5: Platform expansion | Android, iOS, Web SharedLayout evaluation | Lifecycle and platform input preserve the core |
| M6: Developer ecosystem | Complete RSX, DevTools, hot reload, control library | Real applications stabilize the API |

The counter runs throughout every stage. It is the framework's minimum conformance test and must not be deleted as features are added.

## Risks and controls

| Risk | Early signal | Control |
|---|---|---|
| Abstraction pulled toward DOM | Browser-specific concepts appear in native APIs | Include Mock and Native in conformance tests from the start |
| Abstraction pulled toward wgpu | Web components must understand GPU resources | Keep shader capabilities independent; ordinary DOM does not depend on the renderer |
| Text scope grows unchecked | The first version promises all Unicode/CSS behavior | Define the supported subset, reuse a mature text stack, build a corpus |
| Layout frequently invalidates the whole tree | One text update lays out as many nodes as the whole tree | Record invalidation causes and layout counts; add caches |
| Web and native behavior diverge | The same RSX requires extensive conditional code | Fix shared semantics and name platform extensions explicitly |
| Shaders damage performance | Every control has its own pipeline or offscreen texture | Material instancing, explicit Effect boundaries, texture reuse |
| AI output exceeds validation capacity | Many untested modules appear together | Small PRs, tests first, continuous vertical-slice execution |
| APIs become public too early | Example compatibility blocks core refactoring | Remain experimental until validated by real applications |
