mod canonical;
use super::{types::*, *};
use crate::p3::tests::{with_store, State};
use crate::sockets::{WasiSockets, WasiSocketsCtxView};
use futures_channel::oneshot;
use futures_lite::future::{block_on, poll_once, zip};
use std::{
    io::{Read, Write},
    net::{TcpListener, UdpSocket as StdUdp},
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};
use wasmtime::{
    component::{
        Accessor, FutureConsumer, FutureReader, Resource, Source, StreamConsumer, StreamReader,
        StreamResult,
    },
    StoreContextMut,
};
type Outcome = Result<(), ErrorCode>;
fn borrow<T: 'static>(resource: &Resource<T>) -> Resource<T> {
    Resource::new_borrow(resource.rep())
}
fn sockets(accessor: &Accessor<State>) -> Accessor<State, WasiSockets> {
    accessor.with_getter(|s| WasiSocketsCtxView {
        ctx: &mut s.ctx.as_mut().unwrap().sockets,
        table: &mut s.table,
    })
}
fn allow(store: &mut wasmtime::Store<State>) {
    let caller = std::thread::current().id();
    store.data_mut().ctx = Some(crate::WasiCtx::new(32).unwrap());
    store
        .data_mut()
        .ctx
        .as_mut()
        .unwrap()
        .sockets
        .allow_tcp(true)
        .allow_udp(true)
        .allow_ip_name_lookup(true)
        .socket_addr_check(move |address, _| {
            assert_eq!(std::thread::current().id(), caller);
            Box::pin(async move {
                assert_eq!(std::thread::current().id(), caller);
                address.ip().is_loopback() || address.ip().is_unspecified()
            })
        });
}
fn assert_clean(store: &mut wasmtime::Store<State>) {
    store.assert_concurrent_state_empty();
    let ctx = store.data().ctx.as_ref().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while ctx.outstanding_io() != 0 || ctx.driver.resources() != 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "native socket ownership or I/O survived disposal: {} resources, {} operations",
            ctx.driver.resources(),
            ctx.outstanding_io()
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
struct Receive(Option<oneshot::Sender<Outcome>>);
impl<T> FutureConsumer<T> for Receive {
    type Item = Outcome;
    fn poll_consume(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        store: StoreContextMut<T>,
        mut source: Source<Outcome>,
        _: bool,
    ) -> Poll<wasmtime::Result<()>> {
        let mut result = None;
        source.read(store, &mut result)?;
        if let Some(result) = result {
            let _ = self.0.take().unwrap().send(result);
        }
        Poll::Ready(Ok(()))
    }
}
fn receive(
    accessor: &Accessor<State>,
    result: FutureReader<Outcome>,
) -> oneshot::Receiver<Outcome> {
    let (send, receive) = oneshot::channel();
    accessor
        .with(|s| result.pipe(s, Receive(Some(send))))
        .unwrap();
    receive
}
struct Collect(Arc<Mutex<Vec<u8>>>);
impl<T> StreamConsumer<T> for Collect {
    type Item = u8;
    fn poll_consume(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        store: StoreContextMut<T>,
        source: Source<u8>,
        _: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        let mut source = source.as_direct(store);
        let length = source.remaining().len();
        self.0.lock().unwrap().extend_from_slice(source.remaining());
        source.mark_read(length);
        Poll::Ready(Ok(StreamResult::Completed))
    }
}
fn collect(accessor: &Accessor<State>, data: StreamReader<u8>) -> Arc<Mutex<Vec<u8>>> {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    accessor
        .with(|s| data.pipe(s, Collect(bytes.clone())))
        .unwrap();
    bytes
}
struct Accept(Option<oneshot::Sender<Resource<tcp::TcpSocket>>>);
impl<T> StreamConsumer<T> for Accept {
    type Item = Resource<tcp::TcpSocket>;
    fn poll_consume(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        store: StoreContextMut<T>,
        mut source: Source<Self::Item>,
        _: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        let mut socket = None;
        source.read(store, &mut socket)?;
        if let Some(socket) = socket {
            let _ = self.0.take().unwrap().send(socket);
            return Poll::Ready(Ok(StreamResult::Dropped));
        }
        Poll::Ready(Ok(StreamResult::Completed))
    }
}
fn tcp_create(accessor: &Accessor<State, WasiSockets>) -> Resource<tcp::TcpSocket> {
    accessor
        .with(|mut s| HostTcpSocket::create(&mut s.get(), IpAddressFamily::Ipv4))
        .unwrap()
}
fn udp_create(accessor: &Accessor<State, WasiSockets>) -> Resource<udp::UdpSocket> {
    accessor
        .with(|mut s| HostUdpSocket::create(&mut s.get(), IpAddressFamily::Ipv4))
        .unwrap()
}
fn udp_bind(
    accessor: &Accessor<State, WasiSockets>,
    socket: &Resource<udp::UdpSocket>,
) -> std::net::SocketAddr {
    accessor
        .with(|mut s| {
            block_on(HostUdpSocket::bind(
                &mut s.get(),
                borrow(socket),
                "127.0.0.1:0"
                    .parse::<std::net::SocketAddr>()
                    .unwrap()
                    .into(),
            ))
        })
        .unwrap();
    accessor
        .with(|mut s| HostUdpSocket::get_local_address(&mut s.get(), borrow(socket)))
        .unwrap()
        .into()
}
fn peer() -> StdUdp {
    let socket = StdUdp::bind("127.0.0.1:0").unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    socket
}

#[test]
fn sockets_are_denied_by_default_and_dns_is_explicit() {
    use crate::p3::bindings::sockets::ip_name_lookup as dns;
    with_store(|mut store| {
        block_on(store.run_concurrent(async |accessor| {
            let sockets = sockets(accessor);
            assert!(matches!(
                sockets
                    .with(|mut s| HostTcpSocket::create(&mut s.get(), IpAddressFamily::Ipv4))
                    .unwrap_err()
                    .downcast()
                    .unwrap(),
                ErrorCode::AccessDenied
            ));
            assert!(matches!(
                sockets
                    .with(|mut s| HostUdpSocket::create(&mut s.get(), IpAddressFamily::Ipv4))
                    .unwrap_err()
                    .downcast()
                    .unwrap(),
                ErrorCode::AccessDenied
            ));
            assert!(matches!(
                <WasiSockets as dns::HostWithStore<State>>::resolve_addresses(
                    &sockets,
                    "localhost".into()
                )
                .await
                .unwrap(),
                Err(dns::ErrorCode::AccessDenied)
            ));
            sockets.with(|mut s| s.get().ctx.allow_ip_name_lookup = true);
            for name in ["127.0.0.1", "::1", "::ffff:127.0.0.1", "localhost"] {
                let addresses = <WasiSockets as dns::HostWithStore<State>>::resolve_addresses(
                    &sockets,
                    name.into(),
                )
                .await
                .unwrap()
                .unwrap();
                assert!(!addresses.is_empty());
                for address in addresses {
                    let address: std::net::IpAddr = address.into();
                    assert!(address.is_loopback());
                    assert!(
                        !matches!(address,std::net::IpAddr::V6(ip) if ip.to_ipv4_mapped().is_some())
                    );
                }
            }
            assert!(matches!(
                <WasiSockets as dns::HostWithStore<State>>::resolve_addresses(
                    &sockets,
                    "invalid domain".into()
                )
                .await
                .unwrap(),
                Err(dns::ErrorCode::InvalidArgument)
            ));
            let driver = perry_component_io::Driver::new(1).unwrap();
            sockets.with(|mut s| s.get().ctx.submitter = driver.submitter());
            drop(driver);
            assert!(
                <WasiSockets as dns::HostWithStore<State>>::resolve_addresses(
                    &sockets,
                    "localhost".into()
                )
                .await
                .is_err()
            );
        }))
        .unwrap();
        assert_clean(&mut store);
    });
}

#[test]
fn tcp_streams_transfer_large_binary_data_and_half_close_once() {
    with_store(|mut store| {
        allow(&mut store);
        let expected: Vec<u8> = (0..3 * 1024 * 1024 + 137)
            .map(|i| (i * 17 + 31) as u8)
            .collect();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let want = expected.clone();
        let peer = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            socket.read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, want);
            socket.write_all(&bytes).unwrap();
            socket.shutdown(std::net::Shutdown::Write).unwrap();
        });
        block_on(store.run_concurrent(async |accessor| {
            let sockets = sockets(accessor);
            let socket = tcp_create(&sockets);
            <WasiSockets as HostTcpSocketWithStore<State>>::connect(
                &sockets,
                borrow(&socket),
                address.into(),
            )
            .await
            .unwrap();
            let data = accessor
                .with(|s| StreamReader::new(s, expected.clone()))
                .unwrap();
            let sent = sockets
                .with(|s| {
                    <WasiSockets as HostTcpSocketWithStore<State>>::send(s, borrow(&socket), data)
                })
                .unwrap();
            let (data, received) = sockets
                .with(|s| {
                    <WasiSockets as HostTcpSocketWithStore<State>>::receive(s, borrow(&socket))
                })
                .unwrap();
            let bytes = collect(accessor, data);
            let sent = receive(accessor, sent);
            let received = receive(accessor, received);
            let (a, b) = zip(sent, received).await;
            a.unwrap().unwrap();
            b.unwrap().unwrap();
            assert_eq!(*bytes.lock().unwrap(), expected);
            let data = accessor
                .with(|s| StreamReader::new(s, Vec::<u8>::new()))
                .unwrap();
            let sent = sockets
                .with(|s| {
                    <WasiSockets as HostTcpSocketWithStore<State>>::send(s, borrow(&socket), data)
                })
                .unwrap();
            assert!(matches!(
                receive(accessor, sent).await.unwrap(),
                Err(ErrorCode::InvalidState)
            ));
            let (data, result) = sockets
                .with(|s| {
                    <WasiSockets as HostTcpSocketWithStore<State>>::receive(s, borrow(&socket))
                })
                .unwrap();
            let _ = collect(accessor, data);
            assert!(matches!(
                receive(accessor, result).await.unwrap(),
                Err(ErrorCode::InvalidState)
            ));
            sockets
                .with(|mut s| HostTcpSocket::drop(&mut s.get(), socket))
                .unwrap();
        }))
        .unwrap();
        peer.join().unwrap();
        assert_clean(&mut store);
    });
}

