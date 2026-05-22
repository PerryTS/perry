//! Promise combinators — `Promise.all`, `Promise.race`, `Promise.any`,
//! `Promise.allSettled` — plus the executor entry, thenable
//! assimilation, the scheduled-resolve queue, and `is_promise` probes.

use super::*;

#[derive(Clone, Copy)]
pub(super) struct PromiseAllState {
    pub result_promise: *mut Promise,
    pub results_arr: *mut crate::array::ArrayHeader,
    pub state_arr: *mut crate::array::ArrayHeader,
    pub index: u32,
}

thread_local! {
    pub(super) static PROMISE_ALL_STATES: RefCell<Vec<(usize, PromiseAllState)>> =
        const { RefCell::new(Vec::new()) };
}

/// Drain ALL `PromiseAllState` entries associated with `promise`.
///
/// A pending promise can be reused as an input to multiple
/// `Promise.all([...])` calls — e.g.:
///
/// ```js
/// const p = somePending();
/// Promise.all([p, x]);
/// Promise.all([p, y]);
/// ```
///
/// Each `Promise.all` call registers its own `PromiseAllState` keyed by
/// `p as usize`. When `p` settles we must complete every registered
/// state, not just the first one.
#[inline]
pub(super) fn promise_all_take_all_handlers(promise: *mut Promise) -> Vec<PromiseAllState> {
    if promise.is_null() {
        return Vec::new();
    }
    PROMISE_ALL_STATES.with(|states| {
        let mut states = states.borrow_mut();
        let key = promise as usize;
        let mut drained = Vec::new();
        let mut i = 0;
        while i < states.len() {
            if states[i].0 == key {
                drained.push(states.swap_remove(i).1);
            } else {
                i += 1;
            }
        }
        drained
    })
}

#[inline]
pub(super) fn promise_all_settle(state: PromiseAllState, value: f64, is_fulfilled: bool) {
    if is_fulfilled {
        promise_all_fulfill_direct(state, value);
    } else {
        promise_all_reject_direct(state.result_promise, state.state_arr, value);
    }
}

pub(super) fn scan_promise_all_states_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    PROMISE_ALL_STATES.with(|states| {
        let mut states = states.borrow_mut();
        for (key, state) in states.iter_mut() {
            visitor.visit_metadata_usize_slot(key);
            visitor.visit_raw_mut_ptr_slot(&mut state.result_promise);
            visitor.visit_raw_mut_ptr_slot(&mut state.results_arr);
            visitor.visit_raw_mut_ptr_slot(&mut state.state_arr);
        }
    });
}

/// Create a rejected promise with the given reason
#[no_mangle]
pub extern "C" fn js_promise_rejected(reason: f64) -> *mut Promise {
    let promise = js_promise_new();
    js_promise_reject(promise, reason);
    promise
}

/// Check if a value is a promise (by checking if it's a valid pointer)
/// This is a simplified check - in reality we'd need type tags
#[no_mangle]
pub extern "C" fn js_is_promise(ptr: *mut Promise) -> i32 {
    if ptr.is_null() {
        return 0;
    }
    // Basic sanity check - could be more sophisticated
    1
}

