use super::*;
use crate::{
    io::sync::wasi::io::poll::HostPollable,
    p2::bindings::{
        sockets::{instance_network, network, udp_create_socket},
        sync::sockets::udp::{
            HostIncomingDatagramStream, HostOutgoingDatagramStream, HostUdpSocket, OutgoingDatagram,
        },
    },
    sockets::WasiSocketsCtxView,
    WasiCtx, WasiCtxView, WasiView,
};
use std::{net::SocketAddr, sync::mpsc, time::Duration};
use wasmtime::component::{Resource, ResourceTable};
const WAIT: Duration = Duration::from_secs(10);
fn bounded(test: impl FnOnce() + Send + 'static) {
    let (send, recv) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        test();
        send.send(()).unwrap();
    });
    recv.recv_timeout(WAIT).expect("UDP WIT test timed out");
    thread.join().unwrap();
}
fn ctx(limit: usize) -> WasiCtx {
    let mut ctx = WasiCtx::new(limit).unwrap();
    ctx.sockets
        .allow_udp(true)
        .socket_addr_check(|_, _| Box::pin(async { true }));
    ctx
}
fn view<'a>(ctx: &'a mut WasiCtx, table: &'a mut ResourceTable) -> WasiSocketsCtxView<'a> {
    WasiSocketsCtxView {
        ctx: &mut ctx.sockets,
        table,
    }
}
fn borrow<T: 'static>(value: &Resource<T>) -> Resource<T> {
    Resource::new_borrow(value.rep())
}
fn code<T>(result: SocketResult<T>) -> network::ErrorCode {
    match result {
        Err(e) => e.downcast().unwrap(),
        Ok(_) => panic!("expected error"),
    }
}
fn make(
    ctx: &mut WasiCtx,
    table: &mut ResourceTable,
    address: SocketAddr,
) -> (Resource<network::Network>, Resource<UdpSocket>) {
    let mut v = view(ctx, table);
    let network = instance_network::Host::instance_network(&mut v).unwrap();
    let family = if address.is_ipv4() {
        network::IpAddressFamily::Ipv4
    } else {
        network::IpAddressFamily::Ipv6
    };
    let socket = udp_create_socket::Host::create_udp_socket(&mut v, family).unwrap();
    v.start_bind(borrow(&socket), borrow(&network), address.into())
        .unwrap();
    v.finish_bind(borrow(&socket)).unwrap();
    (network, socket)
}
fn receive(
    ctx: &mut WasiCtx,
    table: &mut ResourceTable,
    input: &Resource<IncomingDatagramStream>,
) -> crate::p2::bindings::sync::sockets::udp::IncomingDatagram {
    let poll = HostIncomingDatagramStream::subscribe(&mut view(ctx, table), borrow(input)).unwrap();
    HostPollable::block(table, borrow(&poll)).unwrap();
    HostPollable::drop(table, poll).unwrap();
    let mut packets =
        HostIncomingDatagramStream::receive(&mut view(ctx, table), borrow(input), u64::MAX)
            .unwrap();
    assert_eq!(packets.len(), 1);
    packets.remove(0)
}
fn drop_streams(
    ctx: &mut WasiCtx,
    table: &mut ResourceTable,
    input: Resource<IncomingDatagramStream>,
    output: Resource<OutgoingDatagramStream>,
) {
    HostIncomingDatagramStream::drop(&mut view(ctx, table), input).unwrap();
    HostOutgoingDatagramStream::drop(&mut view(ctx, table), output).unwrap();
}
fn drop_socket(
    ctx: &mut WasiCtx,
    table: &mut ResourceTable,
    network: Resource<network::Network>,
    socket: Resource<UdpSocket>,
) {
    HostUdpSocket::drop(&mut view(ctx, table), socket).unwrap();
    network::HostNetwork::drop(&mut view(ctx, table), network).unwrap();
    assert!(table.is_empty());
}

