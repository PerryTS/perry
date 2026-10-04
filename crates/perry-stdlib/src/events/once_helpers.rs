//! `events.once(emitter, name[, options])`.
//!
//! node's algorithm: a `resolver` added with `emitter.once(name, ...)`, an
//! `errorListener` added with `emitter.once('error', ...)` (unless the event
//! is `'error'` itself), and an abort listener on `options.signal`. Whichever
//! fires first removes the others and settles the promise. Every reference a
//! listener holds (the promise, the emitter, its partner listeners, the
//! signal) is a NaN-boxed capture the collector traces and moves.

use super::*;

use perry_runtime::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64,
};
use perry_runtime::{
    js_array_alloc, js_array_push_f64, js_nanbox_get_pointer, js_nanbox_pointer, js_promise_new,
    js_promise_reject, js_promise_resolve, ClosureHeader, JSValue, ObjectHeader, Promise,
    StringHeader,
};

// Capture slots shared by the three emitter-side closures.
const CAP_PROMISE: u32 = 0;
const CAP_EMITTER: u32 = 1;
const CAP_EVENT: u32 = 2;
const CAP_RESOLVER: u32 = 3;
const CAP_ERROR_LISTENER: u32 = 4;
const CAP_SIGNAL: u32 = 5;
const CAP_ABORT_LISTENER: u32 = 6;
const CAPTURES: u32 = 7;

fn is_present(value: f64) -> bool {
    let value = JSValue::from_bits(value.to_bits());
    !value.is_undefined() && !value.is_null()
}

/// The closure's captures, rooted before anything allocates (the running
/// closure itself may move once a collection runs).
fn rooted_captures<'s>(
    scope: &'s perry_runtime::gc::RuntimeHandleScope,
    closure: *const ClosureHeader,
) -> Vec<perry_runtime::gc::RuntimeHandle<'s>> {
    let values: Vec<f64> = (0..CAPTURES)
        .map(|slot| js_closure_get_capture_f64(closure, slot))
        .collect();
    scope.root_nanbox_f64_slice(&values)
}

