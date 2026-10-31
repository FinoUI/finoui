//! Manual keyed For scenario shared by Mock, Web, and Native.
//! Mock、Web 与 Native 共用的手工 keyed For 场景。

use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

use fino::{
    dom_api::{Attribute, DomBackend, EventKind, Role, Selector, Tag},
    headless::{HeadlessBackend, HeadlessHost, LogicalTreeSnapshot},
    keyed::{For, ForContext},
    metrics::RuntimeStats,
    reactive::{Owner, Signal},
    runtime::Runtime,
};
use serde::{Deserialize, Serialize};

/// Ordered key phases exercised by every backend.
/// 每个后端都会执行的按序 key 阶段。
pub const PHASES: &[&[&str]] = &[
    &[],
    &["A", "B", "C"],
    &["X", "A", "B", "C"],
    &["X", "A", "M", "B", "C"],
    &["X", "M", "A", "B", "C"],
    &["C", "B", "A", "M", "X"],
    &["B", "A", "M", "X", "C"],
    &["B", "M", "X", "C"],
    &["B", "Q", "X", "C"],
    &["B", "Q", "X", "C"],
    &[],
    &["D", "E"],
];

/// Stable phase names used in the conformance report.
/// 一致性报告使用的稳定阶段名。
pub const PHASE_NAMES: &[&str] = &[
    "empty",
    "append",
    "prepend",
    "middle_insert",
    "swap",
    "reverse",
    "rotation",
    "delete",
    "replace",
    "equal_write",
    "clear",
    "readd",
];

/// Returns the always-mounted advance button selector.
/// 返回始终挂载的推进按钮选择器。
pub fn next_selector() -> Selector {
    Selector {
        role: Role::Button,
        name: "Keyed next".to_owned(),
    }
}

/// Returns the row selector for one key.
/// 返回单个 key 的行选择器。
pub fn row_selector(key: &str) -> Selector {
    Selector {
        role: Role::Button,
        name: format!("Row {key}"),
    }
}

/// One mounted keyed probe with handles only to its own runtime.
/// 一个已挂载的 keyed 探针，只持有自己 Runtime 的句柄。
pub struct KeyedMount<B: DomBackend + 'static> {
    /// Host root created once for every phase.
    /// 每个阶段共用的宿主根节点。
    pub root: B::Node,
    list: Rc<For<B, String>>,
    keys: Signal<Vec<String>>,
    step: Rc<Cell<usize>>,
    runs: Rc<RefCell<BTreeMap<String, u32>>>,
    handles: Rc<RefCell<BTreeMap<String, B::Node>>>,
    values: Rc<RefCell<BTreeMap<String, B::Node>>>,
    locals: Rc<RefCell<BTreeMap<String, Signal<i32>>>>,
}

impl<B: DomBackend + 'static> KeyedMount<B> {
    /// Returns the committed keys.
    /// 返回已提交 key。
    pub fn keys(&self) -> Vec<String> {
        self.list.keys()
    }

    /// Returns the current phase index.
    /// 返回当前阶段索引。
    pub fn step(&self) -> usize {
        self.step.get()
    }

    /// Replaces the key list directly for duplicate and recovery probes.
    /// 直接替换 key 列表，用于重复 key 与恢复探针。
    pub fn set_keys(&self, keys: Vec<String>) {
        self.keys.set(keys);
    }

    /// Returns the last duplicate-key rejection.
    /// 返回最近一次重复 key 拒绝。
    pub fn last_error_code(&self) -> Option<String> {
        self.list.last_error().map(|error| error.code)
    }

    /// Returns one item root handle.
    /// 返回单个条目根句柄。
    pub fn item_handle(&self, key: &str) -> Option<B::Node> {
        self.handles.borrow().get(key).cloned()
    }

    /// Returns one item text handle.
    /// 返回单个条目文本句柄。
    pub fn item_text(&self, key: &str) -> Option<B::Node> {
        self.values.borrow().get(key).cloned()
    }

    /// Returns one item local count signal.
    /// 返回单个条目的局部计数 Signal。
    pub fn local_signal(&self, key: &str) -> Option<Signal<i32>> {
        self.locals.borrow().get(key).cloned()
    }

    /// Returns how often each live key builder ran.
    /// 返回每个仍存活 key 的构建器运行次数。
    pub fn builder_runs(&self) -> BTreeMap<String, u32> {
        self.runs.borrow().clone()
    }

    /// Returns the keys still retained by probe instrumentation.
    /// 返回探针仍保留的 key。
    pub fn probe_keys(&self) -> Vec<String> {
        let mut keys = std::collections::BTreeSet::new();
        keys.extend(self.runs.borrow().keys().cloned());
        keys.extend(self.handles.borrow().keys().cloned());
        keys.extend(self.values.borrow().keys().cloned());
        keys.extend(self.locals.borrow().keys().cloned());
        keys.into_iter().collect()
    }

    /// Returns the stable list anchors.
    /// 返回稳定的列表锚点。
    pub fn anchors(&self) -> fino::show::RangeAnchor<B> {
        self.list.anchors()
    }

    /// Releases every item and the list anchors.
    /// 释放全部条目与列表锚点。
    pub fn dispose(&self) {
        self.list.dispose();
    }
}

