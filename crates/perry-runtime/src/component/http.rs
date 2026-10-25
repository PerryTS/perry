//! Native HTTP ownership. Only the agent pump enters a component or JavaScript;
//! driver wakers carry an agent id, and pending work pins traced payload cells.
mod pump;

use super::*;
use crate::native_payload::{self as payload, NativePayloadFamily};
use core::ffi::c_void;

static SERVER: NativePayloadFamily = NativePayloadFamily {
    class_id: crate::native_class_ids::COMPONENT_HTTP_SERVER,
    links_owner: false,
    name: "ComponentHttpServer",
    constructor_export: None,
    constructor_length: 0,
    install_prototype: |_| {},
};
static OPERATION: NativePayloadFamily = NativePayloadFamily {
    class_id: crate::native_class_ids::COMPONENT_HTTP_OPERATION,
    links_owner: true,
    name: "ComponentHttpOperation",
    constructor_export: None,
    constructor_length: 0,
    install_prototype: |_| {},
};

struct NativeHandle {
    pointer: *mut c_void,
    release: unsafe extern "C" fn(*mut c_void),
}
impl Drop for NativeHandle {
    fn drop(&mut self) {
        // No JS, allocation, TLS lookup, or fallible engine-table lookup.
        unsafe { (self.release)(self.pointer) }
    }
}
struct Operation {
    handle: NativeHandle,
    interface: String,
    invoked: bool,
    failed: bool,
    delegated: bool,
}

fn server_pointer(server: f64) -> *mut c_void {
    unsafe { payload::payload_mut::<NativeHandle>(server, &SERVER) }
        .unwrap_or_else(|_| fail("Expected an open ComponentHttpServer"))
        .pointer
}

#[no_mangle]
pub extern "C" fn js_component_http_server_new(options: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let options = scope.root_nanbox_f64(options);
    let owner = scope.root_nanbox_f64(undefined());
    let mut encoded = Value::Null;
    let mut bytes = Vec::new();
    let mut output = Vec::new();
    let mut address = String::new();
    let mut native = None;
    let result = crate::exception::catch_js_throw(|| {
        let ops = host::ops();
        encoded = value::encode(options.get_nanbox_f64());
        bytes = serde_json::to_vec(&encoded).unwrap();
        owner.set_nanbox_f64(payload::alloc_closed(&SERVER, &[]));
        let mut buffer = Buffer::default();
        let pointer = unsafe {
            (ops.perry_component_http_server_new)(bytes.as_ptr(), bytes.len(), &mut buffer)
        };
        if !pointer.is_null() {
            native = Some(NativeHandle {
                pointer,
                release: ops.perry_component_http_server_drop,
            });
        }
        output = unsafe { take(buffer) };
        if pointer.is_null() {
            fail(&String::from_utf8_lossy(&output));
        }
        address = serde_json::from_slice(&output)
            .unwrap_or_else(|_| fail("Invalid HTTP listener address from component engine"));
        payload::attach(owner.get_nanbox_f64(), &SERVER, native.take().unwrap(), 0)
            .unwrap_or_else(|_| fail("Cannot attach component HTTP server"));
        let address = scope.root_string_ptr(crate::string::js_string_from_bytes(
            address.as_ptr(),
            address.len() as u32,
        ));
        let key =
            scope.root_string_ptr(crate::string::js_string_from_bytes(b"address".as_ptr(), 7));
        crate::object::define_builtin_data_property(
            object_pointer(owner.get_nanbox_f64()).unwrap(),
            key.get_raw_const_ptr(),
            crate::value::js_nanbox_string(
                address.get_raw_const_ptr::<crate::string::StringHeader>() as i64,
            ),
            "address".to_owned(),
            crate::object::PropertyAttrs::new(false, true, false),
        );
        owner.get_nanbox_f64()
    });
    if result.is_err() {
        payload::close(owner.get_nanbox_f64(), &SERVER);
    }
    drop((encoded, bytes, output, address, native));
    finish(result)
}

