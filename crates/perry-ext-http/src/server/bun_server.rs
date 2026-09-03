//! Bun-compatible `serve()` facade over Perry's native HTTP server.

use std::collections::HashMap;
use std::os::raw::c_int;
use std::sync::Mutex;

use lazy_static::lazy_static;
use perry_ffi::{
    alloc_null_proto_object, alloc_string, get_handle, get_handle_mut, register_handle, ErrorKind,
    GcRootVisitor, JsClosure, JsValue, RawClosureHeader, StringHeader, TransientRootScope,
};

use crate::server::request::{handle_to_pointer_f64, with_implicit_this, IncomingMessage};
use crate::server::response::ServerResponse;
use crate::server::server::{
    finalize_or_park_request, synthesize_default_response_if_needed, HttpPendingRequest, HttpServer,
};
use crate::server::types::{
    js_handle_clear_side_tables, js_promise_run_microtasks, js_value_is_closure,
    read_string_header, PTR_MASK, TAG_NULL, TAG_UNDEFINED,
};

#[repr(C)]
struct Promise {
    _opaque: [u8; 0],
}

// Standalone ext-http unit tests do not link the selected stdlib provider.
// The Perry integration test exercises the real cross-crate bridge.
#[cfg(test)]
unsafe fn js_bun_http_request_from_json(_snapshot_ptr: *const StringHeader) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

#[cfg(test)]
unsafe fn js_bun_http_response_snapshot_json(_response_handle: f64) -> *mut StringHeader {
    std::ptr::null_mut()
}

#[cfg(not(test))]
extern "C" {
    fn js_bun_http_request_from_json(snapshot_ptr: *const StringHeader) -> f64;
    fn js_bun_http_response_snapshot_json(response_handle: f64) -> *mut StringHeader;
}

extern "C" {
    fn js_value_is_promise(value: f64) -> i32;
    fn js_promise_state(ptr: *mut Promise) -> i32;
    fn js_promise_value(ptr: *mut Promise) -> f64;
    fn js_promise_reason(ptr: *mut Promise) -> f64;
    fn js_promise_resolved(value: f64) -> *mut Promise;
    fn js_try_push() -> *mut c_int;
    fn js_try_end();
    fn js_get_exception() -> f64;
    fn js_clear_exception();
    fn js_jsvalue_to_string(value: f64) -> *mut StringHeader;
    fn js_object_get_field_by_name(
        obj: *const perry_ffi::ObjectHeader,
        key: *const StringHeader,
    ) -> JsValue;
    fn perry_sjlj_try(
        env: *mut core::ffi::c_void,
        body: unsafe extern "C" fn(*mut core::ffi::c_void),
        ctx: *mut core::ffi::c_void,
    ) -> c_int;
}

#[derive(Clone, Copy)]
enum PromiseStage {
    Fetch,
    Error,
}

struct PendingPromise {
    server_handle: i64,
    request_handle: i64,
    response_handle: i64,
    fetch_request: f64,
    promise: i64,
    stage: PromiseStage,
}

struct ResponseSnapshot {
    status: u16,
    status_text: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl ResponseSnapshot {
    fn from_json(json: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(json).ok()?;
        let status = value.get("status")?.as_u64()?.try_into().ok()?;
        let status_text = value.get("status_text")?.as_str()?.to_string();
        let headers = value
            .get("headers")?
            .as_array()?
            .iter()
            .map(|entry| {
                let pair = entry.as_array()?;
                Some((
                    pair.first()?.as_str()?.to_string(),
                    pair.get(1)?.as_str()?.to_string(),
                ))
            })
            .collect::<Option<Vec<_>>>()?;
        let body = value
            .get("body")?
            .as_array()?
            .iter()
            .map(|byte| byte.as_u64().and_then(|byte| byte.try_into().ok()))
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            status,
            status_text,
            headers,
            body,
        })
    }
}

lazy_static! {
    static ref PENDING_PROMISES: Mutex<Vec<PendingPromise>> = Mutex::new(Vec::new());
    static ref REQUEST_IPS: Mutex<HashMap<usize, (usize, String, u16)>> =
        Mutex::new(HashMap::new());
}

