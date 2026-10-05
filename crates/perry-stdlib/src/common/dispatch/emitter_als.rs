use super::*;

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
    this: perry_runtime::closure::JsThis,
    rest: f64,
) -> f64 {
    unsafe {
        let name_ptr = perry_runtime::closure::js_closure_get_capture_ptr(closure, 0) as *const i8;
        let name_len = perry_runtime::closure::js_closure_get_capture_ptr(closure, 1) as usize;
        let name = std::slice::from_raw_parts(name_ptr as *const u8, name_len);
        let name_str = std::str::from_utf8(name).unwrap_or("");
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let rest = scope.root_nanbox_f64(rest);
        let receiver_handle = scope.root_nanbox_f64(this.as_f64());
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
    let closure = perry_runtime::closure::js_closure_alloc(
        perry_runtime::fn_info!(async_local_storage_unbound_method_thunk, 1; with_rest(0)),
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

/// `AsyncLocalStorage` METHOD-VALUE reads (the property-read counterpart of
/// `dispatch_async_local_storage_method`). `als.getStore()` (a direct call)
/// already dispatched, but reading `als.getStore` AS A VALUE (`const gs =
/// als.getStore`, `{ getStore } = als`, `typeof als.getStore`) returned
/// `undefined` — there was no property-read dispatch for ALS handles (only
/// EventEmitter had one, #4995). Next.js' server startup reads `getStore` as a
/// value (cacheComponents / patch-fetch async-storage setup) and then calls it,
/// so it threw `TypeError: getStore is not a function` BEFORE `✓ Ready`. The
/// read yields the method as a callable value that resolves its receiver.
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
}