struct KeyedParts<B: DomBackend + 'static> {
    runs: Rc<RefCell<BTreeMap<String, u32>>>,
    handles: Rc<RefCell<BTreeMap<String, B::Node>>>,
    values: Rc<RefCell<BTreeMap<String, B::Node>>>,
    locals: Rc<RefCell<BTreeMap<String, Signal<i32>>>>,
}

impl<B: DomBackend + 'static> Clone for KeyedParts<B> {
    fn clone(&self) -> Self {
        Self {
            runs: Rc::clone(&self.runs),
            handles: Rc::clone(&self.handles),
            values: Rc::clone(&self.values),
            locals: Rc::clone(&self.locals),
        }
    }
}

/// Mounts the manual keyed For probe and returns its root plus item handles.
/// 挂载手工 keyed For 探针，并返回其根节点与条目句柄。
pub fn mount_keyed<B: DomBackend + 'static>(
    runtime: &mut Runtime<B>,
    owner: &Owner,
) -> KeyedMount<B> {
    runtime.component_executions().increment();
    let keys = owner.signal_named("keyed/keys", Vec::new());
    let step = Rc::new(Cell::new(0));

    let root = runtime.backend_mut().create_element(Tag::Button);
    runtime
        .backend_mut()
        .set_attribute(&root, Attribute::AriaLabel, Some("Keyed host"));
    let host_text = runtime.backend_mut().create_text("Host");
    runtime.backend_mut().insert_before(&root, &host_text, None);

    let next = runtime.backend_mut().create_element(Tag::Button);
    runtime
        .backend_mut()
        .set_attribute(&next, Attribute::AriaLabel, Some("Keyed next"));
    let next_text = runtime.backend_mut().create_text("Next");
    runtime.backend_mut().insert_before(&next, &next_text, None);
    runtime.backend_mut().insert_before(&root, &next, None);

    let advance_keys = keys.clone();
    let advance_step = Rc::clone(&step);
    runtime.listen(owner, &next, EventKind::Click, move |_event, _context| {
        let next_step = (advance_step.get() + 1).min(PHASES.len() - 1);
        advance_step.set(next_step);
        advance_keys.set(keys_of(PHASES[next_step]));
    });

    let tail = runtime.backend_mut().create_text("Tail");
    runtime.backend_mut().insert_before(&root, &tail, None);

    let parts = KeyedParts {
        runs: Rc::new(RefCell::new(BTreeMap::new())),
        handles: Rc::new(RefCell::new(BTreeMap::new())),
        values: Rc::new(RefCell::new(BTreeMap::new())),
        locals: Rc::new(RefCell::new(BTreeMap::new())),
    };
    let read_keys = keys.clone();
    let builder_parts = parts.clone();
    let list = For::mount(
        runtime,
        owner,
        &root,
        Some(&tail),
        "keyed/rows",
        move || read_keys.get(),
        move |context, key| item_row(context, key, &builder_parts),
    );
    let list = Rc::new(list);
    owner.keep_alive(Rc::clone(&list));

    KeyedMount {
        root,
        list,
        keys,
        step,
        runs: parts.runs,
        handles: parts.handles,
        values: parts.values,
        locals: parts.locals,
    }
}

/// Mounts the keyed probe and returns only its root for scenario runners.
/// 挂载 keyed 探针，并只返回根节点供场景运行器使用。
pub fn mount_keyed_root<B: DomBackend + 'static>(
    runtime: &mut Runtime<B>,
    owner: &Owner,
) -> B::Node {
    mount_keyed(runtime, owner).root
}

