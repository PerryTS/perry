use super::*;
use crate::{
    io::sync::wasi::io::{
        poll::HostPollable,
        streams::{HostInputStream, HostOutputStream},
    },
    p2::bindings::{
        sockets::{instance_network, network, tcp_create_socket},
        sync::sockets::tcp::{HostTcpSocket, ShutdownType},
    },
    sockets::WasiSocketsCtxView,
    WasiCtx, WasiCtxView, WasiView,
};
use std::{
    io::{Read, Write},
    sync::mpsc,
    time::Duration,
};
use wasmtime::component::{Resource, ResourceTable};
use wasmtime_wasi_io::streams::StreamError;
const WAIT: Duration = Duration::from_secs(10);
fn bounded(test: impl FnOnce() + Send + 'static) {
    let (send, recv) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        test();
        send.send(()).unwrap();
    });
    recv.recv_timeout(WAIT).expect("TCP WIT test timed out");
    thread.join().unwrap();
}
fn ctx() -> WasiCtx {
    ctx_with_limit(16)
}
fn ctx_with_limit(limit: usize) -> WasiCtx {
    let mut ctx = WasiCtx::new(limit).unwrap();
    ctx.sockets
        .allow_tcp(true)
        .socket_addr_check(|_, _| Box::pin(async { true }));
    ctx
}
fn view<'a>(ctx: &'a mut WasiCtx, table: &'a mut ResourceTable) -> WasiSocketsCtxView<'a> {
    WasiSocketsCtxView {
        ctx: &mut ctx.sockets,
        table,
    }
}
fn borrow<T: 'static>(resource: &Resource<T>) -> Resource<T> {
    Resource::new_borrow(resource.rep())
}
fn code<T>(result: SocketResult<T>) -> ErrorCode {
    match result {
        Err(e) => e.downcast().unwrap(),
        Ok(_) => panic!("expected socket error"),
    }
}
fn make(
    ctx: &mut WasiCtx,
    table: &mut ResourceTable,
    family: network::IpAddressFamily,
) -> (Resource<network::Network>, Resource<TcpSocket>) {
    let mut v = view(ctx, table);
    let network = instance_network::Host::instance_network(&mut v).unwrap();
    let socket = tcp_create_socket::Host::create_tcp_socket(&mut v, family).unwrap();
    (network, socket)
}
fn block_socket(ctx: &mut WasiCtx, table: &mut ResourceTable, socket: &Resource<TcpSocket>) {
    let poll = HostTcpSocket::subscribe(&mut view(ctx, table), borrow(socket)).unwrap();
    HostPollable::block(table, borrow(&poll)).unwrap();
    HostPollable::drop(table, poll).unwrap();
}
fn peer_config(peer: &std::net::TcpStream) {
    peer.set_read_timeout(Some(WAIT)).unwrap();
    peer.set_write_timeout(Some(WAIT)).unwrap();
}

#[test]
fn tcp_wit_capabilities_network_identity_and_start_finish_states() {
    bounded(|| {
        let mut ctx = WasiCtx::new(16).unwrap();
        let mut table = ResourceTable::new();
        assert_eq!(
            code(tcp_create_socket::Host::create_tcp_socket(
                &mut view(&mut ctx, &mut table),
                network::IpAddressFamily::Ipv4
            )),
            ErrorCode::AccessDenied
        );
        ctx.sockets
            .allow_tcp(true)
            .socket_addr_check(|_, _| Box::pin(async { true }));
        let (network, socket) = make(&mut ctx, &mut table, network::IpAddressFamily::Ipv4);
        let alias =
            instance_network::Host::instance_network(&mut view(&mut ctx, &mut table)).unwrap();
        let mut other = super::tests::ctx();
        let foreign =
            instance_network::Host::instance_network(&mut view(&mut other, &mut table)).unwrap();
        let address = "127.0.0.1:0".parse::<SocketAddr>().unwrap().into();
        let mut v = view(&mut ctx, &mut table);
        assert!(v
            .start_bind(borrow(&socket), Resource::new_borrow(u32::MAX), address)
            .unwrap_err()
            .downcast()
            .is_err());
        assert_eq!(
            code(v.start_bind(borrow(&socket), borrow(&foreign), address)),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(v.finish_bind(borrow(&socket))),
            ErrorCode::NotInProgress
        );
        assert_eq!(
            code(v.finish_connect(borrow(&socket))),
            ErrorCode::NotInProgress
        );
        assert_eq!(
            code(v.start_listen(borrow(&socket))),
            ErrorCode::InvalidState
        );
        v.start_bind(borrow(&socket), borrow(&alias), address)
            .unwrap();
        assert_eq!(
            code(v.start_listen(borrow(&socket))),
            ErrorCode::ConcurrencyConflict
        );
        v.finish_bind(borrow(&socket)).unwrap();
        assert_eq!(
            code(v.finish_bind(borrow(&socket))),
            ErrorCode::NotInProgress
        );
        v.start_listen(borrow(&socket)).unwrap();
        assert!(v.is_listening(borrow(&socket)).unwrap());
        assert_eq!(
            code(v.accept(borrow(&socket))),
            ErrorCode::ConcurrencyConflict
        );
        v.finish_listen(borrow(&socket)).unwrap();
        assert_eq!(code(v.accept(borrow(&socket))), ErrorCode::WouldBlock);
        HostTcpSocket::drop(&mut v, socket).unwrap();
        for item in [network, alias, foreign] {
            network::HostNetwork::drop(&mut v, item).unwrap();
        }
        assert!(table.is_empty());
    });
}

