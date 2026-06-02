use perry_ffi::{
    alloc_string, build_object_shape, js_object_alloc_with_shape, js_object_set_field, JsValue,
    ObjectHeader,
};
use std::net::SocketAddr;
use tokio::net::TcpStream;

use crate::statics;

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

#[derive(Debug)]
pub(crate) struct DropInfo {
    pub(crate) local: Option<SocketAddr>,
    pub(crate) remote: Option<SocketAddr>,
}

fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn js_bool(value: bool) -> f64 {
    f64::from_bits(JsValue::from_bool(value).bits())
}

fn addr_string(addr: Option<SocketAddr>) -> JsValue {
    match addr {
        Some(addr) => JsValue::from_string_ptr(alloc_string(&addr.ip().to_string()).as_raw()),
        None => JsValue::UNDEFINED,
    }
}

fn family_string(addr: Option<SocketAddr>) -> JsValue {
    match addr {
        Some(addr) => JsValue::from_string_ptr(
            alloc_string(if addr.is_ipv6() { "IPv6" } else { "IPv4" }).as_raw(),
        ),
        None => JsValue::UNDEFINED,
    }
}

fn port_number(addr: Option<SocketAddr>) -> JsValue {
    match addr {
        Some(addr) => JsValue::from_number(addr.port() as f64),
        None => JsValue::UNDEFINED,
    }
}

pub(crate) fn build_drop_object(info: &DropInfo) -> f64 {
    let keys = [
        "localAddress",
        "localPort",
        "localFamily",
        "remoteAddress",
        "remotePort",
        "remoteFamily",
    ];
    let (packed, shape_id) = build_object_shape(&keys);
    let obj: *mut ObjectHeader =
        unsafe { js_object_alloc_with_shape(shape_id, 6, packed.as_ptr(), packed.len() as u32) };
    if obj.is_null() {
        return undefined();
    }
    let values = [
        addr_string(info.local),
        port_number(info.local),
        family_string(info.local),
        addr_string(info.remote),
        port_number(info.remote),
        family_string(info.remote),
    ];
    for (index, value) in values.into_iter().enumerate() {
        unsafe { js_object_set_field(obj, index as u32, value) };
    }
    f64::from_bits(JsValue::from_object_ptr(obj as *mut u8).bits())
}

pub(crate) fn should_drop_connection(server_id: i64, stream: &TcpStream) -> Option<DropInfo> {
    let mut servers = statics::servers().lock().ok()?;
    let server = servers.get_mut(&server_id)?;
    if server
        .max_connections
        .is_some_and(|max| server.active_connections >= max)
        && server.drop_max_connection.unwrap_or(false)
    {
        return Some(DropInfo {
            local: stream.local_addr().ok(),
            remote: stream.peer_addr().ok(),
        });
    }
    server.active_connections += 1;
    None
}

pub(crate) fn socket_closed(server_id: i64) {
    if let Ok(mut servers) = statics::servers().lock() {
        if let Some(server) = servers.get_mut(&server_id) {
            server.active_connections = server.active_connections.saturating_sub(1);
        }
    }
}

#[no_mangle]
pub extern "C" fn js_net_server_get_listening(handle: i64) -> f64 {
    js_bool(crate::js_net_server_listening(handle) != 0)
}

#[no_mangle]
pub extern "C" fn js_net_server_get_connections(handle: i64) -> f64 {
    statics::servers()
        .lock()
        .ok()
        .and_then(|servers| servers.get(&handle).map(|s| s.active_connections as f64))
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_net_server_get_max_connections(handle: i64) -> f64 {
    statics::servers()
        .lock()
        .ok()
        .and_then(|servers| servers.get(&handle).and_then(|s| s.max_connections))
        .map(|n| n as f64)
        .unwrap_or_else(undefined)
}

#[no_mangle]
pub extern "C" fn js_net_server_set_max_connections(handle: i64, value: f64) -> f64 {
    if let Ok(mut servers) = statics::servers().lock() {
        if let Some(server) = servers.get_mut(&handle) {
            server.max_connections = if value.is_finite() && value >= 0.0 {
                Some(value as usize)
            } else {
                None
            };
        }
    }
    value
}

#[no_mangle]
pub extern "C" fn js_net_server_get_drop_max_connection(handle: i64) -> f64 {
    statics::servers()
        .lock()
        .ok()
        .and_then(|servers| servers.get(&handle).and_then(|s| s.drop_max_connection))
        .map(js_bool)
        .unwrap_or_else(undefined)
}

#[no_mangle]
pub extern "C" fn js_net_server_set_drop_max_connection(handle: i64, value: f64) -> f64 {
    let bool_value = JsValue::from_bits(value.to_bits()).to_bool();
    if let Ok(mut servers) = statics::servers().lock() {
        if let Some(server) = servers.get_mut(&handle) {
            server.drop_max_connection = Some(bool_value);
        }
    }
    value
}
