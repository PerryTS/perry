//! Method / method-value dispatch for EventEmitter receivers whose static
//! type the compiler lost (#11270, #11273).
//!
//! When codegen can prove a receiver is an `EventEmitter` it calls
//! `js_event_emitter_on`/`_emit`/... directly, and those symbols resolve to
//! this crate. Any receiver it cannot prove — an element read out of an
//! `EventEmitter[]`, a `const e = arr[i]` alias, an `any`, an emitter built by
//! `new events.EventEmitter()` through a namespace import — goes through
//! `js_native_call_method` → perry-stdlib's handle dispatcher instead.
//!
//! That dispatcher maps the EventEmitter method names onto `extern "C"`
//! declarations of the same symbols (#4995), intending the linker to pick
//! whichever implementation is linked. That only holds when perry-stdlib is
//! built WITHOUT `bundled-events` (the auto-optimize
//! `external-events-construct` archive). The prebuilt full stdlib (used under
//! `PERRY_NO_AUTO_OPTIMIZE=1` and whenever no rebuild is possible) also
//! DEFINES those symbols, so rustc binds the declarations to its own in-crate
//! definitions: the dispatcher consulted perry-stdlib's empty registry, found
//! no emitter, and every `.on`/`.emit` on one of our handles silently did
//! nothing, while method-value reads came back `undefined`.
//!
//! These functions answer for this crate's handles from inside this crate, so
//! the calls bind to our own registry whatever stdlib archive is linked.
//! perry-stdlib consults them (through the runtime hooks registered below)
//! before its own mapping. They claim a call only when the handle is live in
//! OUR registry and the name is one the stdlib mapping would have answered,
//! with the same arity guard; anything else (e.g. `EventEmitterAsyncResource`'s
//! `asyncId`) is left to perry-stdlib unchanged.

use super::*;

extern "C" {
    fn js_register_event_emitter_method_dispatch(
        f: unsafe extern "C" fn(i64, *const u8, usize, *const f64, usize, *mut f64) -> i32,
    );
    fn js_register_event_emitter_property_dispatch(
        f: unsafe extern "C" fn(i64, *const u8, usize, *mut f64) -> i32,
    );
    fn js_class_method_bind(instance: f64, method_name_ptr: *const u8, len: usize) -> f64;
}

/// Called from `ensure_runtime_hooks_registered`, i.e. before any emitter
/// handle of this crate exists.
pub(super) unsafe fn register_dispatch_hooks() {
    js_register_event_emitter_method_dispatch(event_emitter_method_dispatch_ext);
    js_register_event_emitter_property_dispatch(event_emitter_property_dispatch_ext);
}

/// Method names readable as bound method values. `js_class_method_bind`
/// retains the name pointer, so these must be `'static` literals.
fn method_name_static(property: &[u8]) -> Option<&'static [u8]> {
    Some(match property {
        b"on" => b"on",
        b"addListener" => b"addListener",
        b"once" => b"once",
        b"prependListener" => b"prependListener",
        b"prependOnceListener" => b"prependOnceListener",
        b"off" => b"off",
        b"removeListener" => b"removeListener",
        b"removeAllListeners" => b"removeAllListeners",
        b"emit" => b"emit",
        b"listenerCount" => b"listenerCount",
        b"listeners" => b"listeners",
        b"rawListeners" => b"rawListeners",
        b"eventNames" => b"eventNames",
        b"setMaxListeners" => b"setMaxListeners",
        b"getMaxListeners" => b"getMaxListeners",
        _ => return None,
    })
}

unsafe fn name_bytes<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if ptr.is_null() || len == 0 {
        None
    } else {
        Some(std::slice::from_raw_parts(ptr, len))
    }
}

/// Pack rooted values into a fresh JS array (GC-safe across the pushes).
unsafe fn pack_rooted(scope: &TransientRootScope, values: &[TransientRootedNanbox]) -> i64 {
    let mut array = scope.root_addr(js_array_alloc(values.len() as u32) as i64);
    for value in values {
        let grown = js_array_push_f64(array.get() as *mut ArrayHeader, value.get()) as i64;
        if grown != array.get() {
            array = scope.root_addr(grown);
        }
    }
    array.get()
}