#[test]
fn tcp_listen_implicitly_binds_and_accepted_streams_outlive_resources() {
    with_store(|mut store| {
        allow(&mut store);
        block_on(store.run_concurrent(async |accessor| {
            let sockets = sockets(accessor);
            let listener = tcp_create(&sockets);
            sockets
                .with(|mut s| {
                    HostTcpSocket::set_keep_alive_enabled(&mut s.get(), borrow(&listener), true)
                })
                .unwrap();
            let incoming = sockets
                .with(|s| {
                    block_on(<WasiSockets as HostTcpSocketWithStore<State>>::listen(
                        s,
                        borrow(&listener),
                    ))
                })
                .unwrap();
            let address: std::net::SocketAddr = sockets
                .with(|mut s| HostTcpSocket::get_local_address(&mut s.get(), borrow(&listener)))
                .unwrap()
                .into();
            assert_ne!(address.port(), 0);
            let address =
                std::net::SocketAddr::new(std::net::Ipv4Addr::LOCALHOST.into(), address.port());
            let peer = std::thread::spawn(move || {
                let mut socket = std::net::TcpStream::connect(address).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                socket.write_all(b"accepted").unwrap();
                socket.shutdown(std::net::Shutdown::Write).unwrap();
                let mut output = Vec::new();
                socket.read_to_end(&mut output).unwrap();
                assert_eq!(output, b"accepted");
            });
            let (send, accepted) = oneshot::channel();
            accessor
                .with(|s| incoming.pipe(s, Accept(Some(send))))
                .unwrap();
            let accepted = accepted.await.unwrap();
            sockets
                .with(|mut s| HostTcpSocket::drop(&mut s.get(), listener))
                .unwrap();
            assert!(sockets
                .with(|mut s| HostTcpSocket::get_keep_alive_enabled(
                    &mut s.get(),
                    borrow(&accepted)
                ))
                .unwrap());
            let (data, received) = sockets
                .with(|s| {
                    <WasiSockets as HostTcpSocketWithStore<State>>::receive(s, borrow(&accepted))
                })
                .unwrap();
            let sent = sockets
                .with(|s| {
                    <WasiSockets as HostTcpSocketWithStore<State>>::send(s, borrow(&accepted), data)
                })
                .unwrap();
            sockets
                .with(|mut s| HostTcpSocket::drop(&mut s.get(), accepted))
                .unwrap();
            receive(accessor, received).await.unwrap().unwrap();
            receive(accessor, sent).await.unwrap().unwrap();
            peer.join().unwrap();
        }))
        .unwrap();
        assert_clean(&mut store);
    });
}

