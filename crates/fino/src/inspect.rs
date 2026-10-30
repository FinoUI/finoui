//! Read-only structured artifact inspection independent of component and host implementations.
//! 独立于组件与 Host 实现的只读结构化产物检查。

use std::{fs, path::Path};

use serde_json::Value;

use crate::{error::ToolError, headless::SnapshotKind};

/// Reads a completed run artifact without discovering or mounting its producer.
/// 读取已完成运行的产物，不发现或挂载其生产者。
///
/// ```
/// use fino::{headless::SnapshotKind, inspect::inspect_artifact};
///
/// let run_dir = std::env::temp_dir().join(format!(
///     "fino-inspect-doctest-{}",
///     std::process::id()
/// ));
/// std::fs::create_dir_all(&run_dir).expect("run directory");
/// std::fs::write(run_dir.join("dom.json"), br#"{"schema":1,"nodes":[]}"#)
///     .expect("DOM artifact");
/// let dom = inspect_artifact(&run_dir, SnapshotKind::Dom).expect("inspect DOM");
/// assert_eq!(dom["schema"], 1);
/// std::fs::remove_dir_all(run_dir).expect("cleanup");
/// ```
pub fn inspect_artifact(run_dir: &Path, kind: SnapshotKind) -> Result<Value, ToolError> {
    let path = run_dir.join(kind.artifact_name());
    let source = fs::read_to_string(&path).map_err(|error| {
        ToolError::new(
            "UI_ARTIFACT_NOT_FOUND",
            format!("failed to read {}: {error}", path.display()),
        )
    })?;
    serde_json::from_str(&source).map_err(|error| {
        ToolError::new(
            "UI_ARTIFACT_INVALID_JSON",
            format!("failed to parse {}: {error}", path.display()),
        )
    })
}
