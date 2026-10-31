//! Shared `view!`-built counter component used by every backend and scenario.
//! 所有后端与场景共用的 `view!` 构建计数器组件。

use fino::{
    reactive::Signal,
    view::{View, signal_named},
};

/// Shared cross-backend conformance sequence and stable report.
/// 共享的跨后端一致性序列与稳定报告。
pub mod conformance;

/// Manual keyed For scenario shared by every backend.
/// 所有后端共用的手工 keyed For 场景。
pub mod keyed;
/// Counter-owned smoke checks and scenario invocation.
/// 计数器示例拥有的冒烟检查与场景调用。
pub mod runner;
/// Manual Show range scenario shared by every backend.
/// 所有后端共用的手工 Show 范围场景。
pub mod show;

/// Native browser orchestration for the wasm counter.
/// wasm 计数器的原生浏览器编排。
#[cfg(not(target_arch = "wasm32"))]
pub mod browser;

/// Native Host and Backend initialization for the same counter.
/// 同一计数器的原生 Host 与 Backend 初始化。
#[cfg(not(target_arch = "wasm32"))]
pub mod native;

/// Native winit event loop, rendering, input routing, and smoke runner.
/// 原生 winit 事件循环、渲染、输入路由与 smoke 运行器。
#[cfg(not(target_arch = "wasm32"))]
pub mod native_runner;

/// wasm Host and Backend initialization.
/// wasm Host 与 Backend 初始化。
#[cfg(target_arch = "wasm32")]
pub mod web;

/// wasm lifecycle, scenario, and probe runners.
/// wasm 生命周期、场景与探针运行器。
#[cfg(target_arch = "wasm32")]
pub mod web_runner;

/// Shared counter component authoring surface built with `view!`.
/// 使用 `view!` 构建的共享计数器组件作者接口。
pub fn counter() -> View {
    let count = signal_named("counter/count", 0);
    fino::view! {
        <button
            fino:name="counter"
            aria-label="Increment counter"
            on:click={move || count.update(|value| *value += 1)}
        >
            "Count: " {move || count.get()}
        </button>
    }
}

/// Builds the same counter against an application-owned count signal.
/// 使用应用持有的计数 Signal 构建同一计数器。
///
/// Kept for conformance probes that batch through the signal directly.
/// 保留给直接通过 Signal 执行批处理的一致性探针。
pub(crate) fn counter_with_count(count: Signal<i32>) -> View {
    fino::view! {
        <button
            fino:name="counter"
            aria-label="Increment counter"
            on:click={move || count.update(|value| *value += 1)}
        >
            "Count: " {move || count.get()}
        </button>
    }
}

#[cfg(test)]
mod tests;
