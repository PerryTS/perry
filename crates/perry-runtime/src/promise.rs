//! Promise implementation for async/await support
//!
//! This is a simplified Promise implementation for the Perry runtime.
//! It supports basic resolve/reject and then/catch chaining.

use std::cell::RefCell;
use std::ptr;

/// Promise state
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PromiseState {
    Pending = 0,
    Fulfilled = 1,
    Rejected = 2,
}

/// Closure pointer type for promise handlers (closures, not raw function pointers)
pub type ClosurePtr = *const crate::closure::ClosureHeader;

/// A Promise represents an eventual completion (or failure) of an async operation
#[repr(C)]
pub struct Promise {
    /// Current state of the promise
    pub(crate) state: PromiseState,
    /// The resolved value (if fulfilled)
    pub(crate) value: f64,
    /// The rejection reason (if rejected)
    pub(crate) reason: f64,
    /// Closure to run when fulfilled (null if none)
    pub(crate) on_fulfilled: ClosurePtr,
    /// Closure to run when rejected (null if none)
    pub(crate) on_rejected: ClosurePtr,
    /// Next promise in the chain (for .then())
    pub(crate) next: *mut Promise,
}

impl Promise {
    fn new() -> Self {
        Promise {
            state: PromiseState::Pending,
            value: 0.0,
            reason: 0.0,
            on_fulfilled: ptr::null(),
            on_rejected: ptr::null(),
            next: ptr::null_mut(),
        }
    }
}

// Global task queue for pending promise callbacks. Must be FIFO per
// ECMAScript microtask semantics: `Promise.resolve(1).then(...)` and
// `Promise.resolve(2).then(...)` registered in source order must run
// their continuations in source order (1 first, then 2). Using a
// `Vec` with `.pop()` produces LIFO ordering, breaking every test
// that prints inside multiple parallel promise chains.
thread_local! {
    static TASK_QUEUE: RefCell<std::collections::VecDeque<(*mut Promise, f64, bool)>>
        = RefCell::new(std::collections::VecDeque::new());
}

/// Allocate a new Promise
#[no_mangle]
pub extern "C" fn js_promise_new() -> *mut Promise {
    let raw = crate::gc::gc_malloc(std::mem::size_of::<Promise>(), crate::gc::GC_TYPE_PROMISE);
    let promise = raw as *mut Promise;
    unsafe {
        ptr::write(promise, Promise::new());
    }
    promise
}

/// Free a Promise (no-op — GC handles deallocation)
#[no_mangle]
pub extern "C" fn js_promise_free(_promise: *mut Promise) {
    // GC handles deallocation now
}

/// Get promise state (0=pending, 1=fulfilled, 2=rejected)
#[no_mangle]
pub extern "C" fn js_promise_state(promise: *mut Promise) -> i32 {
    if promise.is_null() {
        return -1;
    }
    unsafe { (*promise).state as i32 }
}

/// Get promise value (if fulfilled)
#[no_mangle]
pub extern "C" fn js_promise_value(promise: *mut Promise) -> f64 {
    if promise.is_null() {
        return 0.0;
    }
    let val = unsafe { (*promise).value };
    val
}

/// Get promise reason (if rejected)
#[no_mangle]
pub extern "C" fn js_promise_reason(promise: *mut Promise) -> f64 {
    if promise.is_null() {
        return 0.0;
    }
    unsafe { (*promise).reason }
}

/// Get promise result (value if fulfilled, reason if rejected)
/// This is what await should use to get the result of a promise.
/// For fulfilled promises, returns the resolved value.
/// For rejected promises, returns the rejection reason.
/// For pending promises (should not happen in normal use), returns 0.0.
#[no_mangle]
pub extern "C" fn js_promise_result(promise: *mut Promise) -> f64 {
    if promise.is_null() {
        return 0.0;
    }
    unsafe {
        match (*promise).state {
            PromiseState::Fulfilled => (*promise).value,
            PromiseState::Rejected => (*promise).reason,
            PromiseState::Pending => 0.0,
        }
    }
}

/// Resolve a promise with a value
#[no_mangle]
pub extern "C" fn js_promise_resolve(promise: *mut Promise, value: f64) {
    if promise.is_null() {
        return;
    }
    unsafe {
        if (*promise).state != PromiseState::Pending {
            return; // Already settled
        }
        (*promise).state = PromiseState::Fulfilled;
        (*promise).value = value;

        // Schedule callbacks. Push to TASK_QUEUE whenever there's anything
        // for the microtask runner to do — either invoke the user callback,
        // or propagate the value to the chained `next` promise. Issue #236:
        // pre-fix the queue push only fired when `on_fulfilled` was non-null,
        // so `.then(console.log)` (where `console.log`-as-value lowers to
        // a NULL ClosurePtr sentinel — see expr.rs:GlobalGet→PropertyGet
        // value path) skipped the queue entirely; the chained promise then
        // never settled and `await chained` busy-waited forever.
        if !(*promise).on_fulfilled.is_null() || !(*promise).next.is_null() {
            TASK_QUEUE.with(|q| {
                q.borrow_mut().push_back((promise, value, true));
            });
        }
    }
    // Issue #84: an `await` busy-wait that called `js_timer_tick` (or any
    // tick fn) which then resolved this promise needs to skip the
    // following `js_wait_for_event` sleep — otherwise it blocks for the
    // 1 s idle cap before the loop re-checks promise state. The notify
    // sets the flag so the immediately-following wait returns at once.
    crate::event_pump::js_notify_main_thread();
}

