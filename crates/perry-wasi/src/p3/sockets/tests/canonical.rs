use super::*;
use wasmtime::component::{Component, Linker};

#[test]
fn canonical_tcp_streams_echo_binary_data_and_preserve_one_shot_errors() {
    with_store(|mut store| {
        allow(&mut store);
        let expected: Vec<u8> = (0..3 * 1024 * 1024 + 137)
            .map(|i| (i * 17 + 31) as u8)
            .collect();
        let component =
            Component::new(store.engine(), include_str!("../streams.component.wat")).unwrap();
        let mut linker = Linker::<State>::new(store.engine());
        crate::p3::add_to_linker(&mut linker).unwrap();
        block_on(async {
            let instance = linker
                .instantiate_async(&mut store, &component)
                .await
                .unwrap();
            let run=instance.get_typed_func::<(Resource<tcp::TcpSocket>,),((FutureReader<Outcome>,FutureReader<Outcome>),)>(&mut store,"run").unwrap();
            store
                .run_concurrent(async |accessor| {
                    let sockets = sockets(accessor);
                    let listener = tcp_create(&sockets);
                    let incoming = sockets
                        .with(|s| {
                            block_on(<WasiSockets as HostTcpSocketWithStore<State>>::listen(
                                s,
                                borrow(&listener),
                            ))
                        })
                        .unwrap();
                    let address: std::net::SocketAddr = sockets
                        .with(|mut s| {
                            HostTcpSocket::get_local_address(&mut s.get(), borrow(&listener))
                        })
                        .unwrap()
                        .into();
                    let address = std::net::SocketAddr::new(
                        std::net::Ipv4Addr::LOCALHOST.into(),
                        address.port(),
                    );
                    let want = expected.clone();
                    let peer = std::thread::spawn(move || {
                        let mut socket = std::net::TcpStream::connect(address).unwrap();
                        socket
                            .set_read_timeout(Some(Duration::from_secs(5)))
                            .unwrap();
                        socket
                            .set_write_timeout(Some(Duration::from_secs(5)))
                            .unwrap();
                        let mut writer = socket.try_clone().unwrap();
                        let sent = want.clone();
                        let writer = std::thread::spawn(move || {
                            writer.write_all(&sent).unwrap();
                            writer.shutdown(std::net::Shutdown::Write).unwrap();
                        });
                        let mut received = Vec::new();
                        socket.read_to_end(&mut received).unwrap();
                        writer.join().unwrap();
                        assert_eq!(received, want);
                    });
                    let (send, accepted) = oneshot::channel();
                    accessor
                        .with(|s| incoming.pipe(s, Accept(Some(send))))
                        .unwrap();
                    let accepted = accepted.await.unwrap();
                    sockets
                        .with(|mut s| HostTcpSocket::drop(&mut s.get(), listener))
                        .unwrap();
                    let ((input, output),) = run
                        .call_concurrent(accessor, (borrow(&accepted),))
                        .await
                        .unwrap();
                    let (a, b) = zip(receive(accessor, input), receive(accessor, output)).await;
                    a.unwrap().unwrap();
                    b.unwrap().unwrap();
                    let ((input, output),) = run
                        .call_concurrent(accessor, (borrow(&accepted),))
                        .await
                        .unwrap();
                    assert!(matches!(
                        receive(accessor, input).await.unwrap(),
                        Err(ErrorCode::InvalidState)
                    ));
                    assert!(matches!(
                        receive(accessor, output).await.unwrap(),
                        Err(ErrorCode::InvalidState)
                    ));
                    sockets
                        .with(|mut s| HostTcpSocket::drop(&mut s.get(), accepted))
                        .unwrap();
                    peer.join().unwrap();
                })
                .await
                .unwrap();
        });
        assert_clean(&mut store);
    });
}

