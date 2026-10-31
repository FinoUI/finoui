//! Versioned backend-neutral JSON scenarios with an explicit caller-supplied mount.
//! 使用调用方显式提供的 mount，执行版本化后端无关 JSON 场景。

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use crate::error::ToolError;
use crate::{
    dom_api::Selector,
    headless::{Environment, ExecutionCounters, HeadlessBackend, HeadlessHost, SnapshotKind},
    metrics::RuntimeStats,
    reactive::Owner,
    runtime::Runtime,
};

/// Current scenario protocol version.
/// 当前场景协议版本。
pub const SCENARIO_SCHEMA: u32 = 1;

/// Artifacts required at the end of every completed scenario run.
/// 每次完成的场景运行都需要的最终产物。
pub const REQUIRED_SNAPSHOTS: [SnapshotKind; 4] = [
    SnapshotKind::Dom,
    SnapshotKind::A11y,
    SnapshotKind::Reactive,
    SnapshotKind::Trace,
];

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
    /// Advance deterministic virtual frames and time.
    /// 推进确定性虚拟帧与虚拟时间。
    AdvanceFrames(u64),
    /// Capture and immediately persist explicitly named snapshots.
    /// 捕获并立即持久化明确指定的快照。
    Capture(Vec<SnapshotKind>),
}

impl Step {
    /// Supplies the protocol action label stored in ordered step results.
    /// 提供写入有序步骤结果的协议动作名。
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

/// Immediately persisted artifacts from one capture step.
/// 单个捕获步骤立即持久化的产物。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CaptureArtifacts {
    /// Zero-based capture step index.
    /// 从 0 开始的捕获步骤索引。
    pub step: usize,
    /// Capability names mapped to immutable paths relative to `run_dir`.
    /// 能力名映射到相对 `run_dir` 的不可覆盖路径。
    pub artifacts: BTreeMap<String, String>,
}

/// Core artifacts written at the end of a completed scenario run.
/// 场景运行结束时写入的核心产物。
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
    /// Point-in-time capture artifacts in step order.
    /// 按步骤顺序排列的时点捕获产物。
    pub captures: Vec<CaptureArtifacts>,
    /// Final snapshot artifact paths.
    /// 最终快照产物路径。
    pub snapshots: SnapshotArtifacts,
    /// Acceptance counters.
    /// 验收计数。
    pub counters: ExecutionCounters,
    /// Unified runtime counters shared by every conformance backend.
    ///
    /// Older schema-1 artifacts predate this field, so deserialization falls
    /// back to zero counters instead of rejecting a valid old result.
    /// 每个一致性后端共享的统一运行时计数。
    ///
    /// 旧 schema-1 产物没有该字段，因此反序列化回退到零计数，而不是拒绝旧结果。
    #[serde(default)]
    pub runtime_stats: RuntimeStats,
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
///
/// ```
/// use fino::{
///     backend_mock::{MockBackend, MockNode},
///     dom_api::DomBackend,
///     headless::{Environment, HeadlessHost, SnapshotKind},
///     reactive::Owner,
///     runtime::Runtime,
///     scenario::{Scenario, Status, run_on_host},
/// };
///
/// fn mount(runtime: &mut Runtime<MockBackend>, _owner: &Owner) -> MockNode {
///     runtime.backend_mut().create_text("ready")
/// }
///
/// let scenario = Scenario {
///     schema: 1,
///     name: "doctest/mount".to_owned(),
///     mount: "doctest".to_owned(),
///     environment: Environment::default(),
///     steps: vec![],
/// };
/// let run_dir = std::env::temp_dir().join(format!(
///     "fino-scenario-doctest-{}",
///     std::process::id()
/// ));
/// let host = HeadlessHost::new(MockBackend::new(), Environment::default());
/// let result = run_on_host(&scenario, &run_dir, host, mount).expect("scenario run");
/// assert_eq!(result.status, Status::Passed);
/// assert!(run_dir.join(SnapshotKind::Dom.artifact_name()).exists());
/// std::fs::remove_dir_all(run_dir).expect("cleanup");
/// ```
pub fn run_on_host<B: HeadlessBackend + 'static>(
    scenario: &Scenario,
    run_dir: &Path,
    host: HeadlessHost<B>,
    mount: impl FnOnce(&mut Runtime<B>, &Owner) -> B::Node,
) -> Result<ScenarioResult, ToolError> {
    let mut sink = FsArtifactSink::new(run_dir);
    execute_on_host(scenario, run_dir, host, mount, &mut sink).map(|run| run.result)
}

