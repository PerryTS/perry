use super::*;
use crate::WasiCtx;
use futures_lite::future::block_on;
use std::{
    io::{Read, Write},
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};

const WAIT: Duration = Duration::from_secs(10);
fn bounded(test: impl FnOnce() + Send + 'static) {
    let (send, recv) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        test();
        send.send(()).unwrap();
    });
    recv.recv_timeout(WAIT)
        .expect("TCP provider test did not finish");
    worker.join().unwrap();
}
fn context() -> WasiCtx {
    let mut ctx = WasiCtx::new(16).unwrap();
    ctx.sockets
        .allow_tcp(true)
        .socket_addr_check(|_, _| Box::pin(async { true }));
    ctx
}
fn ok<T>(request: ResourceRequest<Result<T, Failure>>) -> T {
    block_on(request).unwrap().unwrap()
}
fn error<T>(result: SocketResult<T>) -> ErrorCode {
    match result {
        Err(e) => e.downcast().unwrap(),
        Ok(_) => panic!("expected a socket error"),
    }
}
fn peer_config(peer: &std::net::TcpStream) {
    peer.set_read_timeout(Some(WAIT)).unwrap();
    peer.set_write_timeout(Some(WAIT)).unwrap();
}

#[test]
fn tcp_capabilities_and_address_validation_precede_native_operations() {
    bounded(|| {
        let mut ctx = WasiCtx::new(8).unwrap();
        assert_eq!(
            error(TcpSocket::new(&ctx.sockets, Family::Ipv4)),
            ErrorCode::AccessDenied
        );
        ctx.sockets.allow_tcp(true);
        let mut socket = TcpSocket::new(&ctx.sockets, Family::Ipv4).unwrap();
        assert_eq!(
            error(block_on(socket.bind("127.0.0.1:0".parse().unwrap()))),
            ErrorCode::AccessDenied
        );
        assert!(!socket.is_bound());
        assert_eq!(error(socket.local_address()), ErrorCode::InvalidState);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = calls.clone();
        let owner = std::thread::current().id();
        ctx.sockets.socket_addr_check(move |addr, usage| {
            assert_eq!(
                std::thread::current().id(),
                owner,
                "permission callback crossed to native I/O thread"
            );
            seen.lock().unwrap().push((addr, usage));
            Box::pin(async { true })
        });
        let mut socket = TcpSocket::new(&ctx.sockets, Family::Ipv4).unwrap();
        for addr in [
            "[::1]:80",
            "0.0.0.0:80",
            "127.0.0.1:0",
            "224.0.0.1:80",
            "255.255.255.255:80",
        ] {
            assert_eq!(
                error(block_on(socket.connect(addr.parse().unwrap()))),
                ErrorCode::InvalidArgument
            );
        }
        assert!(calls.lock().unwrap().is_empty());
        assert_eq!(error(block_on(socket.listen())), ErrorCode::InvalidState);
        block_on(socket.bind("127.0.0.1:0".parse().unwrap())).unwrap();
        assert_ne!(socket.local_address().unwrap().port(), 0);
        assert_eq!(
            error(block_on(socket.bind("127.0.0.1:0".parse().unwrap()))),
            ErrorCode::InvalidState
        );
        assert_eq!(calls.lock().unwrap().len(), 1);
        let mut v6 = TcpSocket::new(&ctx.sockets, Family::Ipv6).unwrap();
        assert_eq!(
            error(block_on(v6.bind("[::ffff:127.0.0.1]:0".parse().unwrap()))),
            ErrorCode::InvalidArgument
        );
    });
}