/// Builds one item with a stable root, text binding, local signal, and listener.
/// 构建一个具有稳定根节点、文本绑定、局部 Signal 与监听的条目。
fn item_row<B: DomBackend + 'static>(
    context: &mut ForContext<B>,
    key: &str,
    parts: &KeyedParts<B>,
) -> Vec<B::Node> {
    *parts.runs.borrow_mut().entry(key.to_owned()).or_insert(0) += 1;
    let button = context.create_element(Tag::Button);
    context.set_attribute(&button, Attribute::AriaLabel, Some(&format!("Row {key}")));
    let value = context.create_text("");
    context.insert_before(&button, &value, None);
    let local = context
        .owner()
        .signal_named(&format!("keyed/{key}/count"), 0_i32);
    let read = local.clone();
    let label = key.to_owned();
    let bound = value.clone();
    context.effect(&format!("keyed/{key}/text"), move |effect| {
        effect.set_text(&bound, &format!("{label}={}", read.get()));
    });
    let click = local.clone();
    context.listen(&button, EventKind::Click, move |_event, _context| {
        click.update(|count| *count += 1);
    });
    parts
        .handles
        .borrow_mut()
        .insert(key.to_owned(), button.clone());
    parts.values.borrow_mut().insert(key.to_owned(), value);
    parts.locals.borrow_mut().insert(key.to_owned(), local);
    let removed_key = key.to_owned();
    let handles = Rc::clone(&parts.handles);
    let values = Rc::clone(&parts.values);
    let locals = Rc::clone(&parts.locals);
    let runs = Rc::clone(&parts.runs);
    context.on_dispose(move || {
        handles.borrow_mut().remove(&removed_key);
        values.borrow_mut().remove(&removed_key);
        locals.borrow_mut().remove(&removed_key);
        runs.borrow_mut().remove(&removed_key);
    });
    vec![button]
}

/// Converts one phase to owned keys.
/// 把一个阶段转换为拥有所有权的 key。
pub fn keys_of(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|key| (*key).to_owned()).collect()
}

/// Returns the exact host text for one key list with all local counts at zero.
/// 返回所有局部计数为零时某个 key 列表对应的精确宿主文本。
pub fn expected_text(keys: &[String]) -> String {
    let mut text = String::from("HostNext");
    for key in keys {
        text.push_str(key);
        text.push_str("=0");
    }
    text.push_str("Tail");
    text
}

/// Extracts row keys from a logical tree in document order.
/// 按文档顺序从逻辑树提取行 key。
pub fn row_keys(tree: &LogicalTreeSnapshot) -> Vec<String> {
    tree.roots
        .first()
        .into_iter()
        .flat_map(|root| &root.children)
        .filter_map(|child| child.attributes.get("aria-label"))
        .filter_map(|label| label.strip_prefix("Row "))
        .map(str::to_owned)
        .collect()
}

/// One observable phase in the shared keyed sequence.
/// 共享 keyed 序列中的一个可观测阶段。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyedPhase {
    /// Stable phase name.
    /// 稳定阶段名。
    pub name: String,
    /// Expected committed keys.
    /// 期望的已提交 key。
    pub expected_keys: Vec<String>,
    /// Host descendant text.
    /// 宿主后代文本。
    pub text: String,
    /// Row keys in document order.
    /// 按文档顺序排列的行 key。
    pub rows: Vec<String>,
    /// Backend operation counts caused by this phase.
    /// 该阶段引起的后端操作计数。
    pub operations: BTreeMap<String, u64>,
    /// Whether static siblings, anchors, and retained item handles stayed stable.
    /// 静态兄弟、锚点与保留条目句柄是否保持稳定。
    pub ids_stable: bool,
    /// How often retained key builders reran.
    /// 保留 key 的构建器重跑次数。
    pub builder_reruns: u32,
    /// Live listener registrations.
    /// 仍存活的监听注册数。
    pub listeners: usize,
}

/// Result of one item-local signal update.
/// 一次条目局部 Signal 更新的结果。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyedLocalUpdate {
    /// Clicked key.
    /// 被点击的 key。
    pub key: String,
    /// Host text after the click.
    /// 点击后的宿主文本。
    pub text: String,
    /// Text commits during the click.
    /// 点击期间的文本提交数。
    pub set_text_delta: u64,
    /// Whole-list reconciliation executions during the click.
    /// 点击期间的整表协调执行次数。
    pub reconcile_delta: u64,
    /// Builder reruns during the click.
    /// 点击期间的构建器重跑次数。
    pub builder_reruns: u32,
    /// Whether the clicked item kept its root and text handle.
    /// 被点击条目是否保留根节点与文本句柄。
    pub identity_stable: bool,
}

