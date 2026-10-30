[English](quality.md) · [简体中文](../zh-cn/quality.md)

# Quality, performance, and CI

## Performance contract and observability

High performance is expressed as testable contracts:

| Scenario | Forbidden behavior |
|---|---|
| Updating one text signal | Re-running all components, diffing the whole tree, or laying out the whole tree |
| Repeated writes in a batch | Committing DOM changes or requesting an independent frame for every write |
| Changing only a color | Text shaping or layout |
| Changing only width constraints | Reshaping unchanged text |
| Unchanged static material uniforms | Idling indefinitely at 60/120 FPS |
| Long-list scrolling | Creating DOM or native nodes for every data item |

DevTools records at least:

```text
Component executions
Effect executions and their causes
Backend operations
Shaped nodes/paragraphs
Laid-out nodes
Paint and damage extents
Draw calls / pipeline switches
CPU frame time / GPU frame time
Memory used by nodes, text caches, and offscreen textures
```

Optimization decisions use these counters and real frame times, not text-measurement microbenchmarks alone.

## Test strategy

### Reactive unit tests

Cover:

- Dependency creation, updates, removal, and re-establishment.
- Batches, nested batches, memos, and duplicate notifications.
- Nested owner disposal and idempotent cleanup.
- Effect re-entry, dependency cycles, and error reporting.
- Conditional dependency changes, ensuring old dependencies no longer trigger updates.

### Backend conformance tests

Run the same operation set on Mock, Web, and Native:

```text
Create structure
→ Change text
→ Change attributes
→ Insert and move
→ Dispatch events
→ Unmount
```

Compare normalized logical trees, event results, and operation counts. Platform-specific node handles are excluded.

### Text and layout tests

| Type | Method |
|---|---|
| Unit tests | Width, wrapping, line counts, cursor behavior, and hit testing with fixed fonts |
| Property tests | Arbitrary constraints produce no negative sizes, NaN, out-of-bounds access, or infinite loops |
| Differential tests | Compare supported semantics with browser results and report the first divergence |
| Multilingual corpus | Chinese, Arabic, combining characters, emoji, missing glyphs, and font fallback |
| Incremental tests | Local edits invalidate only the corresponding text blocks and layout subtrees |

The browser is one reference implementation, not a specification that every platform must reproduce pixel for pixel. Cross-platform consistency is defined by the framework's shared semantics.

### Rendering and platform tests

- Validate WGSL compilation on every target backend.
- Use golden images with explicit tolerances for key materials.
- Test SurfaceLost, DeviceLost, Suspend, Resume, and DPR changes.
- Verify touch, soft keyboards, safe areas, and text input on physical Android/iOS devices.
- Verify focus, keyboard input, text selection, DOM order, and the accessibility tree on Web.

### Performance regression scenarios

Maintain at least these fixed scenarios:

```text
High-frequency single-text updates
Streaming append to a long document
10,000-row keyed-list updates
Large-list scrolling
Continuous window resizing
Local animation within a complex static page
```

Each scenario records correctness counters and frame times. A result must not gain apparent performance by omitting required work.

## Determinism and CI

### Reproducible environment

Headless runs fix:

```text
Viewport / DPR / locale / direction
Font files and font versions
Random seed
Virtual clock and frame number
Color scheme
Software renderer version
Scenario schema and snapshot schema
```

Screenshot baselines may be managed separately by renderer with small, defined tolerances. Logical assertions remain strict; image tolerances must not hide text or structural errors.

### CI layers

| Layer | Checks for each commit |
|---|---|
| Fast | fmt, check, clippy, unit tests, Mock scenarios, macro tests |
| Core | NativeOffscreen, software snapshots, WebHeadless, backend conformance |
| Extended | Multiple browsers, DPRs, languages, property tests, performance regressions |
| Platform | Desktop hosts, Android/iOS emulators or physical-device jobs |
| Release | Real GPUs, IME, touch, screen readers, power use, and frame stability |

### Limits of headless validation

Headless environments cover most coding, debugging, structural assertions, visual regression, and performance analysis. They cannot prove:

- Compatibility with every real GPU and driver.
- Correct mobile soft-keyboard, system-gesture, and IME behavior.
- Expected screen-reader announcements.
- Acceptable performance under high refresh rates, thermal throttling, and real touch input.

AI-assisted, headless development is therefore the daily feedback loop. Release acceptance still includes automated checks that require physical devices and human evaluation.