#[test]
fn udp_implicit_binding_preserves_empty_and_maximum_datagrams() {
    with_store(|mut store| {
        allow(&mut store);
        let peer = peer();
        let address = peer.local_addr().unwrap();
        let peer = std::thread::spawn(move || {
            for _ in 0..4 {
                let mut buffer = vec![0; 65536];
                let (n, from) = peer.recv_from(&mut buffer).unwrap();
                peer.send_to(&buffer[..n], from).unwrap();
            }
        });
        block_on(store.run_concurrent(async |accessor| {
            let sockets = sockets(accessor);
            let socket = udp_create(&sockets);
            for length in [0, 1, 65_507, 7] {
                let expected: Vec<u8> = (0..length).map(|i| (i * 11) as u8).collect();
                <WasiSockets as HostUdpSocketWithStore<State>>::send(
                    &sockets,
                    borrow(&socket),
                    expected.clone(),
                    Some(address.into()),
                )
                .await
                .unwrap();
                let (data, from) = <WasiSockets as HostUdpSocketWithStore<State>>::receive(
                    &sockets,
                    borrow(&socket),
                )
                .await
                .unwrap();
                assert_eq!(data, expected);
                assert_eq!(std::net::SocketAddr::from(from), address);
            }
            assert!(matches!(
                <WasiSockets as HostUdpSocketWithStore<State>>::send(
                    &sockets,
                    borrow(&socket),
                    vec![0; 65508],
                    Some(address.into())
                )
                .await
                .unwrap_err()
                .downcast()
                .unwrap(),
                ErrorCode::DatagramTooLarge
            ));
            sockets
                .with(|mut s| HostUdpSocket::drop(&mut s.get(), socket))
                .unwrap();
        }))
        .unwrap();
        peer.join().unwrap();
        assert_clean(&mut store);
    });
}