#[test]
fn tcp_bound_connect_preserves_port_options_and_duplex_half_close() {
    bounded(|| {
        for family in [Family::Ipv4, Family::Ipv6] {
            let addr = if family == Family::Ipv4 {
                "127.0.0.1:0"
            } else {
                "[::1]:0"
            };
            let listener = std::net::TcpListener::bind(addr).unwrap();
            let remote = listener.local_addr().unwrap();
            let worker = std::thread::spawn(move || {
                let (mut peer, address) = listener.accept().unwrap();
                peer_config(&peer);
                let mut bytes = Vec::new();
                peer.read_to_end(&mut bytes).unwrap();
                assert_eq!(bytes, b"bound request");
                peer.write_all(b"response after FIN").unwrap();
                address
            });
            let ctx = context();
            let mut socket = TcpSocket::new(&ctx.sockets, family).unwrap();
            socket.set_keep_alive_idle_time(7_000_000_000).unwrap();
            socket.set_keep_alive_interval(3_000_000_000).unwrap();
            socket.set_keep_alive_count(4).unwrap();
            socket.set_keep_alive_enabled(false).unwrap();
            socket.set_hop_limit(42).unwrap();
            block_on(socket.bind(addr.parse().unwrap())).unwrap();
            let local = socket.local_address().unwrap();
            block_on(socket.connect(remote)).unwrap();
            assert_eq!(socket.local_address().unwrap(), local);
            assert_eq!(socket.remote_address().unwrap(), remote);
            assert_eq!(socket.keep_alive_idle_time().unwrap(), 7_000_000_000);
            assert_eq!(socket.keep_alive_interval().unwrap(), 3_000_000_000);
            assert_eq!(socket.keep_alive_count().unwrap(), 4);
            assert!(!socket.keep_alive_enabled().unwrap());
            assert_eq!(socket.hop_limit().unwrap(), 42);
            socket.set_keep_alive_idle_time(9_000_000_000).unwrap();
            assert!(!socket.keep_alive_enabled().unwrap());
            ok(socket.write(b"bound request".to_vec()).unwrap());
            block_on(socket.shutdown_send()).unwrap();
            block_on(socket.shutdown_send()).unwrap();
            let mut received = Vec::new();
            loop {
                let bytes = ok(socket.read(3).unwrap());
                if bytes.is_empty() {
                    break;
                }
                received.extend(bytes);
            }
            assert_eq!(received, b"response after FIN");
            assert_eq!(worker.join().unwrap(), local);
            assert_eq!(error(block_on(socket.bind(local))), ErrorCode::InvalidState);
            assert_eq!(
                error(socket.set_listen_backlog_size(5)),
                ErrorCode::InvalidState
            );
        }
    });
}

#[test]
fn tcp_implicit_connect_checks_both_addresses_on_calling_thread() {
    bounded(|| {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let remote = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let (peer, _) = listener.accept().unwrap();
            peer_config(&peer);
        });
        let mut ctx = context();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = calls.clone();
        let owner = std::thread::current().id();
        ctx.sockets.socket_addr_check(move |addr, usage| {
            assert_eq!(std::thread::current().id(), owner);
            seen.lock().unwrap().push((addr, usage));
            Box::pin(async { true })
        });
        let mut socket = TcpSocket::new(&ctx.sockets, Family::Ipv4).unwrap();
        block_on(socket.connect(remote)).unwrap();
        assert_eq!(
            *calls.lock().unwrap(),
            [
                (remote, SocketAddrUse::TcpConnect),
                ("0.0.0.0:0".parse().unwrap(), SocketAddrUse::TcpBind)
            ]
        );
        worker.join().unwrap();
        assert_eq!(
            error(block_on(socket.connect(remote))),
            ErrorCode::InvalidState
        );
    });
}

