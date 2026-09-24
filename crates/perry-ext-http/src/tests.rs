use super::*;
use perry_ffi::{drop_handle, get_handle, register_handle};
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

pub(crate) static GC_TEST_LOCK: Mutex<()> = Mutex::new(());

struct GcTestGuard {
    frame: u64,
    previous_force_evacuation: i32,
    _lock: MutexGuard<'static, ()>,
}

impl GcTestGuard {
    fn new() -> Self {
        let lock = GC_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Rewriting is observable only when the collector moves the root.
        // Keep that test policy thread-local so unrelated test threads do not
        // observe a process-wide environment mutation.
        let previous_force_evacuation = perry_runtime::gc::js_gc_force_evacuation_test_override(1);
        perry_runtime::gc::js_gc_write_barriers_emitted(1);
        let frame = perry_runtime::gc::js_shadow_frame_push(0);
        Self {
            frame,
            previous_force_evacuation,
            _lock: lock,
        }
    }
}

impl Drop for GcTestGuard {
    fn drop(&mut self) {
        perry_runtime::gc::js_shadow_frame_pop(self.frame);
        perry_runtime::gc::js_gc_write_barriers_emitted(0);
        perry_runtime::gc::js_gc_force_evacuation_test_override(self.previous_force_evacuation);
    }
}

fn young_gc_root() -> i64 {
    perry_runtime::arena::arena_alloc_gc(32, 8, perry_runtime::gc::GC_TYPE_STRING) as i64
}

fn assert_rewritten(before: i64, after: i64) {
    assert_ne!(after, before);
    assert!(perry_runtime::arena::pointer_in_nursery(after as usize));
}

#[test]
fn gc_scanner_registers_idempotently() {
    // Calling ensure_gc_scanner_registered twice must not panic
    // and must not register the scanner twice (Once guarantees).
    ensure_gc_scanner_registered();
    ensure_gc_scanner_registered();
    ensure_gc_scanner_registered();
}

#[test]
fn gc_mutable_scanner_rewrites_request_response_listener_roots() {
    let _guard = GcTestGuard::new();
    perry_ffi::gc_register_mutable_root_scanner_named("perry-ext-http", scan_http_roots);

    let response_callback = young_gc_root();
    let response_raw_wrapper = young_gc_root();
    let request_listener = young_gc_root();
    let request_once_callback = young_gc_root();
    let request_once_wrapper = young_gc_root();
    let incoming_listener = young_gc_root();
    let mut request_listeners = HashMap::new();
    request_listeners.insert(
        "error".to_string(),
        vec![ClientEventListener::persistent(request_listener)],
    );
    request_listeners.insert(
        "timeout".to_string(),
        vec![ClientEventListener {
            callback: request_once_callback,
            raw_wrapper: request_once_wrapper,
            once: true,
        }],
    );
    let request_handle = register_handle(ClientRequestHandle {
        async_id: 0,
        method: "GET".to_string(),
        url: "http://localhost/".to_string(),
        headers: HashMap::new(),
        body: Vec::new(),
        response_callback,
        response_raw_wrapper,
        listeners: request_listeners,
        timeout_ms: None,
        ended: false,
        flushed_early: false,
        pending_write_callbacks: Vec::new(),
        end_callback: 0,
        completed: false,
        timeout_fired: false,
        close_emitted: false,
        agent_handle: 0,
        agent_key: "localhost::".to_string(),
        request_create_connection: 0,
        agent_active: false,
        agent_queued: false,
        reused_socket: false,
        socket_handle: 0,
        abort_signal_bits: 0,
        abort_listener_bits: 0,
        tls: crate::tls_client::TlsOptions::default(),
        preflight_error: None,
        incoming_handle: 0,
        expects_continue: false,
        continue_body_pending: false,
    });

    let mut incoming_listeners = HashMap::new();
    incoming_listeners.insert("data".to_string(), vec![incoming_listener]);
    let incoming_handle = register_handle(IncomingMessageHandle {
        status_code: 200,
        status_message: "OK".to_string(),
        headers: Vec::new(),
        trailers: HashMap::new(),
        body: Vec::new(),
        listeners: incoming_listeners,
        encoding: None,
        decoder_pending: Vec::new(),
        pipes: Vec::new(),
        socket_handle: 0,
        request_handle,
        http_version: (1, 1),
        complete: true,
    });

    let _ = perry_runtime::gc::gc_collect_minor();

    {
        let req = get_handle::<ClientRequestHandle>(request_handle)
            .expect("request handle should remain live");
        assert_rewritten(response_callback, req.response_callback);
        assert_rewritten(response_raw_wrapper, req.response_raw_wrapper);
        assert_rewritten(request_listener, req.listeners["error"][0].callback);
        assert_rewritten(request_once_callback, req.listeners["timeout"][0].callback);
        assert_rewritten(
            request_once_wrapper,
            req.listeners["timeout"][0].raw_wrapper,
        );
        let msg = get_handle::<IncomingMessageHandle>(incoming_handle)
            .expect("incoming message handle should remain live");
        assert_rewritten(incoming_listener, msg.listeners["data"][0]);
        assert_eq!(msg.request_handle, request_handle);
        assert_eq!(
            js_http_incoming_message_req(incoming_handle).to_bits(),
            POINTER_TAG | (request_handle as u64 & PTR_MASK),
            "client IncomingMessage.req must expose its paired ClientRequest"
        );
    }
    drop_handle(request_handle);
    drop_handle(incoming_handle);
}

