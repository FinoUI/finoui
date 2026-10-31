//! Shared counter conformance sequence and stable cross-backend report.
//! 共享计数器一致性序列与稳定的跨后端报告。

use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use fino::{
    dom_api::{Attribute, Role, Selector},
    headless::{HeadlessBackend, HeadlessHost, LogicalTreeSnapshot},
    metrics::RuntimeStats,
    reactive::{Owner, Signal, batch},
    runtime::Runtime,
};
use serde::{Deserialize, Serialize};

/// Current conformance report schema version.
/// 当前一致性报告 schema 版本。
pub const CONFORMANCE_SCHEMA: u32 = 1;

/// Minimal accessibility projection without platform node identifiers.
/// 不含平台节点标识的最小无障碍投影。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct A11yNodeResult {
    /// Semantic role.
    /// 语义角色。
    pub role: String,
    /// Accessible name.
    /// 无障碍名称。
    pub name: String,
    /// Descendant text.
    /// 后代文本。
    pub text: String,
}

/// Result of one semantic click on the counter.
/// 计数器上一次语义点击的结果。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventResult {
    /// Whether the semantic action found and dispatched a listener.
    /// 语义动作是否找到并分发了监听器。
    pub handled: bool,
    /// Text after the click.
    /// 点击后的文本。
    pub text_after: String,
    /// Matching semantic nodes after the click.
    /// 点击后的语义匹配节点数。
    pub selector_matches: usize,
}

/// Result of one 100-increment transaction.
/// 一次 100 次增量事务的结果。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchResult {
    /// Requested increment count.
    /// 请求的增量次数。
    pub increments: u32,
    /// Final committed text.
    /// 最终提交的文本。
    pub final_text: String,
    /// Actual text commits during the transaction.
    /// 事务期间实际文本提交数。
    pub set_text_delta: u64,
    /// Created nodes during the transaction.
    /// 事务期间创建的节点数。
    pub nodes_created_delta: u64,
    /// Removed nodes during the transaction.
    /// 事务期间删除的节点数。
    pub nodes_removed_delta: u64,
    /// Coalesced frame requests during the transaction.
    /// 事务期间的合并帧请求数。
    pub frames_requested_delta: u64,
}

/// Observable state after disposal.
/// 释放后的可观测状态。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnmountResult {
    /// Semantic matches after unmount.
    /// 卸载后的语义匹配数。
    pub selector_matches: usize,
    /// Accessibility nodes after unmount.
    /// 卸载后的无障碍节点数。
    pub a11y_nodes: usize,
    /// Backend remove operations during unmount.
    /// 卸载期间的后端删除操作数。
    pub nodes_removed_delta: u64,
}

/// Stable conformance report suitable for CI comparison.
/// 适合 CI 比较的稳定一致性报告。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConformanceReport {
    /// Report schema version.
    /// 报告 schema 版本。
    pub schema: u32,
    /// Backend identity.
    /// 后端标识。
    pub backend: String,
    /// Logical tree after the insert/move and batch sequence.
    /// 插入／移动与批量操作后的逻辑树。
    pub tree: LogicalTreeSnapshot,
    /// Accessibility projection with the same state.
    /// 同一状态下的无障碍投影。
    pub a11y: Vec<A11yNodeResult>,
    /// Click result and committed text.
    /// 点击结果与提交文本。
    pub event: EventResult,
    /// Cumulative runtime counters at each checkpoint in name order.
    /// 按名称排序的各检查点累计运行时计数。
    pub checkpoints: BTreeMap<String, RuntimeStats>,
    /// Normalized backend operations in stable verb order.
    /// 按稳定操作名排序的规范化后端操作计数。
    pub operations: BTreeMap<String, u64>,
    /// One-hundred-increment transaction result.
    /// 100 次增量事务的结果。
    pub batch: BatchResult,
    /// Disposal result.
    /// 释放结果。
    pub unmount: UnmountResult,
    /// Logical tree after disposal.
    /// 释放后的逻辑树。
    pub final_tree: LogicalTreeSnapshot,
    /// Final cumulative runtime counters.
    /// 最终累计运行时计数。
    pub final_stats: RuntimeStats,
}