#[test]
fn canonical_tcp_connect_suspends_without_retaining_the_store() {
    with_store(|mut store| {
        allow(&mut store);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = std::thread::spawn(move || {
            let _ = listener.accept().unwrap();
        });
        let (started, start) = oneshot::channel();
        let (release, released) = oneshot::channel();
        let gates = Mutex::new(Some((started, released)));
        let caller = std::thread::current().id();
        store
            .data_mut()
            .ctx
            .as_mut()
            .unwrap()
            .sockets
            .socket_addr_check(move |_, usage| {
                assert_eq!(std::thread::current().id(), caller);
                if usage == crate::sockets::SocketAddrUse::TcpConnect {
                    let (started, released) = gates.lock().unwrap().take().unwrap();
                    Box::pin(async move {
                        let _ = started.send(());
                        released.await.unwrap();
                        true
                    })
                } else {
                    Box::pin(async { true })
                }
            });
        let component =
            Component::new(store.engine(), include_str!("../connect.component.wat")).unwrap();
        let mut linker = Linker::<State>::new(store.engine());
        crate::p3::add_to_linker(&mut linker).unwrap();
        block_on(async {
            let instance = linker
                .instantiate_async(&mut store, &component)
                .await
                .unwrap();
            let run = instance
                .get_typed_func::<(Resource<tcp::TcpSocket>, u16), (u32,)>(&mut store, "run")
                .unwrap();
            let callbacks = instance
                .get_typed_func::<(), (u32,)>(&mut store, "callbacks")
                .unwrap();
            store
                .run_concurrent(async |accessor| {
                    let sockets = sockets(accessor);
                    let socket = tcp_create(&sockets);
                    let local: std::net::SocketAddr = sockets.with(|mut s| {
                        block_on(HostTcpSocket::bind(
                            &mut s.get(),
                            borrow(&socket),
                            std::net::SocketAddr::from(([127, 0, 0, 1], 0)).into(),
                        ))
                        .unwrap();
                        HostTcpSocket::get_local_address(&mut s.get(), borrow(&socket))
                            .unwrap()
                            .into()
                    });
                    let (result, ()) = zip(
                        run.call_concurrent(accessor, (borrow(&socket), port)),
                        async {
                            start.await.unwrap();
                            // Host access while the permission future is held proves
                            // the async method released its Store borrow.
                            sockets.with(|mut s| {
                                let mut view = s.get();
                                let actual: std::net::SocketAddr =
                                    HostTcpSocket::get_local_address(&mut view, borrow(&socket))
                                        .unwrap()
                                        .into();
                                assert_eq!(actual, local);
                                HostTcpSocket::set_keep_alive_enabled(
                                    &mut view,
                                    borrow(&socket),
                                    true,
                                )
                                .unwrap();
                                HostTcpSocket::set_hop_limit(&mut view, borrow(&socket), 42)
                                    .unwrap();
                                HostTcpSocket::set_keep_alive_idle_time(
                                    &mut view,
                                    borrow(&socket),
                                    7_000_000_000,
                                )
                                .unwrap();
                                HostTcpSocket::set_keep_alive_interval(
                                    &mut view,
                                    borrow(&socket),
                                    3_000_000_000,
                                )
                                .unwrap();
                                HostTcpSocket::set_keep_alive_count(&mut view, borrow(&socket), 4)
                                    .unwrap();
                                HostTcpSocket::set_receive_buffer_size(
                                    &mut view,
                                    borrow(&socket),
                                    32_768,
                                )
                                .unwrap();
                                HostTcpSocket::set_send_buffer_size(
                                    &mut view,
                                    borrow(&socket),
                                    32_768,
                                )
                                .unwrap();
                                assert!(HostTcpSocket::get_keep_alive_enabled(
                                    &mut view,
                                    borrow(&socket)
                                )
                                .unwrap());
                                assert_eq!(
                                    HostTcpSocket::get_hop_limit(&mut view, borrow(&socket))
                                        .unwrap(),
                                    42
                                );
                                assert!(matches!(
                                    HostTcpSocket::set_listen_backlog_size(
                                        &mut view,
                                        borrow(&socket),
                                        8
                                    )
                                    .unwrap_err()
                                    .downcast()
                                    .unwrap(),
                                    ErrorCode::InvalidState
                                ));
                            });
                            assert!(matches!(
                                sockets
                                    .with(|mut s| HostTcpSocket::get_remote_address(
                                        &mut s.get(),
                                        borrow(&socket)
                                    ))
                                    .unwrap_err()
                                    .downcast()
                                    .unwrap(),
                                ErrorCode::InvalidState
                            ));
                            release.send(()).unwrap();
                        },
                    )
                    .await;
                    assert_eq!(result.unwrap().0, 0);
                    sockets.with(|mut s| {
                        let mut view = s.get();
                        let actual: std::net::SocketAddr =
                            HostTcpSocket::get_local_address(&mut view, borrow(&socket))
                                .unwrap()
                                .into();
                        assert_eq!(actual, local);
                        assert!(
                            HostTcpSocket::get_keep_alive_enabled(&mut view, borrow(&socket))
                                .unwrap()
                        );
                        assert_eq!(
                            HostTcpSocket::get_hop_limit(&mut view, borrow(&socket)).unwrap(),
                            42
                        );
                        assert_eq!(
                            HostTcpSocket::get_keep_alive_idle_time(&mut view, borrow(&socket))
                                .unwrap(),
                            7_000_000_000
                        );
                        assert_eq!(
                            HostTcpSocket::get_keep_alive_interval(&mut view, borrow(&socket))
                                .unwrap(),
                            3_000_000_000
                        );
                        assert_eq!(
                            HostTcpSocket::get_keep_alive_count(&mut view, borrow(&socket))
                                .unwrap(),
                            4
                        );
                        assert!(
                            HostTcpSocket::get_receive_buffer_size(&mut view, borrow(&socket))
                                .unwrap()
                                >= 32_768
                        );
                        assert!(
                            HostTcpSocket::get_send_buffer_size(&mut view, borrow(&socket))
                                .unwrap()
                                >= 32_768
                        );
                    });
                    assert!(
                        callbacks.call_concurrent(accessor, ()).await.unwrap().0 > 0,
                        "guest did not suspend through its canonical callback"
                    );
                    assert_eq!(
                        run.call_concurrent(accessor, (borrow(&socket), port))
                            .await
                            .unwrap()
                            .0,
                        6
                    );
                    sockets
                        .with(|mut s| HostTcpSocket::drop(&mut s.get(), socket))
                        .unwrap();
                })
                .await
                .unwrap();
        });
        peer.join().unwrap();
        assert_clean(&mut store);
    });
}

