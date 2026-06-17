use perry_runtime::JSValue;

use crate::common::{get_handle_mut, Handle};

use super::{
    js_class_method_bind, js_http_agent_destroy, js_http_agent_get_name, js_http_agent_noop_self,
    AgentHandle, POINTER_TAG, PTR_MASK,
};

fn bind_agent_method(handle: Handle, name: &'static [u8]) -> i64 {
    (bind_agent_method_value(handle, name).to_bits() & PTR_MASK) as i64
}

fn bind_agent_method_value(handle: Handle, name: &'static [u8]) -> f64 {
    let instance = f64::from_bits(POINTER_TAG | (handle as u64 & PTR_MASK));
    unsafe { js_class_method_bind(instance, name.as_ptr(), name.len()) }
}

fn pointer_value(ptr: i64) -> f64 {
    if ptr == 0 {
        f64::from_bits(JSValue::undefined().bits())
    } else {
        f64::from_bits(POINTER_TAG | (ptr as u64 & PTR_MASK))
    }
}

pub(crate) fn dispatch_agent_property(handle: Handle, property: &str) -> Option<f64> {
    get_handle_mut::<AgentHandle>(handle)?;
    Some(match property {
        "createConnection" => pointer_value(js_http_agent_create_connection(handle)),
        "createSocket" => pointer_value(js_http_agent_create_socket(handle)),
        "getName" => bind_agent_method_value(handle, b"getName"),
        "destroy" => bind_agent_method_value(handle, b"destroy"),
        "keepSocketAlive" => bind_agent_method_value(handle, b"keepSocketAlive"),
        "reuseSocket" => bind_agent_method_value(handle, b"reuseSocket"),
        // #4904: data properties — Agents constructed through the dynamic
        // value path (`const { Agent } = require('http'); new Agent(...)`)
        // read these through handle property dispatch rather than the
        // class-filtered native rows.
        "maxSockets" => super::js_http_agent_max_sockets(handle),
        "maxFreeSockets" => super::js_http_agent_max_free_sockets(handle),
        "maxTotalSockets" => super::js_http_agent_max_total_sockets(handle),
        "keepAliveMsecs" => super::js_http_agent_keep_alive_msecs(handle),
        "keepAlive" => super::js_http_agent_keep_alive(handle),
        "destroyed" => super::js_http_agent_destroyed(handle),
        "defaultPort" => super::js_http_agent_default_port(handle),
        "protocol" => {
            let ptr = super::js_http_agent_protocol(handle);
            if ptr.is_null() {
                f64::from_bits(JSValue::undefined().bits())
            } else {
                f64::from_bits(JSValue::string_ptr(ptr).bits())
            }
        }
        "sockets" => js_http_agent_sockets(handle),
        "freeSockets" => js_http_agent_free_sockets(handle),
        "requests" => js_http_agent_requests(handle),
        _ => return None,
    })
}

/// #4904: property writes on a dynamically-dispatched Agent —
/// `agent.maxSockets = 4` and the `agent.createConnection = fn`
/// monkeypatch pattern Node's own tests use. Returns `true` when claimed.
pub(crate) fn dispatch_agent_property_set(handle: Handle, property: &str, value: f64) -> bool {
    if get_handle_mut::<AgentHandle>(handle).is_none() {
        return false;
    }
    match property {
        "maxSockets" => super::js_http_agent_set_max_sockets(handle, value),
        "maxFreeSockets" => super::js_http_agent_set_max_free_sockets(handle, value),
        "maxTotalSockets" => super::js_http_agent_set_max_total_sockets(handle, value),
        "keepAliveMsecs" => super::js_http_agent_set_keep_alive_msecs(handle, value),
        "keepAlive" => super::js_http_agent_set_keep_alive(handle, value),
        "createConnection" | "createSocket" => {
            let bits = value.to_bits();
            let ptr = if JSValue::from_bits(bits).is_pointer() {
                (bits & PTR_MASK) as i64
            } else {
                0
            };
            if property == "createConnection" {
                super::js_http_agent_set_create_connection(handle, ptr);
            } else {
                super::js_http_agent_set_create_socket(handle, ptr);
            }
        }
        _ => return false,
    }
    true
}

pub(crate) unsafe fn dispatch_agent_method(
    handle: Handle,
    method: &str,
    args: &[f64],
) -> Option<f64> {
    get_handle_mut::<AgentHandle>(handle)?;
    Some(match method {
        "getName" => {
            let options = args
                .first()
                .copied()
                .unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits()));
            let ptr = js_http_agent_get_name(handle, options);
            f64::from_bits(JSValue::string_ptr(ptr).bits())
        }
        "destroy" => pointer_value(js_http_agent_destroy(handle)),
        "keepSocketAlive" | "reuseSocket" => pointer_value(js_http_agent_noop_self(handle)),
        _ => return None,
    })
}

/// Allocate the empty object Node exposes for `agent.sockets`,
/// `agent.freeSockets`, and `agent.requests` before any requests are pooled.
fn empty_object_bits_f64() -> f64 {
    let obj = perry_runtime::js_object_alloc(0, 0);
    if obj.is_null() {
        return f64::from_bits(JSValue::undefined().bits());
    }
    f64::from_bits(JSValue::object_ptr(obj as *mut u8).bits())
}

#[no_mangle]
pub extern "C" fn js_http_agent_sockets(handle: Handle) -> f64 {
    let _ = handle;
    empty_object_bits_f64()
}

#[no_mangle]
pub extern "C" fn js_http_agent_free_sockets(handle: Handle) -> f64 {
    let _ = handle;
    empty_object_bits_f64()
}

#[no_mangle]
pub extern "C" fn js_http_agent_requests(handle: Handle) -> f64 {
    let _ = handle;
    empty_object_bits_f64()
}

#[no_mangle]
pub extern "C" fn js_http_agent_create_connection(handle: Handle) -> i64 {
    let stored = get_handle_mut::<AgentHandle>(handle)
        .map(|a| a.create_connection)
        .unwrap_or(0);
    if stored != 0 {
        stored
    } else {
        bind_agent_method(handle, b"createConnection")
    }
}

#[no_mangle]
pub extern "C" fn js_http_agent_create_socket(handle: Handle) -> i64 {
    let stored = get_handle_mut::<AgentHandle>(handle)
        .map(|a| a.create_socket)
        .unwrap_or(0);
    if stored != 0 {
        stored
    } else {
        bind_agent_method(handle, b"createSocket")
    }
}
