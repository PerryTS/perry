//! `util.promisify(original)` and `util.callbackify(original)` adapters.
//!
//! `promisify` wraps a Node-style callback function `original(arg1, …, argN, cb)`
//! (where `cb(err, value)`) into a function returning a Promise.
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
use crate::promise::{
    js_promise_attach_handlers, js_promise_new, js_promise_reject, js_promise_resolve,
    js_value_is_promise, ClosurePtr, Promise,
};
use crate::string::js_string_from_bytes;
use crate::value::{JSValue, POINTER_MASK, POINTER_TAG, TAG_MASK, TAG_NULL, TAG_UNDEFINED};

const TAG_UNDEFINED_F64: f64 = f64::from_bits(TAG_UNDEFINED);
const TAG_NULL_F64: f64 = f64::from_bits(TAG_NULL);
const PROMISIFY_CUSTOM_KEY: &[u8] = b"nodejs.util.promisify.custom";

fn nanbox_pointer(ptr: *const u8) -> f64 {
    f64::from_bits(JSValue::pointer(ptr).bits())
}

fn nanbox_promise(p: *mut Promise) -> f64 {
    f64::from_bits(JSValue::pointer(p as *const u8).bits())
}

fn promise_ptr_from_value(value: f64) -> *mut Promise {
    if js_value_is_promise(value) == 0 {
        return std::ptr::null_mut();
    }
    (value.to_bits() & POINTER_MASK) as *mut Promise
}

fn err_is_nullish(err: f64) -> bool {
    let bits = err.to_bits();
    bits == TAG_UNDEFINED || bits == TAG_NULL
}

pub(crate) fn promisify_custom_symbol() -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let key = crate::string::js_string_from_bytes(
        PROMISIFY_CUSTOM_KEY.as_ptr(),
        PROMISIFY_CUSTOM_KEY.len() as u32,
    );
    let key_handle = scope.root_string_ptr(key);
    let key_value = f64::from_bits(
        JSValue::string_ptr(key_handle.get_raw_const_ptr::<crate::StringHeader>() as *mut _).bits(),
    );
    unsafe { crate::symbol::js_symbol_for(key_value) }
}

fn is_callable_closure(value: f64) -> bool {
    let bits = value.to_bits();
    if (bits & TAG_MASK) != POINTER_TAG {
        return false;
    }
    let ptr = (bits & POINTER_MASK) as usize;
    crate::closure::is_closure_ptr(ptr)
}