/// Stable cross-backend report for one manual keyed For sequence.
/// 一次手工 keyed For 序列的稳定跨后端报告。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyedConformanceReport {
    /// Report schema version.
    /// 报告 schema 版本。
    pub schema: u32,
    /// Backend identity.
    /// 后端标识。
    pub backend: String,
    /// Initial logical tree.
    /// 初始逻辑树。
    pub initial_tree: LogicalTreeSnapshot,
    /// Logical tree after every phase and the local update.
    /// 所有阶段与局部更新后的逻辑树。
    pub reordered_tree: LogicalTreeSnapshot,
    /// Logical tree after whole-list disposal.
    /// 整表释放后的逻辑树。
    pub disposed_tree: LogicalTreeSnapshot,
    /// Logical tree after host disposal.
    /// 宿主释放后的逻辑树。
    pub final_tree: LogicalTreeSnapshot,
    /// Initial text and counters.
    /// 初始文本与计数。
    pub initial_text: String,
    /// Every phase in order.
    /// 按顺序排列的每个阶段。
    pub phases: Vec<KeyedPhase>,
    /// Item-local update evidence.
    /// 条目局部更新证据。
    pub local_update: KeyedLocalUpdate,
    /// Whether writing a removed key's signal touched no backend.
    /// 写入已移除 key 的 Signal 是否完全没有触达后端。
    pub stale_write_is_noop: bool,
    /// Whether a duplicate key update was rejected without backend changes.
    /// 重复 key 更新是否在没有后端变更的情况下被拒绝。
    pub duplicate_key_rejected: bool,
    /// Whether a valid update after rejection recovered.
    /// 拒绝后的合法更新是否恢复。
    pub duplicate_recovered: bool,
    /// Whether repeated whole-list disposal issued no extra operations.
    /// 重复整表释放是否没有额外操作。
    pub dispose_idempotent: bool,
    /// Whether static siblings and For anchors kept their ids.
    /// 静态兄弟与 For 锚点是否保持标识。
    pub static_ids_stable: bool,
    /// Whether retained items kept their root and text identities.
    /// 保留条目是否保持根节点与文本身份。
    pub item_identity_stable: bool,
    /// Final cumulative builder runs.
    /// 最终累计构建器运行次数。
    pub builder_runs: BTreeMap<String, u32>,
    /// Whole-list reconciliation executions.
    /// 整表协调执行次数。
    pub reconcile_executions: u64,
    /// Whether probe instrumentation retains only live keys.
    /// 探针是否只为仍存活 key 保留数据。
    pub probe_storage_bounded: bool,
    /// Normalized backend operation counts.
    /// 规范化后端操作计数。
    pub operations: BTreeMap<String, u64>,
    /// Final cumulative runtime counters.
    /// 最终累计运行时计数。
    pub final_stats: RuntimeStats,
}

/// One failed keyed For contract check.
/// 一项未通过的 keyed For 契约检查。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyedViolation {
    /// Stable dotted field path.
    /// 稳定的点分字段路径。
    pub field: String,
    /// Human-readable expectation.
    /// 供人阅读的期望说明。
    pub message: String,
}

