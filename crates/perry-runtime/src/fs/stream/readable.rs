//! File-backed Readable hooks and construction.

use super::*;

/// `fs.ReadStream.prototype` as user code reads it, with its `Readable` parent.
fn read_stream_prototype() -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let ctor = scope.root_nanbox_f64(crate::object::bound_native_callable_export_value(
        &crate::fs::namespace::NM_FS,
        "fs",
        "ReadStream",
    ));
    let key = js_string_from_bytes(b"prototype".as_ptr(), 9);
    let key = scope.root_nanbox_f64(f64::from_bits(JSValue::string_ptr(key).bits()));
    crate::proxy::js_reflect_get(
        ctor.get_nanbox_f64(),
        key.get_nanbox_f64(),
        ctor.get_nanbox_f64(),
    )
}

/// Whether `value`'s prototype chain reaches `fs.ReadStream.prototype`.
pub(crate) fn is_read_stream_instance(value: f64) -> bool {
    if object_ptr_from_value(value).is_none() {
        return false;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let target = scope.root_nanbox_f64(read_stream_prototype());
    let current = scope.root_nanbox_f64(crate::object::js_object_get_prototype_of(
        value.get_nanbox_f64(),
    ));
    for _ in 0..64 {
        if current.get_nanbox_u64() == target.get_nanbox_u64() {
            return true;
        }
        if object_ptr_from_value(current.get_nanbox_f64()).is_none() {
            return false;
        }
        let next = crate::object::js_object_get_prototype_of(current.get_nanbox_f64());
        current.set_nanbox_f64(next);
    }
    false
}

fn read_stream_object(id: usize) -> Option<f64> {
    STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .map(|state| state.object_value)
            .filter(|object| object_ptr_from_value(*object).is_some())
    })
}

/// The Readable's `read` hook: defers one `read(2)` to a later turn, as Node's
/// asynchronous fs read does, so `readable` fires before the next chunk lands.
extern "C" fn read_stream_read_impl(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _size: f64,
) -> f64 {
    let id = stream_id_of(closure);
    schedule_read_stream_turn(id);
    undefined_value()
}

/// Performs the deferred `read(2)` and pushes the Buffer, EOF, or error.
pub(super) extern "C" fn read_stream_deliver_impl(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let id = stream_id_of(closure);
    let outcome = read_next_chunk(id);
    let Some(object) = read_stream_object(id) else {
        return undefined_value();
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_nanbox_f64(object);
    match outcome {
        Ok(Some(bytes)) => {
            let chunk = scope.root_nanbox_f64(buffer_value_from_bytes(&bytes));
            call_js_method1(object.get_nanbox_f64(), b"push", chunk.get_nanbox_f64());
        }
        Ok(None) => {
            call_js_method1(object.get_nanbox_f64(), b"push", null_value());
        }
        Err(failure) => {
            let error = scope.root_nanbox_f64(unsafe { failure.error_value() });
            call_js_method1(object.get_nanbox_f64(), b"destroy", error.get_nanbox_f64());
        }
    }
    undefined_value()
}

/// The Readable's `construct` hook. The fd is already open, so this only
/// reports the open outcome as Node's `_construct` does.
extern "C" fn read_stream_construct_impl(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    callback: f64,
) -> f64 {
    let id = stream_id_of(closure);
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback = scope.root_nanbox_f64(callback);
    let (stored, message, opened_fd) = STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let Some(state) = registry.get(&id) else {
            return (undefined_value(), None, None);
        };
        let opened_fd = state
            .fd
            .filter(|_| state.opened && matches!(state.owner, FdOwner::Path));
        (state.error_value, state.error_msg.clone(), opened_fd)
    });
    let stored = scope.root_nanbox_f64(stored);
    let cb_ptr = extract_closure_ptr(callback.get_nanbox_f64());
    if cb_ptr.is_null() {
        return undefined_value();
    }
    let has_stored = !JSValue::from_bits(stored.get_nanbox_f64().to_bits()).is_undefined();
    if has_stored || message.is_some() {
        let error = if has_stored {
            stored
        } else {
            scope.root_nanbox_f64(make_error_value(message.as_deref().unwrap_or_default()))
        };
        crate::closure::js_closure_call1(
            extract_closure_ptr(callback.get_nanbox_f64()),
            crate::closure::plain_call_receiver(),
            error.get_nanbox_f64(),
        );
        return undefined_value();
    }
    crate::closure::js_closure_call0(cb_ptr, crate::closure::plain_call_receiver());
    if let (Some(fd), Some(object)) = (opened_fd, read_stream_object(id)) {
        let object = scope.root_nanbox_f64(object);
        call_js_method2(
            object.get_nanbox_f64(),
            b"emit",
            string_value(b"open"),
            fd as f64,
        );
        call_js_method1(object.get_nanbox_f64(), b"emit", string_value(b"ready"));
    }
    undefined_value()
}

