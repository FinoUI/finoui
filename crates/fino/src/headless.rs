//! Deterministic headless mounting, semantic input, snapshots, and run tracing.
//! 确定性的无头挂载、语义输入、快照与运行 trace。

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    dom_api::{DomBackend, Event, EventContext, EventKind, EventListener, Role, Selector},
    reactive::{Owner, ReactiveSnapshot},
    runtime::Runtime,
};

/// Deterministic environment shared by JSON scenarios and headless hosts.
/// JSON 场景与无头 Host 共享的确定性环境。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    /// Logical viewport width and height.
    /// 逻辑视口宽高。
    pub viewport: [u32; 2],
    /// Device pixel ratio.
    /// 设备像素比。
    pub dpr: f64,
    /// Locale identifier.
    /// 区域设置标识。
    pub locale: String,
    /// Requested color scheme.
    /// 请求的颜色主题。
    pub color_scheme: ColorScheme,
    /// Deterministic random seed reserved for scenario inputs.
    /// 为场景输入保留的确定性随机种子。
    pub seed: u64,
}

impl Default for Environment {
    fn default() -> Self {
        Self {
            viewport: [800, 600],
            dpr: 1.0,
            locale: "zh-CN".to_owned(),
            color_scheme: ColorScheme::Dark,
            seed: 42,
        }
    }
}

/// Color schemes accepted by the PR1 scenario schema.
/// PR1 场景 schema 接受的颜色主题。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ColorScheme {
    /// Dark color scheme.
    /// 深色主题。
    Dark,
    /// Light color scheme.
    /// 浅色主题。
    Light,
}

/// Snapshot capabilities known to the versioned scenario protocol.
/// 版本化场景协议已知的快照能力。
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotKind {
    /// Logical DOM tree.
    /// 逻辑 DOM 树。
    Dom,
    /// Accessibility tree.
    /// 无障碍树。
    A11y,
    /// Reactive graph diagnostics.
    /// 响应式图诊断。
    Reactive,
    /// Layout diagnostics.
    /// 布局诊断。
    Layout,
    /// Display list.
    /// 绘制列表。
    DisplayList,
    /// Rendered image.
    /// 渲染图像。
    Screenshot,
    /// Resource diagnostics.
    /// 资源诊断。
    Resources,
    /// Structured run trace.
    /// 结构化运行 trace。
    Trace,
}

impl SnapshotKind {
    /// Returns the stable capability name used in paths and diagnostics.
    /// 返回路径与诊断使用的稳定能力名。
    pub fn name(self) -> &'static str {
        match self {
            Self::Dom => "dom",
            Self::A11y => "a11y",
            Self::Reactive => "reactive",
            Self::Layout => "layout",
            Self::DisplayList => "display_list",
            Self::Screenshot => "screenshot",
            Self::Resources => "resources",
            Self::Trace => "trace",
        }
    }

    /// Returns the stable artifact filename.
    /// 返回稳定的产物文件名。
    pub fn artifact_name(self) -> &'static str {
        match self {
            Self::Dom => "dom.json",
            Self::A11y => "a11y.json",
            Self::Reactive => "reactive.json",
            Self::Layout => "layout.json",
            Self::DisplayList => "display_list.json",
            Self::Screenshot => "screenshot.json",
            Self::Resources => "resources.json",
            Self::Trace => "trace.json",
        }
    }
}

/// Backend-neutral structured backend operation used in traces.
/// trace 使用的后端无关结构化后端操作。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BackendOperation {
    /// Stable operation name.
    /// 稳定操作名。
    pub operation: String,
    /// Operation-specific JSON data.
    /// 操作特定的 JSON 数据。
    pub details: Value,
}

/// DOM node kind in a logical snapshot.
/// 逻辑快照中的 DOM 节点种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DomNodeKind {
    /// Element node.
    /// 元素节点。
    Element,
    /// Text node.
    /// 文本节点。
    Text,
}