#[test]
fn udp_cancelled_and_concurrent_receives_preserve_driver_state() {
    with_store(|mut store| {
        allow(&mut store);
        let peer = peer();
        block_on(store.run_concurrent(async |accessor| {
            let sockets = sockets(accessor);
            let socket = udp_create(&sockets);
            let address = udp_bind(&sockets, &socket);
            let mut cancelled = Box::pin(<WasiSockets as HostUdpSocketWithStore<State>>::receive(
                &sockets,
                borrow(&socket),
            ));
            assert!(poll_once(cancelled.as_mut()).await.is_none());
            drop(cancelled);
            let mut first = Box::pin(<WasiSockets as HostUdpSocketWithStore<State>>::receive(
                &sockets,
                borrow(&socket),
            ));
            let mut second = Box::pin(<WasiSockets as HostUdpSocketWithStore<State>>::receive(
                &sockets,
                borrow(&socket),
            ));
            assert!(poll_once(first.as_mut()).await.is_none());
            assert!(poll_once(second.as_mut()).await.is_none());
            peer.send_to(b"one", address).unwrap();
            peer.send_to(b"two", address).unwrap();
            let (a, b) = zip(first, second).await;
            assert_eq!(a.unwrap().0, b"one");
            assert_eq!(b.unwrap().0, b"two");
            sockets
                .with(|mut s| HostUdpSocket::drop(&mut s.get(), socket))
                .unwrap();
        }))
        .unwrap();
        assert_clean(&mut store);
    });
}

