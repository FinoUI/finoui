//! Thin, UI-oriented adaptation of `reactive_graph` ownership and synchronous effects.
//! 对 `reactive_graph` 的所有权与同步 Effect 做面向 UI 的薄适配。

use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    rc::Rc,
};

use reactive_graph::{
    effect::ImmediateEffect,
    graph::untrack,
    owner::LocalStorage,
    signal::RwSignal,
    traits::{Get, GetUntracked, IsDisposed, Set, Update},
};
use schemars::JsonSchema;
use send_wrapper::SendWrapper;
use serde::{Deserialize, Serialize};

thread_local! {
    static CURRENT_EFFECT: Cell<Option<usize>> = const { Cell::new(None) };
}

/// A deterministic error reported by the reactive adapter.
/// 响应式适配层报告的确定性错误。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReactiveError {
    /// Stable machine-readable error code.
    /// 稳定的机器可读错误码。
    pub code: String,
    /// Human-readable explanation.
    /// 供人阅读的错误说明。
    pub message: String,
    /// Stable debug identifier of the related effect.
    /// 相关 Effect 的稳定调试标识。
    pub effect: Option<String>,
}

/// Serializable signal diagnostics without exposing `reactive_graph` handles.
/// 不暴露 `reactive_graph` 句柄的可序列化 Signal 诊断。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SignalSnapshot {
    /// Stable per-owner identifier.
    /// Owner 内稳定的标识。
    pub id: String,
    /// Development-time debug name.
    /// 开发态调试名。
    pub name: String,
    /// Number of tracked writes.
    /// 可追踪写入的次数。
    pub writes: u64,
}

/// Serializable effect diagnostics and execution counts.
/// 可序列化的 Effect 诊断与执行计数。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EffectSnapshot {
    /// Stable per-owner identifier.
    /// Owner 内稳定的标识。
    pub id: String,
    /// Development-time debug name.
    /// 开发态调试名。
    pub name: String,
    /// Actual callback executions, excluding rejected recursive entries.
    /// 实际回调执行次数，不含被拒绝的递归进入。
    pub executions: u64,
    /// Stable signal identifiers read by the latest execution.
    /// 最近一次执行读取的 Signal 稳定标识。
    pub dependencies: Vec<String>,
    /// Why the latest execution happened.
    /// 最近一次执行的原因。
    pub last_reason: String,
    /// Whether the effect is still owned and able to run.
    /// Effect 是否仍由 Owner 持有并可执行。
    pub active: bool,
}

/// Versioned reactive state used by headless inspection.
/// 无头检查使用的版本化响应式状态。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReactiveSnapshot {
    /// Snapshot schema version.
    /// 快照 schema 版本。
    pub schema: u32,
    /// Number of manual component mount executions.
    /// 手工组件挂载执行次数。
    pub component_executions: u64,
    /// Signals in stable creation order.
    /// 按稳定创建顺序排列的 Signal。
    pub signals: Vec<SignalSnapshot>,
    /// Effects in stable creation order.
    /// 按稳定创建顺序排列的 Effect。
    pub effects: Vec<EffectSnapshot>,
    /// Reactive errors in deterministic reporting order.
    /// 按确定性报告顺序排列的响应式错误。
    pub errors: Vec<ReactiveError>,
    /// Whether this owner has been disposed.
    /// Owner 是否已释放。
    pub disposed: bool,
}

#[derive(Debug)]
struct SignalRecord {
    name: String,
    writes: u64,
    last_write_seq: u64,
}

#[derive(Debug)]
struct EffectRecord {
    name: String,
    executions: u64,
    dependencies: BTreeSet<usize>,
    last_reason: String,
    active: bool,
    last_run_seq: u64,
}

#[derive(Debug, Default)]
pub(crate) struct ReactiveRegistry {
    next_signal: usize,
    next_effect: usize,
    signals: BTreeMap<usize, SignalRecord>,
    effects: BTreeMap<usize, EffectRecord>,
    errors: Vec<ReactiveError>,
    write_seq: u64,
}

