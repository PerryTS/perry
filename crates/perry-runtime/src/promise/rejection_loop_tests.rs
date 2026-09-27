//! #11449: caught rejected awaits can legitimately resume only on error edges.
use super::*;

// Models `for (...) { try { await Promise.reject(remaining) } catch {} }`.
// Every resumption advances the loop even though no fulfilled await occurs.
extern "C" fn catching_step(step: ClosurePtr, remaining: f64, is_error: f64) -> f64 {
    if is_error.to_bits() != crate::value::TAG_TRUE {
        return crate::value::js_nanbox_pointer(js_async_step_done(-1.0, step) as i64);
    }
    if remaining == 0.0 {
        return crate::value::js_nanbox_pointer(js_async_step_done(remaining, step) as i64);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let step_handle = scope.root_raw_const_ptr(step);
    let rejected = js_promise_new();
    js_promise_reject(rejected, remaining - 1.0);
    let next = step_handle.with_const_ptr(|step| {
        js_async_step_chain(crate::value::js_nanbox_pointer(rejected as i64), step)
    });
    crate::value::js_nanbox_pointer(next as i64)
}

#[test]
fn caught_rejected_awaits_have_no_iteration_limit() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let step = crate::closure::js_closure_alloc(catching_step as *const u8, 0);
    let step_handle = scope.root_raw_const_ptr(step);
    let rejected = js_promise_new();
    js_promise_reject(rejected, 12_050.0);
    let result = step_handle.with_const_ptr(|step| {
        js_async_step_chain(crate::value::js_nanbox_pointer(rejected as i64), step)
    });
    let result_handle = scope.root_raw_mut_ptr(result);
    // Bound the test's drain, rather than imposing a language-level limit.
    for _ in 0..20_000 {
        if result_handle
            .with_mut_ptr::<Promise, _>(|p| unsafe { (*p).state != PromiseState::Pending })
        {
            break;
        }
        js_promise_run_microtasks();
    }
    result_handle.with_mut_ptr::<Promise, _>(|p| unsafe {
        assert_eq!((*p).state, PromiseState::Fulfilled);
        assert_eq!(
            (*p).value,
            0.0,
            "every caught rejection must advance the loop"
        );
    });
}