#[test]
fn tcp_wit_connected_streams_send_fin_without_output_polling() {
    bounded(|| {
        for family in [
            network::IpAddressFamily::Ipv4,
            network::IpAddressFamily::Ipv6,
        ] {
            let addr = if family == network::IpAddressFamily::Ipv4 {
                "127.0.0.1:0"
            } else {
                "[::1]:0"
            };
            let listener = std::net::TcpListener::bind(addr).unwrap();
            let address = listener.local_addr().unwrap();
            let peer = std::thread::spawn(move || {
                let (mut peer, _) = listener.accept().unwrap();
                peer_config(&peer);
                let mut bytes = Vec::new();
                peer.read_to_end(&mut bytes).unwrap();
                assert_eq!(bytes, vec![73; 64 * 1024]);
                peer.write_all(b"reply after FIN").unwrap();
            });
            let mut ctx = ctx();
            let mut table = ResourceTable::new();
            let (network, socket) = make(&mut ctx, &mut table, family);
            view(&mut ctx, &mut table)
                .start_connect(borrow(&socket), borrow(&network), address.into())
                .unwrap();
            block_socket(&mut ctx, &mut table, &socket);
            let (input, output) = view(&mut ctx, &mut table)
                .finish_connect(borrow(&socket))
                .unwrap();
            assert_eq!(
                code(view(&mut ctx, &mut table).finish_connect(borrow(&socket))),
                ErrorCode::NotInProgress
            );
            assert!(
                HostTcpSocket::drop(
                    &mut view(&mut ctx, &mut table),
                    Resource::new_own(socket.rep())
                )
                .is_err(),
                "live streams must retain parent"
            );
            assert!(matches!(
                HostOutputStream::write(&mut table, borrow(&output), vec![1]),
                Err(StreamError::Trap(_))
            ));
            assert_eq!(
                HostOutputStream::check_write(&mut table, borrow(&output)).unwrap(),
                64 * 1024
            );
            assert!(matches!(
                HostOutputStream::write(&mut table, borrow(&output), vec![0; 64 * 1024 + 1]),
                Err(StreamError::Trap(_))
            ));
            HostOutputStream::write(&mut table, borrow(&output), vec![73; 64 * 1024]).unwrap();
            view(&mut ctx, &mut table)
                .shutdown(borrow(&socket), ShutdownType::Send)
                .unwrap();
            let mut bytes = Vec::new();
            loop {
                match HostInputStream::blocking_read(&mut table, borrow(&input), 3) {
                    Ok(part) => bytes.extend(part),
                    Err(StreamError::Closed) => break,
                    other => panic!("{other:?}"),
                }
            }
            assert_eq!(bytes, b"reply after FIN");
            peer.join().unwrap();
            HostOutputStream::drop(&mut table, output).unwrap();
            HostInputStream::drop(&mut table, input).unwrap();
            HostTcpSocket::drop(&mut view(&mut ctx, &mut table), socket).unwrap();
            network::HostNetwork::drop(&mut view(&mut ctx, &mut table), network).unwrap();
            assert!(table.is_empty());
        }
    });
}