impl ReactiveRegistry {
    fn register_signal(&mut self, name: &str) -> usize {
        self.next_signal += 1;
        let id = self.next_signal;
        self.signals.insert(
            id,
            SignalRecord {
                name: name.to_owned(),
                writes: 0,
                last_write_seq: 0,
            },
        );
        id
    }

    fn register_effect(&mut self, name: &str) -> usize {
        self.next_effect += 1;
        let id = self.next_effect;
        self.effects.insert(
            id,
            EffectRecord {
                name: name.to_owned(),
                executions: 0,
                dependencies: BTreeSet::new(),
                last_reason: "pending".to_owned(),
                active: true,
                last_run_seq: 0,
            },
        );
        id
    }

    /// Records a diagnostic dependency only for the effect currently being tracked.
    /// 仅为当前被追踪的 Effect 记录诊断依赖。
    fn record_dependency(&mut self, effect: usize, signal: usize) {
        if let Some(effect) = self.effects.get_mut(&effect) {
            effect.dependencies.insert(signal);
        }
    }

    /// Assigns a monotonic write sequence so effects can attribute their own causes.
    /// 分配单调写入序列，使 Effect 能归因到自身依赖的变化。
    fn signal_written(&mut self, signal: usize) {
        if let Some(record) = self.signals.get_mut(&signal) {
            self.write_seq += 1;
            record.writes += 1;
            record.last_write_seq = self.write_seq;
        }
    }

    /// Selects the newest post-run write from the effect's previous dependencies.
    /// 从 Effect 上一轮依赖中选择最近一次写入，避免归因到无关 Signal。
    fn begin_effect(&mut self, effect: usize) {
        let signals = &self.signals;
        let write_seq = self.write_seq;
        if let Some(record) = self.effects.get_mut(&effect) {
            record.executions += 1;
            record.last_reason = if record.executions == 1 {
                "initial".to_owned()
            } else {
                record
                    .dependencies
                    .iter()
                    .filter_map(|id| signals.get(id).map(|signal| (signal.last_write_seq, *id)))
                    .filter(|(sequence, _)| *sequence > record.last_run_seq)
                    .max()
                    .map(|(_, id)| format!("signal-{id} write"))
                    .unwrap_or_else(|| "reactive update".to_owned())
            };
            record.dependencies.clear();
            record.last_run_seq = write_seq;
        }
    }

    /// Reports recursive entry once per consecutive cycle without exposing graph handles.
    /// 对连续循环重入只报告一次，且不暴露响应式图句柄。
    fn report_cycle(&mut self, effect: usize) {
        let effect_id = format!("effect-{effect}");
        let error = ReactiveError {
            code: "UI_REACTIVE_CYCLE".to_owned(),
            message: format!("reactive cycle detected while running {effect_id}"),
            effect: Some(effect_id),
        };
        if self.errors.last() != Some(&error) {
            self.errors.push(error);
        }
    }

    /// Marks owner cleanup in diagnostics without deleting historical execution evidence.
    /// 在诊断中标记 Owner 清理，同时保留历史执行证据。
    fn deactivate_effect(&mut self, effect: usize) {
        if let Some(effect) = self.effects.get_mut(&effect) {
            effect.active = false;
        }
    }

    /// Projects internal records into stable IDs and creation-ordered diagnostics.
    /// 将内部记录映射为稳定 ID 与按创建顺序排列的诊断快照。
    fn snapshot(&self, component_executions: u64, disposed: bool) -> ReactiveSnapshot {
        ReactiveSnapshot {
            schema: 1,
            component_executions,
            signals: self
                .signals
                .iter()
                .map(|(id, signal)| SignalSnapshot {
                    id: format!("signal-{id}"),
                    name: signal.name.clone(),
                    writes: signal.writes,
                })
                .collect(),
            effects: self
                .effects
                .iter()
                .map(|(id, effect)| EffectSnapshot {
                    id: format!("effect-{id}"),
                    name: effect.name.clone(),
                    executions: effect.executions,
                    dependencies: effect
                        .dependencies
                        .iter()
                        .map(|dependency| format!("signal-{dependency}"))
                        .collect(),
                    last_reason: effect.last_reason.clone(),
                    active: effect.active,
                })
                .collect(),
            errors: self.errors.clone(),
            disposed,
        }
    }
}