#[repr(C)]
struct InlineArgsHeader {
    length: u32,
    capacity: u32,
    args: [u64; 1],
}

struct ClosureCallResult {
    value: f64,
    thrown: Option<f64>,
}

fn arm_trap_and_run<R, F: FnOnce() -> R>(env: *mut c_int, f: F) -> Option<R> {
    struct Ctx<F, R> {
        f: Option<F>,
        ret: Option<R>,
    }
    unsafe extern "C" fn invoke<F: FnOnce() -> R, R>(raw: *mut core::ffi::c_void) {
        let ctx = unsafe { &mut *(raw as *mut Ctx<F, R>) };
        let f = ctx.f.take().expect("sjlj trampoline invoked body twice");
        ctx.ret = Some(f());
    }
    let mut ctx = Ctx {
        f: Some(f),
        ret: None,
    };
    let rc = unsafe {
        perry_sjlj_try(
            env as *mut core::ffi::c_void,
            invoke::<F, R>,
            &mut ctx as *mut Ctx<_, R> as *mut core::ffi::c_void,
        )
    };
    if rc == 0 {
        Some(
            ctx.ret
                .take()
                .expect("sjlj trampoline returned 0 without a body result"),
        )
    } else {
        None
    }
}

unsafe fn call_catching(f: impl FnOnce() -> f64) -> ClosureCallResult {
    let trap = js_try_push();
    match arm_trap_and_run(trap, f) {
        Some(value) => {
            js_try_end();
            ClosureCallResult {
                value,
                thrown: None,
            }
        }
        None => {
            let exception = js_get_exception();
            js_clear_exception();
            js_try_end();
            ClosureCallResult {
                value: f64::from_bits(TAG_UNDEFINED),
                thrown: Some(exception),
            }
        }
    }
}

pub(crate) fn scan_pending_roots(visitor: &mut GcRootVisitor<'_>) {
    if let Ok(mut pending) = PENDING_PROMISES.lock() {
        for entry in pending.iter_mut() {
            visitor.visit_i64_slot(&mut entry.promise);
        }
    }
}

pub(crate) fn is_bun_server(handle: i64) -> bool {
    get_handle::<HttpServer>(handle)
        .map(|server| server.is_bun_server)
        .unwrap_or(false)
}

fn field(options: &perry_ffi::TransientRootedNanbox, name: &str) -> JsValue {
    // Allocate the key first, then re-read the rooted options pointer: key
    // allocation may run a moving collection.
    let key = alloc_string(name);
    let object =
        JsValue::from_bits(options.get().to_bits()).as_pointer::<perry_ffi::ObjectHeader>();
    if object.is_null() {
        JsValue::UNDEFINED
    } else {
        unsafe { js_object_get_field_by_name(object, key.as_raw()) }
    }
}

fn field_is_present(options: &perry_ffi::TransientRootedNanbox, name: &str) -> bool {
    let value = field(options, name);
    !value.is_undefined() && !value.is_null() && value != JsValue::FALSE
}