#[test]
fn tcp_wit_accept_pollables_options_and_stream_drop_lifetimes() {
    bounded(|| {
        let mut ctx = ctx();
        let mut table = ResourceTable::new();
        let (network, listener) = make(&mut ctx, &mut table, network::IpAddressFamily::Ipv4);
        let mut v = view(&mut ctx, &mut table);
        v.start_bind(
            borrow(&listener),
            borrow(&network),
            "127.0.0.1:0".parse::<SocketAddr>().unwrap().into(),
        )
        .unwrap();
        v.finish_bind(borrow(&listener)).unwrap();
        v.start_listen(borrow(&listener)).unwrap();
        v.finish_listen(borrow(&listener)).unwrap();
        let address: SocketAddr = v.local_address(borrow(&listener)).unwrap().into();
        let poll = v.subscribe(borrow(&listener)).unwrap();
        assert!(!HostPollable::ready(&mut table, borrow(&poll)).unwrap());
        view(&mut ctx, &mut table)
            .set_keep_alive_idle_time(borrow(&listener), 9_000_000_000)
            .unwrap();
        let peer = std::thread::spawn(move || {
            let mut peer = std::net::TcpStream::connect(address).unwrap();
            peer_config(&peer);
            let mut bytes = Vec::new();
            peer.read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, b"stream drop FIN");
            peer.write_all(b"input remains open").unwrap();
        });
        HostPollable::block(&mut table, borrow(&poll)).unwrap();
        let (child, input, output) = view(&mut ctx, &mut table)
            .accept(borrow(&listener))
            .unwrap();
        assert_eq!(
            view(&mut ctx, &mut table)
                .keep_alive_idle_time(borrow(&child))
                .unwrap(),
            9_000_000_000
        );
        assert!(HostTcpSocket::drop(
            &mut view(&mut ctx, &mut table),
            Resource::new_own(listener.rep())
        )
        .is_err());
        HostPollable::drop(&mut table, poll).unwrap();
        HostTcpSocket::drop(&mut view(&mut ctx, &mut table), listener).unwrap();
        HostOutputStream::check_write(&mut table, borrow(&output)).unwrap();
        HostOutputStream::write(&mut table, borrow(&output), b"stream drop FIN".to_vec()).unwrap();
        HostOutputStream::drop(&mut table, output).unwrap();
        let mut bytes = Vec::new();
        loop {
            match HostInputStream::blocking_read(&mut table, borrow(&input), 64) {
                Ok(part) => bytes.extend(part),
                Err(StreamError::Closed) => break,
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(bytes, b"input remains open");
        peer.join().unwrap();
        HostInputStream::drop(&mut table, input).unwrap();
        HostTcpSocket::drop(&mut view(&mut ctx, &mut table), child).unwrap();
        network::HostNetwork::drop(&mut view(&mut ctx, &mut table), network).unwrap();
        assert!(table.is_empty());
    });
}

#[test]
fn tcp_wit_receive_shutdown_closes_pending_input_and_preserves_output() {
    bounded(|| {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let peer = std::thread::spawn(move || {
            let (mut peer, _) = listener.accept().unwrap();
            peer_config(&peer);
            let mut bytes = Vec::new();
            peer.read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, b"still writable");
        });
        let mut ctx = ctx_with_limit(1);
        let mut table = ResourceTable::new();
        let (network, socket) = make(&mut ctx, &mut table, network::IpAddressFamily::Ipv4);
        view(&mut ctx, &mut table)
            .start_connect(borrow(&socket), borrow(&network), address.into())
            .unwrap();
        block_socket(&mut ctx, &mut table, &socket);
        let (input, output) = view(&mut ctx, &mut table)
            .finish_connect(borrow(&socket))
            .unwrap();
        let poll = HostInputStream::subscribe(&mut table, borrow(&input)).unwrap();
        assert!(!HostPollable::ready(&mut table, borrow(&poll)).unwrap());
        for _ in 0..2 {
            view(&mut ctx, &mut table)
                .shutdown(borrow(&socket), ShutdownType::Receive)
                .unwrap();
        }
        assert!(matches!(
            HostInputStream::read(&mut table, borrow(&input), 1),
            Err(StreamError::Closed)
        ));
        HostPollable::block(&mut table, borrow(&poll)).unwrap();
        HostPollable::drop(&mut table, poll).unwrap();
        HostOutputStream::blocking_write_and_flush(
            &mut table,
            borrow(&output),
            b"still writable".to_vec(),
        )
        .unwrap();
        view(&mut ctx, &mut table)
            .shutdown(borrow(&socket), ShutdownType::Both)
            .unwrap();
        peer.join().unwrap();
        HostInputStream::drop(&mut table, input).unwrap();
        HostOutputStream::drop(&mut table, output).unwrap();
        HostTcpSocket::drop(&mut view(&mut ctx, &mut table), socket).unwrap();
        network::HostNetwork::drop(&mut view(&mut ctx, &mut table), network).unwrap();
        assert!(table.is_empty());
    });
}

