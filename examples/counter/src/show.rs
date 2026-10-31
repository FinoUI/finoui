//! Manual Show range scenario shared by Mock, Web, and Native.
//! Mock、Web 与 Native 共用的手工 Show 范围场景。

use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use fino::{
    dom_api::{Attribute, DomBackend, EventKind, Role, Selector, Tag},
    headless::{DomNodeKind, HeadlessBackend, HeadlessHost, LogicalTreeSnapshot},
    metrics::RuntimeStats,
    reactive::{Owner, Signal},
    runtime::Runtime,
    show::Show,
};
use serde::{Deserialize, Serialize};

/// Selector for the always-mounted toggle that flips the Show condition.
/// 始终挂载、用于切换 Show 条件的触发器选择器。
pub fn toggle_selector() -> Selector {
    Selector {
        role: Role::Button,
        name: "Show toggle".to_owned(),
    }
}

/// Selector for the conditional action inside the dynamic range.
/// 动态范围内条件动作的选择器。
pub fn action_selector() -> Selector {
    Selector {
        role: Role::Button,
        name: "Show action".to_owned(),
    }
}

/// Selector for the manual Show host root.
/// 手工 Show 宿主根节点的选择器。
pub fn host_selector() -> Selector {
    Selector {
        role: Role::Button,
        name: "Show host".to_owned(),
    }
}

struct ShowSignals {
    visible: Signal<bool>,
    count: Signal<i32>,
}

/// Mounts the manual Show probe and returns its root.
/// 挂载手工 Show 探针并返回其根节点。
pub fn mount_show<B: DomBackend + 'static>(runtime: &mut Runtime<B>, owner: &Owner) -> B::Node {
    mount_show_with_signals(runtime, owner).0
}

fn mount_show_with_signals<B: DomBackend + 'static>(
    runtime: &mut Runtime<B>,
    owner: &Owner,
) -> (B::Node, ShowSignals) {
    runtime.component_executions().increment();
    let visible = owner.signal_named("show/visible", false);
    let count = owner.signal_named("show/count", 0);

    let root = runtime.backend_mut().create_element(Tag::Button);
    runtime
        .backend_mut()
        .set_attribute(&root, Attribute::AriaLabel, Some("Show host"));
    let host_text = runtime.backend_mut().create_text("Host");
    runtime.backend_mut().insert_before(&root, &host_text, None);

    let toggle = runtime.backend_mut().create_element(Tag::Button);
    runtime
        .backend_mut()
        .set_attribute(&toggle, Attribute::AriaLabel, Some("Show toggle"));
    let toggle_text = runtime.backend_mut().create_text("Toggle");
    runtime
        .backend_mut()
        .insert_before(&toggle, &toggle_text, None);
    runtime.backend_mut().insert_before(&root, &toggle, None);
    let toggle_visible = visible.clone();
    runtime.listen(owner, &toggle, EventKind::Click, move |_event, _context| {
        toggle_visible.update(|value| *value = !*value);
    });

    let tail = runtime.backend_mut().create_text("Tail");
    runtime.backend_mut().insert_before(&root, &tail, None);

    let condition_visible = visible.clone();
    let action_count = count.clone();
    let show = Show::mount(
        runtime,
        owner,
        &root,
        Some(&tail),
        "show/details",
        move || condition_visible.get(),
        move |runtime, child| action_button(runtime, child, action_count.clone()),
    );
    owner.keep_alive(show);

    (root, ShowSignals { visible, count })
}

fn action_button<B: DomBackend + 'static>(
    runtime: &Runtime<B>,
    owner: &Owner,
    count: Signal<i32>,
) -> B::Node {
    let action = runtime.backend_mut().create_element(Tag::Button);
    runtime
        .backend_mut()
        .set_attribute(&action, Attribute::AriaLabel, Some("Show action"));
    let value = runtime.backend_mut().create_text("");
    runtime.backend_mut().insert_before(&action, &value, None);
    let read_count = count.clone();
    runtime.effect(owner, "show/action/text", move |context| {
        context
            .backend_mut()
            .set_text(&value, &format!("Value: {}", read_count.get()));
    });
    runtime.listen(owner, &action, EventKind::Click, move |_event, _context| {
        count.update(|number| *number += 1);
    });
    action
}