/// The Readable's `destroy` hook (Node's `_destroy`): the fd closes on every
/// destroy, whatever `autoClose` says; `autoClose` only gates auto-destroy.
extern "C" fn read_stream_destroy_impl(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    error: f64,
    callback: f64,
) -> f64 {
    let id = stream_id_of(closure);
    let scope = crate::gc::RuntimeHandleScope::new();
    let error = scope.root_nanbox_f64(error);
    let callback = scope.root_nanbox_f64(callback);
    if let Some(((), props)) = with_stream_props_mut(id, |state| {
        state.destroyed = true;
        Some(())
    }) {
        update_common_props(&props);
    }
    close_stream_fd(id, true);
    release_stream_roots(id);
    // Node completes `_destroy` from the asynchronous `close(2)` callback, so
    // `close` is never emitted in the turn that called `destroy()`.
    let done = scope.root_raw_mut_ptr(js_closure_alloc(
        crate::fn_info!(read_stream_destroy_done_impl, 0; with_declared(0)),
        2,
    ));
    done.with_mut_ptr(|done| {
        crate::closure::js_closure_set_capture_f64(done, 0, callback.get_nanbox_f64());
        crate::closure::js_closure_set_capture_f64(done, 1, error.get_nanbox_f64());
    });
    let _ = done.with_mut_ptr(|done: *mut ClosureHeader| {
        crate::timer::schedule_native_immediate(done as i64, &[], "FSREQCALLBACK")
    });
    undefined_value()
}

extern "C" fn read_stream_destroy_done_impl(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let callback = crate::closure::js_closure_get_capture_f64(closure, 0);
    let error = crate::closure::js_closure_get_capture_f64(closure, 1);
    let cb_ptr = extract_closure_ptr(callback);
    if !cb_ptr.is_null() {
        crate::closure::js_closure_call1(cb_ptr, crate::closure::plain_call_receiver(), error);
    }
    undefined_value()
}

/// `ReadStream.prototype.close(cb)`.
extern "C" fn read_stream_close_impl(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    cb: f64,
) -> f64 {
    let this = current_receiver_value(this);
    if object_ptr_from_value(this).is_none() {
        return undefined_value();
    }
    if is_callable_value(cb) {
        call_js_method2(this, b"once", string_value(b"close"), cb);
    }
    call_js_method0(this, b"destroy");
    undefined_value()
}

pub(super) fn create_read_stream_with_state(state: StreamState) -> f64 {
    // The Readable parent link is applied by the fs attach handler, and the
    // stream constructors must be materialized after their own install. A
    // program installs both only when codegen saw namespace use.
    crate::object::js_nm_install_fs();
    let id = alloc_stream(state);
    store_open_failure(id);
    let (high_water_mark, encoding, auto_close, emit_close) = STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let state = registry.get(&id).expect("read stream state is registered");
        (
            state.high_water_mark,
            state.encoding.clone(),
            state.auto_close,
            state.emit_close,
        )
    });
    let scope = crate::gc::RuntimeHandleScope::new();
    let options = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
    let hooks: [(&[u8], *const crate::closure::JsFunctionInfo); 3] = [
        (
            b"read",
            crate::fn_info!(read_stream_read_impl, 1; with_declared(1)),
        ),
        (
            b"construct",
            crate::fn_info!(read_stream_construct_impl, 1; with_declared(1)),
        ),
        (
            b"destroy",
            crate::fn_info!(read_stream_destroy_impl, 2; with_declared(2)),
        ),
    ];
    for (name, info) in hooks {
        let closure = make_stream_closure(info, id);
        let hook = scope.root_nanbox_f64(f64::from_bits(
            JSValue::pointer(closure as *const u8).bits(),
        ));
        set_object_field_rooted(&scope, &options, name, hook.get_nanbox_f64());
    }
    set_object_field_rooted(&scope, &options, b"highWaterMark", high_water_mark as f64);
    set_object_field_rooted(&scope, &options, b"autoDestroy", bool_value(auto_close));
    set_object_field_rooted(&scope, &options, b"emitClose", bool_value(emit_close));
    if let Some(encoding) = encoding {
        set_object_field_str_rooted(&scope, &options, b"encoding", &encoding);
    }
    // A real node:stream Readable supplies `read()`, `readable` events,
    // listener management and the async iterator; the hooks above are the
    // file-backed source.
    let value = scope.root_nanbox_f64(crate::node_stream::js_node_stream_readable_new(
        options.get_nanbox_f64(),
    ));
    // Iterators must wait for the asynchronous source instead of treating
    // the initially empty Readable buffer as EOF.
    crate::node_stream::async_iterator::mark_foreign_readable(value.get_nanbox_f64());
    let prototype = scope.root_nanbox_f64(read_stream_prototype());
    if let Some(object) = object_ptr_from_value(value.get_nanbox_f64()) {
        crate::object::prototype_chain::object_link_created_prototype(
            object as usize,
            prototype.get_nanbox_u64(),
        );
    }
    if let Some(((), props)) = with_stream_props_mut(id, |state| {
        state.object_value = value.get_nanbox_f64();
        Some(())
    }) {
        update_common_props(&props);
    }
    let close = make_stream_closure(
        crate::fn_info!(read_stream_close_impl, 1; with_declared(1)),
        id,
    );
    let close = scope.root_nanbox_f64(f64::from_bits(JSValue::pointer(close as *const u8).bits()));
    set_object_field_rooted(&scope, &value, b"close", close.get_nanbox_f64());
    value.get_nanbox_f64()
}
