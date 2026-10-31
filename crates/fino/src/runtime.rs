//! Manual component runtime, backend access, and owner-scoped event cleanup.
//! 手工组件 Runtime、后端访问与 Owner 作用域内的事件清理。

use std::{
    cell::{Cell, Ref, RefCell, RefMut},
    rc::Rc,
};

use send_wrapper::SendWrapper;

use crate::{
    dom_api::{DomBackend, EventContext, EventKind, EventListener},
    reactive::{Owner, ReactiveRegistry},
};

/// Observable component execution count shared with reactive inspection.
/// 与响应式检查共享的可观测组件执行计数。
#[derive(Clone, Debug, Default)]
pub struct ExecutionCounter(Rc<Cell<u64>>);

impl ExecutionCounter {
    /// Records one manual component execution.
    /// 记录一次手工组件执行。
    pub fn increment(&self) {
        self.0.set(self.0.get() + 1);
    }

    /// Returns the current execution count.
    /// 返回当前执行计数。
    pub fn get(&self) -> u64 {
        self.0.get()
    }
}

/// Backend access supplied to an effect for each synchronous execution.
/// 每次同步执行时提供给 Effect 的后端访问上下文。
pub struct EffectContext<B: DomBackend> {
    backend: Rc<RefCell<B>>,
}

impl<B: DomBackend> EffectContext<B> {
    /// Mutably borrows the backend only for the duration of the operation.
    /// 仅在操作期间可变借用后端。
    pub fn backend_mut(&mut self) -> RefMut<'_, B> {
        self.backend.borrow_mut()
    }
}

/// Owns shared backend state while effects retain no long-lived mutable references.
/// 持有共享后端状态，Effect 不长期保存可变引用。
pub struct Runtime<B: DomBackend> {
    backend: Rc<RefCell<B>>,
    reactive_registry: Rc<RefCell<ReactiveRegistry>>,
    component_executions: ExecutionCounter,
}

impl<B: DomBackend> Clone for Runtime<B> {
    /// Shares the same backend, registry, and execution counter with a scoped clone.
    /// 克隆出共享同一后端、注册表与执行计数的受限句柄。
    fn clone(&self) -> Self {
        Self {
            backend: Rc::clone(&self.backend),
            reactive_registry: Rc::clone(&self.reactive_registry),
            component_executions: self.component_executions.clone(),
        }
    }
}

impl<B: DomBackend + 'static> Runtime<B> {
    /// Creates a runtime around a backend.
    /// 围绕后端创建 Runtime。
    pub fn new(backend: B) -> Self {
        Self {
            backend: Rc::new(RefCell::new(backend)),
            reactive_registry: Rc::new(RefCell::new(ReactiveRegistry::default())),
            component_executions: ExecutionCounter::default(),
        }
    }

    /// Creates an owner attached to this runtime's diagnostics.
    /// 创建接入该 Runtime 诊断的 Owner。
    pub fn create_owner(&self) -> Owner {
        Owner::with_registry(Rc::clone(&self.reactive_registry))
    }

    /// Borrows the backend for read-only inspection.
    /// 只读借用后端进行检查。
    pub fn backend(&self) -> Ref<'_, B> {
        self.backend.borrow()
    }

    /// Borrows the backend for a manual construction operation.
    /// 可变借用后端执行手工构建操作。
    pub fn backend_mut(&self) -> RefMut<'_, B> {
        self.backend.borrow_mut()
    }

    /// Returns the observable component execution counter.
    /// 返回可观测的组件执行计数。
    pub fn component_executions(&self) -> ExecutionCounter {
        self.component_executions.clone()
    }

    /// Creates an owner-scoped effect whose callback receives backend context.
    /// 创建 Owner 作用域内的 Effect，回调接收后端上下文。
    pub fn effect(
        &self,
        owner: &Owner,
        name: &str,
        mut callback: impl FnMut(&mut EffectContext<B>) + 'static,
    ) {
        self.effect_with_runtime(owner, name, move |context, _runtime| callback(context));
    }

    /// Creates an owner-scoped effect that can also use a scoped runtime clone.
    /// 创建 Owner 作用域内的 Effect，回调还可使用受限的 Runtime 克隆。
    ///
    /// The clone shares the same backend and diagnostics, so it cannot outlive
    /// the host state and never introduces a second DOM.
    /// 克隆共享同一后端与诊断，不会超出 Host 状态存活，也不会引入第二份 DOM。
    pub fn effect_with_runtime(
        &self,
        owner: &Owner,
        name: &str,
        mut callback: impl FnMut(&mut EffectContext<B>, &mut Runtime<B>) + 'static,
    ) {
        let mut context = EffectContext {
            backend: Rc::clone(&self.backend),
        };
        let mut runtime = self.clone();
        owner.effect_named(name, move || callback(&mut context, &mut runtime));
    }

    /// Registers an event and releases its backend handle during owner disposal.
    /// 注册事件，并在 Owner 释放时释放其后端句柄。
    pub fn listen(
        &self,
        owner: &Owner,
        node: &B::Node,
        event: EventKind,
        callback: impl FnMut(crate::dom_api::Event, &mut EventContext) + 'static,
    ) {
        assert!(
            !owner.is_disposed(),
            "cannot register a listener on a disposed owner"
        );
        let listener = self
            .backend
            .borrow_mut()
            .listen(node, event, EventListener::new(callback));
        let cleanup = SendWrapper::new(RefCell::new(Some((Rc::clone(&self.backend), listener))));
        owner.register_cleanup(move || {
            if let Some((backend, listener)) = cleanup.borrow_mut().take() {
                backend.borrow_mut().unlisten(listener);
            }
        });
    }
}