/// One failed conformance contract check.
/// 一项未通过的一致性契约检查。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractViolation {
    /// Stable dotted field path.
    /// 稳定的点分字段路径。
    pub field: String,
    /// Human-readable expectation.
    /// 供人阅读的期望说明。
    pub message: String,
}

/// Result of removing and reinserting the dynamic text node.
/// 移除并重新插入动态文本节点的结果。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetachReinsertResult {
    /// Result schema version.
    /// 结果 schema 版本。
    pub schema: u32,
    /// Backend identity.
    /// 后端标识。
    pub backend: String,
    /// Logical tree before detachment.
    /// 分离前的逻辑树。
    pub tree_before: LogicalTreeSnapshot,
    /// Logical tree after detaching the value node.
    /// 分离值节点后的逻辑树。
    pub tree_detached: LogicalTreeSnapshot,
    /// Logical tree after reinsertion.
    /// 重新插入后的逻辑树。
    pub tree_restored: LogicalTreeSnapshot,
    /// Text after detachment.
    /// 分离后的文本。
    pub text_detached: Option<String>,
    /// Text after reinsertion.
    /// 重新插入后的文本。
    pub text_restored: Option<String>,
    /// Counters after detachment.
    /// 分离后的计数。
    pub detached_stats: RuntimeStats,
    /// Counters after reinsertion.
    /// 重新插入后的计数。
    pub restored_stats: RuntimeStats,
    /// Normalized backend operations in stable verb order.
    /// 按稳定操作名排序的规范化后端操作计数。
    pub operations: BTreeMap<String, u64>,
}

impl ConformanceReport {
    /// Validates every shared contract field and returns stable violations.
    /// 校验全部共享契约字段，并返回稳定违规列表。
    pub fn validate(&self) -> Vec<ContractViolation> {
        let mut violations = Vec::new();

        check(&mut violations, "schema", self.schema, 1);
        if self.tree != expected_tree() {
            push(
                &mut violations,
                "tree",
                "logical tree must match the shared conformance shape",
            );
        }
        match self.a11y.as_slice() {
            [node]
                if node.role == "button"
                    && node.name == "Increment counter"
                    && node.text == "Count: !102" => {}
            _ => push(
                &mut violations,
                "a11y",
                "accessibility projection must contain one named button with the final text",
            ),
        }
        check(&mut violations, "event.handled", self.event.handled, true);
        check(
            &mut violations,
            "event.text_after",
            self.event.text_after.as_str(),
            "Count: !2",
        );
        check(
            &mut violations,
            "event.selector_matches",
            self.event.selector_matches,
            1,
        );

        let expected_checkpoints = [
            ("mounted", 1_u64, 1_u64, 1_u64, 3_u64, 0_u64),
            ("text_changed", 1, 2, 2, 3, 0),
            ("attribute_changed", 1, 2, 2, 3, 0),
            ("inserted", 1, 2, 2, 4, 0),
            ("clicked", 1, 3, 3, 4, 0),
            ("batched", 1, 4, 4, 4, 0),
        ];
        let mut expected_names: Vec<&str> =
            expected_checkpoints.iter().map(|entry| entry.0).collect();
        expected_names.sort_unstable();
        let actual_names: Vec<&str> = self.checkpoints.keys().map(String::as_str).collect();
        if actual_names != expected_names {
            push(
                &mut violations,
                "checkpoints",
                "checkpoint names and ordering must match the shared sequence",
            );
        }
        for (name, component, effect, set_text, created, removed) in expected_checkpoints {
            let Some(stats) = self.checkpoints.get(name) else {
                continue;
            };
            check(
                &mut violations,
                &format!("checkpoints.{name}.component_executions"),
                stats.component_executions,
                component,
            );
            check(
                &mut violations,
                &format!("checkpoints.{name}.effect_executions"),
                stats.effect_executions,
                effect,
            );
            check(
                &mut violations,
                &format!("checkpoints.{name}.backend_set_text"),
                stats.backend_set_text,
                set_text,
            );
            check(
                &mut violations,
                &format!("checkpoints.{name}.nodes_created"),
                stats.nodes_created,
                created,
            );
            check(
                &mut violations,
                &format!("checkpoints.{name}.nodes_removed"),
                stats.nodes_removed,
                removed,
            );
        }

        let expected_operations = expected_operations();
        if self.operations != expected_operations {
            push(
                &mut violations,
                "operations",
                "normalized backend operation counts must match the shared sequence",
            );
        }

        check(
            &mut violations,
            "batch.increments",
            self.batch.increments,
            100,
        );
        check(
            &mut violations,
            "batch.final_text",
            self.batch.final_text.as_str(),
            "Count: !102",
        );
        check(
            &mut violations,
            "batch.set_text_delta",
            self.batch.set_text_delta,
            1,
        );
        check(
            &mut violations,
            "batch.nodes_created_delta",
            self.batch.nodes_created_delta,
            0,
        );
        check(
            &mut violations,
            "batch.nodes_removed_delta",
            self.batch.nodes_removed_delta,
            0,
        );
        let native = self.backend.starts_with("native");
        check(
            &mut violations,
            "batch.frames_requested_delta",
            self.batch.frames_requested_delta,
            if native { 1 } else { 0 },
        );

        check(
            &mut violations,
            "unmount.selector_matches",
            self.unmount.selector_matches,
            0,
        );
        check(
            &mut violations,
            "unmount.a11y_nodes",
            self.unmount.a11y_nodes,
            0,
        );
        check(
            &mut violations,
            "unmount.nodes_removed_delta",
            self.unmount.nodes_removed_delta,
            1,
        );
        if !self.final_tree.roots.is_empty() {
            push(
                &mut violations,
                "final_tree.roots",
                "disposal must leave no reachable roots",
            );
        }

        check(
            &mut violations,
            "final_stats.component_executions",
            self.final_stats.component_executions,
            1,
        );
        check(
            &mut violations,
            "final_stats.effect_executions",
            self.final_stats.effect_executions,
            4,
        );
        check(
            &mut violations,
            "final_stats.backend_set_text",
            self.final_stats.backend_set_text,
            4,
        );
        check(
            &mut violations,
            "final_stats.nodes_created",
            self.final_stats.nodes_created,
            4,
        );
        check(
            &mut violations,
            "final_stats.nodes_removed",
            self.final_stats.nodes_removed,
            1,
        );
        check(
            &mut violations,
            "final_stats.frames_requested",
            self.final_stats.frames_requested,
            if native { 2 } else { 0 },
        );

        violations
    }
}

