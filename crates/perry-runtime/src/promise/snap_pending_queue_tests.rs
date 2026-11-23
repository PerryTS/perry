//! SNAPK4: the census names each queued nextTick job (`snap_pending_ticks`)
//! and each scheduled resolution (`snap_scheduled_resolves`) by the init that
//! queued it and its callback's code address, read from the queues themselves.
use super::*;

extern "C" fn tick_a(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    _v: f64,
) -> f64 {
    1.0
}

extern "C" fn tick_b(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    _v: f64,
) -> f64 {
    2.0
}

#[test]
fn pending_ticks_name_owner_and_callback() {
    let info_a = crate::fn_info!(tick_a, 1);
    let info_b = crate::fn_info!(tick_b, 1);
    let code_a = unsafe { (*info_a).code as usize };
    let code_b = unsafe { (*info_b).code as usize };
    assert_ne!(code_a, code_b);
    let cb_a = crate::closure::js_closure_alloc(info_a, 0) as i64;
    let cb_b = crate::closure::js_closure_alloc(info_b, 0) as i64;
    crate::builtins::test_clear_queued_jobs();
    assert_eq!(
        crate::builtins::snap_pending_ticks(),
        Some(Vec::new()),
        "none pending, no entry"
    );
    crate::builtins::test_push_tick(cb_a, 7);
    crate::builtins::test_push_tick(cb_b, 9);
    let ticks = crate::builtins::snap_pending_ticks().expect("queue not borrowed");
    crate::builtins::test_clear_queued_jobs();
    assert_eq!(ticks, vec![(7, code_a), (9, code_b)]);
}

#[test]
fn scheduled_resolves_name_owner() {
    combinators::SCHEDULED_RESOLVES.with(|q| q.borrow_mut().clear());
    assert_eq!(
        snap_scheduled_resolves(),
        Some(Vec::new()),
        "none pending, no entry"
    );
    let promise = js_promise_new();
    combinators::SCHEDULED_RESOLVES.with(|q| {
        let mut q = q.borrow_mut();
        q.push((promise, 1.0, 9));
        q.push((promise, 2.0, 12));
    });
    let resolves = snap_scheduled_resolves().expect("queue not borrowed");
    combinators::SCHEDULED_RESOLVES.with(|q| q.borrow_mut().clear());
    assert_eq!(resolves, vec![(9, 0), (12, 0)]);
}