#[no_mangle]
pub extern "C" fn js_component_http_server_close(server: f64) -> f64 {
    if payload::lifecycle(server, &SERVER).is_err() {
        fail("Expected a ComponentHttpServer");
    }
    payload::close(server, &SERVER);
    pump::notify(crate::agent::current_agent());
    undefined()
}

#[no_mangle]
pub extern "C" fn js_component_http_handle(server: f64, component: f64, interface: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let server = scope.root_nanbox_f64(server);
    let component = scope.root_nanbox_f64(component);
    let interface_input = scope.root_nanbox_f64(interface);
    let owner = scope.root_nanbox_f64(undefined());
    let promise = scope.root_raw_mut_ptr(std::ptr::null_mut::<crate::promise::Promise>());
    let mut interface = String::new();
    let mut output = Vec::new();
    let mut native = None;
    let result = crate::exception::catch_js_throw(|| {
        let ops = host::ops();
        interface = string(interface_input.get_nanbox_f64());
        // Root submission context before Promise init hooks can collect or
        // alter it. The Promise takes its own managed snapshot afterwards.
        let captured = scope.root_raw_mut_ptr(crate::async_context::alloc_snapshot_holder(
            crate::async_context::capture_context(),
        ));
        promise.set_raw_mut_ptr(crate::promise::js_promise_new());
        crate::promise::set_promise_context_snapshot(promise.get_raw_mut_ptr(), unsafe {
            (*captured.get_raw_mut_ptr::<crate::async_context::AsyncSnapshotHolder>())
                .snapshot
                .clone()
        });
        owner.set_nanbox_f64(payload::alloc_closed(&OPERATION, &[]));
        for key in [&b"promise"[..], &b"server"[..], &b"component"[..]] {
            // Reload handles individually: state_set may move all three.
            let value = match key {
                b"promise" => crate::value::js_nanbox_pointer(
                    promise.get_raw_mut_ptr::<crate::promise::Promise>() as i64,
                ),
                b"server" => server.get_nanbox_f64(),
                _ => component.get_nanbox_f64(),
            };
            payload::state_set(owner.get_nanbox_f64(), &OPERATION, key, value);
        }
        // Init hooks and managed allocations have finished. Revalidate both
        // owners before creating any pending native accept.
        let component_context = context::get(component.get_nanbox_f64());
        if unsafe { (*component_context).host.is_null() || !(*component_context).alive } {
            fail("Component is not initialized or has been disposed");
        }
        let pointer = server_pointer(server.get_nanbox_f64());
        let mut buffer = Buffer::default();
        let pointer = unsafe { (ops.perry_component_http_accept)(pointer, &mut buffer) };
        if !pointer.is_null() {
            native = Some(NativeHandle {
                pointer,
                release: ops.perry_component_http_drop,
            });
        }
        output = unsafe { take(buffer) };
        if pointer.is_null() {
            fail(&String::from_utf8_lossy(&output));
        }
        payload::attach(
            owner.get_nanbox_f64(),
            &OPERATION,
            Operation {
                handle: native.take().unwrap(),
                interface: std::mem::take(&mut interface),
                invoked: false,
                failed: false,
                delegated: false,
            },
            0,
        )
        .unwrap_or_else(|_| fail("Cannot attach component HTTP operation"));
        pump::enqueue(owner.get_nanbox_f64());
        crate::value::js_nanbox_pointer(promise.get_raw_mut_ptr::<crate::promise::Promise>() as i64)
    });
    if result.is_err() {
        payload::close(owner.get_nanbox_f64(), &OPERATION);
    }
    drop((interface, output, native));
    finish(result)
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_SERVER: extern "C" fn(f64) -> f64 = js_component_http_server_new;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_CLOSE: extern "C" fn(f64) -> f64 = js_component_http_server_close;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_HANDLE: extern "C" fn(f64, f64, f64) -> f64 = js_component_http_handle;

/// The async driver owns this traced HTTP cell after handoff. Native accept
/// state is released before its shared public Promise is observably settled.
pub(super) fn finish_delegation(owner: f64) {
    payload::close(owner, &OPERATION);
}