/// One observable checkpoint in the shared Show sequence.
/// 共享 Show 序列中的一个可观测检查点。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShowCheckpoint {
    /// Root descendant text.
    /// 根节点的后代文本。
    pub text: String,
    /// Matching conditional action nodes.
    /// 匹配的条件动作节点数。
    pub action_matches: usize,
    /// Live listener registrations.
    /// 仍存活的监听注册数。
    pub listeners: usize,
    /// Whether the child text effect still exists but is inactive.
    /// 子文本 Effect 是否仍存在但已停止。
    pub stale_effect_inactive: bool,
    /// Unified counters at this checkpoint.
    /// 该检查点的统一计数。
    pub stats: RuntimeStats,
}

/// Stable cross-backend report for one manual Show sequence.
/// 一次手工 Show 序列的稳定跨后端报告。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShowConformanceReport {
    /// Report schema version.
    /// 报告 schema 版本。
    pub schema: u32,
    /// Backend identity.
    /// 后端标识。
    pub backend: String,
    /// Initial logical tree with the condition false.
    /// 条件为 false 时的初始逻辑树。
    pub initial_tree: LogicalTreeSnapshot,
    /// Logical tree with the dynamic range mounted.
    /// 动态范围已挂载时的逻辑树。
    pub visible_tree: LogicalTreeSnapshot,
    /// Logical tree after the dynamic range is removed.
    /// 动态范围移除后的逻辑树。
    pub hidden_tree: LogicalTreeSnapshot,
    /// Logical tree after remounting the range.
    /// 重新挂载范围后的逻辑树。
    pub restored_tree: LogicalTreeSnapshot,
    /// Logical tree after the owner is disposed.
    /// Owner 释放后的逻辑树。
    pub final_tree: LogicalTreeSnapshot,
    /// Text at each lifecycle checkpoint.
    /// 各生命周期检查点的文本。
    pub checkpoints: BTreeMap<String, ShowCheckpoint>,
    /// Text after clicking the conditional action.
    /// 点击条件动作后的文本。
    pub clicked_text: String,
    /// Whether static sibling and anchor identifiers survived every toggle.
    /// 静态兄弟与锚点标识是否在每次切换后保持稳定。
    pub static_ids_stable: bool,
    /// Whether writing an old dependency after removal touched no backend.
    /// 移除后写入旧依赖是否完全没有触达后端。
    pub stale_write_is_noop: bool,
    /// Normalized backend operation counts.
    /// 规范化后端操作计数。
    pub operations: BTreeMap<String, u64>,
    /// Final cumulative runtime counters.
    /// 最终累计运行时计数。
    pub final_stats: RuntimeStats,
}

/// One failed Show contract check.
/// 一项未通过的 Show 契约检查。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShowViolation {
    /// Stable dotted field path.
    /// 稳定的点分字段路径。
    pub field: String,
    /// Human-readable expectation.
    /// 供人阅读的期望说明。
    pub message: String,
}

