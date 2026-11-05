//! Real raw HTTP upgrade -> noServer WS, including deferred authentication.
use super::*;
use perry_ffi::{JsThis, RawClosureHeader};
use std::io::{Read, Write};
use std::time::{Duration, Instant};

fn undefined() -> f64 {
    f64::from_bits(JsValue::UNDEFINED.bits())
}
fn boxed(raw: i64) -> f64 {
    f64::from_bits(JsValue::from_object_ptr(raw as *mut u8).bits())
}
fn closure(info: &'static perry_ffi::JsFunctionInfo, holder: f64) -> i64 {
    let scope = TransientRootScope::enter();
    let holder = scope.root_nanbox(holder);
    let callback = perry_ffi::alloc_closure(info, 1);
    unsafe {
        perry_ffi::set_closure_capture_f64(callback as *mut RawClosureHeader, 0, holder.get())
    };
    callback as i64
}
unsafe extern "C" fn connection(c: *const RawClosureHeader, _: JsThis, socket: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let socket = scope.root_nanbox(socket);
    let holder = scope.root_nanbox(perry_ffi::closure_capture_f64(c, 0));
    net::own_set(holder.get(), "acceptedSocket", socket.get());
    undefined()
}
unsafe extern "C" fn upgrade(
    c: *const RawClosureHeader,
    _: JsThis,
    req: f64,
    socket: f64,
    head: f64,
) -> f64 {
    let scope = TransientRootScope::enter();
    let req = scope.root_nanbox(req);
    let socket = scope.root_nanbox(socket);
    let head = scope.root_nanbox(head);
    let holder = scope.root_nanbox(perry_ffi::closure_capture_f64(c, 0));
    net::own_set(holder.get(), "req", req.get());
    net::own_set(holder.get(), "socket", socket.get());
    net::own_set(holder.get(), "head", head.get());
    undefined()
}
unsafe extern "C" fn message(c: *const RawClosureHeader, _: JsThis, data: f64, _: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let data = scope.root_nanbox(data);
    let holder = scope.root_nanbox(perry_ffi::closure_capture_f64(c, 0));
    let ws = net::own_get(holder.get(), "ws") as i64;
    perry_ext_ws::js_ws_send_value(ws, data.get(), undefined());
    undefined()
}
unsafe extern "C" fn accepted(c: *const RawClosureHeader, _: JsThis, ws: f64, _: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let holder = scope.root_nanbox(perry_ffi::closure_capture_f64(c, 0));
    let id = (ws.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
    net::own_set(holder.get(), "ws", id as f64);
    let cb = scope.root_addr(closure(
        perry_ffi::js_function_info!(message, 2; with_flags(perry_ffi::FN_BUILTIN)),
        holder.get(),
    ));
    let name = perry_ffi::alloc_string("message");
    perry_ext_ws::js_ws_on(id, name.as_raw(), cb.get());
    undefined()
}
extern "C" {
    fn js_run_agent_pumps() -> i32;
}
fn turn_until(mut done: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < until, "noServer upgrade stalled");
        perry_runtime::event_pump::js_loop_turn_bounded(2);
        crate::server::server::js_node_http_server_process_pending();
        // The fixture is a non-main agent, so `ws` events reach it only through
        // the per-agent pump `perry-ext-ws` registers, as in a `worker_threads`
        // worker's own loop; naming `js_ws_process_pending` here would hide a
        // pump that only the main loop runs.
        unsafe { js_run_agent_pumps() };
        perry_runtime::promise::js_promise_run_microtasks();
    }
}
fn run(valid: bool) {
    std::thread::spawn(move || {
        let agent = perry_runtime::agent::enter_worker_agent();
        assert!(super::super::enabled());
        crate::server::ensure_gc_scanner_registered();
        let scope = TransientRootScope::enter();
        let holder = scope.root_addr(perry_runtime::object::js_object_alloc(0, 0) as i64);
        let opts = scope.root_addr(perry_runtime::object::js_object_alloc(0, 0) as i64);
        net::own_set(boxed(opts.get()), "noServer", f64::from_bits(JsValue::TRUE.bits()));
        let wss = perry_ext_ws::js_ws_server_new(boxed(opts.get()));
        let cb = scope.root_addr(closure(perry_ffi::js_function_info!(upgrade, 3; with_flags(perry_ffi::FN_BUILTIN)), boxed(holder.get())));
        let mut server = crate::server::server::HttpServer::with_handler(0);
        server.listeners.insert("upgrade".into(), vec![cb.get()]);
        let connected = scope.root_addr(closure(perry_ffi::js_function_info!(connection, 1; with_flags(perry_ffi::FN_BUILTIN)), boxed(holder.get())));
        server.listeners.insert("connection".into(), vec![connected.get()]);
        let server = perry_ffi::register_handle(server);
        let (listener, port, _) = super::super::listen(server, "127.0.0.1", 0, 128, None, false, true, 6000).unwrap();
        let listener = scope.root_nanbox(listener);
        let (go, admitted) = std::sync::mpsc::channel();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let peer = std::thread::spawn(move || {
            let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            stream.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
            admitted.recv_timeout(Duration::from_secs(10)).unwrap();
            let key = if valid { "dGhlIHNhbXBsZSBub25jZQ==" } else { "invalid" };
            let mut request = format!("GET /ws HTTP/1.1\r\nHost: localhost\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n").into_bytes();
            // Masked "hello", pipelined immediately behind the request head.
            request.extend_from_slice(&[0x81, 0x85, 1, 2, 3, 4, b'h'^1, b'e'^2, b'l'^3, b'l'^4, b'o'^1]);
            stream.write_all(&request).unwrap();
            let mut byte = [0];
            let error = stream.read(&mut byte).expect_err("no 101 before application authentication");
            assert!(matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut));
            ready_tx.send(()).unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            let mut response = Vec::new();
            while !response.ends_with(b"\r\n\r\n") { stream.read_exact(&mut byte).unwrap(); response.push(byte[0]); }
            if valid {
                assert!(response.starts_with(b"HTTP/1.1 101 "), "{:?}", response);
                let mut echo = [0; 7]; stream.read_exact(&mut echo).unwrap();
                assert_eq!(&echo[2..], b"hello", "pipelined message survives and echoes");
                assert_eq!(echo[1], 5);
            } else {
                assert!(response.starts_with(b"HTTP/1.1 400 "), "{:?}", response);
                let mut body = Vec::new(); stream.read_to_end(&mut body).unwrap();
            }
            done_tx.send(()).unwrap();
        });
        // Hold the accepted Socket through an ordinary traced JS edge, as
        // Node's live server does; this low-level fixture bypasses that JS server.
        turn_until(|| net::socket_link(net::own_get(boxed(holder.get()), "acceptedSocket")).is_ok());
        go.send(()).unwrap();
        let mut ready = false;
        turn_until(|| {
            ready |= ready_rx.try_recv().is_ok();
            ready && net::socket_link(net::own_get(boxed(holder.get()), "socket")).is_ok()
        });
        // A separate event-loop turn after the upgrade callback simulates a
        // delayed auth lookup; request/socket/head live in ordinary JS edges.
        let req = scope.root_nanbox(net::own_get(boxed(holder.get()), "req"));
        let socket = scope.root_nanbox(net::own_get(boxed(holder.get()), "socket"));
        let head = scope.root_nanbox(net::own_get(boxed(holder.get()), "head"));
        let cb = scope.root_addr(closure(perry_ffi::js_function_info!(accepted, 2; with_flags(perry_ffi::FN_BUILTIN)), boxed(holder.get())));
        unsafe { perry_ext_ws::js_ws_handle_upgrade(wss, req.get(), socket.get(), head.get(), cb.get()) };
        if valid { assert!(net::own_get(boxed(holder.get()), "ws") > 0.0); }
        else { assert!(JsValue::from_bits(net::own_get(boxed(holder.get()), "ws").to_bits()).is_undefined()); }
        turn_until(|| done_rx.try_recv().is_ok());
        peer.join().unwrap();
        net::destroy(net::own_get(boxed(holder.get()), "socket"));
        net::close_server(listener.get());
        perry_runtime::agent::retire_agent(agent);
        perry_ffi::drop_handle(server);
    }).join().unwrap();
}
#[test]
fn no_server_deferred_upgrade_preserves_pipelined_frame() {
    run(true);
}
#[test]
fn no_server_invalid_handshake_returns_400_without_callback() {
    run(false);
}
