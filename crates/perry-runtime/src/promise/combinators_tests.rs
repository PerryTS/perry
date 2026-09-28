//! Unit tests for `combinators.rs` (split out for the file-size gate).
use super::*;
use crate::array::{js_array_alloc, js_array_get_f64, js_array_set_f64};
use crate::closure::{js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64};
use crate::object::{js_object_alloc, js_object_get_field, js_object_set_field_by_name};
use crate::value::js_nanbox_pointer;

fn reset_promise_test_state() {
    TASK_QUEUE.with(|q| q.borrow_mut().clear());
    PROMISE_ALL_STATES.with(|s| s.borrow_mut().clear());
}

fn thenable_value(func: *const crate::closure::JsFunctionInfo, captured: f64) -> f64 {
    let obj = js_object_alloc(0, 0);
    let then = js_closure_alloc(func, 1);
    js_closure_set_capture_f64(then, 0, captured);
    let key = crate::string::js_string_from_bytes(b"then".as_ptr(), 4);
    js_object_set_field_by_name(obj, key, js_nanbox_pointer(then as i64));
    js_nanbox_pointer(obj as i64)
}

#[test]
fn promise_probe_rejects_pointer_tagged_native_handles() {
    let fetch_family_handle = js_nanbox_pointer(0x40001);
    assert_eq!(js_value_is_promise(fetch_family_handle), 0);
}

// #5226: small typed arrays / buffers are system-`alloc`'d off the GC heap
// with NO 8-byte GcHeader prefix and are tracked only in side tables. The
// type-dispatch probes (`js_value_is_promise`, `is_date_cell_addr`, …) must
// recognize them via the side table and must NOT back-read
// `ptr - GC_HEADER_SIZE` — a read on a block at the start of a freshly
// mapped region crosses into the (unmapped) preceding page and segfaults.
// Reproduce that worst case with a guarded mapping: without the side-table
// skip these probes SIGSEGV; with it, they classify the block correctly.
#[cfg(unix)]
#[test]
fn type_probes_skip_offheap_typed_array_with_unmapped_preceding_page() {
    unsafe {
        let page = libc::sysconf(libc::_SC_PAGESIZE) as usize;
        let total = page * 2;
        let base = libc::mmap(
            std::ptr::null_mut(),
            total,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        );
        assert_ne!(base, libc::MAP_FAILED);
        // Guard the first page so any `ptr - GC_HEADER_SIZE` back-read faults.
        assert_eq!(libc::mprotect(base, page, libc::PROT_NONE), 0);
        let ta = (base as *mut u8).add(page) as *const crate::typedarray::TypedArrayHeader;
        crate::typedarray::register_typed_array(ta, 0);

        let value = js_nanbox_pointer(ta as i64);
        assert_eq!(js_value_is_promise(value), 0);
        assert!(!crate::date::is_date_cell_addr(ta as usize));

        crate::typedarray::unregister_typed_array(ta);
        assert_eq!(libc::munmap(base, total), 0);
    }
}

extern "C" fn test_thenable_resolve_twice(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    on_fulfilled: f64,
    _on_rejected: f64,
) -> f64 {
    let value = js_closure_get_capture_f64(closure, 0);
    let unexpected = value + 1000.0;
    unsafe {
        crate::closure::js_native_call_value(
            on_fulfilled,
            crate::closure::plain_call_receiver(),
            [value].as_ptr(),
            1,
        );
        crate::closure::js_native_call_value(
            on_fulfilled,
            crate::closure::plain_call_receiver(),
            [unexpected].as_ptr(),
            1,
        );
    }
    0.0
}

extern "C" fn test_thenable_reject(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    _on_fulfilled: f64,
    on_rejected: f64,
) -> f64 {
    let reason = js_closure_get_capture_f64(closure, 0);
    unsafe {
        crate::closure::js_native_call_value(
            on_rejected,
            crate::closure::plain_call_receiver(),
            [reason].as_ptr(),
            1,
        );
    }
    0.0
}

#[test]
fn promise_all_assimilates_thenable_and_guards_double_resolve() {
    unsafe {
        reset_promise_test_state();
        let arr = js_array_alloc(1);
        (*arr).length = 1;
        js_array_set_f64(
            arr,
            0,
            thenable_value(crate::fn_info!(test_thenable_resolve_twice, 2), 7.0),
        );

        let all = js_promise_all(arr);
        assert_eq!((*all).state, PromiseState::Pending);

        crate::promise::js_promise_run_microtasks();

        assert_eq!((*all).state, PromiseState::Fulfilled);
        let results =
            crate::value::js_nanbox_get_pointer((*all).value) as *const crate::array::ArrayHeader;
        assert_eq!(js_array_get_f64(results, 0), 7.0);
    }
}

