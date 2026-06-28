//! Thenable assimilation and the `PromiseResolveThenableJob` machinery
//! (ECMA-262 27.2.1.3.2 / 27.2.2.2 — `PromiseResolveThenableJob`).
//!
//! Split out of `combinators.rs` to keep that file under the per-file size
//! gate (#5590). This is the cohesive resolve-with-thenable cluster: the
//! own-`then` classification, `get_then_action`, the microtask-enqueued
//! thenable job and its resolve/reject closures, `promise_resolve_assimilating`
//! (the spec resolve function's assimilation step), and the synchronous
//! `assimilate_via_then_property` helper.

use super::combinators::{
    callable_closure_value, combinator_catch_js, ensure_native_resolving_arity_registered,
    promise_reject_fn, promise_resolve_fn,
};
use super::*;

fn is_native_array_value(value: f64) -> bool {
    let bits = value.to_bits();
    if (bits & crate::value::TAG_MASK) != crate::value::POINTER_TAG {
        return false;
    }
    let ptr = crate::value::js_nanbox_get_pointer(value) as usize;
    if crate::value::addr_class::is_handle_band(ptr) {
        return false;
    }
    // Off-GC-heap header-less typed arrays / buffers are not Arrays and must
    // not be header-probed (#5226).
    if crate::typedarray::is_offheap_sidetable_alloc(ptr) {
        return false;
    }
    unsafe {
        let header = (ptr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        matches!(
            (*header).obj_type,
            crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_LAZY_ARRAY
        )
    }
}

pub(super) fn get_array_prototype_then_action() -> Result<Option<f64>, f64> {
    let array_ctor = crate::object::js_get_global_this_builtin_value(b"Array".as_ptr(), 5);
    if is_definitely_primitive(array_ctor) {
        return Ok(None);
    }
    let array_proto = combinator_catch_js(|| unsafe {
        crate::value::js_dynamic_object_get_property(
            array_ctor,
            b"prototype".as_ptr() as *const i8,
            9,
        )
    })?;
    if is_definitely_primitive(array_proto) {
        return Ok(None);
    }
    let then = combinator_catch_js(|| unsafe {
        crate::value::js_dynamic_object_get_property(array_proto, b"then".as_ptr() as *const i8, 4)
    })?;
    Ok(callable_closure_value(then).map(|_| then))
}

/// #5590: classification of a native Promise resolution's *own* `then` property
/// (`thenable.then = …` or `Object.defineProperty(p, "then", …)`). Per ECMA-262
/// 27.2.1.3.2 the resolve function reads `then = Get(resolution, "then")` and
/// branches on `IsCallable(then)` — even when `resolution` is a genuine promise,
/// where Perry would otherwise take its native promise→promise wiring.
enum OwnThen {
    /// No own `then` — the promise's `then` is the intrinsic; the native
    /// promise→promise fast-path is valid.
    None,
    /// An own `then` exists but is NOT callable — `IsCallable(then)` is false, so
    /// the promise is fulfilled with `resolution` directly (FulfillPromise); it
    /// must NOT adopt the inner promise's eventual state
    /// (test262 `resolve-prms-cstm-then*` with a non-function `then`).
    NonCallable,
    /// A callable own `then` override — assimilate via `PromiseResolveThenableJob`
    /// with this `then` action (test262 `resolve-*-prms-cstm-then*`).
    Callable(f64),
}

/// Classify a native Promise resolution's own `then` (see [`OwnThen`]).
fn promise_own_then(value: f64) -> OwnThen {
    let bits = value.to_bits();
    if (bits & crate::value::TAG_MASK) != crate::value::POINTER_TAG {
        return OwnThen::None;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    let then = unsafe {
        crate::object::exotic_expando::exotic_get_own_property(
            addr,
            crate::object::exotic_expando::ExoticKind::Promise,
            "then",
            value,
        )
    };
    match then {
        None => OwnThen::None,
        Some(t) if callable_closure_value(t).is_some() => OwnThen::Callable(t),
        Some(_) => OwnThen::NonCallable,
    }
}

pub(super) fn get_then_action(value: f64) -> Result<Option<f64>, f64> {
    if is_definitely_primitive(value) {
        return Ok(None);
    }
    let then = combinator_catch_js(|| unsafe {
        crate::value::js_dynamic_object_get_property(value, b"then".as_ptr() as *const i8, 4)
    })?;
    if callable_closure_value(then).is_some() {
        return Ok(Some(then));
    }
    if is_native_array_value(value) {
        return get_array_prototype_then_action();
    }
    Ok(None)
}

pub(super) fn enqueue_thenable_job(promise: *mut Promise, thenable: f64, then_action: f64) {
    use crate::closure::{
        js_closure_alloc, js_closure_set_capture_f64, js_closure_set_capture_ptr,
    };

    let callback = js_closure_alloc(promise_resolve_thenable_job as *const u8, 3);
    js_closure_set_capture_ptr(callback, 0, promise as i64);
    js_closure_set_capture_f64(callback, 1, thenable);
    js_closure_set_capture_f64(callback, 2, then_action);

    let context = capture_context();
    let ids = crate::async_hooks::init_resource(
        "PromiseResolveThenableJob",
        f64::from_bits(crate::value::TAG_UNDEFINED),
        false,
    );
    TASK_QUEUE.with(|q| {
        q.borrow_mut().push_back(Task::Microtask {
            callback,
            context,
            async_id: ids.async_id,
            trigger_async_id: ids.trigger_async_id,
        });
    });
    crate::event_pump::js_notify_main_thread();
}

pub(crate) fn promise_resolve_assimilating(promise: *mut Promise, value: f64) {
    if promise.is_null() {
        return;
    }
    unsafe {
        if (*promise).state != PromiseState::Pending {
            return;
        }
    }

    let value = adapt_foreign_promise_value(value);
    if js_value_is_promise(value) != 0 {
        // #5590: branch on a user-installed own `then` before the native
        // promise→promise fast-path (spec reads `Get(resolution, "then")` and
        // switches on `IsCallable(then)`).
        match promise_own_then(value) {
            // Callable override: assimilate via PromiseResolveThenableJob.
            OwnThen::Callable(then_action) => {
                enqueue_thenable_job(promise, value, then_action);
                return;
            }
            // Present but non-callable: `IsCallable(then)` is false → fulfill
            // with the resolution VALUE directly, do NOT adopt the inner promise.
            OwnThen::NonCallable => {
                js_promise_resolve(promise, value);
                return;
            }
            // No own `then` → intrinsic `then`; native promise→promise wiring.
            OwnThen::None => {
                let inner = crate::value::js_nanbox_get_pointer(value) as *mut Promise;
                js_promise_resolve_with_promise(promise, inner);
                return;
            }
        }
    }

    match get_then_action(value) {
        Ok(Some(then_action)) => enqueue_thenable_job(promise, value, then_action),
        Ok(None) => js_promise_resolve(promise, value),
        Err(reason) => js_promise_reject(promise, reason),
    }
}

#[inline]
fn thenable_job_take_guard(guard_arr: *mut crate::array::ArrayHeader) -> bool {
    use crate::array::{js_array_get_f64, js_array_set_f64};

    if guard_arr.is_null() {
        return false;
    }
    if js_array_get_f64(guard_arr, 0) != 0.0 {
        return false;
    }
    js_array_set_f64(guard_arr, 0, 1.0);
    true
}

pub(super) extern "C" fn thenable_job_resolve_fn(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    use crate::closure::js_closure_get_capture_ptr;

    let promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let guard_arr = js_closure_get_capture_ptr(closure, 1) as *mut crate::array::ArrayHeader;
    if thenable_job_take_guard(guard_arr) {
        promise_resolve_assimilating(promise, value);
    }
    0.0
}

pub(super) extern "C" fn thenable_job_reject_fn(
    closure: *const crate::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    use crate::closure::js_closure_get_capture_ptr;

    let promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let guard_arr = js_closure_get_capture_ptr(closure, 1) as *mut crate::array::ArrayHeader;
    if thenable_job_take_guard(guard_arr) {
        js_promise_reject(promise, reason);
    }
    0.0
}

extern "C" fn promise_resolve_thenable_job(closure: *const crate::closure::ClosureHeader) -> f64 {
    use crate::array::{js_array_alloc, js_array_set_f64};
    use crate::closure::{
        js_closure_alloc, js_closure_get_capture_f64, js_closure_get_capture_ptr,
        js_closure_set_capture_ptr,
    };

    let promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    if promise.is_null() {
        return 0.0;
    }
    let thenable = js_closure_get_capture_f64(closure, 1);
    let then_action = js_closure_get_capture_f64(closure, 2);
    if callable_closure_value(then_action).is_none() {
        js_promise_resolve(promise, thenable);
        return 0.0;
    }
    // The resolve/reject closures below may be invoked with zero arguments by
    // the thenable's `then`; ensure their dispatch arity is registered so the
    // missing value pads to `undefined`.
    ensure_native_resolving_arity_registered();

    let guard_arr = js_array_alloc(1);
    unsafe {
        (*guard_arr).length = 1;
    }
    js_array_set_f64(guard_arr, 0, 0.0);

    let resolve_closure = js_closure_alloc(thenable_job_resolve_fn as *const u8, 2);
    js_closure_set_capture_ptr(resolve_closure, 0, promise as i64);
    js_closure_set_capture_ptr(resolve_closure, 1, guard_arr as i64);
    let reject_closure = js_closure_alloc(thenable_job_reject_fn as *const u8, 2);
    js_closure_set_capture_ptr(reject_closure, 0, promise as i64);
    js_closure_set_capture_ptr(reject_closure, 1, guard_arr as i64);

    let resolve_value = crate::value::js_nanbox_pointer(resolve_closure as i64);
    let reject_value = crate::value::js_nanbox_pointer(reject_closure as i64);
    let args = [resolve_value, reject_value];

    let prev_this = crate::object::js_implicit_this_set(thenable);
    let result = combinator_catch_js(|| unsafe {
        crate::closure::js_native_call_value(then_action, args.as_ptr(), args.len())
    });
    crate::object::js_implicit_this_set(prev_this);
    if let Err(reason) = result {
        if thenable_job_take_guard(guard_arr) {
            js_promise_reject(promise, reason);
        }
    }
    0.0
}

/// allocates a wrapper promise and runs `value.then(resolve, reject)` to follow
/// its eventual state. Returns the wrapper promise, or — when `then` is absent
/// or not callable — the original `value` unchanged (resolve-plain).
pub(super) fn assimilate_via_then_property(value: f64) -> f64 {
    // `Get(value, "then")` (27.2.1.3.2 step 8). A throwing getter is an abrupt
    // completion → resolve-with-thenable rejects the wrapper promise with the
    // thrown value (step 9), rather than letting the exception unwind out of the
    // resolve path. Return that rejected wrapper so callers chain it.
    let then_val = match combinator_catch_js(|| unsafe {
        crate::value::js_dynamic_object_get_property(value, b"then".as_ptr() as *const i8, 4)
    }) {
        Ok(v) => v,
        Err(reason) => {
            let p = js_promise_new();
            js_promise_reject(p, reason);
            return crate::value::js_nanbox_pointer(p as i64);
        }
    };
    if callable_closure_value(then_val).is_none() {
        return value;
    }

    let new_promise = js_promise_new();
    let promise_i64 = new_promise as i64;

    let resolve_closure = crate::closure::js_closure_alloc(promise_resolve_fn as *const u8, 1);
    crate::closure::js_closure_set_capture_ptr(resolve_closure, 0, promise_i64);
    let reject_closure = crate::closure::js_closure_alloc(promise_reject_fn as *const u8, 1);
    crate::closure::js_closure_set_capture_ptr(reject_closure, 0, promise_i64);

    // Pass the resolving functions as proper NaN-boxed function values (not the
    // raw closure-pointer-bits convention used internally by
    // `js_promise_new_with_executor`): a thenable's `then(onFulfilled,
    // onRejected)` is a USER-visible call, and spec/Node hand it real functions —
    // so `typeof onFulfilled === "function"` must hold (test262
    // yield-star-async-* / yield-star-next-then-* check this). A NaN-boxed
    // closure is still invoked through the normal call path.
    let resolve_f64 = crate::value::js_nanbox_pointer(resolve_closure as i64);
    let reject_f64 = crate::value::js_nanbox_pointer(reject_closure as i64);
    let args = [resolve_f64, reject_f64];

    // Bind `this` to the thenable so a non-arrow `then` body reads the right
    // receiver, then call `Get(value, "then")` as a value (own data property).
    let prev = crate::object::js_implicit_this_set(value);
    unsafe {
        crate::closure::js_native_call_value(then_val, args.as_ptr(), args.len());
    }
    crate::object::js_implicit_this_set(prev);

    crate::value::js_nanbox_pointer(new_promise as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::js_nanbox_pointer;

    #[test]
    fn resolve_with_promise_having_noncallable_own_then_fulfills_with_value() {
        // #5590: resolving a promise with a native promise that carries an own
        // NON-callable `then` (`p.then = 123`) must fulfill with that promise
        // VALUE directly — `IsCallable(then)` is false, so FulfillPromise runs
        // and the inner promise's eventual state is NOT adopted. A callable own
        // `then` is classified for the thenable-job path; no own `then` keeps the
        // native promise->promise wiring.
        unsafe {
            use crate::object::exotic_expando::{exotic_set_property, ExoticKind};
            TASK_QUEUE.with(|q| q.borrow_mut().clear());

            // Inner promise left PENDING: if its state were adopted (the bug),
            // `outer` would also stay pending; fulfilling with the value settles
            // `outer` synchronously, which is the discriminating signal.
            let inner = js_promise_new();
            let inner_val = js_nanbox_pointer(inner as i64);
            let inner_addr = (inner_val.to_bits() & crate::value::POINTER_MASK) as usize;

            // No own `then` yet -> intrinsic, native wiring.
            assert!(matches!(promise_own_then(inner_val), OwnThen::None));

            // Install an own, NON-callable `then` (a plain number).
            assert!(exotic_set_property(
                inner_addr,
                ExoticKind::Promise,
                "then",
                123.0,
                inner_val,
            ));
            assert!(matches!(promise_own_then(inner_val), OwnThen::NonCallable));

            let outer = js_promise_new();
            promise_resolve_assimilating(outer, inner_val);

            assert_eq!((*outer).state, PromiseState::Fulfilled);
            assert_eq!((*outer).value.to_bits(), inner_val.to_bits());
        }
    }
}
