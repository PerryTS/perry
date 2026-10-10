//! Real client bytes above ordinary Socket cells, including public request
//! factories and exact capability witnesses on their existing app records.
use super::*;
use crate::client_turnloop::Mode;
use perry_ffi::{JsThis, RawClosureHeader};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

const CERT: &[u8] =
    include_bytes!("../../../../test-parity/node-suite/tls/fixtures/localhost-cert.pem");
const KEY: &[u8] =
    include_bytes!("../../../../test-parity/node-suite/tls/fixtures/localhost-key.pem");
thread_local! {
    static RESPONSE: Cell<bool> = const { Cell::new(false) };
    static UPGRADE: Cell<bool> = const { Cell::new(false) };
    static CLOSED: Cell<bool> = const { Cell::new(false) };
    static TAIL: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}
fn undefined() -> f64 {
    f64::from_bits(JsValue::UNDEFINED.bits())
}
fn boxed(raw: i64) -> f64 {
    f64::from_bits(JsValue::from_object_ptr(raw as *mut u8).bits())
}
fn raw(value: f64) -> i64 {
    JsValue::from_bits(value.to_bits()).as_pointer::<u8>() as i64
}
fn bytes(value: f64) -> Vec<u8> {
    perry_ffi::bytes::no_gc(|scope| {
        perry_ffi::read_buffer_bytes(raw(value) as *const perry_ffi::BufferHeader, scope)
            .expect("Socket data and upgrade head are Buffers")
            .to_vec()
    })
}
fn read_head(stream: &mut impl Read) -> Vec<u8> {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        assert!(bytes.len() < 32768);
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        bytes.push(byte[0]);
    }
    bytes
}
fn drive_until(mut done: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < until, "native HTTP client stalled");
        perry_runtime::event_pump::js_loop_turn_bounded(2);
        unsafe {
            crate::js_http_process_pending();
        }
        perry_runtime::promise::js_promise_run_microtasks();
    }
}
fn request(owner: f64, mode: Mode, port: u16, secure: bool, callback: i64) -> Outbound {
    let url = format!(
        "{}://127.0.0.1:{port}/client",
        if secure { "https" } else { "http" }
    );
    let request = perry_ffi::register_handle(ClientRequestHandle {
        owner_agent: perry_ffi::agent_post::current_agent(),
        surface: crate::client_request_surface::ClientRequestSurfaceState::default(),
        async_id: 0,
        method: "GET".into(),
        url: url.clone(),
        headers: HashMap::new(),
        body: Vec::new(),
        response_callback: if mode == Mode::Upgrade { 0 } else { callback },
        response_raw_wrapper: 0,
        listeners: if mode == Mode::Upgrade {
            HashMap::from([(
                "upgrade".into(),
                vec![crate::ClientEventListener {
                    callback,
                    raw_wrapper: 0,
                    once: false,
                }],
            )])
        } else {
            HashMap::new()
        },
        timeout_ms: None,
        ended: true,
        flushed_early: false,
        pending_write_callbacks: Vec::new(),
        end_callback: 0,
        completed: false,
        timeout_fired: false,
        close_emitted: false,
        agent_handle: 0,
        agent_key: format!("127.0.0.1:{port}:"),
        request_create_connection: 0,
        agent_active: false,
        agent_queued: false,
        reused_socket: false,
        socket_handle: raw(owner),
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
    crate::ensure_gc_scanner_registered();
    let options = crate::tls_client::TlsOptions {
        ca_pems: vec![CERT.to_vec()],
        ..Default::default()
    };
    Outbound {
        request_handle: request,
        method: "GET".into(),
        url: url::Url::parse(&url).unwrap(),
        headers: if mode == Mode::Upgrade {
            HashMap::from([
                ("Connection".into(), "Upgrade".into()),
                ("Upgrade".into(), "raw".into()),
            ])
        } else {
            HashMap::new()
        },
        body: Vec::new(),
        timeout_ms: None,
        mode,
        reuse: None,
        key: PoolKey {
            agent: 0,
            https: secure,
            host: "127.0.0.1".into(),
            port,
            proxy: None,
            tls: 0,
        },
        tls: secure.then(|| super::super::tls::plan(&options, "127.0.0.1").unwrap()),
        proxy: None,
        extra: Vec::new(),
    }
}
unsafe extern "C" fn response(closure: *const RawClosureHeader, _: JsThis, value: f64) -> f64 {
    let incoming = perry_ffi::get_handle::<crate::IncomingMessageHandle>(raw(value)).unwrap();
    assert_eq!(incoming.status_code, 200);
    assert_eq!(incoming.body, b"payload");
    assert_eq!(
        incoming.socket_handle,
        raw(perry_ffi::closure_capture_f64(closure, 0))
    );
    assert_eq!(
        incoming.trailers.get("x-end").map(String::as_str),
        Some("yes")
    );
    RESPONSE.with(|seen| seen.set(true));
    undefined()
}
#[test]
fn a_fragmented_client_response_uses_the_requests_actual_socket() {
    fragmented_response(false);
}
#[test]
fn public_http_request_creates_and_drives_the_actual_socket() {
    fragmented_response(true);
}
fn fragmented_response(public_factory: bool) {
    std::thread::spawn(move || {
        let agent = perry_runtime::agent::enter_worker_agent();
        RESPONSE.with(|seen| seen.set(false));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let head = read_head(&mut stream);
            assert!(head.starts_with(b"GET /client HTTP/1.1\r\n"));
            assert!(head.ends_with(b"Connection: close\r\n\r\n"));
            for bytes in [
                b"HTTP/1.1 200 OK\r\nTransfer-Encod".as_slice(),
                b"ing: chunked\r\n\r\n7\r\npay",
                b"load\r\n0\r\nX-End: yes\r\n\r\n",
            ] {
                stream.write_all(bytes).unwrap();
            }
        });
        let scope = TransientRootScope::enter();
        let completed = super::super::completed_total();
        let accepted = super::super::accepted_total();
        let callback = scope.root_addr(perry_ffi::alloc_closure(
            perry_ffi::js_function_info!(response, 1; with_flags(perry_ffi::FN_BUILTIN)),
            1,
        ) as i64);
        let socket = if public_factory {
            let url = scope.root_nanbox(f64::from_bits(
                JsValue::from_string_ptr(
                    perry_ffi::alloc_string(&format!("http://127.0.0.1:{port}/client")).as_raw(),
                )
                .bits(),
            ));
            let request = unsafe { crate::js_http_request(url.get(), callback.get()) };
            crate::client_request_surface::set_header(request, "TE", "trailers".into());
            let socket = scope.root_nanbox(boxed(
                perry_ffi::get_handle::<ClientRequestHandle>(request)
                    .unwrap()
                    .socket_handle,
            ));
            unsafe {
                perry_ffi::set_closure_capture_f64(
                    callback.get() as *mut RawClosureHeader,
                    0,
                    socket.get(),
                );
                crate::js_http_client_request_end(request, undefined());
            }
            assert_eq!(super::super::accepted_total(), accepted + 1);
            socket
        } else {
            let socket = scope.root_nanbox(net::new_socket(SUBSYSTEM, undefined()));
            unsafe {
                perry_ffi::set_closure_capture_f64(
                    callback.get() as *mut RawClosureHeader,
                    0,
                    socket.get(),
                );
            }
            let out = request(socket.get(), Mode::Trailers, port, false, callback.get());
            start_on_socket(socket.get(), out, None, false).unwrap();
            socket
        };
        let cell = net::socket_link(socket.get()).unwrap();
        drive_until(|| RESPONSE.with(Cell::get));
        assert_eq!(net::socket_link(socket.get()).unwrap(), cell);
        assert_eq!(super::super::completed_total(), completed + 1);
        assert_eq!(
            np::lifecycle(net::own_get(net::state(socket.get()), PARSER_EDGE), &PARSER),
            Ok(np::Lifecycle::Closed)
        );
        assert_eq!(crate::js_ext_http_client_inflight(), 0);
        peer.join().unwrap();
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}
fn diagnostics(owner: f64) -> (usize, String, String, u8, usize) {
    let link = net::socket_link(owner).unwrap();
    let core = unsafe {
        &*(net::core(owner).unwrap()
            as *const perry_runtime::turnloop_net::transport::TransportCore)
    };
    let refs =
        unsafe { (*(link.raw() as *const perry_runtime::native_handle::NativeHandleHeader)).refs };
    (
        link.raw(),
        format!("{:?}", core.read_op()),
        format!("{:?}", core.handle()),
        core.route(),
        refs as usize,
    )
}
unsafe extern "C" fn closed(_: *const RawClosureHeader, _: JsThis, _: f64) -> f64 {
    CLOSED.with(|seen| seen.set(true));
    undefined()
}
unsafe extern "C" fn data(closure: *const RawClosureHeader, this: JsThis, value: f64) -> f64 {
    let socket = RootedSocket::new(this.as_f64());
    let bytes = bytes(value);
    TAIL.with(|tail| tail.borrow_mut().extend_from_slice(&bytes));
    if TAIL.with(|tail| tail.borrow().len() >= 6) {
        TAIL.with(|tail| assert_eq!(&*tail.borrow(), b"TAIL\0\xff"));
        net::destroy(socket.value());
        let scope = TransientRootScope::enter();
        let callback = scope.root_addr(perry_ffi::alloc_closure(
            perry_ffi::js_function_info!(closed, 1; with_flags(perry_ffi::FN_BUILTIN)),
            0,
        ) as i64);
        net::once(socket.value(), "close", boxed(callback.get()));
    } else {
        net::once(socket.value(), "data", boxed(closure as i64));
    }
    undefined()
}
unsafe extern "C" fn upgrade(
    closure: *const RawClosureHeader,
    _: JsThis,
    incoming: f64,
    value: f64,
    head: f64,
) -> f64 {
    let socket = RootedSocket::new(value);
    assert_eq!(
        perry_ffi::closure_capture_f64(closure, 0).to_bits(),
        socket.value().to_bits()
    );
    assert_eq!(
        perry_ffi::get_handle::<crate::IncomingMessageHandle>(raw(incoming))
            .unwrap()
            .socket_handle,
        raw(socket.value())
    );
    let holder = perry_ffi::closure_capture_f64(closure, 1);
    let (cell, read, handle, route, refs) = diagnostics(socket.value());
    assert_eq!(
        route,
        net::ROUTE,
        "client upgrade must change the current route"
    );
    assert_eq!(cell, net::own_get(holder, "cell") as usize);
    assert_eq!(
        read,
        JsValue::from_bits(net::own_get(holder, "read").to_bits())
            .to_owned_string()
            .unwrap(),
        "client upgrade must preserve the pending read"
    );
    assert_eq!(
        handle,
        JsValue::from_bits(net::own_get(holder, "handle").to_bits())
            .to_owned_string()
            .unwrap(),
        "client upgrade must preserve the driver handle"
    );
    assert_eq!(
        refs,
        net::own_get(holder, "refs") as usize,
        "client upgrade must preserve owed refs"
    );
    assert_eq!(
        bytes(head),
        b"\0HEAD\xff",
        "client binary upgrade head must be preserved"
    );
    assert!(
        net::tls_installed(socket.value()),
        "TLS must stay owned by the upgraded Socket"
    );
    assert_eq!(
        np::lifecycle(
            net::own_get(net::state(socket.value()), PARSER_EDGE),
            &PARSER
        ),
        Ok(np::Lifecycle::Closed)
    );
    net::write(socket.value(), b"READY", 0).unwrap();
    let scope = TransientRootScope::enter();
    let callback = scope.root_addr(perry_ffi::alloc_closure(
        perry_ffi::js_function_info!(data, 1; with_flags(perry_ffi::FN_BUILTIN)),
        0,
    ) as i64);
    net::once(socket.value(), "data", boxed(callback.get()));
    UPGRADE.with(|seen| seen.set(true));
    undefined()
}
#[test]
fn n8_https_client_upgrade_preserves_socket_cell_tls_read_head_and_tail() {
    std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent(); UPGRADE.with(|seen| seen.set(false)); CLOSED.with(|seen| seen.set(false)); TAIL.with(|tail| tail.borrow_mut().clear());
        let certs = rustls_pemfile::certs(&mut std::io::Cursor::new(CERT)).collect::<Result<Vec<_>, _>>().unwrap(); let key = rustls_pemfile::private_key(&mut std::io::Cursor::new(KEY)).unwrap().unwrap();
        let config = std::sync::Arc::new(rustls::ServerConfig::builder_with_provider(std::sync::Arc::new(rustls::crypto::ring::default_provider())).with_safe_default_protocol_versions().unwrap().with_no_client_auth().with_single_cert(certs, key).unwrap());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap(); let port = listener.local_addr().unwrap().port(); let (go, ready) = std::sync::mpsc::channel();
        let peer = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap(); stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap(); let session = rustls::ServerConnection::new(config).unwrap(); let mut stream = rustls::StreamOwned::new(session, stream);
            assert!(read_head(&mut stream).starts_with(b"GET /client HTTP/1.1\r\n")); ready.recv_timeout(Duration::from_secs(10)).unwrap();
            stream.write_all(b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: raw\r\n\r\n\0HEAD\xff").unwrap(); stream.flush().unwrap();
            let mut ack = [0;5]; stream.read_exact(&mut ack).unwrap(); assert_eq!(&ack, b"READY"); stream.write_all(b"TAIL\0\xff").unwrap(); stream.flush().unwrap(); let mut rest = Vec::new(); let _ = stream.read_to_end(&mut rest);
        });
        let scope = TransientRootScope::enter(); let socket = scope.root_nanbox(net::new_socket(SUBSYSTEM, undefined())); let holder = scope.root_nanbox(f64::from_bits(perry_ffi::alloc_object().bits()));
        let callback = scope.root_addr(perry_ffi::alloc_closure(perry_ffi::js_function_info!(upgrade, 3; with_flags(perry_ffi::FN_BUILTIN)), 2) as i64);
        unsafe { perry_ffi::set_closure_capture_f64(callback.get() as *mut RawClosureHeader, 0, socket.get()); perry_ffi::set_closure_capture_f64(callback.get() as *mut RawClosureHeader, 1, holder.get()); }
        let out = request(socket.get(), Mode::Upgrade, port, true, callback.get()); let handshakes = super::super::tls_handshakes_total(); start_on_socket(socket.get(), out, None, false).unwrap();
        drive_until(|| super::super::tls_handshakes_total() == handshakes + 1 && net::queued_bytes(socket.get()) == 0);
        let (cell, read, handle, route, refs) = diagnostics(socket.get()); assert_eq!(route, SUBSYSTEM); assert_ne!(read, "None"); net::own_set(holder.get(), "cell", cell as f64); net::own_set(holder.get(), "refs", refs as f64);
        for (key, text) in [("read", read), ("handle", handle)] { let text = scope.root_nanbox(f64::from_bits(JsValue::from_string_ptr(perry_ffi::alloc_string(&text).as_raw()).bits())); net::own_set(holder.get(), key, text.get()); }
        go.send(()).unwrap(); drive_until(|| UPGRADE.with(Cell::get) && CLOSED.with(Cell::get)); assert_eq!(crate::js_ext_http_client_inflight(), 0); peer.join().unwrap(); perry_runtime::agent::retire_agent(agent);
    }).join().unwrap();
}

