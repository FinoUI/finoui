//! Shared runtime metrics across Mock, Web, and Native backends.
//! 跨 Mock、Web 与 Native 后端共享的运行指标。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Backend-side work attributable to one runtime.
/// 归属于单个 Runtime 的后端工作量。
///
/// These counters describe shared DOM semantics only. Platform-specific handles,
/// ids, and GPU statistics are intentionally excluded.
/// 这些计数只描述共享 DOM 语义；平台特有句柄、ID 与 GPU 统计被刻意排除。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackendStats {
    /// Text commits that actually changed an existing text node.
    /// 真正改变现有文本节点的文本提交次数。
    pub backend_set_text: u64,
    /// Element and text nodes created by the backend.
    /// 后端创建的元素与文本节点数。
    pub nodes_created: u64,
    /// Backend remove operations.
    /// 后端 remove 操作次数。
    pub nodes_removed: u64,
    /// Text shapes performed for text nodes.
    /// 为文本节点执行的 shaping 次数。
    pub shaped_texts: u64,
    /// Layout boxes computed by committed layout passes.
    /// 已提交布局过程计算的布局盒数量。
    pub layout_nodes: u64,
    /// Coalesced frame requests issued for the owning window.
    /// 为所属窗口发出的合并帧请求数。
    pub frames_requested: u64,
}

/// Unified runtime counters exposed by every conformance backend.
/// 每个一致性后端都暴露的统一运行时计数。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeStats {
    /// Manual component mount executions.
    /// 手工组件挂载执行次数。
    pub component_executions: u64,
    /// Effect callback executions.
    /// Effect 回调执行次数。
    pub effect_executions: u64,
    /// Text commits that actually changed a text node.
    /// 真正改变文本节点的文本提交次数。
    pub backend_set_text: u64,
    /// Element and text nodes created by the backend.
    /// 后端创建的元素与文本节点数。
    pub nodes_created: u64,
    /// Backend remove operations.
    /// 后端 remove 操作次数。
    pub nodes_removed: u64,
    /// Text shapes performed for text nodes.
    /// 为文本节点执行的 shaping 次数。
    pub shaped_texts: u64,
    /// Layout boxes computed by committed layout passes.
    /// 已提交布局过程计算的布局盒数量。
    pub layout_nodes: u64,
    /// Coalesced frame requests issued for the owning window.
    /// 为所属窗口发出的合并帧请求数。
    pub frames_requested: u64,
}

impl RuntimeStats {
    /// Combines reactive execution counts with backend counters.
    /// 组合响应式执行计数与后端计数。
    pub fn from_parts(
        component_executions: u64,
        effect_executions: u64,
        backend: BackendStats,
    ) -> Self {
        Self {
            component_executions,
            effect_executions,
            backend_set_text: backend.backend_set_text,
            nodes_created: backend.nodes_created,
            nodes_removed: backend.nodes_removed,
            shaped_texts: backend.shaped_texts,
            layout_nodes: backend.layout_nodes,
            frames_requested: backend.frames_requested,
        }
    }

    /// Subtracts an earlier snapshot to expose per-operation deltas.
    /// 减去较早的快照，得到单次操作的增量。
    pub fn delta_since(&self, earlier: &Self) -> Self {
        Self {
            component_executions: self
                .component_executions
                .saturating_sub(earlier.component_executions),
            effect_executions: self
                .effect_executions
                .saturating_sub(earlier.effect_executions),
            backend_set_text: self
                .backend_set_text
                .saturating_sub(earlier.backend_set_text),
            nodes_created: self.nodes_created.saturating_sub(earlier.nodes_created),
            nodes_removed: self.nodes_removed.saturating_sub(earlier.nodes_removed),
            shaped_texts: self.shaped_texts.saturating_sub(earlier.shaped_texts),
            layout_nodes: self.layout_nodes.saturating_sub(earlier.layout_nodes),
            frames_requested: self
                .frames_requested
                .saturating_sub(earlier.frames_requested),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Keeps the unified schema stable for CI artifacts.
    /// 为 CI 产物保持统一 schema 稳定。
    #[test]
    fn runtime_stats_serialize_in_stable_field_order() {
        let stats = RuntimeStats::from_parts(
            1,
            2,
            BackendStats {
                backend_set_text: 3,
                nodes_created: 4,
                nodes_removed: 5,
                shaped_texts: 6,
                layout_nodes: 7,
                frames_requested: 8,
            },
        );
        assert_eq!(
            serde_json::to_value(stats).expect("stats JSON"),
            json!({
                "component_executions": 1,
                "effect_executions": 2,
                "backend_set_text": 3,
                "nodes_created": 4,
                "nodes_removed": 5,
                "shaped_texts": 6,
                "layout_nodes": 7,
                "frames_requested": 8,
            })
        );
    }

    /// Computes per-step deltas without underflowing on reordered snapshots.
    /// 计算步骤增量，并在快照顺序颠倒时不发生下溢。
    #[test]
    fn deltas_saturate_at_zero() {
        let earlier = RuntimeStats {
            backend_set_text: 3,
            ..RuntimeStats::default()
        };
        let later = RuntimeStats {
            backend_set_text: 5,
            ..RuntimeStats::default()
        };
        assert_eq!(later.delta_since(&earlier).backend_set_text, 2);
        assert_eq!(earlier.delta_since(&later).backend_set_text, 0);
    }
}