#[test]
fn tcp_wit_failed_connection_closes_socket_and_projects_network_errors() {
    bounded(|| {
        let reserved = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reserved.local_addr().unwrap();
        drop(reserved);
        let mut ctx = ctx();
        let mut table = ResourceTable::new();
        let (network, socket) = make(&mut ctx, &mut table, network::IpAddressFamily::Ipv4);
        view(&mut ctx, &mut table)
            .start_connect(borrow(&socket), borrow(&network), address.into())
            .unwrap();
        block_socket(&mut ctx, &mut table, &socket);
        assert_eq!(
            code(view(&mut ctx, &mut table).finish_connect(borrow(&socket))),
            ErrorCode::ConnectionRefused
        );
        assert_eq!(
            code(view(&mut ctx, &mut table).start_connect(
                borrow(&socket),
                borrow(&network),
                address.into()
            )),
            ErrorCode::InvalidState
        );
        let native = table
            .push(wasmtime::Error::new(ErrorCode::ConnectionReset))
            .unwrap();
        assert_eq!(
            network::Host::network_error_code(&mut view(&mut ctx, &mut table), borrow(&native))
                .unwrap(),
            Some(ErrorCode::ConnectionReset)
        );
        table.delete(native).unwrap();
        HostTcpSocket::drop(&mut view(&mut ctx, &mut table), socket).unwrap();
        network::HostNetwork::drop(&mut view(&mut ctx, &mut table), network).unwrap();
        assert!(table.is_empty());
    });
}

#[test]
fn tcp_wit_driver_loss_is_repeatable_poll_trap() {
    bounded(|| {
        let mut ctx = ctx();
        let mut table = ResourceTable::new();
        let (network, socket) = make(&mut ctx, &mut table, network::IpAddressFamily::Ipv4);
        view(&mut ctx, &mut table)
            .start_connect(
                borrow(&socket),
                borrow(&network),
                "127.0.0.1:9".parse::<SocketAddr>().unwrap().into(),
            )
            .unwrap();
        let poll = view(&mut ctx, &mut table)
            .subscribe(borrow(&socket))
            .unwrap();
        drop(ctx);
        for _ in 0..2 {
            assert!(HostPollable::block(&mut table, borrow(&poll))
                .unwrap_err()
                .to_string()
                .contains("TCP native driver failed"));
        }
        HostPollable::drop(&mut table, poll).unwrap();
        table.delete(socket).unwrap();
        table.delete(network).unwrap();
        assert!(table.is_empty());
    });
}

struct State {
    ctx: WasiCtx,
    table: ResourceTable,
}
impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.ctx,
            table: &mut self.table,
        }
    }
}
#[test]
fn tcp_wit_component_uses_canonical_streams_in_both_linkers() {
    bounded(|| {
        for asynchronous in [false, true] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let peer = std::thread::spawn(move || {
                let (mut peer, _) = listener.accept().unwrap();
                peer_config(&peer);
                let mut bytes = Vec::new();
                peer.read_to_end(&mut bytes).unwrap();
                assert_eq!(bytes, b"ping");
                peer.write_all(b"pong").unwrap();
            });
            let engine = wasmtime::Engine::default();
            let component =
                wasmtime::component::Component::new(&engine, include_str!("tcp.component.wat"))
                    .unwrap();
            let mut linker = wasmtime::component::Linker::new(&engine);
            if asynchronous {
                crate::p2::add_to_linker_async(&mut linker).unwrap();
            } else {
                crate::p2::add_to_linker_sync(&mut linker).unwrap();
            }
            let mut store = wasmtime::Store::new(
                &engine,
                State {
                    ctx: ctx(),
                    table: ResourceTable::new(),
                },
            );
            let instance = if asynchronous {
                futures_lite::future::block_on(linker.instantiate_async(&mut store, &component))
                    .unwrap()
            } else {
                linker.instantiate(&mut store, &component).unwrap()
            };
            let run = instance
                .get_typed_func::<(u16,), (u32,)>(&mut store, "run")
                .unwrap();
            let result = if asynchronous {
                futures_lite::future::block_on(run.call_async(&mut store, (port,))).unwrap()
            } else {
                run.call(&mut store, (port,)).unwrap()
            };
            assert_eq!(result, (4,));
            peer.join().unwrap();
            assert!(store.data().table.is_empty());
        }
    });
}

