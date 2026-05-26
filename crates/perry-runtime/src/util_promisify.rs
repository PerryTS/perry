//! `util.promisify(original)` — wraps a Node-style callback function
//! `original(arg1, …, argN, cb)` (where `cb(err, value)`) into a function
//! returning a Promise.
//!
//! Implementation strategy:
//!
//! 1. `js_util_promisify(fn)` allocates an outer closure that captures
//!    the original callable. The outer closure is registered as a rest
//!    function with `fixed_arity = 0`, so `dispatch_rest_bundled` bundles
//!    every forwarded arg into one array and invokes the body as
//!    `outer_thunk(closure, rest_array_value)`.
//! 2. `outer_thunk` allocates a fresh `Promise`, allocates an inner
//!    callback closure that captures the promise pointer, builds an args
//!    vector `[…rest, inner_cb_value]`, wraps the user call in
//!    setjmp/longjmp so a synchronous throw rejects the promise instead
//!    of crashing the process, and returns the promise NaN-boxed.
//! 3. `inner_callback_thunk(closure, err, value)` consults `err`: if it's
//!    null/undefined the promise resolves with `value`; otherwise it
//!    rejects with `err`.
//!
//! Out of scope for now: `promisify.custom` (needs Symbol support),
//! multi-value resolution (Node returns the first non-error arg only,
//! which is what we do — that matches the common case), and ad-hoc
//! `.call(thisArg, …)` binding (the wrapper today forwards args
//! transparently but doesn't reach into `this`).

use std::cell::Cell;
use std::os::raw::c_int;

use crate::array::{js_array_alloc, js_array_length, js_array_push_f64, ArrayHeader};
use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_get_capture_ptr,
    js_closure_set_capture_f64, js_closure_set_capture_ptr, js_register_closure_arity,
    js_register_closure_rest, ClosureHeader,
};
use crate::ffi::setjmp::setjmp;
use crate::promise::{js_promise_new, js_promise_reject, js_promise_resolve, Promise};
use crate::value::{JSValue, POINTER_MASK, POINTER_TAG, TAG_MASK, TAG_NULL, TAG_UNDEFINED};

const TAG_UNDEFINED_F64: f64 = unsafe { std::mem::transmute::<u64, f64>(TAG_UNDEFINED) };

fn nanbox_pointer(ptr: *const u8) -> f64 {
    f64::from_bits(JSValue::pointer(ptr).bits())
}

fn nanbox_promise(p: *mut Promise) -> f64 {
    f64::from_bits(JSValue::pointer(p as *const u8).bits())
}

fn err_is_nullish(err: f64) -> bool {
    let bits = err.to_bits();
    bits == TAG_UNDEFINED || bits == TAG_NULL
}

fn register_thunks_once() {
    thread_local! {
        static REGISTERED: Cell<bool> = const { Cell::new(false) };
    }
    REGISTERED.with(|flag| {
        if flag.get() {
            return;
        }
        // Outer thunk: fixed_arity = 0 with a rest param, so dispatch
        // bundles ALL forwarded args into one array and we receive them
        // as `(closure, rest_array_nanbox)`.
        js_register_closure_rest(outer_thunk as *const u8, 0);
        // Inner callback: declared arity 2 — `(err, value)`. Registering
        // the arity lets dispatch pad with `undefined` when callers
        // invoke it as `cb(err)` only, matching Node's contract.
        js_register_closure_arity(inner_callback_thunk as *const u8, 2);
        flag.set(true);
    });
}

/// `util.promisify(fn)` — returns a wrapper closure as a NaN-boxed f64.
///
/// If `fn` isn't pointer-shaped (not a closure / native callable), we fall
/// back to returning `fn` unchanged. Node throws `TypeError [ERR_INVALID_ARG_TYPE]`
/// in that case; we keep behavior conservative for now so callers that
/// accidentally promisify a non-function still get a clear "value is not a
/// function" error at the call site rather than crashing here.
#[no_mangle]
pub extern "C" fn js_util_promisify(fn_value: f64) -> f64 {
    let bits = fn_value.to_bits();
    let tag = bits & TAG_MASK;
    if tag != POINTER_TAG {
        return fn_value;
    }
    register_thunks_once();

    let scope = crate::gc::RuntimeHandleScope::new();
    let fn_handle = scope.root_nanbox_f64(fn_value);
    let closure = js_closure_alloc(outer_thunk as *const u8, 1);
    if closure.is_null() {
        return TAG_UNDEFINED_F64;
    }
    let closure_handle = scope.root_raw_mut_ptr(closure);
    js_closure_set_capture_f64(
        closure_handle.get_raw_mut_ptr(),
        0,
        fn_handle.get_nanbox_f64(),
    );
    nanbox_pointer(closure_handle.get_raw_const_ptr::<ClosureHeader>() as *const u8)
}