/// One completed scenario execution with artifacts held in memory.
/// 一次完成的场景执行，产物保存在内存中。
#[derive(Clone, Debug)]
pub struct ScenarioRun {
    /// Stable machine-readable scenario result.
    /// 稳定的机器可读场景结果。
    pub result: ScenarioResult,
    /// Artifact bytes keyed by path relative to the run directory.
    /// 以相对运行目录的路径为键的产物字节。
    pub artifacts: BTreeMap<String, Vec<u8>>,
}

/// Runs the JSON protocol without touching the filesystem and returns every artifact.
/// 不接触文件系统地运行 JSON 协议，并返回全部产物。
///
/// This is the entry point for hosts without a filesystem, such as a wasm module in
/// a browser. The caller decides where to persist the returned bytes.
/// 这是无文件系统 Host（例如浏览器中的 wasm 模块）的入口；返回的字节由调用方决定写到哪里。
pub fn run_on_host_collect<B: HeadlessBackend + 'static>(
    scenario: &Scenario,
    run_dir: &Path,
    host: HeadlessHost<B>,
    mount: impl FnOnce(&mut Runtime<B>, &Owner) -> B::Node,
) -> Result<ScenarioRun, ToolError> {
    let mut sink = MemoryArtifactSink::default();
    execute_on_host(scenario, run_dir, host, mount, &mut sink)
}

/// Executes the protocol into an artifact sink shared by filesystem and browser hosts.
/// 将协议执行到文件系统与浏览器 Host 共用的产物接收器。
fn execute_on_host<B: HeadlessBackend + 'static>(
    scenario: &Scenario,
    run_dir: &Path,
    mut host: HeadlessHost<B>,
    mount: impl FnOnce(&mut Runtime<B>, &Owner) -> B::Node,
    sink: &mut impl ArtifactSink,
) -> Result<ScenarioRun, ToolError> {
    validate_scenario(scenario)?;
    let backend = host.runtime().backend().backend_name();
    if host.environment() != &scenario.environment {
        return Err(ToolError::new(
            "UI_SCENARIO_ENVIRONMENT_MISMATCH",
            "host environment does not match the scenario",
        ));
    }
    let run_id = stable_run_id(scenario, backend)?;
    sink.prepare()?;

    host.mount(&scenario.mount, mount);
    let mut steps = Vec::new();
    let mut captures = Vec::new();
    let mut errors = Vec::new();

    for (index, step) in scenario.steps.iter().enumerate() {
        match execute_step(&mut host, step, index, backend, sink) {
            Ok(capture) => {
                if let Some(capture) = capture {
                    captures.push(capture);
                }
                steps.push(StepResult {
                    index,
                    action: step.action().to_owned(),
                    status: Status::Passed,
                    error: None,
                });
            }
            Err(error) => {
                record_error(&mut host, &mut errors, error.clone());
                steps.push(StepResult {
                    index,
                    action: step.action().to_owned(),
                    status: Status::Failed,
                    error: Some(error),
                });
                break;
            }
        }
    }

    for reactive_error in host.reactive_snapshot().errors {
        record_error(
            &mut host,
            &mut errors,
            ToolError::from_reactive(reactive_error, backend),
        );
    }

    let snapshots = SnapshotArtifacts {
        dom: SnapshotKind::Dom.artifact_name().to_owned(),
        a11y: SnapshotKind::A11y.artifact_name().to_owned(),
        reactive: SnapshotKind::Reactive.artifact_name().to_owned(),
        trace: SnapshotKind::Trace.artifact_name().to_owned(),
    };
    for kind in REQUIRED_SNAPSHOTS {
        match ensure_snapshot_supported(&host, kind, None, backend) {
            Ok(()) => sink.write(kind.artifact_name(), &snapshot_bytes(&mut host, kind)?)?,
            Err(error) => {
                // Remove only this kind's managed artifact so a reused directory cannot
                // expose a previous run's snapshot as the current result.
                // 只删除该能力对应的受管产物，避免复用的目录把上一次运行的快照当成当前结果。
                sink.remove(kind.artifact_name())?;
                record_error(&mut host, &mut errors, error);
            }
        }
    }

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
        captures,
        snapshots,
        counters: host.execution_counters(),
        runtime_stats: host.runtime_stats(),
        errors,
    };
    sink.write("result.json", &json_bytes(&result, "result.json")?)?;
    // Every invocation owns one host and one root. Dispose after the final
    // snapshots and result exist so repeated browser runs cannot accumulate
    // previous scenario roots; owner cleanup also runs live probes before the
    // root is detached.
    // 每次调用拥有一个 Host 与一个根节点。在最终快照与结果生成后释放，使重复
    // 浏览器运行不会累积旧场景根节点；Owner 清理还会在根节点分离前运行实时
    // 探针。
    host.dispose();
    Ok(ScenarioRun {
        result,
        artifacts: sink.artifacts().clone(),
    })
}