#[test]
fn tcp_wit_shutdown_and_stream_drop_with_full_task_budget() {
    bounded(|| {
        for explicit in [false, true] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let peer = std::thread::spawn(move || {
                let (mut peer, _) = listener.accept().unwrap();
                peer_config(&peer);
                let mut bytes = Vec::new();
                peer.read_to_end(&mut bytes).unwrap();
                assert!(bytes.is_empty());
            });
            let mut ctx = ctx_with_limit(1);
            let mut table = ResourceTable::new();
            let (network, socket) = make(&mut ctx, &mut table, network::IpAddressFamily::Ipv4);
            view(&mut ctx, &mut table)
                .start_connect(borrow(&socket), borrow(&network), address.into())
                .unwrap();
            block_socket(&mut ctx, &mut table, &socket);
            let (input, output) = view(&mut ctx, &mut table)
                .finish_connect(borrow(&socket))
                .unwrap();
            let occupied = ctx.driver.submit(|_| std::future::pending::<()>()).unwrap();
            assert_eq!(ctx.driver.outstanding(), 1);
            if explicit {
                view(&mut ctx, &mut table)
                    .shutdown(borrow(&socket), ShutdownType::Both)
                    .unwrap();
            }
            HostInputStream::drop(&mut table, input).unwrap();
            HostOutputStream::drop(&mut table, output).unwrap();
            // Keep the socket and the ordinary task alive until the peer sees FIN.
            // Resource deletion cannot mask a failed half-close.
            peer.join().unwrap();
            assert_eq!(ctx.driver.outstanding(), 1);
            occupied.cancel();
            assert_eq!(
                futures_lite::future::block_on(occupied),
                Err(perry_component_io::Error::Cancelled)
            );
            HostTcpSocket::drop(&mut view(&mut ctx, &mut table), socket).unwrap();
            network::HostNetwork::drop(&mut view(&mut ctx, &mut table), network).unwrap();
            assert!(table.is_empty());
        }
    });
}

#[test]
fn tcp_wit_backpressured_write_drains_before_fin_without_output_polling() {
    bounded(|| {
        for explicit in [false, true] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            socket2::SockRef::from(&listener)
                .set_recv_buffer_size(4096)
                .unwrap();
            let address = listener.local_addr().unwrap();
            let (release, gate) = mpsc::channel();
            let (connected, accepted) = mpsc::channel();
            let peer = std::thread::spawn(move || {
                let (mut peer, _) = listener.accept().unwrap();
                peer_config(&peer);
                connected.send(()).unwrap();
                gate.recv_timeout(WAIT).unwrap();
                let mut bytes = Vec::new();
                peer.read_to_end(&mut bytes).unwrap();
                assert_eq!(bytes, vec![91; 64 * 1024]);
            });
            let mut ctx = ctx_with_limit(1);
            let mut table = ResourceTable::new();
            let (network, socket) = make(&mut ctx, &mut table, network::IpAddressFamily::Ipv4);
            let mut v = view(&mut ctx, &mut table);
            v.set_send_buffer_size(borrow(&socket), 4096).unwrap();
            v.start_connect(borrow(&socket), borrow(&network), address.into())
                .unwrap();
            block_socket(&mut ctx, &mut table, &socket);
            let (input, output) = view(&mut ctx, &mut table)
                .finish_connect(borrow(&socket))
                .unwrap();
            accepted.recv_timeout(WAIT).unwrap();
            assert_eq!(
                HostOutputStream::check_write(&mut table, borrow(&output)).unwrap(),
                64 * 1024
            );
            HostOutputStream::write(&mut table, borrow(&output), vec![91; 64 * 1024]).unwrap();
            assert_eq!(
                ctx.driver.outstanding(),
                1,
                "peer has not drained its bounded receive window"
            );
            if explicit {
                view(&mut ctx, &mut table)
                    .shutdown(borrow(&socket), ShutdownType::Send)
                    .unwrap();
            } else {
                HostOutputStream::drop(&mut table, Resource::new_own(output.rep())).unwrap();
            }
            release.send(()).unwrap();
            // The native driver must finish the write and then deliver FIN,
            // without another call to the output stream's readiness methods.
            peer.join().unwrap();
            if explicit {
                HostOutputStream::drop(&mut table, output).unwrap();
            }
            HostInputStream::drop(&mut table, input).unwrap();
            HostTcpSocket::drop(&mut view(&mut ctx, &mut table), socket).unwrap();
            network::HostNetwork::drop(&mut view(&mut ctx, &mut table), network).unwrap();
            assert!(table.is_empty());
        }
    });
}