#[test]
fn udp_reassociation_updates_a_pending_receive_filter() {
    with_store(|mut store| {
        allow(&mut store);
        let one = peer();
        let two = peer();
        block_on(store.run_concurrent(async |accessor| {
            let sockets = sockets(accessor);
            let socket = udp_create(&sockets);
            let address = udp_bind(&sockets, &socket);
            sockets
                .with(|mut s| {
                    block_on(HostUdpSocket::connect(
                        &mut s.get(),
                        borrow(&socket),
                        one.local_addr().unwrap().into(),
                    ))
                })
                .unwrap();
            let mut pending = Box::pin(<WasiSockets as HostUdpSocketWithStore<State>>::receive(
                &sockets,
                borrow(&socket),
            ));
            assert!(poll_once(pending.as_mut()).await.is_none());
            sockets
                .with(|mut s| {
                    block_on(HostUdpSocket::connect(
                        &mut s.get(),
                        borrow(&socket),
                        two.local_addr().unwrap().into(),
                    ))
                })
                .unwrap();
            one.send_to(b"wrong-peer", address).unwrap();
            two.send_to(b"right-peer", address).unwrap();
            assert_eq!(pending.await.unwrap().0, b"right-peer");
            sockets
                .with(|mut s| HostUdpSocket::disconnect(&mut s.get(), borrow(&socket)))
                .unwrap();
            one.send_to(b"unconnected", address).unwrap();
            assert_eq!(
                <WasiSockets as HostUdpSocketWithStore<State>>::receive(&sockets, borrow(&socket))
                    .await
                    .unwrap()
                    .0,
                b"unconnected"
            );
            sockets
                .with(|mut s| HostUdpSocket::drop(&mut s.get(), socket))
                .unwrap();
        }))
        .unwrap();
        assert_clean(&mut store);
    });
}

#[test]
fn cancelled_tcp_connect_closes_but_cancelled_udp_permission_restores_owner() {
    use crate::sockets::SocketAddrUse;
    use std::sync::atomic::{AtomicBool, Ordering};
    with_store(|mut store| {
        allow(&mut store);
        let peer = peer();
        let address = peer.local_addr().unwrap();
        let first = Arc::new(AtomicBool::new(true));
        store
            .data_mut()
            .ctx
            .as_mut()
            .unwrap()
            .sockets
            .socket_addr_check(move |_, usage| {
                let wait = usage == SocketAddrUse::TcpConnect
                    || (usage == SocketAddrUse::UdpOutgoingDatagram
                        && first.swap(false, Ordering::SeqCst));
                Box::pin(async move {
                    if wait {
                        std::future::pending().await
                    } else {
                        true
                    }
                })
            });
        block_on(store.run_concurrent(async |accessor| {
            let sockets = sockets(accessor);
            let tcp = tcp_create(&sockets);
            let local: std::net::SocketAddr = sockets.with(|mut s| {
                block_on(HostTcpSocket::bind(
                    &mut s.get(),
                    borrow(&tcp),
                    std::net::SocketAddr::from(([127, 0, 0, 1], 0)).into(),
                ))
                .unwrap();
                HostTcpSocket::get_local_address(&mut s.get(), borrow(&tcp))
                    .unwrap()
                    .into()
            });
            let mut connecting = Box::pin(<WasiSockets as HostTcpSocketWithStore<State>>::connect(
                &sockets,
                borrow(&tcp),
                address.into(),
            ));
            assert!(poll_once(connecting.as_mut()).await.is_none());
            sockets
                .with(|mut s| HostTcpSocket::set_hop_limit(&mut s.get(), borrow(&tcp), 42))
                .unwrap();
            drop(connecting);
            // Even with the guest resource alive, cancellation must release
            // both native handles and make every control operation invalid.
            let rebound = TcpListener::bind(local).unwrap();
            drop(rebound);
            assert!(matches!(
                sockets
                    .with(|mut s| HostTcpSocket::get_hop_limit(&mut s.get(), borrow(&tcp)))
                    .unwrap_err()
                    .downcast()
                    .unwrap(),
                ErrorCode::InvalidState
            ));
            assert!(matches!(
                <WasiSockets as HostTcpSocketWithStore<State>>::connect(
                    &sockets,
                    borrow(&tcp),
                    address.into()
                )
                .await
                .unwrap_err()
                .downcast()
                .unwrap(),
                ErrorCode::InvalidState
            ));
            sockets
                .with(|mut s| HostTcpSocket::drop(&mut s.get(), tcp))
                .unwrap();
            let udp = udp_create(&sockets);
            let mut sending = Box::pin(<WasiSockets as HostUdpSocketWithStore<State>>::send(
                &sockets,
                borrow(&udp),
                vec![1],
                Some(address.into()),
            ));
            assert!(poll_once(sending.as_mut()).await.is_none());
            drop(sending);
            assert!(matches!(
                sockets
                    .with(|mut s| HostUdpSocket::get_local_address(&mut s.get(), borrow(&udp)))
                    .unwrap_err()
                    .downcast()
                    .unwrap(),
                ErrorCode::InvalidState
            ));
            <WasiSockets as HostUdpSocketWithStore<State>>::send(
                &sockets,
                borrow(&udp),
                vec![3, 0, 7],
                Some(address.into()),
            )
            .await
            .unwrap();
            let mut packet = [0; 16];
            let (n, _) = peer.recv_from(&mut packet).unwrap();
            assert_eq!(&packet[..n], [3, 0, 7]);
            sockets
                .with(|mut s| HostUdpSocket::drop(&mut s.get(), udp))
                .unwrap();
        }))
        .unwrap();
        assert_clean(&mut store);
    });
}