/// Safe `await`-side check: given a NaN-boxed JSValue, return 1 if it
/// points at a real Promise allocation and 0 otherwise. Used by the
/// LLVM backend's `Expr::Await` lowering so that `await <non-promise>`
/// doesn't dereference a garbage pointer as if it were a `Promise`.
///
/// Inspects the NaN-box tag and, when the value is a pointer, walks
/// back to the `GcHeader` to read the `obj_type`. Any non-POINTER_TAG
/// bits (primitives, strings, bigints, null, undefined) return 0.
#[no_mangle]
pub extern "C" fn js_value_is_promise(value: f64) -> i32 {
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
    // #854: POINTER_MASK part of the NaN-boxing tag contract — referenced
    // by sibling helpers and kept here so the constants stay co-located.
    #[allow(dead_code)]
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

    let bits = value.to_bits();
    let tag = bits & TAG_MASK;
    if tag != POINTER_TAG {
        return 0;
    }
    let ptr_usize = (bits & crate::value::POINTER_MASK) as usize;
    if ptr_usize < 0x10000 {
        return 0;
    }
    unsafe {
        let gc_header =
            (ptr_usize as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let ot = (*gc_header).obj_type;
        if ot == crate::gc::GC_TYPE_PROMISE {
            1
        } else {
            0
        }
    }
}

// Queue for scheduled promise resolutions
thread_local! {
    pub(in crate::promise) static SCHEDULED_RESOLVES: RefCell<Vec<(*mut Promise, f64)>> = const { RefCell::new(Vec::new()) };
}

/// Schedule a promise to be resolved with a value when microtasks run
/// This simulates an async operation completing
#[no_mangle]
pub extern "C" fn js_promise_schedule_resolve(promise: *mut Promise, value: f64) {
    SCHEDULED_RESOLVES.with(|q| {
        q.borrow_mut().push((promise, value));
    });
}

/// Process scheduled resolutions (called by js_promise_run_microtasks)
pub(super) fn process_scheduled_resolves() -> i32 {
    let mut count = 0;
    loop {
        let item = SCHEDULED_RESOLVES.with(|q| q.borrow_mut().pop());
        match item {
            Some((promise, value)) => {
                js_promise_resolve(promise, value);
                count += 1;
            }
            None => break,
        }
    }
    count
}

/// Create a new Promise with an executor callback.
/// The executor receives (resolve, reject) as arguments.
/// resolve and reject are closures that call js_promise_resolve/js_promise_reject.
///
/// Arguments:
/// - executor: A closure that takes 2 arguments (resolve_fn, reject_fn)
#[no_mangle]
pub extern "C" fn js_promise_new_with_executor(
    executor: *const crate::closure::ClosureHeader,
) -> *mut Promise {
    use crate::closure::{js_closure_alloc, js_closure_call2, js_closure_set_capture_ptr};

    let promise = js_promise_new();
    let promise_i64 = promise as i64;

    // Create resolve closure that captures the promise pointer
    // The resolve function signature is: (closure: *const ClosureHeader, value: f64) -> f64
    let resolve_closure = js_closure_alloc(promise_resolve_fn as *const u8, 1);
    js_closure_set_capture_ptr(resolve_closure, 0, promise_i64);

    // Create reject closure that captures the promise pointer
    let reject_closure = js_closure_alloc(promise_reject_fn as *const u8, 1);
    js_closure_set_capture_ptr(reject_closure, 0, promise_i64);

    // Call the executor with (resolve_closure, reject_closure)
    // The closures are passed as f64 by bitcasting the pointer bits
    // This preserves the exact bits of the pointer when passed through f64 ABI
    let resolve_f64: f64 = f64::from_bits(i64::cast_unsigned(resolve_closure as i64));
    let reject_f64: f64 = f64::from_bits(i64::cast_unsigned(reject_closure as i64));
    js_closure_call2(executor, resolve_f64, reject_f64);

    promise
}

/// Internal resolve function for Promise executor callbacks.
/// Called when user calls resolve(value) inside the executor.
extern "C" fn promise_resolve_fn(closure: *const crate::closure::ClosureHeader, value: f64) -> f64 {
    use crate::closure::js_closure_get_capture_ptr;

    let promise_ptr = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    js_promise_resolve(promise_ptr, value);
    0.0 // resolve returns undefined
}

/// Internal reject function for Promise executor callbacks.
/// Called when user calls reject(reason) inside the executor.
extern "C" fn promise_reject_fn(closure: *const crate::closure::ClosureHeader, reason: f64) -> f64 {
    use crate::closure::js_closure_get_capture_ptr;

    let promise_ptr = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    js_promise_reject(promise_ptr, reason);
    0.0 // reject returns undefined
}

/// Promise.all - takes an array of promises and returns a promise that resolves
/// with an array of all resolved values, or rejects if any promise rejects.
///
/// Arguments:
/// - promises_arr: pointer to an ArrayHeader containing promise pointers (as NaN-boxed f64)
///
/// Returns: a new Promise that resolves with an array of results
#[no_mangle]
pub extern "C" fn js_promise_all(promises_arr: *const crate::array::ArrayHeader) -> *mut Promise {
    use crate::array::{js_array_alloc, js_array_get_f64, js_array_length, js_array_set_f64};
    use crate::value::js_nanbox_get_pointer;

    let result_promise = js_promise_new();

    if promises_arr.is_null() {
        let empty_arr = js_array_alloc(0);
        unsafe {
            (*empty_arr).length = 0;
        }
        let arr_f64 = crate::value::js_nanbox_pointer(empty_arr as i64);
        js_promise_resolve(result_promise, arr_f64);
        return result_promise;
    }

    let count = js_array_length(promises_arr);

    if count == 0 {
        let empty_arr = js_array_alloc(0);
        unsafe {
            (*empty_arr).length = 0;
        }
        let arr_f64 = crate::value::js_nanbox_pointer(empty_arr as i64);
        js_promise_resolve(result_promise, arr_f64);
        return result_promise;
    }

    let results_arr = js_array_alloc(count);
    unsafe {
        (*results_arr).length = count;
    }

    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    for i in 0..count {
        js_array_set_f64(results_arr, i, f64::from_bits(TAG_UNDEFINED));
    }

    let state_arr = js_array_alloc(2);
    unsafe {
        (*state_arr).length = 2;
    }
    js_array_set_f64(state_arr, 0, count as f64);
    js_array_set_f64(state_arr, 1, 0.0);

    for i in 0..count {
        let promise_f64 = adapt_foreign_promise_value(js_array_get_f64(promises_arr, i));

        if js_value_is_promise(promise_f64) == 0 {
            js_array_set_f64(results_arr, i, promise_f64);
            let remaining = js_array_get_f64(state_arr, 0) - 1.0;
            js_array_set_f64(state_arr, 0, remaining);
            continue;
        }

        let promise_ptr = js_nanbox_get_pointer(promise_f64) as *mut Promise;
        let state = PromiseAllState {
            result_promise,
            results_arr,
            state_arr,
            index: i,
        };

        unsafe {
            match (*promise_ptr).state {
                PromiseState::Fulfilled => {
                    TASK_QUEUE.with(|q| {
                        q.borrow_mut().push_back(Task::PromiseAll(
                            state,
                            (*promise_ptr).value,
                            true,
                            context_for_promise(promise_ptr),
                        ));
                    });
                }
                PromiseState::Rejected => {
                    TASK_QUEUE.with(|q| {
                        q.borrow_mut().push_back(Task::PromiseAll(
                            state,
                            (*promise_ptr).reason,
                            false,
                            context_for_promise(promise_ptr),
                        ));
                    });
                }
                PromiseState::Pending => {
                    PROMISE_ALL_STATES.with(|states| {
                        states.borrow_mut().push((promise_ptr as usize, state));
                    });
                    set_promise_callback_context(promise_ptr);
                }
            }
        }
    }

    let remaining = js_array_get_f64(state_arr, 0);
    if remaining == 0.0 {
        let arr_f64 = crate::value::js_nanbox_pointer(results_arr as i64);
        js_promise_resolve(result_promise, arr_f64);
    }

    result_promise
}

#[inline]
fn promise_all_fulfill_direct(state: PromiseAllState, value: f64) {
    use crate::array::{js_array_get_f64, js_array_set_f64};

    if state.result_promise.is_null() || state.results_arr.is_null() || state.state_arr.is_null() {
        return;
    }

    let rejected = js_array_get_f64(state.state_arr, 1);
    if rejected != 0.0 {
        return;
    }

    js_array_set_f64(state.results_arr, state.index, value);
    let remaining = js_array_get_f64(state.state_arr, 0) - 1.0;
    js_array_set_f64(state.state_arr, 0, remaining);

    if remaining == 0.0 {
        let arr_f64 = crate::value::js_nanbox_pointer(state.results_arr as i64);
        js_promise_resolve(state.result_promise, arr_f64);
    }
}

#[inline]
fn promise_all_reject_direct(
    result_promise: *mut Promise,
    state_arr: *mut crate::array::ArrayHeader,
    reason: f64,
) {
    use crate::array::{js_array_get_f64, js_array_set_f64};

    if result_promise.is_null() || state_arr.is_null() {
        return;
    }

    let rejected = js_array_get_f64(state_arr, 1);
    if rejected != 0.0 {
        return;
    }

    js_array_set_f64(state_arr, 1, 1.0);
    js_promise_reject(result_promise, reason);
}

/// Promise.race - takes an array of promises and returns a promise that resolves
/// or rejects with the first promise that settles.
#[no_mangle]
pub extern "C" fn js_promise_race(promises_arr: *const crate::array::ArrayHeader) -> *mut Promise {
    use crate::array::{js_array_get_f64, js_array_length};
    use crate::closure::{js_closure_alloc, js_closure_set_capture_ptr};
    use crate::value::js_nanbox_get_pointer;

    let result_promise = js_promise_new();

    if promises_arr.is_null() {
        // Promise.race([]) — never settles (per spec), but return pending promise
        return result_promise;
    }

    let count = js_array_length(promises_arr);
    if count == 0 {
        return result_promise;
    }

    // Both handlers capture only `result_promise` and don't depend on
    // the input index — so allocate once and share across all N inputs.
    // Saves (N-1) × 2 closure allocs per Promise.race call.
    let shared_resolve = js_closure_alloc(promise_race_resolve_handler as *const u8, 1);
    js_closure_set_capture_ptr(shared_resolve, 0, result_promise as i64);
    let shared_reject = js_closure_alloc(promise_race_reject_handler as *const u8, 1);
    js_closure_set_capture_ptr(shared_reject, 0, result_promise as i64);

    // For each promise, attach resolve/reject handlers that settle the result promise.
    // Per the spec, even when an input promise is already settled we MUST route the
    // resolution through the microtask queue (by registering `.then` handlers) rather
    // than calling js_promise_resolve synchronously.  The synchronous short-circuit was
    // causing race / any results to appear too early in the output when compared against
    // Node's microtask-ordered output.
    for i in 0..count {
        let promise_f64 = adapt_foreign_promise_value(js_array_get_f64(promises_arr, i));
        // Discriminate via GC-header obj_type — string/bigint NaN-boxed
        // values would otherwise pass through pointer extraction and crash
        // js_promise_then.
        if js_value_is_promise(promise_f64) == 0 {
            // Non-promise value — wrap as an already-resolved promise so the
            // resolution goes through the normal microtask path.
            let wrapped = js_promise_resolved(promise_f64);
            js_promise_then(wrapped, shared_resolve, shared_reject);
            continue;
        }
        let promise_ptr = js_nanbox_get_pointer(promise_f64) as *mut Promise;

        // Attach handlers via then — if the input is already settled this will
        // push a microtask rather than resolving result_promise synchronously.
        js_promise_attach_handlers(promise_ptr, shared_resolve, shared_reject);
    }

    result_promise
}

/// Handler for Promise.race fulfill — resolves the race promise with the first value
extern "C" fn promise_race_resolve_handler(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    use crate::closure::js_closure_get_capture_ptr;
    let result_promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    if result_promise.is_null() {
        return 0.0;
    }
    // Only settle if still pending (first one wins)
    if matches!(unsafe { (*result_promise).state }, PromiseState::Pending) {
        js_promise_resolve(result_promise, value);
    }
    0.0
}

/// Handler for Promise.race reject — rejects the race promise with the first reason
extern "C" fn promise_race_reject_handler(
    closure: *const crate::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    use crate::closure::js_closure_get_capture_ptr;
    let result_promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    if result_promise.is_null() {
        return 0.0;
    }
    if matches!(unsafe { (*result_promise).state }, PromiseState::Pending) {
        js_promise_reject(result_promise, reason);
    }
    0.0
}

/// Await any promise value.
/// In native-only mode (no V8), all promises are native POINTER_TAG promises.
/// The codegen-emitted busy-wait loop handles polling the promise state,
/// so we just return the value as-is.
/// In V8 mode (perry-jsruntime), this function is overridden by the V8-aware
/// version that can also handle JS_HANDLE_TAG promises.
#[no_mangle]
pub extern "C" fn js_await_any_promise(value: f64) -> f64 {
    value
}

/// ECMAScript thenable assimilation for `await`. Issue #586.
///
/// `await x` semantics: if `x` is an object with a callable `then` method,
/// the runtime should call `x.then(resolve, reject)` and resume with whatever
/// the underlying then implementation passes to `resolve`. Real Promises take
/// the fast path; thenables (e.g. drizzle-orm's `QueryPromise`) need this.
///
/// Behavior:
/// - Already a Promise → pass through unchanged (caller's await loop polls it).
/// - Object whose class chain contains a `then(onFulfilled, onRejected)` method
///   → allocate a fresh Promise, build resolve/reject closures bound to it,
///     invoke `value.then(resolve, reject)`, and return the new Promise (which
///     the await loop then polls). When the user's `then` calls `resolve(v)`,
///     our handler resolves the wrapper promise; the await loop sees Fulfilled
///     and returns `v`.
/// - Anything else (primitives, plain objects without a `then` method, Map /
///   Set / Buffer / handle values) → pass through unchanged so the await
///   resolves with the value itself per spec.
///
/// `then` is looked up only in the class vtable, not as an instance field.
/// Object literals with a `then: () => ...` arrow stored as a property are
/// uncommon in practice and would require a parallel `js_object_get_field_by_name`
/// probe — out of scope for this fix.
#[no_mangle]
pub extern "C" fn js_assimilate_thenable(value: f64) -> f64 {
    use crate::value::JSValue;

    bump(&MT_THENABLE_PROBE_COUNT);
    // Real Promise — caller's await loop already handles it.
    if js_value_is_promise(value) != 0 {
        return value;
    }

    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);

    if !jsval.is_pointer() {
        return value;
    }

    let raw_ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    if raw_ptr < 0x100000 {
        return value;
    }

    // Side-table-tracked heap types don't have ClassVTable entries; skip.
    if crate::buffer::is_registered_buffer(raw_ptr)
        || crate::set::is_registered_set(raw_ptr)
        || crate::map::is_registered_map(raw_ptr)
        || crate::symbol::is_registered_symbol(raw_ptr)
        || crate::regex::is_regex_pointer(raw_ptr as *const u8)
        || crate::date::is_registered_date_bits(bits)
    {
        return value;
    }

    let obj_ptr = jsval.as_pointer::<crate::object::ObjectHeader>();
    if obj_ptr.is_null() {
        return value;
    }

    // Verify GC type before reading class_id; reading garbage past random
    // pointers would either return a fake match or segfault.
    let class_id = unsafe {
        let gc_header =
            (obj_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc_header).obj_type;
        if gc_type != crate::gc::GC_TYPE_OBJECT {
            return value;
        }
        (*obj_ptr).class_id
    };
    if class_id == 0 {
        return value;
    }

    // Probe the vtable chain for `then`. Bail out on plain objects (no class
    // method) so the await passes the original value through unchanged.
    let (then_func_ptr, then_param_count) =
        match crate::object::lookup_class_method_in_chain(class_id, "then") {
            Some(p) => p,
            None => return value,
        };

    // Allocate the wrapper promise plus resolve/reject closures pointing at it.
    let new_promise = js_promise_new();
    let promise_i64 = new_promise as i64;

    let resolve_closure = crate::closure::js_closure_alloc(promise_resolve_fn as *const u8, 1);
    crate::closure::js_closure_set_capture_ptr(resolve_closure, 0, promise_i64);
    let reject_closure = crate::closure::js_closure_alloc(promise_reject_fn as *const u8, 1);
    crate::closure::js_closure_set_capture_ptr(reject_closure, 0, promise_i64);

    // The user's `then(onFulfilled, onRejected)` reads each parameter as a
    // raw f64 closure pointer (matching the convention used by
    // `js_promise_new_with_executor`).
    let resolve_f64 = f64::from_bits(resolve_closure as u64);
    let reject_f64 = f64::from_bits(reject_closure as u64);

    // Invoke `value.then(resolve, reject)` via the vtable. Mirrors
    // `call_vtable_method` in object.rs: NaN-box `this` with POINTER_TAG so
    // the method body sees a real instance pointer.
    let this_f64 = f64::from_bits(JSValue::pointer(obj_ptr as *mut u8).bits());
    unsafe {
        match then_param_count {
            0 => {
                let f: extern "C" fn(f64) -> f64 = std::mem::transmute(then_func_ptr);
                f(this_f64);
            }
            1 => {
                let f: extern "C" fn(f64, f64) -> f64 = std::mem::transmute(then_func_ptr);
                f(this_f64, resolve_f64);
            }
            _ => {
                // 2+ params: pass resolve/reject; any extra slots arrive as NaN.
                let f: extern "C" fn(f64, f64, f64) -> f64 = std::mem::transmute(then_func_ptr);
                f(this_f64, resolve_f64, reject_f64);
            }
        }
    }

    crate::value::js_nanbox_pointer(new_promise as i64)
}