/// An owner for signals, effects, and registered cleanup work.
/// 管理 Signal、Effect 与注册清理工作的 Owner。
///
/// ```
/// use fino::reactive::{Owner, batch};
///
/// let owner = Owner::new();
/// let count = owner.signal(0);
/// let observed = count.clone();
/// owner.effect(move || {
///     observed.get();
/// });
/// batch(|| {
///     for value in 1..=100 {
///         count.set(value);
///     }
/// });
/// assert_eq!(owner.snapshot().effects[0].executions, 2);
/// owner.dispose();
/// ```
#[derive(Clone, Debug)]
pub struct Owner {
    inner: reactive_graph::owner::Owner,
    registry: Rc<RefCell<ReactiveRegistry>>,
    disposed: Rc<Cell<bool>>,
}

impl Default for Owner {
    fn default() -> Self {
        Self::new()
    }
}

impl Owner {
    /// Creates an independent reactive owner.
    /// 创建独立的响应式 Owner。
    pub fn new() -> Self {
        Self::with_registry(Rc::new(RefCell::new(ReactiveRegistry::default())))
    }

    pub(crate) fn with_registry(registry: Rc<RefCell<ReactiveRegistry>>) -> Self {
        Self {
            inner: reactive_graph::owner::Owner::new(),
            registry,
            disposed: Rc::new(Cell::new(false)),
        }
    }

    /// Creates an owner nested under this one, sharing the same diagnostics registry.
    /// 创建嵌套在该 Owner 下并共享同一诊断注册表的 Owner。
    pub fn child(&self) -> Self {
        Self {
            inner: self.inner.child(),
            registry: Rc::clone(&self.registry),
            disposed: Rc::new(Cell::new(false)),
        }
    }

