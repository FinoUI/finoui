//! Reactive effect panic diagnostics and retry regressions.
//! 响应式 Effect panics 的诊断与重试回归。

use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

use fino::reactive::Owner;

/// A first-run panic happens before cleanup registration and marks the effect inactive.
/// 首次执行 panic 发生在清理注册之前，并把 Effect 标记为非活跃。
#[test]
fn first_effect_run_panic_marks_effect_inactive() {
    let owner = Owner::new();
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        owner.effect_named("panic/first", || panic!("first effect run"));
    }));
    assert!(outcome.is_err(), "the first-run panic must propagate");

    let snapshot = owner.snapshot();
    assert_eq!(snapshot.effects.len(), 1);
    assert_eq!(snapshot.effects[0].name, "panic/first");
    assert_eq!(snapshot.effects[0].executions, 1);
    assert!(
        !snapshot.effects[0].active,
        "a first-run panic has no cleanup registration, so diagnostics must be inactive"
    );
}

/// A later panic leaves the owned effect active and subscriptions can run it again.
/// 后续 panic 保留已安装 Effect 的活跃状态，订阅仍能再次运行它。
#[test]
fn later_effect_panic_stays_active_and_retries() {
    let owner = Owner::new();
    let value = owner.signal_named("panic/value", 0_i32);
    let read = value.clone();
    let fail_once = Rc::new(Cell::new(true));
    owner.effect_named("panic/later", move || {
        let current = read.get();
        if current == 1 && fail_once.replace(false) {
            panic!("later effect run");
        }
    });
    assert!(owner.snapshot().effects[0].active);

    let outcome = catch_unwind(AssertUnwindSafe(|| value.set(1)));
    assert!(outcome.is_err(), "the later panic must propagate");
    let snapshot = owner.snapshot();
    assert_eq!(snapshot.effects[0].executions, 2);
    assert!(
        snapshot.effects[0].active,
        "an effect with registered cleanup stays active after a later panic"
    );

    value.set(2);
    let snapshot = owner.snapshot();
    assert_eq!(
        snapshot.effects[0].executions, 3,
        "the still-subscribed effect runs after the failed attempt"
    );
    assert!(snapshot.effects[0].active);
}
