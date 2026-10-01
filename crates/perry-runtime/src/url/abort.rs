//! `AbortController` / `AbortSignal` runtime implementation.

use super::*;
use crate::event_target::state::{
    get_slot as get_abort_slot, set_slot as set_abort_slot, SIGNAL_ABORTED, SIGNAL_REASON,
};

// =========================================================================
// AbortController implementation
// =========================================================================

// State is held by ordinary symbol-keyed own properties.
pub(crate) const ABORT_CONTROLLER_CLASS_ID: u32 = crate::native_class_ids::ABORT_CONTROLLER;
pub(crate) const ABORT_SIGNAL_CLASS_ID: u32 = crate::native_class_ids::ABORT_SIGNAL;
const ABORT_SIGNAL_FIELD: u32 = crate::event_target::state::CONTROLLER_SIGNAL;

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
        return std::ptr::null_mut();
    }
    (bits & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader
}

fn alloc_abort_signal() -> *mut ObjectHeader {
    let _gc = crate::gc::GcSuppressScope::new();
    crate::event_target::note_constructed();
    let signal = js_object_alloc(ABORT_SIGNAL_CLASS_ID, 0);
    crate::event_target::state::link(signal, "AbortSignal");
    crate::event_target::state::initialize_target(signal, true);
    set_abort_slot(signal, SIGNAL_ABORTED, f64::from_bits(TAG_FALSE_AC));
    set_abort_slot(signal, SIGNAL_REASON, f64::from_bits(TAG_UNDEFINED_AC));
    set_abort_slot(
        signal,
        crate::event_target::state::COMPOSITE,
        f64::from_bits(TAG_FALSE_AC),
    );
    signal
}

/// Create a new AbortController
#[no_mangle]
pub extern "C" fn js_abort_controller_new() -> *mut ObjectHeader {
    let _gc = crate::gc::GcSuppressScope::new();
    crate::event_target::note_constructed();
    let controller = js_object_alloc(ABORT_CONTROLLER_CLASS_ID, 0);
    crate::event_target::state::link(controller, "AbortController");
    crate::event_target::state::set_slot(
        controller,
        ABORT_SIGNAL_FIELD,
        f64::from_bits(TAG_UNDEFINED_AC),
    );
    controller
}

/// Get the signal from an AbortController (returns NaN-boxed object ptr)
#[no_mangle]
pub extern "C" fn js_abort_controller_signal(controller: *mut ObjectHeader) -> *mut ObjectHeader {
    if controller.is_null() {
        return std::ptr::null_mut();
    }
    let signal_val = crate::event_target::state::get_slot(controller, ABORT_SIGNAL_FIELD);
    if signal_val.to_bits() == TAG_UNDEFINED_AC {
        let scope = crate::gc::RuntimeHandleScope::new();
        let controller = scope.root_raw_mut_ptr(controller);
        let signal = scope.root_raw_mut_ptr(alloc_abort_signal());
        let ((), result) = signal.across_mut(|| {
            controller.with_mut_ptr(|controller| {
                signal.with_mut_ptr(|signal| {
                    crate::event_target::state::set_slot(
                        controller,
                        ABORT_SIGNAL_FIELD,
                        nanbox_pointer_ac(signal),
                    )
                });
            });
        });
        result
    } else {
        unbox_pointer_ac(signal_val)
    }
}

fn fire_abort_listeners(signal: *mut ObjectHeader) {
    if signal.is_null() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let signal = scope.root_raw_mut_ptr(signal);
    signal.with_mut_ptr::<ObjectHeader, _>(|ptr| notify_fetch_abort(ptr as i64));
    let event = crate::event_target::js_event_new(
        create_string_f64("abort"),
        f64::from_bits(TAG_UNDEFINED_AC),
        1,
    );
    let event = scope.root_nanbox_f64(nanbox_pointer_ac(event));
    unsafe {
        signal.with_mut_ptr(|ptr| {
            crate::event_target::js_event_target_dispatch_event(ptr, event.get_nanbox_f64())
        });
    }
}

fn abort_signal_is_aborted(signal: *mut ObjectHeader) -> bool {
    if signal.is_null() {
        return false;
    }
    crate::value::js_is_truthy(get_abort_slot(signal, SIGNAL_ABORTED)) != 0
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
    if crate::value::addr_class::is_handle_band(ptr as usize) {
        return None;
    }
    let is_signal = unsafe {
        crate::value::addr_class::try_read_tracked_gc_header(ptr as usize)
            .is_some_and(|h| h.as_ref().obj_type == crate::gc::GC_TYPE_OBJECT)
            && (*ptr).class_id == ABORT_SIGNAL_CLASS_ID
    };
    is_signal.then_some(ptr)
}