    /// Creates a signal owned by this owner.
    /// 创建由该 Owner 管理的 Signal。
    pub fn signal<T: 'static>(&self, value: T) -> Signal<T> {
        self.signal_named("signal", value)
    }

    /// Creates a signal with a development-time debug name.
    /// 创建带开发态调试名的 Signal。
    pub fn signal_named<T: 'static>(&self, name: &str, value: T) -> Signal<T> {
        let id = self.registry.borrow_mut().register_signal(name);
        let inner = self
            .inner
            .with(|| RwSignal::<T, LocalStorage>::new_local(value));
        Signal {
            inner,
            id,
            registry: Rc::clone(&self.registry),
        }
    }

    /// Creates a synchronous effect that runs once immediately.
    /// 创建立即同步首跑的 Effect。
    pub fn effect(&self, callback: impl FnMut() + 'static) {
        self.effect_named("effect", callback);
    }

    /// Creates a named synchronous effect owned by this owner.
    /// 创建由该 Owner 管理的命名同步 Effect。
    pub fn effect_named(&self, name: &str, callback: impl FnMut() + 'static) {
        assert!(
            !self.is_disposed(),
            "cannot create an effect on a disposed owner"
        );
        let id = self.registry.borrow_mut().register_effect(name);
        let runner = SendWrapper::new(RefCell::new(EffectRunner {
            callback: Box::new(callback),
            registry: Rc::clone(&self.registry),
        }));
        let cycle_registry = SendWrapper::new(Rc::clone(&self.registry));
        let panic_registry = SendWrapper::new(Rc::clone(&self.registry));

        // `SendWrapper` confines non-Send UI state to the creating thread, while
        // `ImmediateEffect` retains upstream graph tracking and batch de-duplication.
        // `SendWrapper` 将非 Send 的 UI 状态限制在创建线程，`ImmediateEffect`
        // 则保留上游的依赖图追踪与批内去重。
        let first_run = SendWrapper::new(Cell::new(true));
        let effect = self.inner.with(|| {
            ImmediateEffect::new(move || {
                let was_first_run = first_run.get();
                let outcome = catch_unwind(AssertUnwindSafe(|| {
                    if let Ok(mut runner) = runner.try_borrow_mut() {
                        runner.run(id);
                    } else {
                        cycle_registry.borrow_mut().report_cycle(id);
                    }
                }));
                match outcome {
                    Ok(()) => first_run.set(false),
                    Err(payload) => {
                        // Only a first-run panic precedes cleanup registration.
                        // Later panics leave the effect owned and re-runnable, so
                        // diagnostics must stay active and a retry can succeed.
                        // 只有首次执行 panic 发生在清理注册之前。后续 panic 时
                        // Effect 仍由 Owner 持有并可再次运行，因此诊断必须保持
                        // 活跃，后续重试可以成功。
                        if was_first_run {
                            panic_registry.borrow_mut().deactivate_effect(id);
                        }
                        resume_unwind(payload);
                    }
                }
            })
        });
        if self.is_disposed() {
            // The owner may be disposed during the first effect run, before its
            // cleanup registration exists. Drop the effect and mark diagnostics
            // inactive instead of leaving a stuck active record.
            // Owner 可能在首次执行期间、清理注册之前被释放。此时直接丢弃 Effect
            // 并把诊断标记为非活跃，避免留下卡住的 active 记录。
            self.registry.borrow_mut().deactivate_effect(id);
            drop(effect);
            return;
        }
        let cleanup_registry = SendWrapper::new(Rc::clone(&self.registry));
        self.inner.with(|| {
            reactive_graph::owner::on_cleanup(move || {
                drop(effect);
                cleanup_registry.borrow_mut().deactivate_effect(id);
            });
        });
    }

    /// Disposes effects and cleanup callbacks; repeated calls are safe.
    /// 释放 Effect 与清理回调，可安全重复调用。
    ///
    /// ```
    /// use fino::reactive::Owner;
    ///
    /// let owner = Owner::new();
    /// let count = owner.signal(0);
    /// owner.dispose();
    /// owner.dispose();
    /// count.set(1);
    /// assert!(owner.is_disposed());
    /// ```
    pub fn dispose(&self) {
        if !self.disposed.replace(true) {
            self.inner.cleanup();
        }
    }

    /// Returns whether disposal has already occurred.
    /// 返回是否已完成释放。
    pub fn is_disposed(&self) -> bool {
        self.disposed.get()
    }

    /// Captures versioned reactive diagnostics.
    /// 捕获版本化响应式诊断。
    pub fn snapshot(&self) -> ReactiveSnapshot {
        self.snapshot_with_component_executions(0)
    }

    pub(crate) fn snapshot_with_component_executions(
        &self,
        component_executions: u64,
    ) -> ReactiveSnapshot {
        self.registry
            .borrow()
            .snapshot(component_executions, self.is_disposed())
    }

    /// Keeps one value alive until this owner is disposed.
    /// 让一个值存活到该 Owner 被释放。
    ///
    /// The value is dropped on the owner's thread during normal cleanup, so
    /// non-`Send` UI handles can follow the owner without becoming global state.
    /// 值会在 Owner 线程的正常清理期间释放，因此非 `Send` UI 句柄可以跟随
    /// Owner 生命周期，而不会变成全局状态。
    pub fn keep_alive<T: 'static>(&self, value: T) {
        let value = SendWrapper::new(RefCell::new(Some(value)));
        self.register_cleanup(move || {
            let _ = value.take();
        });
    }

    /// Runs one callback when this owner is disposed; repeated disposal is safe.
    /// 在该 Owner 释放时执行一个回调；重复释放是安全的。
    ///
    /// The callback follows the owner thread and can carry non-`Send` UI state.
    /// 回调跟随 Owner 线程，可以携带非 `Send` 的 UI 状态。
    pub fn on_dispose(&self, cleanup: impl FnOnce() + 'static) {
        let cleanup = SendWrapper::new(RefCell::new(Some(Box::new(cleanup) as Box<dyn FnOnce()>)));
        self.register_cleanup(move || {
            if let Some(cleanup) = cleanup.borrow_mut().take() {
                cleanup();
            }
        });
    }

    pub(crate) fn register_cleanup(&self, cleanup: impl FnOnce() + Send + Sync + 'static) {
        self.inner
            .with(|| reactive_graph::owner::on_cleanup(cleanup));
    }
}