/// The streamed-response drain (`ResponseHead` → N×`ResponseChunk` →
/// `ResponseEnd`) must reassemble a body byte-identically no matter how the
/// transport split it into chunks. The chunk carrier (`Bytes`) delivers to
/// the drain as `&[u8]`, so this pins the reassembly contract a carrier-type
/// change must preserve — a future refactor that corrupted a chunk,
/// reordered chunks, or mishandled a boundary would fail here.
///
/// Drives the buffering branch (no `'data'` listener registered): each
/// chunk is appended to `IncomingMessageHandle::body`, and `ResponseEnd`
/// leaves the unconsumed body on the handle. Reading it back is the
/// reassembly assertion. This branch never calls a JS closure, so it needs
/// no live codegen — only the handle registry the other tests already use.
fn drain_streamed_body(chunks: &[&[u8]]) -> Vec<u8> {
    let request_handle = register_handle(ClientRequestHandle {
        async_id: 0,
        method: "GET".to_string(),
        url: "http://localhost/".to_string(),
        headers: HashMap::new(),
        body: Vec::new(),
        response_callback: 0,
        response_raw_wrapper: 0,
        listeners: HashMap::new(),
        timeout_ms: None,
        ended: false,
        flushed_early: false,
        pending_write_callbacks: Vec::new(),
        end_callback: 0,
        completed: false,
        timeout_fired: false,
        close_emitted: false,
        agent_handle: 0,
        agent_key: "localhost::".to_string(),
        request_create_connection: 0,
        agent_active: false,
        agent_queued: false,
        reused_socket: false,
        socket_handle: 0,
        abort_signal_bits: 0,
        abort_listener_bits: 0,
        tls: crate::tls_client::TlsOptions::default(),
        preflight_error: None,
        incoming_handle: 0,
        expects_continue: false,
        continue_body_pending: false,
    });

    unsafe {
        // Head: allocates the IncomingMessage and stores its handle on the
        // request, so the following chunk/end events route to it.
        client_events::handle_response_head_event(
            request_handle,
            200,
            "OK".to_string(),
            Vec::new(),
            (1, 1),
        );
        // Each production chunk is a refcounted `Bytes` (reqwest's
        // `response.chunk()` shape) — build the input the same way so the
        // test exercises the actual carrier type the drain receives.
        for c in chunks {
            client_events::handle_response_chunk_event(request_handle, Bytes::copy_from_slice(c));
        }
        client_events::handle_response_end_event(request_handle);
    }

    let incoming_handle = get_handle::<ClientRequestHandle>(request_handle)
        .expect("request handle should remain live")
        .incoming_handle;
    let body = get_handle::<IncomingMessageHandle>(incoming_handle)
        .expect("incoming message handle should remain live")
        .body
        .clone();

    drop_handle(incoming_handle);
    drop_handle(request_handle);
    body
}

#[test]
fn streamed_response_reassembles_chunks_byte_identically() {
    let _guard = GcTestGuard::new();

    // Empty body — zero chunks then end.
    assert_eq!(drain_streamed_body(&[]), Vec::<u8>::new());

    // Single chunk delivered whole.
    assert_eq!(drain_streamed_body(&[b"hello world"]), b"hello world");

    // Multi-chunk: the reassembled body is the in-order concatenation,
    // independent of the (arbitrary) chunk boundaries the transport chose.
    assert_eq!(drain_streamed_body(&[b"foo", b"bar", b"baz"]), b"foobarbaz");

    // Boundary-shift: the SAME bytes split differently must reassemble to
    // the same body — the property the streaming path actually guarantees.
    let payload: &[u8] = b"the quick brown fox jumps over the lazy dog";
    let split_a = drain_streamed_body(&[&payload[..10], &payload[10..25], &payload[25..]]);
    let split_b = drain_streamed_body(&[&payload[..1], &payload[1..2], &payload[2..]]);
    assert_eq!(split_a, payload);
    assert_eq!(split_b, payload);

    // Binary payload with embedded NULs and high bytes — the carrier is
    // bytes, not a string, so nothing is lost or re-encoded.
    let bin: &[u8] = &[0x00, 0xFF, 0x10, 0x00, 0x80, 0x7F, 0xC3, 0x28];
    assert_eq!(drain_streamed_body(&[&bin[..3], &bin[3..]]), bin);
}