/// `emitter.removeListener(event, captures[slot])` when that listener exists.
unsafe fn remove_emitter_listener(
    captures: &[perry_runtime::gc::RuntimeHandle<'_>],
    event: f64,
    slot: u32,
) {
    let listener = captures[slot as usize].get_nanbox_f64();
    if is_present(listener) {
        let emitter = captures[CAP_EMITTER as usize].get_nanbox_f64();
        let _ = call_emitter_method(emitter, "removeListener", &[event, listener]);
    }
}

unsafe fn remove_signal_listener(captures: &[perry_runtime::gc::RuntimeHandle<'_>]) {
    let signal = captures[CAP_SIGNAL as usize].get_nanbox_f64();
    let abort_listener = captures[CAP_ABORT_LISTENER as usize].get_nanbox_f64();
    if is_present(signal) && is_present(abort_listener) {
        remove_abort_listener(signal, abort_listener);
    }
}

fn settle(captures: &[perry_runtime::gc::RuntimeHandle<'_>], value: f64, fulfil: bool) {
    let promise =
        js_nanbox_get_pointer(captures[CAP_PROMISE as usize].get_nanbox_f64()) as *mut Promise;
    if promise.is_null() {
        return;
    }
    if fulfil {
        js_promise_resolve(promise, value);
    } else {
        js_promise_reject(promise, value);
    }
}

/// node's `resolver(...args)`.
extern "C" fn events_once_resolver(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    rest: f64,
) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let rest = scope.root_nanbox_f64(rest);
    let captures = rooted_captures(&scope, closure);
    unsafe {
        let error_event = scope.root_nanbox_f64(name_value("error"));
        remove_emitter_listener(&captures, error_event.get_nanbox_f64(), CAP_ERROR_LISTENER);
        remove_signal_listener(&captures);
    }
    let args = if JSValue::from_bits(rest.get_nanbox_u64()).is_pointer() {
        rest.get_nanbox_f64()
    } else {
        js_nanbox_pointer(js_array_alloc(0) as i64)
    };
    settle(&captures, args, true);
    undefined_value()
}

/// node's `errorListener(err)`.
extern "C" fn events_once_error_listener(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    error: f64,
) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let error = scope.root_nanbox_f64(error);
    let captures = rooted_captures(&scope, closure);
    unsafe {
        let event = captures[CAP_EVENT as usize].get_nanbox_f64();
        remove_emitter_listener(&captures, event, CAP_RESOLVER);
        remove_signal_listener(&captures);
    }
    settle(&captures, error.get_nanbox_f64(), false);
    undefined_value()
}

/// node's `abortListener()`.
extern "C" fn events_once_abort_listener(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let captures = rooted_captures(&scope, closure);
    unsafe {
        let event = captures[CAP_EVENT as usize].get_nanbox_f64();
        remove_emitter_listener(&captures, event, CAP_RESOLVER);
        let error_event = scope.root_nanbox_f64(name_value("error"));
        remove_emitter_listener(&captures, error_event.get_nanbox_f64(), CAP_ERROR_LISTENER);
    }
    let error = scope.root_nanbox_f64(perry_runtime::url::js_abort_error_value());
    settle(&captures, error.get_nanbox_f64(), false);
    undefined_value()
}

extern "C" fn events_once_event_target_listener(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    arg0: f64,
) -> f64 {
    use perry_runtime::closure::js_closure_get_capture_ptr;

    let promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let target = js_closure_get_capture_ptr(closure, 1) as *mut ObjectHeader;
    let event_name_ptr = js_closure_get_capture_ptr(closure, 2) as *const StringHeader;
    unsafe {
        if !target.is_null() && !event_name_ptr.is_null() {
            perry_runtime::event_target::js_event_target_remove_event_listener(
                target,
                event_name_ptr,
                closure as i64,
            );
        }
        if !promise.is_null() {
            let mut args = js_array_alloc(0);
            args = js_array_push_f64(args, arg0);
            js_promise_resolve(promise, js_nanbox_pointer(args as i64));
        }
    }
    undefined_value()
}

/// `events.once(emitter, eventName[, options])` — a Promise for the array of
/// arguments the next `emit(eventName, ...)` passes.
#[no_mangle]
pub unsafe extern "C" fn js_events_once(
    target_value: f64,
    event_name_ptr: *const StringHeader,
    options: f64,
) -> *mut Promise {
    use perry_runtime::closure::js_closure_set_capture_ptr;

    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let target_value = scope.root_nanbox_f64(target_value);
    let event_name_handle = scope.root_string_ptr(event_name_ptr);
    let options = scope.root_nanbox_f64(options);
    let promise = scope.root_raw_mut_ptr(js_promise_new());
    let target = match event_helper_target(target_value.get_nanbox_f64()) {
        Some(target) => target,
        None => {
            let error = invalid_arg_type_error(&invalid_instance_arg_message(
                "emitter",
                "EventEmitter",
                target_value.get_nanbox_f64(),
            ));
            js_promise_reject(promise.get_raw_mut_ptr(), error);
            return promise.get_raw_mut_ptr();
        }
    };
    let Some(event_name) = string_from_header(event_name_handle.get_raw_const_ptr()) else {
        return promise.get_raw_mut_ptr();
    };
    let signal = match options_signal_result(options.get_nanbox_f64()) {
        Ok(signal) => signal,
        Err(error) => {
            js_promise_reject(promise.get_raw_mut_ptr(), error);
            return promise.get_raw_mut_ptr();
        }
    };
    let signal = signal.map(|value| scope.root_nanbox_f64(value));
    if signal
        .as_ref()
        .is_some_and(|value| signal_is_aborted(value.get_nanbox_f64()))
    {
        let error = perry_runtime::url::js_abort_error_value();
        js_promise_reject(promise.get_raw_mut_ptr(), error);
        return promise.get_raw_mut_ptr();
    }

    if let EventHelperTarget::EventTarget(_) = target {
        let listener = js_closure_alloc(
            perry_runtime::fn_info!(events_once_event_target_listener, 1),
            3,
        );
        let listener = scope.root_raw_mut_ptr(listener);
        let target =
            object_ptr_from_value(target_value.get_nanbox_f64()).unwrap_or(std::ptr::null_mut());
        js_closure_set_capture_ptr(
            listener.get_raw_mut_ptr(),
            0,
            promise.get_raw_mut_ptr::<Promise>() as i64,
        );
        js_closure_set_capture_ptr(listener.get_raw_mut_ptr(), 1, target as i64);
        js_closure_set_capture_ptr(
            listener.get_raw_mut_ptr(),
            2,
            event_name_handle.get_raw_const_ptr::<StringHeader>() as i64,
        );
        perry_runtime::event_target::js_event_target_add_event_listener(
            target,
            event_name_handle.get_raw_const_ptr(),
            listener.get_raw_mut_ptr::<ClosureHeader>() as i64,
        );
        return promise.get_raw_mut_ptr();
    }

    // An emitter: all three closures first, then their shared captures.
    let event = scope.root_nanbox_f64(event_value(event_name_handle.get_raw_const_ptr()));
    let resolver = scope.root_raw_mut_ptr(js_closure_alloc(
        perry_runtime::fn_info!(events_once_resolver, 1; with_rest(0)),
        CAPTURES,
    ));
    let listens_for_error = event_name == "error";
    let error_listener = (!listens_for_error).then(|| {
        scope.root_raw_mut_ptr(js_closure_alloc(
            perry_runtime::fn_info!(events_once_error_listener, 1),
            CAPTURES,
        ))
    });
    let abort_listener = signal.as_ref().map(|_| {
        scope.root_raw_mut_ptr(js_closure_alloc(
            perry_runtime::fn_info!(events_once_abort_listener, 0),
            CAPTURES,
        ))
    });
    let boxed = |closure: &perry_runtime::gc::RuntimeHandle<'_>| {
        js_nanbox_pointer(closure.get_raw_mut_ptr::<ClosureHeader>() as i64)
    };
    let mut closures = vec![&resolver];
    closures.extend(error_listener.iter());
    closures.extend(abort_listener.iter());
    for closure in &closures {
        let slots = [
            (
                CAP_PROMISE,
                js_nanbox_pointer(promise.get_raw_mut_ptr::<Promise>() as i64),
            ),
            (CAP_EMITTER, target_value.get_nanbox_f64()),
            (CAP_EVENT, event.get_nanbox_f64()),
            (CAP_RESOLVER, boxed(&resolver)),
            (
                CAP_ERROR_LISTENER,
                error_listener
                    .as_ref()
                    .map(boxed)
                    .unwrap_or_else(undefined_value),
            ),
            (
                CAP_SIGNAL,
                signal
                    .as_ref()
                    .map(|signal| signal.get_nanbox_f64())
                    .unwrap_or_else(undefined_value),
            ),
            (
                CAP_ABORT_LISTENER,
                abort_listener
                    .as_ref()
                    .map(boxed)
                    .unwrap_or_else(undefined_value),
            ),
        ];
        for (slot, value) in slots {
            js_closure_set_capture_f64(closure.get_raw_mut_ptr(), slot, value);
        }
    }

    let _ = call_emitter_method(
        target_value.get_nanbox_f64(),
        "once",
        &[event.get_nanbox_f64(), boxed(&resolver)],
    );
    if let Some(error_listener) = &error_listener {
        let error_event = scope.root_nanbox_f64(name_value("error"));
        let _ = call_emitter_method(
            target_value.get_nanbox_f64(),
            "once",
            &[error_event.get_nanbox_f64(), boxed(error_listener)],
        );
    }
    if let (Some(signal), Some(abort_listener)) = (&signal, &abort_listener) {
        let abort_event = scope.root_nanbox_f64(abort_event_value());
        if let Some(signal_ptr) = object_ptr_from_value(signal.get_nanbox_f64()) {
            perry_runtime::url::js_abort_signal_add_listener(
                signal_ptr,
                abort_event.get_nanbox_f64(),
                boxed(abort_listener),
            );
        }
    }
    promise.get_raw_mut_ptr()
}
