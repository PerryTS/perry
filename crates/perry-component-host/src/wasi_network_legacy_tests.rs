//! Exercise the configured WASI providers against local OS services. These
//! checks qualify provider policy, not Perry guest networking or host platforms.
use super::*;
use crate::sync_wasi::bindings::wasi::io::poll::HostPollable;
use crate::sync_wasi::bindings::wasi::sockets::{
    instance_network, ip_name_lookup, network, udp, udp_create_socket,
};
use crate::sync_wasi::State;
use crate::sync_wasi::{
    network::SocketResult,
    sockets::{decode, encode},
    udp::UdpSocket,
};
use std::net::UdpSocket as NativeUdpSocket;
use std::time::Duration;
use wasmtime::component::Resource;

fn borrow<T: 'static>(resource: &Resource<T>) -> Resource<T> {
    Resource::new_borrow(resource.rep())
}

fn bound_socket(view: &mut State) -> SocketResult<Resource<UdpSocket>> {
    let socket = udp_create_socket::Host::create_udp_socket(view, network::IpAddressFamily::Ipv4)?;
    let network = instance_network::Host::instance_network(view).unwrap();
    udp::HostUdpSocket::start_bind(
        view,
        borrow(&socket),
        network,
        encode("127.0.0.1:0".parse::<SocketAddr>().unwrap()),
    )?;
    udp::HostUdpSocket::finish_bind(view, borrow(&socket))?;
    Ok(socket)
}

fn grant(port: u16) -> Address {
    Address {
        address: "127.0.0.1".parse().unwrap(),
        port: Some(port),
    }
}

fn bind(options: &WasiOptions) -> Result<(), network::ErrorCode> {
    {
        let mut state = State::new(&options).unwrap();
        let view = &mut state;
        let socket = bound_socket(view).map_err(|error| error.downcast().unwrap())?;
        udp::HostUdpSocket::drop(view, socket).unwrap();
        Ok(())
    }
}

#[test]
fn udp_requires_its_own_protocol_and_exact_local_address_grant() {
    let mut options = WasiOptions::default();
    assert_eq!(bind(&options), Err(network::ErrorCode::AccessDenied));
    options.network.tcp = true;
    options.network.addresses.push(grant(0));
    assert_eq!(bind(&options), Err(network::ErrorCode::AccessDenied));
    options.network.udp = true;
    options.network.addresses.clear();
    assert_eq!(bind(&options), Err(network::ErrorCode::AccessDenied));
    options.network.addresses.push(grant(1));
    assert_eq!(bind(&options), Err(network::ErrorCode::AccessDenied));
    options.network.addresses[0] = grant(0);
    options.network.addresses[0].address = "127.0.0.2".parse().unwrap();
    assert_eq!(bind(&options), Err(network::ErrorCode::AccessDenied));
    options.network.addresses[0] = grant(0);
    assert_eq!(bind(&options), Ok(()));
    assert_eq!(
        bind(&WasiOptions::default()),
        Err(network::ErrorCode::AccessDenied)
    );
    assert_eq!(bind(&options), Ok(()));
}

