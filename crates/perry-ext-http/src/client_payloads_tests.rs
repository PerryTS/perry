//! A finished exchange's payloads on a real collector (perry-feather#111).
use super::*;
use perry_ffi::handle_exists;
use perry_ffi::handle_lifecycle::is_parked;
use perry_runtime::gc::RuntimeHandleScope;

const UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

/// Two full collections: the first ages every parked id past its young
/// epoch, the second decides by reachability alone. The trace proves the dead
/// records; the mutator releases them outside the pause, so the test releases
/// them as the next turn would.
fn collect() {
    perry_runtime::gc::js_gc_collect();
    perry_runtime::gc::js_gc_collect();
    perry_ffi::handle_lifecycle::release_all_proven();
}

/// Each test runs on its own mutator: the GC, its roots and the parked set
/// are per-thread.
fn on_mutator(f: impl FnOnce() + Send + 'static) {
    let _serial = crate::tests::GC_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    std::thread::spawn(move || {
        perry_runtime::gc::gc_init();
        perry_ffi::gc_register_mutable_root_scanner_named("perry-ext-http-client", scan_http_roots);
        f();
    })
    .join()
    .unwrap();
}

extern "C" fn noop_listener(_: *const RawClosureHeader, _: perry_ffi::JsThis) -> f64 {
    f64::from_bits(UNDEFINED)
}

/// A request whose listener closure captures the request itself, the cycle
/// every `req.on('x', () => req...)` creates.
fn request(closed: bool) -> Handle {
    let request = register_handle(ClientRequestHandle {
        surface: client_request_surface::ClientRequestSurfaceState::default(),
        owner_agent: perry_ffi::agent_post::current_agent(),
        async_id: 0,
        method: "GET".to_string(),
        url: "http://localhost/".to_string(),
        headers: HashMap::new(),
        body: Vec::new(),
        response_callback: 0,
        response_raw_wrapper: 0,
        listeners: HashMap::new(),
        timeout_ms: None,
        ended: true,
        flushed_early: false,
        pending_write_callbacks: Vec::new(),
        end_callback: 0,
        completed: closed,
        timeout_fired: false,
        close_emitted: closed,
        agent_handle: 0,
        agent_key: "localhost::".to_string(),
        request_create_connection: 0,
        agent_active: false,
        agent_queued: false,
        reused_socket: false,
        socket_handle: 0,
        socket_snapshot: None,
        abort_signal_bits: 0,
        abort_listener_bits: 0,
        tls: crate::tls_client::TlsOptions::default(),
        preflight_error: None,
        incoming_handle: 0,
        expects_continue: false,
        continue_body_pending: false,
        agent_false: false,
    });
    let listener = perry_ffi::alloc_closure(perry_ffi::js_function_info!(noop_listener, 0), 1);
    unsafe {
        perry_ffi::set_closure_capture_f64(listener, 0, client_payloads::handle_capture(request));
    }
    with_handle_mut::<ClientRequestHandle, _, _>(request, |req| {
        req.listeners.insert(
            "close".to_string(),
            vec![ClientEventListener::persistent(listener as i64)],
        );
    });
    request
}

/// The response of `request`, as the drain registers it.
fn response(request: Handle) -> Handle {
    let incoming = register_handle(IncomingMessageHandle {
        owner_agent: perry_ffi::agent_post::current_agent(),
        status_code: 200,
        status_message: "OK".to_string(),
        headers: vec![("x-kept".to_string(), "yes".to_string())],
        trailers: HashMap::new(),
        body: b"body".to_vec(),
        listeners: HashMap::new(),
        encoding: None,
        decoder_pending: Vec::new(),
        pipes: Vec::new(),
        socket_handle: 0,
        request_handle: request,
        http_version: (1, 1),
        complete: true,
        read: Default::default(),
    });
    with_handle_mut::<ClientRequestHandle, _, _>(request, |req| req.incoming_handle = incoming);
    incoming
}

#[test]
fn a_finished_exchange_nothing_names_is_collected_with_its_listener_cycle() {
    on_mutator(|| {
        let req = request(true);
        let res = response(req);
        client_payloads::park_client_exchange(req);
        assert!(is_parked(req) && is_parked(res));
        assert!(handle_exists(req), "parked payloads stay readable");
        collect();
        assert!(
            !handle_exists(req),
            "the listener capturing req must not keep it"
        );
        assert!(!handle_exists(res));
        assert!(perry_ffi::handle_registration(req).is_none());
    });
}

#[test]
fn an_unfinished_exchange_is_not_parked() {
    on_mutator(|| {
        let req = request(false);
        client_payloads::park_client_exchange(req);
        assert!(!is_parked(req));
        collect();
        assert!(handle_exists(req), "the native side still owns it");
        with_handle_mut::<ClientRequestHandle, _, _>(req, |r| r.close_emitted = true);
        client_payloads::park_client_exchange(req);
        assert!(is_parked(req));
        collect();
        assert!(!handle_exists(req));
    });
}