#[test]
fn streamed_text_decoder_preserves_base64_and_utf16_boundaries() {
    let _guard = GcTestGuard::new();

    let mut pending = Vec::new();
    assert!(
        client_surface::streaming_body_chunk_value(b"a", Some("base64"), &mut pending, false,)
            .is_none()
    );
    assert!(
        client_surface::streaming_body_chunk_value(b"b", Some("base64"), &mut pending, false,)
            .is_none()
    );
    let final_chunk =
        client_surface::streaming_body_chunk_value(b"", Some("base64"), &mut pending, true)
            .expect("base64 remainder must flush at end");
    assert_eq!(
        unsafe { extract_string_value(final_chunk) }.as_deref(),
        Some("YWI=")
    );

    let mut pending = Vec::new();
    assert!(client_surface::streaming_body_chunk_value(
        &[b'a'],
        Some("utf16le"),
        &mut pending,
        false,
    )
    .is_none());
    let chunk =
        client_surface::streaming_body_chunk_value(&[0], Some("utf16le"), &mut pending, false)
            .expect("split UTF-16 code unit must decode when completed");
    assert_eq!(unsafe { extract_string_value(chunk) }.as_deref(), Some("a"));
    assert!(pending.is_empty());
}

#[test]
fn has_pending_zero_when_idle() {
    // Serialize with tests that queue real events (the in-flight-guard test
    // below) — clearing the shared queue under their feet would break them.
    let _lock = GC_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // Drain anything other tests left; then assert zero.
    let _ = HTTP_PENDING_EVENTS.lock().map(|mut q| q.clear());
    assert_eq!(js_http_has_pending(), 0);
}

// `dispatch_request_stays_visible_to_exit_gate_until_response_queued` moved to
// `client_turnloop/tests.rs`: it now drives the turnloop transport's state
// machine, whose internals are private to that module.

#[test]
fn parse_options_safe_defaults() {
    // Null pointer / undefined value → safe defaults from
    // url_from_options + headers_from_options + timeout_from_options.
    let null_val = f64::from_bits(TAG_UNDEFINED);
    let parsed = unsafe { parse_options_object(null_val) };
    assert!(parsed.is_none());

    let synth = serde_json::Value::Null;
    assert_eq!(url_from_options(&synth, "http"), "http://localhost/");
    assert!(headers_from_options(&synth).is_empty());
    assert!(timeout_from_options(&synth).is_none());
    assert_eq!(method_from_options(&synth), "GET");
}

#[test]
fn url_from_options_with_port_and_path() {
    let v: serde_json::Value =
        serde_json::from_str(r#"{"hostname":"api.example.com","port":8080,"path":"/v1/resource"}"#)
            .unwrap();
    assert_eq!(
        url_from_options(&v, "https"),
        "https://api.example.com:8080/v1/resource"
    );
}

#[test]
fn tls_servername_uses_logical_host_but_not_ip_literals() {
    assert_eq!(
        tls_servername_from_host_header("agent1:443").as_deref(),
        Some("agent1")
    );
    assert_eq!(tls_servername_from_host_header("127.0.0.1:443"), None);
    assert_eq!(tls_servername_from_host_header("[::1]:443"), None);
}

#[test]
fn headers_from_options_extracts() {
    let v: serde_json::Value =
        serde_json::from_str(r#"{"headers":{"X-Foo":"bar","Authorization":"Bearer x"}}"#).unwrap();
    let h = headers_from_options(&v);
    assert_eq!(h.get("X-Foo"), Some(&"bar".to_string()));
    assert_eq!(h.get("Authorization"), Some(&"Bearer x".to_string()));
}

#[test]
fn headers_from_options_normalizes_arrays_and_auth() {
    let object: serde_json::Value = serde_json::from_str(
        r#"{"auth":"foo:bar","headers":{"x-foo":"boom","cookie":["a=1","b=2","c=3"]}}"#,
    )
    .unwrap();
    let object_headers = headers_from_options(&object);
    assert_eq!(object_headers.get("x-foo"), Some(&"boom".to_string()));
    assert_eq!(
        object_headers.get("cookie"),
        Some(&"a=1; b=2; c=3".to_string())
    );
    assert_eq!(
        object_headers.get("Authorization"),
        Some(&"Basic Zm9vOmJhcg==".to_string())
    );

    let raw: serde_json::Value = serde_json::from_str(
        r#"{"auth":"foo:bar","headers":[["x-foo","boom"],["cookie","a=1"],["cookie",["b=2","c=3"]],["Host","example.com"]]}"#,
    )
    .unwrap();
    let raw_headers = headers_from_options(&raw);
    assert_eq!(raw_headers.get("x-foo"), Some(&"boom".to_string()));
    assert_eq!(
        raw_headers.get("cookie"),
        Some(&"a=1; b=2; c=3".to_string())
    );
    assert_eq!(raw_headers.get("Host"), Some(&"example.com".to_string()));
    assert!(!raw_headers
        .keys()
        .any(|name| name.eq_ignore_ascii_case("authorization")));
}
