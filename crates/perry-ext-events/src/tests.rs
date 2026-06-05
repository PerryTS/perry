use super::*;
use perry_ffi::alloc_string;
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

fn young_promise_root() -> *mut Promise {
    let ptr = perry_runtime::arena::arena_alloc_gc(
        std::mem::size_of::<perry_runtime::Promise>(),
        std::mem::align_of::<perry_runtime::Promise>(),
        perry_runtime::gc::GC_TYPE_PROMISE,
    );
    unsafe {
        std::ptr::write_bytes(ptr, 0, std::mem::size_of::<perry_runtime::Promise>());
    }
    ptr as *mut Promise
}

/// The listener-registration FFIs take raw NaN-box bits and validate that the
/// value is a closure. Allocate a real closure and return its NaN-boxed bits
/// as i64; the sentinel function pointer is never invoked in these tests.
extern "C" fn noop_listener(_c: *const RawClosureHeader) -> f64 {
    f64::from_bits(TAG_UNDEFINED_F64_BITS)
}

fn fake_listener() -> i64 {
    let closure = unsafe { js_closure_alloc(noop_listener as *const u8, 0) };
    nanbox_pointer_bits(closure as i64).to_bits() as i64
}

fn string_event_value(name: &JsString) -> f64 {
    f64::from_bits(nanbox_string_bits(name.as_raw()))
}

fn assert_rewritten(before: usize, after: usize) {
    assert_ne!(after, before);
    assert!(perry_runtime::arena::pointer_in_nursery(after));
}

#[test]
fn new_emitter_starts_empty() {
    let h = js_event_emitter_new();
    let event_name = alloc_string("foo");
    let event_bits = event_bits_from_string_ptr(event_name.as_raw() as *const _);
    let count =
        unsafe { js_event_emitter_listener_count(h, event_bits, TAG_UNDEFINED_F64_BITS as i64) };
    assert_eq!(count, 0.0);
    drop_event_emitter_handle(h);
}

#[test]
fn add_then_count_listeners() {
    let h = js_event_emitter_new();
    let event_name = alloc_string("change");
    let event_bits = event_bits_from_string_ptr(event_name.as_raw() as *const _);
    let _ = unsafe { js_event_emitter_on(h, event_bits, fake_listener()) };
    let _ = unsafe { js_event_emitter_on(h, event_bits, fake_listener()) };
    let count =
        unsafe { js_event_emitter_listener_count(h, event_bits, TAG_UNDEFINED_F64_BITS as i64) };
    assert_eq!(count, 2.0);
    drop_event_emitter_handle(h);
}

#[test]
fn remove_listener_drops_one() {
    let h = js_event_emitter_new();
    let event_name = alloc_string("data");
    let event_bits = event_bits_from_string_ptr(event_name.as_raw() as *const _);
    let a = fake_listener();
    let b = fake_listener();
    unsafe {
        js_event_emitter_on(h, event_bits, a);
        js_event_emitter_on(h, event_bits, b);
        js_event_emitter_remove_listener(h, event_bits, a);
    }
    let count =
        unsafe { js_event_emitter_listener_count(h, event_bits, TAG_UNDEFINED_F64_BITS as i64) };
    assert_eq!(count, 1.0);
    drop_event_emitter_handle(h);
}

#[test]
fn remove_all_clears() {
    let h = js_event_emitter_new();
    let event_name = alloc_string("x");
    let event_bits = event_bits_from_string_ptr(event_name.as_raw() as *const _);
    unsafe {
        js_event_emitter_on(h, event_bits, fake_listener());
        js_event_emitter_on(h, event_bits, fake_listener());
        js_event_emitter_remove_all_listeners(h, std::ptr::null());
    }
    let count =
        unsafe { js_event_emitter_listener_count(h, event_bits, TAG_UNDEFINED_F64_BITS as i64) };
    assert_eq!(count, 0.0);
    drop_event_emitter_handle(h);
}

#[test]
fn prepend_listener_inserts_at_front() {
    let h = js_event_emitter_new();
    let event_name = alloc_string("ord");
    let event_bits = event_bits_from_string_ptr(event_name.as_raw() as *const _);
    unsafe {
        js_event_emitter_on(h, event_bits, fake_listener());
        js_event_emitter_prepend_listener(h, event_bits, fake_listener());
    }
    let arr = unsafe { js_event_emitter_listeners(h, event_bits) };
    assert!(!arr.is_null());
    drop_event_emitter_handle(h);
}

#[test]
fn max_listeners_round_trips() {
    let h = js_event_emitter_new();
    assert_eq!(unsafe { js_event_emitter_get_max_listeners(h) }, 10.0);
    unsafe {
        js_event_emitter_set_max_listeners(h, 42.0);
    }
    assert_eq!(unsafe { js_event_emitter_get_max_listeners(h) }, 42.0);
    drop_event_emitter_handle(h);
}

#[test]
fn gc_mutable_scanner_rewrites_listener_and_pending_promise_roots() {
    let _guard = GcTestGuard::new();
    ensure_gc_scanner_registered();

    let listener = young_gc_root();
    let promise = young_promise_root();
    let mut events = HashMap::new();
    events.insert(
        "ready".to_string(),
        vec![Listener {
            callback: listener,
            raw_wrapper: 0,
            once: false,
        }],
    );
    let mut pending_once_promises = HashMap::new();
    pending_once_promises.insert(
        "ready".to_string(),
        vec![PendingOnce {
            promise,
            signal: undefined_value(),
            abort_listener: 0,
        }],
    );
    let handle = register_event_emitter_handle(EventEmitterHandle {
        events,
        event_order: vec!["ready".to_string()],
        pending_once_promises,
        max_listeners: 10,
        capture_rejections: false,
        domain_handle: None,
    });

    let _ = perry_runtime::gc::gc_collect_minor();

    {
        let emitter = get_event_emitter_mut(handle).expect("emitter handle should remain live");
        assert_rewritten(
            listener as usize,
            emitter.events["ready"][0].callback as usize,
        );
        assert_rewritten(
            promise as usize,
            emitter.pending_once_promises["ready"][0].promise as usize,
        );
    }
    drop_event_emitter_handle(handle);
}

#[test]
fn static_once_on_runtime_stream_attaches_and_cleans_error_pair() {
    let stream = perry_runtime::node_stream::js_node_stream_readable_new(undefined_value());
    let handle = handle_from_value(stream);
    let data = alloc_string("data");
    let error = alloc_string("error");
    let data_value = string_event_value(&data);
    let error_value = string_event_value(&error);

    let promise = unsafe { js_events_once(stream, data.as_raw(), undefined_value()) };
    assert!(!promise.is_null());
    assert_eq!(
        perry_runtime::node_stream::js_node_stream_method_listener_count(handle, data_value),
        1.0
    );
    assert_eq!(
        perry_runtime::node_stream::js_node_stream_method_listener_count(handle, error_value),
        1.0
    );

    let chunk = alloc_string("chunk");
    perry_runtime::node_stream::js_node_stream_method_emit(
        handle,
        data_value,
        string_event_value(&chunk),
    );

    assert_eq!(
        perry_runtime::promise::js_promise_state(promise as *mut perry_runtime::Promise),
        1
    );
    assert_eq!(
        perry_runtime::node_stream::js_node_stream_method_listener_count(handle, data_value),
        0.0
    );
    assert_eq!(
        perry_runtime::node_stream::js_node_stream_method_listener_count(handle, error_value),
        0.0
    );
}