/// Compares a field and records a stable violation on mismatch.
/// 比较字段并在不匹配时记录稳定违规。
fn check<T: PartialEq + std::fmt::Debug>(
    violations: &mut Vec<ContractViolation>,
    field: &str,
    actual: T,
    expected: T,
) {
    if actual != expected {
        push(
            violations,
            field,
            &format!("expected {expected:?}, got {actual:?}"),
        );
    }
}

/// Appends one violation with its stable field path.
/// 追加一条带稳定字段路径的违规。
fn push(violations: &mut Vec<ContractViolation>, field: &str, message: &str) {
    violations.push(ContractViolation {
        field: field.to_owned(),
        message: message.to_owned(),
    });
}

/// Returns the normalized operation counts for the shared sequence.
/// 返回共享序列的规范化操作计数。
pub fn expected_operations() -> BTreeMap<String, u64> {
    [
        ("create_element", 1),
        ("create_text", 3),
        ("insert", 4),
        ("listen", 1),
        ("remove", 1),
        ("set_attribute", 3),
        ("set_text", 4),
        ("unlisten", 1),
    ]
    .into_iter()
    .map(|(name, count)| (name.to_owned(), count))
    .collect()
}

/// Mounts the counter and exposes the dynamic text node for conformance probes.
/// 挂载计数器并暴露一致性探针使用的动态文本节点。
fn mount_counter_with_nodes<B: HeadlessBackend + 'static>(
    runtime: &mut Runtime<B>,
    owner: &Owner,
) -> (B::Node, B::Node, Signal<i32>) {
    let count = owner.signal_named("counter/count", 0_i32);
    let mounted = crate::counter_with_count(count.clone()).mount_with_slots(runtime, owner);
    let value = mounted
        .text_slots()
        .first()
        .cloned()
        .expect("counter has one dynamic text slot");
    (mounted.into_parts().0, value, count)
}