/// Destination for scenario artifacts, backed by a filesystem or by memory.
/// 场景产物目的地的抽象，由文件系统或内存实现。
trait ArtifactSink {
    /// Creates the run directory, if the sink has one.
    /// 若接收器有运行目录，则创建它。
    fn prepare(&mut self) -> Result<(), ToolError>;
    /// Creates one directory for point-in-time captures.
    /// 为时点捕获创建一个目录。
    fn create_dir(&mut self, relative: &str) -> Result<(), ToolError>;
    /// Writes one artifact and records its bytes.
    /// 写入一个产物并记录其字节。
    fn write(&mut self, relative: &str, bytes: &[u8]) -> Result<(), ToolError>;
    /// Removes one stale managed artifact.
    /// 删除一个过期的受管产物。
    fn remove(&mut self, relative: &str) -> Result<(), ToolError>;
    /// Returns every written artifact in path order.
    /// 按路径顺序返回全部已写入产物。
    fn artifacts(&self) -> &BTreeMap<String, Vec<u8>>;
}

/// Filesystem sink used by native headless runs.
/// 原生无头运行使用的文件系统接收器。
struct FsArtifactSink {
    root: PathBuf,
    artifacts: BTreeMap<String, Vec<u8>>,
}

impl FsArtifactSink {
    /// Creates a filesystem sink rooted at the caller-supplied run directory.
    /// 创建以调用方运行目录为根的文件系统接收器。
    fn new(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
            artifacts: BTreeMap::new(),
        }
    }

    /// Joins one managed relative artifact path to the run directory.
    /// 把一个受管相对产物路径拼接到运行目录。
    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }
}

impl ArtifactSink for FsArtifactSink {
    /// Creates the run directory before any scenario work begins.
    /// 在任何场景工作开始前创建运行目录。
    fn prepare(&mut self) -> Result<(), ToolError> {
        fs::create_dir_all(&self.root).map_err(|error| {
            ToolError::new(
                "UI_ARTIFACT_IO",
                format!("failed to create {}: {error}", self.root.display()),
            )
        })
    }

    /// Creates one capture directory on disk.
    /// 在磁盘上创建一个捕获目录。
    fn create_dir(&mut self, relative: &str) -> Result<(), ToolError> {
        let path = self.path(relative);
        fs::create_dir_all(&path).map_err(|error| {
            ToolError::new(
                "UI_ARTIFACT_IO",
                format!("failed to create {}: {error}", path.display()),
            )
        })
    }