#[test]
fn udp_wit_network_capabilities_bind_states_and_options() {
    bounded(|| {
        let mut denied = WasiCtx::new(8).unwrap();
        let mut table = ResourceTable::new();
        assert_eq!(
            code(udp_create_socket::Host::create_udp_socket(
                &mut view(&mut denied, &mut table),
                network::IpAddressFamily::Ipv4
            )),
            network::ErrorCode::AccessDenied
        );
        let mut ctx = ctx(8);
        let mut v = view(&mut ctx, &mut table);
        let own = instance_network::Host::instance_network(&mut v).unwrap();
        let alias = instance_network::Host::instance_network(&mut v).unwrap();
        let socket =
            udp_create_socket::Host::create_udp_socket(&mut v, network::IpAddressFamily::Ipv4)
                .unwrap();
        let foreign =
            instance_network::Host::instance_network(&mut view(&mut denied, &mut table)).unwrap();
        let address = SocketAddr::from(([127, 0, 0, 1], 0)).into();
        let mut v = view(&mut ctx, &mut table);
        assert_eq!(
            code(v.finish_bind(borrow(&socket))),
            network::ErrorCode::NotInProgress
        );
        assert_eq!(
            code(v.stream(borrow(&socket), None)),
            network::ErrorCode::InvalidState
        );
        assert!(v
            .start_bind(borrow(&socket), Resource::new_borrow(u32::MAX), address)
            .unwrap_err()
            .downcast()
            .is_err());
        assert_eq!(
            code(v.start_bind(borrow(&socket), borrow(&foreign), address)),
            network::ErrorCode::InvalidArgument
        );
        v.set_unicast_hop_limit(borrow(&socket), 31).unwrap();
        v.start_bind(borrow(&socket), borrow(&alias), address)
            .unwrap();
        assert_eq!(
            code(v.start_bind(borrow(&socket), borrow(&own), address)),
            network::ErrorCode::ConcurrencyConflict
        );
        assert_eq!(
            code(v.stream(borrow(&socket), None)),
            network::ErrorCode::ConcurrencyConflict
        );
        v.finish_bind(borrow(&socket)).unwrap();
        assert_eq!(v.unicast_hop_limit(borrow(&socket)).unwrap(), 31);
        assert_eq!(
            code(v.set_unicast_hop_limit(borrow(&socket), 0)),
            network::ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(v.set_send_buffer_size(borrow(&socket), 0)),
            network::ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(v.set_receive_buffer_size(borrow(&socket), 0)),
            network::ErrorCode::InvalidArgument
        );
        let poll = HostUdpSocket::subscribe(&mut v, borrow(&socket)).unwrap();
        assert!(HostUdpSocket::drop(&mut v, Resource::new_own(socket.rep())).is_err());
        HostPollable::block(&mut table, borrow(&poll)).unwrap();
        HostPollable::drop(&mut table, poll).unwrap();
        for resource in [alias, foreign] {
            network::HostNetwork::drop(&mut view(&mut ctx, &mut table), resource).unwrap();
        }
        drop_socket(&mut ctx, &mut table, own, socket);
    });
}

#[test]
fn udp_wit_packets_permits_and_child_resources() {
    bounded(|| {
        for address in ["127.0.0.1:0", "[::1]:0"] {
            let peer = std::net::UdpSocket::bind(address).unwrap();
            peer.set_read_timeout(Some(WAIT)).unwrap();
            socket2::SockRef::from(&peer)
                .set_send_buffer_size(131_072)
                .unwrap();
            let remote = peer.local_addr().unwrap();
            let peer = std::thread::spawn(move || {
                for expected in [Vec::new(), vec![61; 65_507]] {
                    let mut bytes = vec![0xcc; 65_535];
                    let (n, from) = peer.recv_from(&mut bytes).unwrap();
                    assert_eq!(&bytes[..n], expected);
                    peer.send_to(&bytes[..n], from).unwrap();
                }
            });
            let mut ctx = ctx(8);
            let mut table = ResourceTable::new();
            let (network, socket) = make(&mut ctx, &mut table, address.parse().unwrap());
            view(&mut ctx, &mut table)
                .set_send_buffer_size(borrow(&socket), 131_072)
                .unwrap();
            let (input, output) = view(&mut ctx, &mut table)
                .stream(borrow(&socket), Some(remote.into()))
                .unwrap();
            assert!(HostUdpSocket::drop(
                &mut view(&mut ctx, &mut table),
                Resource::new_own(socket.rep())
            )
            .is_err());
            assert!(HostIncomingDatagramStream::receive(
                &mut view(&mut ctx, &mut table),
                borrow(&input),
                0
            )
            .unwrap()
            .is_empty());
            assert!(HostOutgoingDatagramStream::send(
                &mut view(&mut ctx, &mut table),
                borrow(&output),
                vec![]
            )
            .unwrap_err()
            .downcast()
            .is_err());
            for bytes in [Vec::new(), vec![61; 65_507]] {
                let poll = HostOutgoingDatagramStream::subscribe(
                    &mut view(&mut ctx, &mut table),
                    borrow(&output),
                )
                .unwrap();
                HostPollable::block(&mut table, borrow(&poll)).unwrap();
                assert_eq!(
                    HostOutgoingDatagramStream::check_send(
                        &mut view(&mut ctx, &mut table),
                        borrow(&output)
                    )
                    .unwrap(),
                    1
                );
                assert!(HostOutgoingDatagramStream::send(
                    &mut view(&mut ctx, &mut table),
                    borrow(&output),
                    vec![
                        OutgoingDatagram {
                            data: vec![],
                            remote_address: None
                        },
                        OutgoingDatagram {
                            data: vec![],
                            remote_address: None
                        }
                    ]
                )
                .unwrap_err()
                .downcast()
                .is_err());
                assert_eq!(
                    HostOutgoingDatagramStream::send(
                        &mut view(&mut ctx, &mut table),
                        borrow(&output),
                        vec![OutgoingDatagram {
                            data: bytes.clone(),
                            remote_address: None
                        }]
                    )
                    .unwrap(),
                    1
                );
                HostPollable::block(&mut table, borrow(&poll)).unwrap();
                HostPollable::drop(&mut table, poll).unwrap();
                let packet = receive(&mut ctx, &mut table, &input);
                assert_eq!(packet.data, bytes);
                assert_eq!(SocketAddr::from(packet.remote_address), remote);
            }
            peer.join().unwrap();
            drop_streams(&mut ctx, &mut table, input, output);
            drop_socket(&mut ctx, &mut table, network, socket);
        }
    });
}