#[test]
fn a_parked_socket_expires_on_its_unreferenced_deadline() {
    std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            assert!(read_head(&mut stream).starts_with(b"GET /client HTTP/1.1\r\n"));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\na")
                .unwrap();
            let mut byte = [0];
            assert_eq!(
                stream.read(&mut byte).unwrap(),
                0,
                "idle expiry must close the actual descriptor"
            );
        });
        let scope = TransientRootScope::enter();
        CLOSED.with(|seen| seen.set(false));
        let owner = scope.root_nanbox(net::new_socket(SUBSYSTEM, undefined()));
        let close = scope.root_addr(perry_ffi::alloc_closure(
            perry_ffi::js_function_info!(closed, 1; with_flags(perry_ffi::FN_BUILTIN)),
            0,
        ) as i64);
        net::once(owner.get(), "close", boxed(close.get()));
        let mut out = request(owner.get(), Mode::Normal, port, false, 0);
        out.reuse = Some(super::super::Reuse {
            max_free: 1,
            idle_ms: 25,
        });
        start_on_socket(owner.get(), out, None, false).unwrap();
        let deadline = Instant::now() + Duration::from_millis(500);
        while !JsValue::from_bits(net::get(owner.get(), "destroyed").to_bits()).to_bool() {
            assert!(
                Instant::now() < deadline,
                "the HTTP idle deadline must expire, not be parked"
            );
            perry_runtime::event_pump::js_loop_turn_bounded(2);
            unsafe {
                crate::js_http_process_pending();
            }
        }
        assert_eq!(crate::js_ext_http_client_inflight(), 0);
        assert_eq!(
            np::lifecycle(net::own_get(net::state(owner.get()), PARSER_EDGE), &PARSER),
            Ok(np::Lifecycle::Closed)
        );
        drive_until(|| CLOSED.with(Cell::get));
        peer.join().unwrap();
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}

