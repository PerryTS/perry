use super::*;

/// `js_class_method_bind` retains the name pointer in the closure, so make a
/// non-static forwarded property slice impossible to pass accidentally.
unsafe fn bind_static_handle_method(handle: i64, method: &'static [u8]) -> f64 {
    extern "C" {
        fn js_class_method_bind(
            instance: f64,
            method_name_ptr: *const u8,
            method_name_len: usize,
        ) -> f64;
    }
    js_class_method_bind(nanbox_handle_value(handle), method.as_ptr(), method.len())
}

#[cfg(any(feature = "bundled-events", feature = "external-events-construct"))]
fn event_emitter_method_name_static(property: &str) -> Option<&'static [u8]> {
    match property {
        "on" => Some(b"on"),
        "addListener" => Some(b"addListener"),
        "once" => Some(b"once"),
        "prependListener" => Some(b"prependListener"),
        "prependOnceListener" => Some(b"prependOnceListener"),
        "off" => Some(b"off"),
        "removeListener" => Some(b"removeListener"),
        "removeAllListeners" => Some(b"removeAllListeners"),
        "emit" => Some(b"emit"),
        "listenerCount" => Some(b"listenerCount"),
        "listeners" => Some(b"listeners"),
        "rawListeners" => Some(b"rawListeners"),
        "eventNames" => Some(b"eventNames"),
        "setMaxListeners" => Some(b"setMaxListeners"),
        "getMaxListeners" => Some(b"getMaxListeners"),
        _ => None,
    }
}

fn async_local_storage_method_name_static(property: &str) -> Option<&'static [u8]> {
    match property {
        "run" => Some(b"run"),
        "getStore" => Some(b"getStore"),
        "enterWith" => Some(b"enterWith"),
        "exit" => Some(b"exit"),
        "disable" => Some(b"disable"),
        _ => None,
    }
}

extern "C" fn async_local_storage_unbound_method_thunk(
    closure: *const perry_runtime::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    unsafe {
        let name_ptr = perry_runtime::closure::js_closure_get_capture_ptr(closure, 0) as *const i8;
        let name_len = perry_runtime::closure::js_closure_get_capture_ptr(closure, 1) as usize;
        let name = std::slice::from_raw_parts(name_ptr as *const u8, name_len);
        let name_str = std::str::from_utf8(name).unwrap_or("");
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let rest = scope.root_nanbox_f64(rest);
        let receiver_handle = scope.root_nanbox_f64(perry_runtime::object::js_implicit_this_get());
        let receiver = receiver_handle.get_nanbox_f64();
        let receiver_raw = if receiver.to_bits() >> 48 == 0x7FFD {
            (receiver.to_bits() & POINTER_MASK_BITS) as i64
        } else {
            0
        };

        // Node deliberately makes these two operations harmless when their
        // method value is invoked without an ALS receiver.
        if matches!(name, b"enterWith" | b"disable")
            && crate::async_local_storage::resolve_async_local_storage_handle(receiver_raw)
                .is_none()
        {
            return TAG_UNDEFINED_F64;
        }

        let args_array = perry_runtime::value::js_nanbox_get_pointer(rest.get_nanbox_f64())
            as *const perry_runtime::ArrayHeader;
        let args = if args_array.is_null() {
            Vec::new()
        } else {
            let len = perry_runtime::array::js_array_length(args_array) as usize;
            (0..len)
                .map(|index| {
                    f64::from_bits(
                        perry_runtime::array::js_array_get(args_array, index as u32).bits(),
                    )
                })
                .collect::<Vec<_>>()
        };
        if let Some(value) = dispatch_async_local_storage_method(receiver_raw, name_str, &args) {
            return value;
        }
        let receiver = receiver_handle.get_nanbox_f64();
        let receiver_raw = if receiver.to_bits() >> 48 == 0x7FFD {
            (receiver.to_bits() & POINTER_MASK_BITS) as i64
        } else {
            0
        };

        // The remaining cases are invalid receivers. Brand-checked methods
        // throw; enterWith/disable already returned the deliberate no-op above.
        match name {
            b"getStore" => {
                crate::async_local_storage::js_async_local_storage_get_store(receiver_raw)
            }
            b"run" => crate::async_local_storage::js_async_local_storage_run(
                receiver_raw,
                args.first().copied().unwrap_or(TAG_UNDEFINED_F64),
                args.get(1).copied().unwrap_or(TAG_UNDEFINED_F64),
                0,
            ),
            b"exit" => crate::async_local_storage::js_async_local_storage_exit(
                receiver_raw,
                args.first().copied().unwrap_or(TAG_UNDEFINED_F64),
                0,
            ),
            _ => TAG_UNDEFINED_F64,
        }
    }
}