/// Resolve a promise with another promise (Promise chaining/unwrapping)
/// When the inner promise resolves, the outer promise adopts its value
#[no_mangle]
pub extern "C" fn js_promise_resolve_with_promise(outer: *mut Promise, inner: *mut Promise) {
    if outer.is_null() || inner.is_null() {
        return;
    }

    unsafe {
        if (*outer).state != PromiseState::Pending {
            return; // Already settled
        }

        // Check inner promise state
        match (*inner).state {
            PromiseState::Fulfilled => {
                // Inner already resolved - resolve outer with inner's value
                js_promise_resolve(outer, (*inner).value);
            }
            PromiseState::Rejected => {
                // Inner already rejected - reject outer with inner's reason
                js_promise_reject(outer, (*inner).reason);
            }
            PromiseState::Pending => {
                // Inner is pending - schedule resolution when inner settles
                // We create a closure that captures the outer promise pointer
                // and resolves it when called with the inner's value
                let outer_i64 = outer as i64;

                // Create a resolve forwarding closure
                let resolve_closure = crate::closure::js_closure_alloc(
                    promise_forward_resolve as *const u8,
                    1
                );
                crate::closure::js_closure_set_capture_ptr(resolve_closure, 0, outer_i64);

                // Create a reject forwarding closure
                let reject_closure = crate::closure::js_closure_alloc(
                    promise_forward_reject as *const u8,
                    1
                );
                crate::closure::js_closure_set_capture_ptr(reject_closure, 0, outer_i64);

                // Register the forwarding callbacks on the inner promise
                (*inner).on_fulfilled = resolve_closure;
                (*inner).on_rejected = reject_closure;
                (*inner).next = ptr::null_mut(); // Don't chain, we handle resolution ourselves
            }
        }
    }
}

/// Internal callback for forwarding resolve from inner to outer promise
extern "C" fn promise_forward_resolve(closure: *const crate::closure::ClosureHeader, value: f64) -> f64 {
    let outer_ptr = crate::closure::js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    js_promise_resolve(outer_ptr, value);
    0.0
}

/// Internal callback for forwarding reject from inner to outer promise
extern "C" fn promise_forward_reject(closure: *const crate::closure::ClosureHeader, reason: f64) -> f64 {
    let outer_ptr = crate::closure::js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    js_promise_reject(outer_ptr, reason);
    0.0
}

/// Reject a promise with a reason
#[no_mangle]
pub extern "C" fn js_promise_reject(promise: *mut Promise, reason: f64) {
    if promise.is_null() {
        return;
    }
    unsafe {
        if (*promise).state != PromiseState::Pending {
            return; // Already settled
        }
        (*promise).state = PromiseState::Rejected;
        (*promise).reason = reason;

        // Schedule callbacks. Same propagation rule as `js_promise_resolve`
        // (#236): push to the queue whenever there's a callback to invoke
        // OR a chained `next` promise to forward to.
        if !(*promise).on_rejected.is_null() || !(*promise).next.is_null() {
            TASK_QUEUE.with(|q| {
                q.borrow_mut().push_back((promise, reason, false));
            });
        }
    }
    // Issue #84: see js_promise_resolve — same wake reasoning.
    crate::event_pump::js_notify_main_thread();
}

/// Register fulfillment callback, returns a new promise for chaining
#[no_mangle]
pub extern "C" fn js_promise_then(
    promise: *mut Promise,
    on_fulfilled: ClosurePtr,
    on_rejected: ClosurePtr,
) -> *mut Promise {
    if promise.is_null() {
        return ptr::null_mut();
    }

    let next = js_promise_new();

    unsafe {
        (*promise).on_fulfilled = on_fulfilled;
        (*promise).on_rejected = on_rejected;
        (*promise).next = next;

        // If already settled, schedule callback immediately. Same propagation
        // rule as `js_promise_resolve`/`js_promise_reject` (#236): push to the
        // queue when there's either a callback to invoke OR a chained `next`
        // promise to forward to. `next` is always non-null here (we just
        // created it), so this is effectively unconditional — the explicit
        // checks document the intent.
        match (*promise).state {
            PromiseState::Fulfilled => {
                if !on_fulfilled.is_null() || !next.is_null() {
                    TASK_QUEUE.with(|q| {
                        q.borrow_mut().push_back((promise, (*promise).value, true));
                    });
                }
            }
            PromiseState::Rejected => {
                if !on_rejected.is_null() || !next.is_null() {
                    TASK_QUEUE.with(|q| {
                        q.borrow_mut().push_back((promise, (*promise).reason, false));
                    });
                }
            }
            PromiseState::Pending => {}
        }
    }

    next
}

/// Register rejection callback, returns a new promise for chaining
/// This is equivalent to .catch(onRejected) in JavaScript
#[no_mangle]
pub extern "C" fn js_promise_catch(
    promise: *mut Promise,
    on_rejected: ClosurePtr,
) -> *mut Promise {
    js_promise_then(promise, ptr::null(), on_rejected)
}

/// Register finally callback, returns a new promise for chaining
/// This is equivalent to .finally(onFinally) in JavaScript
#[no_mangle]
pub extern "C" fn js_promise_finally(
    promise: *mut Promise,
    on_finally: ClosurePtr,
) -> *mut Promise {
    // For finally, we pass the same callback for both fulfilled and rejected
    // The finally callback doesn't receive any arguments in JS
    js_promise_then(promise, on_finally, on_finally)
}

