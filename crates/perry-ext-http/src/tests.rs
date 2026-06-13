use super::*;
use perry_ffi::{drop_handle, get_handle, register_handle};
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

static GC_TEST_LOCK: Mutex<()> = Mutex::new(());

struct GcTestGuard {
    frame: u64,
    _lock: MutexGuard<'static, ()>,
}

impl GcTestGuard {
    fn new() -> Self {
        let lock = GC_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        perry_runtime::gc::js_gc_write_barriers_emitted(1);
        let frame = perry_runtime::gc::js_shadow_frame_push(0);
        Self { frame, _lock: lock }
    }
}

impl Drop for GcTestGuard {
    fn drop(&mut self) {
        perry_runtime::gc::js_shadow_frame_pop(self.frame);
        perry_runtime::gc::js_gc_write_barriers_emitted(0);
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
    let request_listener = young_gc_root();
    let incoming_listener = young_gc_root();
    let mut request_listeners = HashMap::new();
    request_listeners.insert("error".to_string(), vec![request_listener]);
    let request_handle = register_handle(ClientRequestHandle {
        method: "GET".to_string(),
        url: "http://localhost/".to_string(),
        headers: HashMap::new(),
        body: Vec::new(),
        response_callback,
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
        tls: crate::tls_client::TlsOptions::default(),
        incoming_handle: 0,
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
    });

    let _ = perry_runtime::gc::gc_collect_minor();

    {
        let req = get_handle::<ClientRequestHandle>(request_handle)
            .expect("request handle should remain live");
        assert_rewritten(response_callback, req.response_callback);
        assert_rewritten(request_listener, req.listeners["error"][0]);
        let msg = get_handle::<IncomingMessageHandle>(incoming_handle)
            .expect("incoming message handle should remain live");
        assert_rewritten(incoming_listener, msg.listeners["data"][0]);
    }
    drop_handle(request_handle);
    drop_handle(incoming_handle);
}

#[test]
fn has_pending_zero_when_idle() {
    // Drain anything other tests left; then assert zero.
    let _ = HTTP_PENDING_EVENTS.lock().map(|mut q| q.clear());
    assert_eq!(js_http_has_pending(), 0);
}

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
fn headers_from_options_extracts() {
    let v: serde_json::Value =
        serde_json::from_str(r#"{"headers":{"X-Foo":"bar","Authorization":"Bearer x"}}"#).unwrap();
    let h = headers_from_options(&v);
    assert_eq!(h.get("X-Foo"), Some(&"bar".to_string()));
    assert_eq!(h.get("Authorization"), Some(&"Bearer x".to_string()));
}
