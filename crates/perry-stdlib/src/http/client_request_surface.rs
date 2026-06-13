use super::*;
use std::sync::Mutex;

#[derive(Default)]
struct ClientRequestSurfaceState {
    aborted: bool,
    destroyed: bool,
    socket: f64,
}

static CLIENT_REQUEST_SURFACE: once_cell::sync::Lazy<
    Mutex<HashMap<Handle, ClientRequestSurfaceState>>,
> = once_cell::sync::Lazy::new(|| Mutex::new(HashMap::new()));

fn undefined_value() -> f64 {
    f64::from_bits(JSValue::undefined().bits())
}

fn null_value() -> f64 {
    f64::from_bits(JSValue::null().bits())
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(JSValue::bool(value).bits())
}

fn string_value(value: &str) -> f64 {
    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn handle_value(handle: Handle) -> f64 {
    f64::from_bits(POINTER_TAG | (handle as u64 & PTR_MASK))
}

pub(super) fn scan_roots(visitor: &mut perry_runtime::gc::RuntimeRootVisitor<'_>) {
    for state in CLIENT_REQUEST_SURFACE.lock().unwrap().values_mut() {
        if state.socket != 0.0 {
            visitor.visit_nanbox_f64_slot(&mut state.socket);
        }
    }
}

pub(crate) fn is_client_request_handle(handle: Handle) -> bool {
    get_handle_mut::<ClientRequestHandle>(handle).is_some()
}

fn with_state_mut<T>(handle: Handle, f: impl FnOnce(&mut ClientRequestSurfaceState) -> T) -> T {
    let mut states = CLIENT_REQUEST_SURFACE.lock().unwrap();
    f(states.entry(handle).or_default())
}

fn find_header_key(req: &ClientRequestHandle, name: &str) -> Option<String> {
    req.headers
        .keys()
        .find(|key| key.eq_ignore_ascii_case(name))
        .cloned()
}

fn header_names(handle: Handle, raw: bool) -> Vec<String> {
    let mut names = get_handle_mut::<ClientRequestHandle>(handle)
        .map(|req| {
            req.headers
                .keys()
                .map(|key| {
                    if raw {
                        key.clone()
                    } else {
                        key.to_ascii_lowercase()
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    names.sort();
    names.dedup();
    names
}

pub(super) fn set_header(handle: Handle, name: &str, value: String) {
    if let Some(req) = get_handle_mut::<ClientRequestHandle>(handle) {
        if let Some(existing) = find_header_key(req, name) {
            req.headers.remove(&existing);
        }
        req.headers.insert(name.to_string(), value);
    }
}

fn get_header_by_name(handle: Handle, name: &str) -> Option<String> {
    get_handle_mut::<ClientRequestHandle>(handle).and_then(|req| {
        let key = find_header_key(req, name)?;
        req.headers.get(&key).cloned()
    })
}

fn remove_header_by_name(handle: Handle, name: &str) {
    if let Some(req) = get_handle_mut::<ClientRequestHandle>(handle) {
        if let Some(key) = find_header_key(req, name) {
            req.headers.remove(&key);
        }
    }
}

fn headers_array(handle: Handle, raw: bool) -> f64 {
    let names = header_names(handle, raw);
    let mut arr = perry_runtime::js_array_alloc(names.len() as u32);
    for name in names {
        let ptr = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        arr = perry_runtime::js_array_push(arr, JSValue::string_ptr(ptr));
    }
    f64::from_bits(JSValue::array_ptr(arr).bits())
}

fn headers_object(handle: Handle) -> f64 {
    let mut entries = get_handle_mut::<ClientRequestHandle>(handle)
        .map(|req| {
            req.headers
                .iter()
                .map(|(key, value)| (key.to_ascii_lowercase(), value.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries.dedup_by(|a, b| a.0 == b.0);

    let obj = perry_runtime::js_object_alloc_null_proto(0, entries.len() as u32);
    let mut keys = perry_runtime::js_array_alloc(entries.len() as u32);
    for (index, (key, value)) in entries.iter().enumerate() {
        let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
        let value_ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
        perry_runtime::js_object_set_field(obj, index as u32, JSValue::string_ptr(value_ptr));
        keys = perry_runtime::js_array_push(keys, JSValue::string_ptr(key_ptr));
    }
    perry_runtime::js_object_set_keys(obj, keys);
    f64::from_bits(JSValue::object_ptr(obj as *mut u8).bits())
}

fn socket_value(handle: Handle) -> f64 {
    if !is_client_request_handle(handle) {
        return undefined_value();
    }
    with_state_mut(handle, |state| {
        if state.socket == 0.0 {
            let obj = perry_runtime::js_object_alloc(0, 0);
            state.socket = f64::from_bits(JSValue::object_ptr(obj as *mut u8).bits());
        }
        state.socket
    })
}

fn state_bool(handle: Handle, property: &str) -> f64 {
    let ended = get_handle_mut::<ClientRequestHandle>(handle)
        .map(|req| req.ended)
        .unwrap_or(false);
    let states = CLIENT_REQUEST_SURFACE.lock().unwrap();
    let state = states.get(&handle);
    bool_value(match property {
        "aborted" => state.map(|s| s.aborted).unwrap_or(false),
        "destroyed" => state.map(|s| s.destroyed).unwrap_or(false),
        "finished" | "writableEnded" | "writableFinished" => ended,
        "reusedSocket" => false,
        _ => false,
    })
}

fn string_arg(args: &[f64], index: usize) -> Option<String> {
    args.get(index)
        .copied()
        .and_then(|value| unsafe { extract_string_value(value) })
}

#[no_mangle]
pub unsafe extern "C" fn js_http_client_request_get_header(
    handle: Handle,
    name_ptr: *const StringHeader,
) -> f64 {
    string_from_header(name_ptr)
        .and_then(|name| get_header_by_name(handle, &name))
        .map(|value| string_value(&value))
        .unwrap_or_else(undefined_value)
}

#[no_mangle]
pub unsafe extern "C" fn js_http_client_request_has_header(
    handle: Handle,
    name_ptr: *const StringHeader,
) -> f64 {
    let has = string_from_header(name_ptr)
        .and_then(|name| get_header_by_name(handle, &name))
        .is_some();
    bool_value(has)
}

#[no_mangle]
pub unsafe extern "C" fn js_http_client_request_remove_header(
    handle: Handle,
    name_ptr: *const StringHeader,
) -> f64 {
    if let Some(name) = string_from_header(name_ptr) {
        remove_header_by_name(handle, &name);
    }
    undefined_value()
}

#[no_mangle]
pub extern "C" fn js_http_client_request_get_header_names(handle: Handle) -> f64 {
    headers_array(handle, false)
}

#[no_mangle]
pub extern "C" fn js_http_client_request_get_raw_header_names(handle: Handle) -> f64 {
    headers_array(handle, true)
}

#[no_mangle]
pub extern "C" fn js_http_client_request_get_headers(handle: Handle) -> f64 {
    headers_object(handle)
}

#[no_mangle]
pub extern "C" fn js_http_client_request_abort(handle: Handle) -> f64 {
    if is_client_request_handle(handle) {
        with_state_mut(handle, |state| {
            state.aborted = true;
            state.destroyed = true;
        });
    }
    undefined_value()
}

#[no_mangle]
pub extern "C" fn js_http_client_request_destroy(handle: Handle, _error: f64) -> Handle {
    if is_client_request_handle(handle) {
        with_state_mut(handle, |state| state.destroyed = true);
    }
    handle
}

#[no_mangle]
pub extern "C" fn js_http_client_request_noop_undefined(
    handle: Handle,
    _arg0: f64,
    _arg1: f64,
) -> f64 {
    let _ = handle;
    undefined_value()
}

/// Twin of perry-ext-http's `js_http_client_request_flush_headers` for
/// non-auto-optimize links: the stdlib client dispatches the whole exchange
/// at `end()`, so flushHeaders stays a no-op here.
#[no_mangle]
pub extern "C" fn js_http_client_request_flush_headers(
    handle: Handle,
    _arg0: f64,
    _arg1: f64,
) -> f64 {
    let _ = handle;
    undefined_value()
}

#[no_mangle]
pub extern "C" fn js_http_client_request_aborted(handle: Handle) -> f64 {
    state_bool(handle, "aborted")
}

#[no_mangle]
pub extern "C" fn js_http_client_request_destroyed(handle: Handle) -> f64 {
    state_bool(handle, "destroyed")
}

#[no_mangle]
pub extern "C" fn js_http_client_request_finished(handle: Handle) -> f64 {
    state_bool(handle, "finished")
}

#[no_mangle]
pub extern "C" fn js_http_client_request_reused_socket(handle: Handle) -> f64 {
    state_bool(handle, "reusedSocket")
}

#[no_mangle]
pub extern "C" fn js_http_client_request_max_headers_count(handle: Handle) -> f64 {
    let _ = handle;
    null_value()
}

#[no_mangle]
pub extern "C" fn js_http_client_request_writable_ended(handle: Handle) -> f64 {
    state_bool(handle, "writableEnded")
}

#[no_mangle]
pub extern "C" fn js_http_client_request_writable_finished(handle: Handle) -> f64 {
    state_bool(handle, "writableFinished")
}

#[no_mangle]
pub extern "C" fn js_http_client_request_socket(handle: Handle) -> f64 {
    socket_value(handle)
}

pub(crate) fn dispatch_client_request_property(handle: Handle, property: &str) -> Option<f64> {
    if !is_client_request_handle(handle) {
        return None;
    }
    let method: Option<&'static [u8]> = match property {
        "on" => Some(b"on"),
        "end" => Some(b"end"),
        "write" => Some(b"write"),
        "setHeader" => Some(b"setHeader"),
        "setTimeout" => Some(b"setTimeout"),
        "listenerCount" => Some(b"listenerCount"),
        "getHeader" => Some(b"getHeader"),
        "hasHeader" => Some(b"hasHeader"),
        "removeHeader" => Some(b"removeHeader"),
        "getHeaderNames" => Some(b"getHeaderNames"),
        "getHeaders" => Some(b"getHeaders"),
        "getRawHeaderNames" => Some(b"getRawHeaderNames"),
        "abort" => Some(b"abort"),
        "destroy" => Some(b"destroy"),
        "flushHeaders" => Some(b"flushHeaders"),
        "cork" => Some(b"cork"),
        "uncork" => Some(b"uncork"),
        "setNoDelay" => Some(b"setNoDelay"),
        "setSocketKeepAlive" => Some(b"setSocketKeepAlive"),
        _ => None,
    };
    if let Some(name) = method {
        return Some(unsafe {
            js_class_method_bind(handle_value(handle), name.as_ptr(), name.len())
        });
    }
    Some(match property {
        "method" => {
            f64::from_bits(JSValue::string_ptr(js_http_client_request_method(handle)).bits())
        }
        "protocol" => {
            f64::from_bits(JSValue::string_ptr(js_http_client_request_protocol(handle)).bits())
        }
        "host" => f64::from_bits(JSValue::string_ptr(js_http_client_request_host(handle)).bits()),
        "path" => f64::from_bits(JSValue::string_ptr(js_http_client_request_path(handle)).bits()),
        "aborted" => js_http_client_request_aborted(handle),
        "destroyed" => js_http_client_request_destroyed(handle),
        "finished" => js_http_client_request_finished(handle),
        "reusedSocket" => js_http_client_request_reused_socket(handle),
        "maxHeadersCount" => js_http_client_request_max_headers_count(handle),
        "writableEnded" => js_http_client_request_writable_ended(handle),
        "writableFinished" => js_http_client_request_writable_finished(handle),
        "socket" | "connection" => js_http_client_request_socket(handle),
        _ => return None,
    })
}

pub(crate) fn dispatch_client_request_method(
    handle: Handle,
    method: &str,
    args: &[f64],
) -> Option<f64> {
    if !is_client_request_handle(handle) {
        return None;
    }
    Some(match method {
        "setHeader" => {
            let name = string_arg(args, 0).unwrap_or_default();
            let value = string_arg(args, 1).unwrap_or_default();
            set_header(handle, &name, value);
            handle_value(handle)
        }
        "getHeader" => string_arg(args, 0)
            .and_then(|name| get_header_by_name(handle, &name))
            .map(|value| string_value(&value))
            .unwrap_or_else(undefined_value),
        "hasHeader" => bool_value(
            string_arg(args, 0)
                .and_then(|name| get_header_by_name(handle, &name))
                .is_some(),
        ),
        "removeHeader" => {
            if let Some(name) = string_arg(args, 0) {
                remove_header_by_name(handle, &name);
            }
            undefined_value()
        }
        "getHeaderNames" => headers_array(handle, false),
        "getHeaders" => headers_object(handle),
        "getRawHeaderNames" => headers_array(handle, true),
        "listenerCount" => {
            let event = string_arg(args, 0).unwrap_or_default();
            get_handle_mut::<ClientRequestHandle>(handle)
                .map(|req| {
                    let explicit = req.listeners.get(&event).map(|v| v.len()).unwrap_or(0);
                    let implicit_response = if event == "response" && req.response_callback != 0 {
                        1
                    } else {
                        0
                    };
                    (explicit + implicit_response) as f64
                })
                .unwrap_or(0.0)
        }
        "abort" => js_http_client_request_abort(handle),
        "destroy" => handle_value(js_http_client_request_destroy(handle, undefined_value())),
        "flushHeaders" | "cork" | "uncork" | "setNoDelay" | "setSocketKeepAlive" => {
            undefined_value()
        }
        _ => return None,
    })
}
