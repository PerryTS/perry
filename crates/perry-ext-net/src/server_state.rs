use bytes::Bytes;
use perry_ffi::{
    alloc_string, build_object_shape, js_object_alloc_with_shape, js_object_set_field, JsValue,
    ObjectHeader,
};
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::{Mutex, OnceLock};
use tokio::net::TcpStream;

use crate::{statics, PendingNetEvent};

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

#[derive(Default)]
struct ConnectionOrderState {
    // An accepted loopback socket must not become visible to
    // `server.getConnections()` until the matching client-side `connect`
    // callback has returned to the event loop. The accept and connect tasks
    // run concurrently, so either side can reach the main-thread queue first.
    deferred_connections: HashMap<i64, VecDeque<i64>>,
    // A connect callback can run before accept queues ServerConnection. Keep
    // a per-server credit so the later accept still crosses a pump boundary.
    completed_local_connects: HashMap<i64, usize>,
    // The accepted socket's read task can receive bytes during that boundary.
    // Preserve those chunks until the server `connection` callback has had a
    // chance to install its socket listeners.
    pending_socket_data: HashMap<i64, VecDeque<Bytes>>,
}

fn connection_order_state() -> &'static Mutex<ConnectionOrderState> {
    static STATE: OnceLock<Mutex<ConnectionOrderState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(ConnectionOrderState::default()))
}

fn schedule_server_connection(server_id: i64, socket_id: i64) {
    perry_ffi::spawn_async(async move {
        // `js_run_stdlib_pump` can reach ext-net twice in one invocation
        // (the stdlib feature arm and the auxiliary-pump registry). Crossing
        // the timer boundary lets the compiled await poll observe the client
        // callback before the server connection becomes visible.
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        statics::pending_events()
            .lock()
            .unwrap()
            .push(PendingNetEvent::ServerConnection(
                server_id, socket_id, true,
            ));
        perry_ffi::notify_main_thread();
    });
}

fn take_completed_local_connect(state: &mut ConnectionOrderState, server_id: i64) -> bool {
    let remove = match state.completed_local_connects.get_mut(&server_id) {
        Some(count) if *count > 0 => {
            *count -= 1;
            *count == 0
        }
        _ => return false,
    };
    if remove {
        state.completed_local_connects.remove(&server_id);
    }
    true
}

fn pop_deferred_connection(state: &mut ConnectionOrderState, server_id: i64) -> Option<i64> {
    let (socket_id, remove) = {
        let queue = state.deferred_connections.get_mut(&server_id)?;
        let socket_id = queue.pop_front();
        (socket_id, queue.is_empty())
    };
    if remove {
        state.deferred_connections.remove(&server_id);
    }
    socket_id
}

pub(crate) fn queue_server_connection(server_id: i64, socket_id: i64) -> bool {
    let mut state = connection_order_state().lock().unwrap();
    if !take_completed_local_connect(&mut state, server_id) {
        return true;
    }
    drop(state);
    schedule_server_connection(server_id, socket_id);
    false
}

pub(crate) fn defer_server_connection(server_id: i64, socket_id: i64) -> bool {
    let pending_local_connect = statics::servers()
        .lock()
        .unwrap()
        .get(&server_id)
        .is_some_and(|server| server.pending_local_connect_events > 0);
    let mut state = connection_order_state().lock().unwrap();
    if take_completed_local_connect(&mut state, server_id) {
        drop(state);
        schedule_server_connection(server_id, socket_id);
        return true;
    }
    if !pending_local_connect {
        return false;
    }
    state
        .deferred_connections
        .entry(server_id)
        .or_default()
        .push_back(socket_id);
    true
}

pub(crate) fn buffer_pending_server_data(socket_id: i64, bytes: Bytes) {
    let mut state = connection_order_state().lock().unwrap();
    let pending_connection = statics::sockets()
        .lock()
        .unwrap()
        .get(&socket_id)
        .is_some_and(|socket| socket.server_id.is_some() && !socket.server_connection_active);
    if !pending_connection {
        return;
    }
    state
        .pending_socket_data
        .entry(socket_id)
        .or_default()
        .push_back(bytes);
}

pub(crate) fn release_pending_server_data(socket_id: i64) {
    let chunks = connection_order_state()
        .lock()
        .unwrap()
        .pending_socket_data
        .remove(&socket_id);
    let Some(chunks) = chunks else {
        return;
    };
    let mut events = statics::pending_events().lock().unwrap();
    events.extend(
        chunks
            .into_iter()
            .map(|bytes| PendingNetEvent::Data(socket_id, bytes)),
    );
    drop(events);
    perry_ffi::notify_main_thread();
}

pub(crate) fn discard_pending_server_data(socket_id: i64) {
    connection_order_state()
        .lock()
        .unwrap()
        .pending_socket_data
        .remove(&socket_id);
}

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
        .is_some_and(|max| server.active_connections + server.pending_connections >= max)
        && server.drop_max_connection.unwrap_or(false)
    {
        return Some(DropInfo {
            local: stream.local_addr().ok(),
            remote: stream.peer_addr().ok(),
        });
    }
    server.pending_connections += 1;
    None
}