impl KeyedConformanceReport {
    /// Validates identity, move, lifecycle, duplicate, and counter contracts.
    /// 校验身份、移动、生命周期、重复 key 与计数契约。
    pub fn validate(&self) -> Vec<KeyedViolation> {
        let mut violations = Vec::new();
        check(&mut violations, "schema", self.schema, 1);
        check(
            &mut violations,
            "initial_text",
            self.initial_text.as_str(),
            "HostNextTail",
        );
        if self.phases.len() != PHASE_NAMES.len() - 1 {
            push(
                &mut violations,
                "phases",
                "every transition phase must be recorded",
            );
        }
        for (index, phase) in self.phases.iter().enumerate() {
            let name = PHASE_NAMES[index + 1];
            check(
                &mut violations,
                &format!("phases.{name}.name"),
                phase.name.as_str(),
                name,
            );
            let expected_keys = keys_of(PHASES[index + 1]);
            check(
                &mut violations,
                &format!("phases.{name}.keys"),
                &phase.expected_keys,
                &expected_keys,
            );
            check(
                &mut violations,
                &format!("phases.{name}.rows"),
                &phase.rows,
                &expected_keys,
            );
            check(
                &mut violations,
                &format!("phases.{name}.text"),
                phase.text.as_str(),
                expected_text(&expected_keys).as_str(),
            );
            check(
                &mut violations,
                &format!("phases.{name}.ids_stable"),
                phase.ids_stable,
                true,
            );
            check(
                &mut violations,
                &format!("phases.{name}.builder_reruns"),
                phase.builder_reruns,
                0,
            );
            let expected_listeners = match name {
                "append" => 4,
                "prepend" => 5,
                "middle_insert" | "swap" | "reverse" | "rotation" => 6,
                "delete" | "replace" | "equal_write" => 5,
                "clear" => 1,
                "readd" => 3,
                _ => 0,
            };
            check(
                &mut violations,
                &format!("phases.{name}.listeners"),
                phase.listeners,
                expected_listeners,
            );
            let expected_ops = expected_phase_operations(name);
            if phase.operations != expected_ops {
                push(
                    &mut violations,
                    &format!("phases.{name}.operations"),
                    "phase operation counts must match the shared keyed sequence",
                );
            }
        }
        check(
            &mut violations,
            "local_update.key",
            self.local_update.key.as_str(),
            "D",
        );
        check(
            &mut violations,
            "local_update.text",
            self.local_update.text.as_str(),
            "HostNextD=1E=0Tail",
        );
        check(
            &mut violations,
            "local_update.set_text_delta",
            self.local_update.set_text_delta,
            1,
        );
        check(
            &mut violations,
            "local_update.reconcile_delta",
            self.local_update.reconcile_delta,
            0,
        );
        check(
            &mut violations,
            "local_update.builder_reruns",
            self.local_update.builder_reruns,
            0,
        );
        check(
            &mut violations,
            "local_update.identity_stable",
            self.local_update.identity_stable,
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
            "duplicate_key_rejected",
            self.duplicate_key_rejected,
            true,
        );
        check(
            &mut violations,
            "duplicate_recovered",
            self.duplicate_recovered,
            true,
        );
        check(
            &mut violations,
            "dispose_idempotent",
            self.dispose_idempotent,
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
            "item_identity_stable",
            self.item_identity_stable,
            true,
        );
        check(
            &mut violations,
            "reconcile_executions",
            self.reconcile_executions,
            14,
        );
        check(
            &mut violations,
            "probe_storage_bounded",
            self.probe_storage_bounded,
            true,
        );
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
            23,
        );
        check(
            &mut violations,
            "final_stats.backend_set_text",
            self.final_stats.backend_set_text,
            9,
        );
        check(
            &mut violations,
            "final_stats.nodes_created",
            self.final_stats.nodes_created,
            21,
        );
        check(
            &mut violations,
            "final_stats.nodes_removed",
            self.final_stats.nodes_removed,
            35,
        );
        if self.disposed_tree.roots.len() != 1 {
            push(
                &mut violations,
                "disposed_tree.roots",
                "whole-list disposal must keep only the static host",
            );
        }
        if !self.final_tree.roots.is_empty() {
            push(
                &mut violations,
                "final_tree.roots",
                "host disposal must leave no reachable root",
            );
        }
        let expected_ops = expected_operations();
        if self.operations != expected_ops {
            push(
                &mut violations,
                "operations",
                "normalized backend operation counts must match the shared keyed sequence",
            );
        }
        violations
    }
}

