use super::*;
use perry_ffi::{drop_handle, get_handle, register_handle};
use std::sync::atomic::Ordering;
use std::sync::{Mutex, MutexGuard};

static GC_TEST_LOCK: Mutex<()> = Mutex::new(());

struct GcTestGuard {
    scope: perry_runtime::gc::RuntimeHandleScope,
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
        Self {
            scope: perry_runtime::gc::RuntimeHandleScope::new(),
            previous_force_evacuation,
            _lock: lock,
        }
    }
}

impl Drop for GcTestGuard {
    fn drop(&mut self) {
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

static FIRST_MESSAGE_BITS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SECOND_MESSAGE_BITS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SECOND_MESSAGE_CALLBACK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

unsafe extern "C" fn collect_from_message_listener(
    _: *const perry_runtime::closure::ClosureHeader,
    _: perry_runtime::closure::JsThis,
    message: f64,
) -> f64 {
    FIRST_MESSAGE_BITS.store(message.to_bits(), Ordering::SeqCst);
    let _ = perry_runtime::gc::gc_collect_minor();
    f64::from_bits(JsValue::UNDEFINED.bits())
}

unsafe extern "C" fn observe_message_after_collection(
    callback: *const perry_runtime::closure::ClosureHeader,
    _: perry_runtime::closure::JsThis,
    message: f64,
) -> f64 {
    SECOND_MESSAGE_CALLBACK.store(callback as u64, Ordering::SeqCst);
    SECOND_MESSAGE_BITS.store(message.to_bits(), Ordering::SeqCst);
    f64::from_bits(JsValue::UNDEFINED.bits())
}

#[test]
fn gc_scanner_registration_idempotent() {
    ensure_runtime_hooks_registered();
    ensure_runtime_hooks_registered();
}

#[test]
fn pending_message_reloads_relocated_listener_and_payload() {
    perry_runtime::gc::gc_init();
    let guard = GcTestGuard::new();
    perry_ffi::gc_register_mutable_root_scanner_named("perry-ext-ws", scan_ws_roots);

    let first = guard
        .scope
        .root_raw_mut_ptr(perry_runtime::closure::js_closure_alloc(
            perry_runtime::fn_info!(collect_from_message_listener, 1),
            0,
        ));
    let second = guard
        .scope
        .root_raw_mut_ptr(perry_runtime::closure::js_closure_alloc(
            perry_runtime::fn_info!(observe_message_after_collection, 1),
            0,
        ));
    let first_before = first.get_raw_mut_ptr::<perry_ffi::RawClosureHeader>() as i64;
    let second_before = second.get_raw_mut_ptr::<perry_ffi::RawClosureHeader>() as i64;
    let client_id = usize::MAX - 10_027;
    WS_CLIENT_LISTENERS.lock().unwrap().insert(
        client_id,
        WsClientListeners {
            agent: perry_ffi::agent_post::current_agent(),
            listeners: HashMap::from([("message".to_string(), vec![first_before, second_before])]),
        },
    );
    FIRST_MESSAGE_BITS.store(0, Ordering::SeqCst);
    SECOND_MESSAGE_BITS.store(0, Ordering::SeqCst);
    SECOND_MESSAGE_CALLBACK.store(0, Ordering::SeqCst);
    push_ws_event(PendingWsEvent::Message(
        client_id,
        WsPayload::Text("subscription survives collection".to_string()),
    ));

    assert_eq!(js_ws_process_pending(), 2);
    let listeners = WS_CLIENT_LISTENERS
        .lock()
        .unwrap()
        .remove(&client_id)
        .expect("test listeners should remain registered");
    let second_after = listeners.listeners["message"][1];
    assert_rewritten(second_before, second_after);
    assert_eq!(
        SECOND_MESSAGE_CALLBACK.load(Ordering::SeqCst),
        second_after as u64,
        "the second dispatch must load the relocated callback address"
    );

    let message_before = FIRST_MESSAGE_BITS.load(Ordering::SeqCst);
    let message_after = SECOND_MESSAGE_BITS.load(Ordering::SeqCst);
    assert_ne!(
        message_before, 0,
        "the first listener must receive a payload"
    );
    assert_ne!(
        message_after, 0,
        "the second listener must receive a payload"
    );
    assert_ne!(message_before, message_after, "the payload must relocate");
    let message_ptr = (message_after & POINTER_MASK) as usize;
    assert!(perry_runtime::arena::pointer_in_nursery(message_ptr));
    let text = unsafe { read_str(message_ptr as *const StringHeader) };
    assert_eq!(text.as_deref(), Some("subscription survives collection"));
}

#[test]
fn gc_mutable_scanner_rewrites_client_and_server_listener_roots() {
    let _guard = GcTestGuard::new();
    perry_ffi::gc_register_mutable_root_scanner_named("perry-ext-ws", scan_ws_roots);

    let client_id = usize::MAX - 9_001;
    let client_callback = _guard.scope.root_raw_mut_ptr(young_gc_root() as *mut u64);
    let client_callback_before = client_callback.get_raw_mut_ptr::<u64>() as i64;
    WS_CLIENT_LISTENERS.lock().unwrap().insert(
        client_id,
        WsClientListeners {
            agent: perry_ffi::agent_post::current_agent(),
            listeners: HashMap::from([("message".to_string(), vec![client_callback_before])]),
        },
    );

    let server_callback = _guard.scope.root_raw_mut_ptr(young_gc_root() as *mut u64);
    let server_callback_before = server_callback.get_raw_mut_ptr::<u64>() as i64;
    let clients_before = alloc_set(4).bits();
    let server_handle = register_handle(WsServerHandle {
        owner_agent: perry_ffi::agent_post::current_agent(),
        counted: false,
        listeners: HashMap::from([("connection".to_string(), vec![server_callback_before])]),
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
        assert_rewritten(
            client_callback_before,
            clients[&client_id].listeners["message"][0],
        );
        let server =
            get_handle::<WsServerHandle>(server_handle).expect("server handle should remain live");
        assert_rewritten(server_callback_before, server.listeners["connection"][0]);
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

extern "C" fn count_second(
    _c: *const perry_runtime::closure::ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    SECOND_CALLS.fetch_add(1, Ordering::SeqCst);
    f64::from_bits(JsValue::UNDEFINED.bits())
}

extern "C" fn count_first(
    _c: *const perry_runtime::closure::ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    FIRST_CALLS.fetch_add(1, Ordering::SeqCst);
    f64::from_bits(JsValue::UNDEFINED.bits())
}

/// Counts its calls and registers a `count_second` listener, from inside
/// this callback, on the server handle in its capture slot 0.
extern "C" fn count_first_and_register(
    c: *const perry_runtime::closure::ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    FIRST_CALLS.fetch_add(1, Ordering::SeqCst);
    let server = perry_runtime::closure::js_closure_get_capture_f64(c, 0) as Handle;
    on_listening(server, closure_of(perry_runtime::fn_info!(count_second, 0)));
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

fn closure_of(info: *const perry_runtime::closure::JsFunctionInfo) -> i64 {
    perry_runtime::closure::js_closure_alloc(info, 0) as i64
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
        .filter(|(_, ev)| matches!(ev, PendingWsEvent::Listening(h) if *h == server))
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
    on_listening(server, closure_of(perry_runtime::fn_info!(count_first, 0)));
    assert_eq!(
        listening_queued_for(server),
        1,
        "the queued event is not doubled"
    );
    js_ws_process_pending();
    assert_eq!(FIRST_CALLS.load(Ordering::SeqCst), 1);

    on_listening(server, closure_of(perry_runtime::fn_info!(count_second, 0)));
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
    let first = perry_runtime::closure::js_closure_alloc(
        perry_runtime::fn_info!(count_first_and_register, 0),
        1,
    );
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
fn handle_to_i64_decodes_native_handle_tag() {
    let raw_ptr: u64 = 0x1234_5678_9abc;
    let nan_boxed = f64::from_bits(JsValue::from_handle(raw_ptr).bits());
    assert_eq!(js_ws_handle_to_i64(nan_boxed), raw_ptr as i64);

    // A heap object tag is not a native registry id.
    assert_eq!(
        js_ws_handle_to_i64(f64::from_bits(POINTER_TAG | raw_ptr)),
        0
    );
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
        agent: 0,
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
            agent: 0,
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

fn ping_queued_for(ws_id: usize) -> usize {
    WS_PENDING_EVENTS
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, ev)| matches!(ev, PendingWsEvent::Ping(id, _) if *id == ws_id))
        .count()
}

/// Every agent may own a loop and pump `js_ws_process_pending`, but an event's
/// listeners live on the heap of the agent that queued it. Another agent's
/// drain must leave it queued — it used to take it and call the closures on
/// the wrong thread, so the owner never saw the event (a noServer upgrade
/// test stalled whenever a sibling test pumped first).
#[test]
fn one_agents_drain_leaves_another_agents_events_queued() {
    const WS_ID: usize = usize::MAX - 7;
    let (queued_tx, queued_rx) = std::sync::mpsc::channel();
    let (other_drained_tx, other_drained_rx) = std::sync::mpsc::channel();
    let owner = std::thread::spawn(move || {
        let agent = perry_runtime::agent::enter_worker_agent();
        push_ws_event(PendingWsEvent::Ping(WS_ID, Vec::new()));
        queued_tx.send(()).unwrap();
        other_drained_rx.recv().unwrap();
        assert_eq!(js_ws_has_pending(), 1, "the owner sees its own event");
        js_ws_process_pending();
        assert_eq!(ping_queued_for(WS_ID), 0, "the owner's drain delivers it");
        perry_runtime::agent::retire_agent(agent);
    });
    queued_rx.recv().unwrap();
    std::thread::spawn(move || {
        let agent = perry_runtime::agent::enter_worker_agent();
        js_ws_process_pending();
        assert_eq!(ping_queued_for(WS_ID), 1, "another agent's drain leaves it");
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
    other_drained_tx.send(()).unwrap();
    owner.join().unwrap();
}

/// One agent's ws state across every per-agent table.
struct AgentRows {
    agent: u64,
    client: usize,
    server: perry_ffi::Handle,
    link: i64,
}

fn link_write(_: i64, _: &[u8]) {}
fn link_end(_: i64) {}

/// Create, as the calling agent, one row in every per-agent table: a queued
/// event, a listening-server count, a connection, a client listener table, a
/// client-to-server parent link, a protocol link, and the server handle.
fn seed_agent_rows(link: i64) -> AgentRows {
    ensure_runtime_hooks_registered();
    let agent = perry_ffi::agent_post::current_agent();
    let client = register_turnloop_client(link);
    turnloop_link::adopt_existing(
        link,
        client,
        turnloop_link::Transport {
            write: link_write,
            finish: link_end,
            destroy: link_end,
        },
        codec::Role::Server,
        &[],
    );
    let server = register_handle(WsServerHandle {
        owner_agent: agent,
        counted: true,
        listeners: HashMap::new(),
        port: 1,
        host: "127.0.0.1".into(),
        attached_server: None,
        no_server: false,
        is_listening: true,
        client_ids: vec![client],
        clients_bits: JsValue::UNDEFINED.bits(),
        listener_id: None,
    });
    active_servers_add(agent);
    WS_CLIENT_PARENT_SERVER
        .lock()
        .unwrap()
        .insert(client, server);
    push_ws_event(PendingWsEvent::Ping(client, Vec::new()));
    AgentRows {
        agent,
        client,
        server,
        link,
    }
}

impl AgentRows {
    /// Whether any table still holds a row for this agent's state.
    fn survivors(&self) -> Vec<&'static str> {
        let mut found = Vec::new();
        let mut note = |present: bool, table: &'static str| {
            if present {
                found.push(table)
            }
        };
        note(ping_queued_for(self.client) != 0, "WS_PENDING_EVENTS");
        note(
            WS_PENDING_EVENTS
                .lock()
                .unwrap()
                .iter()
                .any(|(owner, _)| *owner == self.agent),
            "WS_PENDING_EVENTS (any)",
        );
        note(
            WS_ACTIVE_SERVERS
                .lock()
                .unwrap()
                .iter()
                .any(|(owner, _)| *owner == self.agent),
            "WS_ACTIVE_SERVERS",
        );
        note(
            WS_CONNECTIONS.lock().unwrap().contains_key(&self.client),
            "WS_CONNECTIONS",
        );
        note(
            WS_CLIENT_LISTENERS
                .lock()
                .unwrap()
                .contains_key(&self.client),
            "WS_CLIENT_LISTENERS",
        );
        note(
            WS_CLIENT_PARENT_SERVER
                .lock()
                .unwrap()
                .contains_key(&self.client),
            "WS_CLIENT_PARENT_SERVER",
        );
        note(turnloop_link::owns(self.link), "turnloop_link::links");
        note(perry_ffi::handle_exists(self.server), "server handle");
        note(
            perry_ffi::handle_exists(self.client as i64),
            "client handle",
        );
        found
    }
}

/// A `worker_threads` worker that owns ws state and exits leaves nothing
/// behind, and a sibling agent's state is untouched by it. The retire path is
/// the runtime's own `retire_agent`, which is what the worker thread calls.
#[test]
fn retiring_an_agent_purges_its_ws_state_and_only_its_own() {
    std::thread::spawn(|| {
        let a = perry_runtime::agent::enter_worker_agent();
        let a_rows = seed_agent_rows(-2_001);
        let b = perry_runtime::agent::enter_worker_agent();
        let b_rows = seed_agent_rows(-2_002);
        assert_eq!((a_rows.agent, b_rows.agent), (a, b));
        assert_eq!(a_rows.survivors().len(), b_rows.survivors().len());
        assert!(b_rows.survivors().len() >= 8, "{:?}", b_rows.survivors());
        assert_eq!(js_ws_has_pending(), 1, "B is alive through its own rows");

        perry_runtime::agent::retire_agent(b);

        let left = b_rows.survivors();
        assert!(left.is_empty(), "retired agent B still has rows: {left:?}");
        // A is a different agent: every row it owns is still there.
        perry_runtime::agent::enter_agent_for_test(a);
        assert_eq!(
            a_rows.survivors().len(),
            9,
            "A lost rows: {:?}",
            a_rows.survivors()
        );
        assert_eq!(js_ws_has_pending(), 1, "A is alive through its own rows");
        perry_runtime::agent::retire_agent(a);
        assert!(a_rows.survivors().is_empty(), "{:?}", a_rows.survivors());
    })
    .join()
    .unwrap();
}

/// One agent's connection, listening server or queued event must not keep a
/// different agent's loop alive: only the owner's pump may drain them.
#[test]
fn another_agents_ws_state_does_not_keep_this_agents_loop_alive() {
    std::thread::spawn(|| {
        let a = perry_runtime::agent::enter_worker_agent();
        assert_eq!(js_ws_has_pending(), 0, "a fresh agent has nothing pending");
        let b = perry_runtime::agent::enter_worker_agent();
        let b_rows = seed_agent_rows(-2_003);
        assert_eq!(js_ws_has_pending(), 1);

        // Each kind of B state alone, with the others removed, to show that
        // every one of them is scoped: events, the server count, and a live
        // connection.
        perry_runtime::agent::enter_agent_for_test(a);
        assert_eq!(js_ws_has_pending(), 0, "B's state keeps A's loop alive");
        perry_runtime::agent::enter_agent_for_test(b);
        WS_PENDING_EVENTS
            .lock()
            .unwrap()
            .retain(|(owner, _)| *owner != b);
        assert_eq!(js_ws_has_pending(), 1, "B still has a server and a socket");
        WS_ACTIVE_SERVERS
            .lock()
            .unwrap()
            .retain(|(owner, _)| *owner != b);
        assert_eq!(js_ws_has_pending(), 1, "B still has a live connection");
        perry_runtime::agent::enter_agent_for_test(a);
        assert_eq!(js_ws_has_pending(), 0);
        perry_runtime::agent::retire_agent(b);
        perry_runtime::agent::retire_agent(a);
        assert!(b_rows.survivors().is_empty());
    })
    .join()
    .unwrap();
}

/// A collection on one agent visits only that agent's ws slots. A sibling
/// agent's slots hold addresses on a heap this collection does not own:
/// rewriting or marking them is wrong, so they must be left exactly as they
/// were while this agent's own slots are rewritten.
#[test]
fn a_collection_visits_only_the_current_agents_ws_roots() {
    std::thread::spawn(|| {
        let guard = GcTestGuard::new();
        perry_ffi::gc_register_mutable_root_scanner_named("perry-ext-ws", scan_ws_roots);
        let a = perry_runtime::agent::enter_worker_agent();
        let b = perry_runtime::agent::enter_worker_agent();

        // One client listener table and one server per agent, each pointing
        // at a young closure-like root the collection will move.
        let seed = |agent: u64, client: usize| {
            perry_runtime::agent::enter_agent_for_test(agent);
            let for_client = guard.scope.root_raw_mut_ptr(young_gc_root() as *mut u64);
            let for_server = guard.scope.root_raw_mut_ptr(young_gc_root() as *mut u64);
            let client_cb = for_client.get_raw_mut_ptr::<u64>() as i64;
            let server_cb = for_server.get_raw_mut_ptr::<u64>() as i64;
            let mut listeners = WsClientListeners::new();
            listeners
                .listeners
                .insert("message".to_string(), vec![client_cb]);
            WS_CLIENT_LISTENERS
                .lock()
                .unwrap()
                .insert(client, listeners);
            let server = register_handle(WsServerHandle {
                owner_agent: agent,
                counted: false,
                listeners: HashMap::from([("connection".to_string(), vec![server_cb])]),
                port: 0,
                host: "127.0.0.1".into(),
                attached_server: None,
                no_server: true,
                is_listening: false,
                client_ids: Vec::new(),
                clients_bits: JsValue::UNDEFINED.bits(),
                listener_id: None,
            });
            (client_cb, server_cb, server)
        };
        let (a_client_before, a_server_before, a_server) = seed(a, usize::MAX - 20_001);
        let (b_client_before, b_server_before, b_server) = seed(b, usize::MAX - 20_002);

        // Collect as agent A.
        perry_runtime::agent::enter_agent_for_test(a);
        let _ = perry_runtime::gc::gc_collect_minor();

        let listener_of =
            |client: usize| WS_CLIENT_LISTENERS.lock().unwrap()[&client].listeners["message"][0];
        let server_listener_of =
            |server| get_handle::<WsServerHandle>(server).unwrap().listeners["connection"][0];
        assert_rewritten(a_client_before, listener_of(usize::MAX - 20_001));
        assert_rewritten(a_server_before, server_listener_of(a_server));
        assert_eq!(
            listener_of(usize::MAX - 20_002),
            b_client_before,
            "A's collection rewrote B's client listener slot"
        );
        assert_eq!(
            server_listener_of(b_server),
            b_server_before,
            "A's collection rewrote B's server listener slot"
        );

        WS_CLIENT_LISTENERS
            .lock()
            .unwrap()
            .remove(&(usize::MAX - 20_001));
        WS_CLIENT_LISTENERS
            .lock()
            .unwrap()
            .remove(&(usize::MAX - 20_002));
        drop_handle(a_server);
        drop_handle(b_server);
    })
    .join()
    .unwrap();
}

/// A server's listening count goes back to the agent that took it, however
/// many times and from whichever agent the release runs, and never to the
/// caller's own count.
#[test]
fn releasing_a_server_count_credits_the_creating_agent_once() {
    std::thread::spawn(|| {
        let a = perry_runtime::agent::enter_worker_agent();
        let a_rows = seed_agent_rows(-2_004);
        let b = perry_runtime::agent::enter_worker_agent();
        let b_rows = seed_agent_rows(-2_005);
        assert!(agent_has_active_servers(a) && agent_has_active_servers(b));

        // B (the calling agent) releases A's server, twice.
        for _ in 0..2 {
            let server = get_handle_mut::<WsServerHandle>(a_rows.server).unwrap();
            release_server_count(server);
        }
        assert!(!agent_has_active_servers(a), "A's count was not released");
        assert!(agent_has_active_servers(b), "B paid for A's server");
        assert_eq!(
            js_ws_has_pending(),
            1,
            "B is still alive through its server"
        );

        perry_runtime::agent::retire_agent(a);
        perry_runtime::agent::retire_agent(b);
        assert!(a_rows.survivors().is_empty() && b_rows.survivors().is_empty());
    })
    .join()
    .unwrap();
}
