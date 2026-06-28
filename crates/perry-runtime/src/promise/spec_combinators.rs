//! Spec-compliant `Promise.all` / `Promise.allSettled` / `Promise.race` /
//! `Promise.any` (ECMA-262 27.2.4.1–27.2.4.x).
//!
//! Unlike the simplified native path in `combinators.rs`, this implementation
//! follows the observable algorithm:
//!
//!   * `NewPromiseCapability(C)` — constructs `new C(executor)` with the
//!     `GetCapabilitiesExecutor` "called twice / not callable" `TypeError`s and
//!     the `IsConstructor(C)` guard.
//!   * `GetPromiseResolve(C)` — reads `C.resolve` once and requires it callable.
//!   * Per element: `nextPromise = Call(promiseResolve, C, «next»)`, then
//!     `Invoke(nextPromise, "then", «resolveElement, reject»)` with REAL
//!     resolve-element closures carrying the `[[AlreadyCalled]]`, `[[Index]]`,
//!     `[[Values]]`, `[[Capability]]`, `[[RemainingElements]]` slots.
//!
//! These observable steps are required by test262 (`built-ins/Promise/all/*`,
//! `allSettled/*`, `race/*`, `any/*`) — the per-element `this.resolve`
//! invocations, the `.then` invocations, and the resolve-element call counts
//! are all asserted.

use super::combinators::{combinator_catch_js, combinator_iterable_to_array_caught};
use super::*;
use crate::array::{js_array_alloc, js_array_get_f64, js_array_set_f64};
use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_get_capture_ptr,
    js_closure_set_capture_f64, js_closure_set_capture_ptr,
};
use crate::value::{js_nanbox_pointer, JSValue};

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

#[inline]
fn undef() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

