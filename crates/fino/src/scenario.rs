//! Versioned backend-neutral JSON scenarios with an explicit caller-supplied mount.
//! 使用调用方显式提供的 mount，执行版本化后端无关 JSON 场景。

use std::{fmt::Write as _, fs, path::Path};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use crate::error::ToolError;
use crate::{
    dom_api::Selector,
    headless::{Environment, ExecutionCounters, HeadlessBackend, HeadlessHost, SnapshotKind},
    reactive::Owner,
    runtime::Runtime,
};

/// Current scenario protocol version.
/// 当前场景协议版本。
pub const SCENARIO_SCHEMA: u32 = 1;

/// One executable, versioned user-behavior scenario.
/// 一个可执行的版本化用户行为场景。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    /// Protocol version; only version 1 is accepted by PR1.
    /// 协议版本，PR1 仅接受版本 1。
    pub schema: u32,
    /// Stable scenario name.
    /// 稳定场景名。
    pub name: String,
    /// Caller-defined mount identifier used for diagnostics, never registry lookup.
    /// 调用方定义的 mount 标识，仅用于诊断，绝不用于注册表查找。
    pub mount: String,
    /// Deterministic execution environment.
    /// 确定性执行环境。
    pub environment: Environment,
    /// Ordered input, assertion, frame, and capture actions.
    /// 有序输入、断言、帧推进与捕获动作。
    pub steps: Vec<Step>,
}

/// Text assertion addressed by role and accessible name.
/// 通过角色与无障碍名称定位的文本断言。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextAssertion {
    /// Semantic target.
    /// 语义目标。
    #[serde(flatten)]
    pub selector: Selector,
    /// Exact expected descendant text.
    /// 精确期望的后代文本。
    pub value: String,
}

/// Backend-neutral scenario action.
/// 后端无关场景动作。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// Assert exact text on a semantic target.
    /// 断言语义目标的精确文本。
    AssertText(TextAssertion),
    /// Click a semantic target.
    /// 点击语义目标。
    Click(Selector),
    /// Advance deterministic virtual frames.
    /// 推进确定性虚拟帧。
    AdvanceFrames(u64),
    /// Capture explicitly named snapshot capabilities.
    /// 捕获明确指定的快照能力。
    Capture(Vec<SnapshotKind>),
}

impl Step {
    fn action(&self) -> &'static str {
        match self {
            Self::AssertText(_) => "assert_text",
            Self::Click(_) => "click",
            Self::AdvanceFrames(_) => "advance_frames",
            Self::Capture(_) => "capture",
        }
    }
}

/// Pass/fail state shared by reports and individual steps.
/// 报告与单独步骤共用的通过／失败状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// All required checks passed.
    /// 所有必需检查通过。
    Passed,
    /// At least one required check failed.
    /// 至少一个必需检查失败。
    Failed,
}

/// One scenario step outcome.
/// 单个场景步骤结果。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StepResult {
    /// Zero-based step index.
    /// 从 0 开始的步骤索引。
    pub index: usize,
    /// Stable action name.
    /// 稳定动作名。
    pub action: String,
    /// Step status.
    /// 步骤状态。
    pub status: Status,
    /// Structured failure, if any.
    /// 可选结构化失败。
    pub error: Option<ToolError>,
}

/// Core artifacts written for every completed scenario run.
/// 每次完成场景运行都会写入的核心产物。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SnapshotArtifacts {
    /// Logical DOM artifact path relative to `run_dir`.
    /// 相对 `run_dir` 的逻辑 DOM 产物路径。
    pub dom: String,
    /// Accessibility artifact path relative to `run_dir`.
    /// 相对 `run_dir` 的无障碍产物路径。
    pub a11y: String,
    /// Reactive artifact path relative to `run_dir`.
    /// 相对 `run_dir` 的响应式产物路径。
    pub reactive: String,
    /// Trace artifact path relative to `run_dir`.
    /// 相对 `run_dir` 的 trace 产物路径。
    pub trace: String,
}