/// Stable logical DOM node without addresses or platform handles.
/// 不含地址或平台句柄的稳定逻辑 DOM 节点。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DomNodeSnapshot {
    /// Stable creation-order semantic identifier.
    /// 按创建顺序生成的稳定语义标识。
    pub id: String,
    /// Node kind.
    /// 节点种类。
    pub kind: DomNodeKind,
    /// Parent semantic identifier.
    /// 父节点语义标识。
    pub parent: Option<String>,
    /// Children in committed order.
    /// 按提交顺序排列的子节点。
    pub children: Vec<String>,
    /// Element tag, when applicable.
    /// 适用时的元素标签。
    pub tag: Option<String>,
    /// Text content for text nodes.
    /// 文本节点的文本内容。
    pub text: Option<String>,
    /// Stable attribute mapping.
    /// 稳定属性映射。
    pub attributes: BTreeMap<String, String>,
}

/// Versioned logical DOM snapshot.
/// 版本化逻辑 DOM 快照。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DomSnapshot {
    /// Snapshot schema version.
    /// 快照 schema 版本。
    pub schema: u32,
    /// Detached or mounted roots in stable order.
    /// 按稳定顺序排列的分离或已挂载根节点。
    pub roots: Vec<String>,
    /// Nodes in stable creation order.
    /// 按稳定创建顺序排列的节点。
    pub nodes: Vec<DomNodeSnapshot>,
}

/// Accessible node state used by semantic assertions.
/// 语义断言使用的无障碍节点状态。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct A11yNodeSnapshot {
    /// Stable semantic node identifier.
    /// 稳定语义节点标识。
    pub id: String,
    /// Accessibility role.
    /// 无障碍角色。
    pub role: Role,
    /// Accessible name.
    /// 无障碍名称。
    pub name: String,
    /// Whether the node can receive focus.
    /// 节点是否可获得焦点。
    pub focusable: bool,
    /// Supported semantic actions.
    /// 支持的语义动作。
    pub actions: Vec<String>,
    /// Stable accessibility states.
    /// 稳定无障碍状态。
    pub states: BTreeMap<String, String>,
    /// Current descendant text used by assertions.
    /// 断言使用的当前后代文本。
    pub text: String,
}

/// Versioned accessibility snapshot.
/// 版本化无障碍快照。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct A11ySnapshot {
    /// Snapshot schema version.
    /// 快照 schema 版本。
    pub schema: u32,
    /// Accessible nodes in stable creation order.
    /// 按稳定创建顺序排列的无障碍节点。
    pub nodes: Vec<A11yNodeSnapshot>,
}

/// One structured trace event with no wall-clock timestamp.
/// 不含实际时间戳的单条结构化 trace 事件。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TraceEvent {
    /// One-based deterministic sequence number.
    /// 从 1 开始的确定性序号。
    pub sequence: u64,
    /// Stable event kind.
    /// 稳定事件种类。
    pub kind: String,
    /// Structured event data.
    /// 结构化事件数据。
    pub details: Value,
}

/// Versioned run trace.
/// 版本化运行 trace。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TraceSnapshot {
    /// Snapshot schema version.
    /// 快照 schema 版本。
    pub schema: u32,
    /// Events in deterministic execution order.
    /// 按确定性执行顺序排列的事件。
    pub events: Vec<TraceEvent>,
}