#[test]
fn udp_wit_reassociation_filters_peers_and_cancels_old_receive() {
    bounded(|| {
        let a = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let b = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let mut ctx = ctx(8);
        let mut table = ResourceTable::new();
        let (network, socket) = make(&mut ctx, &mut table, "127.0.0.1:0".parse().unwrap());
        let local = SocketAddr::from(
            view(&mut ctx, &mut table)
                .local_address(borrow(&socket))
                .unwrap(),
        );
        for (source, other, data) in [
            (&a, &b, b"first".as_slice()),
            (&b, &a, b"second".as_slice()),
        ] {
            let remote = source.local_addr().unwrap();
            let (input, output) = view(&mut ctx, &mut table)
                .stream(borrow(&socket), Some(remote.into()))
                .unwrap();
            assert!(view(&mut ctx, &mut table)
                .stream(borrow(&socket), None)
                .unwrap_err()
                .downcast()
                .is_err());
            HostOutgoingDatagramStream::check_send(
                &mut view(&mut ctx, &mut table),
                borrow(&output),
            )
            .unwrap();
            assert_eq!(
                code(HostOutgoingDatagramStream::send(
                    &mut view(&mut ctx, &mut table),
                    borrow(&output),
                    vec![OutgoingDatagram {
                        data: vec![1],
                        remote_address: Some(other.local_addr().unwrap().into())
                    }]
                )),
                network::ErrorCode::InvalidArgument
            );
            other.send_to(b"wrong source", local).unwrap();
            source.send_to(data, local).unwrap();
            assert_eq!(receive(&mut ctx, &mut table, &input).data, data);
            let poll = HostIncomingDatagramStream::subscribe(
                &mut view(&mut ctx, &mut table),
                borrow(&input),
            )
            .unwrap();
            assert!(!HostPollable::ready(&mut table, borrow(&poll)).unwrap());
            assert!(HostIncomingDatagramStream::drop(
                &mut view(&mut ctx, &mut table),
                Resource::new_own(input.rep())
            )
            .is_err());
            HostPollable::drop(&mut table, poll).unwrap();
            drop_streams(&mut ctx, &mut table, input, output);
        }
        let (input, output) = view(&mut ctx, &mut table)
            .stream(borrow(&socket), None)
            .unwrap();
        assert_eq!(
            code(view(&mut ctx, &mut table).remote_address(borrow(&socket))),
            network::ErrorCode::InvalidState
        );
        a.send_to(b"unconnected", local).unwrap();
        assert_eq!(receive(&mut ctx, &mut table, &input).data, b"unconnected");
        assert_eq!(
            SocketAddr::from(
                view(&mut ctx, &mut table)
                    .local_address(borrow(&socket))
                    .unwrap()
            ),
            local
        );
        drop_streams(&mut ctx, &mut table, input, output);
        drop_socket(&mut ctx, &mut table, network, socket);
    });
}