    /// Writes bytes and records the same bytes for caller inspection.
    /// 写入字节，并记录相同字节供调用方检查。
    fn write(&mut self, relative: &str, bytes: &[u8]) -> Result<(), ToolError> {
        let path = self.path(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                ToolError::new(
                    "UI_ARTIFACT_IO",
                    format!("failed to create {}: {error}", parent.display()),
                )
            })?;
        }
        fs::write(&path, bytes).map_err(|error| {
            ToolError::new(
                "UI_ARTIFACT_IO",
                format!("failed to write {}: {error}", path.display()),
            )
        })?;
        self.artifacts.insert(relative.to_owned(), bytes.to_vec());
        Ok(())
    }

    /// Removes one stale artifact while treating absence as success.
    /// 删除一个过期产物，并把不存在视为成功。
    fn remove(&mut self, relative: &str) -> Result<(), ToolError> {
        let path = self.path(relative);
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(ToolError::new(
                    "UI_ARTIFACT_IO",
                    format!("failed to remove {}: {error}", path.display()),
                ));
            }
        }
        self.artifacts.remove(relative);
        Ok(())
    }

    /// Returns all artifacts written so far in stable path order.
    /// 按稳定路径顺序返回目前写入的全部产物。
    fn artifacts(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.artifacts
    }
}

/// In-memory sink used where no filesystem exists, such as wasm in a browser.
/// 在无文件系统环境（例如浏览器中的 wasm）使用的内存接收器。
#[derive(Default)]
struct MemoryArtifactSink {
    artifacts: BTreeMap<String, Vec<u8>>,
}

impl ArtifactSink for MemoryArtifactSink {
    /// Has no directory to create before an in-memory run.
    /// 内存运行前没有需要创建的目录。
    fn prepare(&mut self) -> Result<(), ToolError> {
        Ok(())
    }

    /// Ignores capture directories because memory has no directory structure.
    /// 内存没有目录结构，因此忽略捕获目录。
    fn create_dir(&mut self, _relative: &str) -> Result<(), ToolError> {
        Ok(())
    }

    /// Stores artifact bytes under their managed relative path.
    /// 按受管相对路径保存产物字节。
    fn write(&mut self, relative: &str, bytes: &[u8]) -> Result<(), ToolError> {
        self.artifacts.insert(relative.to_owned(), bytes.to_vec());
        Ok(())
    }

    /// Drops one managed artifact from memory.
    /// 从内存中删除一个受管产物。
    fn remove(&mut self, relative: &str) -> Result<(), ToolError> {
        self.artifacts.remove(relative);
        Ok(())
    }

    /// Returns all artifacts written so far in stable path order.
    /// 按稳定路径顺序返回目前写入的全部产物。
    fn artifacts(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.artifacts
    }
}

/// Executes one action and returns artifact metadata only for capture steps.
/// 执行单个动作，并仅为 capture 步骤返回产物元数据。
fn execute_step<B: HeadlessBackend + 'static>(
    host: &mut HeadlessHost<B>,
    step: &Step,
    index: usize,
    backend: &str,
    sink: &mut impl ArtifactSink,
) -> Result<Option<CaptureArtifacts>, ToolError> {
    match step {
        Step::AssertText(assertion) => host
            .assert_text(&assertion.selector, &assertion.value)
            .map_err(|error| ToolError::from_headless(error, index, backend))
            .map(|_| None),
        Step::Click(selector) => host
            .click(selector)
            .map_err(|error| ToolError::from_headless(error, index, backend))
            .map(|_| None),
        Step::AdvanceFrames(frames) => host
            .advance_frames(*frames)
            .map_err(|error| ToolError::from_headless(error, index, backend))
            .map(|_| None),
        Step::Capture(kinds) => capture_step(host, kinds, index, backend, sink).map(Some),
    }
}

/// Validates every requested capability before persisting a point-in-time capture.
/// 先验证本步全部能力，再持久化时点快照，避免部分空产物。
fn capture_step<B: HeadlessBackend + 'static>(
    host: &mut HeadlessHost<B>,
    kinds: &[SnapshotKind],
    index: usize,
    backend: &str,
    sink: &mut impl ArtifactSink,
) -> Result<CaptureArtifacts, ToolError> {
    for kind in kinds {
        ensure_snapshot_supported(host, *kind, Some(index), backend)?;
    }

    let capture_dir = format!("captures/step-{index}");
    sink.create_dir(&capture_dir)
        .map_err(|error| error.at_step(index, backend))?;
    let mut artifacts = BTreeMap::new();
    for kind in kinds {
        let relative = format!("{capture_dir}/{}", kind.artifact_name());
        host.record_capture(*kind, &relative);
        sink.write(&relative, &snapshot_bytes(host, *kind)?)?;
        artifacts.insert(kind.name().to_owned(), relative);
    }
    Ok(CaptureArtifacts {
        step: index,
        artifacts,
    })
}

