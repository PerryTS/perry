use super::*;
use crate::WasiCtx;
use futures_lite::future::block_on;
use std::{
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};
const WAIT: Duration = Duration::from_secs(10);
fn bounded(test: impl FnOnce() + Send + 'static) {
    let (send, receive) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        test();
        send.send(()).unwrap();
    });
    receive.recv_timeout(WAIT).expect("UDP test timed out");
    thread.join().unwrap();
}
fn ctx() -> WasiCtx {
    let mut ctx = WasiCtx::new(8).unwrap();
    ctx.sockets
        .allow_udp(true)
        .socket_addr_check(|_, _| Box::pin(async { true }));
    ctx
}
fn code<T>(result: SocketResult<T>) -> ErrorCode {
    match result {
        Err(e) => e.downcast().unwrap(),
        Ok(_) => panic!("expected error"),
    }
}
fn send(connection: &UdpConnection, bytes: Vec<u8>, to: Option<SocketAddr>) {
    block_on(block_on(connection.send(bytes, to)).unwrap())
        .unwrap()
        .unwrap();
}
fn receive(connection: &UdpConnection) -> Datagram {
    block_on(connection.receive().unwrap()).unwrap().unwrap()
}

#[test]
fn udp_native_ipv4_ipv6_empty_full_and_following_packets() {
    bounded(|| {
        for (family, address) in [(Family::Ipv4, "127.0.0.1:0"), (Family::Ipv6, "[::1]:0")] {
            let peer = std::net::UdpSocket::bind(address).unwrap();
            peer.set_read_timeout(Some(WAIT)).unwrap();
            socket2::SockRef::from(&peer)
                .set_send_buffer_size(131_072)
                .unwrap();
            let remote = peer.local_addr().unwrap();
            let peer = std::thread::spawn(move || {
                for expected in [Vec::new(), vec![47; 65_507], b"after full packet".to_vec()] {
                    let mut bytes = vec![0xcc; 65_535];
                    let (n, from) = peer.recv_from(&mut bytes).unwrap();
                    assert_eq!(&bytes[..n], expected);
                    assert_eq!(peer.send_to(&bytes[..n], from).unwrap(), n);
                }
            });
            let ctx = ctx();
            let mut socket = UdpSocket::new(&ctx.sockets, family).unwrap();
            socket.set_send_buffer_size(131_072).unwrap();
            block_on(socket.bind(address.parse().unwrap())).unwrap();
            let local = socket.local_address().unwrap();
            let connection = block_on(socket.stream(Some(remote))).unwrap();
            assert_eq!(socket.remote_address().unwrap(), remote);
            assert_eq!(socket.local_address().unwrap(), local);
            for bytes in [Vec::new(), vec![47; 65_507], b"after full packet".to_vec()] {
                send(&connection, bytes.clone(), None);
                let received = receive(&connection);
                assert_eq!(received.data, bytes);
                assert_eq!(received.from, remote);
            }
            peer.join().unwrap();
        }
    });
}

#[test]
fn udp_native_capabilities_address_validation_and_exclusive_bind() {
    bounded(|| {
        let denied = WasiCtx::new(8).unwrap();
        assert_eq!(
            code(UdpSocket::new(&denied.sockets, Family::Ipv4)),
            ErrorCode::AccessDenied
        );
        let mut denied = denied;
        denied.sockets.allow_udp(true);
        let mut socket = UdpSocket::new(&denied.sockets, Family::Ipv4).unwrap();
        assert_eq!(
            code(block_on(socket.bind("127.0.0.1:0".parse().unwrap()))),
            ErrorCode::AccessDenied
        );
        assert_eq!(code(socket.local_address()), ErrorCode::InvalidState);
        let ctx = ctx();
        let mut socket = UdpSocket::new(&ctx.sockets, Family::Ipv4).unwrap();
        assert_eq!(code(block_on(socket.stream(None))), ErrorCode::InvalidState);
        assert_eq!(
            code(block_on(socket.bind("[::1]:0".parse().unwrap()))),
            ErrorCode::InvalidArgument
        );
        block_on(socket.bind("127.0.0.1:0".parse().unwrap())).unwrap();
        let address = socket.local_address().unwrap();
        assert_eq!(
            code(block_on(socket.bind(address))),
            ErrorCode::InvalidState
        );
        let mut duplicate = UdpSocket::new(&ctx.sockets, Family::Ipv4).unwrap();
        assert_eq!(
            code(block_on(duplicate.bind(address))),
            ErrorCode::AddressInUse
        );
        for invalid in [
            "127.0.0.1:0",
            "0.0.0.0:7",
            "[::1]:7",
            "224.0.0.1:7",
            "255.255.255.255:7",
        ] {
            assert_eq!(
                code(block_on(socket.stream(Some(invalid.parse().unwrap())))),
                ErrorCode::InvalidArgument
            );
        }
        let connection = block_on(socket.stream(None)).unwrap();
        assert_eq!(
            code(block_on(connection.send(vec![], None))),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(block_on(
                connection.send(vec![0; 65_508], Some("127.0.0.1:7".parse().unwrap()))
            )),
            ErrorCode::DatagramTooLarge
        );
        assert_eq!(code(socket.remote_address()), ErrorCode::InvalidState);
    });
}

