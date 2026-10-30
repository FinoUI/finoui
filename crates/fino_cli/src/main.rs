//! Generic Fino project checks and read-only artifact inspection.
//! 通用 Fino 项目检查与只读产物检查。

use std::{path::PathBuf, process::ExitCode};

use clap::{Parser, Subcommand, ValueEnum};
use fino::{
    error::ToolError,
    headless::SnapshotKind,
    inspect::inspect_artifact,
    scenario::{SCENARIO_SCHEMA, Status},
};
use serde::Serialize;

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
                print_json(&value);
                ExitCode::SUCCESS
            }
            Err(error) => output_error(error, format),
        },
    }
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
