//! Cargo runner for counter-owned checks and scenarios.
//! 计数器示例自有检查与场景的 Cargo 运行器。

use std::{path::PathBuf, process::ExitCode};

use clap::{Parser, Subcommand, ValueEnum};
use fino::scenario::{self, ScenarioResult, Status, ToolError};
use fino_counter::runner::{check_counter, run_mock_scenario};
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
        out: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
        format: OutputFormat,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Backend {
    Mock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    Human,
    Json,
}

fn main() -> ExitCode {
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
            let Backend::Mock = backend;
            match scenario::load_scenario(&path)
                .and_then(|scenario| run_mock_scenario(&scenario, &out))
            {
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

fn exit_status(failed: bool) -> ExitCode {
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn status_name(status: Status) -> &'static str {
    match status {
        Status::Passed => "passed",
        Status::Failed => "failed",
    }
}
