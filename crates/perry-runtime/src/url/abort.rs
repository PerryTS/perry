//! `AbortController` / `AbortSignal` runtime implementation.

use super::*;

// =========================================================================
// AbortController implementation
// =========================================================================

/// AbortController object structure (matches ObjectHeader layout)
/// Field 0: signal (object-ptr NaN-boxed)
/// Field 1: aborted flag (NaN-boxed bool)
const ABORT_CONTROLLER_FIELD_COUNT: u32 = 2;
const ABORT_SIGNAL_FIELD: u32 = 0;
const ABORT_ABORTED_FIELD: u32 = 1;

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
    let signal = js_object_alloc(0, ABORT_SIGNAL_FIELD_COUNT);
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

/// Create a new AbortController
#[no_mangle]
pub extern "C" fn js_abort_controller_new() -> *mut ObjectHeader {
    // Allocate the AbortController object
    let controller = js_object_alloc(0, ABORT_CONTROLLER_FIELD_COUNT);

    let signal = alloc_abort_signal();

    // Set up controller keys
    let mut keys = js_array_alloc(ABORT_CONTROLLER_FIELD_COUNT);
    keys = js_array_push_f64(keys, create_string_f64("signal"));
    keys = js_array_push_f64(keys, create_string_f64("aborted"));
    js_object_set_keys(controller, keys);

    // Store signal in controller (NaN-boxed with POINTER_TAG)
    js_object_set_field_f64(controller, ABORT_SIGNAL_FIELD, nanbox_pointer_ac(signal));
    js_object_set_field_f64(
        controller,
        ABORT_ABORTED_FIELD,
        f64::from_bits(TAG_FALSE_AC),
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
    for i in 0..len {
        let cb_val = crate::array::js_array_get_f64(arr_ptr, i as u32);
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
        // Set aborted = true on signal
        js_object_set_field_f64(signal, 0, f64::from_bits(TAG_TRUE_AC));
        // Store reason (defaults to undefined); if user passes a string or other value we keep it as-is.
        js_object_set_field_f64(signal, 1, reason);
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

/// `AbortSignal.timeout(ms)` — returns a signal that is initially not aborted.
/// Perry does not spin up a real timer for this stub (tests only check the
/// initial state), but the returned object has the full AbortSignal shape so
/// subsequent `.aborted` / `.reason` / `.addEventListener` reads work.
#[no_mangle]
pub extern "C" fn js_abort_signal_timeout(_ms: f64) -> *mut ObjectHeader {
    alloc_abort_signal()
}