#[test]
fn udp_native_peer_filter_reassociation_and_cancelled_receive() {
    bounded(|| {
        let ctx = ctx();
        let a = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let b = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let mut socket = UdpSocket::new(&ctx.sockets, Family::Ipv4).unwrap();
        block_on(socket.bind("127.0.0.1:0".parse().unwrap())).unwrap();
        let local = socket.local_address().unwrap();
        let first = block_on(socket.stream(Some(a.local_addr().unwrap()))).unwrap();
        let pending = first.receive().unwrap();
        b.send_to(b"wrong peer", local).unwrap();
        a.send_to(b"first peer", local).unwrap();
        assert_eq!(block_on(pending).unwrap().unwrap().data, b"first peer");
        assert_eq!(
            code(block_on(first.send(vec![], Some(b.local_addr().unwrap())))),
            ErrorCode::InvalidArgument
        );
        let pending = first.receive().unwrap();
        block_on(first.cancel_receive(Some(pending)).unwrap())
            .unwrap()
            .unwrap();
        drop(first);
        let second = block_on(socket.stream(Some(b.local_addr().unwrap()))).unwrap();
        a.send_to(b"old peer", local).unwrap();
        b.send_to(b"second peer", local).unwrap();
        assert_eq!(receive(&second).data, b"second peer");
        block_on(second.cancel_receive(None).unwrap())
            .unwrap()
            .unwrap();
        drop(second);
        let unconnected = block_on(socket.stream(None)).unwrap();
        assert_eq!(socket.local_address().unwrap(), local);
        assert_eq!(code(socket.remote_address()), ErrorCode::InvalidState);
        a.send_to(b"any peer", local).unwrap();
        let packet = receive(&unconnected);
        assert_eq!(packet.data, b"any peer");
        assert_eq!(packet.from, a.local_addr().unwrap());
    });
}

#[test]
fn udp_native_policy_callbacks_stay_on_caller_and_gate_destinations() {
    bounded(|| {
        let peer = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let remote = peer.local_addr().unwrap();
        let thread = std::thread::current().id();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = calls.clone();
        let mut ctx = ctx();
        ctx.sockets.socket_addr_check(move |address, usage| {
            assert_eq!(std::thread::current().id(), thread);
            observed.lock().unwrap().push((address, usage));
            Box::pin(async move {
                assert_eq!(std::thread::current().id(), thread);
                usage != SocketAddrUse::UdpOutgoingDatagram
            })
        });
        let mut socket = UdpSocket::new(&ctx.sockets, Family::Ipv4).unwrap();
        block_on(socket.bind("127.0.0.1:0".parse().unwrap())).unwrap();
        let connection = block_on(socket.stream(Some(remote))).unwrap();
        assert_eq!(
            code(block_on(connection.send(b"denied".to_vec(), None))),
            ErrorCode::AccessDenied
        );
        assert_eq!(
            calls
                .lock()
                .unwrap()
                .iter()
                .map(|(_, usage)| *usage)
                .collect::<Vec<_>>(),
            [
                SocketAddrUse::UdpBind,
                SocketAddrUse::UdpConnect,
                SocketAddrUse::UdpOutgoingDatagram
            ]
        );
        assert_eq!(ctx.outstanding_io(), 0);
    });
}

#[test]
fn udp_native_options_survive_binding_and_driver_shutdown() {
    bounded(|| {
        let ctx = ctx();
        let mut socket = UdpSocket::new(&ctx.sockets, Family::Ipv6).unwrap();
        assert_eq!(code(socket.set_hop_limit(0)), ErrorCode::InvalidArgument);
        assert_eq!(
            code(socket.set_receive_buffer_size(0)),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(socket.set_send_buffer_size(0)),
            ErrorCode::InvalidArgument
        );
        socket.set_hop_limit(31).unwrap();
        socket.set_receive_buffer_size(32_768).unwrap();
        socket.set_send_buffer_size(32_768).unwrap();
        let before = (
            socket.hop_limit().unwrap(),
            socket.receive_buffer_size().unwrap(),
            socket.send_buffer_size().unwrap(),
        );
        block_on(socket.bind("[::1]:0".parse().unwrap())).unwrap();
        assert_eq!(
            before,
            (
                socket.hop_limit().unwrap(),
                socket.receive_buffer_size().unwrap(),
                socket.send_buffer_size().unwrap()
            )
        );
        socket.set_hop_limit(33).unwrap();
        assert_eq!(socket.hop_limit().unwrap(), 33);
        let connection = block_on(socket.stream(None)).unwrap();
        let request = connection.receive().unwrap();
        drop(ctx);
        assert!(matches!(
            block_on(request),
            Err(perry_component_io::Error::Stopped)
        ));
        assert!(matches!(
            connection.receive(),
            Err(perry_component_io::Error::Stopped)
        ));
    });
}
