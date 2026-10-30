//! Generic Fino project checks and read-only artifact inspection.
//! 通用 Fino 项目检查与只读产物检查。

use std::{
    panic::AssertUnwindSafe,
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, Subcommand, ValueEnum};
use fino::{
    error::ToolError,
    headless::SnapshotKind,
    inspect::inspect_artifact,
    scenario::{SCENARIO_SCHEMA, Status},
};
use serde::Serialize;
use serde_json::Value;

#[derive(Parser)]
#[command(name = "fino", version)]
/// Runs generic project checks and inspects existing artifacts.
/// 运行通用项目检查并检查现有产物。
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Checks generic workspace and tool protocol prerequisites.
    /// 检查通用工作区与工具协议前置条件。
    Check {
        #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
        format: OutputFormat,
    },
    /// Reads a structured artifact without discovering its producer.
    /// 读取结构化产物，不发现其生产者。
    Inspect {
        #[arg(value_enum)]
        kind: InspectKind,
        #[arg(long)]
        run: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
        format: OutputFormat,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum InspectKind {
    Dom,
    A11y,
    Reactive,
    Trace,
    Layout,
    #[value(name = "display_list")]
    DisplayList,
    Screenshot,
    Resources,
}

impl From<InspectKind> for SnapshotKind {
    /// Maps CLI protocol names to the core snapshot capability enum.
    /// 将 CLI 协议名映射到核心快照能力枚举。
    fn from(kind: InspectKind) -> Self {
        match kind {
            InspectKind::Dom => Self::Dom,
            InspectKind::A11y => Self::A11y,
            InspectKind::Reactive => Self::Reactive,
            InspectKind::Trace => Self::Trace,
            InspectKind::Layout => Self::Layout,
            InspectKind::DisplayList => Self::DisplayList,
            InspectKind::Screenshot => Self::Screenshot,
            InspectKind::Resources => Self::Resources,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    Human,
    Json,
}

#[derive(Serialize)]
struct CheckResult {
    name: String,
    status: Status,
    message: Option<String>,
}

#[derive(Serialize)]
struct CheckReport {
    schema: u32,
    status: Status,
    checks: Vec<CheckResult>,
}

/// Runs the command behind the panic-to-structured-error boundary.
/// 在 panic 转结构化错误的边界内运行命令。
fn main() -> ExitCode {
    let format = panic_output_format();
    match catch_tool_panic(run) {
        Ok(code) => code,
        Err(error) => output_error(error, format),
    }
}

/// Dispatches generic checks and inspection while respecting the selected output format.
/// 分发通用检查与产物检查，并遵循所选输出格式。
fn run() -> ExitCode {
    match Cli::parse().command {
        Command::Check { format } => {
            let report = generic_check();
            let failed = report.status == Status::Failed;
            if format == OutputFormat::Json {
                print_json(&report);
            } else {
                println!("check: {}", status_name(report.status));
                for check in report.checks {
                    println!("  {}: {}", check.name, status_name(check.status));
                }
            }
            exit_status(failed)
        }
        Command::Inspect { kind, run, format } => match inspect_artifact(&run, kind.into()) {
            Ok(value) => {
                if format == OutputFormat::Json {
                    print_json(&value);
                } else {
                    print_inspect_summary(kind, &run, &value);
                }
                ExitCode::SUCCESS
            }
            Err(error) => output_error(error, format),
        },
    }
}

/// Converts an unwinding tool failure into a diagnostic instead of aborting silently.
/// 将展开中的工具故障转换为诊断，避免静默中断。
fn catch_tool_panic(operation: impl FnOnce() -> ExitCode) -> Result<ExitCode, ToolError> {
    std::panic::catch_unwind(AssertUnwindSafe(operation))
        .map_err(|payload| ToolError::tool_panic(panic_payload(payload)))
}

/// Recovers JSON panic formatting from either supported `--format` spelling.
/// 从两种受支持的 `--format` 写法恢复 JSON panic 输出格式。
fn panic_output_format() -> OutputFormat {
    if std::env::args_os().any(|argument| argument == "json" || argument == "--format=json") {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    }
}

/// Preserves standard string panic payloads and labels non-string payloads explicitly.
/// 保留标准字符串 panic 信息，并显式标记非字符串 payload。
fn panic_payload(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}

/// Prints bounded human counts instead of exposing the complete artifact payload.
/// 输出有界的人类可读计数，而不是完整产物内容。
fn print_inspect_summary(kind: InspectKind, run: &Path, value: &Value) {
    println!("inspect {}: {}", inspect_name(kind), run.display());
    match kind {
        InspectKind::Dom => {
            println!("roots: {}", array_len(value, "roots"));
            println!("nodes: {}", array_len(value, "nodes"));
        }
        InspectKind::A11y => println!("nodes: {}", array_len(value, "nodes")),
        InspectKind::Reactive => {
            println!(
                "component_executions: {}",
                value["component_executions"].as_u64().unwrap_or(0)
            );
            println!("signals: {}", array_len(value, "signals"));
            println!("effects: {}", array_len(value, "effects"));
            println!("errors: {}", array_len(value, "errors"));
        }
        InspectKind::Trace => println!("events: {}", array_len(value, "events")),
        InspectKind::Layout
        | InspectKind::DisplayList
        | InspectKind::Screenshot
        | InspectKind::Resources => {
            println!(
                "fields: {}",
                value.as_object().map_or(0, |object| object.len())
            );
        }
    }
}

/// Uses the same protocol spelling in summaries and artifact filenames.
/// 在摘要与产物文件名中使用相同协议名称。
fn inspect_name(kind: InspectKind) -> &'static str {
    match kind {
        InspectKind::Dom => "dom",
        InspectKind::A11y => "a11y",
        InspectKind::Reactive => "reactive",
        InspectKind::Trace => "trace",
        InspectKind::Layout => "layout",
        InspectKind::DisplayList => "display_list",
        InspectKind::Screenshot => "screenshot",
        InspectKind::Resources => "resources",
    }
}

fn array_len(value: &Value, field: &str) -> usize {
    value[field].as_array().map_or(0, Vec::len)
}

/// Validates tool prerequisites without inspecting or executing any workspace member.
/// 验证工具前置条件，不检查或执行任何 workspace 成员。
fn generic_check() -> CheckReport {
    let inspect_artifacts = [
        SnapshotKind::Dom,
        SnapshotKind::A11y,
        SnapshotKind::Reactive,
        SnapshotKind::Trace,
    ];
    let unique_artifacts: std::collections::BTreeSet<_> = inspect_artifacts
        .iter()
        .map(|kind| kind.artifact_name())
        .collect();
    let inspect_protocol_valid = unique_artifacts.len() == inspect_artifacts.len();
    let checks = vec![
        workspace_manifest_check(Path::new("Cargo.toml")),
        CheckResult {
            name: "scenario_protocol".to_owned(),
            status: if SCENARIO_SCHEMA == 1 {
                Status::Passed
            } else {
                Status::Failed
            },
            message: (SCENARIO_SCHEMA != 1)
                .then(|| format!("unsupported scenario schema {SCENARIO_SCHEMA}")),
        },
        CheckResult {
            name: "inspect_protocol".to_owned(),
            status: if inspect_protocol_valid {
                Status::Passed
            } else {
                Status::Failed
            },
            message: (!inspect_protocol_valid)
                .then(|| "core inspect artifact names are not unique".to_owned()),
        },
    ];
    CheckReport {
        schema: 1,
        status: if checks.iter().all(|check| check.status == Status::Passed) {
            Status::Passed
        } else {
            Status::Failed
        },
        checks,
    }
}

/// Parses Cargo's TOML and requires a non-empty string array of workspace members.
/// 解析 Cargo TOML，并要求 workspace members 是非空字符串数组。
fn workspace_manifest_check(path: &Path) -> CheckResult {
    let validation = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))
        .and_then(|source| {
            let manifest = toml::from_str::<toml::Value>(&source)
                .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
            let members = manifest
                .get("workspace")
                .and_then(|workspace| workspace.get("members"))
                .and_then(toml::Value::as_array)
                .ok_or_else(|| "workspace.members must be an array".to_owned())?;
            if members.is_empty() {
                return Err("workspace.members must not be empty".to_owned());
            }
            if members.iter().any(|member| {
                member
                    .as_str()
                    .is_none_or(|member| member.trim().is_empty())
            }) {
                return Err("workspace.members must contain only non-empty strings".to_owned());
            }
            Ok(())
        });
    CheckResult {
        name: "workspace_manifest".to_owned(),
        status: if validation.is_ok() {
            Status::Passed
        } else {
            Status::Failed
        },
        message: validation.err(),
    }
}