#[test]
fn tcp_accept_inherits_options_and_outlives_listener() {
    bounded(|| {
        for family in [Family::Ipv4, Family::Ipv6] {
            let ctx = context();
            let mut listener = TcpSocket::new(&ctx.sockets, family).unwrap();
            listener.set_keep_alive_idle_time(5_000_000_000).unwrap();
            listener.set_keep_alive_interval(2_000_000_000).unwrap();
            listener.set_keep_alive_count(3).unwrap();
            listener.set_keep_alive_enabled(false).unwrap();
            listener.set_hop_limit(39).unwrap();
            listener.set_receive_buffer_size(32768).unwrap();
            listener.set_send_buffer_size(32768).unwrap();
            listener.set_listen_backlog_size(3).unwrap();
            block_on(
                listener.bind(
                    if family == Family::Ipv4 {
                        "127.0.0.1:0"
                    } else {
                        "[::1]:0"
                    }
                    .parse()
                    .unwrap(),
                ),
            )
            .unwrap();
            block_on(listener.listen()).unwrap();
            let address = listener.local_address().unwrap();
            listener.set_listen_backlog_size(9).unwrap();
            assert_eq!(listener.local_address().unwrap(), address);
            let worker = std::thread::spawn(move || {
                let mut peer = std::net::TcpStream::connect(address).unwrap();
                peer_config(&peer);
                peer.write_all(b"ping").unwrap();
                let mut received = Vec::new();
                peer.read_to_end(&mut received).unwrap();
                assert_eq!(received, b"pong");
            });
            let child = block_on(listener.accept()).unwrap();
            assert_eq!(child.local_address().unwrap(), address);
            assert!(child.remote_address().unwrap().ip().is_loopback());
            assert_eq!(child.keep_alive_idle_time().unwrap(), 5_000_000_000);
            assert_eq!(child.keep_alive_interval().unwrap(), 2_000_000_000);
            assert_eq!(child.keep_alive_count().unwrap(), 3);
            assert!(!child.keep_alive_enabled().unwrap());
            assert_eq!(child.hop_limit().unwrap(), 39);
            assert_eq!(
                child.receive_buffer_size().unwrap(),
                listener.receive_buffer_size().unwrap()
            );
            assert_eq!(
                child.send_buffer_size().unwrap(),
                listener.send_buffer_size().unwrap()
            );
            drop(listener);
            let mut bytes = Vec::new();
            while bytes.len() < 4 {
                bytes.extend(ok(child.read(4 - bytes.len()).unwrap()));
            }
            assert_eq!(bytes, b"ping");
            ok(child.write(b"pong".to_vec()).unwrap());
            block_on(child.shutdown_send()).unwrap();
            worker.join().unwrap();
        }
    });
}

#[test]
fn tcp_denied_peer_is_closed_and_accept_retries() {
    bounded(|| {
        let mut ctx = context();
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = count.clone();
        let owner = std::thread::current().id();
        ctx.sockets.socket_addr_check(move |_, usage| {
            assert_eq!(std::thread::current().id(), owner);
            let allow = usage != SocketAddrUse::TcpAccept
                || seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst) != 0;
            Box::pin(async move { allow })
        });
        let mut listener = TcpSocket::new(&ctx.sockets, Family::Ipv4).unwrap();
        block_on(listener.bind("127.0.0.1:0".parse().unwrap())).unwrap();
        block_on(listener.listen()).unwrap();
        let address = listener.local_address().unwrap();
        let worker = std::thread::spawn(move || {
            let mut denied = std::net::TcpStream::connect(address).unwrap();
            peer_config(&denied);
            assert_eq!(denied.read(&mut [0]).unwrap(), 0);
            let mut allowed = std::net::TcpStream::connect(address).unwrap();
            peer_config(&allowed);
            allowed.write_all(b"allowed").unwrap();
        });
        let child = block_on(listener.accept()).unwrap();
        let mut bytes = Vec::new();
        loop {
            let part = ok(child.read(32).unwrap());
            if part.is_empty() {
                break;
            }
            bytes.extend(part);
        }
        assert_eq!(bytes, b"allowed");
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 2);
        worker.join().unwrap();
    });
}