#[test]
fn delayed_agent_cleanup_preserves_a_reopened_socket_capability() {
    std::thread::spawn(|| {
        let worker = perry_runtime::agent::enter_worker_agent();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let scope = TransientRootScope::enter();
        let owner = scope.root_nanbox(net::new_socket(net::ROUTE, undefined()));
        let original = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (accepted, _) = listener.accept().unwrap();
        let cell = net::socket_link(owner.get()).unwrap();
        unsafe {
            tl::link_adopt_stream(&mut *net::core(owner.get()).unwrap(), cell, original.into())
                .unwrap();
        }
        net::adopted(owner.get());
        let old = net::snapshot(owner.get()).unwrap();
        let agent = unsafe { crate::agent::js_http_agent_new(undefined()) };
        let out = request(owner.get(), Mode::Normal, port, false, 0);
        let request = out.request_handle;
        // Use the request's actual existing Agent key and ownership edges.
        let key = perry_ffi::get_handle_mut::<ClientRequestHandle>(request)
            .map(|req| {
                req.agent_handle = agent;
                req.agent_active = true;
                req.socket_snapshot = Some(old);
                req.agent_key.clone()
            })
            .unwrap();
        let agent_record = perry_ffi::get_handle_mut::<crate::agent::AgentHandle>(agent).unwrap();
        agent_record.sockets.insert(key.clone(), 1);
        agent_record
            .active_socket_handles
            .insert(key.clone(), vec![raw(owner.get())]);
        net::destroy(owner.get());
        let host = scope.root_nanbox(f64::from_bits(
            JsValue::from_string_ptr(perry_ffi::alloc_string("127.0.0.1").as_raw()).bits(),
        ));
        net::connect(owner.get(), port as f64, host.get(), undefined());
        let replacement =
            net::snapshot(owner.get()).expect("reopen creates a new driver capability");
        assert_eq!(
            net::socket_link(owner.get()).unwrap(),
            cell,
            "reopen retains the same cell"
        );
        assert!(!net::matches(owner.get(), &old));
        assert!(
            crate::current_request_socket(request).is_none(),
            "request continuation must refuse the replacement incarnation"
        );
        let stale = net::RootedSocket::with_snapshot(owner.get(), old);
        assert!(
            stale.write(b"stale response", 0).is_none(),
            "carried snapshot must prevent native writes to the reopened cell"
        );
        assert!(unsafe { stale.with_native_io(|_| ()) }.is_none());
        unsafe {
            crate::finish_agent_request(request, false);
        }
        assert!(
            net::matches(owner.get(), &replacement),
            "delayed HTTP cleanup must preserve the reopened driver capability"
        );
        assert!(!JsValue::from_bits(net::get(owner.get(), "destroyed").to_bits()).to_bool());
        assert!(perry_ffi::get_handle::<crate::agent::AgentHandle>(agent)
            .unwrap()
            .active_socket_handles
            .get(&key)
            .is_none_or(Vec::is_empty));
        net::destroy(owner.get());
        drop(accepted);
        perry_runtime::agent::retire_agent(worker);
    })
    .join()
    .unwrap();
}