/// Additional read and input operations needed by a headless backend.
/// 无头后端所需的额外读取与输入操作。
pub trait HeadlessBackend: DomBackend {
    /// Stable backend name used in results and traces.
    /// 结果与 trace 使用的稳定后端名。
    fn backend_name(&self) -> &'static str;
    /// Reports snapshot support without manufacturing empty artifacts.
    /// 报告快照能力，不伪造空产物。
    fn supports_snapshot(&self, kind: SnapshotKind) -> bool;
    /// Finds all nodes matching role and accessible name.
    /// 查找所有符合角色与无障碍名称的节点。
    fn find_all(&self, selector: &Selector) -> Vec<Self::Node>;
    /// Returns concatenated descendant text.
    /// 返回拼接后的后代文本。
    fn text_content(&self, node: &Self::Node) -> Option<String>;
    /// Clones a registered listener for dispatch outside a backend borrow.
    /// 克隆已注册监听器，以便在后端借用之外分发。
    fn listener(&self, node: &Self::Node, event: EventKind) -> Option<EventListener>;
    /// Captures the logical DOM tree.
    /// 捕获逻辑 DOM 树。
    fn dom_snapshot(&self) -> DomSnapshot;
    /// Captures the accessibility tree.
    /// 捕获无障碍树。
    fn a11y_snapshot(&self) -> A11ySnapshot;
    /// Returns normalized structured operations in stable order.
    /// 按稳定顺序返回规范化结构化操作。
    fn operation_log(&self) -> Vec<BackendOperation>;
}

/// Structured headless action failure.
/// 结构化无头动作失败。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadlessError {
    /// Stable machine-readable code.
    /// 稳定机器可读错误码。
    pub code: String,
    /// Human-readable explanation.
    /// 供人阅读的错误说明。
    pub message: String,
}

impl HeadlessError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
        }
    }
}

/// Fixed deterministic frame duration used by the PR1 virtual clock.
/// PR1 虚拟时钟使用的固定确定性帧时长。
pub const FRAME_DURATION_MS: u64 = 16;

/// Minimum counters needed to detect accidental component or backend reruns.
/// 检测意外组件重跑或后端重跑所需的最小计数。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExecutionCounters {
    /// Manual component executions.
    /// 手工组件执行次数。
    pub component_executions: u64,
    /// Total effect callback executions.
    /// Effect 回调总执行次数。
    pub effect_executions: u64,
    /// Total backend operations.
    /// 后端总操作数。
    pub backend_operations: usize,
    /// Current virtual frame.
    /// 当前虚拟帧。
    pub frame: u64,
    /// Deterministic virtual time in milliseconds.
    /// 以毫秒表示的确定性虚拟时间。
    pub virtual_time_ms: u64,
}

/// A deterministic host that runs real reactive and backend code without a display.
/// 在无显示器环境中运行真实响应式与后端代码的确定性 Host。
pub struct HeadlessHost<B: HeadlessBackend> {
    runtime: Runtime<B>,
    owner: Owner,
    root: Option<B::Node>,
    environment: Environment,
    frame: u64,
    virtual_time_ms: u64,
    disposed: bool,
    trace: Vec<TraceEvent>,
    backend_trace_cursor: usize,
}