#[test]
fn udp_remote_grant_controls_real_bidirectional_datagrams() {
    let server = NativeUdpSocket::bind("127.0.0.1:0").unwrap();
    server
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let remote = server.local_addr().unwrap();
    let mut options = WasiOptions::default();
    options.network.udp = true;
    options.network.addresses.push(grant(0));

    // Binding a local socket does not grant a remote endpoint.
    {
        let mut state = State::new(&options).unwrap();
        let view = &mut state;
        let socket = bound_socket(view).unwrap();
        let error =
            udp::HostUdpSocket::stream(view, borrow(&socket), Some(encode(remote))).unwrap_err();
        assert_eq!(error.downcast().unwrap(), network::ErrorCode::AccessDenied);
        udp::HostUdpSocket::drop(view, socket).unwrap();
    };

    options.network.addresses.push(grant(remote.port()));
    {
        let mut state = State::new(&options).unwrap();
        let view = &mut state;
        let socket = bound_socket(view).unwrap();
        let local = decode(udp::HostUdpSocket::local_address(view, borrow(&socket)).unwrap());
        let (incoming, outgoing) =
            udp::HostUdpSocket::stream(view, borrow(&socket), Some(encode(remote))).unwrap();
        let pollable = udp::HostOutgoingDatagramStream::subscribe(view, borrow(&outgoing)).unwrap();
        HostPollable::block(view, borrow(&pollable)).unwrap();
        HostPollable::drop(view, pollable).unwrap();
        assert!(udp::HostOutgoingDatagramStream::check_send(view, borrow(&outgoing)).unwrap() > 0);
        let count = udp::HostOutgoingDatagramStream::send(
            view,
            borrow(&outgoing),
            vec![udp::OutgoingDatagram {
                data: b"perry-wasi-udp".to_vec(),
                remote_address: None,
            }],
        )
        .unwrap();
        assert_eq!(count, 1);
        // Dropping the stream waits for any acknowledged asynchronous send.
        udp::HostOutgoingDatagramStream::drop(view, outgoing).unwrap();
        let mut bytes = [0; 64];
        let (len, peer) = server.recv_from(&mut bytes).unwrap();
        assert_eq!(&bytes[..len], b"perry-wasi-udp");
        assert_eq!(peer, local);
        server.send_to(b"reply-from-loopback", peer).unwrap();
        let pollable = udp::HostIncomingDatagramStream::subscribe(view, borrow(&incoming)).unwrap();
        HostPollable::block(view, borrow(&pollable)).unwrap();
        HostPollable::drop(view, pollable).unwrap();
        let packets = udp::HostIncomingDatagramStream::receive(view, borrow(&incoming), 1).unwrap();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].data, b"reply-from-loopback");
        assert_eq!(
            decode(packets.into_iter().next().unwrap().remote_address),
            remote
        );
        udp::HostIncomingDatagramStream::drop(view, incoming).unwrap();
        udp::HostUdpSocket::drop(view, socket).unwrap();
    };
    assert_eq!(
        bind(&WasiOptions::default()),
        Err(network::ErrorCode::AccessDenied)
    );
}

fn resolve(options: &WasiOptions, name: &str) -> Result<Vec<IpAddr>, network::ErrorCode> {
    {
        let mut state = State::new(&options).unwrap();
        let view = &mut state;
        let network = instance_network::Host::instance_network(view).unwrap();
        let stream = ip_name_lookup::Host::resolve_addresses(view, network, name.into())
            .map_err(|error| error.downcast().unwrap())?;
        let pollable =
            ip_name_lookup::HostResolveAddressStream::subscribe(view, borrow(&stream)).unwrap();
        HostPollable::block(view, borrow(&pollable)).unwrap();
        HostPollable::drop(view, pollable).unwrap();
        let mut addresses = Vec::new();
        while let Some(address) =
            ip_name_lookup::HostResolveAddressStream::resolve_next_address(view, borrow(&stream))
                .map_err(|error| error.downcast().unwrap())?
        {
            addresses.push(match address {
                network::IpAddress::Ipv4((a, b, c, d)) => IpAddr::V4([a, b, c, d].into()),
                network::IpAddress::Ipv6((a, b, c, d, e, f, g, h)) => {
                    IpAddr::V6(std::net::Ipv6Addr::new(a, b, c, d, e, f, g, h))
                }
            });
        }
        ip_name_lookup::HostResolveAddressStream::drop(view, stream).unwrap();
        Ok(addresses)
    }
}

#[test]
fn dns_grant_is_separate_from_socket_grants_and_other_contexts() {
    let mut options = WasiOptions::default();
    options.network.tcp = true;
    options.network.udp = true;
    options.network.addresses.push(grant(0));
    assert_eq!(
        resolve(&options, "localhost"),
        Err(network::ErrorCode::PermanentResolverFailure)
    );
    options = WasiOptions::default();
    options.network.dns = true;
    for _ in 0..2 {
        let addresses = resolve(&options, "localhost").unwrap();
        assert!(!addresses.is_empty());
        assert!(addresses.iter().all(IpAddr::is_loopback));
        assert_eq!(
            resolve(&WasiOptions::default(), "localhost"),
            Err(network::ErrorCode::PermanentResolverFailure)
        );
    }
    assert_eq!(
        resolve(&options, "127.0.0.1").unwrap(),
        vec!["127.0.0.1".parse::<IpAddr>().unwrap()]
    );
    assert_eq!(
        resolve(&options, "invalid name"),
        Err(network::ErrorCode::InvalidArgument)
    );
    assert_eq!(bind(&options), Err(network::ErrorCode::AccessDenied));
}
