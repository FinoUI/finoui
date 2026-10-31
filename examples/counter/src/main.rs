//! Cargo runner for counter-owned checks and scenarios.
//! 计数器示例自有检查与场景的 Cargo 运行器。

use std::{panic::AssertUnwindSafe, path::PathBuf, process::ExitCode};

use clap::{Parser, Subcommand, ValueEnum};
use fino::scenario::{self, ScenarioResult, Status, ToolError};
#[cfg(not(target_arch = "wasm32"))]
use fino_counter::browser::run_web_scenario;
use fino_counter::conformance::{ConformanceReport, ContractViolation};
use fino_counter::runner::{check_counter, run_mock_conformance, run_mock_scenario};
#[cfg(not(target_arch = "wasm32"))]
use fino_counter::runner::{run_native_offscreen_conformance, run_native_offscreen_scenario};
use serde::Serialize;

#[derive(Parser)]
#[command(name = "fino_counter", version)]
/// Runs the counter example's own checks and scenario mounting.
/// 运行计数器示例自己的检查与场景挂载。
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Runs counter-owned smoke checks.
    /// 运行计数器示例自己的冒烟检查。
    Check {
        #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
        format: OutputFormat,
    },
    /// Runs a JSON scenario with an explicit counter mount.
    /// 使用显式计数器 mount 运行 JSON 场景。
    Scenario {
        #[command(subcommand)]
        command: ScenarioCommand,
    },
    /// Runs the cross-backend conformance sequence and prints its stable report.
    /// 运行跨后端一致性序列并输出稳定报告。
    Conformance {
        #[arg(long, value_enum)]
        backend: ConformanceBackend,
        #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
        format: OutputFormat,
    },
    /// Opens the native winit/wgpu counter window.
    /// 打开原生 winit/wgpu 计数器窗口。
    #[cfg(not(target_arch = "wasm32"))]
    Native {
        /// Clicks the button twice through the real window input path, then exits.
        /// 通过真实窗口输入路径点击按钮两次后退出。
        #[arg(long)]
        smoke: bool,
    },
}

