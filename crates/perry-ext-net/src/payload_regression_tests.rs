//! Lost pre-payload coverage, using ordinary rooted Socket owners and real I/O.
use super::{native_transport as net, payload_events as events, payload_io as io};
use super::{payload_socket as socket, payload_transport as p};
use perry_ffi::{JsThis, RawClosureHeader, TransientRootScope};
use std::cell::RefCell;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

thread_local! { static SEEN: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) }; }
fn until(mut done: impl FnMut() -> bool) {
    let limit = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(
            Instant::now() < limit,
            "Socket completion did not reach its owning loop"
        );
        perry_runtime::event_pump::js_loop_turn_bounded(2);
        perry_runtime::promise::js_promise_run_microtasks();
    }
}
unsafe extern "C" fn observe(closure: *const RawClosureHeader, _: JsThis, error: f64) -> f64 {
    let label = perry_ffi::closure_capture_f64(closure, 0);
    let scope = TransientRootScope::enter();
    let error = scope.root_nanbox(error);
    let text = if label == 3.0 {
        "close".into()
    } else if label == 4.0 {
        "drain".into()
    } else {
        let code = crate::jsvalue_to_owned_string(p::own_get(error.get(), "code")).unwrap();
        let message = crate::jsvalue_to_owned_string(p::own_get(error.get(), "message")).unwrap();
        format!("{label}:{code}:{message}")
    };
    SEEN.with(|seen| seen.borrow_mut().push(text));
    p::undefined()
}
fn observer(scope: &TransientRootScope, label: f64) -> f64 {
    let callback = scope.root_addr(perry_ffi::alloc_closure(
        perry_ffi::js_function_info!(observe, 1; with_flags(perry_ffi::FN_BUILTIN)),
        1,
    ) as i64);
    unsafe {
        perry_ffi::set_closure_capture_f64(callback.get() as *mut RawClosureHeader, 0, label);
    }
    p::boxed_addr(callback.get())
}
#[test]
fn unopened_socket_refuses_native_bytes_and_queued_callbacks_error_once_before_close() {
    std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        SEEN.with(|seen| seen.borrow_mut().clear());
        let scope = TransientRootScope::enter();
        let owner = scope.root_nanbox(net::new_socket(io::ROUTE, p::undefined()));
        assert!(net::RootedSocket::new(owner.get())
            .write(b"refused", 0)
            .unwrap()
            .is_err());
        socket::once(owner.get(), "error", observer(&scope, 2.0));
        socket::once(owner.get(), "close", observer(&scope, 3.0));
        let chunk = scope.root_nanbox(events::string("refused"));
        for label in [0.0, 1.0] {
            assert_eq!(
                socket::write(
                    owner.get(),
                    chunk.get(),
                    p::undefined(),
                    observer(&scope, label)
                )
                .to_bits(),
                perry_ffi::JsValue::FALSE.bits()
            );
        }
        until(|| SEEN.with(|seen| seen.borrow().len() == 4));
        perry_runtime::promise::js_promise_run_microtasks();
        SEEN.with(|seen| {
            assert_eq!(
                *seen.borrow(),
                [
                    "0:ERR_SOCKET_CLOSED:Socket is closed",
                    "1:ERR_SOCKET_CLOSED:Socket is closed",
                    "2:ERR_SOCKET_CLOSED:Socket is closed",
                    "close"
                ]
            )
        });
        assert_eq!(socket::get(owner.get(), "bytesWritten"), 0.0);
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}
#[test]
fn destroyed_socket_write_preserves_node_code_and_message() {
    std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        SEEN.with(|seen| seen.borrow_mut().clear());
        let scope = TransientRootScope::enter();
        let owner = scope.root_nanbox(net::new_socket(io::ROUTE, p::undefined()));
        socket::destroy(owner.get(), p::undefined());
        let chunk = scope.root_nanbox(events::string("refused"));
        assert_eq!(
            socket::write(
                owner.get(),
                chunk.get(),
                p::undefined(),
                observer(&scope, 0.0)
            )
            .to_bits(),
            perry_ffi::JsValue::FALSE.bits()
        );
        until(|| SEEN.with(|seen| !seen.borrow().is_empty()));
        SEEN.with(|seen| {
            assert_eq!(
                *seen.borrow(),
                ["0:ERR_STREAM_DESTROYED:Cannot call write after a stream was destroyed"]
            )
        });
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}
#[test]
fn deferred_connect_and_native_submission_reach_each_registered_socket_route() {
    std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        assert!(io::enabled());
        for route in [io::ROUTE, io::TLS_SUBSYSTEM] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let scope = TransientRootScope::enter();
            let owner = scope.root_nanbox(net::new_socket(route, p::undefined()));
            let host = scope.root_nanbox(events::string("localhost"));
            // localhost takes the deferred resolver route, unlike numeric IPs.
            socket::connect(
                owner.get(),
                listener.local_addr().unwrap().port() as f64,
                host.get(),
                p::undefined(),
            );
            until(|| unsafe {
                p::socket_ptr(socket::link(owner.get())).is_ok_and(|payload| (*payload).ext.opened)
            });
            let (mut peer, _) = listener.accept().unwrap();
            peer.set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let rooted = net::RootedSocket::new(owner.get());
            assert!(rooted.write(b"route", 0).unwrap().is_ok());
            until(|| net::queued_bytes(owner.get()) == 0);
            let mut bytes = [0; 5];
            peer.read_exact(&mut bytes).unwrap();
            assert_eq!(&bytes, b"route");
            socket::destroy(owner.get(), p::undefined());
        }
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}
#[test]
fn adopted_upgraded_stream_writes_on_the_callers_loop_and_refuses_a_foreign_cell() {
    std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        assert!(io::enabled());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let stream = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let scope = TransientRootScope::enter();
        let owner = scope.root_nanbox(net::new_socket(io::ROUTE, p::undefined()));
        let link = net::socket_link(owner.get()).unwrap();
        unsafe {
            perry_ffi::turnloop_net::link_adopt_stream(
                &mut *net::core(owner.get()).unwrap(),
                link,
                stream.into(),
            )
            .unwrap();
        }
        net::adopted(owner.get());
        assert!(net::RootedSocket::new(owner.get())
            .write(b"upgrade", 0)
            .unwrap()
            .is_ok());
        until(|| net::queued_bytes(owner.get()) == 0);
        let mut bytes = [0; 7];
        peer.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"upgrade");
        // A cell from this agent cannot be projected on another agent's loop.
        let address = link.raw();
        let refused = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut refused_peer, _) = listener.accept().unwrap();
        std::thread::spawn(move || {
            let foreign = perry_runtime::agent::enter_worker_agent();
            unsafe {
                assert!(
                    p::socket_core(perry_ffi::native_payload::OwnerLink::from_raw(address))
                        .is_err()
                );
            }
            let scope = TransientRootScope::enter();
            let local = scope.root_nanbox(net::new_socket(io::ROUTE, p::undefined()));
            unsafe {
                let foreign_link = perry_ffi::native_payload::OwnerLink::from_raw(address);
                assert!(
                    perry_ffi::turnloop_net::link_adopt_stream(
                        &mut *net::core(local.get()).unwrap(),
                        foreign_link,
                        refused.into()
                    )
                    .is_err(),
                    "adoption must refuse a cell from another loop and consume its stream"
                );
            }
            socket::destroy(local.get(), p::undefined());
            perry_runtime::agent::retire_agent(foreign);
        })
        .join()
        .unwrap();
        refused_peer
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        assert_eq!(
            refused_peer.read(&mut [0; 1]).unwrap(),
            0,
            "refused adoption closes its stream"
        );
        socket::destroy(owner.get(), p::undefined());
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}
#[test]
fn preconnect_backpressure_returns_false_then_one_drain_after_bytes_arrive() {
    std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        SEEN.with(|seen| seen.borrow_mut().clear());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut bytes = vec![0; socket::HIGH_WATER_MARK];
            stream.read_exact(&mut bytes).unwrap();
            assert!(bytes.iter().all(|&byte| byte == b'x'));
        });
        let scope = TransientRootScope::enter();
        let owner = scope.root_nanbox(net::new_socket(io::ROUTE, p::undefined()));
        socket::once(owner.get(), "drain", observer(&scope, 4.0));
        let host = scope.root_nanbox(events::string("127.0.0.1"));
        socket::connect(owner.get(), port as f64, host.get(), p::undefined());
        let chunk = scope.root_nanbox(events::string(&"x".repeat(socket::HIGH_WATER_MARK)));
        assert_eq!(
            socket::write(owner.get(), chunk.get(), p::undefined(), p::undefined()).to_bits(),
            perry_ffi::JsValue::FALSE.bits()
        );
        until(|| SEEN.with(|seen| !seen.borrow().is_empty()));
        until(|| net::queued_bytes(owner.get()) == 0);
        SEEN.with(|seen| assert_eq!(*seen.borrow(), ["drain"]));
        peer.join().unwrap();
        socket::destroy(owner.get(), p::undefined());
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}
unsafe extern "C" fn tls_data(_: *const RawClosureHeader, _: JsThis, chunk: f64) -> f64 {
    let bytes = crate::jsvalue_to_socket_bytes(chunk).unwrap();
    SEEN.with(|seen| seen.borrow_mut().push(String::from_utf8(bytes).unwrap()));
    p::undefined()
}
#[test]
fn tls_roundtrip_through_a_rooted_wrapper_and_its_native_parent() {
    std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        SEEN.with(|seen| seen.borrow_mut().clear());
        const CERT: &[u8] =
            include_bytes!("../../../test-parity/node-suite/tls/fixtures/localhost-cert.pem");
        const KEY: &[u8] =
            include_bytes!("../../../test-parity/node-suite/tls/fixtures/localhost-key.pem");
        let certs = rustls_pemfile::certs(&mut std::io::Cursor::new(CERT))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let key = rustls_pemfile::private_key(&mut std::io::Cursor::new(KEY))
            .unwrap()
            .unwrap();
        let config = std::sync::Arc::new(
            rustls::ServerConfig::builder_with_provider(std::sync::Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .unwrap(),
        );
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut tls =
                rustls::StreamOwned::new(rustls::ServerConnection::new(config).unwrap(), stream);
            let mut bytes = [0; 9];
            tls.read_exact(&mut bytes).unwrap();
            assert_eq!(&bytes, b"encrypted");
            tls.write_all(&bytes).unwrap();
            tls.flush().unwrap();
            let mut public = [0; 9];
            tls.read_exact(&mut public).unwrap();
            assert_eq!(&public, b"publictls");
            tls.write_all(&public).unwrap();
            tls.flush().unwrap();
            // Keep the peer alive until the binding has observed the plaintext.
            let mut rest = Vec::new();
            let _ = tls.read_to_end(&mut rest);
        });
        let scope = TransientRootScope::enter();
        let parent = scope.root_nanbox(net::new_socket(io::ROUTE, p::undefined()));
        let host = scope.root_nanbox(events::string("localhost"));
        socket::connect(parent.get(), port as f64, host.get(), p::undefined());
        let wrapper = scope.root_nanbox(socket::new_tls_wrapper(parent.get()));
        super::payload_tls::install_client(
            wrapper.get(),
            "localhost".into(),
            false,
            crate::TlsClientConfigData::default(),
        )
        .unwrap();
        assert!(
            net::snapshot(wrapper.get()).is_some(),
            "DNS owns a resolve generation"
        );
        // SAFETY: The parent is rooted on this worker; this test-only read is
        // callback-free and proves the resolver subject has not been pumped.
        assert!(unsafe {
            perry_runtime::turnloop_net::transport::pending_resolve_for_test(
                net::socket_link(parent.get()).unwrap().raw(),
            )
            .is_some()
        });
        let callback = scope.root_addr(perry_ffi::alloc_closure(
            perry_ffi::js_function_info!(tls_data, 1; with_flags(perry_ffi::FN_BUILTIN)),
            0,
        ) as i64);
        socket::once(wrapper.get(), "data", p::boxed_addr(callback.get()));
        assert!(net::RootedSocket::new(wrapper.get())
            .write(b"encrypted", 0)
            .unwrap()
            .is_ok());
        let empty = scope.root_nanbox(events::string(""));
        assert_eq!(
            socket::write(wrapper.get(), empty.get(), p::undefined(), p::undefined()).to_bits(),
            perry_ffi::JsValue::TRUE.bits(),
            "a TLS write below the high water mark stays true while DNS is pending"
        );
        until(|| SEEN.with(|seen| !seen.borrow().is_empty()));
        SEEN.with(|seen| assert_eq!(*seen.borrow(), ["encrypted"]));
        socket::once(wrapper.get(), "data", p::boxed_addr(callback.get()));
        let chunk = scope.root_nanbox(events::string("publictls"));
        assert_eq!(
            socket::write(wrapper.get(), chunk.get(), p::undefined(), p::undefined()).to_bits(),
            perry_ffi::JsValue::TRUE.bits(),
            "public write must select the same TLS parent"
        );
        until(|| SEEN.with(|seen| seen.borrow().len() == 2));
        SEEN.with(|seen| assert_eq!(*seen.borrow(), ["encrypted", "publictls"]));
        socket::destroy(wrapper.get(), p::undefined());
        peer.join().unwrap();
        perry_runtime::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn worker_retirement_releases_live_sockets_without_dispatching_their_js() {
    fn fds() -> usize {
        std::fs::read_dir("/proc/self/fd").unwrap().count()
    }
    let baseline = fds();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let worker = std::thread::spawn(move || {
        let agent = perry_runtime::agent::enter_worker_agent();
        SEEN.with(|seen| seen.borrow_mut().clear());
        let scope = TransientRootScope::enter();
        let owner = scope.root_nanbox(net::new_socket(io::ROUTE, p::undefined()));
        socket::once(owner.get(), "close", observer(&scope, 3.0));
        let host = scope.root_nanbox(events::string("127.0.0.1"));
        socket::connect(owner.get(), port as f64, host.get(), p::undefined());
        until(|| {
            !perry_ffi::JsValue::from_bits(socket::get(owner.get(), "connecting").to_bits())
                .to_bool()
        });
        assert!(
            net::snapshot(owner.get()).is_some(),
            "the Worker must retire with a live driver"
        );
        assert!(
            fds() >= baseline + 2,
            "the Worker must own real descriptors"
        );
        perry_runtime::agent::retire_agent(agent);
        assert_eq!(fds(), baseline + 1,
            "retirement must release its live socket and loop before the Worker returns; only the listener remains");
        SEEN.with(|seen| assert!(seen.borrow().is_empty(), "retirement cannot call JS"));
    });
    worker.join().unwrap();
    let (mut peer, _) = listener.accept().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    assert_eq!(
        peer.read(&mut [0; 1]).unwrap(),
        0,
        "retirement closes the live peer"
    );
    drop(peer);
    drop(listener);
    assert_eq!(
        fds(),
        baseline,
        "live Worker sockets and loop descriptors cannot leak"
    );
}