pub(crate) fn cancel_pending_connection(server_id: i64) {
    if let Some(server) = statics::servers().lock().unwrap().get_mut(&server_id) {
        server.pending_connections = server.pending_connections.saturating_sub(1);
    }
}

pub(crate) fn begin_local_connect(host: &str, port: u16) -> Option<(i64, bool)> {
    if !matches!(host, "localhost" | "127.0.0.1" | "::1" | "0.0.0.0") {
        return None;
    }
    let mut servers = statics::servers().lock().ok()?;
    let (server_id, server) = servers
        .iter_mut()
        .find(|(_, server)| server.listening && server.bound_port == port)?;
    let completed = connection_order_state()
        .lock()
        .unwrap()
        .completed_local_connects
        .get(server_id)
        .copied()
        .unwrap_or(0);
    let expects_drop = server.drop_max_connection.unwrap_or(false)
        && server.max_connections.is_some_and(|max| {
            server.active_connections + server.pending_connections + completed >= max
        });
    server.pending_local_connect_events += 1;
    Some((*server_id, expects_drop))
}

fn complete_local_connect(local_server: Option<(i64, bool)>, connected: bool) {
    let Some((server_id, expects_drop)) = local_server else {
        return;
    };
    if let Some(server) = statics::servers().lock().unwrap().get_mut(&server_id) {
        server.pending_local_connect_events = server.pending_local_connect_events.saturating_sub(1);
    }
    if !connected || expects_drop {
        return;
    }
    let mut state = connection_order_state().lock().unwrap();
    let socket_id = pop_deferred_connection(&mut state, server_id);
    if let Some(socket_id) = socket_id {
        drop(state);
        schedule_server_connection(server_id, socket_id);
    } else {
        *state.completed_local_connects.entry(server_id).or_default() += 1;
    }
}

pub(crate) fn finish_local_connect(local_server: Option<(i64, bool)>) {
    complete_local_connect(local_server, true);
}

pub(crate) fn cancel_local_connect(local_server: Option<(i64, bool)>) {
    complete_local_connect(local_server, false);
}

pub(crate) fn activate_connection(server_id: i64, socket_id: i64) {
    let mut sockets = statics::sockets().lock().unwrap();
    let Some(socket) = sockets.get_mut(&socket_id) else {
        return;
    };
    if socket.server_id != Some(server_id) || socket.server_connection_active {
        return;
    }
    socket.server_connection_active = true;
    if let Some(server) = statics::servers().lock().unwrap().get_mut(&server_id) {
        server.pending_connections = server.pending_connections.saturating_sub(1);
        server.active_connections += 1;
    }
}

pub(crate) fn mark_socket_closed(socket_id: i64) {
    let mut sockets = statics::sockets().lock().unwrap();
    let Some(socket) = sockets.get_mut(&socket_id) else {
        return;
    };
    socket.is_open = false;
    let Some(server_id) = socket.server_id.take() else {
        return;
    };
    if let Some(server) = statics::servers().lock().unwrap().get_mut(&server_id) {
        if socket.server_connection_active {
            server.active_connections = server.active_connections.saturating_sub(1);
        } else {
            server.pending_connections = server.pending_connections.saturating_sub(1);
        }
    }
}

pub(crate) fn remove_server(server_id: i64) {
    let mut state = connection_order_state().lock().unwrap();
    state.deferred_connections.remove(&server_id);
    state.completed_local_connects.remove(&server_id);
}

/// Returns true while queued net events, connecting/open sockets, or listening
/// servers need the runtime event loop to stay alive. Constructed but
/// unlistened sockets/servers match Node by not keeping the process alive.
pub(crate) fn has_active_handles() -> bool {
    if !statics::pending_events().lock().unwrap().is_empty() {
        return true;
    }
    if statics::sockets()
        .lock()
        .unwrap()
        .values()
        .any(|socket| !socket.destroyed && (socket.is_open || socket.pending_rx.is_none()))
    {
        return true;
    }
    statics::servers()
        .lock()
        .unwrap()
        .values()
        .any(|server| server.listening || server.shutdown_tx.is_some())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deferred_connections_are_fifo_and_remove_empty_server_queues() {
        let mut state = ConnectionOrderState::default();
        state
            .deferred_connections
            .insert(7, VecDeque::from([101, 102]));

        assert_eq!(pop_deferred_connection(&mut state, 7), Some(101));
        assert!(state.deferred_connections.contains_key(&7));
        assert_eq!(pop_deferred_connection(&mut state, 7), Some(102));
        assert!(!state.deferred_connections.contains_key(&7));
        assert_eq!(pop_deferred_connection(&mut state, 7), None);
    }

    #[test]
    fn completed_connect_credits_are_consumed_and_removed() {
        let mut state = ConnectionOrderState::default();
        state.completed_local_connects.insert(7, 2);

        assert!(take_completed_local_connect(&mut state, 7));
        assert_eq!(state.completed_local_connects.get(&7), Some(&1));
        assert!(take_completed_local_connect(&mut state, 7));
        assert!(!state.completed_local_connects.contains_key(&7));
        assert!(!take_completed_local_connect(&mut state, 7));
    }
}