impl ShowConformanceReport {
    /// Validates the shared structural, event, stale-dependency, and counter contract.
    /// 校验共享结构、事件、旧依赖与计数契约。
    pub fn validate(&self) -> Vec<ShowViolation> {
        let mut violations = Vec::new();
        if self.schema != 1 {
            push(&mut violations, "schema", "Show report schema must be 1");
        }
        if self.initial_tree.roots.len() != 1 {
            push(
                &mut violations,
                "initial_tree",
                "one static host root is required",
            );
        }
        if self.visible_tree.roots.len() != 1 || self.hidden_tree.roots.len() != 1 {
            push(
                &mut violations,
                "toggle_tree",
                "the static host must remain the only root",
            );
        }
        if !self.final_tree.roots.is_empty() {
            push(
                &mut violations,
                "final_tree",
                "disposal must leave no reachable root",
            );
        }
        let initial = &self.checkpoints["initial"];
        let visible = &self.checkpoints["visible"];
        let clicked = &self.checkpoints["clicked"];
        let hidden = &self.checkpoints["hidden"];
        let restored = &self.checkpoints["restored"];
        check(
            &mut violations,
            "initial_text",
            initial.text.as_str(),
            "HostToggleTail",
        );
        check(
            &mut violations,
            "visible_text",
            visible.text.as_str(),
            "HostToggleValue: 0Tail",
        );
        check(
            &mut violations,
            "clicked_text",
            self.clicked_text.as_str(),
            "HostToggleValue: 1Tail",
        );
        check(
            &mut violations,
            "hidden_text",
            hidden.text.as_str(),
            "HostToggleTail",
        );
        check(
            &mut violations,
            "restored_text",
            restored.text.as_str(),
            "HostToggleValue: 9Tail",
        );
        check(&mut violations, "initial_action", initial.action_matches, 0);
        check(&mut violations, "visible_action", visible.action_matches, 1);
        check(&mut violations, "clicked_action", clicked.action_matches, 1);
        check(&mut violations, "hidden_action", hidden.action_matches, 0);
        check(
            &mut violations,
            "restored_action",
            restored.action_matches,
            1,
        );
        check(&mut violations, "initial_listeners", initial.listeners, 1);
        check(&mut violations, "visible_listeners", visible.listeners, 2);
        check(&mut violations, "hidden_listeners", hidden.listeners, 1);
        check(&mut violations, "restored_listeners", restored.listeners, 2);
        check(
            &mut violations,
            "hidden_stale_effect",
            hidden.stale_effect_inactive,
            true,
        );
        check(
            &mut violations,
            "static_ids_stable",
            self.static_ids_stable,
            true,
        );
        check(
            &mut violations,
            "stale_write_is_noop",
            self.stale_write_is_noop,
            true,
        );
        check(
            &mut violations,
            "component_executions",
            self.final_stats.component_executions,
            1,
        );
        check(
            &mut violations,
            "nodes_created",
            self.final_stats.nodes_created,
            9,
        );
        check(
            &mut violations,
            "nodes_removed",
            self.final_stats.nodes_removed,
            5,
        );
        check(
            &mut violations,
            "backend_set_text",
            self.final_stats.backend_set_text,
            3,
        );
        check(
            &mut violations,
            "create_anchor",
            self.operations.get("create_anchor").copied().unwrap_or(0),
            2,
        );
        check(
            &mut violations,
            "create_element",
            self.operations.get("create_element").copied().unwrap_or(0),
            4,
        );
        check(
            &mut violations,
            "create_text",
            self.operations.get("create_text").copied().unwrap_or(0),
            5,
        );
        check(
            &mut violations,
            "listen",
            self.operations.get("listen").copied().unwrap_or(0),
            3,
        );
        check(
            &mut violations,
            "unlisten",
            self.operations.get("unlisten").copied().unwrap_or(0),
            3,
        );
        check(
            &mut violations,
            "remove",
            self.operations.get("remove").copied().unwrap_or(0),
            5,
        );
        violations
    }
}

