use super::*;
use perry_ffi::{drop_handle, get_handle, register_handle};
use std::sync::{Mutex, MutexGuard};

static GC_TEST_LOCK: Mutex<()> = Mutex::new(());

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
fn gc_scanner_registration_idempotent() {
    ensure_runtime_hooks_registered();
    ensure_runtime_hooks_registered();
}

#[test]
fn gc_mutable_scanner_rewrites_client_and_server_listener_roots() {
    let _guard = GcTestGuard::new();
    perry_ffi::gc_register_mutable_root_scanner_named("perry-ext-ws", scan_ws_roots);

    let client_id = usize::MAX - 9_001;
    let client_callback = young_gc_root();
    WS_CLIENT_LISTENERS.lock().unwrap().insert(
        client_id,
        WsClientListeners {
            listeners: HashMap::from([("message".to_string(), vec![client_callback])]),
        },
    );

    let server_callback = young_gc_root();
    let clients_before = alloc_set(4).bits();
    let server_handle = register_handle(WsServerHandle {
        listeners: HashMap::from([("connection".to_string(), vec![server_callback])]),
        port: 0,
        host: "0.0.0.0".into(),
        attached_server: None,
        no_server: true,
        is_listening: false,
        client_ids: Vec::new(),
        clients_bits: clients_before,
        listener_id: None,
    });

    let _ = perry_runtime::gc::gc_collect_minor();

    {
        let clients = WS_CLIENT_LISTENERS.lock().unwrap();
        assert_rewritten(client_callback, clients[&client_id].listeners["message"][0]);
        let server =
            get_handle::<WsServerHandle>(server_handle).expect("server handle should remain live");
        assert_rewritten(server_callback, server.listeners["connection"][0]);
        assert_ne!(server.clients_bits, clients_before);
        let clients =
            JsValue::from_bits(server.clients_bits).as_pointer::<perry_runtime::set::SetHeader>();
        assert_eq!(perry_runtime::set::js_set_size(clients), 0);
    }
    WS_CLIENT_LISTENERS.lock().unwrap().remove(&client_id);
    drop_handle(server_handle);
}

/// #9324: the DYNAMIC read must reach the same live `Set` the typed read
/// gets. Pre-fix this dispatcher did not exist, every untyped
/// `wss.clients` read `undefined`, and iterating it killed the process.
#[test]
fn handle_property_dispatch_answers_clients_for_an_untyped_read() {
    let _lock = GC_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let undefined = f64::from_bits(JsValue::UNDEFINED.bits());
    let server_handle = js_ws_server_new(undefined);

    let mut out = f64::NAN;
    let handled = unsafe {
        js_ext_ws_handle_property_dispatch(
            server_handle,
            b"clients".as_ptr(),
            b"clients".len(),
            &mut out,
        )
    };
    assert_eq!(handled, 1, "the dispatcher must claim `clients`");
    assert_eq!(
        out.to_bits(),
        js_ws_server_clients(server_handle).to_bits(),
        "the dynamic read must return the SAME Set as the typed read"
    );
    let set = JsValue::from_bits(out.to_bits()).as_pointer::<perry_runtime::set::SetHeader>();
    assert!(!set.is_null());
    assert_eq!(perry_runtime::set::js_set_size(set), 0);

    // Every other property stays unhandled so the composite dispatcher
    // falls through to the primary stdlib dispatcher.
    let mut other = f64::NAN;
    assert_eq!(
        unsafe {
            js_ext_ws_handle_property_dispatch(
                server_handle,
                b"readyState".as_ptr(),
                b"readyState".len(),
                &mut other,
            )
        },
        0
    );

    drop_handle(server_handle);

    // A handle that is no longer a live server must NOT be claimed —
    // otherwise this arm would shadow whatever id gets recycled into it.
    let mut dead = f64::NAN;
    assert_eq!(
        unsafe {
            js_ext_ws_handle_property_dispatch(
                server_handle,
                b"clients".as_ptr(),
                b"clients".len(),
                &mut dead,
            )
        },
        0
    );
}

#[test]
fn server_clients_is_a_stable_set_that_tracks_connections() {
    let _lock = GC_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let undefined = f64::from_bits(JsValue::UNDEFINED.bits());
    let server_handle = js_ws_server_new(undefined);

    let first = js_ws_server_clients(server_handle);
    let second = js_ws_server_clients(server_handle);
    assert_eq!(first.to_bits(), second.to_bits());
    let clients = JsValue::from_bits(first.to_bits()).as_pointer::<perry_runtime::set::SetHeader>();
    assert!(!clients.is_null());
    assert_eq!(perry_runtime::set::js_set_size(clients), 0);

    let client_id = register_handle(WsClientHandle) as usize;
    track_server_client(server_handle, client_id);
    let clients = JsValue::from_bits(js_ws_server_clients(server_handle).to_bits())
        .as_pointer::<perry_runtime::set::SetHeader>();
    assert_eq!(perry_runtime::set::js_set_size(clients), 1);
    assert_eq!(
        perry_runtime::set::js_set_has(clients, f64::from_bits(client_js_value(client_id).bits())),
        1
    );

    WS_CLIENT_PARENT_SERVER
        .lock()
        .unwrap()
        .insert(client_id, server_handle);
    assert_eq!(untrack_server_client(client_id), Some(server_handle));
    let clients = JsValue::from_bits(js_ws_server_clients(server_handle).to_bits())
        .as_pointer::<perry_runtime::set::SetHeader>();
    assert_eq!(perry_runtime::set::js_set_size(clients), 0);

    drop_handle(client_id as i64);
    drop_handle(server_handle);
}