/// Stable machine-readable scenario result saved as `result.json`.
/// 保存为 `result.json` 的稳定机器可读场景结果。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScenarioResult {
    /// Result schema version.
    /// 结果 schema 版本。
    pub schema: u32,
    /// Content-derived stable run identifier.
    /// 由内容派生的稳定运行标识。
    pub run_id: String,
    /// Artifact directory supplied by the caller.
    /// 调用方提供的产物目录。
    pub run_dir: String,
    /// Scenario name.
    /// 场景名。
    pub name: String,
    /// Selected backend.
    /// 所选后端。
    pub backend: String,
    /// Overall run status.
    /// 总体运行状态。
    pub status: Status,
    /// Ordered step outcomes.
    /// 有序步骤结果。
    pub steps: Vec<StepResult>,
    /// Core snapshot artifact paths.
    /// 核心快照产物路径。
    pub snapshots: SnapshotArtifacts,
    /// Acceptance counters.
    /// 验收计数。
    pub counters: ExecutionCounters,
    /// Structured failures in execution order.
    /// 按执行顺序排列的结构化失败。
    pub errors: Vec<ToolError>,
}

/// Loads and validates JSON syntax for a scenario.
/// 加载场景并验证 JSON 语法。
pub fn load_scenario(path: &Path) -> Result<Scenario, ToolError> {
    let source = fs::read_to_string(path).map_err(|error| {
        ToolError::new(
            "UI_SCENARIO_IO",
            format!("failed to read {}: {error}", path.display()),
        )
    })?;
    serde_json::from_str(&source).map_err(|error| {
        ToolError::new(
            "UI_SCENARIO_INVALID_JSON",
            format!("failed to parse {}: {error}", path.display()),
        )
    })
}

/// Runs the JSON protocol on a host using only the mount supplied by the caller.
/// 仅使用调用方提供的 mount，在 Host 上运行 JSON 协议。
pub fn run_on_host<B: HeadlessBackend + 'static>(
    scenario: &Scenario,
    run_dir: &Path,
    mut host: HeadlessHost<B>,
    mount: impl FnOnce(&mut Runtime<B>, &Owner) -> B::Node,
) -> Result<ScenarioResult, ToolError> {
    validate_scenario(scenario)?;
    let backend = host.runtime().backend().backend_name();
    if host.environment() != &scenario.environment {
        return Err(ToolError::new(
            "UI_SCENARIO_ENVIRONMENT_MISMATCH",
            "host environment does not match the scenario",
        ));
    }
    let run_id = stable_run_id(scenario, backend)?;
    fs::create_dir_all(run_dir).map_err(|error| {
        ToolError::new(
            "UI_ARTIFACT_IO",
            format!("failed to create {}: {error}", run_dir.display()),
        )
    })?;

    host.mount(&scenario.mount, mount);
    let mut steps = Vec::new();
    let mut errors = Vec::new();

    for (index, step) in scenario.steps.iter().enumerate() {
        let outcome = execute_step(&mut host, step, index, backend);
        match outcome {
            Ok(()) => steps.push(StepResult {
                index,
                action: step.action().to_owned(),
                status: Status::Passed,
                error: None,
            }),
            Err(error) => {
                host.record_error(&error.code, &error.message);
                steps.push(StepResult {
                    index,
                    action: step.action().to_owned(),
                    status: Status::Failed,
                    error: Some(error.clone()),
                });
                errors.push(error);
                break;
            }
        }
    }

    let snapshots = SnapshotArtifacts {
        dom: SnapshotKind::Dom.artifact_name().to_owned(),
        a11y: SnapshotKind::A11y.artifact_name().to_owned(),
        reactive: SnapshotKind::Reactive.artifact_name().to_owned(),
        trace: SnapshotKind::Trace.artifact_name().to_owned(),
    };
    write_json(&run_dir.join(&snapshots.dom), &host.dom_snapshot())?;
    write_json(&run_dir.join(&snapshots.a11y), &host.a11y_snapshot())?;
    write_json(
        &run_dir.join(&snapshots.reactive),
        &host.reactive_snapshot(),
    )?;
    write_json(&run_dir.join(&snapshots.trace), &host.trace_snapshot())?;

    let result = ScenarioResult {
        schema: SCENARIO_SCHEMA,
        run_id,
        run_dir: run_dir.to_string_lossy().into_owned(),
        name: scenario.name.clone(),
        backend: backend.to_owned(),
        status: if errors.is_empty() {
            Status::Passed
        } else {
            Status::Failed
        },
        steps,
        snapshots,
        counters: host.execution_counters(),
        errors,
    };
    write_json(&run_dir.join("result.json"), &result)?;
    Ok(result)
}