#[derive(Subcommand)]
enum ScenarioCommand {
    /// Executes the scenario on the selected example backend.
    /// 在所选示例后端上执行场景。
    Run {
        scenario: PathBuf,
        #[arg(long, value_enum)]
        backend: Backend,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
        format: OutputFormat,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Backend {
    Mock,
    #[cfg(not(target_arch = "wasm32"))]
    WebHeadless,
    #[cfg(not(target_arch = "wasm32"))]
    NativeOffscreen,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ConformanceBackend {
    Mock,
    #[cfg(not(target_arch = "wasm32"))]
    NativeOffscreen,
}

impl Backend {
    /// Supplies the short artifact directory suffix used by the documented commands.
    /// 提供文档命令使用的短产物目录后缀。
    fn artifact_suffix(self) -> &'static str {
        match self {
            Self::Mock => "mock",
            #[cfg(not(target_arch = "wasm32"))]
            Self::WebHeadless => "web",
            #[cfg(not(target_arch = "wasm32"))]
            Self::NativeOffscreen => "native",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    Human,
    Json,
}

/// Runs the example-owned tool behind structured panic reporting.
/// 在结构化 panic 报告边界内运行示例自有工具。
fn main() -> ExitCode {
    let format = panic_output_format();
    match catch_tool_panic(run) {
        Ok(code) => code,
        Err(error) => output_error(error, format),
    }
}

/// Dispatches only counter checks and counter-mounted scenario execution.
/// 仅分发计数器检查与显式计数器挂载的场景执行。
fn run() -> ExitCode {
    match Cli::parse().command {
        Command::Check { format } => {
            let report = check_counter();
            let failed = report.status == Status::Failed;
            if format == OutputFormat::Json {
                print_json(&report);
            } else {
                println!("counter check: {}", status_name(report.status));
            }
            exit_status(failed)
        }
        Command::Scenario {
            command:
                ScenarioCommand::Run {
                    scenario: path,
                    backend,
                    out,
                    format,
                },
        } => {
            let out = out.unwrap_or_else(|| {
                PathBuf::from(format!(
                    "artifacts/runs/counter-{}",
                    backend.artifact_suffix()
                ))
            });
            let run = scenario::load_scenario(&path).and_then(|scenario| match backend {
                Backend::Mock => run_mock_scenario(&scenario, &out),
                #[cfg(not(target_arch = "wasm32"))]
                Backend::WebHeadless => run_web_scenario(&path, &out),
                #[cfg(not(target_arch = "wasm32"))]
                Backend::NativeOffscreen => run_native_offscreen_scenario(&scenario, &out),
            });
            match run {
                Ok(result) => {
                    let failed = result.status == Status::Failed;
                    output_scenario(&result, format);
                    exit_status(failed)
                }
                Err(error) => output_error(error, format),
            }
        }
        Command::Conformance { backend, format } => {
            let run = match backend {
                ConformanceBackend::Mock => run_mock_conformance(),
                #[cfg(not(target_arch = "wasm32"))]
                ConformanceBackend::NativeOffscreen => run_native_offscreen_conformance(),
            };
            match run {
                Ok(report) => {
                    let (status, violations) = conformance_outcome(&report);
                    if format == OutputFormat::Json {
                        let mut value =
                            serde_json::to_value(&report).expect("conformance report JSON");
                        value["status"] = serde_json::to_value(status).expect("status JSON");
                        value["violations"] =
                            serde_json::to_value(&violations).expect("violation JSON");
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&value).expect("conformance output JSON")
                        );
                    } else {
                        println!(
                            "conformance {}: {} (set_text {}, nodes +{} / -{}, frames {})",
                            report.backend,
                            status_name(status),
                            report.final_stats.backend_set_text,
                            report.final_stats.nodes_created,
                            report.final_stats.nodes_removed,
                            report.final_stats.frames_requested,
                        );
                        for violation in &violations {
                            println!("  {}: {}", violation.field, violation.message);
                        }
                    }
                    exit_status(status == Status::Failed)
                }
                Err(message) => output_error(
                    ToolError::for_backend("UI_CONFORMANCE", message, "conformance"),
                    format,
                ),
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        Command::Native { smoke } => match fino_counter::native::run(smoke) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => output_error(
                ToolError::for_backend("UI_NATIVE_HOST", error.to_string(), "native"),
                panic_output_format(),
            ),
        },
    }
}

/// Preserves runner failures as structured diagnostics with their original payload.
/// 将运行器故障保留为包含原始 payload 的结构化诊断。
fn catch_tool_panic(operation: impl FnOnce() -> ExitCode) -> Result<ExitCode, ToolError> {
    std::panic::catch_unwind(AssertUnwindSafe(operation))
        .map_err(|payload| ToolError::tool_panic(panic_payload(payload)))
}

/// Selects JSON panic output for either accepted format argument spelling.
/// 为两种受支持的格式参数写法选择 JSON panic 输出。
fn panic_output_format() -> OutputFormat {
    if std::env::args_os().any(|argument| argument == "json" || argument == "--format=json") {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    }
}

/// Extracts standard panic text and explicitly identifies opaque payloads.
/// 提取标准 panic 文本，并显式标识不透明 payload。
fn panic_payload(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}

/// Prints either the complete machine result or a bounded scenario summary.
/// 输出完整机器结果或有界的场景摘要。
fn output_scenario(result: &ScenarioResult, format: OutputFormat) {
    if format == OutputFormat::Json {
        print_json(result);
    } else {
        println!(
            "scenario {}: {} ({})",
            result.name,
            status_name(result.status),
            result.run_id
        );
        println!("run_dir: {}", result.run_dir);
    }
}

/// Reports runner errors using the requested channel and returns the tool-error exit code.
/// 按请求格式报告运行器错误，并返回工具错误退出码。
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

/// Prints one machine-readable value as stable pretty JSON.
/// 以稳定格式化 JSON 输出单个机器可读值。
fn print_json(value: &impl Serialize) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("result JSON")
    );
}

/// Maps a failure flag to the process exit code used by every command.
/// 把失败标记映射为所有命令使用的进程退出码。
fn failure_code(failed: bool) -> u8 {
    if failed { 1 } else { 0 }
}

/// Distinguishes failed counter assertions from successful scenario completion.
/// 区分计数器断言失败与场景成功完成。
fn exit_status(failed: bool) -> ExitCode {
    ExitCode::from(failure_code(failed))
}

/// Uses stable lowercase status labels in human runner output.
/// 在人类可读的运行器输出中使用稳定小写状态名。
fn status_name(status: Status) -> &'static str {
    match status {
        Status::Passed => "passed",
        Status::Failed => "failed",
    }
}

/// Validates a conformance report and returns its shared status and violations.
/// 校验一致性报告，并返回共享状态与违规列表。
fn conformance_outcome(report: &ConformanceReport) -> (Status, Vec<ContractViolation>) {
    let violations = report.validate();
    let status = if violations.is_empty() {
        Status::Passed
    } else {
        Status::Failed
    };
    (status, violations)
}

#[cfg(test)]
mod tests {
    use fino::scenario::Status;
    use fino_counter::runner::run_mock_conformance;

    use super::{catch_tool_panic, conformance_outcome, failure_code};

    /// Ensures example runner panics retain their original text after conversion.
    /// 确保示例运行器 panic 转换后仍保留原始文本。
    #[test]
    fn panic_boundary_preserves_original_message() {
        let error = match catch_tool_panic(|| panic!("counter runner regression panic")) {
            Ok(_) => panic!("operation should not succeed"),
            Err(error) => error,
        };
        assert_eq!(error.code.as_ref(), "UI_TOOL_PANIC");
        assert!(error.message.contains("counter runner regression panic"));
    }

    /// Rejects an altered contract field instead of reporting a passing run.
    /// 字段偏离契约时拒绝报告通过。
    #[test]
    fn conformance_outcome_fails_on_a_contract_violation() {
        let mut report = run_mock_conformance().expect("mock conformance");
        assert_eq!(conformance_outcome(&report).0, Status::Passed);

        report.batch.set_text_delta = 100;
        let (status, violations) = conformance_outcome(&report);
        assert_eq!(status, Status::Failed);
        assert!(
            violations
                .iter()
                .any(|violation| violation.field == "batch.set_text_delta")
        );
        assert_eq!(failure_code(status == Status::Failed), 1);
        assert_eq!(failure_code(false), 0);
    }
}
