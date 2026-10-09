//! Raw-owner compatibility entry points for independently linked archives.
//! These carriers are actual GC object addresses, never transport ids.
use super::*;
use super::{payload_server as server, payload_socket as socket, payload_transport as p};
use perry_ffi::JsThis;
extern "C" {
    fn js_json_stringify(value: f64, hint: u32) -> *mut StringHeader;
    fn js_dynamic_object_get_property(owner: f64, key: *const i8, len: usize) -> f64;
}
fn owner(raw: i64) -> f64 {
    p::boxed_addr(raw)
}
fn call(raw: i64, name: &str, args: &[f64]) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner(raw));
    let args: Vec<_> = args.iter().map(|arg| scope.root_nanbox(*arg)).collect();
    if p::socket_link(owner.get()).is_err() && p::server_link(owner.get()).is_err() {
        super::payload_prototype::throw_miss::<()>(perry_ffi::native_payload::PayloadMiss::Foreign);
    }
    let callback = scope.root_nanbox(unsafe {
        js_dynamic_object_get_property(owner.get(), name.as_ptr() as *const i8, name.len())
    });
    let args: Vec<_> = args.iter().map(|arg| arg.get()).collect();
    super::payload_events::call(callback.get(), JsThis::from_f64(owner.get()), &args)
}
fn event_value(raw: i64) -> f64 {
    unsafe { string_from_header_i64(raw) }
        .map(|name| super::payload_events::string(&name))
        .unwrap_or_else(p::undefined)
}
fn callback_value(raw: i64) -> f64 {
    if raw == 0 {
        p::undefined()
    } else {
        p::boxed_addr(raw)
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_read(handle: i64, _size: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let _size_root = scope.root_nanbox(_size);

    {
        let args = [_size_root.get()];
        call(handle_root.get(), "read", &args)
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_read(handle: i64, size: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let size_root = scope.root_nanbox(size);

    {
        let args = [size_root.get()];
        call(handle_root.get(), "read", &args)
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_pending(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    socket::get(owner(handle_root.get()), "pending")
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_connecting(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    socket::get(owner(handle_root.get()), "connecting")
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_destroyed(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    socket::get(owner(handle_root.get()), "destroyed")
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_writable(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    socket::get(owner(handle_root.get()), "writable")
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_readable(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    socket::get(owner(handle_root.get()), "readable")
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_writable_ended(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    socket::get(owner(handle_root.get()), "writableEnded")
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_readable_ended(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    socket::get(owner(handle_root.get()), "readableEnded")
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_writable_state(handle: i64) -> *mut StringHeader {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    let value = socket::get(owner(handle_root.get()), "_writableState");
    js_json_stringify(value, 0)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_readable_state(handle: i64) -> *mut StringHeader {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    let value = socket::get(owner(handle_root.get()), "_readableState");
    js_json_stringify(value, 0)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_ready_state(handle: i64) -> *mut StringHeader {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    let value = socket::get(owner(handle_root.get()), "readyState");
    alloc_string(&jsvalue_to_owned_string(value).unwrap_or_default()).as_raw()
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_bytes_read(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    socket::get(owner(handle_root.get()), "bytesRead")
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_bytes_written(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    socket::get(owner(handle_root.get()), "bytesWritten")
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_timeout(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    socket::get(owner(handle_root.get()), "timeout")
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_local_address(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    socket::get(owner(handle_root.get()), "localAddress")
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_local_port(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    socket::get(owner(handle_root.get()), "localPort")
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_local_family(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    socket::get(owner(handle_root.get()), "localFamily")
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_remote_address(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    socket::get(owner(handle_root.get()), "remoteAddress")
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_remote_port(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    socket::get(owner(handle_root.get()), "remotePort")
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_remote_family(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    socket::get(owner(handle_root.get()), "remoteFamily")
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_buffer_size(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    socket::get(owner(handle_root.get()), "bufferSize")
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_writable_length(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    socket::get(owner(handle_root.get()), "writableLength")
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_writable_need_drain(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    socket::get(owner(handle_root.get()), "writableNeedDrain")
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_write(handle: i64, chunk_bits: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let chunk_bits_root = scope.root_nanbox(f64::from_bits(chunk_bits as u64));

    {
        let args = [f64::from_bits(chunk_bits_root.get().to_bits() as i64 as u64)];
        call(handle_root.get(), "write", &args)
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_write(handle: i64, chunk_bits: i64) {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let chunk_bits_root = scope.root_nanbox(f64::from_bits(chunk_bits as u64));

    {
        let args = [f64::from_bits(chunk_bits_root.get().to_bits() as i64 as u64)];
        call(handle_root.get(), "write", &args)
    };
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_write3(
    handle: i64,
    chunk: f64,
    encoding_or_callback: f64,
    callback: f64,
) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let chunk_root = scope.root_nanbox(chunk);
    let encoding_or_callback_root = scope.root_nanbox(encoding_or_callback);
    let callback_root = scope.root_nanbox(callback);

    {
        let args = [
            chunk_root.get(),
            encoding_or_callback_root.get(),
            callback_root.get(),
        ];
        call(handle_root.get(), "write", &args)
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_end(handle: i64, chunk_bits: i64) {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let chunk_bits_root = scope.root_nanbox(f64::from_bits(chunk_bits as u64));

    {
        let args = [f64::from_bits(chunk_bits_root.get().to_bits() as i64 as u64)];
        call(handle_root.get(), "end", &args)
    };
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_end(handle: i64, chunk_bits: i64) {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let chunk_bits_root = scope.root_nanbox(f64::from_bits(chunk_bits as u64));

    {
        let args = [f64::from_bits(chunk_bits_root.get().to_bits() as i64 as u64)];
        call(handle_root.get(), "end", &args)
    };
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_end3(
    handle: i64,
    chunk_or_callback: f64,
    encoding_or_callback: f64,
    callback: f64,
) {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let chunk_or_callback_root = scope.root_nanbox(chunk_or_callback);
    let encoding_or_callback_root = scope.root_nanbox(encoding_or_callback);
    let callback_root = scope.root_nanbox(callback);

    call(
        handle_root.get(),
        "end",
        &[
            chunk_or_callback_root.get(),
            encoding_or_callback_root.get(),
            callback_root.get(),
        ],
    );
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_destroy(handle: i64) {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    {
        let args = [];
        call(handle_root.get(), "destroy", &args)
    };
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_destroy_socket(handle: i64) {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    {
        let args = [];
        call(handle_root.get(), "destroy", &args)
    };
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_address(handle: i64) -> *mut StringHeader {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    let value = {
        let args = [];
        call(handle_root.get(), "address", &args)
    };
    js_json_stringify(value, 0)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_once(handle: i64, event_ptr: i64, cb: i64) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let event_ptr_root = scope.root_addr(event_ptr);
    let cb_root = scope.root_addr(cb);

    let args = [
        event_value(event_ptr_root.get()),
        callback_value(cb_root.get()),
    ];
    let value = call(handle_root.get(), "once", &args);
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_prepend_listener(
    handle: i64,
    event_ptr: i64,
    cb: i64,
) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let event_ptr_root = scope.root_addr(event_ptr);
    let cb_root = scope.root_addr(cb);

    let args = [
        event_value(event_ptr_root.get()),
        callback_value(cb_root.get()),
    ];
    let value = call(handle_root.get(), "prependListener", &args);
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_prepend_once_listener(
    handle: i64,
    event_ptr: i64,
    cb: i64,
) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let event_ptr_root = scope.root_addr(event_ptr);
    let cb_root = scope.root_addr(cb);

    let args = [
        event_value(event_ptr_root.get()),
        callback_value(cb_root.get()),
    ];
    let value = call(handle_root.get(), "prependOnceListener", &args);
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_remove_listener(
    handle: i64,
    event_ptr: i64,
    cb: i64,
) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let event_ptr_root = scope.root_addr(event_ptr);
    let cb_root = scope.root_addr(cb);

    let args = [
        event_value(event_ptr_root.get()),
        callback_value(cb_root.get()),
    ];
    let value = call(handle_root.get(), "removeListener", &args);
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_remove_all_listeners(handle: i64, event_ptr: i64) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let event_ptr_root = scope.root_addr(event_ptr);

    let value = {
        let args = [event_value(event_ptr_root.get())];
        call(handle_root.get(), "removeAllListeners", &args)
    };
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_listener_count(handle: i64, event_ptr: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let event_ptr_root = scope.root_addr(event_ptr);

    {
        let args = [event_value(event_ptr_root.get())];
        call(handle_root.get(), "listenerCount", &args)
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_listener_count(handle: i64, event_ptr: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let event_ptr_root = scope.root_addr(event_ptr);

    {
        let args = [event_value(event_ptr_root.get())];
        call(handle_root.get(), "listenerCount", &args)
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_get_max_listeners(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    {
        let args = [];
        call(handle_root.get(), "getMaxListeners", &args)
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_set_max_listeners(handle: i64, n: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let n_root = scope.root_nanbox(n);

    {
        let args = [n_root.get()];
        call(handle_root.get(), "setMaxListeners", &args)
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_event_names(handle: i64) -> *mut StringHeader {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    let value = {
        let args = [];
        call(handle_root.get(), "eventNames", &args)
    };
    js_json_stringify(value, 0)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_listeners(handle: i64, event_ptr: i64) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let event_ptr_root = scope.root_addr(event_ptr);

    let value = {
        let args = [event_value(event_ptr_root.get())];
        call(handle_root.get(), "listeners", &args)
    };
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_listeners(handle: i64, event_ptr: i64) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let event_ptr_root = scope.root_addr(event_ptr);

    let value = {
        let args = [event_value(event_ptr_root.get())];
        call(handle_root.get(), "listeners", &args)
    };
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_raw_listeners(handle: i64, event_ptr: i64) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let event_ptr_root = scope.root_addr(event_ptr);

    let value = {
        let args = [event_value(event_ptr_root.get())];
        call(handle_root.get(), "rawListeners", &args)
    };
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_reset_and_destroy(handle: i64) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    let value = {
        let args = [];
        call(handle_root.get(), "resetAndDestroy", &args)
    };
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_server_once(handle: i64, event_ptr: i64, cb: i64) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let event_ptr_root = scope.root_addr(event_ptr);
    let cb_root = scope.root_addr(cb);

    let args = [
        event_value(event_ptr_root.get()),
        callback_value(cb_root.get()),
    ];
    let value = call(handle_root.get(), "once", &args);
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_server_prepend_listener(
    handle: i64,
    event_ptr: i64,
    cb: i64,
) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let event_ptr_root = scope.root_addr(event_ptr);
    let cb_root = scope.root_addr(cb);

    let args = [
        event_value(event_ptr_root.get()),
        callback_value(cb_root.get()),
    ];
    let value = call(handle_root.get(), "prependListener", &args);
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_server_prepend_once_listener(
    handle: i64,
    event_ptr: i64,
    cb: i64,
) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let event_ptr_root = scope.root_addr(event_ptr);
    let cb_root = scope.root_addr(cb);

    let args = [
        event_value(event_ptr_root.get()),
        callback_value(cb_root.get()),
    ];
    let value = call(handle_root.get(), "prependOnceListener", &args);
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_server_remove_listener(
    handle: i64,
    event_ptr: i64,
    cb: i64,
) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let event_ptr_root = scope.root_addr(event_ptr);
    let cb_root = scope.root_addr(cb);

    let args = [
        event_value(event_ptr_root.get()),
        callback_value(cb_root.get()),
    ];
    let value = call(handle_root.get(), "removeListener", &args);
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_server_remove_all_listeners(handle: i64, event_ptr: i64) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let event_ptr_root = scope.root_addr(event_ptr);

    let value = {
        let args = [event_value(event_ptr_root.get())];
        call(handle_root.get(), "removeAllListeners", &args)
    };
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_server_listener_count(handle: i64, event_ptr: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let event_ptr_root = scope.root_addr(event_ptr);

    {
        let args = [event_value(event_ptr_root.get())];
        call(handle_root.get(), "listenerCount", &args)
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_net_server_event_names(handle: i64) -> *mut StringHeader {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    let value = {
        let args = [];
        call(handle_root.get(), "eventNames", &args)
    };
    js_json_stringify(value, 0)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_server_listeners(handle: i64, event_ptr: i64) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let event_ptr_root = scope.root_addr(event_ptr);

    let value = {
        let args = [event_value(event_ptr_root.get())];
        call(handle_root.get(), "listeners", &args)
    };
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_server_raw_listeners(handle: i64, event_ptr: i64) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let event_ptr_root = scope.root_addr(event_ptr);

    let value = {
        let args = [event_value(event_ptr_root.get())];
        call(handle_root.get(), "rawListeners", &args)
    };
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_noop_self(handle: i64) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    let value = {
        let args = [];
        call(handle_root.get(), "ref", &args)
    };
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_ref(handle: i64) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    let value = {
        let args = [];
        call(handle_root.get(), "ref", &args)
    };
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_unref(handle: i64) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    let value = {
        let args = [];
        call(handle_root.get(), "unref", &args)
    };
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_set_timeout(
    handle: i64,
    msecs: f64,
    _callback_i64: i64,
) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let msecs_root = scope.root_nanbox(msecs);
    let _callback_i64_root = scope.root_nanbox(f64::from_bits(_callback_i64 as u64));

    let value = call(
        handle_root.get(),
        "setTimeout",
        &[
            msecs_root.get(),
            f64::from_bits(_callback_i64_root.get().to_bits() as i64 as u64),
        ],
    );
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_server_noop_self(handle: i64) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    let value = {
        let args = [];
        call(handle_root.get(), "ref", &args)
    };
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_type_of_service(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    {
        let args = [];
        call(handle_root.get(), "getTypeOfService", &args)
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_set_type_of_service(handle: i64, tos: f64) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let tos_root = scope.root_nanbox(tos);

    let value = {
        let args = [tos_root.get()];
        call(handle_root.get(), "setTypeOfService", &args)
    };
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_set_no_delay(handle: i64, arg_bits: i64) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let arg_bits_root = scope.root_nanbox(f64::from_bits(arg_bits as u64));

    let value = {
        let args = [f64::from_bits(arg_bits_root.get().to_bits() as i64 as u64)];
        call(handle_root.get(), "setNoDelay", &args)
    };
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_set_encoding(handle: i64, enc_ptr: i64) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let enc_ptr_root = scope.root_addr(enc_ptr);

    let value = {
        let args = [event_value(enc_ptr_root.get())];
        call(handle_root.get(), "setEncoding", &args)
    };
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_server_on(handle: i64, event_ptr: i64, cb: i64) {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let event_ptr_root = scope.root_addr(event_ptr);
    let cb_root = scope.root_addr(cb);

    {
        let args = [
            event_value(event_ptr_root.get()),
            callback_value(cb_root.get()),
        ];
        call(handle_root.get(), "on", &args)
    };
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_on(handle: i64, event_ptr: i64, cb: i64) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let event_ptr_root = scope.root_addr(event_ptr);
    let cb_root = scope.root_addr(cb);

    let value = {
        let args = [
            event_value(event_ptr_root.get()),
            callback_value(cb_root.get()),
        ];
        call(handle_root.get(), "on", &args)
    };
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_once(handle: i64, event_ptr: i64, cb: i64) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let event_ptr_root = scope.root_addr(event_ptr);
    let cb_root = scope.root_addr(cb);

    let args = [
        event_value(event_ptr_root.get()),
        callback_value(cb_root.get()),
    ];
    let value = call(handle_root.get(), "once", &args);
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_remove_listener(
    handle: i64,
    event_ptr: i64,
    cb: i64,
) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let event_ptr_root = scope.root_addr(event_ptr);
    let cb_root = scope.root_addr(cb);

    let args = [
        event_value(event_ptr_root.get()),
        callback_value(cb_root.get()),
    ];
    let value = call(handle_root.get(), "removeListener", &args);
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_remove_all_listeners(
    handle: i64,
    event_ptr: i64,
) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let event_ptr_root = scope.root_addr(event_ptr);

    let value = {
        let args = [event_value(event_ptr_root.get())];
        call(handle_root.get(), "removeAllListeners", &args)
    };
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_cork(handle: i64) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    let value = {
        let args = [];
        call(handle_root.get(), "cork", &args)
    };
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_uncork(handle: i64) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    let value = {
        let args = [];
        call(handle_root.get(), "uncork", &args)
    };
    p::raw_owner(value)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_writable_corked(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    socket::get(owner(handle_root.get()), "writableCorked")
}

#[no_mangle]
pub unsafe extern "C" fn js_net_server_get_listening(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    server::get(owner(handle_root.get()), "listening")
}

#[no_mangle]
pub unsafe extern "C" fn js_net_server_get_connections(handle: i64) -> f64 {
    server::get(owner(handle), "_connections")
}

#[no_mangle]
pub unsafe extern "C" fn js_net_server_get_max_connections(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    server::get(owner(handle_root.get()), "maxConnections")
}

#[no_mangle]
pub unsafe extern "C" fn js_net_server_set_max_connections(handle: i64, value: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let value_root = scope.root_nanbox(value);

    {
        server::link(owner(handle_root.get()));
        p::own_set(owner(handle_root.get()), "maxConnections", value_root.get());
        value_root.get()
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_net_server_get_drop_max_connection(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);

    server::get(owner(handle_root.get()), "dropMaxConnection")
}

#[no_mangle]
pub unsafe extern "C" fn js_net_server_set_drop_max_connection(handle: i64, value: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let value_root = scope.root_nanbox(value);

    {
        server::link(owner(handle_root.get()));
        p::own_set(
            owner(handle_root.get()),
            "dropMaxConnection",
            value_root.get(),
        );
        value_root.get()
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_pipe(handle: i64, dest: f64, options: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let dest_root = scope.root_nanbox(dest);
    let options_root = scope.root_nanbox(options);

    {
        let args = [dest_root.get(), options_root.get()];
        call(handle_root.get(), "pipe", &args)
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_unpipe(handle: i64, dest: f64) -> i64 {
    let scope = TransientRootScope::enter();
    let handle_root = scope.root_addr(handle);
    let dest_root = scope.root_nanbox(dest);

    let value = {
        let args = [dest_root.get()];
        call(handle_root.get(), "unpipe", &args)
    };
    p::raw_owner(value)
}

#[no_mangle]
pub extern "C" fn js_net_has_pending() -> i32 {
    0
}
#[no_mangle]
pub unsafe extern "C" fn js_net_process_pending() -> i32 {
    0
}
pub fn is_net_socket_handle(raw: i64) -> bool {
    p::socket_link(owner(raw)).is_ok()
}
pub fn is_net_server_handle(raw: i64) -> bool {
    p::server_link(owner(raw)).is_ok()
}
#[no_mangle]
pub extern "C" fn js_ext_net_is_server_handle(raw: i64) -> i32 {
    is_net_server_handle(raw) as i32
}
#[no_mangle]
pub extern "C" fn js_net_server_listening(raw: i64) -> i32 {
    JsValue::from_bits(server::get(owner(raw), "listening").to_bits()).to_bool() as i32
}
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_on(raw: i64, event_ptr: i64, cb: i64) {
    let scope = TransientRootScope::enter();
    let raw_root = scope.root_addr(raw);
    let event_ptr_root = scope.root_addr(event_ptr);
    let cb_root = scope.root_addr(cb);

    {
        let args = [
            event_value(event_ptr_root.get()),
            callback_value(cb_root.get()),
        ];
        call(raw_root.get(), "on", &args)
    };
}
#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_emit(
    raw: i64,
    event_ptr: i64,
    args: *const f64,
    count: usize,
) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner(raw));
    let args: Vec<_> = if args.is_null() {
        Vec::new()
    } else {
        std::slice::from_raw_parts(args, count)
            .iter()
            .map(|arg| scope.root_nanbox(*arg))
            .collect()
    };
    let name = scope.root_nanbox(event_value(event_ptr));
    let mut values = vec![name.get()];
    values.extend(args.iter().map(|arg| arg.get()));
    call(p::raw_owner(owner.get()), "emit", &values)
}
#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_emit_abort_error(raw: i64) {
    let scope = TransientRootScope::enter();
    let raw_root = scope.root_addr(raw);

    socket::destroy(
        owner(raw_root.get()),
        socket::error("ABORT_ERR", "The operation was aborted"),
    );
}
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_upgrade_tls(
    raw: i64,
    servername_ptr: i64,
    verify: f64,
) -> *mut perry_ffi::Promise {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner(raw));
    socket::link(owner.get());
    let name = string_from_header_i64(servername_ptr).unwrap_or_default();
    let promise = scope.root_addr(perry_ffi::JsPromise::new().as_raw() as i64);
    let state = scope.root_nanbox(socket::state(owner.get()));
    p::own_set(state.get(), "upgradePromise", p::boxed_addr(promise.get()));
    if let Err(message) = super::payload_tls::install_client(
        owner.get(),
        name,
        verify != 0.0,
        super::TlsClientConfigData::default(),
    ) {
        super::payload_tls::settle_upgrade(owner.get(), Some(&message));
    }
    promise.get() as *mut perry_ffi::Promise
}
