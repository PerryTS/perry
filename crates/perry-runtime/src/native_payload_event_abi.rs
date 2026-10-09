//! Event boundaries for binding callbacks: catch JS throws inside one call so
//! binding roots, driver refs and owned buffers retire on the normal path.
use super::*;

fn undefined() -> f64 {
    bytes_undefined()
}

/// Emit directly from a native argument span. The emitter and provider hooks
/// share one root scope; no intermediate JS array survives this dispatch.
///
/// # Safety
/// args is readable for len NaN-boxed values, or null with len zero.
/// event_bytes is readable for event_len bytes.
#[no_mangle]
pub unsafe extern "C" fn js_perry_event_emit_span_in_resource(
    resource: f64,
    owner: f64,
    event_bytes: *const u8,
    event_len: usize,
    args: *const f64,
    len: usize,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let resource = scope.root_nanbox_f64(resource);
    let owner = scope.root_nanbox_f64(owner);
    emit_span_rooted(&scope, &resource, &owner, event_bytes, event_len, args, len)
}

/// Borrow the binding's one dispatch scope and existing owner/resource roots.
/// # Safety
/// scope_base and both slot tokens belong to a live FFI scope on this thread.
/// The byte/argument spans have the same contract as emit_span_in_resource.
#[no_mangle]
pub unsafe extern "C" fn js_perry_event_emit_span_rooted(
    scope_base: usize,
    resource_slot: usize,
    owner_slot: usize,
    event_bytes: *const u8,
    event_len: usize,
    args: *const f64,
    len: usize,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::borrow_ffi(scope_base);
    let resource = scope.ffi_nanbox_handle(resource_slot);
    let owner = scope.ffi_nanbox_handle(owner_slot);
    emit_span_rooted(&scope, &resource, &owner, event_bytes, event_len, args, len)
}

unsafe fn emit_span_rooted(
    scope: &crate::gc::RuntimeHandleScope,
    resource: &crate::gc::RuntimeHandle<'_>,
    owner: &crate::gc::RuntimeHandle<'_>,
    event_bytes: *const u8,
    event_len: usize,
    args: *const f64,
    len: usize,
) -> f64 {
    let args = if args.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(args, len)
    };
    let args = crate::node_stream::RootedArgs::new(scope, args);
    let event_bytes = std::slice::from_raw_parts(event_bytes, event_len);
    let event = crate::value::JSValue::try_short_string(event_bytes).unwrap_or_else(|| {
        crate::value::JSValue::string_ptr(crate::string::js_string_from_bytes(
            event_bytes.as_ptr(),
            event_len as u32,
        ))
    });
    let event = scope.root_nanbox_f64(f64::from_bits(event.bits()));
    let emit = || crate::node_stream::emit_stream_event_rooted(scope, owner, &event, &args);
    #[cfg(test)]
    if std::env::var("PERRY_TEST_NET_SABOTAGE").as_deref() == Ok("event_catch") {
        return emit();
    }
    match crate::async_hooks::owned_provider_scope_rooted(scope, resource, emit) {
        Ok(value) => value,
        Err(error) => event_exception(error),
    }
}

fn event_exception(error: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let error = scope.root_nanbox_f64(error);
    match crate::exception::catch_js_throw(|| {
        crate::os::emit_process_event("uncaughtException", &[error.get_nanbox_f64()])
    }) {
        Ok(true) => f64::from_bits(crate::value::TAG_UNDEFINED),
        Ok(false) => crate::exception::exit_on_uncaught(error.get_nanbox_f64()),
        Err(error) => crate::exception::exit_on_uncaught(error),
    }
}

/// A property Get used by a native event callback. A destination's getter may
/// throw; catch inside this call so binding roots and Rust temporaries still
/// retire on the ordinary return path.
#[no_mangle]
pub extern "C" fn js_perry_event_get(owner: f64, key: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let key = scope.root_nanbox_f64(key);
    match crate::exception::catch_js_throw(|| {
        crate::value::js_dyn_index_get(owner.get_nanbox_f64(), key.get_nanbox_f64())
    }) {
        Ok(value) => value,
        Err(error) => event_exception(error),
    }
}

/// Emit from a native completion. Catch inside this one call so a listener
/// throw cannot skip its caller's driver-ref retirement or owned-buffer drop.
/// This is the same uncaught path as a timer callback, with no payload busy
/// guard: explicit close and same-cell reopen remain possible in a listener.
#[no_mangle]
pub extern "C" fn js_perry_event_emit(owner: f64, event: f64, arguments: f64) -> f64 {
    js_perry_event_emit_in_resource(undefined(), owner, event, arguments)
}