#[inline]
fn is_undef(v: f64) -> bool {
    v.to_bits() == TAG_UNDEFINED
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CombinatorKind {
    All,
    AllSettled,
    Race,
    Any,
}

/// A spec `PromiseCapability` record: the constructed promise plus its
/// resolving functions, all as NaN-boxed JS values.
pub(super) struct Capability {
    pub(super) promise: f64,
    pub(super) resolve: f64,
    pub(super) reject: f64,
}

// ---------------------------------------------------------------------------
// Arity registration so closures invoked with fewer args than declared pad the
// missing slots with `undefined` (rather than reading uninitialised registers).
// ---------------------------------------------------------------------------

thread_local! {
    static ARITY_REGISTERED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn ensure_arity_registered() {
    ARITY_REGISTERED.with(|done| {
        if done.get() {
            return;
        }
        done.set(true);
        crate::closure::js_register_closure_arity(capability_executor_fn as *const u8, 2);
        crate::closure::js_register_closure_arity(all_resolve_element_fn as *const u8, 1);
        crate::closure::js_register_closure_arity(settled_fulfill_element_fn as *const u8, 1);
        crate::closure::js_register_closure_arity(settled_reject_element_fn as *const u8, 1);
        crate::closure::js_register_closure_arity(any_reject_element_fn as *const u8, 1);
    });
}

// ---------------------------------------------------------------------------
// TypeError helpers
// ---------------------------------------------------------------------------

fn throw_type_error(msg: &str) -> ! {
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    let v = f64::from_bits(JSValue::pointer(err as *const u8).bits());
    crate::exception::js_throw(v);
}

fn type_error_value(msg: &str) -> f64 {
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    f64::from_bits(JSValue::pointer(err as *const u8).bits())
}

/// Spec `IsCallable` for our purposes — a JS function value.
fn is_callable(value: f64) -> bool {
    // Tag-safe: the capability storage can hold ARBITRARY user values (numbers,
    // strings, objects passed to a user `executor(resolve, reject)`), so we must
    // not hand a bare number to `is_closure_ptr` (which dereferences the payload
    // at `ptr+12`). Two callable encodings exist:
    //   * NaN-boxed POINTER_TAG closures (user functions, our reified statics).
    //   * RAW pointer-bits closures — `js_promise_new_with_executor` passes the
    //     native resolve/reject as `f64::from_bits(closure_ptr)`, i.e. the bits
    //     ARE a heap address, outside the NaN range.
    // A finite JS number's bits-as-address always lands at/above 2^48 (the
    // exponent/sign bits are set), so an upper bound on the candidate address
    // cleanly separates real heap pointers from numbers.
    const HEAP_ADDR_CEILING: u64 = 0x0001_0000_0000_0000; // 2^48
    let bits = value.to_bits();
    let tag = bits & crate::value::TAG_MASK;
    let raw = if tag == crate::value::POINTER_TAG {
        (bits & crate::value::POINTER_MASK) as usize
    } else if bits < HEAP_ADDR_CEILING {
        bits as usize
    } else {
        return false;
    };
    if crate::value::addr_class::is_handle_band(raw) || (raw as u64) >= HEAP_ADDR_CEILING {
        return false;
    }
    crate::closure::is_closure_ptr(raw) || crate::proxy::js_proxy_is_proxy(value) == 1
}

// ---------------------------------------------------------------------------
// NewPromiseCapability(C)
// ---------------------------------------------------------------------------

/// `GetCapabilitiesExecutor` function. Captures the 2-slot capability storage
/// array (slot0 = resolve, slot1 = reject, both init `undefined`). Throws a
/// TypeError if either slot is already set (executor called twice).
extern "C" fn capability_executor_fn(
    closure: *const crate::closure::ClosureHeader,
    resolve: f64,
    reject: f64,
) -> f64 {
    let storage = js_closure_get_capture_ptr(closure, 0) as *mut crate::array::ArrayHeader;
    if storage.is_null() {
        return undef();
    }
    let cur_resolve = js_array_get_f64(storage, 0);
    if !is_undef(cur_resolve) {
        throw_type_error("Promise resolve or reject function is not callable");
    }
    let cur_reject = js_array_get_f64(storage, 1);
    if !is_undef(cur_reject) {
        throw_type_error("Promise resolve or reject function is not callable");
    }
    js_array_set_f64(storage, 0, resolve);
    js_array_set_f64(storage, 1, reject);
    undef()
}

/// `NewPromiseCapability(C)`. Throws (via `js_throw`) on the `IsConstructor`
/// failure, the executor "already called" failure (propagated from the
/// constructor), and the post-construct "resolve/reject not callable" failure.
pub(super) fn new_promise_capability(c: f64) -> Capability {
    if !crate::object::js_value_is_constructor(c) {
        throw_type_error("Promise.all called on non-constructor");
    }

    // Fast path: the intrinsic `Promise` constructor. Build a native capability
    // directly (the generic construct path does not model `new Promise`).
    if is_default_promise_constructor(c) {
        let promise = js_promise_new();
        let (resolve, reject) = super::combinators::make_resolving_functions(promise);
        crate::object::set_builtin_closure_length(resolve as usize, 1);
        crate::object::set_builtin_closure_length(reject as usize, 1);
        return Capability {
            promise: js_nanbox_pointer(promise as i64),
            resolve: js_nanbox_pointer(resolve as i64),
            reject: js_nanbox_pointer(reject as i64),
        };
    }

    // Generic path: `new C(executor)`.
    let storage = js_array_alloc(2);
    unsafe {
        (*storage).length = 2;
    }
    js_array_set_f64(storage, 0, undef());
    js_array_set_f64(storage, 1, undef());

    let executor = js_closure_alloc(capability_executor_fn as *const u8, 1);
    js_closure_set_capture_ptr(executor, 0, storage as i64);
    crate::object::set_builtin_closure_length(executor as usize, 2);

    let executor_val = js_nanbox_pointer(executor as i64);
    let args = [executor_val];
    // Any exception thrown by the constructor (including the executor's
    // "called twice" TypeError) propagates as a real throw — correct for the
    // `? NewPromiseCapability(C)` step in Promise.all.
    let promise = unsafe { crate::object::js_new_function_construct(c, args.as_ptr(), args.len()) };

    let resolve = js_array_get_f64(storage, 0);
    let reject = js_array_get_f64(storage, 1);
    if !is_callable(resolve) || !is_callable(reject) {
        throw_type_error("Promise resolve or reject function is not callable");
    }

    Capability {
        promise,
        resolve,
        reject,
    }
}

pub(super) fn is_default_promise_constructor(c: f64) -> bool {
    let promise_ctor =
        crate::object::js_get_global_this_builtin_value(b"Promise".as_ptr(), b"Promise".len());
    !is_undef(promise_ctor) && promise_ctor.to_bits() == c.to_bits()
}

pub(super) fn is_callable_value(value: f64) -> bool {
    is_callable(value)
}

// ---------------------------------------------------------------------------
// GetPromiseResolve(C) + Call/Invoke helpers
// ---------------------------------------------------------------------------

/// `GetPromiseResolve(C)` = `? Get(C, "resolve")`, require callable. Returns the
/// `resolve` function value on success, or `Err(thrown)` (a TypeError or the
/// getter's own throw) to be funnelled through `IfAbruptRejectPromise`.
fn get_promise_resolve(c: f64) -> Result<f64, f64> {
    let resolve = combinator_catch_js(|| unsafe {
        crate::value::js_dynamic_object_get_property(c, b"resolve".as_ptr() as *const i8, 7)
    })?;
    if !is_callable(resolve) {
        return Err(type_error_value("Promise.resolve is not a function"));
    }
    Ok(resolve)
}

/// `Call(func, thisArg, args)` catching exceptions into `Err`.
fn call_with_this(func: f64, this_arg: f64, args: &[f64]) -> Result<f64, f64> {
    let (ptr, len) = if args.is_empty() {
        (std::ptr::null(), 0)
    } else {
        (args.as_ptr(), args.len())
    };
    let prev = crate::object::js_implicit_this_set(this_arg);
    let result =
        combinator_catch_js(|| unsafe { crate::closure::js_native_call_value(func, ptr, len) });
    crate::object::js_implicit_this_set(prev);
    result
}

/// `Invoke(obj, "then", args)` catching exceptions into `Err`.
fn invoke_then(obj: f64, args: &[f64]) -> Result<f64, f64> {
    let (ptr, len) = if args.is_empty() {
        (std::ptr::null(), 0)
    } else {
        (args.as_ptr(), args.len())
    };
    combinator_catch_js(|| unsafe {
        crate::object::js_native_call_method(obj, b"then".as_ptr() as *const i8, 4, ptr, len)
    })
}

// ---------------------------------------------------------------------------
// Resolve-element closures
// ---------------------------------------------------------------------------
//
// Shared capture layout for the per-element functions:
//   slot 0: already_called guard array ([0] = 0/1)   (per element)
//   slot 1: index (f64)
//   slot 2: values/errors array (shared)
//   slot 3: remaining-count state array ([0] = count) (shared)
//   slot 4: capability resolve (f64) (shared)
//   slot 5: capability reject (f64) (shared)   [allSettled/any use one of these]

#[inline]
fn take_already_called(guard: *mut crate::array::ArrayHeader) -> bool {
    if guard.is_null() {
        return false;
    }
    if js_array_get_f64(guard, 0) != 0.0 {
        return false;
    }
    js_array_set_f64(guard, 0, 1.0);
    true
}

#[inline]
fn build_element_closure(
    func: *const u8,
    guard: *mut crate::array::ArrayHeader,
    index: u32,
    values: *mut crate::array::ArrayHeader,
    state: *mut crate::array::ArrayHeader,
    cap_resolve: f64,
    cap_reject: f64,
) -> *mut crate::closure::ClosureHeader {
    let c = js_closure_alloc(func, 6);
    js_closure_set_capture_ptr(c, 0, guard as i64);
    js_closure_set_capture_f64(c, 1, index as f64);
    js_closure_set_capture_ptr(c, 2, values as i64);
    js_closure_set_capture_ptr(c, 3, state as i64);
    js_closure_set_capture_f64(c, 4, cap_resolve);
    js_closure_set_capture_f64(c, 5, cap_reject);
    crate::object::set_builtin_closure_length(c as usize, 1);
    // Spec: the resolve/reject element functions are anonymous built-in
    // functions and are NOT constructors — `new resolveElement()` throws.
    crate::object::set_builtin_closure_non_constructable(c as usize);
    c
}

/// Decrement the shared remaining-count; return true if it just hit zero.
#[inline]
fn dec_remaining(state: *mut crate::array::ArrayHeader) -> bool {
    let remaining = js_array_get_f64(state, 0) - 1.0;
    js_array_set_f64(state, 0, remaining);
    remaining == 0.0
}

/// Promise.all Resolve Element Function.
extern "C" fn all_resolve_element_fn(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let guard = js_closure_get_capture_ptr(closure, 0) as *mut crate::array::ArrayHeader;
    if !take_already_called(guard) {
        return undef();
    }
    let index = js_closure_get_capture_f64(closure, 1) as u32;
    let values = js_closure_get_capture_ptr(closure, 2) as *mut crate::array::ArrayHeader;
    let state = js_closure_get_capture_ptr(closure, 3) as *mut crate::array::ArrayHeader;
    let cap_resolve = js_closure_get_capture_f64(closure, 4);

    js_array_set_f64(values, index, value);
    if dec_remaining(state) {
        let arr = js_nanbox_pointer(values as i64);
        let _ = call_with_this(cap_resolve, undef(), &[arr]);
    }
    undef()
}

/// Promise.allSettled Resolve Element Function → `{status:"fulfilled", value}`.
extern "C" fn settled_fulfill_element_fn(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let guard = js_closure_get_capture_ptr(closure, 0) as *mut crate::array::ArrayHeader;
    if !take_already_called(guard) {
        return undef();
    }
    let index = js_closure_get_capture_f64(closure, 1) as u32;
    let values = js_closure_get_capture_ptr(closure, 2) as *mut crate::array::ArrayHeader;
    let state = js_closure_get_capture_ptr(closure, 3) as *mut crate::array::ArrayHeader;
    let cap_resolve = js_closure_get_capture_f64(closure, 4);

    js_array_set_f64(values, index, build_settled_fulfilled(value));
    if dec_remaining(state) {
        let arr = js_nanbox_pointer(values as i64);
        let _ = call_with_this(cap_resolve, undef(), &[arr]);
    }
    undef()
}

/// Promise.allSettled Reject Element Function → `{status:"rejected", reason}`.
extern "C" fn settled_reject_element_fn(
    closure: *const crate::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    let guard = js_closure_get_capture_ptr(closure, 0) as *mut crate::array::ArrayHeader;
    if !take_already_called(guard) {
        return undef();
    }
    let index = js_closure_get_capture_f64(closure, 1) as u32;
    let values = js_closure_get_capture_ptr(closure, 2) as *mut crate::array::ArrayHeader;
    let state = js_closure_get_capture_ptr(closure, 3) as *mut crate::array::ArrayHeader;
    let cap_resolve = js_closure_get_capture_f64(closure, 4);

    js_array_set_f64(values, index, build_settled_rejected(reason));
    if dec_remaining(state) {
        let arr = js_nanbox_pointer(values as i64);
        let _ = call_with_this(cap_resolve, undef(), &[arr]);
    }
    undef()
}

/// Promise.any Reject Element Function → collect into errors array; reject with
/// AggregateError once all reject.
extern "C" fn any_reject_element_fn(
    closure: *const crate::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    let guard = js_closure_get_capture_ptr(closure, 0) as *mut crate::array::ArrayHeader;
    if !take_already_called(guard) {
        return undef();
    }
    let index = js_closure_get_capture_f64(closure, 1) as u32;
    let errors = js_closure_get_capture_ptr(closure, 2) as *mut crate::array::ArrayHeader;
    let state = js_closure_get_capture_ptr(closure, 3) as *mut crate::array::ArrayHeader;
    let cap_reject = js_closure_get_capture_f64(closure, 5);

    js_array_set_f64(errors, index, reason);
    if dec_remaining(state) {
        let msg = crate::string::js_string_from_bytes(b"All promises were rejected".as_ptr(), 26);
        let agg = crate::error::js_aggregateerror_new(errors, msg);
        let agg_v = js_nanbox_pointer(agg as i64);
        let _ = call_with_this(cap_reject, undef(), &[agg_v]);
    }
    undef()
}

fn build_settled_fulfilled(value: f64) -> f64 {
    use crate::object::{js_object_alloc_with_shape, js_object_set_field};
    let packed = b"status\0value\0";
    let obj = js_object_alloc_with_shape(0x7FFF_FF10, 2, packed.as_ptr(), packed.len() as u32);
    let status = crate::string::js_string_from_bytes(b"fulfilled".as_ptr(), 9);
    js_object_set_field(
        obj,
        0,
        JSValue::from_bits(crate::value::js_nanbox_string(status as i64).to_bits()),
    );
    js_object_set_field(obj, 1, JSValue::from_bits(value.to_bits()));
    js_nanbox_pointer(obj as i64)
}

fn build_settled_rejected(reason: f64) -> f64 {
    use crate::object::{js_object_alloc_with_shape, js_object_set_field};
    let packed = b"status\0reason\0";
    let obj = js_object_alloc_with_shape(0x7FFF_FF11, 2, packed.as_ptr(), packed.len() as u32);
    let status = crate::string::js_string_from_bytes(b"rejected".as_ptr(), 8);
    js_object_set_field(
        obj,
        0,
        JSValue::from_bits(crate::value::js_nanbox_string(status as i64).to_bits()),
    );
    js_object_set_field(obj, 1, JSValue::from_bits(reason.to_bits()));
    js_nanbox_pointer(obj as i64)
}

// ---------------------------------------------------------------------------
// PerformPromise{All,AllSettled,Race,Any}
// ---------------------------------------------------------------------------

/// Run the per-element loop. `cap` is the result capability, `promise_resolve`
/// the `C.resolve` function, `elements` the snapshot of iterated values.
/// Returns `Ok(())` on success or `Err(thrown)` for `IfAbruptRejectPromise`.
fn perform(
    kind: CombinatorKind,
    c: f64,
    cap: &Capability,
    promise_resolve: f64,
    elements: *mut crate::array::ArrayHeader,
) -> Result<(), f64> {
    let count = unsafe { (*elements).length };

    // Shared state: remaining-count (init 1, spec's remainingElementsCount).
    let state = js_array_alloc(1);
    unsafe {
        (*state).length = 1;
    }
    js_array_set_f64(state, 0, 1.0);

    // Shared values/errors array (not used by Race).
    let values = if kind == CombinatorKind::Race {
        std::ptr::null_mut()
    } else {
        let v = js_array_alloc(count.max(1));
        unsafe {
            (*v).length = count;
        }
        for i in 0..count {
            js_array_set_f64(v, i, undef());
        }
        v
    };

    for i in 0..count {
        let next = js_array_get_f64(elements, i);
        // nextPromise = ? Call(promiseResolve, C, «next»)
        let next_promise = call_with_this(promise_resolve, c, &[next])?;

        match kind {
            CombinatorKind::All => {
                let guard = new_guard();
                let elem = build_element_closure(
                    all_resolve_element_fn as *const u8,
                    guard,
                    i,
                    values,
                    state,
                    cap.resolve,
                    cap.reject,
                );
                js_array_set_f64(state, 0, js_array_get_f64(state, 0) + 1.0);
                invoke_then(next_promise, &[js_nanbox_pointer(elem as i64), cap.reject])?;
            }
            CombinatorKind::AllSettled => {
                let guard = new_guard();
                let on_ful = build_element_closure(
                    settled_fulfill_element_fn as *const u8,
                    guard,
                    i,
                    values,
                    state,
                    cap.resolve,
                    cap.reject,
                );
                let on_rej = build_element_closure(
                    settled_reject_element_fn as *const u8,
                    guard,
                    i,
                    values,
                    state,
                    cap.resolve,
                    cap.reject,
                );
                js_array_set_f64(state, 0, js_array_get_f64(state, 0) + 1.0);
                invoke_then(
                    next_promise,
                    &[
                        js_nanbox_pointer(on_ful as i64),
                        js_nanbox_pointer(on_rej as i64),
                    ],
                )?;
            }
            CombinatorKind::Any => {
                let guard = new_guard();
                let on_rej = build_element_closure(
                    any_reject_element_fn as *const u8,
                    guard,
                    i,
                    values,
                    state,
                    cap.resolve,
                    cap.reject,
                );
                js_array_set_f64(state, 0, js_array_get_f64(state, 0) + 1.0);
                invoke_then(
                    next_promise,
                    &[cap.resolve, js_nanbox_pointer(on_rej as i64)],
                )?;
            }
            CombinatorKind::Race => {
                invoke_then(next_promise, &[cap.resolve, cap.reject])?;
            }
        }
    }

    // Iterator exhausted: remainingElementsCount -= 1.
    if kind != CombinatorKind::Race {
        let remaining = js_array_get_f64(state, 0) - 1.0;
        js_array_set_f64(state, 0, remaining);
        if remaining == 0.0 {
            match kind {
                CombinatorKind::All | CombinatorKind::AllSettled => {
                    let arr = js_nanbox_pointer(values as i64);
                    call_with_this(cap.resolve, undef(), &[arr])?;
                }
                CombinatorKind::Any => {
                    let msg = crate::string::js_string_from_bytes(
                        b"All promises were rejected".as_ptr(),
                        26,
                    );
                    let agg = crate::error::js_aggregateerror_new(values, msg);
                    let agg_v = js_nanbox_pointer(agg as i64);
                    call_with_this(cap.reject, undef(), &[agg_v])?;
                }
                CombinatorKind::Race => unreachable!(),
            }
        }
    }

    Ok(())
}

fn new_guard() -> *mut crate::array::ArrayHeader {
    let g = js_array_alloc(1);
    unsafe {
        (*g).length = 1;
    }
    js_array_set_f64(g, 0, 0.0);
    g
}

// ---------------------------------------------------------------------------
// Top-level entry: Promise.<combinator>(iterable) with `this` = C
// ---------------------------------------------------------------------------

/// Spec entry for a Promise combinator. `c` is the `this` constructor; throws
/// synchronously for `NewPromiseCapability` failures, otherwise returns the
/// (possibly already-rejected) result promise.
pub fn run_combinator(kind: CombinatorKind, c: f64, iterable: f64) -> f64 {
    ensure_arity_registered();

    // 2. Let promiseCapability be ? NewPromiseCapability(C). (may throw)
    let cap = new_promise_capability(c);

    // 3-8. The remaining steps are IfAbruptRejectPromise-guarded.
    let result: Result<(), f64> = (|| {
        let promise_resolve = get_promise_resolve(c)?;
        let elements = combinator_iterable_to_array_caught(iterable)?;
        perform(kind, c, &cap, promise_resolve, elements)
    })();

    if let Err(reason) = result {
        let _ = call_with_this(cap.reject, undef(), &[reason]);
    }
    cap.promise
}

// ---------------------------------------------------------------------------
// Public C-ABI entries (constructor-aware). Used by codegen direct-call path
// (C = intrinsic Promise) and by the reified static thunks (C = implicit this).
// ---------------------------------------------------------------------------

/// The intrinsic `Promise` constructor value — `this` for the codegen
/// direct-call path (`Promise.all([...])`).
pub(super) fn default_promise_ctor() -> f64 {
    crate::object::js_get_global_this_builtin_value(b"Promise".as_ptr(), b"Promise".len())
}

// NOTE: `this_ctor` is passed through verbatim — `undefined`/`null`/primitive
// `this` (e.g. `Promise.all.call(undefined, [])`) must reach
// `NewPromiseCapability` and throw a TypeError, NOT silently default to the
// intrinsic Promise. The codegen direct-call path supplies the real Promise
// constructor via the `*_iterable` entries in `combinators.rs`.

#[no_mangle]
pub extern "C" fn js_promise_all_spec(this_ctor: f64, iterable: f64) -> f64 {
    run_combinator(CombinatorKind::All, this_ctor, iterable)
}

#[no_mangle]
pub extern "C" fn js_promise_all_settled_spec(this_ctor: f64, iterable: f64) -> f64 {
    run_combinator(CombinatorKind::AllSettled, this_ctor, iterable)
}

#[no_mangle]
pub extern "C" fn js_promise_race_spec(this_ctor: f64, iterable: f64) -> f64 {
    run_combinator(CombinatorKind::Race, this_ctor, iterable)
}

#[no_mangle]
pub extern "C" fn js_promise_any_spec(this_ctor: f64, iterable: f64) -> f64 {
    run_combinator(CombinatorKind::Any, this_ctor, iterable)
}

/// Spec `IsObject` — true only for heap object/function values, NOT primitives.
/// Symbols are NaN-boxed pointers but are primitives, so exclude them.
fn is_object_value(value: f64) -> bool {
    let bits = value.to_bits();
    if (bits & crate::value::TAG_MASK) != crate::value::POINTER_TAG {
        return false;
    }
    let raw = (bits & crate::value::POINTER_MASK) as usize;
    if crate::value::addr_class::is_handle_band(raw) {
        return false;
    }
    !crate::symbol::is_registered_symbol(raw)
}

/// `Promise.reject(r)` (ECMA-262 27.2.4.6) with `this` = C: build the result
/// capability via `NewPromiseCapability(C)` (so a custom/subclass constructor's
/// executor runs and a non-constructor `this` throws), then
/// `Call(capability.[[Reject]], undefined, «r»)`.
#[no_mangle]
pub extern "C" fn js_promise_reject_spec(this_ctor: f64, reason: f64) -> f64 {
    ensure_arity_registered();
    // Default `Promise`: the optimized native rejected-promise path.
    if is_default_promise_constructor(this_ctor) {
        let p = crate::promise::js_promise_rejected(reason);
        return js_nanbox_pointer(p as i64);
    }
    let cap = new_promise_capability(this_ctor);
    if let Err(thrown) = call_with_this(cap.reject, undef(), &[reason]) {
        crate::exception::js_throw(thrown);
    }
    cap.promise
}

/// `Promise.resolve(x)` (ECMA-262 27.2.4.7 → PromiseResolve) with `this` = C:
/// require C to be an Object, short-circuit when `x` is already a promise whose
/// `constructor` is C, else `NewPromiseCapability(C)` +
/// `Call(capability.[[Resolve]], undefined, «x»)`.
#[no_mangle]
pub extern "C" fn js_promise_resolve_spec(this_ctor: f64, value: f64) -> f64 {
    ensure_arity_registered();
    if !is_object_value(this_ctor) {
        throw_type_error("Promise.resolve called on non-object");
    }
    // Default `Promise`: keep the optimized native path — it preserves promise
    // identity AND assimilates thenables (object-literal `then`), which the
    // generic capability path below would not (its resolve just stores the
    // value). The per-element resolve of the combinators routes through here.
    if is_default_promise_constructor(this_ctor) {
        let p = crate::promise::js_promise_resolved(value);
        return js_nanbox_pointer(p as i64);
    }
    if crate::promise::js_value_is_promise(value) != 0 {
        let xctor = unsafe {
            crate::value::js_dynamic_object_get_property(
                value,
                b"constructor".as_ptr() as *const i8,
                11,
            )
        };
        if xctor.to_bits() == this_ctor.to_bits() {
            return value;
        }
    }
    let cap = new_promise_capability(this_ctor);
    if let Err(thrown) = call_with_this(cap.resolve, undef(), &[value]) {
        crate::exception::js_throw(thrown);
    }
    cap.promise
}

/// `Promise.withResolvers()` (ECMA-262 27.2.4.x) with `this` = C:
/// `NewPromiseCapability(C)` then return `{ promise, resolve, reject }`. Honors a
/// subclass / custom constructor `this`; a non-constructor `this` throws from the
/// capability flow (test262 `withResolvers/newpromisecapability-this-value`).
#[no_mangle]
pub extern "C" fn js_promise_with_resolvers_spec(this_ctor: f64) -> f64 {
    ensure_arity_registered();
    if is_default_promise_constructor(this_ctor) {
        let obj = crate::promise::js_promise_with_resolvers();
        return js_nanbox_pointer(obj as i64);
    }
    let cap = new_promise_capability(this_ctor);
    let packed = b"promise\0resolve\0reject\0";
    let obj = crate::object::js_object_alloc_with_shape(
        0xFFF0_0001,
        3,
        packed.as_ptr(),
        packed.len() as u32,
    );
    crate::object::js_object_set_field(obj, 0, JSValue::from_bits(cap.promise.to_bits()));
    crate::object::js_object_set_field(obj, 1, JSValue::from_bits(cap.resolve.to_bits()));
    crate::object::js_object_set_field(obj, 2, JSValue::from_bits(cap.reject.to_bits()));
    js_nanbox_pointer(obj as i64)
}

/// `Promise.try(callbackfn, ...args)` (ECMA-262 27.2.4.x) with `this` = C:
/// `NewPromiseCapability(C)`, call the callback synchronously, then resolve with
/// its return value or reject with the thrown completion. Honors a subclass /
/// custom constructor `this`; a non-object `this` throws.
#[no_mangle]
pub extern "C" fn js_promise_try_spec(this_ctor: f64, callback: f64, rest: f64) -> f64 {
    ensure_arity_registered();
    if !is_object_value(this_ctor) {
        throw_type_error("Promise.try called on non-object");
    }
    // Default `Promise`: keep the optimized native path (synchronous call +
    // resolve/reject), which also assimilates a thenable return value.
    if is_default_promise_constructor(this_ctor) {
        let rest_v = JSValue::from_bits(rest.to_bits());
        let rest_ptr = if rest_v.is_pointer() {
            rest_v.as_pointer::<crate::array::ArrayHeader>()
        } else {
            std::ptr::null()
        };
        let p = crate::promise::js_promise_try(callback, rest_ptr);
        return js_nanbox_pointer(p as i64);
    }
    let cap = new_promise_capability(this_ctor);
    let rest_v = JSValue::from_bits(rest.to_bits());
    let args: Vec<f64> = if rest_v.is_pointer() {
        let arr = rest_v.as_pointer::<crate::array::ArrayHeader>();
        let n = unsafe { (*arr).length };
        (0..n).map(|i| js_array_get_f64(arr, i)).collect()
    } else {
        Vec::new()
    };
    match call_with_this(callback, undef(), &args) {
        Ok(value) => {
            let _ = call_with_this(cap.resolve, undef(), &[value]);
        }
        Err(reason) => {
            let _ = call_with_this(cap.reject, undef(), &[reason]);
        }
    }
    cap.promise
}
