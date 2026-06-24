use super::super::handle::*;
use super::*;

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
    if get_handle_mut::<crate::async_local_storage::AsyncLocalStorageHandle>(handle).is_none() {
        return None;
    }
    Some(match method {
        "getStore" => crate::async_local_storage::js_async_local_storage_get_store(handle),
        "run" if args.len() >= 2 => {
            let rest = if args.len() > 2 { &args[2..] } else { &[] };
            let rest_array = if rest.is_empty() {
                0
            } else {
                pack_args_array(rest) as i64
            };
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

    // EventEmitterAsyncResource extras exist only in the bundled impl;
    // perry-ext-events has no async-resource constructor, so its handles
    // never satisfy this probe.
    #[cfg(feature = "bundled-events")]
    if crate::events::is_event_emitter_async_resource_handle(handle) {
        match method {
            "asyncId" => {
                return Some(crate::events::js_event_emitter_async_resource_async_id(
                    handle,
                ));
            }
            "triggerAsyncId" => {
                return Some(
                    crate::events::js_event_emitter_async_resource_trigger_async_id(handle),
                );
            }
            "asyncResource" => {
                return Some(crate::events::js_event_emitter_async_resource_async_resource(handle));
            }
            "emitDestroy" => {
                return Some(crate::events::js_event_emitter_async_resource_emit_destroy(
                    handle,
                ));
            }
            _ => {}
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

    let bind_method = |method: &[u8]| -> f64 {
        extern "C" {
            fn js_class_method_bind(
                instance: f64,
                method_name_ptr: *const u8,
                method_name_len: usize,
            ) -> f64;
        }
        js_class_method_bind(nanbox_handle_value(handle), method.as_ptr(), method.len())
    };

    #[cfg(feature = "bundled-events")]
    if crate::events::is_event_emitter_async_resource_handle(handle) {
        match property {
            "asyncId" => {
                return Some(crate::events::js_event_emitter_async_resource_async_id(
                    handle,
                ));
            }
            "triggerAsyncId" => {
                return Some(
                    crate::events::js_event_emitter_async_resource_trigger_async_id(handle),
                );
            }
            "asyncResource" => {
                return Some(crate::events::js_event_emitter_async_resource_async_resource(handle));
            }
            "emitDestroy" => return Some(bind_method(b"emitDestroy")),
            _ => {}
        }
    }

    let method = match property {
        "on"
        | "addListener"
        | "once"
        | "prependListener"
        | "prependOnceListener"
        | "off"
        | "removeListener"
        | "removeAllListeners"
        | "emit"
        | "listenerCount"
        | "listeners"
        | "rawListeners"
        | "eventNames"
        | "setMaxListeners"
        | "getMaxListeners" => Some(property.as_bytes()),
        _ => None,
    }?;

    Some(bind_method(method))
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
    if !matches!(
        property,
        "run" | "getStore" | "enterWith" | "exit" | "disable"
    ) {
        return None;
    }
    if get_handle_mut::<crate::async_local_storage::AsyncLocalStorageHandle>(handle).is_none() {
        return None;
    }
    extern "C" {
        fn js_class_method_bind(
            instance: f64,
            method_name_ptr: *const u8,
            method_name_len: usize,
        ) -> f64;
    }
    let m = property.as_bytes();
    Some(js_class_method_bind(
        nanbox_handle_value(handle),
        m.as_ptr(),
        m.len(),
    ))
}
