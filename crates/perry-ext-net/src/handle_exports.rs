use super::*;

pub(super) fn listeners_for(id: i64, event: &str) -> Vec<i64> {
    statics::listeners()
        .lock()
        .unwrap()
        .get(&id)
        .and_then(|m| m.get(event).cloned())
        .unwrap_or_default()
}

#[no_mangle]
pub extern "C" fn js_net_has_pending() -> i32 {
    server_state::has_active_handles() as i32
}

pub fn is_net_socket_handle(handle: i64) -> bool {
    statics::sockets().lock().unwrap().contains_key(&handle)
}

pub fn is_net_server_handle(handle: i64) -> bool {
    statics::servers().lock().unwrap().contains_key(&handle)
}

#[no_mangle]
pub extern "C" fn js_net_server_listening(handle: i64) -> i32 {
    statics::servers()
        .lock()
        .ok()
        .and_then(|servers| {
            servers
                .get(&handle)
                .map(|server| i32::from(server.listening))
        })
        .unwrap_or(0)
}

/// `server.on(event, cb)` — register a server-level listener for
/// `'connection'`, `'listening'`, `'close'`, or `'error'`.
///
/// # Safety
///
/// `event_ptr` must be null or a Perry-runtime `StringHeader`. `cb`
/// is a raw `*const ClosureHeader` cast to `i64`.
#[no_mangle]
pub unsafe extern "C" fn js_net_server_on(handle: i64, event_ptr: i64, cb: i64) {
    ensure_gc_scanner_registered();
    let event = match string_from_header_i64(event_ptr) {
        Some(e) => e,
        None => return,
    };
    let mut listeners = statics::listeners().lock().unwrap();
    let entry = listeners.entry(handle).or_default();
    entry.entry(event).or_default().push(cb);
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_on(handle: i64, event_ptr: i64, cb: i64) {
    js_net_socket_on(handle, event_ptr, cb)
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_once(handle: i64, event_ptr: i64, cb: i64) -> i64 {
    js_net_socket_once(handle, event_ptr, cb)
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_remove_listener(
    handle: i64,
    event_ptr: i64,
    cb: i64,
) -> i64 {
    js_net_socket_remove_listener(handle, event_ptr, cb)
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_remove_all_listeners(
    handle: i64,
    event_ptr: i64,
) -> i64 {
    js_net_socket_remove_all_listeners(handle, event_ptr)
}

#[no_mangle]
pub extern "C" fn js_ext_net_is_server_handle(handle: i64) -> i32 {
    i32::from(is_net_server_handle(handle))
}
