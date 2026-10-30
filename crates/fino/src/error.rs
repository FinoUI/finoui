//! Shared machine-readable diagnostics for scenario and inspection libraries.
//! Scenario 与 Inspect library 共用的机器可读诊断。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{headless::HeadlessError, reactive::ReactiveError};

/// Machine-readable tool failure without producer-specific context.
/// 不含生产者特定上下文的机器可读工具错误。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ToolError {
    /// Stable error code.
    /// 稳定错误码。
    pub code: Box<str>,
    /// Human-readable explanation.
    /// 供人阅读的错误说明。
    pub message: Box<str>,
    /// Error severity.
    /// 错误严重级别。
    pub severity: Box<str>,
    /// Backend associated with the failure, when available.
    /// 可用时与错误关联的后端。
    pub backend: Option<Box<str>>,
    /// Zero-based scenario step index, when applicable.
    /// 适用时从 0 开始的场景步骤索引。
    pub step: Option<usize>,
    /// Unsupported capability, when applicable.
    /// 适用时不支持的能力。
    pub capability: Option<Box<str>>,
}

impl ToolError {
    /// Converts a caught tool-entry panic while preserving its original payload.
    /// 转换工具入口捕获的 panic，并保留原始 panic 信息。
    pub fn tool_panic(payload: impl Into<String>) -> Self {
        Self::new(
            "UI_TOOL_PANIC",
            format!("tool panicked: {}", payload.into()),
        )
    }

    pub(crate) fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into().into_boxed_str(),
            severity: "error".into(),
            backend: None,
            step: None,
            capability: None,
        }
    }

    /// Creates a structured failure attributed to one backend for caller-owned tools.
    /// 为调用方自有工具创建归属于某个后端的结构化失败。
    ///
    /// ```
    /// use fino::error::ToolError;
    ///
    /// let error = ToolError::for_backend("UI_WASM_BUILD", "build failed", "web-headless");
    /// assert_eq!(error.code.as_ref(), "UI_WASM_BUILD");
    /// assert_eq!(error.backend.as_deref(), Some("web-headless"));
    /// ```
    pub fn for_backend(code: &str, message: impl Into<String>, backend: &str) -> Self {
        Self::new(code, message).at_backend(backend)
    }

    pub(crate) fn at_backend(mut self, backend: &str) -> Self {
        self.backend = Some(backend.into());
        self
    }

    pub(crate) fn at_step(mut self, step: usize, backend: &str) -> Self {
        self.step = Some(step);
        self.at_backend(backend)
    }

    pub(crate) fn from_headless(error: HeadlessError, step: usize, backend: &str) -> Self {
        Self::new(&error.code, error.message).at_step(step, backend)
    }

    pub(crate) fn from_reactive(error: ReactiveError, backend: &str) -> Self {
        let mut converted = Self::new(&error.code, error.message);
        converted.backend = Some(backend.into());
        converted
    }

    pub(crate) fn with_capability(mut self, capability: String) -> Self {
        self.capability = Some(capability.into_boxed_str());
        self
    }
}
