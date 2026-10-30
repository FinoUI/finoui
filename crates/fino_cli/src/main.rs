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
    DisplayList,
    Screenshot,
    Resources,
}

impl From<InspectKind> for SnapshotKind {
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

fn main() -> ExitCode {
    let format = panic_output_format();
    match catch_tool_panic(run) {
        Ok(code) => code,
        Err(error) => output_error(error, format),
    }
}

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

fn catch_tool_panic(operation: impl FnOnce() -> ExitCode) -> Result<ExitCode, ToolError> {
    std::panic::catch_unwind(AssertUnwindSafe(operation))
        .map_err(|payload| ToolError::tool_panic(panic_payload(payload)))
}

fn panic_output_format() -> OutputFormat {
    if std::env::args_os().any(|argument| argument == "json" || argument == "--format=json") {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    }
}

fn panic_payload(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}

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

fn generic_check() -> CheckReport {
    let manifest = std::path::Path::new("Cargo.toml");
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
        CheckResult {
            name: "workspace_manifest".to_owned(),
            status: if manifest.is_file() {
                Status::Passed
            } else {
                Status::Failed
            },
            message: (!manifest.is_file()).then(|| "Cargo.toml is not readable".to_owned()),
        },
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

#[cfg(test)]
mod tests {
    use super::catch_tool_panic;

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
