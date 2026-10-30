//! Reactive, DOM, layout, rendering, and host infrastructure.
//! 响应式、DOM、布局、渲染与 Host 基础设施。

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
/// Thin `reactive_graph` adaptation.
/// `reactive_graph` 薄适配层。
pub mod reactive;
/// Manual runtime and owner-scoped backend effects.
/// 手工 Runtime 与 Owner 作用域内的后端 Effect。
pub mod runtime;
/// Versioned JSON scenarios and machine-readable artifacts.
/// 版本化 JSON 场景与机器可读产物。
pub mod scenario;