/// Build a `{ status: "fulfilled", value: v }` object for Promise.allSettled.
fn build_settled_fulfilled(value: f64) -> f64 {
    use crate::object::{js_object_alloc_with_shape, js_object_set_field};
    let packed = b"status\0value\0";
    let obj = js_object_alloc_with_shape(0x7FFF_FF10, 2, packed.as_ptr(), packed.len() as u32);
    let status_str = crate::string::js_string_from_bytes(b"fulfilled".as_ptr(), 9);
    let status_nb = crate::value::js_nanbox_string(status_str as i64);
    js_object_set_field(
        obj,
        0,
        crate::value::JSValue::from_bits(status_nb.to_bits()),
    );
    js_object_set_field(obj, 1, crate::value::JSValue::from_bits(value.to_bits()));
    crate::value::js_nanbox_pointer(obj as i64)
}

/// Build a `{ status: "rejected", reason: r }` object for Promise.allSettled.
fn build_settled_rejected(reason: f64) -> f64 {
    use crate::object::{js_object_alloc_with_shape, js_object_set_field};
    let packed = b"status\0reason\0";
    let obj = js_object_alloc_with_shape(0x7FFF_FF11, 2, packed.as_ptr(), packed.len() as u32);
    let status_str = crate::string::js_string_from_bytes(b"rejected".as_ptr(), 8);
    let status_nb = crate::value::js_nanbox_string(status_str as i64);
    js_object_set_field(
        obj,
        0,
        crate::value::JSValue::from_bits(status_nb.to_bits()),
    );
    js_object_set_field(obj, 1, crate::value::JSValue::from_bits(reason.to_bits()));
    crate::value::js_nanbox_pointer(obj as i64)
}