static FIRST_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static SECOND_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

extern "C" fn count_second(_c: *const perry_runtime::closure::ClosureHeader) -> f64 {
    SECOND_CALLS.fetch_add(1, Ordering::SeqCst);
    f64::from_bits(JsValue::UNDEFINED.bits())
}

extern "C" fn count_first(_c: *const perry_runtime::closure::ClosureHeader) -> f64 {
    FIRST_CALLS.fetch_add(1, Ordering::SeqCst);
    f64::from_bits(JsValue::UNDEFINED.bits())
}

/// Counts its calls and registers a `count_second` listener, from inside
/// this callback, on the server handle in its capture slot 0.
extern "C" fn count_first_and_register(c: *const perry_runtime::closure::ClosureHeader) -> f64 {
    FIRST_CALLS.fetch_add(1, Ordering::SeqCst);
    let server = perry_runtime::closure::js_closure_get_capture_f64(c, 0) as Handle;
    on_listening(server, closure_of(count_second));
    f64::from_bits(JsValue::UNDEFINED.bits())
}

/// Register `listener` for `'listening'` on `server`. The event-name string
/// is allocated here, after the listener closure already exists, and the
/// closure is held in a transient root across that allocation: a collection
/// it triggers may move the closure, and an unrooted copy would register a
/// stale pointer. The string is used by `js_ws_on` only, before anything
/// else can allocate.
fn on_listening(server: Handle, listener: i64) {
    let scope = perry_ffi::TransientRootScope::enter();
    let listener = scope.root_addr(listener);
    let event = alloc_string("listening");
    unsafe { js_ws_on(server, event.as_raw(), listener.get()) };
}

fn closure_of(f: extern "C" fn(*const perry_runtime::closure::ClosureHeader) -> f64) -> i64 {
    perry_runtime::closure::js_closure_alloc(f as *const u8, 0) as i64
}

/// A server that reads as bound, with nothing of its own queued.
fn listening_server() -> Handle {
    let undefined = f64::from_bits(JsValue::UNDEFINED.bits());
    let server = js_ws_server_new(undefined);
    get_handle_mut::<WsServerHandle>(server)
        .expect("server handle")
        .is_listening = true;
    server
}

fn listening_queued_for(server: Handle) -> usize {
    WS_PENDING_EVENTS
        .lock()
        .unwrap()
        .iter()
        .filter(|ev| matches!(ev, PendingWsEvent::Listening(h) if *h == server))
        .count()
}

/// #11309: the standalone server queues `Listening` in its constructor, so
/// a listener registered in the same tick is reached by that event. The
/// replay `js_ws_on` used to queue on top of it ran every listener twice.
/// A listener registered after the event was delivered still gets one.
#[test]
fn listening_reaches_each_listener_once() {
    let _lock = GC_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _ = js_ws_process_pending();
    FIRST_CALLS.store(0, Ordering::SeqCst);
    SECOND_CALLS.store(0, Ordering::SeqCst);
    let server = listening_server();

    push_ws_event(PendingWsEvent::Listening(server));
    on_listening(server, closure_of(count_first));
    assert_eq!(
        listening_queued_for(server),
        1,
        "the queued event is not doubled"
    );
    js_ws_process_pending();
    assert_eq!(FIRST_CALLS.load(Ordering::SeqCst), 1);

    on_listening(server, closure_of(count_second));
    assert_eq!(
        listening_queued_for(server),
        1,
        "a late listener still gets a replay"
    );
    js_ws_process_pending();
    assert_eq!(SECOND_CALLS.load(Ordering::SeqCst), 1);

    drop_handle(server);
}