/// Process all pending promise callbacks (run microtasks)
#[no_mangle]
pub extern "C" fn js_promise_run_microtasks() -> i32 {
    let mut ran = 0;

    // First, tick timers to resolve any expired timer promises
    ran += crate::timer::js_timer_tick();

    // Process callback timers (setTimeout with callbacks)
    ran += crate::timer::js_callback_timer_tick();

    // Process interval timers (setInterval)
    ran += crate::timer::js_interval_timer_tick();

    // Process any scheduled resolutions (simulates async completions)
    ran += process_scheduled_resolves();

    // Process pending thread results (from perry/thread spawn)
    ran += crate::thread::js_thread_process_pending();

    // Drain queued microtasks (from queueMicrotask() calls).
    crate::builtins::js_drain_queued_microtasks();

    // Then process the task queue
    loop {
        let task = TASK_QUEUE.with(|q| q.borrow_mut().pop_front());

        match task {
            Some((promise, value, is_fulfilled)) => {
                unsafe {
                    let callback = if is_fulfilled {
                        (*promise).on_fulfilled
                    } else {
                        (*promise).on_rejected
                    };

                    // No callback registered → propagate the value/reason
                    // to the next promise without invoking anything.
                    if callback.is_null() {
                        if !(*promise).next.is_null() {
                            if is_fulfilled {
                                js_promise_resolve((*promise).next, value);
                            } else {
                                js_promise_reject((*promise).next, value);
                            }
                        }
                        ran += 1;
                        continue;
                    }

                    // Wrap the callback in a setjmp so a `throw` inside
                    // it rejects the next promise instead of crashing
                    // through `print_uncaught` (TRY_DEPTH would otherwise
                    // be 0 here — microtask runner has no surrounding
                    // user-level try block). Same setjmp-from-Rust
                    // pattern as `gc.rs::mark_stack_roots`.
                    extern "C" {
                        fn setjmp(env: *mut i32) -> i32;
                    }
                    let buf = crate::exception::js_try_push();
                    let jumped = setjmp(buf);
                    if jumped == 0 {
                        let result = crate::closure::js_closure_call1(callback, value);
                        crate::exception::js_try_end();
                        if !(*promise).next.is_null() {
                            // Spec: when a .then callback returns a thenable
                            // (a Promise), the chained `next` promise must
                            // adopt the thenable's eventual state, not store
                            // the Promise pointer as its value. Issue #256:
                            // pre-fix Perry's runtime stored the Promise
                            // pointer directly, so the async-step driver's
                            // recursive `step()` returns produced
                            // `Promise<Promise<...>>` chains that never
                            // unwrapped to the real value. Without this
                            // unwrap, `await fn()` of an async function with
                            // multiple `await`s observes a Pending Promise as
                            // the resolved value.
                            if js_value_is_promise(result) != 0 {
                                let inner = crate::value::js_nanbox_get_pointer(result)
                                    as *mut Promise;
                                if !inner.is_null() && inner != (*promise).next {
                                    js_promise_resolve_with_promise((*promise).next, inner);
                                } else {
                                    js_promise_resolve((*promise).next, result);
                                }
                            } else {
                                js_promise_resolve((*promise).next, result);
                            }
                        }
                    } else {
                        // Callback threw — convert to rejection of next
                        // promise. Pull the exception value, clear it,
                        // pop the try block (longjmp doesn't unwind it).
                        let exc = crate::exception::js_get_exception();
                        crate::exception::js_clear_exception();
                        crate::exception::js_try_end();
                        if !(*promise).next.is_null() {
                            js_promise_reject((*promise).next, exc);
                        }
                    }
                }
                ran += 1;
            }
            None => break,
        }
    }

    ran
}

/// Create a resolved promise with the given value.
///
/// Matches ES spec `Promise.resolve(x)`: when `x` is itself a Promise the
/// returned promise adopts its state instead of storing the inner Promise
/// pointer as a plain value. This is the path async-function `return <expr>`
/// lowers through (see `perry-codegen/src/stmt.rs::Stmt::Return`) — without
/// the unwrap, `async function produce(): Promise<T> { return new Promise(...) }`
/// would return a promise whose `value` is a NaN-boxed pointer to the inner
/// Promise struct, so `await produce()` would see `typeof = 'object'` with all
/// user fields undefined (the Promise struct's layout) before the inner's
/// `setTimeout`/`resolve` ever fires. Closes #77.
#[no_mangle]
pub extern "C" fn js_promise_resolved(value: f64) -> *mut Promise {
    let promise = js_promise_new();
    if js_value_is_promise(value) != 0 {
        let inner = crate::value::js_nanbox_get_pointer(value) as *mut Promise;
        if !inner.is_null() && inner != promise {
            js_promise_resolve_with_promise(promise, inner);
            return promise;
        }
    }
    js_promise_resolve(promise, value);
    promise
}