/// Promise.allSettled — never rejects; resolves with an array of result objects
/// where each entry is `{ status: "fulfilled", value }` or `{ status: "rejected", reason }`.
#[no_mangle]
pub extern "C" fn js_promise_all_settled(
    promises_arr: *const crate::array::ArrayHeader,
) -> *mut Promise {
    use crate::array::{js_array_alloc, js_array_get_f64, js_array_length, js_array_set_f64};
    use crate::closure::{
        js_closure_alloc, js_closure_set_capture_f64, js_closure_set_capture_ptr,
    };
    use crate::value::js_nanbox_get_pointer;

    let result_promise = js_promise_new();

    if promises_arr.is_null() {
        let empty_arr = js_array_alloc(0);
        unsafe {
            (*empty_arr).length = 0;
        }
        let arr_f64 = crate::value::js_nanbox_pointer(empty_arr as i64);
        js_promise_resolve(result_promise, arr_f64);
        return result_promise;
    }

    let count = js_array_length(promises_arr);
    if count == 0 {
        let empty_arr = js_array_alloc(0);
        unsafe {
            (*empty_arr).length = 0;
        }
        let arr_f64 = crate::value::js_nanbox_pointer(empty_arr as i64);
        js_promise_resolve(result_promise, arr_f64);
        return result_promise;
    }

    let results_arr = js_array_alloc(count);
    unsafe {
        (*results_arr).length = count;
    }
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    for i in 0..count {
        js_array_set_f64(results_arr, i, f64::from_bits(TAG_UNDEFINED));
    }

    // state: [remaining_count]
    let state_arr = js_array_alloc(1);
    unsafe {
        (*state_arr).length = 1;
    }
    js_array_set_f64(state_arr, 0, count as f64);

    for i in 0..count {
        let promise_f64 = adapt_foreign_promise_value(js_array_get_f64(promises_arr, i));

        // Only treat as a Promise if the value is a POINTER_TAG that walks
        // back to a GcHeader with obj_type == GC_TYPE_PROMISE. Otherwise
        // (string, plain number, undefined, null, object, etc.) wrap the
        // value as already-fulfilled — Promise.allSettled spec passes any
        // non-thenable through as `{status: "fulfilled", value}`.
        let is_promise = js_value_is_promise(promise_f64) != 0;

        if !is_promise {
            // Non-promise value — wrap as fulfilled and decrement
            let wrapped = build_settled_fulfilled(promise_f64);
            js_array_set_f64(results_arr, i, wrapped);
            let remaining = js_array_get_f64(state_arr, 0) - 1.0;
            js_array_set_f64(state_arr, 0, remaining);
            continue;
        }

        let promise_ptr = js_nanbox_get_pointer(promise_f64) as *mut Promise;

        // Fulfill: store {status:"fulfilled", value:v}
        let fulfill_closure = js_closure_alloc(promise_all_settled_fulfill_handler as *const u8, 4);
        js_closure_set_capture_ptr(fulfill_closure, 0, result_promise as i64);
        js_closure_set_capture_ptr(fulfill_closure, 1, results_arr as i64);
        js_closure_set_capture_ptr(fulfill_closure, 2, state_arr as i64);
        js_closure_set_capture_f64(fulfill_closure, 3, i as f64);

        // Reject: store {status:"rejected", reason:r}
        let reject_closure = js_closure_alloc(promise_all_settled_reject_handler as *const u8, 4);
        js_closure_set_capture_ptr(reject_closure, 0, result_promise as i64);
        js_closure_set_capture_ptr(reject_closure, 1, results_arr as i64);
        js_closure_set_capture_ptr(reject_closure, 2, state_arr as i64);
        js_closure_set_capture_f64(reject_closure, 3, i as f64);

        js_promise_attach_handlers(promise_ptr, fulfill_closure, reject_closure);
    }

    // If all were already non-promises
    let remaining = js_array_get_f64(state_arr, 0);
    if remaining == 0.0 {
        let arr_f64 = crate::value::js_nanbox_pointer(results_arr as i64);
        js_promise_resolve(result_promise, arr_f64);
    }

    result_promise
}