#[test]
fn canonical_udp_async_calls_exchange_binary_packets_and_reuse_the_binding() {
    with_store(|mut store| {
        allow(&mut store);
        let peer = peer();
        let port = peer.local_addr().unwrap().port();
        let peer = std::thread::spawn(move || {
            for _ in 0..2 {
                let mut bytes = [0; 16];
                let (n, from) = peer.recv_from(&mut bytes).unwrap();
                assert_eq!(&bytes[..n], [97, 0, 255]);
                peer.send_to(&bytes[..n], from).unwrap();
            }
        });
        let component =
            Component::new(store.engine(), include_str!("../datagram.component.wat")).unwrap();
        let mut linker = Linker::<State>::new(store.engine());
        crate::p3::add_to_linker(&mut linker).unwrap();
        block_on(async {
            let instance = linker
                .instantiate_async(&mut store, &component)
                .await
                .unwrap();
            let run = instance
                .get_typed_func::<(Resource<udp::UdpSocket>, u16), (u32,)>(&mut store, "run")
                .unwrap();
            store
                .run_concurrent(async |accessor| {
                    let sockets = sockets(accessor);
                    let socket = udp_create(&sockets);
                    let mut previous = None;
                    for _ in 0..2 {
                        assert_eq!(
                            run.call_concurrent(accessor, (borrow(&socket), port))
                                .await
                                .unwrap()
                                .0,
                            0
                        );
                        let address: std::net::SocketAddr = sockets
                            .with(|mut s| {
                                HostUdpSocket::get_local_address(&mut s.get(), borrow(&socket))
                            })
                            .unwrap()
                            .into();
                        if let Some(previous) = previous {
                            assert_eq!(address, previous);
                        } else {
                            previous = Some(address);
                        }
                    }
                    sockets
                        .with(|mut s| HostUdpSocket::drop(&mut s.get(), socket))
                        .unwrap();
                })
                .await
                .unwrap();
        });
        peer.join().unwrap();
        assert_clean(&mut store);
    });
}