#[test]
fn udp_ipv6_concurrent_sends_and_denied_destinations() {
    with_store(|mut store| {
        allow(&mut store);
        let peer = StdUdp::bind("[::1]:0").unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let address = peer.local_addr().unwrap();
        block_on(store.run_concurrent(async |accessor| {
            let sockets = sockets(accessor);
            let socket = sockets
                .with(|mut s| HostUdpSocket::create(&mut s.get(), IpAddressFamily::Ipv6))
                .unwrap();
            let denied: std::net::SocketAddr = "[2001:db8::1]:4444".parse().unwrap();
            assert!(matches!(
                <WasiSockets as HostUdpSocketWithStore<State>>::send(
                    &sockets,
                    borrow(&socket),
                    vec![0],
                    Some(denied.into())
                )
                .await
                .unwrap_err()
                .downcast()
                .unwrap(),
                ErrorCode::AccessDenied
            ));
            assert!(matches!(
                sockets
                    .with(|mut s| HostUdpSocket::get_local_address(&mut s.get(), borrow(&socket)))
                    .unwrap_err()
                    .downcast()
                    .unwrap(),
                ErrorCode::InvalidState
            ));
            let (a, b) = zip(
                <WasiSockets as HostUdpSocketWithStore<State>>::send(
                    &sockets,
                    borrow(&socket),
                    vec![1, 2],
                    Some(address.into()),
                ),
                <WasiSockets as HostUdpSocketWithStore<State>>::send(
                    &sockets,
                    borrow(&socket),
                    vec![3, 4],
                    Some(address.into()),
                ),
            )
            .await;
            a.unwrap();
            b.unwrap();
            let mut packets = Vec::new();
            for _ in 0..2 {
                let mut packet = [0; 16];
                let (n, _) = peer.recv_from(&mut packet).unwrap();
                packets.push(packet[..n].to_vec());
            }
            packets.sort();
            assert_eq!(packets, [vec![1, 2], vec![3, 4]]);
            let bad: std::net::SocketAddr = "[::ffff:127.0.0.1]:3333".parse().unwrap();
            assert!(matches!(
                <WasiSockets as HostUdpSocketWithStore<State>>::send(
                    &sockets,
                    borrow(&socket),
                    vec![],
                    Some(bad.into())
                )
                .await
                .unwrap_err()
                .downcast()
                .unwrap(),
                ErrorCode::InvalidArgument
            ));
            assert!(matches!(
                sockets
                    .with(|mut s| HostUdpSocket::get_address_family(&mut s.get(), borrow(&socket)))
                    .unwrap(),
                IpAddressFamily::Ipv6
            ));
            sockets
                .with(|mut s| HostUdpSocket::drop(&mut s.get(), socket))
                .unwrap();
        }))
        .unwrap();
        assert_clean(&mut store);
    });
}

#[test]
fn p3_broken_pipe_is_distinct_while_p2_mapping_is_preserved() {
    use crate::sockets::tcp::Failure;
    let native = || Failure::from(std::io::Error::from(std::io::ErrorKind::BrokenPipe));
    let old = crate::p2::bindings::sockets::network::ErrorCode::from(std::io::Error::from(
        std::io::ErrorKind::BrokenPipe,
    ));
    assert_eq!(native().socket().downcast().unwrap(), old);
    assert!(matches!(
        SocketError::from(native()).downcast().unwrap(),
        ErrorCode::ConnectionBroken
    ));
}