fn custom_promisified_value(fn_value: f64) -> Option<f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let fn_handle = scope.root_nanbox_f64(fn_value);
    let custom_symbol = promisify_custom_symbol();
    let symbol_handle = scope.root_nanbox_f64(custom_symbol);
    let custom_value = unsafe {
        crate::symbol::js_object_get_symbol_property(
            fn_handle.get_nanbox_f64(),
            symbol_handle.get_nanbox_f64(),
        )
    };
    if !is_callable_closure(custom_value) {
        return None;
    }
    let custom_handle = scope.root_nanbox_f64(custom_value);
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            custom_handle.get_nanbox_f64(),
            symbol_handle.get_nanbox_f64(),
            custom_handle.get_nanbox_f64(),
        );
    }
    Some(custom_handle.get_nanbox_f64())
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
        js_register_closure_rest(callbackify_outer_thunk as *const u8, 0);
        js_register_closure_arity(callbackify_fulfilled_thunk as *const u8, 1);
        js_register_closure_arity(callbackify_rejected_thunk as *const u8, 1);
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

    if let Some(custom) = custom_promisified_value(fn_value) {
        return custom;
    }

    // #1857: `child_process.exec` / `execFile` carry a Node custom-promisify
    // hook that resolves to `{ stdout, stderr }` — not the single first-result
    // value the general wrapper below yields. Detect the bound export and hand
    // off to the child_process-specific wrapper.
    if let Some((module, method)) =
        unsafe { crate::object::bound_native_callable_module_and_method(fn_value) }
    {
        if module == "child_process" && (method == "exec" || method == "execFile") {
            return crate::child_process::make_promisified_child_process(&method);
        }
        // node:zlib's callback-form codecs (`gzip`/`gunzip`/`deflate`/`inflate`
        // /`deflateRaw`/`inflateRaw`/`unzip`/`brotliCompress`/`brotliDecompress`)
        // are wired in Perry to *return a Promise* directly — there's no
        // callback parameter that the generic outer_thunk could await. Routing
        // them through the standard wrapper would inject a callback that never
        // fires and the awaiter would hang. Since `util.promisify(p)` is meant
        // to expose a Promise-returning API and `p` already is one, the
        // identity transform is the correct semantics here.
        if module == "zlib"
            && matches!(
                method.as_str(),
                "gzip"
                    | "gunzip"
                    | "deflate"
                    | "inflate"
                    | "deflateRaw"
                    | "inflateRaw"
                    | "unzip"
                    | "brotliCompress"
                    | "brotliDecompress"
            )
        {
            return fn_value;
        }
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

/// Minimal `util.deprecate(fn, msg, code)` shape.
///
/// Full warning emission is separate; callers must at least receive a callable
/// that forwards to the original function, and Node accepts string codes that
/// contain spaces.
#[no_mangle]
pub extern "C" fn js_util_deprecate(fn_value: f64, _msg: f64, _code: f64) -> f64 {
    fn_value
}

/// `util.callbackify(fn)` — returns a wrapper closure as a NaN-boxed f64.
#[no_mangle]
pub extern "C" fn js_util_callbackify(fn_value: f64) -> f64 {
    let bits = fn_value.to_bits();
    let tag = bits & TAG_MASK;
    if tag != POINTER_TAG {
        return fn_value;
    }

    register_thunks_once();

    let scope = crate::gc::RuntimeHandleScope::new();
    let fn_handle = scope.root_nanbox_f64(fn_value);
    let closure = js_closure_alloc(callbackify_outer_thunk as *const u8, 1);
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

/// Outer callbackify body: `(closure, rest_array_value) -> undefined`.
///
/// The last incoming argument is the Node-style callback. Every preceding
/// argument is forwarded to the original promise-returning function.
extern "C" fn callbackify_outer_thunk(closure: *const ClosureHeader, rest_value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();

    let fn_value = if closure.is_null() {
        TAG_UNDEFINED_F64
    } else {
        js_closure_get_capture_f64(closure, 0)
    };
    let fn_handle = scope.root_nanbox_f64(fn_value);

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
    if rest_len == 0 {
        return TAG_UNDEFINED_F64;
    }

    let rest_data = unsafe {
        (rest_arr_ptr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64
    };
    let callback_value = unsafe { *rest_data.add(rest_len - 1) };
    let callback_handle = scope.root_nanbox_f64(callback_value);

    let original_arg_len = rest_len - 1;
    let mut original_args = js_array_alloc(original_arg_len as u32);
    for i in 0..original_arg_len {
        let v = unsafe { *rest_data.add(i) };
        original_args = js_array_push_f64(original_args, v);
    }
    let original_args_handle = scope.root_raw_mut_ptr(original_args);

    let mut returned = TAG_UNDEFINED_F64;
    let trap_buf = crate::exception::js_try_push();
    let jumped = unsafe { setjmp(trap_buf as *mut c_int) };
    if jumped == 0 {
        let arr = original_args_handle.get_raw_const_ptr::<ArrayHeader>();
        let data =
            unsafe { (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64 };
        returned = unsafe {
            crate::closure::js_native_call_value(fn_handle.get_nanbox_f64(), data, original_arg_len)
        };
    } else {
        let exc = crate::exception::js_get_exception();
        crate::exception::js_clear_exception();
        call_callback_rejected(callback_handle.get_nanbox_f64(), exc);
    }
    crate::exception::js_try_end();

    let promise_ptr = promise_ptr_from_value(returned);
    if promise_ptr.is_null() {
        return TAG_UNDEFINED_F64;
    }
    let promise_handle = scope.root_raw_mut_ptr(promise_ptr);

    let fulfilled = js_closure_alloc(callbackify_fulfilled_thunk as *const u8, 1);
    if fulfilled.is_null() {
        return TAG_UNDEFINED_F64;
    }
    let fulfilled_handle = scope.root_raw_mut_ptr(fulfilled);
    js_closure_set_capture_f64(
        fulfilled_handle.get_raw_mut_ptr(),
        0,
        callback_handle.get_nanbox_f64(),
    );

    let rejected = js_closure_alloc(callbackify_rejected_thunk as *const u8, 1);
    if rejected.is_null() {
        return TAG_UNDEFINED_F64;
    }
    let rejected_handle = scope.root_raw_mut_ptr(rejected);
    js_closure_set_capture_f64(
        rejected_handle.get_raw_mut_ptr(),
        0,
        callback_handle.get_nanbox_f64(),
    );

    js_promise_attach_handlers(
        promise_handle.get_raw_mut_ptr(),
        fulfilled_handle.get_raw_const_ptr::<ClosureHeader>() as ClosurePtr,
        rejected_handle.get_raw_const_ptr::<ClosureHeader>() as ClosurePtr,
    );

    TAG_UNDEFINED_F64
}

extern "C" fn callbackify_fulfilled_thunk(closure: *const ClosureHeader, value: f64) -> f64 {
    if closure.is_null() {
        return TAG_UNDEFINED_F64;
    }
    let callback_value = js_closure_get_capture_f64(closure, 0);
    let args = [TAG_NULL_F64, value];
    unsafe {
        crate::closure::js_native_call_value(callback_value, args.as_ptr(), args.len());
    }
    TAG_UNDEFINED_F64
}

extern "C" fn callbackify_rejected_thunk(closure: *const ClosureHeader, reason: f64) -> f64 {
    if closure.is_null() {
        return TAG_UNDEFINED_F64;
    }
    let callback_value = js_closure_get_capture_f64(closure, 0);
    call_callback_rejected(callback_value, reason);
    TAG_UNDEFINED_F64
}

fn call_callback_rejected(callback_value: f64, reason: f64) {
    let err = if crate::value::js_is_truthy(reason) == 0 {
        make_falsy_rejection_error(reason)
    } else {
        reason
    };
    let args = [err];
    unsafe {
        crate::closure::js_native_call_value(callback_value, args.as_ptr(), args.len());
    }
}

fn make_falsy_rejection_error(reason: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let reason_handle = scope.root_nanbox_f64(reason);

    let msg = js_string_from_bytes(
        b"Promise was rejected with falsy value".as_ptr(),
        b"Promise was rejected with falsy value".len() as u32,
    );
    let msg_handle = scope.root_string_ptr(msg);
    let error = crate::error::js_error_new_with_message(
        msg_handle.get_raw_const_ptr::<crate::StringHeader>() as *mut crate::StringHeader,
    );
    let error_handle = scope.root_raw_mut_ptr(error);

    let reason_key = js_string_from_bytes(b"reason".as_ptr(), b"reason".len() as u32);
    let reason_key_handle = scope.root_string_ptr(reason_key);
    crate::object::js_object_set_field_by_name(
        error_handle.get_raw_mut_ptr::<crate::error::ErrorHeader>()
            as *mut crate::object::ObjectHeader,
        reason_key_handle.get_raw_const_ptr::<crate::StringHeader>(),
        reason_handle.get_nanbox_f64(),
    );

    nanbox_pointer(error_handle.get_raw_const_ptr::<crate::error::ErrorHeader>() as *const u8)
}