/// Outer wrapper body: `(closure, rest_array_value) -> promise_value`.
///
/// Receives the rest array of forwarded args (NaN-boxed pointer to an
/// `ArrayHeader`). Builds an args list `[…rest, inner_cb]`, runs the
/// original under a setjmp trap so a sync `throw` rejects the promise.
extern "C" fn outer_thunk(closure: *const ClosureHeader, rest_value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();

    let fn_value = if closure.is_null() {
        TAG_UNDEFINED_F64
    } else {
        js_closure_get_capture_f64(closure, 0)
    };
    let fn_handle = scope.root_nanbox_f64(fn_value);

    let promise_ptr = js_promise_new();
    if promise_ptr.is_null() {
        return TAG_UNDEFINED_F64;
    }
    let promise_handle = scope.root_raw_mut_ptr(promise_ptr);

    // Allocate the inner (err, value) callback that captures the promise.
    let cb_closure = js_closure_alloc(inner_callback_thunk as *const u8, 1);
    if cb_closure.is_null() {
        return nanbox_promise(promise_handle.get_raw_mut_ptr());
    }
    let cb_handle = scope.root_raw_mut_ptr(cb_closure);
    js_closure_set_capture_ptr(
        cb_handle.get_raw_mut_ptr(),
        0,
        promise_handle.get_raw_const_ptr::<Promise>() as i64,
    );
    let cb_value = nanbox_pointer(cb_handle.get_raw_const_ptr::<ClosureHeader>() as *const u8);
    let cb_value_handle = scope.root_nanbox_f64(cb_value);

    // Compose args: copy rest array elements then push the callback as
    // the trailing arg. A fresh array keeps us from mutating the
    // dispatch-supplied rest array (and lets us tolerate a null/empty
    // rest cleanly).
    let rest_bits = rest_value.to_bits();
    let rest_arr_ptr = if (rest_bits & TAG_MASK) == POINTER_TAG {
        (rest_bits & POINTER_MASK) as *const ArrayHeader
    } else {
        std::ptr::null()
    };
    let rest_len = if rest_arr_ptr.is_null() {
        0
    } else {
        js_array_length(rest_arr_ptr) as usize
    };

    let mut combined = js_array_alloc((rest_len + 1) as u32);
    if !rest_arr_ptr.is_null() && rest_len > 0 {
        let rest_data = unsafe {
            (rest_arr_ptr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64
        };
        for i in 0..rest_len {
            let v = unsafe { *rest_data.add(i) };
            combined = js_array_push_f64(combined, v);
        }
    }
    combined = js_array_push_f64(combined, cb_value_handle.get_nanbox_f64());
    let combined_handle = scope.root_raw_mut_ptr(combined);

    // Trap synchronous throws so the wrapper can reject the promise
    // instead of crashing the process. Mirrors the timer / microtask
    // runners' guard shape.
    let trap_buf = crate::exception::js_try_push();
    let jumped = unsafe { setjmp(trap_buf as *mut c_int) };
    if jumped == 0 {
        let arr = combined_handle.get_raw_const_ptr::<ArrayHeader>();
        let data =
            unsafe { (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64 };
        let n = js_array_length(arr) as usize;
        unsafe {
            crate::closure::js_native_call_value(fn_handle.get_nanbox_f64(), data, n);
        }
    } else {
        let exc = crate::exception::js_get_exception();
        crate::exception::js_clear_exception();
        js_promise_reject(promise_handle.get_raw_mut_ptr(), exc);
    }
    crate::exception::js_try_end();

    nanbox_promise(promise_handle.get_raw_mut_ptr())
}

/// Inner callback body: `(closure, err, value) -> undefined`.
///
/// Standard Node convention: a null/undefined `err` means the call
/// succeeded with `value`. Anything else is a rejection.
extern "C" fn inner_callback_thunk(closure: *const ClosureHeader, err: f64, value: f64) -> f64 {
    if closure.is_null() {
        return TAG_UNDEFINED_F64;
    }
    let promise_ptr = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    if promise_ptr.is_null() {
        return TAG_UNDEFINED_F64;
    }
    if err_is_nullish(err) {
        js_promise_resolve(promise_ptr, value);
    } else {
        js_promise_reject(promise_ptr, err);
    }
    TAG_UNDEFINED_F64
}