fn check<T: PartialEq + std::fmt::Debug>(
    violations: &mut Vec<KeyedViolation>,
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

fn push(violations: &mut Vec<KeyedViolation>, field: &str, message: &str) {
    violations.push(KeyedViolation {
        field: field.to_owned(),
        message: message.to_owned(),
    });
}

/// Returns the exact per-phase backend operation deltas.
/// 返回每个阶段的精确后端操作增量。
pub fn expected_phase_operations(name: &str) -> BTreeMap<String, u64> {
    let mut operations = BTreeMap::new();
    let entries: &[(&str, u64)] = match name {
        "append" => &[
            ("create_anchor", 6),
            ("create_element", 3),
            ("create_text", 3),
            ("insert", 12),
            ("listen", 3),
            ("set_attribute", 3),
            ("set_text", 3),
        ],
        "prepend" | "middle_insert" => &[
            ("create_anchor", 2),
            ("create_element", 1),
            ("create_text", 1),
            ("insert", 4),
            ("listen", 1),
            ("set_attribute", 1),
            ("set_text", 1),
        ],
        "swap" => &[("insert", 3)],
        "reverse" => &[("insert", 12)],
        "rotation" => &[("insert", 3)],
        "delete" => &[("release", 4), ("unlisten", 1)],
        "replace" => &[
            ("create_anchor", 2),
            ("create_element", 1),
            ("create_text", 1),
            ("insert", 4),
            ("listen", 1),
            ("release", 4),
            ("set_attribute", 1),
            ("set_text", 1),
            ("unlisten", 1),
        ],
        "equal_write" => &[],
        "clear" => &[("release", 16), ("unlisten", 4)],
        "readd" => &[
            ("create_anchor", 4),
            ("create_element", 2),
            ("create_text", 2),
            ("insert", 8),
            ("listen", 2),
            ("set_attribute", 2),
            ("set_text", 2),
        ],
        _ => &[],
    };
    for (operation, count) in entries {
        operations.insert((*operation).to_owned(), *count);
    }
    operations
}

/// Returns the cumulative normalized operation counts for the shared sequence.
/// 返回共享序列的累计规范化操作计数。
pub fn expected_operations() -> BTreeMap<String, u64> {
    [
        ("create_anchor", 18),
        ("create_element", 10),
        ("create_text", 11),
        ("insert", 59),
        ("listen", 9),
        ("release", 34),
        ("remove", 1),
        ("set_attribute", 10),
        ("set_text", 9),
        ("unlisten", 9),
    ]
    .into_iter()
    .map(|(name, count)| (name.to_owned(), count))
    .collect()
}

/// Runs the same keyed sequence on any headless backend.
/// 在任意无头后端运行同一套 keyed 序列。
pub fn run_keyed_conformance<B: HeadlessBackend + 'static>(
    mut host: HeadlessHost<B>,
) -> Result<KeyedConformanceReport, String> {
    let backend = host.runtime().backend().backend_name().to_owned();
    let slot: Rc<RefCell<Option<KeyedMount<B>>>> = Rc::new(RefCell::new(None));
    let capture = Rc::clone(&slot);
    let root = host.mount("keyed/conformance", move |runtime, owner| {
        let mount = mount_keyed(runtime, owner);
        let root = mount.root.clone();
        *capture.borrow_mut() = Some(mount);
        root
    });
    let mount = slot
        .borrow_mut()
        .take()
        .ok_or_else(|| "keyed mount did not expose its handles".to_owned())?;

    let initial_tree = host.runtime().backend().logical_tree();
    let initial_text = text_of(&host, &root)?;
    let initial_static = static_node_ids(&host);
    let initial_anchors = (
        mount.anchors().start().clone(),
        mount.anchors().end().clone(),
    );
    let mut previous_keys = mount.keys();
    let mut previous_handles = item_handles(&mount, &previous_keys);
    let mut previous_texts = item_texts(&mount, &previous_keys);
    let mut previous_runs = mount.builder_runs();
    let mut item_identity_stable = true;
    let mut static_ids_stable = true;
    let mut phases = Vec::new();
    let mut stale_signal = None;

    for index in 1..PHASES.len() {
        let name = PHASE_NAMES[index];
        if name == "delete" {
            stale_signal = mount.local_signal("A");
        }
        let before_operations = operation_counts(&host);
        let before_reconcile = reconcile_executions(&host);
        host.click(&next_selector())
            .map_err(|error| format!("{}: {}", error.code, error.message))?;
        let after_operations = operation_counts(&host);
        let after_reconcile = reconcile_executions(&host);
        let expected_keys = keys_of(PHASES[index]);
        let actual_keys = mount.keys();
        if actual_keys != expected_keys {
            return Err(format!(
                "phase {name} committed {:?}, expected {:?}",
                actual_keys, expected_keys
            ));
        }
        let after_handles = item_handles(&mount, &actual_keys);
        let after_texts = item_texts(&mount, &actual_keys);
        let after_runs = mount.builder_runs();
        let mut phase_builder_reruns = 0;
        for key in &previous_keys {
            if !expected_keys.contains(key) {
                continue;
            }
            phase_builder_reruns += after_runs.get(key).copied().unwrap_or(0)
                - previous_runs.get(key).copied().unwrap_or(0);
            if previous_handles.get(key) != after_handles.get(key)
                || previous_texts.get(key) != after_texts.get(key)
            {
                item_identity_stable = false;
            }
        }
        for key in expected_keys
            .iter()
            .filter(|key| !previous_keys.contains(key))
        {
            if after_runs.get(key).copied().unwrap_or(0) != 1 {
                return Err(format!("new key {key} must build exactly once"));
            }
        }
        let current_static = static_node_ids(&host);
        let current_anchors = (
            mount.anchors().start().clone(),
            mount.anchors().end().clone(),
        );
        if current_static != initial_static || current_anchors != initial_anchors {
            static_ids_stable = false;
        }
        let tree = host.runtime().backend().logical_tree();
        let rows = row_keys(&tree);
        if rows != expected_keys {
            return Err(format!("phase {name} rows {:?} do not match keys", rows));
        }
        let text = text_of(&host, &root)?;
        if text != expected_text(&expected_keys) {
            return Err(format!(
                "phase {name} text {text:?} does not match the shared sequence"
            ));
        }
        let operations = delta(&after_operations, &before_operations);
        let _ = after_reconcile - before_reconcile;
        phases.push(KeyedPhase {
            name: name.to_owned(),
            expected_keys: expected_keys.clone(),
            text,
            rows,
            operations,
            ids_stable: current_static == initial_static
                && current_anchors == initial_anchors
                && item_identity_stable,
            builder_reruns: phase_builder_reruns,
            listeners: host.runtime().backend().listener_count(),
        });
        previous_keys = actual_keys;
        previous_handles = after_handles;
        previous_texts = after_texts;
        previous_runs = after_runs;
    }

    // Item-local update: one target text commits and reconciliation never runs.
    // 条目局部更新：只提交一个目标文本，协调绝不运行。
    let local_key = "D";
    let local_before_ops = operation_counts(&host);
    let local_before_reconcile = reconcile_executions(&host);
    let local_before_runs = mount.builder_runs();
    let local_before_handle = mount.item_handle(local_key);
    let local_before_text = mount.item_text(local_key);
    host.click(&row_selector(local_key))
        .map_err(|error| format!("{}: {}", error.code, error.message))?;
    let local_after_ops = operation_counts(&host);
    let local_after_reconcile = reconcile_executions(&host);
    let local_after_runs = mount.builder_runs();
    let local_delta = delta(&local_after_ops, &local_before_ops);
    let local_update = KeyedLocalUpdate {
        key: local_key.to_owned(),
        text: text_of(&host, &root)?,
        set_text_delta: local_delta.get("set_text").copied().unwrap_or(0),
        reconcile_delta: local_after_reconcile - local_before_reconcile,
        builder_reruns: local_after_runs
            .values()
            .sum::<u32>()
            .saturating_sub(local_before_runs.values().sum::<u32>()),
        identity_stable: local_before_handle == mount.item_handle(local_key)
            && local_before_text == mount.item_text(local_key),
    };

    // Stale signal write after a key was removed must not reach the backend.
    // 已移除 key 的旧 Signal 写入不得触达后端。
    let _ = host.runtime().backend().take_frame_request();
    let stale_operations = operation_counts(&host);
    if let Some(stale) = stale_signal {
        stale.set(9);
    }
    let stale_after = operation_counts(&host);
    let stale_frame = host.runtime().backend().take_frame_request();
    let stale_write_is_noop = stale_operations == stale_after && !stale_frame;

    // Duplicate rejection: no backend operations, no builder runs, same order.
    // 重复 key 拒绝：没有后端操作、没有构建器运行，顺序不变。
    let duplicate_before_ops = operation_counts(&host);
    let duplicate_before_runs = mount.builder_runs();
    let duplicate_before_keys = mount.keys();
    mount.set_keys(vec!["D".to_owned(), "D".to_owned()]);
    let duplicate_after_ops = operation_counts(&host);
    let duplicate_after_runs = mount.builder_runs();
    let duplicate_key_rejected = mount.last_error_code().as_deref() == Some("UI_FOR_DUPLICATE_KEY")
        && duplicate_after_ops == duplicate_before_ops
        && duplicate_after_runs == duplicate_before_runs
        && mount.keys() == duplicate_before_keys;

    // A valid update after rejection must recover without rebuilding items.
    // 拒绝后的合法更新必须在不重建条目的情况下恢复。
    let recovery_before_runs = mount.builder_runs();
    mount.set_keys(vec!["E".to_owned(), "D".to_owned()]);
    let recovery_after_runs = mount.builder_runs();
    let duplicate_recovered = mount.last_error_code().is_none()
        && mount.keys() == vec!["E".to_owned(), "D".to_owned()]
        && recovery_before_runs == recovery_after_runs
        && row_keys(&host.runtime().backend().logical_tree())
            == vec!["E".to_owned(), "D".to_owned()];

    let mut probe_keys = mount.probe_keys();
    let mut live_keys = mount.keys();
    probe_keys.sort();
    live_keys.sort();
    let probe_storage_bounded = probe_keys == live_keys;

    let reordered_tree = host.runtime().backend().logical_tree();
    let current_static = static_node_ids(&host);
    let current_anchors = (
        mount.anchors().start().clone(),
        mount.anchors().end().clone(),
    );
    if current_static != initial_static || current_anchors != initial_anchors {
        static_ids_stable = false;
    }

    // Whole-list disposal releases items and anchors once.
    // 整表释放只释放一次条目与锚点。
    let dispose_before_ops = operation_counts(&host);
    mount.dispose();
    mount.dispose();
    let dispose_after_ops = operation_counts(&host);
    let disposed_tree = host.runtime().backend().logical_tree();
    let dispose_idempotent = dispose_before_ops != dispose_after_ops
        && disposed_tree.roots.first().is_none_or(|root| {
            !root.children.iter().any(|child| {
                child
                    .attributes
                    .get("aria-label")
                    .is_some_and(|label| label.starts_with("Row "))
            })
        });
    let second_dispose_before = operation_counts(&host);
    mount.dispose();
    let dispose_idempotent = dispose_idempotent && operation_counts(&host) == second_dispose_before;

    let builder_runs = mount.builder_runs();
    let reconcile_executions = reconcile_executions(&host);

    host.dispose();
    let final_tree = host.runtime().backend().logical_tree();
    let operations = operation_counts(&host);
    let final_stats = host.runtime_stats();

    Ok(KeyedConformanceReport {
        schema: 1,
        backend,
        initial_tree,
        reordered_tree,
        disposed_tree,
        final_tree,
        initial_text,
        phases,
        local_update,
        stale_write_is_noop,
        duplicate_key_rejected,
        duplicate_recovered,
        dispose_idempotent,
        static_ids_stable,
        item_identity_stable,
        builder_runs,
        reconcile_executions,
        probe_storage_bounded,
        operations,
        final_stats,
    })
}