/// `Bun.serve(options)` — create and synchronously bind an HTTP server.
#[no_mangle]
pub unsafe extern "C" fn js_bun_serve(options: f64) -> i64 {
    let options_value = JsValue::from_bits(options.to_bits());
    if !options_value.is_pointer() {
        perry_ffi::throw_with_code(
            "Bun.serve requires an options object",
            "ERR_INVALID_ARG_TYPE",
            ErrorKind::TypeError,
        );
    }

    let scope = TransientRootScope::enter();
    let options = scope.root_nanbox(options);
    if ["tls", "key", "cert", "ca"]
        .iter()
        .any(|name| field_is_present(&options, name))
    {
        perry_ffi::throw_with_code(
            "Bun.serve TLS options are not supported by Perry yet",
            "ERR_NOT_SUPPORTED",
            ErrorKind::Error,
        );
    }

    let fetch = scope.root_nanbox(f64::from_bits(field(&options, "fetch").bits()));
    if js_value_is_closure(fetch.get().to_bits() as i64) == 0 {
        perry_ffi::throw_with_code(
            "Bun.serve requires a fetch function",
            "ERR_INVALID_ARG_TYPE",
            ErrorKind::TypeError,
        );
    }
    let error = scope.root_nanbox(f64::from_bits(field(&options, "error").bits()));
    if !JsValue::from_bits(error.get().to_bits()).is_undefined()
        && js_value_is_closure(error.get().to_bits() as i64) == 0
    {
        perry_ffi::throw_with_code(
            "Bun.serve error must be a function",
            "ERR_INVALID_ARG_TYPE",
            ErrorKind::TypeError,
        );
    }

    let development = field(&options, "development");
    let idle_timeout = field(&options, "idleTimeout");
    let mut server = HttpServer::with_handler((fetch.get().to_bits() & PTR_MASK) as i64);
    server.is_bun_server = true;
    server.bun_error_handler = if js_value_is_closure(error.get().to_bits() as i64) != 0 {
        (error.get().to_bits() & PTR_MASK) as i64
    } else {
        0
    };
    server.bun_development = development.to_bool();
    if idle_timeout.is_number() {
        let seconds = idle_timeout.to_number();
        if seconds.is_finite() && seconds >= 0.0 {
            server.idle_timeout = seconds * 1_000.0;
        }
    }

    crate::server::ensure_gc_scanner_registered();
    let handle = register_handle(server);
    let args = InlineArgsHeader {
        length: 1,
        capacity: 1,
        args: [options.get().to_bits()],
    };
    crate::server::server::js_node_http_server_listen(handle, &args as *const _ as i64);
    if !get_handle::<HttpServer>(handle)
        .map(|server| server.listening)
        .unwrap_or(false)
    {
        perry_ffi::drop_handle(handle);
        perry_ffi::throw_with_code(
            "Bun.serve failed to bind the requested address",
            "ERR_SERVER_NOT_RUNNING",
            ErrorKind::Error,
        );
    }
    handle
}

fn fetch_request_id(request: f64) -> usize {
    (request.to_bits() & PTR_MASK) as usize
}

fn request_url(server_handle: i64, request: &IncomingMessage) -> String {
    if request.url.starts_with("http://") || request.url.starts_with("https://") {
        return request.url.clone();
    }
    let authority = request.headers.get("host").cloned().unwrap_or_else(|| {
        get_handle::<HttpServer>(server_handle)
            .map(|server| {
                let host = if server.bound_host.contains(':') && !server.bound_host.starts_with('[')
                {
                    format!("[{}]", server.bound_host)
                } else {
                    server.bound_host.clone()
                };
                format!("{}:{}", host, server.bound_port)
            })
            .unwrap_or_else(|| "127.0.0.1".to_string())
    });
    let path = if request.url.starts_with('/') {
        request.url.clone()
    } else {
        format!("/{}", request.url)
    };
    format!("http://{authority}{path}")
}

fn make_fetch_request(server_handle: i64, request_handle: i64) -> Option<f64> {
    let request = get_handle::<IncomingMessage>(request_handle)?;
    let body = if request.body_bytes.is_empty() {
        None
    } else {
        Some(request.body_bytes.clone())
    };
    let snapshot = serde_json::json!({
        "url": request_url(server_handle, request),
        "method": request.method,
        "headers": request.raw_headers,
        "body": body,
    });
    let json = serde_json::to_string(&snapshot).ok()?;
    let json = alloc_string(&json);
    let fetch_request = unsafe { js_bun_http_request_from_json(json.as_raw()) };
    if JsValue::from_bits(fetch_request.to_bits()).is_undefined() {
        return None;
    }
    REQUEST_IPS.lock().ok()?.insert(
        fetch_request_id(fetch_request),
        (
            server_handle as usize,
            request.remote_address.clone(),
            request.remote_port,
        ),
    );
    Some(fetch_request)
}

fn handler_for(server_handle: i64, error: bool) -> i64 {
    get_handle::<HttpServer>(server_handle)
        .map(|server| {
            if error {
                server.bun_error_handler
            } else {
                server.handler
            }
        })
        .unwrap_or(0)
}