pub(crate) fn unbound_async_local_storage_method(method: &'static [u8]) -> f64 {
    perry_runtime::closure::js_register_closure_rest(
        async_local_storage_unbound_method_thunk as *const u8,
        0,
    );
    let closure = perry_runtime::closure::js_closure_alloc(
        async_local_storage_unbound_method_thunk as *const u8,
        2,
    );
    if closure.is_null() {
        return TAG_UNDEFINED_F64;
    }
    perry_runtime::closure::js_closure_set_capture_ptr(closure, 0, method.as_ptr() as i64);
    perry_runtime::closure::js_closure_set_capture_ptr(closure, 1, method.len() as i64);
    perry_runtime::value::js_nanbox_pointer(closure as i64)
}

/// Dynamic dispatch for `AsyncLocalStorage` receivers whose static type the
/// codegen lost (`any`-typed bindings, closure captures). Gated on registry
/// type membership so no other subsystem's handle is claimed (#788).
pub(crate) unsafe fn dispatch_async_local_storage_method(
    handle: i64,
    method: &str,
    args: &[f64],
) -> Option<f64> {
    if !matches!(
        method,
        "run" | "getStore" | "enterWith" | "exit" | "disable"
    ) {
        return None;
    }
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let arg_handles = scope.root_nanbox_f64_slice(args);
    let handle = crate::async_local_storage::resolve_async_local_storage_handle(handle)?;
    let args = perry_runtime::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    Some(match method {
        "getStore" => crate::async_local_storage::js_async_local_storage_get_store(handle),
        "run" if args.len() >= 2 => {
            let rest = if args.len() > 2 { &args[2..] } else { &[] };
            let rest_array = if rest.is_empty() {
                0
            } else {
                pack_args_array(rest) as i64
            };
            let args =
                perry_runtime::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
            crate::async_local_storage::js_async_local_storage_run(
                handle, args[0], args[1], rest_array,
            )
        }
        "enterWith" => {
            let store = args.first().copied().unwrap_or(TAG_UNDEFINED_F64);
            crate::async_local_storage::js_async_local_storage_enter_with(handle, store);
            TAG_UNDEFINED_F64
        }
        "exit" if !args.is_empty() => {
            let rest = if args.len() > 1 { &args[1..] } else { &[] };
            let rest_array = if rest.is_empty() {
                0
            } else {
                pack_args_array(rest) as i64
            };
            let args =
                perry_runtime::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
            crate::async_local_storage::js_async_local_storage_exit(handle, args[0], rest_array)
        }
        "disable" => {
            crate::async_local_storage::js_async_local_storage_disable(handle);
            TAG_UNDEFINED_F64
        }
        _ => return None,
    })
}

#[cfg(any(feature = "bundled-events", feature = "external-events-construct"))]
pub(crate) unsafe fn dispatch_event_emitter_method(
    handle: i64,
    method: &str,
    args: &[f64],
) -> Option<f64> {
    if !js_event_emitter_is_handle(handle) {
        return None;
    }

    let event_bits = |index: usize| {
        args.get(index)
            .copied()
            .unwrap_or(TAG_UNDEFINED_F64)
            .to_bits() as i64
    };
    let nanbox_array = |ptr: *mut perry_runtime::ArrayHeader| {
        f64::from_bits(POINTER_TAG_BITS | (ptr as u64 & POINTER_MASK_BITS))
    };

    if perry_runtime::object::event_emitter_async_resource_handle_probe()
        .is_some_and(|probe| probe(handle))
    {
        let operation = match method {
            "asyncId" => Some(0),
            "triggerAsyncId" => Some(1),
            "asyncResource" => Some(2),
            "emitDestroy" => Some(3),
            _ => None,
        };
        if let (Some(operation), Some(dispatch)) = (
            operation,
            perry_runtime::object::event_emitter_async_resource_dispatch(),
        ) {
            return Some(dispatch(handle, operation));
        }
    }

    let value = match method {
        "on" | "addListener" if args.len() >= 2 => {
            js_event_emitter_on(handle, event_bits(0), event_bits(1));
            nanbox_handle_value(handle)
        }
        "once" if args.len() >= 2 => {
            js_event_emitter_once(handle, event_bits(0), event_bits(1));
            nanbox_handle_value(handle)
        }
        "prependListener" if args.len() >= 2 => {
            js_event_emitter_prepend_listener(handle, event_bits(0), event_bits(1));
            nanbox_handle_value(handle)
        }
        "prependOnceListener" if args.len() >= 2 => {
            js_event_emitter_prepend_once_listener(handle, event_bits(0), event_bits(1));
            nanbox_handle_value(handle)
        }
        "off" | "removeListener" if args.len() >= 2 => {
            js_event_emitter_remove_listener(handle, event_bits(0), event_bits(1));
            nanbox_handle_value(handle)
        }
        "removeAllListeners" => {
            js_event_emitter_remove_all_listeners(handle, pack_args_array(args));
            nanbox_handle_value(handle)
        }
        "emit" => {
            let rest = if args.len() > 1 { &args[1..] } else { &[] };
            js_event_emitter_emit(handle, event_bits(0), pack_args_array(rest))
        }
        "listenerCount" if !args.is_empty() => js_event_emitter_listener_count(
            handle,
            event_bits(0),
            args.get(1)
                .copied()
                .map(|value| value.to_bits() as i64)
                .unwrap_or(TAG_UNDEFINED_BITS),
        ),
        "listeners" if !args.is_empty() => {
            nanbox_array(js_event_emitter_listeners(handle, event_bits(0)))
        }
        "rawListeners" if !args.is_empty() => {
            nanbox_array(js_event_emitter_raw_listeners(handle, event_bits(0)))
        }
        "eventNames" => nanbox_array(js_event_emitter_event_names(handle)),
        "setMaxListeners" if !args.is_empty() => {
            js_event_emitter_set_max_listeners(handle, args[0]);
            nanbox_handle_value(handle)
        }
        "getMaxListeners" => js_event_emitter_get_max_listeners(handle),
        "domain" => js_event_emitter_domain_value(handle),
        _ => return None,
    };
    Some(value)
}

