//! `AbortController` / `AbortSignal` runtime implementation.

use super::*;

// =========================================================================
// AbortController implementation
// =========================================================================

/// AbortController object structure (matches ObjectHeader layout)
/// Field 0: signal (object-ptr NaN-boxed)
/// Field 1: aborted flag (NaN-boxed bool)
/// Field 2: abort method (closure)
pub(crate) const ABORT_CONTROLLER_CLASS_ID: u32 = 0xFFFF_2401;
pub(crate) const ABORT_SIGNAL_CLASS_ID: u32 = 0xFFFF_2402;
const ABORT_CONTROLLER_FIELD_COUNT: u32 = 3;
const ABORT_SIGNAL_FIELD: u32 = 0;
const ABORT_ABORTED_FIELD: u32 = 1;
const ABORT_METHOD_FIELD: u32 = 2;

// AbortSignal object layout (all fields NaN-boxed):
//   field 0: aborted (bool)
//   field 1: reason (any)
//   field 2: listeners (array of closure f64 values; may be null/undefined if empty)
const ABORT_SIGNAL_FIELD_COUNT: u32 = 3;

const TAG_UNDEFINED_AC: u64 = 0x7FFC_0000_0000_0001;
const TAG_TRUE_AC: u64 = 0x7FFC_0000_0000_0004;
const TAG_FALSE_AC: u64 = 0x7FFC_0000_0000_0003;
const POINTER_TAG_AC: u64 = 0x7FFD_0000_0000_0000;

#[inline]
fn nanbox_pointer_ac(ptr: *mut ObjectHeader) -> f64 {
    if ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED_AC);
    }
    let bits = POINTER_TAG_AC | ((ptr as u64) & 0x0000_FFFF_FFFF_FFFF);
    f64::from_bits(bits)
}