/// Runs the same create, text, attribute, insert/move, event, batch, and unmount sequence.
/// 运行相同的创建、文本、属性、插入／移动、事件、批量与卸载序列。
pub fn run_conformance<B: HeadlessBackend + 'static>(
    mut host: HeadlessHost<B>,
) -> Result<ConformanceReport, String> {
    let backend = host.runtime().backend().backend_name().to_owned();
    let selector = counter_selector();
    let value_slot: Rc<RefCell<Option<B::Node>>> = Rc::new(RefCell::new(None));
    let count_slot = Rc::new(RefCell::new(None));
    let value_capture = Rc::clone(&value_slot);
    let count_capture = Rc::clone(&count_slot);

    let button = host.mount("counter/conformance", move |runtime, owner| {
        let (button, value, count) = mount_counter_with_nodes(runtime, owner);
        *value_capture.borrow_mut() = Some(value);
        *count_capture.borrow_mut() = Some(count);
        button
    });
    let value = value_slot
        .borrow_mut()
        .take()
        .ok_or_else(|| "conformance mount did not expose the value node".to_owned())?;
    let count = count_slot
        .borrow_mut()
        .take()
        .ok_or_else(|| "conformance mount did not expose the count signal".to_owned())?;

    let mut checkpoints = BTreeMap::new();
    checkpoints.insert("mounted".to_owned(), host.runtime_stats());

    // Text change: one unbatched signal write commits one text update.
    // 文本变化：一次未批处理 Signal 写入只提交一次文本更新。
    count.set(1);
    checkpoints.insert("text_changed".to_owned(), host.runtime_stats());

    // Attribute change: non-visual semantics, then restore the clickable name.
    // 属性变化：非视觉语义，然后恢复可点击名称。
    host.runtime_mut().backend_mut().set_attribute(
        &button,
        Attribute::AriaLabel,
        Some("Conformance counter"),
    );
    host.runtime_mut().backend_mut().set_attribute(
        &button,
        Attribute::AriaLabel,
        Some("Increment counter"),
    );
    checkpoints.insert("attribute_changed".to_owned(), host.runtime_stats());

    // Insert one node and move it before the dynamic value node.
    // 插入一个节点，并把它移动到动态值节点之前。
    let suffix = host.runtime_mut().backend_mut().create_text("!");
    host.runtime_mut()
        .backend_mut()
        .insert_before(&button, &suffix, None);
    host.runtime_mut()
        .backend_mut()
        .insert_before(&button, &suffix, Some(&value));
    checkpoints.insert("inserted".to_owned(), host.runtime_stats());

    // Event: one unbatched click commits exactly one text update.
    // 事件：一次未批处理点击恰好提交一次文本更新。
    host.click(&selector)
        .map_err(|error| format!("{}: {}", error.code, error.message))?;
    let text_after = host
        .runtime()
        .backend()
        .text_content(&button)
        .ok_or_else(|| "clicked counter has no text".to_owned())?;
    let selector_matches = host.runtime().backend().find_all(&selector).len();
    checkpoints.insert("clicked".to_owned(), host.runtime_stats());

    // Batch: 100 increments commit only the final text and coalesce frame work.
    // Drain the mount/click request first so batch frame work is attributed exactly.
    // 批处理：100 次增量只提交最终文本，并合并帧工作量。
    // 先取走挂载／点击请求，使批处理帧工作量可精确归因。
    let _ = host.runtime().backend().take_frame_request();
    let before_batch = host.runtime_stats();
    batch(|| {
        for _ in 0..100 {
            count.update(|number| *number += 1);
        }
    });
    let after_batch = host.runtime_stats();
    let batch_delta = after_batch.delta_since(&before_batch);
    checkpoints.insert("batched".to_owned(), after_batch);
    let final_text = host
        .runtime()
        .backend()
        .text_content(&button)
        .ok_or_else(|| "batched counter has no text".to_owned())?;

    let tree = host.runtime().backend().logical_tree();
    let a11y = host
        .a11y_snapshot()
        .nodes
        .into_iter()
        .map(|node| A11yNodeResult {
            role: match node.role {
                Role::Button => "button",
            }
            .to_owned(),
            name: node.name,
            text: node.text,
        })
        .collect();

    // Unmount: dispose listeners and effects, then detach the root.
    // 卸载：释放监听与 Effect，再分离根节点。
    let before_unmount = host.runtime_stats();
    host.dispose();
    let final_stats = host.runtime_stats();
    let unmount_delta = final_stats.delta_since(&before_unmount);
    let unmounted_matches = host.runtime().backend().find_all(&selector).len();
    let a11y_nodes = host.a11y_snapshot().nodes.len();
    let final_tree = host.runtime().backend().logical_tree();

    let mut operations = BTreeMap::new();
    for operation in host.runtime().backend().operation_log() {
        *operations.entry(operation.operation).or_insert(0) += 1;
    }

    Ok(ConformanceReport {
        schema: CONFORMANCE_SCHEMA,
        backend,
        tree,
        a11y,
        event: EventResult {
            handled: true,
            text_after,
            selector_matches,
        },
        checkpoints,
        operations,
        batch: BatchResult {
            increments: 100,
            final_text,
            set_text_delta: batch_delta.backend_set_text,
            nodes_created_delta: batch_delta.nodes_created,
            nodes_removed_delta: batch_delta.nodes_removed,
            frames_requested_delta: batch_delta.frames_requested,
        },
        unmount: UnmountResult {
            selector_matches: unmounted_matches,
            a11y_nodes,
            nodes_removed_delta: unmount_delta.nodes_removed,
        },
        final_tree,
        final_stats,
    })
}