extern "C" fn promise_all_settled_fulfill_handler(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    use crate::array::{js_array_get_f64, js_array_set_f64, ArrayHeader};
    use crate::closure::{js_closure_get_capture_f64, js_closure_get_capture_ptr};

    let result_promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let results_arr = js_closure_get_capture_ptr(closure, 1) as *mut ArrayHeader;
    let state_arr = js_closure_get_capture_ptr(closure, 2) as *mut ArrayHeader;
    if result_promise.is_null() || results_arr.is_null() || state_arr.is_null() {
        return 0.0;
    }
    let index = js_closure_get_capture_f64(closure, 3) as u32;

    let wrapped = build_settled_fulfilled(value);
    js_array_set_f64(results_arr, index, wrapped);

    let remaining = js_array_get_f64(state_arr, 0) - 1.0;
    js_array_set_f64(state_arr, 0, remaining);

    if remaining == 0.0 {
        let arr_f64 = crate::value::js_nanbox_pointer(results_arr as i64);
        js_promise_resolve(result_promise, arr_f64);
    }
    0.0
}

extern "C" fn promise_all_settled_reject_handler(
    closure: *const crate::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    use crate::array::{js_array_get_f64, js_array_set_f64, ArrayHeader};
    use crate::closure::{js_closure_get_capture_f64, js_closure_get_capture_ptr};

    let result_promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let results_arr = js_closure_get_capture_ptr(closure, 1) as *mut ArrayHeader;
    let state_arr = js_closure_get_capture_ptr(closure, 2) as *mut ArrayHeader;
    if result_promise.is_null() || results_arr.is_null() || state_arr.is_null() {
        return 0.0;
    }
    let index = js_closure_get_capture_f64(closure, 3) as u32;

    let wrapped = build_settled_rejected(reason);
    js_array_set_f64(results_arr, index, wrapped);

    let remaining = js_array_get_f64(state_arr, 0) - 1.0;
    js_array_set_f64(state_arr, 0, remaining);

    if remaining == 0.0 {
        let arr_f64 = crate::value::js_nanbox_pointer(results_arr as i64);
        js_promise_resolve(result_promise, arr_f64);
    }
    0.0
}

