[English](ai-native-development.md) · [简体中文](../zh-cn/ai-native-development.md)

# AI-native development

## AI-assisted implementation protocol

AI can accelerate implementation, but every task must include executable acceptance criteria. The recommended work unit is:

```text
One narrowly scoped specification
→ Write a failing test first
→ Implement the minimum code
→ Run unit and conformance tests
→ Check public types for leaked platform dependencies
→ Collect performance counters
→ Commit
```

Architecture maintainers retain explicit ownership of the core invariants:

- Signal dependencies and effect scheduling.
- Owner/disposal and Rust ownership.
- DOM node lifecycle.
- Layout invalidation propagation.
- Text cache keys.
- GPU resources and surface lifecycle.

AI must not generate every platform, complete CSS, a complete text engine, and a control library in one step. Each PR must be independently revertable and preserve the counter conformance tests.

## Goals

The complete AI development loop for frameworks and applications must run without a display server:

```text
Read machine-readable capabilities
→ Generate or modify Rust/RSX
→ fmt / check / clippy / test
→ Run headless scenarios
→ Read DOM, a11y, reactive, layout, trace, and image snapshots
→ Fix using structured failures
→ Rerun and produce a verification report
```

The goal is executable feedback for every generation, not bypassing validation. The CLI is the source of truth. An Agent Skill teaches agents the same workflows without duplicating business logic. MCP is deferred until external integrations, authorization, or cross-client requirements justify a thin adapter.

## Tool layering

```text
fino::runtime / fino::dom_api / fino::backends::*
        ↓
fino::headless: virtual clock, deterministic input, offscreen targets
        ↓
fino::scenario: Action / Assert / Capture
        ↓
fino::inspect + fino::test: structured state and validation
        ↓
fino_cli: generic commands, JSON output, artifacts
        ↓
.agents/skills/fino: workflow guidance only
        ↓
Any AI agent / CI / human developer
```

`fino::headless` does not simulate reactivity or the DOM. It runs real framework code and replaces only time, windows, frame presentation, and input sources.

## Headless backend matrix

| Backend | Purpose | Display required |
|---|---|---|
| Mock | Reactivity, DOM operations, disposal, and event logic | No |
| NativeOffscreen | Real text, layout, display lists, and offscreen images | No |
| NativeSoftware | Deterministic snapshots with a fixed software renderer | No |
| WebHeadless | Real DOM, events, and screenshots in headless Chromium | No |
| HostIntegration | winit, window, and platform-service integration; may run under Xvfb in CI | Usually no |
| MobileEmulator | Android/iOS builds, lifecycle, and limited integration tests | No, but requires the corresponding CI toolchain |

NativeOffscreen creates texture targets directly and does not require a winit surface. Deterministic image baselines prefer a fixed-version software rendering path. Hardware GPU testing is a separate performance and compatibility task, not part of logical snapshot comparisons.

## Scenario protocol and structured inspection

### Scenarios as the application test-authoring interface

Scenarios use versioned JSON, with JSON Schema generated from the same Rust types:

```json
{
  "schema": 1,
  "name": "counter/increment",
  "mount": "counter",
  "environment": {
    "viewport": [800, 600],
    "dpr": 1.0,
    "locale": "zh-CN",
    "color_scheme": "dark",
    "seed": 42
  },
  "steps": [
    { "assert_text": { "role": "button", "name": "Increment counter", "value": "Count: 0" } },
    { "click": { "role": "button", "name": "Increment counter" } },
    { "advance_frames": 1 },
    { "assert_text": { "role": "button", "name": "Increment counter", "value": "Count: 1" } },
    { "capture": ["dom", "a11y", "reactive"] }
  ]
}
```

Cross-backend shared scenarios capture only logical snapshots supported by every backend. `layout`, `display_list`, and `screenshot` are added through backend-specific run parameters for NativeOffscreen or WebHeadless. Mock must still execute the core assertions, and unsupported capabilities must be reported explicitly rather than producing silently empty snapshots.

Selector priority is fixed: `role + accessible name`, then stable `test_id`, then semantic queries. AI must not use unstable internal node IDs, absolute screen coordinates, or generated classes as default locators.

Scenarios support virtual time and explicit frame advancement. Animation, debounce, async tasks, and random data use injectable clocks and seeds rather than test sleeps that guess completion times.

### Inspection snapshots

All tools provide these read-only snapshots:

| Snapshot | Contents |
|---|---|
| `dom` | Elements, text, attributes, parent/child relationships, stable semantic IDs |
| `a11y` | Role, name, focus, actions, and state |
| `reactive` | Signal/effect debug names, dependencies, execution counts, latest change reasons |
| `layout` | Constraints, dimensions, positions, cache hits, invalidation reasons |
| `display_list` | Draw commands, layers, materials, damage, offscreen bounds |
| `resources` | Fonts, textures, GPU buffers, cache sizes, surface state |
| `trace` | Transactions, backend operations, frame phases, errors, source locations |

Snapshots use stable ordering and versioned schemas. Internal pointers, random addresses, and meaningless timestamps are excluded so that AI can compare snapshots reliably.

### Structured errors

Tool errors use a shared shape wherever possible:

```json
{
  "code": "UI_LAYOUT_INVALID_CONSTRAINT",
  "message": "layout produced a non-finite width",
  "severity": "error",
  "source": {
    "file": "examples/counter/src/lib.rs",
    "line": 18,
    "column": 9
  },
  "backend": "native-offscreen",
  "node": { "test_id": "counter-button" },
  "artifacts": ["trace.json", "dom.json", "layout.json"],
  "hint": "Inspect the last width constraint before frame 3"
}
```

A `hint` is an investigation starting point and cannot replace trace evidence. Rust panics, wgpu validation failures, browser exceptions, and macro errors are converted into tool results that retain their original output and exit codes.

## CLI, Skill, and application tooling

### Shared CLI

`fino_cli` produces the `fino` command. It provides only generic project and tooling operations; it never discovers, mounts, checks, or depends on examples. Its minimum command set is:

```bash
fino doctor --format json
fino check --format json
fino inspect dom --run artifacts/runs/counter-web --format json
fino inspect a11y --run artifacts/runs/counter-web --format json
fino shader check --format json
fino bench --baseline main --format json
```

Output is human-readable by default, while `--format json` returns the same structured results used by agents. Large snapshots are written to artifacts, and commands return only summaries and paths rather than placing megabytes of DOM or Base64 images into context. Every scenario run returns a stable `run_id` and `run_dir`. `fino inspect --run` reads artifacts from a completed run; `fino inspect --session` reads an explicitly kept-alive debug session. Neither depends on hidden global application state.

### Examples run through Cargo

Each example owns its component, mount function, smoke checks, and scenario runner. Developers invoke that runner through Cargo; `fino_cli` never performs example-specific work:

```bash
cargo run -p fino_counter -- check --format json
cargo run -p fino_counter -- scenario run examples/counter/scenarios/counter.json --backend mock --out artifacts/runs/counter-mock --format json
cargo run -p fino_counter -- scenario run examples/counter/scenarios/counter.json --backend web-headless --out artifacts/runs/counter-web --format json
cargo run -p fino_counter -- scenario run examples/counter/scenarios/counter.json --backend native-offscreen --out artifacts/runs/counter-native --format json
```

The example calls the shared `fino::scenario` and `fino::headless` library interfaces. `fino inspect` reads the resulting artifacts without knowing which example produced them.

### Agent Skill

The repository provides `.agents/skills/fino/SKILL.md`. It records command selection, Cargo-based example workflows, JSON output conventions, and artifact navigation. The Skill contains no scenario implementation, mount registry, or business logic; agents call the CLI and Cargo commands directly rather than parsing human terminal output.

MCP is not part of the initial toolchain. If external integrations, authorization, or cross-client access later requires it, an adapter may expose the same structured CLI results without becoming a second implementation.

### Application template

Applications built on the framework include by default:

```text
src/                  # Application components and business code
scenarios/            # AI-executable user behavior and assertions
assets/fonts/         # Licensed, reproducible test fonts
tests/                # Rust, backend conformance, and macro tests
fino.toml             # Backend, environment, and snapshot configuration
AGENTS.md             # Project commands, boundaries, validation requirements
artifacts/            # Ignored generated reports, images, and traces
```

Applications own their mount and scenario binaries and run them through Cargo. They do not need a separate test host. Using the framework's components, semantics, and platform services provides headless mounting, input scenarios, snapshots, and performance counters without adding application dependencies to `fino_cli`.

### AI-friendly Rust and RSX

- Use `rstml` recoverable parsing and spans so macro errors point to accurate source locations.
- Fix compiler-error expectations with `trybuild`, avoiding opaque macro-internal panics.
- Give public types and functions consistent names, rustdoc examples, and runnable doctests.
- Give signals, effects, materials, and other debug objects development-time names and `#[track_caller]` origins.
- Provide capability manifests and versioned schemas so AI does not infer command support from source.
- Share semantic IDs across component metadata, scenarios, and inspection snapshots.
- Compose formatting, expansion, and diagnostics from rustfmt, cargo expand, and leptosfmt/rstml ecosystem experience instead of reimplementing a complete Rust analyzer.