pub(crate) fn is_abort_signal_value(value: f64) -> bool {
    abort_signal_ptr_from_value(value).is_some()
}

/// Resolve a JS value to its `AbortSignal` object pointer, or null when it is
/// not an AbortSignal. FFI accessor over `abort_signal_ptr_from_value` for
/// cross-crate callers — perry-stdlib's fetch abort bridge uses it to learn the
/// signal handle behind a `fetch(url, { signal })` option.
#[no_mangle]
pub extern "C" fn js_abort_signal_resolve_ptr(value: f64) -> *mut ObjectHeader {
    abort_signal_ptr_from_value(value).unwrap_or(std::ptr::null_mut())
}

/// Notify any in-flight `fetch` request bound to this signal that it has
/// aborted (so it rejects with an AbortError and drops the request).
///
/// Under `external-fetch-symbols` — a fetch-using build — this calls the linked
/// stdlib hook directly, the same link invariant `call_fetch_with_options`
/// relies on. A non-fetch build links no stdlib fetch, so there is nothing to
/// notify. Routing through `fire_abort_listeners` (rather than a per-fetch JS
/// listener) means reused signals never accumulate stale listener closures.
fn notify_fetch_abort(signal_ptr: i64) {
    #[cfg(feature = "external-fetch-symbols")]
    {
        unsafe extern "C" {
            fn js_fetch_notify_signal_aborted(signal_ptr: i64);
        }
        unsafe { js_fetch_notify_signal_aborted(signal_ptr) };
    }
    #[cfg(not(feature = "external-fetch-symbols"))]
    {
        // A default build reaches the global `fetch` through the registered
        // function pointer, not the linked symbol, so the abort hook has to be
        // registered too. Without this the whole `AbortSignal` path was dead in
        // exactly the configuration `fetch(url, { signal })` normally compiles
        // to — see `global_fetch::js_register_global_fetch_notify_abort`.
        crate::object::global_fetch::notify_fetch_abort_registered(signal_ptr);
    }
}

extern "C" fn abort_error_constructor_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    crate::error::js_throw_illegal_constructor_type_error()
}

fn abort_error_constructor_value() -> f64 {
    let func = crate::fn_info!(abort_error_constructor_thunk, 0; with_declared(0));
    let closure = crate::closure::js_closure_alloc(func, 0);
    crate::object::set_bound_native_closure_name(closure, "AbortError");
    crate::value::js_nanbox_pointer(closure as i64)
}

/// Construct a Node-compatible AbortError value.
#[no_mangle]
pub extern "C" fn js_abort_error_value() -> f64 {
    let msg = b"The operation was aborted";
    let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    crate::node_submodules::register_error_code_pub(msg_ptr, "ABORT_ERR");
    let err = crate::error::js_error_new_with_name_message(b"AbortError", msg_ptr);
    crate::node_submodules::set_error_user_prop(
        err as usize,
        "constructor",
        abort_error_constructor_value(),
    );
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let reason = scope.root_nanbox_f64(reason);
    let signal = js_abort_controller_signal(controller);

    if !signal.is_null() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let rooted_signal = scope.root_raw_mut_ptr(signal);
        if abort_signal_is_aborted(signal) {
            return;
        }
        // Set aborted = true on signal
        set_abort_slot(signal, SIGNAL_ABORTED, f64::from_bits(TAG_TRUE_AC));
        // Node defaults omitted/undefined reasons to a DOMException AbortError.
        let effective = if reason.get_nanbox_u64() == TAG_UNDEFINED_AC {
            crate::event_target::abort_dom_exception_value()
        } else {
            reason.get_nanbox_f64()
        };
        rooted_signal.with_mut_ptr(|ptr| set_abort_slot(ptr, SIGNAL_REASON, effective));
        // Fire listeners
        rooted_signal.with_mut_ptr(fire_abort_listeners);
    }
}

/// Register a listener through the shared EventTarget state.
#[no_mangle]
pub extern "C" fn js_abort_signal_add_listener(
    signal: *mut ObjectHeader,
    event_type: f64,
    listener: f64,
) {
    let _gc = crate::gc::GcSuppressScope::new();
    let name = crate::builtins::js_string_coerce(event_type);
    if crate::value::JSValue::from_bits(listener.to_bits()).is_pointer() {
        unsafe {
            crate::event_target::js_event_target_add_event_listener(
                signal,
                name,
                crate::value::js_nanbox_get_pointer(listener),
            );
        }
    }
}