impl<B: HeadlessBackend + 'static> HeadlessHost<B> {
    /// Creates a host and its root owner with a fixed environment.
    /// 使用固定环境创建 Host 及其根 Owner。
    pub fn new(backend: B, environment: Environment) -> Self {
        let runtime = Runtime::new(backend);
        let owner = runtime.create_owner();
        Self {
            runtime,
            owner,
            root: None,
            environment,
            frame: 0,
            virtual_time_ms: 0,
            disposed: false,
            trace: Vec::new(),
            backend_trace_cursor: 0,
        }
    }

    /// Mounts the explicitly supplied component exactly once and records its caller-defined name.
    /// 只挂载一次显式提供的组件，并记录调用方定义的名称。
    pub fn mount(
        &mut self,
        mount_name: &str,
        mount: impl FnOnce(&mut Runtime<B>, &Owner) -> B::Node,
    ) -> B::Node {
        assert!(self.root.is_none(), "a PR1 host supports one mounted root");
        assert!(!self.disposed, "cannot mount a disposed host");
        self.record("mount", json!({ "component": mount_name }));
        let root = mount(&mut self.runtime, &self.owner);
        self.root = Some(root.clone());
        self.sync_backend_trace();
        root
    }

    /// Returns the runtime for focused backend and lifecycle tests.
    /// 返回 Runtime，用于聚焦的后端与生命周期测试。
    pub fn runtime(&self) -> &Runtime<B> {
        &self.runtime
    }

    /// Returns the root owner.
    /// 返回根 Owner。
    pub fn owner(&self) -> &Owner {
        &self.owner
    }

    /// Returns the fixed scenario environment.
    /// 返回固定场景环境。
    pub fn environment(&self) -> &Environment {
        &self.environment
    }

    /// Returns the current virtual frame number.
    /// 返回当前虚拟帧号。
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// Returns deterministic virtual time in milliseconds.
    /// 返回以毫秒表示的确定性虚拟时间。
    pub fn virtual_time_ms(&self) -> u64 {
        self.virtual_time_ms
    }

    /// Clicks one uniquely matching semantic node.
    /// 点击唯一匹配的语义节点。
    pub fn click(&mut self, selector: &Selector) -> Result<(), HeadlessError> {
        self.ensure_active()?;
        let node = self.find_one(selector)?;
        let listener = self
            .runtime
            .backend()
            .listener(&node, EventKind::Click)
            .ok_or_else(|| {
                HeadlessError::new("UI_EVENT_NOT_SUPPORTED", "no click listener is registered")
            })?;
        self.record(
            "event",
            json!({ "kind": "click", "role": selector.role, "name": selector.name }),
        );
        listener.call(
            Event {
                kind: EventKind::Click,
            },
            &mut EventContext,
        );
        self.sync_backend_trace();
        Ok(())
    }

    /// Asserts concatenated text through the same semantic selector used for clicks.
    /// 使用与点击相同的语义选择器断言拼接文本。
    pub fn assert_text(
        &mut self,
        selector: &Selector,
        expected: &str,
    ) -> Result<(), HeadlessError> {
        self.ensure_active()?;
        let node = self.find_one(selector)?;
        let actual = self.runtime.backend().text_content(&node).ok_or_else(|| {
            HeadlessError::new("UI_SELECTOR_INVALID", "selected node has no text")
        })?;
        let passed = actual == expected;
        self.record(
            "assert_text",
            json!({
                "role": selector.role,
                "name": selector.name,
                "expected": expected,
                "actual": actual,
                "passed": passed,
            }),
        );
        if passed {
            Ok(())
        } else {
            Err(HeadlessError::new(
                "UI_ASSERT_TEXT",
                format!("expected text {expected:?}, got {actual:?}"),
            ))
        }
    }

    /// Advances an explicit number of deterministic frames.
    /// 显式推进指定数量的确定性帧。
    pub fn advance_frames(&mut self, frames: u64) -> Result<(), HeadlessError> {
        self.ensure_active()?;
        let next_frame = self.frame.checked_add(frames).ok_or_else(|| {
            HeadlessError::new("UI_VIRTUAL_TIME_OVERFLOW", "virtual frame counter overflow")
        })?;
        let elapsed = frames.checked_mul(FRAME_DURATION_MS).ok_or_else(|| {
            HeadlessError::new(
                "UI_VIRTUAL_TIME_OVERFLOW",
                "virtual frame duration overflow",
            )
        })?;
        let next_time = self.virtual_time_ms.checked_add(elapsed).ok_or_else(|| {
            HeadlessError::new("UI_VIRTUAL_TIME_OVERFLOW", "virtual clock overflow")
        })?;
        self.frame = next_frame;
        self.virtual_time_ms = next_time;
        self.record(
            "advance_frames",
            json!({
                "frames": frames,
                "frame": self.frame,
                "virtual_time_ms": self.virtual_time_ms,
            }),
        );
        Ok(())
    }

    /// Disposes reactive resources and listeners before detaching the root.
    /// 先释放响应式资源与监听器，再分离根节点。
    pub fn dispose(&mut self) {
        if self.disposed {
            return;
        }
        self.record("dispose", json!({}));
        self.owner.dispose();
        if let Some(root) = self.root.take() {
            self.runtime.backend_mut().remove(&root);
        }
        self.disposed = true;
        self.sync_backend_trace();
    }

    /// Captures reactive diagnostics including the component execution count.
    /// 捕获包含组件执行计数的响应式诊断。
    pub fn reactive_snapshot(&self) -> ReactiveSnapshot {
        self.owner
            .snapshot_with_component_executions(self.runtime.component_executions().get())
    }

    /// Returns acceptance counters for the current host instance.
    /// 返回当前 Host 实例的验收计数。
    pub fn execution_counters(&self) -> ExecutionCounters {
        let reactive = self.reactive_snapshot();
        ExecutionCounters {
            component_executions: reactive.component_executions,
            effect_executions: reactive
                .effects
                .iter()
                .map(|effect| effect.executions)
                .sum(),
            backend_operations: self.backend_operation_count(),
            frame: self.frame,
            virtual_time_ms: self.virtual_time_ms,
        }
    }

    /// Captures the backend's logical DOM snapshot.
    /// 捕获后端的逻辑 DOM 快照。
    pub fn dom_snapshot(&self) -> DomSnapshot {
        self.runtime.backend().dom_snapshot()
    }

    /// Captures the backend's accessibility snapshot.
    /// 捕获后端的无障碍快照。
    pub fn a11y_snapshot(&self) -> A11ySnapshot {
        self.runtime.backend().a11y_snapshot()
    }

    /// Returns the normalized backend operation count.
    /// 返回规范化后端操作计数。
    pub fn backend_operation_count(&self) -> usize {
        self.runtime.backend().operation_log().len()
    }

    /// Captures the trace after flushing all observed backend operations.
    /// 刷新所有已观测后端操作后捕获 trace。
    pub fn trace_snapshot(&mut self) -> TraceSnapshot {
        self.sync_backend_trace();
        TraceSnapshot {
            schema: 1,
            events: self.trace.clone(),
        }
    }

    /// Records a structured capture action and its immutable relative artifact path.
    /// 记录结构化捕获动作及其不可覆盖的相对产物路径。
    pub fn record_capture(&mut self, kind: SnapshotKind, path: &str) {
        self.record("capture", json!({ "kind": kind, "path": path }));
    }

    /// Records a structured step failure before artifacts are written.
    /// 在写入产物前记录结构化步骤失败。
    pub fn record_error(&mut self, code: &str, message: &str) {
        self.record("error", json!({ "code": code, "message": message }));
    }

    fn find_one(&self, selector: &Selector) -> Result<B::Node, HeadlessError> {
        let matches = self.runtime.backend().find_all(selector);
        match matches.len() {
            0 => Err(HeadlessError::new(
                "UI_SELECTOR_NOT_FOUND",
                format!(
                    "no node matches role {:?} and name {:?}",
                    selector.role, selector.name
                ),
            )),
            1 => Ok(matches.into_iter().next().expect("one match")),
            count => Err(HeadlessError::new(
                "UI_SELECTOR_AMBIGUOUS",
                format!("{count} nodes match the semantic selector"),
            )),
        }
    }

    fn ensure_active(&self) -> Result<(), HeadlessError> {
        if self.disposed {
            Err(HeadlessError::new(
                "UI_HOST_DISPOSED",
                "the headless host has been disposed",
            ))
        } else {
            Ok(())
        }
    }

    fn sync_backend_trace(&mut self) {
        let operations = self.runtime.backend().operation_log();
        let backend = self.runtime.backend().backend_name();
        for operation in operations.iter().skip(self.backend_trace_cursor) {
            self.record(
                "backend",
                json!({
                    "backend": backend,
                    "operation": operation.operation,
                    "details": operation.details,
                }),
            );
        }
        self.backend_trace_cursor = operations.len();
    }

    fn record(&mut self, kind: &str, details: Value) {
        self.trace.push(TraceEvent {
            sequence: self.trace.len() as u64 + 1,
            kind: kind.to_owned(),
            details,
        });
    }
}