/// Promise.any — settles with the first FULFILLED promise. If all reject, rejects
/// with an `AggregateError` whose `errors` array carries the collected reasons
/// (constructed via `js_aggregate_error_new` in the all-rejected path below).
#[no_mangle]
pub extern "C" fn js_promise_any(promises_arr: *const crate::array::ArrayHeader) -> *mut Promise {
    use crate::array::{js_array_alloc, js_array_get_f64, js_array_length, js_array_set_f64};
    use crate::closure::{
        js_closure_alloc, js_closure_set_capture_f64, js_closure_set_capture_ptr,
    };
    use crate::value::js_nanbox_get_pointer;

    let result_promise = js_promise_new();

    if promises_arr.is_null() {
        // Empty input — Promise.any rejects immediately with empty errors array
        let errors_arr = js_array_alloc(0);
        unsafe {
            (*errors_arr).length = 0;
        }
        let arr_f64 = crate::value::js_nanbox_pointer(errors_arr as i64);
        js_promise_reject(result_promise, arr_f64);
        return result_promise;
    }

    let count = js_array_length(promises_arr);
    if count == 0 {
        let errors_arr = js_array_alloc(0);
        unsafe {
            (*errors_arr).length = 0;
        }
        let arr_f64 = crate::value::js_nanbox_pointer(errors_arr as i64);
        js_promise_reject(result_promise, arr_f64);
        return result_promise;
    }

    let errors_arr = js_array_alloc(count);
    unsafe {
        (*errors_arr).length = count;
    }
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    for i in 0..count {
        js_array_set_f64(errors_arr, i, f64::from_bits(TAG_UNDEFINED));
    }

    // state: [remaining_rejections, settled_flag]
    let state_arr = js_array_alloc(2);
    unsafe {
        (*state_arr).length = 2;
    }
    js_array_set_f64(state_arr, 0, count as f64);
    js_array_set_f64(state_arr, 1, 0.0);

    // Fulfill closure captures only `[result_promise, state_arr]` — no
    // per-index payload, so we share one across all N inputs (mirrors
    // the Promise.all reject-closure sharing in commit 7c89fcc6).
    // Reject still needs per-index since it must write its error into
    // the correct slot of `errors_arr` for the eventual AggregateError.
    let shared_fulfill = js_closure_alloc(promise_any_fulfill_handler as *const u8, 2);
    js_closure_set_capture_ptr(shared_fulfill, 0, result_promise as i64);
    js_closure_set_capture_ptr(shared_fulfill, 1, state_arr as i64);

    for i in 0..count {
        let promise_f64 = adapt_foreign_promise_value(js_array_get_f64(promises_arr, i));
        // Discriminate via GC-header obj_type — string/bigint NaN-boxed
        // values would otherwise pass through pointer extraction and crash
        // js_promise_then.
        if js_value_is_promise(promise_f64) == 0 {
            // Non-promise value — treat as fulfilled, settle immediately if not yet settled
            let already_settled = js_array_get_f64(state_arr, 1);
            if already_settled == 0.0 {
                js_array_set_f64(state_arr, 1, 1.0);
                js_promise_resolve(result_promise, promise_f64);
            }
            return result_promise;
        }
        let promise_ptr = js_nanbox_get_pointer(promise_f64) as *mut Promise;

        let reject_closure = js_closure_alloc(promise_any_reject_handler as *const u8, 4);
        js_closure_set_capture_ptr(reject_closure, 0, result_promise as i64);
        js_closure_set_capture_ptr(reject_closure, 1, errors_arr as i64);
        js_closure_set_capture_ptr(reject_closure, 2, state_arr as i64);
        js_closure_set_capture_f64(reject_closure, 3, i as f64);

        js_promise_attach_handlers(promise_ptr, shared_fulfill, reject_closure);
    }

    result_promise
}