fn execute_step<B: HeadlessBackend + 'static>(
    host: &mut HeadlessHost<B>,
    step: &Step,
    index: usize,
    backend: &str,
) -> Result<(), ToolError> {
    match step {
        Step::AssertText(assertion) => host
            .assert_text(&assertion.selector, &assertion.value)
            .map_err(|error| ToolError::from_headless(error, index, backend)),
        Step::Click(selector) => host
            .click(selector)
            .map_err(|error| ToolError::from_headless(error, index, backend)),
        Step::AdvanceFrames(frames) => host
            .advance_frames(*frames)
            .map_err(|error| ToolError::from_headless(error, index, backend)),
        Step::Capture(kinds) => {
            for kind in kinds {
                if !host.runtime().backend().supports_snapshot(*kind) {
                    let capability = kind
                        .artifact_name()
                        .strip_suffix(".json")
                        .expect("artifact suffix")
                        .to_owned();
                    return Err(ToolError::new(
                        "UI_SNAPSHOT_UNSUPPORTED",
                        format!("backend {backend} does not support {capability} snapshots"),
                    )
                    .at_step(index, backend)
                    .with_capability(capability));
                }
                host.record_capture(*kind);
            }
            Ok(())
        }
    }
}

fn validate_scenario(scenario: &Scenario) -> Result<(), ToolError> {
    if scenario.schema != SCENARIO_SCHEMA {
        return Err(ToolError::new(
            "UI_SCENARIO_SCHEMA_UNSUPPORTED",
            format!(
                "scenario schema {} is unsupported; expected {SCENARIO_SCHEMA}",
                scenario.schema
            ),
        ));
    }
    if scenario.mount.trim().is_empty() {
        return Err(ToolError::new(
            "UI_SCENARIO_INVALID_MOUNT",
            "scenario mount identifier must not be empty",
        ));
    }
    if scenario.environment.viewport.contains(&0)
        || !scenario.environment.dpr.is_finite()
        || scenario.environment.dpr <= 0.0
    {
        return Err(ToolError::new(
            "UI_SCENARIO_INVALID_ENVIRONMENT",
            "viewport dimensions and DPR must be positive and finite",
        ));
    }
    Ok(())
}

fn stable_run_id(scenario: &Scenario, backend: &str) -> Result<String, ToolError> {
    let canonical = serde_json::to_vec(scenario).map_err(|error| {
        ToolError::new(
            "UI_SCENARIO_SERIALIZATION",
            format!("failed to canonicalize scenario: {error}"),
        )
    })?;
    let mut hasher = Sha256::new();
    hasher.update(&canonical);
    hasher.update([0]);
    hasher.update(backend.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::new();
    for byte in digest.iter().take(8) {
        write!(&mut hex, "{byte:02x}").expect("writing to a string cannot fail");
    }
    let name: String = scenario
        .name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let name = name.trim_matches('-');
    Ok(format!(
        "{}-{hex}",
        if name.is_empty() { "scenario" } else { name }
    ))
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), ToolError> {
    let mut json = serde_json::to_vec_pretty(value).map_err(|error| {
        ToolError::new(
            "UI_ARTIFACT_SERIALIZATION",
            format!("failed to serialize {}: {error}", path.display()),
        )
    })?;
    json.push(b'\n');
    fs::write(path, json).map_err(|error| {
        ToolError::new(
            "UI_ARTIFACT_IO",
            format!("failed to write {}: {error}", path.display()),
        )
    })
}