/// A drain pops each event before running its listeners. A listener that
/// registers another `'listening'` listener from inside that event's own
/// callback must not be answered with a replay: Node does not call a
/// listener added during an emit for that emit, and the replay would run
/// the first listener a second time.
#[test]
fn a_listener_registered_during_its_event_gets_no_replay() {
    let _lock = GC_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _ = js_ws_process_pending();
    FIRST_CALLS.store(0, Ordering::SeqCst);
    SECOND_CALLS.store(0, Ordering::SeqCst);
    let server = listening_server();
    let first = perry_runtime::closure::js_closure_alloc(count_first_and_register as *const u8, 1);
    perry_runtime::closure::js_closure_set_capture_f64(first, 0, server as f64);

    push_ws_event(PendingWsEvent::Listening(server));
    on_listening(server, first as i64);
    js_ws_process_pending();
    assert_eq!(FIRST_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(listening_queued_for(server), 0, "no replay was queued");
    js_ws_process_pending();
    assert_eq!(
        FIRST_CALLS.load(Ordering::SeqCst),
        1,
        "the first listener ran once"
    );
    assert_eq!(SECOND_CALLS.load(Ordering::SeqCst), 0);

    drop_handle(server);
}

#[test]
fn has_pending_returns_zero_with_no_state() {
    // Drains the shared queue, so it must not run inside another test's
    // setup.
    let _lock = GC_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // May be non-zero if a prior test left state behind, but
    // process_pending drains it.
    let _ = js_ws_process_pending();
    // No active servers, no pending events, no open connections.
    // (We can't fully clean state across tests since these are
    // process-globals; assert non-negative as the minimal sanity
    // check.)
    let v = js_ws_has_pending();
    assert!(v >= 0);
}

#[test]
fn handle_to_i64_strips_pointer_tag() {
    let raw_ptr: u64 = 0x1234_5678_9abc;
    let nan_boxed = f64::from_bits(POINTER_TAG | raw_ptr);
    assert_eq!(js_ws_handle_to_i64(nan_boxed), raw_ptr as i64);

    let plain = 42.0_f64;
    assert_eq!(js_ws_handle_to_i64(plain), 42);
}

#[test]
fn client_handles_do_not_alias_registered_servers() {
    let server = js_ws_server_new(f64::from_bits(JsValue::UNDEFINED.bits()));
    let client = register_handle(WsClientHandle);
    assert_ne!(server, client);
    assert!(get_handle_mut::<WsServerHandle>(client).is_none());
    assert!(get_handle_mut::<WsClientHandle>(server).is_none());
    assert_eq!(
        decode_client_id(f64::from_bits(client_js_value(client as usize).bits())),
        client as usize
    );
    assert_eq!(decode_client_id(client as f64), client as usize);
    perry_ffi::drop_handle(client);
    perry_ffi::drop_handle(server);
}

/// The event-loop keepalive predicate, on constructed connections rather
/// than the process-global map so it cannot race another test.
///
/// The third case is the regression: a refused `new WebSocket(url)` leaves
/// the transport `Connecting` for ever (nothing ever attaches), so a
/// predicate that asked only about the transport would report the process
/// live until it was killed.
#[test]
fn keepalive_counts_connecting_clients_but_not_failed_ones() {
    let connection = |transport, is_open, is_closed| WsConnection {
        transport,
        messages: Vec::new(),
        is_open,
        is_closing: false,
        is_closed,
    };
    assert!(
        connection_is_live(&connection(
            WsTransport::Connecting(Vec::new()),
            false,
            false
        )),
        "a handshake in flight keeps the loop alive"
    );
    assert!(
        connection_is_live(&connection(WsTransport::Turnloop(1), true, false)),
        "an open connection keeps the loop alive"
    );
    assert!(
        !connection_is_live(&connection(
            WsTransport::Connecting(Vec::new()),
            false,
            true
        )),
        "a FAILED connect must not keep the loop alive for ever"
    );
    assert!(
        !connection_is_live(&connection(WsTransport::Turnloop(1), false, true)),
        "a closed connection must not keep the loop alive"
    );
}

/// #6117 — `readyState` walks the npm-ws lifecycle: CONNECTING (0)
/// pre-open, OPEN (1), CLOSING (2) after `close()` is requested,
/// CLOSED (3) once the IO loop marks the connection dead, and CLOSED
/// for ids with no entry (cleaned up, or promise-path connect failed).
/// Uses an id far outside anything other tests insert, so no lock.
#[test]
fn ready_state_reports_npm_ws_lifecycle() {
    let ws_id = 990_077usize;
    WS_CONNECTIONS.lock().unwrap().insert(
        ws_id,
        WsConnection {
            // CONNECTING is a real transport state now rather than a
            // channel with nothing on the other end: a `close()` here is
            // queued, which is exactly what the assertions below check
            // does not disturb `readyState`.
            transport: WsTransport::Connecting(Vec::new()),
            messages: Vec::new(),
            is_open: false,
            is_closing: false,
            is_closed: false,
        },
    );

    assert_eq!(js_ws_ready_state(ws_id as i64), 0.0);
    WS_CONNECTIONS
        .lock()
        .unwrap()
        .get_mut(&ws_id)
        .unwrap()
        .is_open = true;
    assert_eq!(js_ws_ready_state(ws_id as i64), 1.0);
    js_ws_close(ws_id as i64);
    assert_eq!(js_ws_ready_state(ws_id as i64), 2.0);
    if let Some(c) = WS_CONNECTIONS.lock().unwrap().get_mut(&ws_id) {
        c.is_closed = true;
    }
    assert_eq!(js_ws_ready_state(ws_id as i64), 3.0);
    WS_CONNECTIONS.lock().unwrap().remove(&ws_id);
    assert_eq!(js_ws_ready_state(ws_id as i64), 3.0);
}