#[test]
fn udp_wit_cleanup_with_full_task_budget() {
    bounded(|| {
        let mut ctx = ctx(1);
        let mut table = ResourceTable::new();
        let (network, socket) = make(&mut ctx, &mut table, "127.0.0.1:0".parse().unwrap());
        let (input, output) = view(&mut ctx, &mut table)
            .stream(borrow(&socket), None)
            .unwrap();
        let poll =
            HostIncomingDatagramStream::subscribe(&mut view(&mut ctx, &mut table), borrow(&input))
                .unwrap();
        assert!(!HostPollable::ready(&mut table, borrow(&poll)).unwrap());
        assert_eq!(ctx.outstanding_io(), 1);
        HostPollable::drop(&mut table, poll).unwrap();
        HostIncomingDatagramStream::drop(&mut view(&mut ctx, &mut table), input).unwrap();
        let occupied = ctx.driver.submit(|_| std::future::pending::<()>()).unwrap();
        HostOutgoingDatagramStream::drop(&mut view(&mut ctx, &mut table), output).unwrap();
        assert_eq!(ctx.outstanding_io(), 1);
        occupied.cancel();
        assert_eq!(
            futures_lite::future::block_on(occupied),
            Err(perry_component_io::Error::Cancelled)
        );
        drop_socket(&mut ctx, &mut table, network, socket);
    });
}

#[test]
fn udp_wit_driver_loss_stays_a_poll_and_receive_trap() {
    bounded(|| {
        let mut ctx = ctx(8);
        let mut table = ResourceTable::new();
        let (_network, socket) = make(&mut ctx, &mut table, "127.0.0.1:0".parse().unwrap());
        let (input, output) = view(&mut ctx, &mut table)
            .stream(borrow(&socket), None)
            .unwrap();
        let poll =
            HostIncomingDatagramStream::subscribe(&mut view(&mut ctx, &mut table), borrow(&input))
                .unwrap();
        assert!(!HostPollable::ready(&mut table, borrow(&poll)).unwrap());
        let output_poll =
            HostOutgoingDatagramStream::subscribe(&mut view(&mut ctx, &mut table), borrow(&output))
                .unwrap();
        assert_eq!(table.get_mut(&output).unwrap().check_send().unwrap(), 1);
        drop(ctx);
        let packet = super::OutgoingDatagram {
            data: vec![],
            remote_address: Some(SocketAddr::from(([127, 0, 0, 1], 7)).into()),
        };
        assert!(
            futures_lite::future::block_on(table.get_mut(&output).unwrap().send(vec![packet]))
                .unwrap_err()
                .downcast()
                .is_err()
        );
        for _ in 0..2 {
            assert!(HostPollable::block(&mut table, borrow(&output_poll))
                .unwrap_err()
                .to_string()
                .contains("Stopped"));
            assert!(table
                .get_mut(&output)
                .unwrap()
                .check_send()
                .unwrap_err()
                .downcast()
                .is_err());
            assert!(HostPollable::block(&mut table, borrow(&poll))
                .unwrap_err()
                .to_string()
                .contains("Stopped"));
            assert!(table
                .get_mut(&input)
                .unwrap()
                .receive(1)
                .unwrap_err()
                .downcast()
                .is_err());
        }
    });
}