/// The event boundary with a JS-owned async provider. Scope restoration stays
/// inside the runtime call even when a listener throws or reopens its Socket.
#[no_mangle]
pub extern "C" fn js_perry_event_emit_in_resource(
    resource: f64,
    owner: f64,
    event: f64,
    arguments: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let resource = scope.root_nanbox_f64(resource);
    let owner = scope.root_nanbox_f64(owner);
    let event = scope.root_nanbox_f64(event);
    let arguments = scope.root_nanbox_f64(arguments);
    #[cfg(test)]
    if std::env::var("PERRY_TEST_NET_SABOTAGE").as_deref() == Ok("event_catch") {
        return crate::node_stream::js_node_stream_method_emit_args(
            crate::value::js_nanbox_get_pointer(owner.get_nanbox_f64()),
            event.get_nanbox_f64(),
            crate::value::js_nanbox_get_pointer(arguments.get_nanbox_f64()),
        );
    }
    match crate::async_hooks::owned_provider_scope(resource.get_nanbox_f64(), || {
        crate::node_stream::js_node_stream_method_emit_args(
            crate::value::js_nanbox_get_pointer(owner.get_nanbox_f64()),
            event.get_nanbox_f64(),
            crate::value::js_nanbox_get_pointer(arguments.get_nanbox_f64()),
        )
    }) {
        Ok(value) => value,
        Err(error) => event_exception(error),
    }
}

/// Invoke a callback from a native event, preserving the ordinary receiver
/// and the pump's uncaught path. It does not begin a deferred payload close.
///
/// # Safety
/// args is readable for len NaN-boxed values, or null with len zero.
#[no_mangle]
pub unsafe extern "C" fn js_perry_event_call(
    callback: f64,
    receiver: f64,
    args: *const f64,
    len: usize,
) -> f64 {
    js_perry_event_call_in_resource(undefined(), callback, receiver, args, len)
}

/// Callback counterpart to emit_in_resource. All inputs remain rooted through
/// before/after hooks; the argument Vec drops after caught JS control returns.
///
/// # Safety
/// As js_perry_event_call.
#[no_mangle]
pub unsafe extern "C" fn js_perry_event_call_in_resource(
    resource: f64,
    callback: f64,
    receiver: f64,
    args: *const f64,
    len: usize,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let resource = scope.root_nanbox_f64(resource);
    let callback = scope.root_nanbox_f64(callback);
    let receiver = scope.root_nanbox_f64(receiver);
    let args = if args.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(args, len)
    };
    if args.len() <= 4 {
        let args = crate::node_stream::RootedArgs::new(&scope, args);
        return match crate::async_hooks::owned_provider_scope_rooted(&scope, &resource, || {
            args.with_live(|args| {
                crate::closure::native_call_value_this(
                    callback.get_nanbox_f64(),
                    crate::closure::JsThis::from_f64(receiver.get_nanbox_f64()),
                    args.as_ptr(),
                    args.len(),
                )
            })
        }) {
            Ok(value) => value,
            Err(error) => event_exception(error),
        };
    }
    let args = scope.root_nanbox_f64_slice(args);
    let mut values = Vec::with_capacity(args.len());
    match crate::async_hooks::owned_provider_scope(resource.get_nanbox_f64(), || {
        // A before hook may move arguments. Read the rooted slots after it;
        // keep the owning Vec outside the JS exception boundary.
        values.extend(args.iter().map(|arg| arg.get_nanbox_f64()));
        crate::closure::native_call_value_this(
            callback.get_nanbox_f64(),
            crate::closure::JsThis::from_f64(receiver.get_nanbox_f64()),
            values.as_ptr(),
            values.len(),
        )
    }) {
        Ok(value) => value,
        Err(error) => event_exception(error),
    }
}

#[cfg(feature = "keepalive-anchors")]
mod keepalive {
    use super::*;
    #[used(compiler)]
    static GET: extern "C" fn(f64, f64) -> f64 = js_perry_event_get;
    #[used(compiler)]
    static EMIT: extern "C" fn(f64, f64, f64) -> f64 = js_perry_event_emit;
    #[used(compiler)]
    static EMIT_IN: extern "C" fn(f64, f64, f64, f64) -> f64 = js_perry_event_emit_in_resource;
    #[used(compiler)]
    static CALL: unsafe extern "C" fn(f64, f64, *const f64, usize) -> f64 = js_perry_event_call;
    #[used(compiler)]
    static CALL_IN: unsafe extern "C" fn(f64, f64, f64, *const f64, usize) -> f64 =
        js_perry_event_call_in_resource;
}