#[test]
fn tcp_option_validation_and_native_failures_are_typed() {
    bounded(|| {
        let ctx = context();
        let mut socket = TcpSocket::new(&ctx.sockets, Family::Ipv4).unwrap();
        let before = (
            socket.keep_alive_idle_time().unwrap(),
            socket.keep_alive_interval().unwrap(),
            socket.keep_alive_count().unwrap(),
        );
        for failure in [
            socket.set_keep_alive_idle_time(0),
            socket.set_keep_alive_interval(0),
            socket.set_keep_alive_count(0),
            socket.set_hop_limit(0),
            socket.set_receive_buffer_size(0),
            socket.set_send_buffer_size(0),
            socket.set_listen_backlog_size(0),
        ] {
            assert_eq!(error(failure), ErrorCode::InvalidArgument);
        }
        assert_eq!(
            before,
            (
                socket.keep_alive_idle_time().unwrap(),
                socket.keep_alive_interval().unwrap(),
                socket.keep_alive_count().unwrap()
            )
        );
        socket.set_keep_alive_idle_time(1).unwrap();
        assert_eq!(socket.keep_alive_idle_time().unwrap(), 1_000_000_000);
        assert!(!socket.keep_alive_enabled().unwrap());
        let reserved = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let remote = reserved.local_addr().unwrap();
        drop(reserved);
        assert_eq!(
            error(block_on(socket.connect(remote))),
            ErrorCode::ConnectionRefused
        );
        assert_eq!(error(socket.local_address()), ErrorCode::InvalidState);
        assert_eq!(error(socket.hop_limit()), ErrorCode::InvalidState);
    });
}

#[test]
fn tcp_pending_accept_cancels_without_retaining_listener_or_capacity() {
    bounded(|| {
        let ctx = context();
        let mut listener = TcpSocket::new(&ctx.sockets, Family::Ipv4).unwrap();
        block_on(listener.bind("127.0.0.1:0".parse().unwrap())).unwrap();
        block_on(listener.listen()).unwrap();
        for _ in 0..32 {
            let mut pending = Box::pin(listener.accept());
            use std::future::Future;
            assert!(pending
                .as_mut()
                .poll(&mut std::task::Context::from_waker(
                    futures_util::task::noop_waker_ref()
                ))
                .is_pending());
            drop(pending);
            // A driver barrier acknowledges queued cancellation before the next
            // attempt; this tests reclamation rather than relying on timing.
            block_on(ctx.sockets.submitter.submit(|_| async {}).unwrap()).unwrap();
        }
        assert_eq!(ctx.outstanding_io(), 0);
        assert_eq!(ctx.driver.resources(), 1);
        drop(listener);
        drop(ctx);
    });
}

#[test]
fn tcp_read_cancellation_retains_bytes_and_rejects_concurrent_readers() {
    bounded(|| {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (send, recv) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let (mut peer, _) = listener.accept().unwrap();
            peer_config(&peer);
            recv.recv_timeout(WAIT).unwrap();
            peer.write_all(b"retained").unwrap();
        });
        let ctx = context();
        let mut socket = TcpSocket::new(&ctx.sockets, Family::Ipv4).unwrap();
        block_on(socket.connect(address)).unwrap();
        let read = socket.read(8).unwrap();
        block_on(ctx.sockets.submitter.submit(|_| async {}).unwrap()).unwrap();
        assert!(matches!(
            block_on(socket.read(8).unwrap()).unwrap(),
            Err(Failure::Socket(ErrorCode::ConcurrencyConflict))
        ));
        read.cancel();
        assert!(matches!(
            block_on(read),
            Err(perry_component_io::Error::Cancelled)
        ));
        send.send(()).unwrap();
        let mut bytes = Vec::new();
        loop {
            let part = ok(socket.read(8).unwrap());
            if part.is_empty() {
                break;
            }
            bytes.extend(part);
        }
        assert_eq!(bytes, b"retained");
        worker.join().unwrap();
        drop(ctx);
        assert!(
            socket.read(1).err().unwrap().downcast().is_err(),
            "driver loss must trap, not become a guest socket error"
        );
    });
}