#[no_mangle]
pub extern "C" fn js_abort_signal_remove_listener(
    signal: *mut ObjectHeader,
    event_type: f64,
    listener: f64,
) {
    let _gc = crate::gc::GcSuppressScope::new();
    let name = crate::builtins::js_string_coerce(event_type);
    if crate::value::JSValue::from_bits(listener.to_bits()).is_pointer() {
        unsafe {
            crate::event_target::js_event_target_remove_event_listener(
                signal,
                name,
                crate::value::js_nanbox_get_pointer(listener),
            );
        }
    }
}

#[no_mangle]
pub extern "C" fn js_abort_signal_listener_count(signal: *mut ObjectHeader) -> f64 {
    let _gc = crate::gc::GcSuppressScope::new();
    crate::array::js_array_length(js_abort_signal_listeners_copy(signal)) as f64
}

#[no_mangle]
pub extern "C" fn js_abort_signal_listeners_copy(
    signal: *mut ObjectHeader,
) -> *mut crate::array::ArrayHeader {
    let _gc = crate::gc::GcSuppressScope::new();
    let name = js_string_from_bytes(b"abort".as_ptr(), 5);
    unsafe { crate::event_target::js_event_target_get_event_listeners(signal, name) }
}

/// Build the `TimeoutError` DOMException that `AbortSignal.timeout(ms)` aborts
/// with when its deadline elapses (Node names it `TimeoutError`, distinct from
/// the `AbortError` used by `controller.abort()`).
fn timeout_dom_exception_value() -> f64 {
    let err = crate::event_target::js_dom_exception_new(
        create_string_f64("The operation was aborted due to timeout"),
        create_string_f64("TimeoutError"),
    );
    crate::value::js_nanbox_pointer(err as i64)
}

/// Callback-timer thunk: fires on the main thread (via `js_callback_timer_tick`)
/// when an `AbortSignal.timeout(ms)` deadline elapses, aborting the captured
/// signal with a `TimeoutError` and firing its `abort` listeners (which is how
/// a pending `fetch` bound by the signal learns to reject — see
/// `js_fetch_with_options`).
extern "C" fn abort_signal_timeout_fire(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let signal_bits = crate::closure::js_closure_get_capture_ptr(closure, 0) as u64;
    let signal =
        crate::value::js_nanbox_get_pointer(f64::from_bits(signal_bits)) as *mut ObjectHeader;
    if !signal.is_null() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let signal = scope.root_raw_mut_ptr(signal);
        let reason = timeout_dom_exception_value();
        signal.with_mut_ptr(|ptr| abort_signal_set_aborted(ptr, reason));
    }
    f64::from_bits(TAG_UNDEFINED_AC)
}

/// `AbortSignal.timeout(ms)` — a signal that auto-aborts after `ms`.
///
/// Schedules a callback timer (drained on the main thread by
/// `js_callback_timer_tick`) that marks the signal aborted with a
/// `TimeoutError` and fires its listeners. The timer is `unref`'d so a pending
/// timeout signal does not by itself keep the event loop alive (Node behavior);
/// it still fires while any other ref'd work (e.g. an in-flight `fetch`) keeps
/// the loop running. Previously this returned a never-aborting stub, so
/// `fetch(url, { signal: AbortSignal.timeout(ms) })` could hang forever on a
/// slow/held response instead of timing out.
#[no_mangle]
pub extern "C" fn js_abort_signal_timeout(ms: f64) -> *mut ObjectHeader {
    let _gc = crate::gc::GcSuppressScope::new();
    let signal = alloc_abort_signal();
    let func = crate::fn_info!(abort_signal_timeout_fire, 0; with_declared(0));
    let closure = crate::closure::js_closure_alloc(func, 1);
    let signal_value = crate::value::js_nanbox_pointer(signal as i64);
    crate::closure::js_closure_set_capture_ptr(closure, 0, signal_value.to_bits() as i64);
    let timer_id = crate::timer::js_set_timeout_callback(closure as i64, ms);
    crate::timer::js_timer_unref(timer_id);
    signal
}

/// Mark a signal as aborted with `reason` and fire its listeners. Idempotent:
/// re-aborting an already-aborted signal is a no-op (Node behavior). Shared by
/// `AbortSignal.abort()` and the `AbortSignal.any()` propagation listener.
fn abort_signal_set_aborted(signal: *mut ObjectHeader, reason: f64) {
    if signal.is_null() || abort_signal_is_aborted(signal) {
        return;
    }
    set_abort_slot(signal, SIGNAL_ABORTED, f64::from_bits(TAG_TRUE_AC));
    set_abort_slot(signal, SIGNAL_REASON, reason);
    fire_abort_listeners(signal);
}

