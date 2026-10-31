//! Reactive, DOM, layout, rendering, and host infrastructure.
//! 响应式、DOM、布局、渲染与 Host 基础设施。

// Lets macro expansions refer to `::fino` from inside this crate as well.
// 让宏展开在本 crate 内部也能引用 `::fino`。
extern crate self as fino;

/// Non-rendering deterministic mock backend.
/// 无渲染的确定性 Mock 后端。
pub mod backend_mock;
/// Real browser DOM backend for wasm32 targets.
/// wasm32 目标的真实浏览器 DOM 后端。
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod backend_web;
/// Backend-neutral DOM contracts.
/// 后端无关 DOM 契约。
pub mod dom_api;
/// Shared structured diagnostics.
/// 共享结构化诊断。
pub mod error;
/// Deterministic headless host and snapshots.
/// 确定性无头 Host 与快照。
pub mod headless;
/// Read-only structured artifact inspection.
/// 只读结构化产物检查。
pub mod inspect;
/// Shared runtime metrics across Mock, Web, and Native backends.
/// 跨 Mock、Web 与 Native 后端共享的运行指标。
pub mod metrics;
/// Native DOM, text, layout, offscreen, and winit/wgpu host.
/// 原生 DOM、文本、布局、离屏与 winit/wgpu Host。
#[cfg(feature = "native")]
pub mod native;
/// Thin `reactive_graph` adaptation.
/// `reactive_graph` 薄适配层。
pub mod reactive;
/// Manual runtime and owner-scoped backend effects.
/// 手工 Runtime 与 Owner 作用域内的后端 Effect。
pub mod runtime;
/// Versioned JSON scenarios and machine-readable artifacts.
/// 版本化 JSON 场景与机器可读产物。
pub mod scenario;
/// Minimal Parley/Fontique text engine with explicit shape and layout phases.
/// 明确区分 shape 与 layout 阶段的最小 Parley/Fontique 文本引擎。
#[cfg(feature = "native")]
pub mod text;
/// Backend-neutral `view!` authoring support.
/// 后端无关的 `view!` 作者支持。
pub mod view;

/// RSX authoring macro for the counter-stage subset.
/// 计数器阶段子集的 RSX 作者宏。
pub use fino_macros::view;
/// Creates a Signal on the currently mounting View owner.
/// 在正在挂载的 View Owner 上创建 Signal。
pub use view::{View, signal, signal_named};
