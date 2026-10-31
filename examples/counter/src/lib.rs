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

/// wasm browser entry points that mount the shared counter on `document.body`.
/// 将共享计数器挂载到 `document.body` 的 wasm 浏览器入口。
#[cfg(target_arch = "wasm32")]
pub mod web;

/// Shared counter component authoring surface built with `view!`.
/// 使用 `view!` 构建的共享计数器组件作者接口。
pub fn counter<B: DomBackend + 'static>() -> View<B> {
    View::new(|runtime, owner| {
        let count = signal_named("counter/count", 0);
        counter_markup(runtime, owner, count)
    })
}

/// Mounts the shared counter component and returns its root node.
/// 挂载共享计数器组件并返回其根节点。
pub fn mount_counter<B: DomBackend + 'static>(runtime: &mut Runtime<B>, owner: &Owner) -> B::Node {
    counter().mount(runtime, owner)
}

/// Mounts the counter against an application-owned shared count signal.
/// 使用应用持有的共享计数 Signal 挂载计数器。
pub fn mount_counter_with_count<B: DomBackend + 'static>(
    runtime: &mut Runtime<B>,
    owner: &Owner,
    count: Signal<i32>,
) -> B::Node {
    counter_with_count(count).mount(runtime, owner)
}

#[allow(dead_code)] // Used by the wasm page and by crate tests.
// wasm 页面与 crate 测试会使用。
pub(crate) fn mount_counter_with_state<B: DomBackend + 'static>(
    runtime: &mut Runtime<B>,
    owner: &Owner,
) -> (B::Node, Signal<i32>) {
    let count = owner.signal_named("counter/count", 0_i32);
    let root = counter_with_count(count.clone()).mount(runtime, owner);
    (root, count)
}

/// Mounts the counter and returns the dynamic value node used by conformance probes.
/// 挂载计数器，并返回一致性探针使用的动态文本节点。
pub(crate) fn mount_counter_with_nodes<B: DomBackend + 'static>(
    runtime: &mut Runtime<B>,
    owner: &Owner,
) -> (B::Node, B::Node, Signal<i32>) {
    let count = owner.signal_named("counter/count", 0_i32);
    let mounted = counter_with_count(count.clone()).mount_with_slots(runtime, owner);
    let value = mounted
        .text_slots()
        .first()
        .cloned()
        .expect("counter has one dynamic text slot");
    (mounted.into_parts().0, value, count)
}

/// Builds the counter markup against an existing count signal.
/// 使用现有计数 Signal 构建计数器标记。
fn counter_with_count<B: DomBackend + 'static>(count: Signal<i32>) -> View<B> {
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
