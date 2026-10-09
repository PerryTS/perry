//! N8 exercises the actual HTTP decoder and public upgrade delivery against
//! a TCP peer. Test bookkeeping contains no JS values or native transport maps.

use super::*;
use perry_ffi::{JsThis, RawClosureHeader};
use std::cell::{Cell, RefCell};
use std::io::{Read, Write};
use std::time::{Duration, Instant};

thread_local! {
    static CELL: Cell<usize> = const { Cell::new(0) };
    static READ: RefCell<String> = const { RefCell::new(String::new()) };
    static HANDLE: RefCell<String> = const { RefCell::new(String::new()) };
    static CLOSED: Cell<bool> = const { Cell::new(false) };
    static UPGRADED: Cell<bool> = const { Cell::new(false) };
    static TAIL: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn undefined() -> f64 {
    f64::from_bits(JsValue::UNDEFINED.bits())
}
fn boxed(raw: i64) -> f64 {
    f64::from_bits(JsValue::from_object_ptr(raw as *mut u8).bits())
}

fn diagnostics(socket: f64) -> (String, String, u8) {
    // The ABI validates the opaque core's layout before returning its pointer.
    // This diagnostic borrow is read-only and ends before any callback or GC.
    let core = unsafe {
        &*(net::core(socket).unwrap()
            as *const perry_runtime::turnloop_net::transport::TransportCore)
    };
    (
        format!("{:?}", core.read_op()),
        format!("{:?}", core.handle()),
        core.route(),
    )
}

unsafe extern "C" fn connection(closure: *const RawClosureHeader, _: JsThis, owner: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let socket = scope.root_nanbox(owner);
    let holder = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 0));
    net::own_set(holder.get(), "socket", socket.get());
    undefined()
}

unsafe extern "C" fn closed(_: *const RawClosureHeader, _: JsThis, _: f64) -> f64 {
    CLOSED.with(|seen| seen.set(true));
    undefined()
}

unsafe extern "C" fn data(closure: *const RawClosureHeader, this: JsThis, bytes: f64) -> f64 {
    let socket = RootedSocket::new(this.as_f64());
    let bytes = crate::server::types::jsvalue_to_body_bytes(bytes).unwrap();
    TAIL.with(|tail| tail.borrow_mut().extend_from_slice(&bytes));
    let done = TAIL.with(|tail| tail.borrow().len() >= 6);
    if done {
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
        net::flow(socket.value());
    }
    undefined()
}

unsafe extern "C" fn upgraded(
    closure: *const RawClosureHeader,
    _: JsThis,
    request: f64,
    owner: f64,
    head: f64,
) -> f64 {
    let socket = RootedSocket::new(owner);
    let scope = TransientRootScope::enter();
    let holder = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 0));
    assert_eq!(
        net::own_get(holder.get(), "socket").to_bits(),
        socket.value().to_bits()
    );
    assert_eq!(
        net::socket_link(socket.value()).unwrap().raw(),
        CELL.with(Cell::get)
    );
    let (read, handle, route) = diagnostics(socket.value());
    assert_eq!(route, net::ROUTE, "upgrade must change the current route");
    READ.with(|before| {
        assert_eq!(
            &read,
            &*before.borrow(),
            "the pending read must not be resubmitted"
        )
    });
    HANDLE.with(|before| {
        assert_eq!(
            &handle,
            &*before.borrow(),
            "the driver handle must not be replaced"
        )
    });
    let request = (request.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
    assert_eq!(
        perry_ffi::get_handle::<IncomingMessage>(request)
            .unwrap()
            .socket_value
            .to_bits(),
        socket.value().to_bits()
    );
    assert_eq!(
        crate::server::types::jsvalue_to_body_bytes(head).unwrap(),
        b"\0HEAD\xff",
        "binary upgrade head must be preserved"
    );
    let state = scope.root_nanbox(net::state(socket.value()));
    let parser = scope.root_nanbox(net::own_get(state.get(), "parser"));
    assert_eq!(
        np::lifecycle(parser.get(), &PARSER),
        Ok(np::Lifecycle::Closed)
    );
    assert_eq!(net::write(socket.value(), b"READY", 0).unwrap(), 5);
    let callback = scope.root_addr(perry_ffi::alloc_closure(
        perry_ffi::js_function_info!(data, 1; with_flags(perry_ffi::FN_BUILTIN)),
        0,
    ) as i64);
    net::once(socket.value(), "data", boxed(callback.get()));
    net::flow(socket.value());
    UPGRADED.with(|seen| seen.set(true));
    undefined()
}

fn turn_until(mut done: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < until, "HTTP upgrade stalled");
        perry_runtime::event_pump::js_loop_turn_bounded(2);
        crate::server::server::js_node_http_server_process_pending();
        perry_runtime::promise::js_promise_run_microtasks();
    }
}

#[test]
fn n8_http_upgrade_preserves_socket_cell_read_and_binary_head_and_tail() {
    std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        assert!(super::super::enabled());
        crate::server::ensure_gc_scanner_registered();
        let scope = TransientRootScope::enter();
        let holder = scope.root_addr(perry_runtime::object::js_object_alloc(0, 0) as i64);
        let mut server = crate::server::server::HttpServer::with_handler(0);
        for (event, callback) in [
            ("connection", perry_ffi::js_function_info!(connection, 1; with_flags(perry_ffi::FN_BUILTIN))),
            ("upgrade", perry_ffi::js_function_info!(upgraded, 3; with_flags(perry_ffi::FN_BUILTIN))),
        ] {
            let callback = scope.root_addr(perry_ffi::alloc_closure(callback, 1) as i64);
            unsafe { perry_ffi::set_closure_capture_f64(callback.get() as *mut RawClosureHeader, 0, boxed(holder.get())); }
            server.listeners.insert(event.into(), vec![callback.get()]);
        }
        let server = perry_ffi::register_handle(server);
        let (listener, port, _) = super::super::listen(server, "127.0.0.1", 0, 128, None, false, true, 6000).unwrap();
        let listener = scope.root_nanbox(listener);
        let (go, ready) = std::sync::mpsc::channel();
        let peer = std::thread::spawn(move || {
            let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            ready.recv_timeout(Duration::from_secs(10)).unwrap();
            stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: Upgrade\r\nUpgrade: raw\r\n\r\n\0HEAD\xff").unwrap();
            let mut ack = [0; 5];
            stream.read_exact(&mut ack).unwrap();
            assert_eq!(&ack, b"READY");
            stream.write_all(b"TAIL\0\xff").unwrap();
            let mut eof = [0];
            assert_eq!(stream.read(&mut eof).unwrap(), 0, "Socket.destroy must close the peer descriptor");
        });
        turn_until(|| net::socket_link(net::own_get(boxed(holder.get()), "socket")).is_ok());
        let socket = RootedSocket::new(net::own_get(boxed(holder.get()), "socket"));
        let (read, handle, _) = diagnostics(socket.value());
        assert_ne!(read, "None", "the accepted Socket has already armed its real read");
        READ.with(|before| *before.borrow_mut() = read);
        HANDLE.with(|before| *before.borrow_mut() = handle);
        CELL.with(|before| before.set(net::socket_link(socket.value()).unwrap().raw()));
        go.send(()).unwrap();
        turn_until(|| UPGRADED.with(Cell::get) && CLOSED.with(Cell::get) && TAIL.with(|tail| tail.borrow().len() == 6));
        peer.join().unwrap();
        net::close_server(listener.get());
        perry_runtime::agent::retire_agent(agent);
        perry_ffi::drop_handle(server);
    }).join().unwrap();
}