fn invoke_fetch(server_handle: i64, request: f64) -> ClosureCallResult {
    let scope = TransientRootScope::enter();
    let handler = scope.root_addr(handler_for(server_handle, false));
    if handler.get() == 0 {
        return ClosureCallResult {
            value: f64::from_bits(TAG_UNDEFINED),
            thrown: Some(f64::from_bits(
                perry_ffi::error_value_with_code(
                    "Bun.serve fetch handler is unavailable",
                    "ERR_INVALID_STATE",
                    ErrorKind::Error,
                )
                .bits(),
            )),
        };
    }
    let closure = unsafe { JsClosure::from_raw(handler.get() as *const RawClosureHeader) };
    let server = handle_to_pointer_f64(server_handle);
    unsafe { call_catching(|| with_implicit_this(server, || closure.call2(request, server))) }
}

fn invoke_error(server_handle: i64, reason: f64) -> Option<ClosureCallResult> {
    let scope = TransientRootScope::enter();
    let reason = scope.root_nanbox(reason);
    let handler = scope.root_addr(handler_for(server_handle, true));
    if handler.get() == 0 {
        return None;
    }
    let closure = unsafe { JsClosure::from_raw(handler.get() as *const RawClosureHeader) };
    let server = handle_to_pointer_f64(server_handle);
    Some(unsafe { call_catching(|| with_implicit_this(server, || closure.call1(reason.get()))) })
}

fn apply_response(response_handle: i64, value: f64) -> bool {
    let snapshot_ptr = unsafe { js_bun_http_response_snapshot_json(value) };
    let Some(snapshot_json) = read_string_header(snapshot_ptr) else {
        return false;
    };
    let Some(snapshot) = ResponseSnapshot::from_json(&snapshot_json) else {
        return false;
    };
    let Some(response) = get_handle_mut::<ServerResponse>(response_handle) else {
        return false;
    };
    response.status_code = snapshot.status;
    response.status_message = (!snapshot.status_text.is_empty()).then_some(snapshot.status_text);
    response.headers.clear();
    response.header_value_lists.clear();
    response.raw_header_names.clear();
    response.header_order.clear();
    for (name, value) in snapshot.headers {
        let lower = name.to_ascii_lowercase();
        if let Some(previous) = response.headers.get(&lower).cloned() {
            response
                .header_value_lists
                .entry(lower.clone())
                .or_insert_with(|| vec![previous])
                .push(value.clone());
        } else {
            response.header_order.push(lower.clone());
            response.raw_header_names.insert(lower.clone(), name);
        }
        response.headers.insert(lower, value);
    }
    response.buffered_body = snapshot.body;
    synthesize_default_response_if_needed(response_handle);
    true
}

fn remove_request_ip(fetch_request: f64) {
    if let Ok(mut ips) = REQUEST_IPS.lock() {
        ips.remove(&fetch_request_id(fetch_request));
    }
}

fn error_message(reason: f64) -> String {
    let scope = TransientRootScope::enter();
    let reason = scope.root_nanbox(reason);
    let ptr = unsafe { js_jsvalue_to_string(reason.get()) };
    read_string_header(ptr).unwrap_or_else(|| "Internal Server Error".to_string())
}

fn apply_default_error(response_handle: i64, reason: f64) {
    let message = error_message(reason);
    if let Some(response) = get_handle_mut::<ServerResponse>(response_handle) {
        response.status_code = 500;
        response.headers.insert(
            "content-type".to_string(),
            "text/plain;charset=utf-8".to_string(),
        );
        response
            .raw_header_names
            .insert("content-type".to_string(), "Content-Type".to_string());
        response.header_order.push("content-type".to_string());
        response.buffered_body = message.into_bytes();
    }
    synthesize_default_response_if_needed(response_handle);
}

fn queue_promise(
    context: &HttpPendingRequest,
    fetch_request: f64,
    promise: *mut Promise,
    stage: PromiseStage,
) {
    let entry = PendingPromise {
        server_handle: context.server_handle,
        request_handle: context.request_handle,
        response_handle: context.response_handle,
        fetch_request,
        promise: promise as i64,
        stage,
    };
    match PENDING_PROMISES.lock() {
        Ok(mut pending) => pending.push(entry),
        Err(_) => {
            let reason = f64::from_bits(
                perry_ffi::error_value_with_code(
                    "Bun.serve promise queue is unavailable",
                    "ERR_INVALID_STATE",
                    ErrorKind::Error,
                )
                .bits(),
            );
            apply_default_error(context.response_handle, reason);
            remove_request_ip(fetch_request);
        }
    }
}