/// Removes and reinserts the dynamic value node on the same backend.
/// 在同一后端移除并重新插入动态值节点。
pub fn run_detach_reinsert<B: HeadlessBackend + 'static>(
    mut host: HeadlessHost<B>,
) -> Result<DetachReinsertResult, String> {
    let backend = host.runtime().backend().backend_name().to_owned();
    let value_slot: Rc<RefCell<Option<B::Node>>> = Rc::new(RefCell::new(None));
    let value_capture = Rc::clone(&value_slot);
    let button = host.mount("counter/detach-reinsert", move |runtime, owner| {
        let (button, value, _count) = mount_counter_with_nodes(runtime, owner);
        *value_capture.borrow_mut() = Some(value);
        button
    });
    let value = value_slot
        .borrow_mut()
        .take()
        .ok_or_else(|| "detach probe did not expose the value node".to_owned())?;

    let tree_before = host.runtime().backend().logical_tree();
    host.runtime_mut().backend_mut().remove(&value);
    let text_detached = host.runtime().backend().text_content(&button);
    let tree_detached = host.runtime().backend().logical_tree();
    let detached_stats = host.runtime_stats();

    host.runtime_mut()
        .backend_mut()
        .insert_before(&button, &value, None);
    let text_restored = host.runtime().backend().text_content(&button);
    let tree_restored = host.runtime().backend().logical_tree();
    let restored_stats = host.runtime_stats();

    let mut operations = BTreeMap::new();
    for operation in host.runtime().backend().operation_log() {
        *operations.entry(operation.operation).or_insert(0) += 1;
    }

    Ok(DetachReinsertResult {
        schema: 1,
        backend,
        tree_before,
        tree_detached,
        tree_restored,
        text_detached,
        text_restored,
        detached_stats,
        restored_stats,
        operations,
    })
}

/// Returns the stable semantic selector used by every conformance backend.
/// 返回每个一致性后端使用的稳定语义选择器。
pub fn counter_selector() -> Selector {
    Selector {
        role: Role::Button,
        name: "Increment counter".to_owned(),
    }
}

/// Returns the expected logical tree independent of backend node handles.
/// 返回与后端节点句柄无关的预期逻辑树。
pub fn expected_tree() -> LogicalTreeSnapshot {
    use fino::headless::{DomNodeKind, LogicalNodeSnapshot};

    fn text(value: &str) -> LogicalNodeSnapshot {
        LogicalNodeSnapshot {
            kind: DomNodeKind::Text,
            tag: None,
            text: Some(value.to_owned()),
            attributes: BTreeMap::new(),
            children: Vec::new(),
        }
    }

    LogicalTreeSnapshot {
        schema: 1,
        roots: vec![LogicalNodeSnapshot {
            kind: DomNodeKind::Element,
            tag: Some("button".to_owned()),
            text: None,
            attributes: BTreeMap::from([("aria-label".to_owned(), "Increment counter".to_owned())]),
            children: vec![text("Count: "), text("!"), text("102")],
        }],
    }
}