extern "C" fn promise_any_fulfill_handler(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    use crate::array::{js_array_get_f64, js_array_set_f64, ArrayHeader};
    use crate::closure::js_closure_get_capture_ptr;

    let result_promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let state_arr = js_closure_get_capture_ptr(closure, 1) as *mut ArrayHeader;
    if result_promise.is_null() || state_arr.is_null() {
        return 0.0;
    }

    let already_settled = js_array_get_f64(state_arr, 1);
    if already_settled != 0.0 {
        return 0.0;
    }
    js_array_set_f64(state_arr, 1, 1.0);

    js_promise_resolve(result_promise, value);
    0.0
}

extern "C" fn promise_any_reject_handler(
    closure: *const crate::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    use crate::array::{js_array_get_f64, js_array_set_f64, ArrayHeader};
    use crate::closure::{js_closure_get_capture_f64, js_closure_get_capture_ptr};

    let result_promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let errors_arr = js_closure_get_capture_ptr(closure, 1) as *mut ArrayHeader;
    let state_arr = js_closure_get_capture_ptr(closure, 2) as *mut ArrayHeader;
    if result_promise.is_null() || errors_arr.is_null() || state_arr.is_null() {
        return 0.0;
    }
    let index = js_closure_get_capture_f64(closure, 3) as u32;

    let already_settled = js_array_get_f64(state_arr, 1);
    if already_settled != 0.0 {
        return 0.0;
    }

    js_array_set_f64(errors_arr, index, reason);

    let remaining = js_array_get_f64(state_arr, 0) - 1.0;
    js_array_set_f64(state_arr, 0, remaining);

    if remaining == 0.0 {
        // All rejected — create an AggregateError with the collected
        // errors array and reject the result promise with it.
        js_array_set_f64(state_arr, 1, 1.0);
        let msg = crate::string::js_string_from_bytes(b"All promises were rejected".as_ptr(), 26);
        let agg_err = crate::error::js_aggregateerror_new(errors_arr, msg);
        let err_f64 = crate::value::js_nanbox_pointer(agg_err as i64);
        js_promise_reject(result_promise, err_f64);
    }
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::array::{js_array_alloc, js_array_set_f64};
    use crate::value::js_nanbox_pointer;

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
            let registered = PROMISE_ALL_STATES.with(|s| {
                s.borrow()
                    .iter()
                    .filter(|(k, _)| *k == shared as usize)
                    .count()
            });
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
}