#[test]
fn a_held_response_keeps_both_payloads_until_its_last_holder_dies() {
    on_mutator(|| {
        let scope = RuntimeHandleScope::new();
        let req = request(true);
        let res = response(req);
        let root = scope.root_nanbox_u64(client_payloads::handle_word(res));
        client_payloads::park_client_exchange(req);
        collect();
        collect();
        // `res.req` and the headers answer after 'close', as in Node.
        let kept = perry_ffi::with_handle::<IncomingMessageHandle, _, _>(res, |m| {
            (m.request_handle, m.headers[0].1.clone(), m.body.clone())
        });
        assert_eq!(kept, Some((req, "yes".to_string(), b"body".to_vec())));
        assert!(handle_exists(req), "res.req keeps the request");
        root.set_nanbox_u64(UNDEFINED);
        collect();
        assert!(!handle_exists(res) && !handle_exists(req));
    });
}

#[test]
fn a_held_request_keeps_its_response() {
    on_mutator(|| {
        let scope = RuntimeHandleScope::new();
        let req = request(true);
        let res = response(req);
        let root = scope.root_nanbox_u64(client_payloads::handle_word(req));
        client_payloads::park_client_exchange(req);
        collect();
        collect();
        assert!(handle_exists(req) && handle_exists(res));
        root.set_nanbox_u64(UNDEFINED);
        collect();
        assert!(!handle_exists(res) && !handle_exists(req));
    });
}

#[test]
fn a_queued_event_names_its_parked_request() {
    on_mutator(|| {
        let req = request(true);
        client_payloads::park_client_exchange(req);
        push_event(PendingHttpEvent::ResponseEnd {
            request_handle: req,
        });
        collect();
        collect();
        assert!(
            handle_exists(req),
            "a pending event still owes its handler the id"
        );
        let owner = perry_ffi::agent_post::current_agent();
        HTTP_PENDING_EVENTS
            .lock()
            .unwrap()
            .retain(|(agent, _)| *agent != owner);
        collect();
        assert!(!handle_exists(req));
    });
}

#[test]
fn an_in_flight_request_is_named_by_the_inflight_set() {
    on_mutator(|| {
        let req = request(true);
        client_payloads::park_client_exchange(req);
        let entry = (perry_ffi::agent_post::current_agent(), req);
        CLIENT_REQUESTS_INFLIGHT.lock().unwrap().insert(entry);
        collect();
        collect();
        assert!(
            handle_exists(req),
            "the transport still owes it a terminal edge"
        );
        CLIENT_REQUESTS_INFLIGHT.lock().unwrap().remove(&entry);
        collect();
        assert!(!handle_exists(req));
    });
}

#[test]
fn a_native_closure_capture_names_its_request() {
    on_mutator(|| {
        let scope = RuntimeHandleScope::new();
        let req = request(true);
        // A pending timer/signal/continuation closure, held by JS-reachable
        // state (here a root), captures the request as a handle word.
        let closure = perry_ffi::alloc_closure(perry_ffi::js_function_info!(noop_listener, 0), 1);
        unsafe {
            perry_ffi::set_closure_capture_f64(closure, 0, client_payloads::handle_capture(req));
        }
        let root = scope.root_nanbox_u64(client_payloads::handle_word(0) | closure as u64);
        client_payloads::park_client_exchange(req);
        collect();
        collect();
        assert!(handle_exists(req), "the capture names the request");
        assert_eq!(
            unsafe { client_payloads::capture_handle_at(closure, 0) },
            req
        );
        root.set_nanbox_u64(UNDEFINED);
        collect();
        assert!(!handle_exists(req));
    });
}

#[test]
fn a_lease_defers_reissue_of_a_collected_request_id() {
    on_mutator(|| {
        let req = request(true);
        let lease = client_payloads::request_lease(req).expect("live registration");
        client_payloads::park_client_exchange(req);
        collect();
        assert!(!handle_exists(req), "the payload goes with the proof");
        perry_ffi::drain_quarantined_handles();
        assert!(
            perry_ffi::shared_handle_id_pool()
                .begin_registration_with_id(req, perry_ffi::NativeRegistrationKind::Payload)
                .is_err(),
            "a loop-thread holder still holds the id"
        );
        drop(lease);
        perry_ffi::drain_quarantined_handles();
        let identity = perry_ffi::shared_handle_id_pool()
            .begin_registration_with_id(req, perry_ffi::NativeRegistrationKind::Payload)
            .expect("reissuable once the lease is gone");
        let pool = perry_ffi::shared_handle_id_pool();
        assert!(pool.publish(identity));
        assert!(pool.begin_retirement_of(identity));
        assert!(pool.finish_retirement_reusable(identity));
    });
}
