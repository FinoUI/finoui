//! Shared `view!`-built counter component used by every backend and scenario.
//! 所有后端与场景共用的 `view!` 构建计数器组件。

use fino::{
    dom_api::DomBackend,
    reactive::{Owner, Signal},
    runtime::Runtime,
    view::{View, ViewOutput, signal_named},
};

/// Shared cross-backend conformance sequence and stable report.
/// 共享的跨后端一致性序列与稳定报告。
pub mod conformance;

/// Counter-owned smoke checks and scenario invocation.
/// 计数器示例拥有的冒烟检查与场景调用。
pub mod runner;

/// Native browser orchestration for the wasm counter.
/// wasm 计数器的原生浏览器编排。
#[cfg(not(target_arch = "wasm32"))]
pub mod browser;

/// Native winit/wgpu host for the same counter.
/// 同一计数器的原生 winit/wgpu Host。
#[cfg(not(target_arch = "wasm32"))]
pub mod native;

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
pub fn counter<B: DomBackend + 'static>() -> View<B> {
    View::new(|runtime, owner| {
        let count = signal_named("counter/count", 0);
        counter_markup(runtime, owner, count)
    })
}

/// Builds the counter against an application-owned count signal.
/// 使用应用持有的计数 Signal 构建计数器。
pub(crate) fn counter_with_count<B: DomBackend + 'static>(count: Signal<i32>) -> View<B> {
    View::new(move |runtime, owner| counter_markup(runtime, owner, count.clone()))
}

/// Authoring form required by the counter: one static button template, one
/// dynamic text binding, and one click event binding.
/// 计数器要求的作者形式：一个静态按钮模板、一个动态文本绑定与一个点击事件绑定。
fn counter_markup<B: DomBackend + 'static>(
    runtime: &mut Runtime<B>,
    owner: &Owner,
    count: Signal<i32>,
) -> ViewOutput<B> {
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