/// Rejects unavailable snapshot generation with backend, capability, and step context.
/// 拒绝不可用的快照生成，并保留后端、能力与步骤上下文。
fn ensure_snapshot_supported<B: HeadlessBackend + 'static>(
    host: &HeadlessHost<B>,
    kind: SnapshotKind,
    step: Option<usize>,
    backend: &str,
) -> Result<(), ToolError> {
    let supported = host.runtime().backend().supports_snapshot(kind);
    if supported {
        return Ok(());
    }

    let capability = kind.name().to_owned();
    let error = ToolError::new(
        "UI_SNAPSHOT_UNSUPPORTED",
        format!("backend {backend} does not support {capability} snapshots"),
    )
    .with_capability(capability);
    Err(match step {
        Some(step) => error.at_step(step, backend),
        None => error.at_backend(backend),
    })
}

/// Serializes only snapshot kinds that have real host or backend data sources.
/// 仅序列化具有真实 Host 或后端数据来源的快照种类。
fn snapshot_bytes<B: HeadlessBackend + 'static>(
    host: &mut HeadlessHost<B>,
    kind: SnapshotKind,
) -> Result<Vec<u8>, ToolError> {
    match kind {
        SnapshotKind::Dom => json_bytes(&host.dom_snapshot(), kind.artifact_name()),
        SnapshotKind::A11y => json_bytes(&host.a11y_snapshot(), kind.artifact_name()),
        SnapshotKind::Reactive => json_bytes(&host.reactive_snapshot(), kind.artifact_name()),
        SnapshotKind::Trace => json_bytes(&host.trace_snapshot(), kind.artifact_name()),
        SnapshotKind::Layout => host
            .runtime_mut()
            .backend_mut()
            .layout_snapshot()
            .ok_or_else(|| {
                ToolError::new(
                    "UI_SNAPSHOT_UNSUPPORTED",
                    "backend cannot generate layout snapshots",
                )
            })
            .and_then(|value| json_bytes(&value, kind.artifact_name())),
        SnapshotKind::DisplayList => host
            .runtime_mut()
            .backend_mut()
            .display_list_snapshot()
            .ok_or_else(|| {
                ToolError::new(
                    "UI_SNAPSHOT_UNSUPPORTED",
                    "backend cannot generate display_list snapshots",
                )
            })
            .and_then(|value| json_bytes(&value, kind.artifact_name())),
        _ => Err(ToolError::new(
            "UI_SNAPSHOT_UNSUPPORTED",
            format!("backend cannot generate {} snapshots", kind.name()),
        )),
    }
}

/// Adds a unique failure to both the run result and trace without duplicating diagnostics.
/// 将唯一错误同时加入运行结果与 trace，避免重复诊断。
fn record_error<B: HeadlessBackend + 'static>(
    host: &mut HeadlessHost<B>,
    errors: &mut Vec<ToolError>,
    error: ToolError,
) {
    let duplicate = errors.iter().any(|existing| {
        existing.code == error.code
            && existing.message == error.message
            && existing.step == error.step
            && existing.capability == error.capability
    });
    if duplicate {
        return;
    }
    host.record_error(&error.code, &error.message);
    errors.push(error);
}

/// Rejects unsupported protocol and nondeterministic environment values before mounting.
/// 在挂载前拒绝不支持的协议与不确定的环境配置。
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

/// Derives a reproducible run ID from canonical scenario content and backend identity.
/// 根据规范化场景内容与后端身份派生可复现 run ID，不包含时间戳。
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

/// Serializes stable pretty JSON with a trailing newline.
/// 序列化带结尾换行的稳定格式化 JSON。
fn json_bytes(value: &impl Serialize, label: &str) -> Result<Vec<u8>, ToolError> {
    let mut json = serde_json::to_vec_pretty(value).map_err(|error| {
        ToolError::new(
            "UI_ARTIFACT_SERIALIZATION",
            format!("failed to serialize {label}: {error}"),
        )
    })?;
    json.push(b'\n');
    Ok(json)
}
