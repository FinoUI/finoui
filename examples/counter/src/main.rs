//! Cargo runner for counter-owned checks and scenarios.
//! 计数器示例自有检查与场景的 Cargo 运行器。

use std::{panic::AssertUnwindSafe, path::PathBuf, process::ExitCode};

use clap::{Parser, Subcommand, ValueEnum};
use fino::scenario::{self, ScenarioResult, Status, ToolError};
use fino_counter::{
    browser::run_web_scenario,
    runner::{check_counter, run_mock_scenario},
};
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
    WebHeadless,
}

impl Backend {
    /// Supplies the short artifact directory suffix used by the documented commands.
    /// 提供文档命令使用的短产物目录后缀。
    fn artifact_suffix(self) -> &'static str {
        match self {
            Self::Mock => "mock",
            Self::WebHeadless => "web",
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
                Backend::WebHeadless => run_web_scenario(&path, &out),
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

fn print_json(value: &impl Serialize) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("result JSON")
    );
}

/// Distinguishes failed counter assertions from successful scenario completion.
/// 区分计数器断言失败与场景成功完成。
fn exit_status(failed: bool) -> ExitCode {
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Uses stable lowercase status labels in human runner output.
/// 在人类可读的运行器输出中使用稳定小写状态名。
fn status_name(status: Status) -> &'static str {
    match status {
        Status::Passed => "passed",
        Status::Failed => "failed",
    }
}

#[cfg(test)]
mod tests {
    use super::catch_tool_panic;

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
}
