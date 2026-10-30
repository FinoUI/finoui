//! Native orchestration that builds the wasm counter and drives headless Chromium.
//! 构建 wasm 计数器并驱动无头 Chromium 的原生编排。

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use fino::{error::ToolError, scenario::ScenarioResult};
use serde::Deserialize;

const BACKEND: &str = "web-headless";
const WASM_BINDGEN_VERSION: &str = "0.2.129";

#[derive(Deserialize)]
struct DriverReport {
    status: String,
    result: Option<ScenarioResult>,
    error: Option<DriverError>,
    #[serde(default)]
    checks: Vec<DriverCheck>,
}

#[derive(Deserialize)]
struct DriverError {
    code: String,
    message: String,
}

#[derive(Deserialize)]
struct DriverCheck {
    name: String,
    status: String,
    message: Option<String>,
}

/// Builds the wasm bundle, runs the unchanged scenario in Chromium, and returns its result.
/// 构建 wasm 包，在 Chromium 中运行未修改的场景，并返回其结果。
pub fn run_web_scenario(scenario_path: &Path, out: &Path) -> Result<ScenarioResult, ToolError> {
    let example_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = example_dir
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| {
            ToolError::for_backend(
                "UI_WASM_LAYOUT",
                "example is not inside a workspace",
                BACKEND,
            )
        })?
        .to_owned();
    let web_dir = example_dir.join("web");
    let pkg_dir = web_dir.join("pkg");

    ensure_playwright(&web_dir)?;
    build_wasm(&workspace, &pkg_dir)?;

    let scenario = scenario_path.canonicalize().map_err(|error| {
        ToolError::for_backend(
            "UI_SCENARIO_IO",
            format!("failed to resolve {}: {error}", scenario_path.display()),
            BACKEND,
        )
    })?;
    let out_absolute = absolute(out).map_err(|error| {
        ToolError::for_backend(
            "UI_ARTIFACT_IO",
            format!("failed to resolve {}: {error}", out.display()),
            BACKEND,
        )
    })?;

    let node = env::var_os("NODE").unwrap_or_else(|| "node".into());
    let driver = web_dir.join("driver.mjs");
    let mut command = Command::new(&node);
    command
        .current_dir(&workspace)
        .arg(&driver)
        .arg("--scenario")
        .arg(&scenario)
        .arg("--out")
        .arg(&out_absolute)
        .arg("--pkg")
        .arg(&pkg_dir)
        .arg("--run-dir-label")
        .arg(out.as_os_str());
    if let Some(browser) = env::var_os("FINO_CHROMIUM") {
        command.arg("--browser").arg(browser);
    }

    let output = command.output().map_err(|error| {
        ToolError::for_backend(
            "UI_NODE_MISSING",
            format!(
                "failed to launch node ({}): {error}; install Node.js or set NODE",
                Path::new(&node).display()
            ),
            BACKEND,
        )
    })?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let report = parse_report(&stdout, &output.stderr)?;

    if report.status != "passed" {
        let error = report.error.unwrap_or(DriverError {
            code: "UI_WEB_BROWSER".to_owned(),
            message: "the browser driver reported a failure".to_owned(),
        });
        let failed = failed_checks(&report.checks);
        return Err(ToolError::for_backend(
            &error.code,
            if failed.is_empty() {
                error.message
            } else {
                format!("{}; failed checks: {failed}", error.message)
            },
            BACKEND,
        ));
    }

    let result = report.result.ok_or_else(|| {
        ToolError::for_backend(
            "UI_WEB_BROWSER",
            "the browser driver returned no scenario result",
            BACKEND,
        )
    })?;
    Ok(result)
}

/// Requires the example's pinned Playwright package before launching any browser.
/// 在启动浏览器前要求示例中固定版本的 Playwright 包已安装。
fn ensure_playwright(web_dir: &Path) -> Result<(), ToolError> {
    if web_dir
        .join("node_modules/playwright/package.json")
        .is_file()
    {
        return Ok(());
    }
    Err(ToolError::for_backend(
        "UI_PLAYWRIGHT_MISSING",
        format!(
            "Playwright is not installed; run `npm ci --prefix {}` and `npx --prefix {} playwright install chromium`",
            web_dir.display(),
            web_dir.display()
        ),
        BACKEND,
    ))
}