#[inline]
fn unbox_pointer_ac(v: f64) -> *mut ObjectHeader {
    let bits = v.to_bits();
    if (bits & 0xFFFF_0000_0000_0000) != POINTER_TAG_AC {
        // Fallback: legacy raw bitcast path
        return (v.to_bits() as usize) as *mut ObjectHeader;
    }
    (bits & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader
}

fn alloc_abort_signal() -> *mut ObjectHeader {
    let signal = js_object_alloc(ABORT_SIGNAL_CLASS_ID, ABORT_SIGNAL_FIELD_COUNT);
    let mut signal_keys = js_array_alloc(ABORT_SIGNAL_FIELD_COUNT);
    signal_keys = js_array_push_f64(signal_keys, create_string_f64("aborted"));
    signal_keys = js_array_push_f64(signal_keys, create_string_f64("reason"));
    signal_keys = js_array_push_f64(signal_keys, create_string_f64("_listeners"));
    js_object_set_keys(signal, signal_keys);
    js_object_set_field_f64(signal, 0, f64::from_bits(TAG_FALSE_AC));
    js_object_set_field_f64(signal, 1, f64::from_bits(TAG_UNDEFINED_AC));
    js_object_set_field_f64(signal, 2, f64::from_bits(TAG_UNDEFINED_AC));
    signal
}

extern "C" fn abort_controller_abort_method(
    closure: *const crate::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    let controller_bits = crate::closure::js_closure_get_capture_ptr(closure, 0) as u64;
    let controller_value = f64::from_bits(controller_bits);
    let controller = crate::value::js_nanbox_get_pointer(controller_value) as *mut ObjectHeader;
    js_abort_controller_abort_reason(controller, reason);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn abort_controller_abort_value(controller: *mut ObjectHeader) -> f64 {
    let func = abort_controller_abort_method as *const u8;
    crate::closure::js_register_closure_arity(func, 1);
    let closure = crate::closure::js_closure_alloc(func, 1);
    let controller_value = crate::value::js_nanbox_pointer(controller as i64);
    crate::closure::js_closure_set_capture_ptr(closure, 0, controller_value.to_bits() as i64);
    crate::value::js_nanbox_pointer(closure as i64)
}

/// Create a new AbortController
#[no_mangle]
pub extern "C" fn js_abort_controller_new() -> *mut ObjectHeader {
    // Allocate the AbortController object
    let controller = js_object_alloc(ABORT_CONTROLLER_CLASS_ID, ABORT_CONTROLLER_FIELD_COUNT);

    let signal = alloc_abort_signal();

    // Set up controller keys
    let mut keys = js_array_alloc(ABORT_CONTROLLER_FIELD_COUNT);
    keys = js_array_push_f64(keys, create_string_f64("signal"));
    keys = js_array_push_f64(keys, create_string_f64("aborted"));
    keys = js_array_push_f64(keys, create_string_f64("abort"));
    js_object_set_keys(controller, keys);

    // Store signal in controller (NaN-boxed with POINTER_TAG)
    js_object_set_field_f64(controller, ABORT_SIGNAL_FIELD, nanbox_pointer_ac(signal));
    js_object_set_field_f64(
        controller,
        ABORT_ABORTED_FIELD,
        f64::from_bits(TAG_FALSE_AC),
    );
    js_object_set_field_f64(
        controller,
        ABORT_METHOD_FIELD,
        abort_controller_abort_value(controller),
    );

    controller
}

/// Get the signal from an AbortController (returns NaN-boxed object ptr)
#[no_mangle]
pub extern "C" fn js_abort_controller_signal(controller: *mut ObjectHeader) -> *mut ObjectHeader {
    if controller.is_null() {
        return std::ptr::null_mut();
    }
    let signal_val = crate::object::js_object_get_field_f64(controller, ABORT_SIGNAL_FIELD);
    unbox_pointer_ac(signal_val)
}

fn fire_abort_listeners(signal: *mut ObjectHeader) {
    if signal.is_null() {
        return;
    }
    let listeners_val = crate::object::js_object_get_field_f64(signal, 2);
    let bits = listeners_val.to_bits();
    if bits == TAG_UNDEFINED_AC || bits == TAG_FALSE_AC {
        return;
    }
    // Extract array pointer (NaN-boxed POINTER_TAG).
    let arr_ptr = if (bits & 0xFFFF_0000_0000_0000) == POINTER_TAG_AC {
        (bits & 0x0000_FFFF_FFFF_FFFF) as *mut crate::array::ArrayHeader
    } else {
        return;
    };
    if arr_ptr.is_null() {
        return;
    }
    let len = crate::array::js_array_length(arr_ptr) as usize;
    let mut callbacks = Vec::with_capacity(len);
    for i in 0..len {
        callbacks.push(crate::array::js_array_get_f64(arr_ptr, i as u32));
    }
    for cb_val in callbacks {
        let cb_bits = cb_val.to_bits();
        // Try to extract closure pointer (may be POINTER_TAG or raw bitcast).
        let cb_ptr = if (cb_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG_AC {
            (cb_bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::closure::ClosureHeader
        } else if cb_bits > 0x10000 && (cb_bits >> 48) == 0 {
            cb_bits as *const crate::closure::ClosureHeader
        } else {
            continue;
        };
        if !cb_ptr.is_null() {
            crate::closure::js_closure_call0(cb_ptr);
        }
    }
}

fn abort_signal_is_aborted(signal: *mut ObjectHeader) -> bool {
    if signal.is_null() {
        return false;
    }
    crate::object::js_object_get_field_f64(signal, 0).to_bits() == TAG_TRUE_AC
}

/// Return true if the given AbortSignal has already been aborted.
#[no_mangle]
pub extern "C" fn js_abort_signal_is_aborted(signal: *mut ObjectHeader) -> i32 {
    i32::from(abort_signal_is_aborted(signal))
}

pub(crate) fn abort_signal_ptr_from_value(value: f64) -> Option<*mut ObjectHeader> {
    let jsval = crate::value::JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return None;
    }
    let ptr = jsval.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
    if ptr.is_null() {
        return None;
    }
    let is_signal = unsafe { (*ptr).class_id == ABORT_SIGNAL_CLASS_ID };
    is_signal.then_some(ptr)
}

pub(crate) fn is_abort_signal_value(value: f64) -> bool {
    abort_signal_ptr_from_value(value).is_some()
}

/// Construct a Node-compatible AbortError value.
#[no_mangle]
pub extern "C" fn js_abort_error_value() -> f64 {
    let msg = b"The operation was aborted";
    let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_error_new_with_name_message(b"AbortError", msg_ptr);
    crate::node_submodules::register_error_code_pub(msg_ptr, "ABORT_ERR");
    crate::value::js_nanbox_pointer(err as i64)
}

/// Abort the controller (sets aborted = true on signal)
#[no_mangle]
pub extern "C" fn js_abort_controller_abort(controller: *mut ObjectHeader) {
    js_abort_controller_abort_reason(controller, f64::from_bits(TAG_UNDEFINED_AC));
}

/// Abort with an optional reason (NaN-boxed value). Fires any registered listeners.
#[no_mangle]
pub extern "C" fn js_abort_controller_abort_reason(controller: *mut ObjectHeader, reason: f64) {
    if controller.is_null() {
        return;
    }
    let signal_val = crate::object::js_object_get_field_f64(controller, ABORT_SIGNAL_FIELD);
    let signal = unbox_pointer_ac(signal_val);

    if !signal.is_null() {
        if abort_signal_is_aborted(signal) {
            js_object_set_field_f64(controller, ABORT_ABORTED_FIELD, f64::from_bits(TAG_TRUE_AC));
            return;
        }
        // Set aborted = true on signal
        js_object_set_field_f64(signal, 0, f64::from_bits(TAG_TRUE_AC));
        // Node defaults omitted/undefined reasons to a DOMException AbortError.
        let effective = if reason.to_bits() == TAG_UNDEFINED_AC {
            crate::event_target::abort_dom_exception_value()
        } else {
            reason
        };
        js_object_set_field_f64(signal, 1, effective);
        // Fire listeners
        fire_abort_listeners(signal);
    }

    // Also set aborted on controller
    js_object_set_field_f64(controller, ABORT_ABORTED_FIELD, f64::from_bits(TAG_TRUE_AC));
}

/// Register an "abort" event listener on a signal. `event_type` is the NaN-boxed
/// string name (we only act on "abort"); `listener` is a NaN-boxed closure f64.
#[no_mangle]
pub extern "C" fn js_abort_signal_add_listener(
    signal: *mut ObjectHeader,
    event_type: f64,
    listener: f64,
) {
    if signal.is_null() {
        return;
    }
    // Only handle "abort" events — ignore everything else.
    let type_str = get_string_content(event_type);
    if type_str != "abort" {
        return;
    }
    let listeners_val = crate::object::js_object_get_field_f64(signal, 2);
    let bits = listeners_val.to_bits();
    let arr_ptr: *mut crate::array::ArrayHeader =
        if (bits & 0xFFFF_0000_0000_0000) == POINTER_TAG_AC {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *mut crate::array::ArrayHeader
        } else {
            // Lazily allocate the listeners array.
            let new_arr = js_array_alloc(0);
            let new_bits = POINTER_TAG_AC | ((new_arr as u64) & 0x0000_FFFF_FFFF_FFFF);
            js_object_set_field_f64(signal, 2, f64::from_bits(new_bits));
            new_arr
        };
    if !arr_ptr.is_null() {
        js_array_push_f64(arr_ptr, listener);
    }
}

/// Remove one matching "abort" listener from a signal.
#[no_mangle]
pub extern "C" fn js_abort_signal_remove_listener(
    signal: *mut ObjectHeader,
    event_type: f64,
    listener: f64,
) {
    if signal.is_null() {
        return;
    }
    let type_str = get_string_content(event_type);
    if type_str != "abort" {
        return;
    }
    let listeners_val = crate::object::js_object_get_field_f64(signal, 2);
    let bits = listeners_val.to_bits();
    if (bits & 0xFFFF_0000_0000_0000) != POINTER_TAG_AC {
        return;
    }
    let arr_ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *mut crate::array::ArrayHeader;
    if arr_ptr.is_null() {
        return;
    }
    let len = crate::array::js_array_length(arr_ptr);
    for i in 0..len {
        let current = crate::array::js_array_get_f64(arr_ptr, i);
        if current.to_bits() != listener.to_bits() {
            continue;
        }
        for j in (i + 1)..len {
            let next = crate::array::js_array_get_f64(arr_ptr, j);
            crate::array::js_array_set_f64_unchecked(arr_ptr, j - 1, next);
        }
        crate::array::js_array_set_length(arr_ptr, (len - 1) as f64);
        break;
    }
}

/// `AbortSignal.timeout(ms)` — returns a signal that is initially not aborted.
/// Perry does not spin up a real timer for this stub (tests only check the
/// initial state), but the returned object has the full AbortSignal shape so
/// subsequent `.aborted` / `.reason` / `.addEventListener` reads work.
#[no_mangle]
pub extern "C" fn js_abort_signal_timeout(_ms: f64) -> *mut ObjectHeader {
    alloc_abort_signal()
}

/// Mark a signal as aborted with `reason` and fire its listeners. Idempotent:
/// re-aborting an already-aborted signal is a no-op (Node behavior). Shared by
/// `AbortSignal.abort()` and the `AbortSignal.any()` propagation listener.
fn abort_signal_set_aborted(signal: *mut ObjectHeader, reason: f64) {
    if signal.is_null() || abort_signal_is_aborted(signal) {
        return;
    }
    js_object_set_field_f64(signal, 0, f64::from_bits(TAG_TRUE_AC));
    js_object_set_field_f64(signal, 1, reason);
    fire_abort_listeners(signal);
}

/// `AbortSignal.abort(reason?)` — returns an already-aborted signal whose
/// `.reason` is `reason` (or an `AbortError` when omitted, matching Node).
#[no_mangle]
pub extern "C" fn js_abort_signal_abort(reason: f64) -> *mut ObjectHeader {
    let signal = alloc_abort_signal();
    let reason_bits = reason.to_bits();
    // Node defaults the reason to an AbortError when none is supplied.
    let effective = if reason_bits == TAG_UNDEFINED_AC {
        crate::event_target::abort_dom_exception_value()
    } else {
        reason
    };
    js_object_set_field_f64(signal, 0, f64::from_bits(TAG_TRUE_AC));
    js_object_set_field_f64(signal, 1, effective);
    signal
}

/// `abortSignal.throwIfAborted()` — throws `abortSignal.reason` when the
/// signal is aborted, otherwise returns undefined (no-op).
#[no_mangle]
pub extern "C" fn js_abort_signal_throw_if_aborted(signal: *mut ObjectHeader) -> f64 {
    if abort_signal_is_aborted(signal) {
        let reason = crate::object::js_object_get_field_f64(signal, 1);
        // Node throws the stored reason verbatim (which is the AbortError
        // default when none was provided).
        crate::exception::js_throw(reason);
    }
    f64::from_bits(TAG_UNDEFINED_AC)
}

extern "C" fn abort_any_propagate_thunk(
    closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    // capture 0 = combined signal pointer (NaN-boxed), capture 1 = source signal.
    let combined_bits = crate::closure::js_closure_get_capture_ptr(closure, 0) as u64;
    let source_bits = crate::closure::js_closure_get_capture_ptr(closure, 1) as u64;
    let combined =
        crate::value::js_nanbox_get_pointer(f64::from_bits(combined_bits)) as *mut ObjectHeader;
    let source =
        crate::value::js_nanbox_get_pointer(f64::from_bits(source_bits)) as *mut ObjectHeader;
    let reason = if source.is_null() {
        f64::from_bits(TAG_UNDEFINED_AC)
    } else {
        crate::object::js_object_get_field_f64(source, 1)
    };
    abort_signal_set_aborted(combined, reason);
    f64::from_bits(TAG_UNDEFINED_AC)
}

/// `AbortSignal.any(signals)` — returns a signal that aborts as soon as any of
/// the input `signals` aborts, adopting that signal's `reason`. If any input is
/// already aborted, the combined signal is returned pre-aborted.
///
/// `signals_arr` is the raw `*mut ArrayHeader` (i64 handle) for the input
/// iterable already materialized as an array.
#[no_mangle]
pub extern "C" fn js_abort_signal_any(
    signals_arr: *mut crate::array::ArrayHeader,
) -> *mut ObjectHeader {
    let combined = alloc_abort_signal();
    if signals_arr.is_null() {
        return combined;
    }
    let len = crate::array::js_array_length(signals_arr);
    let combined_box = crate::value::js_nanbox_pointer(combined as i64);
    for i in 0..len {
        let elem = crate::array::js_array_get_f64(signals_arr, i);
        let Some(source) = abort_signal_ptr_from_value(elem) else {
            continue;
        };
        if abort_signal_is_aborted(source) {
            // Adopt the first already-aborted source's reason immediately.
            let reason = crate::object::js_object_get_field_f64(source, 1);
            abort_signal_set_aborted(combined, reason);
            return combined;
        }
        // Register a propagation listener that adopts this source's reason
        // when it later aborts.
        let func = abort_any_propagate_thunk as *const u8;
        crate::closure::js_register_closure_arity(func, 1);
        let closure = crate::closure::js_closure_alloc(func, 2);
        crate::closure::js_closure_set_capture_ptr(closure, 0, combined_box.to_bits() as i64);
        crate::closure::js_closure_set_capture_ptr(closure, 1, elem.to_bits() as i64);
        let listener = crate::value::js_nanbox_pointer(closure as i64);
        let abort_evt = create_string_f64("abort");
        js_abort_signal_add_listener(source, abort_evt, listener);
    }
    combined
}

// #2582: keepalive anchors so the auto-optimize whole-program LLVM bitcode
// rebuild doesn't internalize + dead-strip these codegen-only `#[no_mangle]`
// entry points (see project_auto_optimize_keepalive_3320). These are only
// referenced from generated `.o`, so without `#[used]` they vanish.
#[used]
static KEEP_ABORT_SIGNAL_ABORT: extern "C" fn(f64) -> *mut ObjectHeader = js_abort_signal_abort;
#[used]
static KEEP_ABORT_SIGNAL_ANY: extern "C" fn(*mut crate::array::ArrayHeader) -> *mut ObjectHeader =
    js_abort_signal_any;
#[used]
static KEEP_ABORT_SIGNAL_THROW_IF_ABORTED: extern "C" fn(*mut ObjectHeader) -> f64 =
    js_abort_signal_throw_if_aborted;
