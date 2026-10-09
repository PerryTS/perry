//! Bun's TCP calling convention on ordinary Socket and Server owners.
//! Handler tables, data, listeners and promises are traced JS edges.
use crate::{
    payload_events as events, payload_server as server, payload_socket as socket,
    payload_transport as p,
};
use perry_ffi::{JsPromise, JsThis, JsValue, Promise, RawClosureHeader, TransientRootScope};

const EVENTS: [&str; 7] = [
    "connect", "data", "drain", "close", "error", "end", "timeout",
];
const HANDLERS: [&str; 7] = ["open", "data", "drain", "close", "error", "end", "timeout"];
fn boolean(value: f64) -> bool {
    JsValue::from_bits(value.to_bits()).to_bool()
}
fn bun_state(owner: f64) -> f64 {
    if p::server_link(owner).is_ok() {
        server::state(owner)
    } else {
        socket::state(owner)
    }
}

unsafe extern "C" fn event(closure: *const RawClosureHeader, _: JsThis, arg: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 0));
    let index = perry_ffi::closure_capture_f64(closure, 1) as usize;
    let arg = scope.root_nanbox(arg);
    let state = scope.root_nanbox(bun_state(owner.get()));
    let handlers = scope.root_nanbox(p::own_get(state.get(), "bunHandlers"));
    let promise = scope.root_nanbox(p::own_get(state.get(), "bunPromise"));
    let opened = boolean(p::own_get(state.get(), "bunOpened"));
    if index == 0 || index == 4 {
        p::own_set(state.get(), "bunPromise", p::undefined());
        if JsValue::from_bits(promise.get().to_bits()).is_pointer() {
            let promise = JsPromise::from_raw(p::raw_owner(promise.get()) as *mut Promise);
            if index == 0 {
                promise.resolve(JsValue::from_bits(owner.get().to_bits()));
            } else {
                promise.reject(JsValue::from_bits(arg.get().to_bits()));
            }
        }
    }
    if index == 0 {
        p::own_set(
            state.get(),
            "bunOpened",
            f64::from_bits(JsValue::TRUE.bits()),
        );
    }
    let handler = scope.root_nanbox(p::own_get(
        handlers.get(),
        if index == 4 && !opened {
            "connectError"
        } else {
            HANDLERS[index]
        },
    ));
    if socket::is_callback(handler.get()) {
        if index == 1 || index == 4 {
            events::call(handler.get(), JsThis::UNDEFINED, &[owner.get(), arg.get()]);
        } else {
            events::call(handler.get(), JsThis::UNDEFINED, &[owner.get()]);
        }
    }
    p::undefined()
}
fn bind_event(owner: f64, index: usize) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let callback_root = scope.root_addr(perry_ffi::alloc_closure(
        perry_ffi::js_function_info!(event, 1; with_flags(perry_ffi::FN_BUILTIN)),
        2,
    ) as i64);
    unsafe {
        let callback = callback_root.get() as *mut RawClosureHeader;
        perry_ffi::set_closure_capture_f64(callback, 0, owner.get());
        perry_ffi::set_closure_capture_f64(callback, 1, index as f64);
        extern "C" {
            fn js_node_stream_method_on(owner: i64, event: f64, callback: f64) -> f64;
        }
        let name = scope.root_nanbox(events::string(EVENTS[index]));
        js_node_stream_method_on(
            p::raw_owner(owner.get()),
            name.get(),
            p::boxed_addr(callback_root.get()),
        );
    }
}
fn install(owner: f64, options: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let options = scope.root_nanbox(options);
    let state = scope.root_nanbox(bun_state(owner.get()));
    let handlers = scope.root_nanbox(p::own_get(options.get(), "socket"));
    p::own_set(state.get(), "bunHandlers", handlers.get());
    let data = scope.root_nanbox(p::own_get(options.get(), "data"));
    p::own_set(owner.get(), "data", data.get());
    p::own_set(
        state.get(),
        "bunOpened",
        f64::from_bits(JsValue::FALSE.bits()),
    );
    if p::socket_link(owner.get()).is_ok() {
        unsafe {
            if let Ok(payload) = p::socket_ptr(socket::link(owner.get())) {
                (*payload).ext.bun = true;
            }
        }
        for index in 0..EVENTS.len() {
            bind_event(owner.get(), index);
        }
        method(
            owner.get(),
            "write",
            perry_ffi::js_function_info!(write, 3; with_flags(perry_ffi::FN_BUILTIN)),
        );
        method(
            owner.get(),
            "end",
            perry_ffi::js_function_info!(end, 3; with_flags(perry_ffi::FN_BUILTIN)),
        );
        method(
            owner.get(),
            "close",
            perry_ffi::js_function_info!(close, 0; with_flags(perry_ffi::FN_BUILTIN)),
        );
        method(
            owner.get(),
            "terminate",
            perry_ffi::js_function_info!(terminate, 0; with_flags(perry_ffi::FN_BUILTIN)),
        );
        method(
            owner.get(),
            "shutdown",
            perry_ffi::js_function_info!(shutdown, 1; with_flags(perry_ffi::FN_BUILTIN)),
        );
        method(
            owner.get(),
            "flush",
            perry_ffi::js_function_info!(flush, 0; with_flags(perry_ffi::FN_BUILTIN)),
        );
        method(
            owner.get(),
            "timeout",
            perry_ffi::js_function_info!(timeout, 1; with_flags(perry_ffi::FN_BUILTIN)),
        );
        extern "C" {
            fn js_object_define_getter(owner: f64, key: f64, callback: f64) -> f64;
        }
        // The numeric Bun surface is an own accessor; the canonical Socket
        // prototype remains the shared Node surface.
        let getter = scope.root_addr(perry_ffi::alloc_closure(
            perry_ffi::js_function_info!(ready, 0; with_flags(perry_ffi::FN_BUILTIN)),
            0,
        ) as i64);
        unsafe {
            let name = scope.root_nanbox(events::string("readyState"));
            js_object_define_getter(owner.get(), name.get(), p::boxed_addr(getter.get()));
        }
    } else {
        method(
            owner.get(),
            "stop",
            perry_ffi::js_function_info!(stop, 1; with_flags(perry_ffi::FN_BUILTIN)),
        );
    }
    method(
        owner.get(),
        "reload",
        perry_ffi::js_function_info!(reload, 1; with_flags(perry_ffi::FN_BUILTIN)),
    );
}
fn method(owner: f64, key: &str, info: &'static perry_ffi::JsFunctionInfo) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let callback = perry_ffi::alloc_closure(info, 0);
    p::own_set(owner.get(), key, p::boxed_addr(callback as i64));
}
unsafe extern "C" fn write(
    _: *const RawClosureHeader,
    this: JsThis,
    chunk: f64,
    offset: f64,
    length: f64,
) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(this.as_f64());
    let bytes = crate::jsvalue_to_socket_bytes(chunk).unwrap_or_default();
    let offset = if JsValue::from_bits(offset.to_bits()).is_number() {
        offset.max(0.0) as usize
    } else {
        0
    }
    .min(bytes.len());
    let length = if JsValue::from_bits(length.to_bits()).is_number() {
        length.max(0.0) as usize
    } else {
        bytes.len() - offset
    }
    .min(bytes.len() - offset);
    let queued = socket::get(owner.get(), "writableLength");
    let count = length.min(socket::HIGH_WATER_MARK.saturating_sub(queued.max(0.0) as usize));
    if count > 0 {
        let buffer = perry_ffi::alloc_buffer(&bytes[offset..offset + count]);
        socket::write(
            owner.get(),
            p::boxed_addr(buffer as i64),
            p::undefined(),
            p::undefined(),
        );
    }
    if count < length {
        if let Ok(payload) = p::socket_ptr(socket::link(owner.get())) {
            (*payload).ext.need_drain = true;
        }
    }
    count as f64
}
unsafe extern "C" fn end(
    c: *const RawClosureHeader,
    this: JsThis,
    chunk: f64,
    offset: f64,
    length: f64,
) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(this.as_f64());
    let n = write(c, this, chunk, offset, length);
    socket::end(owner.get(), p::undefined(), p::undefined(), p::undefined());
    n
}
unsafe extern "C" fn close(_: *const RawClosureHeader, this: JsThis) -> f64 {
    socket::end(
        this.as_f64(),
        p::undefined(),
        p::undefined(),
        p::undefined(),
    );
    p::undefined()
}
unsafe extern "C" fn terminate(_: *const RawClosureHeader, this: JsThis) -> f64 {
    socket::destroy(this.as_f64(), p::undefined());
    p::undefined()
}
unsafe extern "C" fn shutdown(c: *const RawClosureHeader, this: JsThis, graceful: f64) -> f64 {
    if boolean(graceful) {
        close(c, this)
    } else {
        terminate(c, this)
    }
}
unsafe extern "C" fn flush(_: *const RawClosureHeader, this: JsThis) -> f64 {
    socket::link(this.as_f64());
    p::undefined()
}
unsafe extern "C" fn timeout(_: *const RawClosureHeader, this: JsThis, seconds: f64) -> f64 {
    socket::set_timeout(this.as_f64(), seconds * 1000.0, p::undefined());
    p::undefined()
}
unsafe extern "C" fn ready(_: *const RawClosureHeader, this: JsThis) -> f64 {
    let owner = this.as_f64();
    socket::link(owner);
    if boolean(socket::get(owner, "destroyed")) || boolean(socket::get(owner, "connecting")) {
        0.0
    } else if boolean(socket::get(owner, "writableEnded")) {
        -2.0
    } else {
        1.0
    }
}
unsafe extern "C" fn reload(_: *const RawClosureHeader, this: JsThis, options: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(this.as_f64());
    let options = scope.root_nanbox(options);
    let state = scope.root_nanbox(bun_state(owner.get()));
    let nested = scope.root_nanbox(p::own_get(options.get(), "socket"));
    let handlers = if JsValue::from_bits(nested.get().to_bits()).is_undefined() {
        options.get()
    } else {
        nested.get()
    };
    let handlers = scope.root_nanbox(handlers);
    p::own_set(state.get(), "bunHandlers", handlers.get());
    if p::server_link(owner.get()).is_ok() {
        crate::native_transport::for_each_server_child(owner.get(), |child| {
            let state = scope.root_nanbox(bun_state(child.value()));
            p::own_set(state.get(), "bunHandlers", handlers.get());
        });
    }
    p::undefined()
}
unsafe extern "C" fn stop(_: *const RawClosureHeader, this: JsThis, active: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(this.as_f64());
    if boolean(active) {
        crate::native_transport::for_each_server_child(owner.get(), |child| {
            socket::destroy(child.value(), p::undefined());
        });
    }
    server::close(owner.get(), p::undefined());
    p::undefined()
}
unsafe extern "C" fn connection(closure: *const RawClosureHeader, _: JsThis, child: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 0));
    let child = scope.root_nanbox(child);
    let state = scope.root_nanbox(bun_state(owner.get()));
    let handlers = scope.root_nanbox(p::own_get(state.get(), "bunHandlers"));
    let options = scope.root_nanbox(f64::from_bits(perry_ffi::alloc_object().bits()));
    p::own_set(options.get(), "socket", handlers.get());
    let data = scope.root_nanbox(p::own_get(owner.get(), "data"));
    p::own_set(options.get(), "data", data.get());
    install(child.get(), options.get());
    p::own_set(child.get(), "listener", owner.get());
    let callback = scope.root_nanbox(p::own_get(handlers.get(), "open"));
    let state = scope.root_nanbox(bun_state(child.get()));
    p::own_set(
        state.get(),
        "bunOpened",
        f64::from_bits(JsValue::TRUE.bits()),
    );
    if socket::is_callback(callback.get()) {
        events::call(callback.get(), JsThis::UNDEFINED, &[child.get()]);
    }
    p::undefined()
}
fn endpoint(options: f64, listen: bool) -> (Option<String>, String, f64) {
    unsafe {
        (
            crate::get_object_string_field(options, "unix")
                .or_else(|| crate::get_object_string_field(options, "path")),
            crate::get_object_string_field(options, "hostname")
                .or_else(|| crate::get_object_string_field(options, "host"))
                .unwrap_or_else(|| if listen { "0.0.0.0" } else { "127.0.0.1" }.to_owned()),
            crate::get_object_number_field(options, "port").unwrap_or(0.0),
        )
    }
}
#[no_mangle]
pub unsafe extern "C" fn js_bun_tcp_nm_install() {
    extern "C" {
        fn js_nm_install_bun();
        fn js_set_native_bun_tcp_dispatch(
            f: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64,
        );
    }
    js_set_native_bun_tcp_dispatch(js_bun_tcp_native_dispatch);
    js_nm_install_bun();
}
#[no_mangle]
pub unsafe extern "C" fn js_bun_tcp_listen(options: f64) -> i64 {
    let scope = TransientRootScope::enter();
    let options = scope.root_nanbox(options);
    let (path, host, port) = endpoint(options.get(), true);
    let owner = scope.root_nanbox(server::new_server(
        crate::payload_io::ROUTE,
        p::undefined(),
        p::undefined(),
    ));
    install(owner.get(), options.get());
    let callback = scope.root_addr(perry_ffi::alloc_closure(
        perry_ffi::js_function_info!(connection, 1; with_flags(perry_ffi::FN_BUILTIN)),
        1,
    ) as i64);
    perry_ffi::set_closure_capture_f64(callback.get() as *mut RawClosureHeader, 0, owner.get());
    extern "C" {
        fn js_node_stream_method_on(owner: i64, event: f64, callback: f64) -> f64;
    }
    let name = scope.root_nanbox(events::string("connection"));
    js_node_stream_method_on(
        p::raw_owner(owner.get()),
        name.get(),
        p::boxed_addr(callback.get()),
    );
    if let Some(path) = path {
        server::listen(
            owner.get(),
            events::string(&path),
            p::undefined(),
            p::undefined(),
        );
        p::own_set(owner.get(), "unix", events::string(&path));
    } else {
        server::listen(owner.get(), port, events::string(&host), p::undefined());
    }
    let address = scope.root_nanbox(server::address(owner.get()));
    let port = p::own_get(address.get(), "port");
    p::own_set(owner.get(), "port", port);
    p::own_set(owner.get(), "hostname", events::string(&host));
    p::raw_owner(owner.get())
}
#[no_mangle]
pub unsafe extern "C" fn js_bun_tcp_connect(options: f64) -> *mut Promise {
    let scope = TransientRootScope::enter();
    let options = scope.root_nanbox(options);
    let (path, host, port) = endpoint(options.get(), false);
    let owner = scope.root_nanbox(socket::new_socket(crate::payload_io::ROUTE, options.get()));
    install(owner.get(), options.get());
    let promise = scope.root_addr(JsPromise::new().as_raw() as i64);
    let state = scope.root_nanbox(bun_state(owner.get()));
    p::own_set(state.get(), "bunPromise", p::boxed_addr(promise.get()));
    if let Some(path) = path {
        socket::connect(
            owner.get(),
            events::string(&path),
            p::undefined(),
            p::undefined(),
        );
    } else {
        socket::connect(owner.get(), port, events::string(&host), p::undefined());
    }
    promise.get() as *mut Promise
}
#[no_mangle]
pub unsafe extern "C" fn js_bun_tcp_native_dispatch(
    name: *const u8,
    len: usize,
    args: *const f64,
    count: usize,
) -> f64 {
    if name.is_null() {
        return p::undefined();
    }
    let name = std::str::from_utf8(std::slice::from_raw_parts(name, len)).unwrap_or("");
    let options = if count > 0 && !args.is_null() {
        *args
    } else {
        p::undefined()
    };
    match name {
        "listen" => p::boxed_addr(js_bun_tcp_listen(options)),
        "connect" => p::boxed_addr(js_bun_tcp_connect(options) as i64),
        _ => p::undefined(),
    }
}
