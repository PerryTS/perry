//! Exercise the configured WASI providers against local OS services. These
//! checks qualify provider policy, not Perry guest networking or host platforms.
use super::*;
use perry_wasi::p2::bindings::sockets::{
    instance_network, ip_name_lookup, network, udp, udp_create_socket,
};
use perry_wasi::sockets::WasiSocketsCtxView;
use perry_wasi::{
    io::poll::Pollable,
    p2::{udp::UdpSocket, SocketResult},
};
use std::net::UdpSocket as NativeUdpSocket;
use std::time::Duration;
use wasmtime::component::Resource;

fn borrow<T: 'static>(resource: &Resource<T>) -> Resource<T> {
    Resource::new_borrow(resource.rep())
}

fn view(state: &mut State) -> WasiSocketsCtxView<'_> {
    WasiSocketsCtxView {
        ctx: &mut state.ctx.sockets,
        table: &mut state.table,
    }
}

async fn bound_socket(view: &mut WasiSocketsCtxView<'_>) -> SocketResult<Resource<UdpSocket>> {
    let socket = udp_create_socket::Host::create_udp_socket(view, network::IpAddressFamily::Ipv4)?;
    let network = instance_network::Host::instance_network(view).unwrap();
    udp::HostUdpSocket::start_bind(
        view,
        borrow(&socket),
        network,
        "127.0.0.1:0".parse::<SocketAddr>().unwrap().into(),
    )
    .await?;
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
    futures_lite::future::block_on(async {
        let mut state = options.build().unwrap();
        let mut view = view(&mut state);
        let socket = bound_socket(&mut view)
            .await
            .map_err(|error| error.downcast().unwrap())?;
        udp::HostUdpSocket::drop(&mut view, socket).unwrap();
        Ok(())
    })
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
    futures_lite::future::block_on(async {
        let mut state = options.build().unwrap();
        let mut view = view(&mut state);
        let socket = bound_socket(&mut view).await.unwrap();
        let error = udp::HostUdpSocket::stream(&mut view, borrow(&socket), Some(remote.into()))
            .await
            .unwrap_err();
        assert_eq!(error.downcast().unwrap(), network::ErrorCode::AccessDenied);
        udp::HostUdpSocket::drop(&mut view, socket).unwrap();
    });

    options.network.addresses.push(grant(remote.port()));
    futures_lite::future::block_on(async {
        let mut state = options.build().unwrap();
        let mut view = view(&mut state);
        let socket = bound_socket(&mut view).await.unwrap();
        let local: SocketAddr = udp::HostUdpSocket::local_address(&mut view, borrow(&socket))
            .unwrap()
            .into();
        let (incoming, outgoing) =
            udp::HostUdpSocket::stream(&mut view, borrow(&socket), Some(remote.into()))
                .await
                .unwrap();
        view.table
            .get_mut(&outgoing)
            .unwrap()
            .ready()
            .await
            .unwrap();
        assert!(
            udp::HostOutgoingDatagramStream::check_send(&mut view, borrow(&outgoing)).unwrap() > 0
        );
        let count = udp::HostOutgoingDatagramStream::send(
            &mut view,
            borrow(&outgoing),
            vec![udp::OutgoingDatagram {
                data: b"perry-wasi-udp".to_vec(),
                remote_address: None,
            }],
        )
        .await
        .unwrap();
        assert_eq!(count, 1);
        // Dropping the stream waits for any acknowledged asynchronous send.
        udp::HostOutgoingDatagramStream::drop(&mut view, outgoing)
            .await
            .unwrap();
        let mut bytes = [0; 64];
        let (len, peer) = server.recv_from(&mut bytes).unwrap();
        assert_eq!(&bytes[..len], b"perry-wasi-udp");
        assert_eq!(peer, local);
        server.send_to(b"reply-from-loopback", peer).unwrap();
        view.table
            .get_mut(&incoming)
            .unwrap()
            .ready()
            .await
            .unwrap();
        let packets =
            udp::HostIncomingDatagramStream::receive(&mut view, borrow(&incoming), 1).unwrap();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].data, b"reply-from-loopback");
        assert_eq!(
            SocketAddr::from(packets.into_iter().next().unwrap().remote_address),
            remote
        );
        udp::HostIncomingDatagramStream::drop(&mut view, incoming)
            .await
            .unwrap();
        udp::HostUdpSocket::drop(&mut view, socket).unwrap();
    });
    assert_eq!(
        bind(&WasiOptions::default()),
        Err(network::ErrorCode::AccessDenied)
    );
}

fn resolve(options: &WasiOptions, name: &str) -> Result<Vec<IpAddr>, network::ErrorCode> {
    futures_lite::future::block_on(async {
        let mut state = options.build().unwrap();
        let mut view = view(&mut state);
        let network = instance_network::Host::instance_network(&mut view).unwrap();
        let stream = ip_name_lookup::Host::resolve_addresses(&mut view, network, name.into())
            .map_err(|error| error.downcast().unwrap())?;
        view.table.get_mut(&stream).unwrap().ready().await.unwrap();
        let mut addresses = Vec::new();
        while let Some(address) = ip_name_lookup::HostResolveAddressStream::resolve_next_address(
            &mut view,
            borrow(&stream),
        )
        .map_err(|error| error.downcast().unwrap())?
        {
            addresses.push(match address {
                network::IpAddress::Ipv4((a, b, c, d)) => IpAddr::V4([a, b, c, d].into()),
                network::IpAddress::Ipv6((a, b, c, d, e, f, g, h)) => {
                    IpAddr::V6(std::net::Ipv6Addr::new(a, b, c, d, e, f, g, h))
                }
            });
        }
        ip_name_lookup::HostResolveAddressStream::drop(&mut view, stream).unwrap();
        Ok(addresses)
    })
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