fn settle_value(context: &HttpPendingRequest, fetch_request: f64, value: f64, stage: PromiseStage) {
    let js_value = JsValue::from_bits(value.to_bits());
    if js_value.is_pointer() && unsafe { js_value_is_promise(value) } != 0 {
        let promise = js_value.as_pointer::<Promise>();
        if !promise.is_null() {
            match unsafe { js_promise_state(promise) } {
                1 => {
                    let value = unsafe { js_promise_value(promise) };
                    settle_value(context, fetch_request, value, stage);
                }
                2 => {
                    let reason = unsafe { js_promise_reason(promise) };
                    settle_failure(context, fetch_request, reason, stage);
                }
                _ => queue_promise(context, fetch_request, promise, stage),
            }
            return;
        }
    }

    if apply_response(context.response_handle, value) {
        remove_request_ip(fetch_request);
    } else {
        let reason = f64::from_bits(
            perry_ffi::error_value_with_code(
                "Bun.serve handlers must return a Response",
                "ERR_INVALID_RETURN_VALUE",
                ErrorKind::TypeError,
            )
            .bits(),
        );
        settle_failure(context, fetch_request, reason, stage);
    }
}

fn settle_failure(
    context: &HttpPendingRequest,
    fetch_request: f64,
    reason: f64,
    stage: PromiseStage,
) {
    if matches!(stage, PromiseStage::Error) {
        apply_default_error(context.response_handle, reason);
        remove_request_ip(fetch_request);
        return;
    }
    match invoke_error(context.server_handle, reason) {
        Some(call) => {
            if let Some(thrown) = call.thrown {
                apply_default_error(context.response_handle, thrown);
                remove_request_ip(fetch_request);
            } else {
                settle_value(context, fetch_request, call.value, PromiseStage::Error);
            }
        }
        None => {
            apply_default_error(context.response_handle, reason);
            remove_request_ip(fetch_request);
        }
    }
}

pub(crate) fn process_request(pending: HttpPendingRequest) {
    unsafe {
        js_handle_clear_side_tables(pending.request_handle);
        js_handle_clear_side_tables(pending.response_handle);
    }
    let Some(fetch_request) = make_fetch_request(pending.server_handle, pending.request_handle)
    else {
        let reason = f64::from_bits(
            perry_ffi::error_value_with_code(
                "Bun.serve could not construct the Request",
                "ERR_INVALID_STATE",
                ErrorKind::Error,
            )
            .bits(),
        );
        settle_failure(
            &pending,
            f64::from_bits(TAG_UNDEFINED),
            reason,
            PromiseStage::Fetch,
        );
        finalize_or_park_request(&pending);
        return;
    };

    let call = invoke_fetch(pending.server_handle, fetch_request);
    if let Some(reason) = call.thrown {
        settle_failure(&pending, fetch_request, reason, PromiseStage::Fetch);
    } else {
        let scope = TransientRootScope::enter();
        let result = scope.root_nanbox(call.value);
        unsafe {
            js_promise_run_microtasks();
        }
        settle_value(&pending, fetch_request, result.get(), PromiseStage::Fetch);
    }
    finalize_or_park_request(&pending);
}