fn check<T: PartialEq + std::fmt::Debug>(
    violations: &mut Vec<ShowViolation>,
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

fn push(violations: &mut Vec<ShowViolation>, field: &str, message: &str) {
    violations.push(ShowViolation {
        field: field.to_owned(),
        message: message.to_owned(),
    });
}

/// Runs the same manual Show sequence on any headless backend.
/// 在任意无头后端运行同一套手工 Show 序列。
pub fn run_show_conformance<B: HeadlessBackend + 'static>(
    mut host: HeadlessHost<B>,
) -> Result<ShowConformanceReport, String> {
    let backend = host.runtime().backend().backend_name().to_owned();
    let state: Rc<RefCell<Option<ShowSignals>>> = Rc::new(RefCell::new(None));
    let capture = Rc::clone(&state);
    let root = host.mount("show/conformance", move |runtime, owner| {
        let (root, signals) = mount_show_with_signals(runtime, owner);
        *capture.borrow_mut() = Some(signals);
        root
    });
    let signals = state
        .borrow_mut()
        .take()
        .ok_or_else(|| "Show mount did not expose its signals".to_owned())?;

    let initial_ids = static_ids(&host);
    let initial_tree = host.runtime().backend().logical_tree();
    let mut checkpoints = BTreeMap::new();
    checkpoints.insert("initial".to_owned(), checkpoint(&host, &root));

    signals.visible.set(true);
    checkpoints.insert("visible".to_owned(), checkpoint(&host, &root));
    let visible_tree = host.runtime().backend().logical_tree();
    let ids_after_show = static_ids(&host);

    let before_click = host.runtime_stats();
    host.click(&action_selector())
        .map_err(|error| format!("{}: {}", error.code, error.message))?;
    let after_click = host.runtime_stats();
    let clicked_text = host
        .runtime()
        .backend()
        .text_content(&root)
        .ok_or_else(|| "Show host lost its text".to_owned())?;
    if after_click.component_executions != before_click.component_executions {
        return Err("clicking the Show action reran the component".to_owned());
    }
    checkpoints.insert("clicked".to_owned(), checkpoint(&host, &root));

    let hidden_tree_before = host.runtime_stats();
    signals.visible.set(false);
    let hidden_tree = host.runtime().backend().logical_tree();
    let hidden_stats = host.runtime_stats();
    if hidden_stats.nodes_created != hidden_tree_before.nodes_created {
        return Err("hiding the range created backend nodes".to_owned());
    }
    checkpoints.insert("hidden".to_owned(), checkpoint(&host, &root));

    let operations_before_stale_write = host.backend_operation_count();
    let stats_before_stale_write = host.runtime_stats();
    signals.count.set(9);
    let stats_after_stale_write = host.runtime_stats();
    let stale_write_is_noop = host.backend_operation_count() == operations_before_stale_write
        && stats_after_stale_write.backend_set_text == stats_before_stale_write.backend_set_text
        && stats_after_stale_write.frames_requested == stats_before_stale_write.frames_requested;
    if !stale_write_is_noop {
        return Err("writing the removed range dependency touched the backend".to_owned());
    }

    signals.visible.set(true);
    let restored_tree = host.runtime().backend().logical_tree();
    let ids_after_restore = static_ids(&host);
    let static_ids_stable = initial_ids == ids_after_show && initial_ids == ids_after_restore;
    checkpoints.insert("restored".to_owned(), checkpoint(&host, &root));

    let before_dispose = host.runtime_stats();
    host.dispose();
    let final_tree = host.runtime().backend().logical_tree();
    let final_stats = host.runtime_stats();
    if final_stats.component_executions != before_dispose.component_executions {
        return Err("disposal reran the Show component".to_owned());
    }

    let mut operations = BTreeMap::new();
    for operation in host.runtime().backend().operation_log() {
        *operations.entry(operation.operation).or_insert(0) += 1;
    }

    Ok(ShowConformanceReport {
        schema: 1,
        backend,
        initial_tree,
        visible_tree,
        hidden_tree,
        restored_tree,
        final_tree,
        checkpoints,
        clicked_text,
        static_ids_stable,
        stale_write_is_noop,
        operations,
        final_stats,
    })
}

fn checkpoint<B: HeadlessBackend + 'static>(
    host: &HeadlessHost<B>,
    root: &B::Node,
) -> ShowCheckpoint {
    let text = host
        .runtime()
        .backend()
        .text_content(root)
        .unwrap_or_default();
    let action_matches = host.runtime().backend().find_all(&action_selector()).len();
    let listeners = host.runtime().backend().listener_count();
    let stale_effect_inactive = host
        .reactive_snapshot()
        .effects
        .iter()
        .any(|effect| effect.name == "show/action/text" && !effect.active);
    ShowCheckpoint {
        text,
        action_matches,
        listeners,
        stale_effect_inactive,
        stats: host.runtime_stats(),
    }
}

/// Collects stable identifiers for every static sibling and range anchor.
/// 收集每个静态兄弟与范围锚点的稳定标识。
fn static_ids<B: HeadlessBackend + 'static>(host: &HeadlessHost<B>) -> Vec<String> {
    let snapshot = host.dom_snapshot();
    let mut ids = Vec::new();
    for node in snapshot.nodes {
        let static_text = node.text.as_deref() == Some("Host")
            || node.text.as_deref() == Some("Tail")
            || node.text.as_deref() == Some("Toggle");
        let toggle = node.attributes.get("aria-label").map(String::as_str) == Some("Show toggle");
        if static_text || toggle || node.kind == DomNodeKind::Anchor {
            ids.push(node.id);
        }
    }
    ids
}
