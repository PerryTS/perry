use std::sync::Once;

use crate::value::JSValue;

use super::{
    as_object_ptr, closure_ptr_from_value, collect_rest_args, entry_to_object, is_function_value,
    notify_observers, option_value, perf_now, str_value, string_of, throw_type_error,
    throw_type_error_with_code, PerfEntry, ENTRY_TYPE_FUNCTION,
};

static TIMERIFY_WRAPPER_REGISTERED: Once = Once::new();

unsafe fn function_value_name(value: f64) -> String {
    let Some(closure) = closure_ptr_from_value(value) else {
        return String::new();
    };
    crate::builtins::function_name_for_ptr((*closure).func_ptr as usize)
        .or_else(|| {
            let name_value = crate::closure::closure_get_dynamic_prop(closure as usize, "name");
            string_of(JSValue::from_bits(name_value.to_bits()))
        })
        .unwrap_or_default()
}

unsafe fn finish_timerify_entry(name_value: f64, start_time: f64, histogram: f64, detail: f64) {
    let duration = (perf_now() - start_time).max(0.0);
    // Node records timerify durations in nanoseconds. A sub-nanosecond call
    // still has to land in a bucket — the histogram's lowest trackable value
    // is 1 — or `histogram.count` would not move.
    crate::perf_histogram::record_timerify_duration(
        histogram,
        ((duration * 1.0e6).round() as i64).max(1),
    );
    let name = string_of(JSValue::from_bits(name_value.to_bits())).unwrap_or_default();
    let mut entry = PerfEntry {
        name,
        entry_type: ENTRY_TYPE_FUNCTION,
        start_time,
        duration,
        detail_bits: detail.to_bits(),
        object_bits: 0,
        initiator_type: None,
    };
    let obj = entry_to_object(&entry);
    entry.object_bits = obj.to_bits();
    notify_observers(&entry);
}

extern "C" fn perf_timerify_settle(
    closure: *const crate::closure::ClosureHeader,
    outcome: f64,
) -> f64 {
    unsafe {
        let name = crate::closure::js_closure_get_capture_f64(closure, 0);
        let start_time = crate::closure::js_closure_get_capture_f64(closure, 1);
        let histogram = crate::closure::js_closure_get_capture_f64(closure, 2);
        let detail = crate::closure::js_closure_get_capture_f64(closure, 3);
        finish_timerify_entry(name, start_time, histogram, detail);
    }
    // A settle listener is observational only; preserving the outcome keeps
    // the native promise's fulfillment/rejection untouched.
    outcome
}

extern "C" fn perf_timerify_wrapper(
    closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let wrapper_handle = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(closure as i64));
        let target_handle =
            scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
        let name_handle =
            scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 1));
        let histogram_handle =
            scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 2));
        let args = collect_rest_args(rest);
        // The detail array allocation may move any heap-valued argument. Root
        // each one first, then republish the post-collection values into both
        // the call buffer and the array.
        let arg_handles: Vec<_> = args.iter().map(|arg| scope.root_nanbox_f64(*arg)).collect();
        let detail = crate::array::js_array_alloc(arg_handles.len() as u32);
        (*detail).length = arg_handles.len() as u32;
        for (i, arg) in arg_handles.iter().enumerate() {
            crate::array::js_array_set_f64(detail, i as u32, arg.get_nanbox_f64());
        }
        let detail_handle = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(detail as i64));
        let call_args: Vec<f64> = arg_handles.iter().map(|arg| arg.get_nanbox_f64()).collect();

        let start_time = perf_now();
        let target = target_handle.get_nanbox_f64();
        let new_target = crate::object::js_new_target_get();
        let result = if new_target.to_bits() != crate::value::TAG_UNDEFINED {
            let wrapper_value = wrapper_handle.get_nanbox_f64();
            let forwarded_new_target = if new_target.to_bits() == wrapper_value.to_bits() {
                target
            } else {
                new_target
            };
            crate::object::js_new_function_construct_with_new_target(
                target,
                call_args.as_ptr(),
                call_args.len(),
                forwarded_new_target,
            )
        } else {
            // Declared classes have no [[Call]]. The generic native-value call
            // path treats Perry's compact class-ref representation as a no-op,
            // so reject it explicitly just as a direct class call does.
            if crate::object::class_ref_id(target).is_some() {
                throw_type_error("Class constructor cannot be invoked without 'new'");
            }
            crate::closure::js_native_call_value(target, call_args.as_ptr(), call_args.len())
        };
        let result_handle = scope.root_nanbox_f64(result);

        // Normalize only to discover thenables; return the original result.
        // Native promises pass through unchanged, while a user thenable gets a
        // native proxy promise whose settlement can use the same listener.
        let normalized = crate::promise::js_assimilate_thenable(result_handle.get_nanbox_f64());
        let normalized_handle = scope.root_nanbox_f64(normalized);
        if crate::promise::js_value_is_promise(normalized_handle.get_nanbox_f64()) != 0 {
            let listener = crate::closure::js_closure_alloc(perf_timerify_settle as *const u8, 4);
            crate::closure::js_closure_set_capture_f64(listener, 0, name_handle.get_nanbox_f64());
            crate::closure::js_closure_set_capture_f64(listener, 1, start_time);
            crate::closure::js_closure_set_capture_f64(
                listener,
                2,
                histogram_handle.get_nanbox_f64(),
            );
            crate::closure::js_closure_set_capture_f64(listener, 3, detail_handle.get_nanbox_f64());
            let promise = crate::value::js_nanbox_get_pointer(normalized_handle.get_nanbox_f64())
                as *mut crate::promise::Promise;
            crate::promise::js_promise_attach_settle_listener(promise, listener, listener);
        } else {
            finish_timerify_entry(
                name_handle.get_nanbox_f64(),
                start_time,
                histogram_handle.get_nanbox_f64(),
                detail_handle.get_nanbox_f64(),
            );
        }
        result_handle.get_nanbox_f64()
    }
}