#[cfg(any(feature = "bundled-events", feature = "external-events-construct"))]
pub(crate) unsafe fn dispatch_event_emitter_property(handle: i64, property: &str) -> Option<f64> {
    if !js_event_emitter_is_handle(handle) {
        return None;
    }

    if perry_runtime::object::event_emitter_async_resource_handle_probe()
        .is_some_and(|probe| probe(handle))
    {
        if property == "emitDestroy" {
            return Some(bind_static_handle_method(handle, b"emitDestroy"));
        }
        let operation = match property {
            "asyncId" => Some(0),
            "triggerAsyncId" => Some(1),
            "asyncResource" => Some(2),
            _ => None,
        };
        if let (Some(operation), Some(dispatch)) = (
            operation,
            perry_runtime::object::event_emitter_async_resource_dispatch(),
        ) {
            return Some(dispatch(handle, operation));
        }
    }

    let method = event_emitter_method_name_static(property)?;

    Some(bind_static_handle_method(handle, method))
}

/// `AsyncLocalStorage` METHOD-VALUE reads (the property-read counterpart of
/// `dispatch_async_local_storage_method`). `als.getStore()` (a direct call)
/// already dispatched, but reading `als.getStore` AS A VALUE (`const gs =
/// als.getStore`, `{ getStore } = als`, `typeof als.getStore`) returned
/// `undefined` — there was no property-read dispatch for ALS handles (only
/// EventEmitter had one, #4995). Next.js' server startup reads `getStore` as a
/// value (cacheComponents / patch-fetch async-storage setup) and then calls it,
/// so it threw `TypeError: getStore is not a function` BEFORE `✓ Ready`. Bind
/// each method to the handle so the read yields a callable bound method, exactly
/// like `dispatch_event_emitter_property`.
pub(crate) unsafe fn dispatch_async_local_storage_property(
    handle: i64,
    property: &str,
) -> Option<f64> {
    let method = async_local_storage_method_name_static(property)?;
    crate::async_local_storage::resolve_async_local_storage_handle(handle)?;
    Some(unbound_async_local_storage_method(method))
}

#[cfg(test)]
mod static_method_name_tests {
    use super::*;

    fn assert_static_lookup(lookup: fn(&str) -> Option<&'static [u8]>, names: &[&str]) {
        for name in names {
            let owned = (*name).to_owned();
            let found = lookup(&owned).expect("known method must resolve");
            assert_eq!(found, name.as_bytes());
            assert_ne!(
                found.as_ptr(),
                owned.as_ptr(),
                "lookup borrowed the forwarded property name for {name}"
            );
            assert_eq!(found.as_ptr(), lookup(name).unwrap().as_ptr());
        }
        assert!(lookup("notAHandleMethod").is_none());
    }

    #[test]
    fn async_local_storage_method_name_lookup_returns_static_literals() {
        assert_static_lookup(
            async_local_storage_method_name_static,
            &["run", "getStore", "enterWith", "exit", "disable"],
        );
    }

    #[cfg(any(feature = "bundled-events", feature = "external-events-construct"))]
    #[test]
    fn event_emitter_method_name_lookup_returns_static_literals() {
        assert_static_lookup(
            event_emitter_method_name_static,
            &[
                "on",
                "addListener",
                "once",
                "prependListener",
                "prependOnceListener",
                "off",
                "removeListener",
                "removeAllListeners",
                "emit",
                "listenerCount",
                "listeners",
                "rawListeners",
                "eventNames",
                "setMaxListeners",
                "getMaxListeners",
            ],
        );
    }
}