/// `Array.fromAsync(input)` — Node 22+ static method.
///
/// Returns a Promise that resolves to an Array. Two input shapes:
///   1. **Array**: each element is awaited (if it's a Promise) and the
///      results are collected. Equivalent to `Promise.all(input)`.
///   2. **Async iterator** (object with a `.next()` method): we call
///      `.next()` repeatedly via the closure-chained .then() pattern,
///      pushing each `value` until `done` is true, then resolve the
///      output Promise with the collected array.
///
/// `input` is the NaN-boxed input value. Returns a NaN-boxed Promise
/// pointer (POINTER_TAG) so the caller's `await` can unwrap it.
#[no_mangle]
pub extern "C" fn js_array_from_async(input: f64) -> f64 {
    use crate::array::{js_array_alloc, ArrayHeader};
    use crate::closure::{js_closure_alloc, js_closure_set_capture_ptr};
    use crate::value::js_nanbox_get_pointer;

    // Strip NaN-box to get the raw pointer.
    let raw_ptr = js_nanbox_get_pointer(input) as usize;
    if raw_ptr == 0 {
        // null/undefined input — resolve to empty array
        let empty = js_array_alloc(0);
        unsafe { (*empty).length = 0; }
        let arr_f64 = crate::value::js_nanbox_pointer(empty as i64);
        let p = js_promise_resolved(arr_f64);
        return crate::value::js_nanbox_pointer(p as i64);
    }

    // Path 1: input is an Array. Reuse Promise.all behavior — js_promise_all
    // handles a mix of promise and non-promise elements correctly.
    unsafe {
        let gc_header = (raw_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
            let arr_ptr = raw_ptr as *const ArrayHeader;
            let p = js_promise_all(arr_ptr);
            return crate::value::js_nanbox_pointer(p as i64);
        }
    }

    // Path 2: async iterator (or any other object). Allocate a result
    // Promise and an empty result Array, then kick off the .next() chain.
    let result_promise = js_promise_new();
    let result_arr = js_array_alloc(0);
    unsafe { (*result_arr).length = 0; }

    // Build the recursive .next() handler closure. Captures:
    //   [0] result_promise (Promise to resolve at the end)
    //   [1] result_arr (Array to push each value into)
    //   [2] iter object (raw pointer; we re-NaN-box on .next() call)
    let chain_closure = js_closure_alloc(array_from_async_step as *const u8, 3);
    js_closure_set_capture_ptr(chain_closure, 0, result_promise as i64);
    js_closure_set_capture_ptr(chain_closure, 1, result_arr as i64);
    js_closure_set_capture_ptr(chain_closure, 2, raw_ptr as i64);

    // Kick off the first .next() call. The handler returns the iter result
    // (or undefined for done) — we wire it through `.then(chain_closure)`
    // which will recurse.
    unsafe {
        array_from_async_call_next(raw_ptr, chain_closure);
    }

    crate::value::js_nanbox_pointer(result_promise as i64)
}

/// Helper that calls `iter.next()` (returning a Promise) and attaches
/// `chain_closure` as both fulfill and reject handlers. Used by the async
/// iterator path of `js_array_from_async`.
unsafe fn array_from_async_call_next(
    iter_ptr: usize,
    chain_closure: *const crate::closure::ClosureHeader,
) {
    // Re-NaN-box the iter pointer for js_native_call_method.
    let iter_f64 = crate::value::js_nanbox_pointer(iter_ptr as i64);
    let method_name = b"next";
    // Call iter.next() — returns a Promise<{value, done}> for async generators
    // or `{value, done}` directly for sync iterators.
    let next_result = crate::object::js_native_call_method(
        iter_f64,
        method_name.as_ptr() as *const i8,
        method_name.len(),
        std::ptr::null(),
        0,
    );

    // If the result is a Promise pointer, attach the handler via .then.
    let next_ptr = crate::value::js_nanbox_get_pointer(next_result) as usize;
    if next_ptr != 0 {
        let gc_header = (next_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE)
            as *const crate::gc::GcHeader;
        if (*gc_header).obj_type == crate::gc::GC_TYPE_PROMISE {
            let next_promise = next_ptr as *mut Promise;
            // Use the chain_closure for both fulfill and reject. On rejection
            // we just propagate by resolving the result_promise with undefined.
            js_promise_then(next_promise, chain_closure as *const _, chain_closure as *const _);
            return;
        }
    }
    // Synchronous iterator path: invoke the handler directly with the
    // result so the iteration loop continues without going through .then.
    array_from_async_step(chain_closure as *const _, next_result);
}

/// `.then(...)` handler invoked once per `.next()` resolution. Reads the
/// `{value, done}` iter-result, pushes `value` into the accumulator array,
/// and either resolves the output Promise (when `done`) or schedules
/// another `.next()` call.
extern "C" fn array_from_async_step(
    closure: *const crate::closure::ClosureHeader,
    iter_result: f64,
) -> f64 {
    use crate::array::{ArrayHeader, js_array_push_f64};
    use crate::closure::{js_closure_get_capture_ptr, js_closure_set_capture_ptr};

    let result_promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let mut result_arr = js_closure_get_capture_ptr(closure, 1) as *mut ArrayHeader;
    let iter_ptr = js_closure_get_capture_ptr(closure, 2) as usize;

    if result_promise.is_null() || result_arr.is_null() || iter_ptr == 0 {
        return 0.0;
    }

    // Read `done` and `value` off the iter result. The result is either an
    // object with those fields, or undefined (if next() returned undefined).
    let result_bits = iter_result.to_bits();
    let result_obj_ptr = if (result_bits >> 48) == 0x7FFD {
        (result_bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::object::ObjectHeader
    } else if result_bits != 0 && result_bits <= 0x0000_FFFF_FFFF_FFFF {
        result_bits as *const crate::object::ObjectHeader
    } else {
        // Treat malformed result as `done: true`.
        std::ptr::null()
    };

    if result_obj_ptr.is_null() {
        // No more values — resolve the output promise with the collected array.
        let arr_f64 = crate::value::js_nanbox_pointer(result_arr as i64);
        unsafe { js_promise_resolve(result_promise, arr_f64); }
        return 0.0;
    }

    // Look up "done" and "value" fields by name.
    let done_key = make_static_string(b"done");
    let value_key = make_static_string(b"value");
    let done_jv = unsafe {
        crate::object::js_object_get_field_by_name(result_obj_ptr, done_key)
    };
    let value_jv = unsafe {
        crate::object::js_object_get_field_by_name(result_obj_ptr, value_key)
    };
    let done_f64 = f64::from_bits(done_jv.bits());
    let value_f64 = f64::from_bits(value_jv.bits());

    if crate::value::js_is_truthy(done_f64) != 0 {
        // Iteration complete — resolve with the accumulated array.
        let arr_f64 = crate::value::js_nanbox_pointer(result_arr as i64);
        unsafe { js_promise_resolve(result_promise, arr_f64); }
        return 0.0;
    }

    // Push the value (push_f64 may grow & return a new pointer).
    result_arr = js_array_push_f64(result_arr, value_f64);
    // Update the closure capture so subsequent steps see the (possibly
    // moved) array.
    js_closure_set_capture_ptr(closure as *mut _, 1, result_arr as i64);

    // Recurse: call iter.next() again. The same closure will be invoked
    // when the next promise resolves.
    unsafe {
        array_from_async_call_next(iter_ptr, closure);
    }

    0.0
}

/// Helper to allocate a static StringHeader for property-name lookups.
/// Reuses `js_string_from_bytes` so the result is GC-tracked.
fn make_static_string(bytes: &[u8]) -> *const crate::string::StringHeader {
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
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
        let gc_header = (ptr_usize as *const u8).sub(crate::gc::GC_HEADER_SIZE)
            as *const crate::gc::GcHeader;
        if (*gc_header).obj_type == crate::gc::GC_TYPE_PROMISE {
            1
        } else {
            0
        }
    }
}

