//! Counter-owned smoke checks and explicit Mock scenario mounting.
//! 计数器示例拥有的冒烟检查与显式 Mock 场景挂载。

use std::path::Path;

#[cfg(not(target_arch = "wasm32"))]
use fino::native::{Size, offscreen_backend};
use fino::{
    backend_mock::MockBackend,
    dom_api::{Role, Selector},
    headless::{Environment, ExecutionCounters, HeadlessError, HeadlessHost},
    metrics::RuntimeStats,
    scenario::{self, Scenario, ScenarioResult, Status, ToolError},
};
use serde::Serialize;

use crate::{conformance::ConformanceReport, mount_counter};

/// One counter smoke-check result.
/// 单项计数器冒烟检查结果。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CounterCheckResult {
    /// Stable check name.
    /// 稳定检查名。
    pub name: String,
    /// Check status.
    /// 检查状态。
    pub status: Status,
    /// Optional failure message.
    /// 可选失败消息。
    pub message: Option<String>,
}

/// Counter-owned smoke-check report.
/// 计数器示例拥有的冒烟检查报告。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CounterCheckReport {
    /// Report schema version.
    /// 报告 schema 版本。
    pub schema: u32,
    /// Checked backend.
    /// 被检查后端。
    pub backend: String,
    /// Overall status.
    /// 总体状态。
    pub status: Status,
    /// Individual checks.
    /// 各项检查。
    pub checks: Vec<CounterCheckResult>,
    /// Acceptance counters after the smoke check.
    /// 冒烟检查后的验收计数。
    pub counters: ExecutionCounters,
    /// Unified runtime counters after the smoke check.
    /// 冒烟检查后的统一运行时计数。
    pub runtime_stats: RuntimeStats,
}

/// Runs the counter's initial-text and consecutive-click checks.
/// 运行计数器初始文本与连续点击检查。
pub fn check_counter() -> CounterCheckReport {
    let mut host = HeadlessHost::new(MockBackend::new(), Environment::default());
    host.mount("counter", mount_counter);
    let selector = counter_selector();
    let mut checks = Vec::new();
    run_check(&mut checks, "initial_text", || {
        host.assert_text(&selector, "Count: 0")
    });
    run_check(&mut checks, "first_click", || {
        host.click(&selector)?;
        host.assert_text(&selector, "Count: 1")
    });
    run_check(&mut checks, "second_click", || {
        host.click(&selector)?;
        host.assert_text(&selector, "Count: 2")
    });
    CounterCheckReport {
        schema: 1,
        backend: "mock".to_owned(),
        status: if checks.iter().all(|check| check.status == Status::Passed) {
            Status::Passed
        } else {
            Status::Failed
        },
        checks,
        counters: host.execution_counters(),
        runtime_stats: host.runtime_stats(),
    }
}

/// Runs the shared conformance sequence on the deterministic mock backend.
/// 在确定性 Mock 后端运行共享一致性序列。
pub fn run_mock_conformance() -> Result<ConformanceReport, String> {
    let host = HeadlessHost::new(MockBackend::new(), Environment::default());
    crate::conformance::run_conformance(host)
}

/// Runs the shared conformance sequence on the native offscreen backend.
/// 在原生离屏后端运行共享一致性序列。
#[cfg(not(target_arch = "wasm32"))]
pub fn run_native_offscreen_conformance() -> Result<ConformanceReport, String> {
    let backend = offscreen_backend(Size::new(800.0, 600.0), 1.0);
    let host = HeadlessHost::new(backend, Environment::default());
    crate::conformance::run_conformance(host)
}

/// Runs a scenario with the counter mount supplied explicitly to the generic library.
/// 将计数器 mount 显式传给通用 library 并运行场景。
pub fn run_mock_scenario(scenario: &Scenario, run_dir: &Path) -> Result<ScenarioResult, ToolError> {
    let host = HeadlessHost::new(MockBackend::new(), scenario.environment.clone());
    scenario::run_on_host(scenario, run_dir, host, mount_counter)
}

/// Runs the same scenario on the real native text/layout/paint backend without a GPU surface.
/// 在无 GPU Surface 的真实原生文本、布局与绘制后端上运行同一场景。
#[cfg(not(target_arch = "wasm32"))]
pub fn run_native_offscreen_scenario(
    scenario: &Scenario,
    run_dir: &Path,
) -> Result<ScenarioResult, ToolError> {
    let [width, height] = scenario.environment.viewport;
    let backend = offscreen_backend(
        Size::new(width as f32, height as f32),
        scenario.environment.dpr as f32,
    );
    let host = HeadlessHost::new(backend, scenario.environment.clone());
    scenario::run_on_host(scenario, run_dir, host, mount_counter)
}

fn counter_selector() -> Selector {
    Selector {
        role: Role::Button,
        name: "Increment counter".to_owned(),
    }
}

/// Records each smoke-check outcome while allowing later independent checks to run.
/// 记录每项冒烟检查结果，同时允许后续独立检查继续执行。
fn run_check(
    checks: &mut Vec<CounterCheckResult>,
    name: &str,
    check: impl FnOnce() -> Result<(), HeadlessError>,
) {
    match check() {
        Ok(()) => checks.push(CounterCheckResult {
            name: name.to_owned(),
            status: Status::Passed,
            message: None,
        }),
        Err(error) => checks.push(CounterCheckResult {
            name: name.to_owned(),
            status: Status::Failed,
            message: Some(error.message),
        }),
    }
}