struct StoreData {
    ctx: WasiCtx,
    table: ResourceTable,
}
impl WasiView for StoreData {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.ctx,
            table: &mut self.table,
        }
    }
}
#[test]
fn udp_wit_component_preserves_empty_packet_and_sender_in_both_linkers() {
    bounded(|| {
        for asynchronous in [false, true] {
            let peer = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            peer.set_read_timeout(Some(WAIT)).unwrap();
            let port = peer.local_addr().unwrap().port();
            let peer = std::thread::spawn(move || {
                let mut bytes = [0xcc; 8];
                let (n, from) = peer.recv_from(&mut bytes).unwrap();
                assert_eq!(n, 0, "empty guest payload is a real datagram");
                assert_eq!(bytes, [0xcc; 8]);
                peer.send_to(b"pong", from).unwrap();
            });
            let engine = wasmtime::Engine::default();
            let component =
                wasmtime::component::Component::new(&engine, include_str!("udp.component.wat"))
                    .unwrap();
            let mut store = wasmtime::Store::new(
                &engine,
                StoreData {
                    ctx: ctx(8),
                    table: ResourceTable::new(),
                },
            );
            let mut linker = wasmtime::component::Linker::new(&engine);
            let instance = if asynchronous {
                crate::p2::add_to_linker_async(&mut linker).unwrap();
                futures_lite::future::block_on(linker.instantiate_async(&mut store, &component))
                    .unwrap()
            } else {
                crate::p2::add_to_linker_sync(&mut linker).unwrap();
                linker.instantiate(&mut store, &component).unwrap()
            };
            let run = instance
                .get_typed_func::<(u32,), (u32,)>(&mut store, "run")
                .unwrap();
            let result = if asynchronous {
                futures_lite::future::block_on(run.call_async(&mut store, (u32::from(port),)))
                    .unwrap()
            } else {
                run.call(&mut store, (u32::from(port),)).unwrap()
            };
            assert_eq!(result, (4,));
            assert!(store.data().table.is_empty());
            peer.join().unwrap();
        }
    });
}

#[test]
fn udp_wit_unconnected_destinations_empty_batch_and_error_recovery() {
    bounded(|| {
        let a = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let b = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        for peer in [&a, &b] {
            peer.set_read_timeout(Some(WAIT)).unwrap();
        }
        let mut ctx = ctx(8);
        let mut table = ResourceTable::new();
        let (network, socket) = make(&mut ctx, &mut table, "127.0.0.1:0".parse().unwrap());
        let local = SocketAddr::from(
            view(&mut ctx, &mut table)
                .local_address(borrow(&socket))
                .unwrap(),
        );
        let (input, output) = view(&mut ctx, &mut table)
            .stream(borrow(&socket), None)
            .unwrap();
        let poll =
            HostOutgoingDatagramStream::subscribe(&mut view(&mut ctx, &mut table), borrow(&output))
                .unwrap();
        HostOutgoingDatagramStream::check_send(&mut view(&mut ctx, &mut table), borrow(&output))
            .unwrap();
        assert_eq!(
            HostOutgoingDatagramStream::send(
                &mut view(&mut ctx, &mut table),
                borrow(&output),
                vec![]
            )
            .unwrap(),
            0
        );
        assert!(HostOutgoingDatagramStream::send(
            &mut view(&mut ctx, &mut table),
            borrow(&output),
            vec![]
        )
        .unwrap_err()
        .downcast()
        .is_err());
        for (peer, bytes) in [(&a, b"one".as_slice()), (&b, b"two".as_slice())] {
            let remote = peer.local_addr().unwrap().into();
            for packet in [
                OutgoingDatagram {
                    data: vec![],
                    remote_address: None,
                },
                OutgoingDatagram {
                    data: vec![0; 65_508],
                    remote_address: Some(remote),
                },
            ] {
                HostOutgoingDatagramStream::check_send(
                    &mut view(&mut ctx, &mut table),
                    borrow(&output),
                )
                .unwrap();
                let expected = if packet.remote_address.is_none() {
                    network::ErrorCode::InvalidArgument
                } else {
                    network::ErrorCode::DatagramTooLarge
                };
                assert_eq!(
                    code(HostOutgoingDatagramStream::send(
                        &mut view(&mut ctx, &mut table),
                        borrow(&output),
                        vec![packet]
                    )),
                    expected
                );
            }
            HostOutgoingDatagramStream::check_send(
                &mut view(&mut ctx, &mut table),
                borrow(&output),
            )
            .unwrap();
            assert_eq!(
                HostOutgoingDatagramStream::send(
                    &mut view(&mut ctx, &mut table),
                    borrow(&output),
                    vec![OutgoingDatagram {
                        data: bytes.to_vec(),
                        remote_address: Some(remote)
                    }]
                )
                .unwrap(),
                1
            );
            HostPollable::block(&mut table, borrow(&poll)).unwrap();
            let mut actual = [0; 16];
            let (n, from) = peer.recv_from(&mut actual).unwrap();
            assert_eq!(from, local);
            assert_eq!(&actual[..n], bytes);
        }
        HostPollable::drop(&mut table, poll).unwrap();
        drop_streams(&mut ctx, &mut table, input, output);
        drop_socket(&mut ctx, &mut table, network, socket);
    });
}