#[test]
fn tcp_fractional_keepalive_and_partial_updates_survive_accept() {
    bounded(|| {
        let ctx = context();
        let mut listener = TcpSocket::new(&ctx.sockets, Family::Ipv4).unwrap();
        listener.set_keep_alive_idle_time(1_500_000_000).unwrap();
        listener.set_keep_alive_interval(2_500_000_000).unwrap();
        listener.set_keep_alive_count(5).unwrap();
        listener.set_keep_alive_idle_time(3_500_000_000).unwrap();
        assert_eq!(listener.keep_alive_idle_time().unwrap(), 4_000_000_000);
        assert_eq!(listener.keep_alive_interval().unwrap(), 3_000_000_000);
        assert!(!listener.keep_alive_enabled().unwrap());
        assert_eq!(
            listener.inherited.len(),
            3,
            "partial updates must not grow the override list"
        );
        block_on(listener.bind("127.0.0.1:0".parse().unwrap())).unwrap();
        block_on(listener.listen()).unwrap();
        let peer = std::net::TcpStream::connect(listener.local_address().unwrap()).unwrap();
        peer_config(&peer);
        let child = block_on(listener.accept()).unwrap();
        assert_eq!(
            child.keep_alive_idle_time().unwrap(),
            listener.keep_alive_idle_time().unwrap()
        );
        assert_eq!(
            child.keep_alive_interval().unwrap(),
            listener.keep_alive_interval().unwrap()
        );
        assert_eq!(child.keep_alive_count().unwrap(), 5);
        assert!(!child.keep_alive_enabled().unwrap());
    });
}

#[test]
fn tcp_receive_shutdown_discards_pending_read_and_preserves_send() {
    bounded(|| {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let (mut peer, _) = listener.accept().unwrap();
            peer_config(&peer);
            let mut bytes = Vec::new();
            peer.read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, b"send after receive shutdown");
        });
        let ctx = context();
        let mut socket = TcpSocket::new(&ctx.sockets, Family::Ipv4).unwrap();
        block_on(socket.connect(address)).unwrap();
        let read = socket.read(64).unwrap();
        block_on(ctx.sockets.submitter.submit(|_| async {}).unwrap()).unwrap();
        assert_eq!(
            error(block_on(socket.shutdown_receive())),
            ErrorCode::ConcurrencyConflict
        );
        read.cancel();
        assert!(matches!(
            block_on(read),
            Err(perry_component_io::Error::Cancelled)
        ));
        for _ in 0..3 {
            block_on(socket.shutdown_receive()).unwrap();
            assert!(
                ok(socket.read(64).unwrap()).is_empty(),
                "shutdown must not resubmit a native read"
            );
        }
        ok(socket
            .write(b"send after receive shutdown".to_vec())
            .unwrap());
        block_on(socket.shutdown_send()).unwrap();
        worker.join().unwrap();
        let fresh = TcpSocket::new(&ctx.sockets, Family::Ipv4).unwrap();
        assert_eq!(
            error(block_on(fresh.shutdown_receive())),
            ErrorCode::InvalidState
        );
    });
}

#[test]
fn tcp_pending_accept_owns_listener_and_uses_current_options() {
    bounded(|| {
        let ctx = context();
        let mut listener = TcpSocket::new(&ctx.sockets, Family::Ipv4).unwrap();
        listener.set_keep_alive_idle_time(2_000_000_000).unwrap();
        listener.set_hop_limit(30).unwrap();
        block_on(listener.bind("127.0.0.1:0".parse().unwrap())).unwrap();
        block_on(listener.listen()).unwrap();
        let address = listener.local_address().unwrap();
        let mut accepting = Box::pin(listener.accept());
        use std::future::Future;
        assert!(accepting
            .as_mut()
            .poll(&mut std::task::Context::from_waker(
                futures_util::task::noop_waker_ref()
            ))
            .is_pending());
        block_on(ctx.sockets.submitter.submit(|_| async {}).unwrap()).unwrap();
        listener.set_keep_alive_idle_time(9_000_000_000).unwrap();
        listener.set_hop_limit(44).unwrap();
        listener.set_listen_backlog_size(11).unwrap();
        drop(listener);
        let mut peer = std::net::TcpStream::connect(address).unwrap();
        peer_config(&peer);
        let child = block_on(accepting).unwrap();
        assert_eq!(child.keep_alive_idle_time().unwrap(), 9_000_000_000);
        assert_eq!(child.hop_limit().unwrap(), 44);
        block_on(ctx.sockets.submitter.submit(|_| async {}).unwrap()).unwrap();
        assert_eq!(
            ctx.driver.resources(),
            1,
            "accept must release its listener reference"
        );
        ok(child.write(b"independent".to_vec()).unwrap());
        block_on(child.shutdown_send()).unwrap();
        let mut bytes = Vec::new();
        peer.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"independent");
    });
}
