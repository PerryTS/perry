//! `node:timers/promises` + `node:timers` namespace thunks (#1213).
//!
//! Extracted from `mod.rs` so the parent module stays under the file-size
//! gate. Pure code movement — no logic changes.

use super::TAG_UNDEFINED;
use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64, ClosureHeader,
};
use crate::object::{js_object_alloc, js_object_set_field_by_name};
use crate::string::js_string_from_bytes;
use crate::value::JSValue;

/// #3067 — `node:timers/promises` helpers reject a non-number `delay` and a
/// non-object `options` with the same `TypeError [ERR_INVALID_ARG_TYPE]` shape
/// Node emits. A missing (`undefined`) argument is allowed: Node defaults the
/// delay and treats absent options as the empty object. `NaN` is a number, so
/// it passes here (the warn/coerce path is tracked by #2966).
fn validate_delay(delay: f64) {
    let jv = JSValue::from_bits(delay.to_bits());
    if jv.is_undefined() || crate::fs::validate::is_numeric(jv) {
        return;
    }
    let message = format!(
        "The \"delay\" argument must be of type number. Received {}",
        crate::fs::validate::describe_received(delay)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

/// Accept `undefined` (no options) or a non-null, non-array object; anything
/// else throws like Node's `validateObject`. Mirrors the `Array`/`null`
/// detection used by `describe_received`.
fn validate_options(options: f64) {
    let jv = JSValue::from_bits(options.to_bits());
    if jv.is_undefined() || is_plain_object(options) {
        return;
    }
    let message = format!(
        "The \"options\" argument must be of type object. Received {}",
        crate::fs::validate::describe_received(options)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

/// True when `value` is a heap object that is not an array — the shape Node's
/// `validateObject` admits for a timer `options` bag.
fn is_plain_object(value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let ptr = jv.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return false;
    }
    let gc_header = unsafe { &*(ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader) };
    gc_header.obj_type != crate::gc::GC_TYPE_ARRAY
}

/// node:timers/promises.setTimeout(delay, value?) — a Promise that resolves
/// with `value` (or undefined) after `delay` ms. Composes the existing
/// promise-returning timer primitive; the closure dispatch pads a missing
/// `value` arg with undefined (arity registered in `ensure_export_singleton`).
/// Refs #1213.
pub(crate) extern "C" fn timers_promises_set_timeout(
    _closure: *const ClosureHeader,
    delay_ms: f64,
    value: f64,
    options: f64,
) -> f64 {
    validate_delay(delay_ms);
    validate_options(options);
    let signal = super::stream_promises::options_signal(options);
    if let Some(signal) = signal {
        if super::stream_promises::signal_aborted(signal) {
            let reason = super::stream_promises::signal_reason(signal);
            return crate::value::js_nanbox_pointer(
                crate::promise::js_promise_rejected(reason) as i64
            );
        }
    }
    let promise = crate::timer::js_set_timeout_value(delay_ms, value);
    if let Some(signal) = signal {
        super::stream_promises::register_abort_listener(signal, promise);
    }
    crate::value::js_nanbox_pointer(promise as i64)
}

/// node:timers/promises.setImmediate(value?, options?) — a Promise that
/// resolves with `value` (or undefined) on a later turn. `options` is validated
/// for type (#3067); honoring its `signal`/`ref` fields is tracked by #2603.
/// Refs #1213.
pub(crate) extern "C" fn timers_promises_set_immediate(
    _closure: *const ClosureHeader,
    value: f64,
    options: f64,
) -> f64 {
    validate_options(options);
    let promise = crate::timer::js_set_timeout_value(0.0, value);
    crate::value::js_nanbox_pointer(promise as i64)
}

pub(crate) extern "C" fn timers_promises_scheduler_wait(
    _closure: *const ClosureHeader,
    delay_ms: f64,
    options: f64,
) -> f64 {
    timers_promises_set_timeout(_closure, delay_ms, f64::from_bits(TAG_UNDEFINED), options)
}

pub(crate) extern "C" fn timers_promises_scheduler_yield(_closure: *const ClosureHeader) -> f64 {
    let promise = crate::promise::js_promise_resolved(f64::from_bits(TAG_UNDEFINED));
    crate::value::js_nanbox_pointer(promise as i64)
}

fn string_key(bytes: &[u8]) -> *mut crate::string::StringHeader {
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn boxed_ptr<T>(ptr: *const T) -> f64 {
    f64::from_bits(JSValue::pointer(ptr as *const u8).bits())
}

fn boxed_value_to_ptr<T>(value: f64) -> *mut T {
    let value = JSValue::from_bits(value.to_bits());
    if value.is_pointer() {
        value.as_pointer::<T>() as *mut T
    } else {
        std::ptr::null_mut()
    }
}

fn iter_result(value: f64, done: bool) -> f64 {
    let obj = js_object_alloc(0, 2);
    js_object_set_field_by_name(obj, string_key(b"value"), value);
    js_object_set_field_by_name(
        obj,
        string_key(b"done"),
        f64::from_bits(JSValue::bool(done).bits()),
    );
    boxed_ptr(obj as *const u8)
}

extern "C" fn timers_promises_interval_next(closure: *const ClosureHeader) -> f64 {
    let value = js_closure_get_capture_f64(closure, 0);
    let signal = js_closure_get_capture_f64(closure, 1);
    let delay_ms = js_closure_get_capture_f64(closure, 2);
    let closed = js_closure_get_capture_f64(closure, 3);
    if closed != 0.0 {
        return boxed_ptr(crate::promise::js_promise_resolved(iter_result(
            f64::from_bits(TAG_UNDEFINED),
            true,
        )) as *const u8);
    }
    if !JSValue::from_bits(signal.to_bits()).is_undefined()
        && super::stream_promises::signal_aborted(signal)
    {
        let reason = super::stream_promises::signal_reason(signal);
        js_closure_set_capture_f64(closure as *mut ClosureHeader, 3, 1.0);
        return boxed_ptr(crate::promise::js_promise_rejected(reason) as *const u8);
    }

    let promise = crate::timer::js_set_timeout_value(delay_ms, iter_result(value, false));
    if !JSValue::from_bits(signal.to_bits()).is_undefined() {
        super::stream_promises::register_abort_listener(signal, promise);
    }
    boxed_ptr(promise as *const u8)
}

extern "C" fn timers_promises_interval_self(closure: *const ClosureHeader) -> f64 {
    js_closure_get_capture_f64(closure, 0)
}

extern "C" fn timers_promises_interval_return(closure: *const ClosureHeader) -> f64 {
    let next_value = js_closure_get_capture_f64(closure, 0);
    let next = boxed_value_to_ptr::<ClosureHeader>(next_value);
    js_closure_set_capture_f64(next, 3, 1.0);
    boxed_ptr(
        crate::promise::js_promise_resolved(iter_result(f64::from_bits(TAG_UNDEFINED), true))
            as *const u8,
    )
}

/// node:timers/promises.setInterval(delay, value, options) — async iterator
/// that resolves each `.next()` after the requested delay until it is
/// returned or the optional AbortSignal rejects the pending tick.
pub(crate) extern "C" fn timers_promises_set_interval(
    _closure: *const ClosureHeader,
    delay_ms: f64,
    value: f64,
    options: f64,
) -> f64 {
    let signal = super::stream_promises::options_signal(options)
        .unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED));
    let obj = js_object_alloc(0, 4);
    let obj_value = boxed_ptr(obj as *const u8);

    let next = js_closure_alloc(timers_promises_interval_next as *const u8, 4);
    js_closure_set_capture_f64(next, 0, value);
    js_closure_set_capture_f64(next, 1, signal);
    js_closure_set_capture_f64(next, 2, delay_ms);
    js_closure_set_capture_f64(next, 3, 0.0);
    js_object_set_field_by_name(obj, string_key(b"next"), boxed_ptr(next as *const u8));

    let ret = js_closure_alloc(timers_promises_interval_return as *const u8, 1);
    js_closure_set_capture_f64(ret, 0, boxed_ptr(next as *const u8));
    js_object_set_field_by_name(obj, string_key(b"return"), boxed_ptr(ret as *const u8));

    let ret = js_closure_alloc(timers_promises_interval_self as *const u8, 1);
    js_closure_set_capture_f64(ret, 0, obj_value);
    let sym = crate::symbol::well_known_symbol("asyncIterator");
    if !sym.is_null() {
        unsafe {
            crate::symbol::js_object_set_symbol_property(
                obj_value,
                boxed_ptr(sym as *const u8),
                boxed_ptr(ret as *const u8),
            );
        }
    }

    obj_value
}

// ── node:timers namespace (`import * as timers from "node:timers"`) ──────────
// Route to the SAME global timer runtime fns the bare globals use, so
// `timers.setTimeout(...)` matches `setTimeout(...)`. NOTE: named imports
// (`import { setTimeout } from "node:timers"`) deliberately bypass this and
// keep the codegen global fast-path (which handles `setTimeout(fn, delay,
// ...args)` varargs) — compile.rs skips registering node:timers named imports
// as submodule exports. Refs #1213.
fn callback_arg_to_i64(v: f64) -> i64 {
    (v.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64
}
pub(crate) extern "C" fn timers_ns_set_timeout(
    _c: *const ClosureHeader,
    cb: f64,
    ms: f64,
    arg0: f64,
) -> f64 {
    let args = [arg0];
    crate::value::js_nanbox_pointer(unsafe {
        crate::timer::js_set_timeout_callback_args(callback_arg_to_i64(cb), ms, args.as_ptr(), 1)
    })
}
pub(crate) extern "C" fn timers_ns_set_interval(
    _c: *const ClosureHeader,
    cb: f64,
    ms: f64,
    arg0: f64,
) -> f64 {
    let args = [arg0];
    crate::value::js_nanbox_pointer(unsafe {
        crate::timer::js_set_interval_callback_args(callback_arg_to_i64(cb), ms, args.as_ptr(), 1)
    })
}
pub(crate) extern "C" fn timers_ns_set_immediate(
    _c: *const ClosureHeader,
    cb: f64,
    arg0: f64,
) -> f64 {
    let args = [arg0];
    crate::value::js_nanbox_pointer(unsafe {
        crate::timer::js_set_immediate_callback_args(callback_arg_to_i64(cb), args.as_ptr(), 1)
    })
}
pub(crate) extern "C" fn timers_ns_clear_timeout(_c: *const ClosureHeader, arg: f64) -> f64 {
    crate::timer::js_clear_timeout_value(arg);
    f64::from_bits(TAG_UNDEFINED)
}
pub(crate) extern "C" fn timers_ns_clear_interval(_c: *const ClosureHeader, arg: f64) -> f64 {
    crate::timer::js_clear_interval_value(arg);
    f64::from_bits(TAG_UNDEFINED)
}
pub(crate) extern "C" fn timers_ns_clear_immediate(_c: *const ClosureHeader, arg: f64) -> f64 {
    crate::timer::js_clear_immediate_value(arg);
    f64::from_bits(TAG_UNDEFINED)
}

pub(crate) extern "C" fn timers_promises_scheduler(
    _closure: *const ClosureHeader,
    _arg: f64,
) -> f64 {
    let msg = b"scheduler is not a function";
    let msg = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(boxed_ptr(err as *const u8))
}
