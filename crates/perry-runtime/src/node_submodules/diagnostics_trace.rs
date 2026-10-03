//! `diagnostics_channel` TracingChannel methods: event-name/channel lookup,
//! `subscribe`/`unsubscribe`, and `traceSync`/`tracePromise`/`traceCallback`.
//! Split out of `diagnostics.rs` to stay under the 2,000-line cap (#10750).

use super::*;

pub(crate) fn tracing_event_name(base: f64, event: &str) -> f64 {
    let base = decode_string_value(base).unwrap_or_else(|| "unknown".to_string());
    let s = format!("tracing:{base}:{event}");
    let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(crate::value::STRING_TAG | (ptr as u64 & crate::value::POINTER_MASK))
}

pub(crate) fn channel_from_object_property(obj_value: f64, prop: &str) -> i64 {
    let obj = crate::value::js_nanbox_get_pointer(obj_value) as *mut ObjectHeader;
    if obj.is_null() {
        throw_invalid_arg();
    }
    let ch_value = get_field_value(obj, prop);
    if ch_value.to_bits() == crate::value::TAG_UNDEFINED {
        throw_type_error_no_code(b"Invalid channel object");
    }
    let ch_obj = crate::value::js_nanbox_get_pointer(ch_value) as *mut ObjectHeader;
    if ch_obj.is_null() {
        throw_invalid_arg();
    }
    DIAG_CHANNELS.with(|m| {
        for (id, state) in m.borrow().iter() {
            if state.obj == ch_obj {
                return *id;
            }
        }
        throw_invalid_arg()
    })
}

pub(crate) extern "C" fn diag_trace_subscribe(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    handlers: f64,
) -> f64 {
    let id = method_id(closure);
    let events = DIAG_TRACES.with(|m| m.borrow().get(&id).map(|t| t.events).unwrap_or([0; 5]));
    for (idx, name) in ["start", "end", "asyncStart", "asyncEnd", "error"]
        .iter()
        .enumerate()
    {
        let h = get_field_value(
            crate::value::js_nanbox_get_pointer(handlers) as *mut ObjectHeader,
            name,
        );
        // Absent keys (undefined OR null) are silently skipped — Node
        // only requires that *present-and-defined* handler values be
        // callable. Anything else present throws ERR_INVALID_ARG_TYPE.
        let h_bits = h.to_bits();
        if h_bits == TAG_UNDEFINED || h_bits == crate::value::TAG_NULL {
            continue;
        }
        if !valid_closure_value(h) {
            throw_invalid_arg();
        }
        add_subscriber(events[idx], h);
    }
    undefined()
}

pub(crate) extern "C" fn diag_trace_unsubscribe(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    handlers: f64,
) -> f64 {
    let id = method_id(closure);
    let events = DIAG_TRACES.with(|m| m.borrow().get(&id).map(|t| t.events).unwrap_or([0; 5]));
    let mut ok = true;
    for (idx, name) in ["start", "end", "asyncStart", "asyncEnd", "error"]
        .iter()
        .enumerate()
    {
        let h = get_field_value(
            crate::value::js_nanbox_get_pointer(handlers) as *mut ObjectHeader,
            name,
        );
        if valid_closure_value(h) && !remove_subscriber(events[idx], h) {
            ok = false;
        }
    }
    bool_value(ok)
}