#[no_mangle]
pub extern "C" fn js_perf_timerify(fn_value: f64, options: f64) -> f64 {
    unsafe {
        if !is_function_value(fn_value) {
            throw_type_error_with_code(
                "The \"fn\" argument must be of type function",
                "ERR_INVALID_ARG_TYPE",
            );
        }
        let mut histogram = f64::from_bits(JSValue::undefined().bits());
        if let Some(opts) = as_object_ptr(options) {
            let candidate = f64::from_bits(option_value(opts, "histogram").bits());
            if !JSValue::from_bits(candidate.to_bits()).is_undefined() {
                if crate::perf_histogram::histogram_id_from_value(candidate).is_none() {
                    throw_type_error_with_code(
                        "The \"options.histogram\" argument must be an instance of RecordableHistogram",
                        "ERR_INVALID_ARG_TYPE",
                    );
                }
                histogram = candidate;
            }
        }
        TIMERIFY_WRAPPER_REGISTERED.call_once(|| {
            crate::closure::js_register_closure_rest(perf_timerify_wrapper as *const u8, 0);
            crate::closure::js_register_closure_arity(perf_timerify_settle as *const u8, 1);
        });
        let name = function_value_name(fn_value);
        let closure = crate::closure::js_closure_alloc(perf_timerify_wrapper as *const u8, 3);
        crate::closure::js_closure_set_capture_f64(closure, 0, fn_value);
        let name_value = str_value(&name);
        crate::closure::js_closure_set_capture_f64(closure, 1, f64::from_bits(name_value.bits()));
        crate::closure::js_closure_set_capture_f64(closure, 2, histogram);

        if let Some(target) = closure_ptr_from_value(fn_value) {
            if let Some(length) = crate::closure::closure_length(target) {
                crate::object::set_builtin_closure_length(closure as usize, length);
            }
        }

        let wrapper_name = if name.is_empty() {
            "timerified".to_string()
        } else {
            format!("timerified {name}")
        };
        let wrapper_name_value = str_value(&wrapper_name);
        crate::closure::closure_set_dynamic_prop(
            closure as usize,
            "name",
            f64::from_bits(wrapper_name_value.bits()),
        );
        let attrs = crate::object::PropertyAttrs::new(false, true, false);
        crate::object::set_property_attrs(closure as usize, "name".to_string(), attrs);
        crate::object::set_property_attrs(closure as usize, "length".to_string(), attrs);
        crate::gc::runtime_write_barrier_root_heap_word(closure as u64);
        f64::from_bits(JSValue::pointer(closure as *mut u8).bits())
    }
}