#[test]
fn promise_all_rejects_from_thenable_job() {
    unsafe {
        reset_promise_test_state();
        let arr = js_array_alloc(1);
        (*arr).length = 1;
        js_array_set_f64(
            arr,
            0,
            thenable_value(crate::fn_info!(test_thenable_reject, 2), 13.0),
        );

        let all = js_promise_all(arr);
        assert_eq!((*all).state, PromiseState::Pending);

        crate::promise::js_promise_run_microtasks();

        assert_eq!((*all).state, PromiseState::Rejected);
        assert_eq!((*all).reason, 13.0);
    }
}

#[test]
fn promise_all_settled_assimilates_thenables_in_input_order() {
    unsafe {
        reset_promise_test_state();
        let arr = js_array_alloc(2);
        (*arr).length = 2;
        js_array_set_f64(
            arr,
            0,
            thenable_value(crate::fn_info!(test_thenable_reject, 2), 1.0),
        );
        js_array_set_f64(
            arr,
            1,
            thenable_value(crate::fn_info!(test_thenable_resolve_twice, 2), 2.0),
        );

        let settled = js_promise_all_settled(arr);
        assert_eq!((*settled).state, PromiseState::Pending);

        crate::promise::js_promise_run_microtasks();

        assert_eq!((*settled).state, PromiseState::Fulfilled);
        let results = crate::value::js_nanbox_get_pointer((*settled).value)
            as *const crate::array::ArrayHeader;
        let first = crate::value::js_nanbox_get_pointer(js_array_get_f64(results, 0))
            as *const crate::object::ObjectHeader;
        let second = crate::value::js_nanbox_get_pointer(js_array_get_f64(results, 1))
            as *const crate::object::ObjectHeader;
        assert_eq!(js_object_get_field(first, 1).bits(), 1.0f64.to_bits());
        assert_eq!(js_object_get_field(second, 1).bits(), 2.0f64.to_bits());
    }
}

#[test]
fn promise_result_resolution_assimilates_thenable_objects() {
    unsafe {
        reset_promise_test_state();
        let promise = js_promise_new();
        promise_resolve_assimilating(
            promise,
            thenable_value(crate::fn_info!(test_thenable_resolve_twice, 2), 21.0),
        );
        assert_eq!((*promise).state, PromiseState::Pending);

        crate::promise::js_promise_run_microtasks();

        assert_eq!((*promise).state, PromiseState::Fulfilled);
        assert_eq!((*promise).value, 21.0);
    }
}

/// Regression: a pending promise reused as input to two `Promise.all`
/// calls must settle BOTH all-promises when it resolves. Pre-fix,
/// `promise_all_take_handler` only popped the first matching state
/// (via `swap_remove`), so the second `Promise.all` hung forever.
#[test]
fn promise_all_with_shared_pending_input_resolves_both() {
    unsafe {
        // Pending promise that will be shared across two Promise.all calls.
        let shared = js_promise_new();

        // Second input for each all() — a pre-resolved promise so the
        // remaining counter only needs `shared` to settle.
        let other_a = js_promise_new();
        js_promise_resolve(other_a, 100.0);
        let other_b = js_promise_new();
        js_promise_resolve(other_b, 200.0);

        // Build [shared, other_a]
        let arr_a = js_array_alloc(2);
        (*arr_a).length = 2;
        js_array_set_f64(arr_a, 0, js_nanbox_pointer(shared as i64));
        js_array_set_f64(arr_a, 1, js_nanbox_pointer(other_a as i64));
        let all_a = js_promise_all(arr_a);

        // Build [shared, other_b]
        let arr_b = js_array_alloc(2);
        (*arr_b).length = 2;
        js_array_set_f64(arr_b, 0, js_nanbox_pointer(shared as i64));
        js_array_set_f64(arr_b, 1, js_nanbox_pointer(other_b as i64));
        let all_b = js_promise_all(arr_b);

        // Both all() results should still be pending.
        assert_eq!((*all_a).state, PromiseState::Pending);
        assert_eq!((*all_b).state, PromiseState::Pending);

        // PROMISE_ALL_STATES must hold TWO entries keyed on `shared`.
        let registered = PROMISE_ALL_STATES.with(|s| s.borrow_mut().count_for_key(shared as usize));
        assert_eq!(
            registered, 2,
            "expected two Promise.all states keyed on the shared pending promise"
        );

        // Settle the shared promise; drain microtasks so PromiseAll tasks
        // run and update both result arrays.
        js_promise_resolve(shared, 42.0);
        crate::promise::js_promise_run_microtasks();

        // Both Promise.all results must now be Fulfilled.
        assert_eq!(
            (*all_a).state,
            PromiseState::Fulfilled,
            "first Promise.all should have settled"
        );
        assert_eq!(
            (*all_b).state,
            PromiseState::Fulfilled,
            "second Promise.all should have settled (was hanging pre-fix)"
        );
    }
}