/// Emits a structured or concise failure and reserves exit code 2 for tool errors.
/// 输出结构化或简洁错误，并以退出码 2 表示工具错误。
fn output_error(error: ToolError, format: OutputFormat) -> ExitCode {
    if format == OutputFormat::Json {
        eprintln!(
            "{}",
            serde_json::to_string_pretty(&error).expect("tool error JSON")
        );
    } else {
        eprintln!("{}: {}", error.code, error.message);
    }
    ExitCode::from(2)
}

fn print_json(value: &impl Serialize) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("result JSON")
    );
}

/// Maps a completed failed check to exit code 1 without conflating tool errors.
/// 将完成的失败检查映射为退出码 1，与工具错误区分。
fn exit_status(failed: bool) -> ExitCode {
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Formats status values consistently in human-readable summaries.
/// 在人类可读摘要中统一格式化状态值。
fn status_name(status: Status) -> &'static str {
    match status {
        Status::Passed => "passed",
        Status::Failed => "failed",
    }
}

#[cfg(test)]
mod tests {
    use super::catch_tool_panic;

    /// Ensures caught panics retain their payload in the structured error code and message.
    /// 确保捕获的 panic 在结构化错误码与消息中保留原始信息。
    #[test]
    fn panic_boundary_preserves_original_message() {
        let error = match catch_tool_panic(|| panic!("generic cli regression panic")) {
            Ok(_) => panic!("operation should not succeed"),
            Err(error) => error,
        };
        assert_eq!(error.code.as_ref(), "UI_TOOL_PANIC");
        assert!(error.message.contains("generic cli regression panic"));
    }
}