/// Builds the release wasm library and generates browser bindings beside the page.
/// 构建 release wasm library，并在页面旁生成浏览器绑定。
fn build_wasm(workspace: &Path, pkg_dir: &Path) -> Result<(), ToolError> {
    let build = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(workspace)
        .args([
            "build",
            "--lib",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
            "-p",
            "fino_counter",
        ])
        .output()
        .map_err(|error| {
            ToolError::for_backend(
                "UI_WASM_BUILD",
                format!("failed to launch cargo: {error}"),
                BACKEND,
            )
        })?;
    if !build.status.success() {
        return Err(ToolError::for_backend(
            "UI_WASM_BUILD",
            format!(
                "wasm build failed: {}",
                tail(&String::from_utf8_lossy(&build.stderr))
            ),
            BACKEND,
        ));
    }

    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                workspace.join(path)
            }
        })
        .unwrap_or_else(|| workspace.join("target"));
    let wasm = target_dir.join("wasm32-unknown-unknown/release/fino_counter.wasm");
    if !wasm.is_file() {
        return Err(ToolError::for_backend(
            "UI_WASM_BUILD",
            format!(
                "wasm build succeeded but {} is missing; add the wasm32-unknown-unknown target",
                wasm.display()
            ),
            BACKEND,
        ));
    }

    if pkg_dir.exists() {
        fs::remove_dir_all(pkg_dir).map_err(|error| {
            ToolError::for_backend(
                "UI_ARTIFACT_IO",
                format!("failed to clear {}: {error}", pkg_dir.display()),
                BACKEND,
            )
        })?;
    }
    fs::create_dir_all(pkg_dir).map_err(|error| {
        ToolError::for_backend(
            "UI_ARTIFACT_IO",
            format!("failed to create {}: {error}", pkg_dir.display()),
            BACKEND,
        )
    })?;

    let bindgen = env::var_os("WASM_BINDGEN").unwrap_or_else(|| "wasm-bindgen".into());
    let bindings = Command::new(&bindgen)
        .arg(&wasm)
        .arg("--out-dir")
        .arg(pkg_dir)
        .arg("--target")
        .arg("web")
        .arg("--no-typescript")
        .output()
        .map_err(|error| {
            ToolError::for_backend(
                "UI_WASM_BINDGEN_MISSING",
                format!(
                    "failed to launch wasm-bindgen ({}): {error}; install wasm-bindgen-cli {WASM_BINDGEN_VERSION}",
                    Path::new(&bindgen).display()
                ),
                BACKEND,
            )
        })?;
    if !bindings.status.success() {
        return Err(ToolError::for_backend(
            "UI_WASM_BINDGEN",
            format!(
                "wasm-bindgen failed (requires wasm-bindgen-cli {WASM_BINDGEN_VERSION}): {}",
                tail(&String::from_utf8_lossy(&bindings.stderr))
            ),
            BACKEND,
        ));
    }
    Ok(())
}

/// Parses the driver's single JSON line while retaining stderr for failures.
/// 解析驱动的单行 JSON，并在失败时保留 stderr。
fn parse_report(stdout: &str, stderr: &[u8]) -> Result<DriverReport, ToolError> {
    let line = stdout
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('{'));
    let Some(line) = line else {
        return Err(ToolError::for_backend(
            "UI_WEB_BROWSER",
            format!(
                "the browser driver returned no JSON report; stderr: {}",
                tail(&String::from_utf8_lossy(stderr))
            ),
            BACKEND,
        ));
    };
    serde_json::from_str(line).map_err(|error| {
        ToolError::for_backend(
            "UI_WEB_BROWSER",
            format!(
                "failed to parse the browser driver report: {error}; stderr: {}",
                tail(&String::from_utf8_lossy(stderr))
            ),
            BACKEND,
        )
    })
}

/// Collects the names of failed browser checks for one structured error message.
/// 收集失败浏览器检查的名称，用于单条结构化错误消息。
fn failed_checks(checks: &[DriverCheck]) -> String {
    checks
        .iter()
        .filter(|check| check.status != "passed")
        .map(|check| match &check.message {
            Some(message) => format!("{} ({message})", check.name),
            None => check.name.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Resolves a possibly relative path against the process working directory.
/// 把可能为相对的路径解析到进程工作目录下。
fn absolute(path: &Path) -> std::io::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_owned())
    } else {
        env::current_dir().map(|current| current.join(path))
    }
}

/// Keeps structured errors readable when a child process emits a long log.
/// 当子进程输出很长的日志时，保持结构化错误可读。
/// Keeps structured errors readable when a child process emits a long log.
/// 当子进程输出很长的日志时，保持结构化错误可读。
fn tail(output: &str) -> String {
    const LIMIT: usize = 4000;
    let output = output.trim();
    if output.len() <= LIMIT {
        return output.to_owned();
    }
    let mut start = output.len() - LIMIT;
    while !output.is_char_boundary(start) {
        start += 1;
    }
    format!("...{}", &output[start..])
}