struct EffectRunner {
    callback: Box<dyn FnMut()>,
    registry: Rc<RefCell<ReactiveRegistry>>,
}

impl EffectRunner {
    /// Installs the diagnostic observer for one callback and restores the outer observer on unwind.
    /// 为单次回调安装诊断观察者，并在展开时恢复外层观察者。
    fn run(&mut self, id: usize) {
        struct ResetEffect(Option<usize>);
        impl Drop for ResetEffect {
            fn drop(&mut self) {
                CURRENT_EFFECT.with(|current| current.set(self.0));
            }
        }

        self.registry.borrow_mut().begin_effect(id);
        let previous = CURRENT_EFFECT.with(|current| current.replace(Some(id)));
        let _reset = ResetEffect(previous);
        (self.callback)();
    }
}

/// A read-write signal handle whose platform-independent API hides the graph implementation.
/// 隐藏图实现且不包含平台类型的可读写 Signal 句柄。
#[derive(Debug)]
pub struct Signal<T: 'static> {
    inner: RwSignal<T, LocalStorage>,
    id: usize,
    registry: Rc<RefCell<ReactiveRegistry>>,
}

impl<T: 'static> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner,
            id: self.id,
            registry: Rc::clone(&self.registry),
        }
    }
}

impl<T: Clone + 'static> Signal<T> {
    /// Reads and subscribes the current effect to this signal.
    /// 读取值，并让当前 Effect 订阅该 Signal。
    pub fn get(&self) -> T {
        if let Some(effect) = CURRENT_EFFECT.with(Cell::get) {
            self.registry
                .borrow_mut()
                .record_dependency(effect, self.id);
        }
        self.inner.get()
    }

    /// Reads without creating a reactive dependency.
    /// 读取值但不建立响应式依赖。
    pub fn get_untracked(&self) -> T {
        self.inner.get_untracked()
    }
}

impl<T: 'static> Signal<T> {
    /// Mutates the value and notifies dependents, or does nothing after disposal.
    /// 修改值并通知依赖者；Owner 释放后不执行任何操作。
    pub fn update(&self, update: impl FnOnce(&mut T)) {
        if self.inner.is_disposed() {
            return;
        }
        self.registry.borrow_mut().signal_written(self.id);
        self.inner.update(update);
    }

    /// Replaces the value and notifies dependents, or does nothing after disposal.
    /// 替换值并通知依赖者；Owner 释放后不执行任何操作。
    pub fn set(&self, value: T) {
        if self.inner.is_disposed() {
            return;
        }
        self.registry.borrow_mut().signal_written(self.id);
        self.inner.set(value);
    }
}

/// Runs a transaction using `reactive_graph`'s nested, de-duplicating effect batch.
/// 使用 `reactive_graph` 支持嵌套与 Effect 去重的事务执行批处理。
///
/// ```
/// use fino::reactive::{Owner, batch};
///
/// let owner = Owner::new();
/// let count = owner.signal(0);
/// let observed = count.clone();
/// owner.effect(move || {
///     observed.get();
/// });
/// batch(|| {
///     count.set(1);
///     count.set(2);
/// });
/// assert_eq!(owner.snapshot().effects[0].executions, 2);
/// owner.dispose();
/// ```
pub fn batch<T>(transaction: impl FnOnce() -> T) -> T {
    reactive_graph::effect::batch(transaction)
}

/// Disables graph and diagnostic tracking, restoring both even when the read panics.
/// 禁用图与诊断依赖追踪，并在正常返回或 panic 时都恢复原状态。
pub fn untracked<T>(read: impl FnOnce() -> T) -> T {
    struct RestoreEffect(Option<usize>);
    impl Drop for RestoreEffect {
        fn drop(&mut self) {
            CURRENT_EFFECT.with(|current| current.set(self.0));
        }
    }

    let _restore = RestoreEffect(CURRENT_EFFECT.with(|current| current.replace(None)));
    untrack(read)
}