// Queue for scheduled promise resolutions
thread_local! {
    static SCHEDULED_RESOLVES: RefCell<Vec<(*mut Promise, f64)>> = RefCell::new(Vec::new());
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
fn process_scheduled_resolves() -> i32 {
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
pub extern "C" fn js_promise_new_with_executor(executor: *const crate::closure::ClosureHeader) -> *mut Promise {
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
    let resolve_f64: f64 = unsafe { std::mem::transmute(resolve_closure as i64) };
    let reject_f64: f64 = unsafe { std::mem::transmute(reject_closure as i64) };
    unsafe {
        js_closure_call2(executor, resolve_f64, reject_f64);
    }

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
    use crate::array::{ArrayHeader, js_array_alloc, js_array_get_f64, js_array_length, js_array_set_f64};
    use crate::closure::{js_closure_alloc, js_closure_set_capture_ptr, js_closure_set_capture_f64};
    use crate::value::js_nanbox_get_pointer;

    // Create the result promise
    let result_promise = js_promise_new();

    if promises_arr.is_null() {
        // Promise.all([]) resolves immediately with empty array
        let empty_arr = js_array_alloc(0);
        unsafe { (*empty_arr).length = 0; }
        let arr_f64 = crate::value::js_nanbox_pointer(empty_arr as i64);
        js_promise_resolve(result_promise, arr_f64);
        return result_promise;
    }

    let count = js_array_length(promises_arr);

    if count == 0 {
        // Promise.all([]) resolves immediately with empty array
        let empty_arr = js_array_alloc(0);
        unsafe { (*empty_arr).length = 0; }
        let arr_f64 = crate::value::js_nanbox_pointer(empty_arr as i64);
        js_promise_resolve(result_promise, arr_f64);
        return result_promise;
    }

    // Allocate result array to hold resolved values
    let results_arr = js_array_alloc(count);
    unsafe { (*results_arr).length = count; }

    // Initialize all elements to undefined
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    for i in 0..count {
        js_array_set_f64(results_arr, i, f64::from_bits(TAG_UNDEFINED));
    }

    // Allocate state: [remaining_count, rejected_flag]
    // We use an array to hold mutable shared state across closures
    let state_arr = js_array_alloc(2);
    unsafe { (*state_arr).length = 2; }
    js_array_set_f64(state_arr, 0, count as f64);  // remaining count
    js_array_set_f64(state_arr, 1, 0.0);            // rejected flag (0 = not rejected)

    // For each promise in the array, attach a .then handler
    for i in 0..count {
        let promise_f64 = js_array_get_f64(promises_arr, i);

        // Discriminate via the GC-header obj_type, not via raw pointer
        // extraction: string/bigint NaN-boxed values produce non-null
        // pointers from js_nanbox_get_pointer and would be passed to
        // js_promise_then as if they were Promises.
        if js_value_is_promise(promise_f64) == 0 {
            // Not a promise — treat as already resolved value
            js_array_set_f64(results_arr, i, promise_f64);
            let remaining = js_array_get_f64(state_arr, 0) - 1.0;
            js_array_set_f64(state_arr, 0, remaining);
            continue;
        }

        let promise_ptr = js_nanbox_get_pointer(promise_f64) as *mut Promise;

        // Create fulfill closure for this promise
        // Captures: [result_promise, results_arr, state_arr, index]
        let fulfill_closure = js_closure_alloc(promise_all_fulfill_handler as *const u8, 4);
        js_closure_set_capture_ptr(fulfill_closure, 0, result_promise as i64);
        js_closure_set_capture_ptr(fulfill_closure, 1, results_arr as i64);
        js_closure_set_capture_ptr(fulfill_closure, 2, state_arr as i64);
        js_closure_set_capture_f64(fulfill_closure, 3, i as f64);

        // Create reject closure for this promise
        // Captures: [result_promise, state_arr]
        let reject_closure = js_closure_alloc(promise_all_reject_handler as *const u8, 2);
        js_closure_set_capture_ptr(reject_closure, 0, result_promise as i64);
        js_closure_set_capture_ptr(reject_closure, 1, state_arr as i64);

        // Attach handlers to the promise
        js_promise_then(promise_ptr, fulfill_closure, reject_closure);
    }

    // Check if all were non-promises (already resolved)
    let remaining = js_array_get_f64(state_arr, 0);
    if remaining == 0.0 {
        let arr_f64 = crate::value::js_nanbox_pointer(results_arr as i64);
        js_promise_resolve(result_promise, arr_f64);
    }

    result_promise
}

/// Internal handler called when a promise in Promise.all fulfills
extern "C" fn promise_all_fulfill_handler(closure: *const crate::closure::ClosureHeader, value: f64) -> f64 {
    use crate::array::{js_array_get_f64, js_array_set_f64, ArrayHeader};
    use crate::closure::{js_closure_get_capture_ptr, js_closure_get_capture_f64};

    let result_promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let results_arr = js_closure_get_capture_ptr(closure, 1) as *mut ArrayHeader;
    let state_arr = js_closure_get_capture_ptr(closure, 2) as *mut ArrayHeader;
    if result_promise.is_null() || results_arr.is_null() || state_arr.is_null() {
        return 0.0;
    }
    let index = js_closure_get_capture_f64(closure, 3) as u32;

    // Check if already rejected
    let rejected = js_array_get_f64(state_arr, 1);
    if rejected != 0.0 {
        return 0.0;
    }

    // Store the resolved value
    js_array_set_f64(results_arr, index, value);

    // Decrement remaining count
    let remaining = js_array_get_f64(state_arr, 0) - 1.0;
    js_array_set_f64(state_arr, 0, remaining);

    // If all promises have resolved, resolve the result promise with the array
    if remaining == 0.0 {
        let arr_f64 = crate::value::js_nanbox_pointer(results_arr as i64);
        js_promise_resolve(result_promise, arr_f64);
    }

    0.0
}

/// Internal handler called when a promise in Promise.all rejects
extern "C" fn promise_all_reject_handler(closure: *const crate::closure::ClosureHeader, reason: f64) -> f64 {
    use crate::array::{js_array_get_f64, js_array_set_f64, ArrayHeader};
    use crate::closure::js_closure_get_capture_ptr;

    let result_promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let state_arr = js_closure_get_capture_ptr(closure, 1) as *mut ArrayHeader;
    if result_promise.is_null() || state_arr.is_null() {
        return 0.0;
    }

    // Check if already rejected (only reject once)
    let rejected = js_array_get_f64(state_arr, 1);
    if rejected != 0.0 {
        return 0.0;
    }

    // Mark as rejected
    js_array_set_f64(state_arr, 1, 1.0);

    // Reject the result promise with the reason
    js_promise_reject(result_promise, reason);

    0.0
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

    // For each promise, attach resolve/reject handlers that settle the result promise
    for i in 0..count {
        let promise_f64 = js_array_get_f64(promises_arr, i);
        // Discriminate via GC-header obj_type — string/bigint NaN-boxed
        // values would otherwise pass through pointer extraction and crash
        // js_promise_then.
        if js_value_is_promise(promise_f64) == 0 {
            // Non-promise value — resolve immediately with the value
            js_promise_resolve(result_promise, promise_f64);
            return result_promise;
        }
        let promise_ptr = js_nanbox_get_pointer(promise_f64) as *mut Promise;

        // Check if already settled — resolve/reject immediately
        let state = unsafe { (*promise_ptr).state };
        if matches!(state, PromiseState::Fulfilled) {
            js_promise_resolve(result_promise, unsafe { (*promise_ptr).value });
            return result_promise;
        } else if matches!(state, PromiseState::Rejected) {
            js_promise_reject(result_promise, unsafe { (*promise_ptr).reason });
            return result_promise;
        }

        // Create resolve handler closure (captures result_promise)
        let resolve_closure = js_closure_alloc(
            promise_race_resolve_handler as *const u8,
            1, // 1 capture: result_promise
        );
        js_closure_set_capture_ptr(resolve_closure, 0, result_promise as i64);

        // Create reject handler closure (captures result_promise)
        let reject_closure = js_closure_alloc(
            promise_race_reject_handler as *const u8,
            1,
        );
        js_closure_set_capture_ptr(reject_closure, 0, result_promise as i64);

        // Attach handlers via then
        js_promise_then(promise_ptr, resolve_closure, reject_closure);
    }

    result_promise
}

/// Handler for Promise.race fulfill — resolves the race promise with the first value
extern "C" fn promise_race_resolve_handler(closure: *const crate::closure::ClosureHeader, value: f64) -> f64 {
    use crate::closure::js_closure_get_capture_ptr;
    let result_promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    if result_promise.is_null() { return 0.0; }
    // Only settle if still pending (first one wins)
    if matches!(unsafe { (*result_promise).state }, PromiseState::Pending) {
        js_promise_resolve(result_promise, value);
    }
    0.0
}

/// Handler for Promise.race reject — rejects the race promise with the first reason
extern "C" fn promise_race_reject_handler(closure: *const crate::closure::ClosureHeader, reason: f64) -> f64 {
    use crate::closure::js_closure_get_capture_ptr;
    let result_promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    if result_promise.is_null() { return 0.0; }
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

/// Build a `{ status: "fulfilled", value: v }` object for Promise.allSettled.
fn build_settled_fulfilled(value: f64) -> f64 {
    use crate::object::{js_object_alloc_with_shape, js_object_set_field};
    let packed = b"status\0value\0";
    let obj = js_object_alloc_with_shape(0x7FFF_FF10, 2, packed.as_ptr(), packed.len() as u32);
    let status_str = crate::string::js_string_from_bytes(b"fulfilled".as_ptr(), 9);
    let status_nb = crate::value::js_nanbox_string(status_str as i64);
    js_object_set_field(obj, 0, crate::value::JSValue::from_bits(status_nb.to_bits()));
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
    js_object_set_field(obj, 0, crate::value::JSValue::from_bits(status_nb.to_bits()));
    js_object_set_field(obj, 1, crate::value::JSValue::from_bits(reason.to_bits()));
    crate::value::js_nanbox_pointer(obj as i64)
}

/// Promise.allSettled — never rejects; resolves with an array of result objects
/// where each entry is `{ status: "fulfilled", value }` or `{ status: "rejected", reason }`.
#[no_mangle]
pub extern "C" fn js_promise_all_settled(promises_arr: *const crate::array::ArrayHeader) -> *mut Promise {
    use crate::array::{js_array_alloc, js_array_get_f64, js_array_length, js_array_set_f64};
    use crate::closure::{js_closure_alloc, js_closure_set_capture_ptr, js_closure_set_capture_f64};
    use crate::value::js_nanbox_get_pointer;

    let result_promise = js_promise_new();

    if promises_arr.is_null() {
        let empty_arr = js_array_alloc(0);
        unsafe { (*empty_arr).length = 0; }
        let arr_f64 = crate::value::js_nanbox_pointer(empty_arr as i64);
        js_promise_resolve(result_promise, arr_f64);
        return result_promise;
    }

    let count = js_array_length(promises_arr);
    if count == 0 {
        let empty_arr = js_array_alloc(0);
        unsafe { (*empty_arr).length = 0; }
        let arr_f64 = crate::value::js_nanbox_pointer(empty_arr as i64);
        js_promise_resolve(result_promise, arr_f64);
        return result_promise;
    }

    let results_arr = js_array_alloc(count);
    unsafe { (*results_arr).length = count; }
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    for i in 0..count {
        js_array_set_f64(results_arr, i, f64::from_bits(TAG_UNDEFINED));
    }

    // state: [remaining_count]
    let state_arr = js_array_alloc(1);
    unsafe { (*state_arr).length = 1; }
    js_array_set_f64(state_arr, 0, count as f64);

    for i in 0..count {
        let promise_f64 = js_array_get_f64(promises_arr, i);

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

        js_promise_then(promise_ptr, fulfill_closure, reject_closure);
    }

    // If all were already non-promises
    let remaining = js_array_get_f64(state_arr, 0);
    if remaining == 0.0 {
        let arr_f64 = crate::value::js_nanbox_pointer(results_arr as i64);
        js_promise_resolve(result_promise, arr_f64);
    }

    result_promise
}

extern "C" fn promise_all_settled_fulfill_handler(closure: *const crate::closure::ClosureHeader, value: f64) -> f64 {
    use crate::array::{js_array_get_f64, js_array_set_f64, ArrayHeader};
    use crate::closure::{js_closure_get_capture_ptr, js_closure_get_capture_f64};

    let result_promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let results_arr = js_closure_get_capture_ptr(closure, 1) as *mut ArrayHeader;
    let state_arr = js_closure_get_capture_ptr(closure, 2) as *mut ArrayHeader;
    if result_promise.is_null() || results_arr.is_null() || state_arr.is_null() { return 0.0; }
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

extern "C" fn promise_all_settled_reject_handler(closure: *const crate::closure::ClosureHeader, reason: f64) -> f64 {
    use crate::array::{js_array_get_f64, js_array_set_f64, ArrayHeader};
    use crate::closure::{js_closure_get_capture_ptr, js_closure_get_capture_f64};

    let result_promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let results_arr = js_closure_get_capture_ptr(closure, 1) as *mut ArrayHeader;
    let state_arr = js_closure_get_capture_ptr(closure, 2) as *mut ArrayHeader;
    if result_promise.is_null() || results_arr.is_null() || state_arr.is_null() { return 0.0; }
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
/// with an array of rejection reasons (Perry doesn't have AggregateError yet).
#[no_mangle]
pub extern "C" fn js_promise_any(promises_arr: *const crate::array::ArrayHeader) -> *mut Promise {
    use crate::array::{js_array_alloc, js_array_get_f64, js_array_length, js_array_set_f64};
    use crate::closure::{js_closure_alloc, js_closure_set_capture_ptr, js_closure_set_capture_f64};
    use crate::value::js_nanbox_get_pointer;

    let result_promise = js_promise_new();

    if promises_arr.is_null() {
        // Empty input — Promise.any rejects immediately with empty errors array
        let errors_arr = js_array_alloc(0);
        unsafe { (*errors_arr).length = 0; }
        let arr_f64 = crate::value::js_nanbox_pointer(errors_arr as i64);
        js_promise_reject(result_promise, arr_f64);
        return result_promise;
    }

    let count = js_array_length(promises_arr);
    if count == 0 {
        let errors_arr = js_array_alloc(0);
        unsafe { (*errors_arr).length = 0; }
        let arr_f64 = crate::value::js_nanbox_pointer(errors_arr as i64);
        js_promise_reject(result_promise, arr_f64);
        return result_promise;
    }

    let errors_arr = js_array_alloc(count);
    unsafe { (*errors_arr).length = count; }
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    for i in 0..count {
        js_array_set_f64(errors_arr, i, f64::from_bits(TAG_UNDEFINED));
    }

    // state: [remaining_rejections, settled_flag]
    let state_arr = js_array_alloc(2);
    unsafe { (*state_arr).length = 2; }
    js_array_set_f64(state_arr, 0, count as f64);
    js_array_set_f64(state_arr, 1, 0.0);

    for i in 0..count {
        let promise_f64 = js_array_get_f64(promises_arr, i);
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

        let fulfill_closure = js_closure_alloc(promise_any_fulfill_handler as *const u8, 2);
        js_closure_set_capture_ptr(fulfill_closure, 0, result_promise as i64);
        js_closure_set_capture_ptr(fulfill_closure, 1, state_arr as i64);

        let reject_closure = js_closure_alloc(promise_any_reject_handler as *const u8, 4);
        js_closure_set_capture_ptr(reject_closure, 0, result_promise as i64);
        js_closure_set_capture_ptr(reject_closure, 1, errors_arr as i64);
        js_closure_set_capture_ptr(reject_closure, 2, state_arr as i64);
        js_closure_set_capture_f64(reject_closure, 3, i as f64);

        js_promise_then(promise_ptr, fulfill_closure, reject_closure);
    }

    result_promise
}

extern "C" fn promise_any_fulfill_handler(closure: *const crate::closure::ClosureHeader, value: f64) -> f64 {
    use crate::array::{js_array_get_f64, js_array_set_f64, ArrayHeader};
    use crate::closure::js_closure_get_capture_ptr;

    let result_promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let state_arr = js_closure_get_capture_ptr(closure, 1) as *mut ArrayHeader;
    if result_promise.is_null() || state_arr.is_null() { return 0.0; }

    let already_settled = js_array_get_f64(state_arr, 1);
    if already_settled != 0.0 { return 0.0; }
    js_array_set_f64(state_arr, 1, 1.0);

    js_promise_resolve(result_promise, value);
    0.0
}

extern "C" fn promise_any_reject_handler(closure: *const crate::closure::ClosureHeader, reason: f64) -> f64 {
    use crate::array::{js_array_get_f64, js_array_set_f64, ArrayHeader};
    use crate::closure::{js_closure_get_capture_ptr, js_closure_get_capture_f64};

    let result_promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let errors_arr = js_closure_get_capture_ptr(closure, 1) as *mut ArrayHeader;
    let state_arr = js_closure_get_capture_ptr(closure, 2) as *mut ArrayHeader;
    if result_promise.is_null() || errors_arr.is_null() || state_arr.is_null() { return 0.0; }
    let index = js_closure_get_capture_f64(closure, 3) as u32;

    let already_settled = js_array_get_f64(state_arr, 1);
    if already_settled != 0.0 { return 0.0; }

    js_array_set_f64(errors_arr, index, reason);

    let remaining = js_array_get_f64(state_arr, 0) - 1.0;
    js_array_set_f64(state_arr, 0, remaining);

    if remaining == 0.0 {
        // All rejected — create an AggregateError with the collected
        // errors array and reject the result promise with it.
        js_array_set_f64(state_arr, 1, 1.0);
        let msg = crate::string::js_string_from_bytes(
            b"All promises were rejected".as_ptr(),
            26,
        );
        let agg_err = crate::error::js_aggregateerror_new(
            errors_arr,
            msg,
        );
        let err_f64 = crate::value::js_nanbox_pointer(agg_err as i64);
        js_promise_reject(result_promise, err_f64);
    }
    0.0
}

/// GC root scanner: mark all values reachable from promise task queues
pub fn scan_promise_roots(mark: &mut dyn FnMut(f64)) {
    // Scan TASK_QUEUE entries
    TASK_QUEUE.with(|q| {
        let q = q.borrow();
        for &(promise_ptr, value, _) in q.iter() {
            // Mark the promise pointer (NaN-box it as a POINTER)
            if !promise_ptr.is_null() {
                let boxed = f64::from_bits(0x7FFD_0000_0000_0000 | (promise_ptr as u64 & 0x0000_FFFF_FFFF_FFFF));
                mark(boxed);
            }
            // Mark the value
            mark(value);
        }
    });

    // Scan SCHEDULED_RESOLVES entries
    SCHEDULED_RESOLVES.with(|q| {
        let q = q.borrow();
        for &(promise_ptr, value) in q.iter() {
            if !promise_ptr.is_null() {
                let boxed = f64::from_bits(0x7FFD_0000_0000_0000 | (promise_ptr as u64 & 0x0000_FFFF_FFFF_FFFF));
                mark(boxed);
            }
            mark(value);
        }
    });
}

/// Promise.withResolvers<T>() — returns an object with { promise, resolve, reject }.
/// The resolve/reject are closures that settle the promise when called.
#[no_mangle]
pub extern "C" fn js_promise_with_resolvers() -> *mut crate::object::ObjectHeader {
    
    use crate::object::{js_object_alloc_with_shape, ObjectHeader};
    use crate::closure::js_closure_alloc;

    // Create the pending promise.
    let promise = js_promise_new();
    let promise_box = crate::value::js_nanbox_pointer(promise as i64);

    // Create resolve closure that resolves this promise.
    let resolve_fn = js_closure_alloc(
        with_resolvers_resolve_handler as *const u8,
        1, // 1 capture: the promise pointer
    );
    unsafe {
        crate::closure::js_closure_set_capture_f64(resolve_fn, 0, promise_box);
    }
    let resolve_box = crate::value::js_nanbox_pointer(resolve_fn as i64);

    // Create reject closure.
    let reject_fn = js_closure_alloc(
        with_resolvers_reject_handler as *const u8,
        1,
    );
    unsafe {
        crate::closure::js_closure_set_capture_f64(reject_fn, 0, promise_box);
    }
    let reject_box = crate::value::js_nanbox_pointer(reject_fn as i64);

    // Build the { promise, resolve, reject } object.
    // Use a 3-field object with packed keys "promise\0resolve\0reject\0".
    let packed = b"promise\0resolve\0reject\0";
    let obj = js_object_alloc_with_shape(
        0xFFF0_0001, // unique shape id
        3,
        packed.as_ptr(),
        packed.len() as u32,
    );

    // Store the three fields.
    unsafe {
        let fields = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut f64;
        *fields.add(0) = promise_box;  // .promise
        *fields.add(1) = resolve_box;  // .resolve
        *fields.add(2) = reject_box;   // .reject
    }

    obj
}

extern "C" fn with_resolvers_resolve_handler(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    unsafe {
        let promise_box = crate::closure::js_closure_get_capture_f64(closure, 0);
        let promise_ptr = (f64::to_bits(promise_box) & crate::value::POINTER_MASK) as *mut Promise;
        js_promise_resolve(promise_ptr, value);
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn with_resolvers_reject_handler(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    unsafe {
        let promise_box = crate::closure::js_closure_get_capture_f64(closure, 0);
        let promise_ptr = (f64::to_bits(promise_box) & crate::value::POINTER_MASK) as *mut Promise;
        js_promise_reject(promise_ptr, value);
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}