fn text_of<B: HeadlessBackend + 'static>(
    host: &HeadlessHost<B>,
    root: &B::Node,
) -> Result<String, String> {
    host.runtime()
        .backend()
        .text_content(root)
        .ok_or_else(|| "keyed host lost its text".to_owned())
}

fn item_handles<B: DomBackend + 'static>(
    mount: &KeyedMount<B>,
    keys: &[String],
) -> BTreeMap<String, B::Node> {
    keys.iter()
        .filter_map(|key| mount.item_handle(key).map(|handle| (key.clone(), handle)))
        .collect()
}

fn item_texts<B: DomBackend + 'static>(
    mount: &KeyedMount<B>,
    keys: &[String],
) -> BTreeMap<String, B::Node> {
    keys.iter()
        .filter_map(|key| mount.item_text(key).map(|handle| (key.clone(), handle)))
        .collect()
}

fn reconcile_executions<B: HeadlessBackend + 'static>(host: &HeadlessHost<B>) -> u64 {
    host.reactive_snapshot()
        .effects
        .iter()
        .find(|effect| effect.name == "keyed/rows/reconcile")
        .map(|effect| effect.executions)
        .unwrap_or(0)
}

fn operation_counts<B: HeadlessBackend + 'static>(host: &HeadlessHost<B>) -> BTreeMap<String, u64> {
    let mut counts = BTreeMap::new();
    for operation in host.runtime().backend().operation_log() {
        *counts.entry(operation.operation).or_insert(0) += 1;
    }
    counts
}

fn delta(after: &BTreeMap<String, u64>, before: &BTreeMap<String, u64>) -> BTreeMap<String, u64> {
    after
        .iter()
        .filter_map(|(operation, count)| {
            let previous = before.get(operation).copied().unwrap_or(0);
            (count > &previous).then(|| (operation.clone(), count - previous))
        })
        .collect()
}

fn static_node_ids<B: HeadlessBackend + 'static>(host: &HeadlessHost<B>) -> Vec<String> {
    host.dom_snapshot()
        .nodes
        .into_iter()
        .filter(|node| {
            node.text.as_deref() == Some("Host")
                || node.text.as_deref() == Some("Tail")
                || node.text.as_deref() == Some("Next")
                || node.attributes.get("aria-label").map(String::as_str) == Some("Keyed host")
                || node.attributes.get("aria-label").map(String::as_str) == Some("Keyed next")
        })
        .map(|node| node.id)
        .collect()
}