/// Poll promise-returning fetch/error handlers without blocking the JS thread.
pub(crate) fn process_pending_promises() -> i32 {
    let mut settled = Vec::new();
    if let Ok(mut pending) = PENDING_PROMISES.lock() {
        let mut index = 0;
        while index < pending.len() {
            let promise = pending[index].promise as *mut Promise;
            let state = if promise.is_null() {
                2
            } else {
                unsafe { js_promise_state(promise) }
            };
            let handles_alive = get_handle::<IncomingMessage>(pending[index].request_handle)
                .is_some()
                && get_handle::<ServerResponse>(pending[index].response_handle).is_some();
            if state != 0 || !handles_alive {
                settled.push((pending.remove(index), state, handles_alive));
            } else {
                index += 1;
            }
        }
    }

    let count = settled.len() as i32;
    for (entry, state, handles_alive) in settled {
        if !handles_alive {
            remove_request_ip(entry.fetch_request);
            continue;
        }
        let context = HttpPendingRequest {
            server_handle: entry.server_handle,
            request_handle: entry.request_handle,
            response_handle: entry.response_handle,
            skip_default_response: false,
            h2_stream_handle: 0,
            h2_stream_headers: Vec::new(),
            is_check_continue: false,
        };
        let promise = entry.promise as *mut Promise;
        if state == 1 {
            settle_value(
                &context,
                entry.fetch_request,
                unsafe { js_promise_value(promise) },
                entry.stage,
            );
        } else {
            let reason = if promise.is_null() {
                f64::from_bits(
                    perry_ffi::error_value_with_code(
                        "Bun.serve handler promise became unavailable",
                        "ERR_INVALID_STATE",
                        ErrorKind::Error,
                    )
                    .bits(),
                )
            } else {
                unsafe { js_promise_reason(promise) }
            };
            settle_failure(&context, entry.fetch_request, reason, entry.stage);
        }
    }
    count
}

/// `server.requestIP(request)`.
#[no_mangle]
pub extern "C" fn js_bun_server_request_ip(server_handle: i64, request: f64) -> f64 {
    let Some((address, port)) = REQUEST_IPS.lock().ok().and_then(|ips| {
        ips.get(&fetch_request_id(request))
            .filter(|(owner, _, _)| *owner == server_handle as usize)
            .map(|(_, address, port)| (address.clone(), *port))
    }) else {
        return f64::from_bits(TAG_NULL);
    };
    let family = if address.contains(':') {
        "IPv6"
    } else {
        "IPv4"
    };
    let scope = TransientRootScope::enter();
    let address = scope.root_nanbox(f64::from_bits(
        JsValue::from_string_ptr(alloc_string(&address).as_raw()).bits(),
    ));
    let family = scope.root_nanbox(f64::from_bits(
        JsValue::from_string_ptr(alloc_string(family).as_raw()).bits(),
    ));
    f64::from_bits(
        alloc_null_proto_object(&[
            ("address", JsValue::from_bits(address.get().to_bits())),
            ("port", JsValue::from_number(port as f64)),
            ("family", JsValue::from_bits(family.get().to_bits())),
        ])
        .bits(),
    )
}

fn cancel_pending_for_server(server_handle: i64) {
    let mut requests = Vec::new();
    if let Ok(mut pending) = PENDING_PROMISES.lock() {
        pending.retain(|entry| {
            if entry.server_handle == server_handle {
                requests.push(entry.fetch_request);
                false
            } else {
                true
            }
        });
    }
    for request in requests {
        remove_request_ip(request);
    }
}

/// `server.stop(closeActiveConnections?)`.
#[no_mangle]
pub unsafe extern "C" fn js_bun_server_stop(server_handle: i64, close_active: f64) -> f64 {
    crate::server::server::js_node_http_server_close(server_handle, 0);
    if JsValue::from_bits(close_active.to_bits()).to_bool() {
        cancel_pending_for_server(server_handle);
        crate::server::server::js_node_http_server_close_all_connections(server_handle);
    }
    let promise = js_promise_resolved(f64::from_bits(TAG_UNDEFINED));
    f64::from_bits(JsValue::from_object_ptr(promise).bits())
}

pub(crate) fn hostname(handle: i64) -> Option<String> {
    get_handle::<HttpServer>(handle)
        .filter(|server| server.is_bun_server)
        .map(|server| server.bound_host.clone())
}

pub(crate) fn port(handle: i64) -> Option<u16> {
    get_handle::<HttpServer>(handle)
        .filter(|server| server.is_bun_server)
        .map(|server| server.bound_port)
}

pub(crate) fn development(handle: i64) -> Option<bool> {
    get_handle::<HttpServer>(handle)
        .filter(|server| server.is_bun_server)
        .map(|server| server.bun_development)
}