/// `AbortSignal.abort(reason?)` — returns an already-aborted signal whose
/// `.reason` is `reason` (or an `AbortError` when omitted, matching Node).
#[no_mangle]
pub extern "C" fn js_abort_signal_abort(reason: f64) -> *mut ObjectHeader {
    let _gc = crate::gc::GcSuppressScope::new();
    let signal = alloc_abort_signal();
    let reason_bits = reason.to_bits();
    // Node defaults the reason to an AbortError when none is supplied.
    let effective = if reason_bits == TAG_UNDEFINED_AC {
        crate::event_target::abort_dom_exception_value()
    } else {
        reason
    };
    set_abort_slot(signal, SIGNAL_ABORTED, f64::from_bits(TAG_TRUE_AC));
    set_abort_slot(signal, SIGNAL_REASON, effective);
    signal
}

/// `abortSignal.throwIfAborted()` — throws `abortSignal.reason` when the
/// signal is aborted, otherwise returns undefined (no-op).
#[no_mangle]
pub extern "C" fn js_abort_signal_throw_if_aborted(signal: *mut ObjectHeader) -> f64 {
    if abort_signal_is_aborted(signal) {
        let reason = get_abort_slot(signal, SIGNAL_REASON);
        // Node throws the stored reason verbatim (which is the AbortError
        // default when none was provided).
        crate::exception::js_throw(reason);
    }
    f64::from_bits(TAG_UNDEFINED_AC)
}

extern "C" fn abort_any_propagate_thunk(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
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
        get_abort_slot(source, SIGNAL_REASON)
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
    if signals_arr.is_null() {
        return alloc_abort_signal();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let sources = scope.root_raw_mut_ptr(signals_arr);
    let combined = scope.root_raw_mut_ptr(alloc_abort_signal());
    let (_, result) = combined.across_mut(|| {
        let len = sources.with_const_ptr(|ptr| crate::array::js_array_length(ptr));
        for i in 0..len {
            let elem = scope.root_nanbox_f64(
                sources.with_const_ptr(|ptr| crate::array::js_array_get_f64(ptr, i)),
            );
            let Some(source) = abort_signal_ptr_from_value(elem.get_nanbox_f64()) else {
                continue;
            };
            if abort_signal_is_aborted(source) {
                let reason = get_abort_slot(source, SIGNAL_REASON);
                combined.with_mut_ptr(|ptr| abort_signal_set_aborted(ptr, reason));
                break;
            }
            let closure = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
                crate::fn_info!(abort_any_propagate_thunk, 1; with_declared(1)),
                2,
            ));
            closure.with_mut_ptr(|closure| {
                combined.with_mut_ptr(|ptr| {
                    crate::closure::js_closure_set_capture_ptr(
                        closure,
                        0,
                        nanbox_pointer_ac(ptr).to_bits() as i64,
                    );
                });
                crate::closure::js_closure_set_capture_ptr(
                    closure,
                    1,
                    elem.get_nanbox_f64().to_bits() as i64,
                );
            });
            let abort_evt = create_string_f64("abort");
            let listener = closure.with_mut_ptr::<crate::closure::ClosureHeader, _>(|ptr| {
                crate::value::js_nanbox_pointer(ptr as i64)
            });
            let source = abort_signal_ptr_from_value(elem.get_nanbox_f64()).unwrap();
            js_abort_signal_add_listener(source, abort_evt, listener);
        }
    });
    result
}

// #2582: keepalive anchors so the auto-optimize whole-program LLVM bitcode
// rebuild doesn't internalize + dead-strip these codegen-only `#[no_mangle]`
// entry points (see project_auto_optimize_keepalive_3320). These are only
// referenced from generated `.o`, so without `#[used]` they vanish.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_ABORT_SIGNAL_ABORT: extern "C" fn(f64) -> *mut ObjectHeader = js_abort_signal_abort;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_ABORT_SIGNAL_ANY: extern "C" fn(*mut crate::array::ArrayHeader) -> *mut ObjectHeader =
    js_abort_signal_any;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_ABORT_SIGNAL_THROW_IF_ABORTED: extern "C" fn(*mut ObjectHeader) -> f64 =
    js_abort_signal_throw_if_aborted;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_ptr_rejects_pointer_tagged_handle() {
        let value = f64::from_bits(POINTER_TAG_AC | 5);
        assert!(js_abort_signal_resolve_ptr(value).is_null());
    }
}