pub(crate) fn call_fn_value(fn_value: f64, this_arg: f64, args: &[f64]) -> f64 {
    if !valid_closure_value(fn_value) {
        crate::closure::throw_not_callable();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    // The rebind clone allocates, so the receiver is re-read from a root.
    let this_arg_handle = scope.root_nanbox_f64(this_arg);
    let rebound = crate::closure::clone_closure_rebind_this(fn_value.to_bits(), this_arg);
    let cb = (rebound & crate::value::POINTER_MASK) as *const ClosureHeader;
    unsafe {
        crate::closure::js_closure_call_array(
            cb as i64,
            crate::closure::JsThis::from_f64(this_arg_handle.get_nanbox_f64()),
            args.as_ptr(),
            args.len() as i64,
        )
    }
}

pub(crate) extern "C" fn diag_trace_sync(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    fn_value: f64,
    context: f64,
    this_arg: f64,
    arg: f64,
) -> f64 {
    let events = DIAG_TRACES.with(|m| {
        m.borrow()
            .get(&method_id(closure))
            .map(|t| t.events)
            .unwrap_or([0; 5])
    });
    let active = DIAG_TRACES.with(|m| {
        m.borrow()
            .get(&method_id(closure))
            .map(|t| get_field_value(t.obj, "hasSubscribers").to_bits() == TAG_TRUE)
            .unwrap_or(false)
    });
    if !active {
        return call_fn_value(fn_value, this_arg, &[arg]);
    }
    publish_channel(events[0], context);
    match catch_js(|| call_fn_value(fn_value, this_arg, &[arg])) {
        Ok(result) => {
            set_field_value(
                crate::value::js_nanbox_get_pointer(context) as *mut ObjectHeader,
                "result",
                result,
            );
            publish_channel(events[1], context);
            result
        }
        Err(err) => {
            set_field_value(
                crate::value::js_nanbox_get_pointer(context) as *mut ObjectHeader,
                "error",
                err,
            );
            publish_channel(events[4], context);
            publish_channel(events[1], context);
            crate::exception::js_throw(err)
        }
    }
}

pub(crate) extern "C" fn diag_trace_promise(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    fn_value: f64,
    context: f64,
    this_arg: f64,
) -> f64 {
    let events = DIAG_TRACES.with(|m| {
        m.borrow()
            .get(&method_id(closure))
            .map(|t| t.events)
            .unwrap_or([0; 5])
    });
    publish_channel(events[0], context);
    let result = call_fn_value(fn_value, this_arg, &[]);
    let result_ptr = crate::value::js_nanbox_get_pointer(result) as *mut crate::promise::Promise;
    if !result_ptr.is_null()
        && (result.to_bits() & crate::value::TAG_MASK) == crate::value::POINTER_TAG
    {
        let _ = crate::promise::js_promise_run_microtasks();
        let state = crate::promise::js_promise_state(result_ptr);
        if state == 1 {
            publish_channel(events[1], context);
            let value = crate::promise::js_promise_value(result_ptr);
            set_field_value(
                crate::value::js_nanbox_get_pointer(context) as *mut ObjectHeader,
                "result",
                value,
            );
            publish_channel(events[2], context);
            publish_channel(events[3], context);
            result
        } else if state == 2 {
            // Node's TracingChannel#tracePromise rejection order is
            // start, end, error, asyncStart, asyncEnd — confirmed
            // against `node --experimental-strip-types` 24.x. The
            // earlier handwritten ordering shipped here matches that
            // sequence after the `end` publish.
            publish_channel(events[1], context);
            let reason = crate::promise::js_promise_reason(result_ptr);
            set_field_value(
                crate::value::js_nanbox_get_pointer(context) as *mut ObjectHeader,
                "error",
                reason,
            );
            publish_channel(events[4], context);
            publish_channel(events[2], context);
            publish_channel(events[3], context);
            result
        } else {
            publish_channel(events[1], context);
            set_field_value(
                crate::value::js_nanbox_get_pointer(context) as *mut ObjectHeader,
                "result",
                result,
            );
            publish_channel(events[2], context);
            publish_channel(events[3], context);
            result
        }
    } else {
        // Node publishes only start/end for non-thenable returns, sets
        // context.result, returns the plain value, and emits a process warning.
        // The parity runner normalizes the warning away, so preserve the
        // observable channel ordering and return/context shape here.
        set_field_value(
            crate::value::js_nanbox_get_pointer(context) as *mut ObjectHeader,
            "result",
            result,
        );
        publish_channel(events[1], context);
        result
    }
}

/// The callback installed in place of the user's callback by
/// `traceCallback`. Its body is a synthetic-arguments body so it forwards
/// every argument the traced function passes (`cb(null, "a", "b")`) to the
/// user callback, matching Node's `ReflectApply(callback, this, arguments)`
/// (#3086). `arguments[0]` is `err`, `arguments[1]` is `res`.
pub(crate) extern "C" fn diag_trace_wrapped_callback(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    all_args: f64,
) -> f64 {
    let callback = js_closure_get_capture_f64(closure, 0);
    let context = js_closure_get_capture_f64(closure, 1);
    let async_start = js_closure_get_capture_ptr(closure, 2);
    let async_end = js_closure_get_capture_ptr(closure, 3);
    let error = js_closure_get_capture_ptr(closure, 4);
    let context_obj = crate::value::js_nanbox_get_pointer(context) as *mut ObjectHeader;

    let cb_args = unbox_arg_array(all_args);
    let err = cb_args.first().copied().unwrap_or_else(undefined);
    let res = cb_args.get(1).copied().unwrap_or_else(undefined);

    if crate::value::js_is_truthy(err) != 0 {
        set_field_value(context_obj, "error", err);
        publish_channel(error, context);
    } else {
        set_field_value(context_obj, "result", res);
    }

    publish_channel(async_start, context);
    let result = match catch_js(|| call_fn_value(callback, undefined(), &cb_args)) {
        Ok(result) => result,
        Err(callback_err) => {
            publish_channel(async_end, context);
            crate::exception::js_throw(callback_err)
        }
    };
    publish_channel(async_end, context);
    result
}

/// Throw the Node `ERR_INVALID_ARG_TYPE` for a non-function callback at the
/// requested `traceCallback` position (#3086).
fn throw_trace_callback_not_function() -> ! {
    let msg = b"The \"callback\" argument must be of type function.";
    let s = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    register_error_code(s, "ERR_INVALID_ARG_TYPE");
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(boxed_ptr(err))
}

pub(crate) extern "C" fn diag_trace_callback(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    all_args: f64,
) -> f64 {
    // Synthetic-arguments rest array: [fn, position, context, thisArg, ...args]
    // (#3086). `position` defaults to -1 (the last trailing arg), the callback
    // lives at `args[position]`, and the wrapped callback is spliced in at that
    // position while all surrounding args are preserved.
    let all = unbox_arg_array(all_args);
    let undef = undefined();
    let fn_value = all.first().copied().unwrap_or(undef);
    let position_value = all.get(1).copied().unwrap_or(undef);
    let context = all.get(2).copied().unwrap_or(undef);
    let this_arg = all.get(3).copied().unwrap_or(undef);
    let mut args: Vec<f64> = if all.len() > 4 {
        all[4..].to_vec()
    } else {
        Vec::new()
    };

    // `position` defaults to -1. Resolve to a forward index using
    // Array.prototype.at semantics (negative counts from the end). `position`
    // arrives NaN-boxed (int32 or f64), so decode it as a JS number.
    let position = if position_value.to_bits() == TAG_UNDEFINED {
        -1i64
    } else {
        JSValue::from_bits(position_value.to_bits()).to_number() as i64
    };
    let idx = if position < 0 {
        args.len() as i64 + position
    } else {
        position
    };
    let cb_index = if idx >= 0 && (idx as usize) < args.len() {
        Some(idx as usize)
    } else {
        None
    };
    let callback = cb_index.map(|i| args[i]).unwrap_or(undef);

    let events = DIAG_TRACES.with(|m| {
        m.borrow()
            .get(&method_id(closure))
            .map(|t| t.events)
            .unwrap_or([0; 5])
    });
    let active = DIAG_TRACES.with(|m| {
        m.borrow()
            .get(&method_id(closure))
            .map(|t| get_field_value(t.obj, "hasSubscribers").to_bits() == TAG_TRUE)
            .unwrap_or(false)
    });
    if !active {
        // Inactive fast path: forward every argument verbatim, no callback
        // validation and no wrapping (matches Node's `ReflectApply(fn,
        // thisArg, args)`).
        return call_fn_value(fn_value, this_arg, &args);
    }

    // Active path validates the resolved callback (Node's
    // `validateFunction(callback, 'callback')`).
    if !valid_closure_value(callback) {
        throw_trace_callback_not_function();
    }

    let wrapped = js_closure_alloc(
        crate::fn_info!(diag_trace_wrapped_callback, 1; with_rest_kind(0, crate::closure::FN_REST_SYNTHETIC_ARGUMENTS)),
        5,
    );
    js_closure_set_capture_f64(wrapped, 0, callback);
    js_closure_set_capture_f64(wrapped, 1, context);
    js_closure_set_capture_ptr(wrapped, 2, events[2]);
    js_closure_set_capture_ptr(wrapped, 3, events[3]);
    js_closure_set_capture_ptr(wrapped, 4, events[4]);

    let wrapped_value = boxed_ptr(wrapped);

    // Splice the wrapped callback in at the resolved position, preserving all
    // surrounding arguments (#3086).
    if let Some(i) = cb_index {
        args[i] = wrapped_value;
    }

    publish_channel(events[0], context);
    match catch_js(|| call_fn_value(fn_value, this_arg, &args)) {
        Ok(ret) => {
            publish_channel(events[1], context);
            ret
        }
        Err(err) => {
            set_field_value(
                crate::value::js_nanbox_get_pointer(context) as *mut ObjectHeader,
                "error",
                err,
            );
            publish_channel(events[4], context);
            publish_channel(events[1], context);
            crate::exception::js_throw(err)
        }
    }
}