unsafe extern "C" fn event_emitter_method_dispatch_ext(
    handle: i64,
    name_ptr: *const u8,
    name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
    out: *mut f64,
) -> i32 {
    if out.is_null() || !registry::is_local_event_emitter_handle(handle) {
        return 0;
    }
    let Some(name) = name_bytes(name_ptr, name_len) else {
        return 0;
    };
    let raw_args: &[f64] = if args_ptr.is_null() || args_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(args_ptr, args_len)
    };
    let argc = raw_args.len();
    let scope = TransientRootScope::enter();
    let args: Vec<TransientRootedNanbox> = raw_args
        .iter()
        .map(|value| scope.root_nanbox(*value))
        .collect();
    let bits = |index: usize| {
        args.get(index)
            .map(|value| value.get())
            .unwrap_or_else(undefined_value)
            .to_bits() as i64
    };
    let this = nanbox_pointer_bits(handle);
    let value = match name {
        b"on" | b"addListener" if argc >= 2 => {
            js_event_emitter_on(handle, bits(0), bits(1));
            this
        }
        b"once" if argc >= 2 => {
            js_event_emitter_once(handle, bits(0), bits(1));
            this
        }
        b"prependListener" if argc >= 2 => {
            js_event_emitter_prepend_listener(handle, bits(0), bits(1));
            this
        }
        b"prependOnceListener" if argc >= 2 => {
            js_event_emitter_prepend_once_listener(handle, bits(0), bits(1));
            this
        }
        b"off" | b"removeListener" if argc >= 2 => {
            js_event_emitter_remove_listener(handle, bits(0), bits(1));
            this
        }
        b"removeAllListeners" => {
            let packed = pack_rooted(&scope, &args);
            js_event_emitter_remove_all_listeners(handle, packed as *const ArrayHeader);
            this
        }
        b"emit" => {
            let rest = if argc > 1 { &args[1..] } else { &[] };
            let packed = pack_rooted(&scope, rest);
            js_event_emitter_emit(handle, bits(0), packed as *mut ArrayHeader)
        }
        b"listenerCount" if argc >= 1 => js_event_emitter_listener_count(handle, bits(0), bits(1)),
        b"listeners" if argc >= 1 => {
            nanbox_pointer_bits(js_event_emitter_listeners(handle, bits(0)) as i64)
        }
        b"rawListeners" if argc >= 1 => {
            nanbox_pointer_bits(js_event_emitter_raw_listeners(handle, bits(0)) as i64)
        }
        b"eventNames" => nanbox_pointer_bits(js_event_emitter_event_names(handle) as i64),
        b"setMaxListeners" if argc >= 1 => {
            js_event_emitter_set_max_listeners(handle, f64::from_bits(bits(0) as u64));
            this
        }
        b"getMaxListeners" => js_event_emitter_get_max_listeners(handle),
        b"domain" => js_event_emitter_domain_value(handle),
        _ => return 0,
    };
    *out = value;
    1
}

unsafe extern "C" fn event_emitter_property_dispatch_ext(
    handle: i64,
    name_ptr: *const u8,
    name_len: usize,
    out: *mut f64,
) -> i32 {
    if out.is_null() || !registry::is_local_event_emitter_handle(handle) {
        return 0;
    }
    let Some(method) = name_bytes(name_ptr, name_len).and_then(method_name_static) else {
        return 0;
    };
    *out = js_class_method_bind(nanbox_pointer_bits(handle), method.as_ptr(), method.len());
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_ffi::alloc_string;

    extern "C" fn noop_listener(_c: *const RawClosureHeader) -> f64 {
        undefined_value()
    }

    fn listener_value() -> f64 {
        let closure = unsafe { js_closure_alloc(noop_listener as *const u8, 0) };
        nanbox_pointer_bits(closure as i64)
    }

    unsafe fn call(handle: i64, name: &str, args: &[f64]) -> Option<f64> {
        let mut out = undefined_value();
        let claimed = event_emitter_method_dispatch_ext(
            handle,
            name.as_ptr(),
            name.len(),
            args.as_ptr(),
            args.len(),
            &mut out,
        );
        (claimed != 0).then_some(out)
    }

    /// The #11270 shape: a receiver the compiler could not type reaches the
    /// emitter only through the runtime-registered dispatcher perry-stdlib
    /// consults. After the hooks are registered it must reach THIS registry.
    #[test]
    fn registered_dispatcher_reaches_ext_emitter() {
        let h = js_event_emitter_new();
        let event = alloc_string("x");
        let event = f64::from_bits(nanbox_string_bits(event.as_raw()));
        let dispatch = perry_runtime::object::event_emitter_method_dispatch()
            .expect("constructing an emitter registers the method dispatcher");
        let call = |name: &str, args: &[f64]| {
            let mut out = undefined_value();
            let claimed = unsafe {
                dispatch(
                    h,
                    name.as_ptr(),
                    name.len(),
                    args.as_ptr(),
                    args.len(),
                    &mut out,
                )
            };
            assert_eq!(claimed, 1, "{name} on our own handle must be claimed");
            out
        };
        let args = [event, listener_value()];
        assert_eq!(
            call("on", &args).to_bits(),
            nanbox_pointer_bits(h).to_bits()
        );
        assert_eq!(call("listenerCount", &args[..1]), 1.0);
        let had = call("emit", &args[..1]);
        assert_eq!(
            had.to_bits(),
            0x7FFC_0000_0000_0004,
            "emit reports a listener"
        );
        assert!(perry_runtime::object::event_emitter_property_dispatch().is_some());
        drop_event_emitter_handle(h);
    }

    #[test]
    fn declines_foreign_handles_unknown_names_and_short_arity() {
        let h = js_event_emitter_new();
        unsafe {
            // Not in our registry: never claimed, whatever the name.
            assert!(call(EVENT_EMITTER_HANDLE_ID_START - 1, "on", &[]).is_none());
            assert!(call(1, "emit", &[]).is_none());
            // Ours, but a name the primary dispatcher owns (async resource).
            assert!(call(h, "asyncId", &[]).is_none());
            // Same arity guard as perry-stdlib's mapping.
            assert!(call(h, "on", &[undefined_value()]).is_none());
            assert_eq!(call(h, "getMaxListeners", &[]), Some(10.0));
            assert!(call(h, "setMaxListeners", &[42.0]).is_some());
            assert_eq!(call(h, "getMaxListeners", &[]), Some(42.0));
        }
        drop_event_emitter_handle(h);
    }

    #[test]
    fn property_reads_bind_only_listener_methods() {
        let h = js_event_emitter_new();
        let mut out = undefined_value();
        let read = |name: &str, out: &mut f64| unsafe {
            event_emitter_property_dispatch_ext(h, name.as_ptr(), name.len(), out)
        };
        assert_eq!(read("on", &mut out), 1);
        assert!(JsValue::from_bits(out.to_bits()).is_pointer());
        assert_eq!(read("asyncId", &mut out), 0);
        assert_eq!(read("nope", &mut out), 0);
        drop_event_emitter_handle(h);
    }
}
