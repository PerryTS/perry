wit_bindgen::generate!({ path: "..", world: "plugin", generate_all });

use exports::perry::network_probe::probe::Guest;
use wasi::sockets::{
    instance_network, ip_name_lookup, network, tcp_create_socket, udp, udp_create_socket,
};

struct Probe;

fn error(error: network::ErrorCode) -> String {
    format!("{error:?}")
}

fn address(port: u16) -> network::IpSocketAddress {
    network::IpSocketAddress::Ipv4(network::Ipv4SocketAddress {
        port,
        address: (127, 0, 0, 1),
    })
}

impl Guest for Probe {
    fn connect(port: u16, payload: String) -> Result<String, String> {
        let network = instance_network::instance_network();
        let socket =
            tcp_create_socket::create_tcp_socket(network::IpAddressFamily::Ipv4).map_err(error)?;
        socket
            .start_connect(&network, address(port))
            .map_err(error)?;
        let (input, output) = loop {
            match socket.finish_connect() {
                Ok(streams) => break streams,
                Err(network::ErrorCode::WouldBlock) => socket.subscribe().block(),
                Err(e) => return Err(error(e)),
            }
        };
        match socket.remote_address().map_err(error)? {
            network::IpSocketAddress::Ipv4(remote)
                if remote.address == (127, 0, 0, 1) && remote.port == port => {}
            _ => return Err("unexpected TCP peer".into()),
        }
        output
            .blocking_write_and_flush(format!("{payload}\n").as_bytes())
            .map_err(|e| format!("TCP write: {e:?}"))?;
        let mut response = Vec::new();
        loop {
            response.extend(
                input
                    .blocking_read(256)
                    .map_err(|e| format!("TCP read: {e:?}"))?,
            );
            if response.len() > 256 {
                return Err("oversized TCP response".into());
            }
            if response.last() == Some(&b'\n') {
                response.pop();
                return String::from_utf8(response).map_err(|_| "non-UTF8 TCP response".into());
            }
        }
    }
    fn resolve(name: String) -> Result<Vec<String>, String> {
        let network = instance_network::instance_network();
        let stream = ip_name_lookup::resolve_addresses(&network, &name).map_err(error)?;
        let mut addresses = Vec::new();
        loop {
            match stream.resolve_next_address() {
                Ok(Some(network::IpAddress::Ipv4((a, b, c, d)))) => {
                    addresses.push(format!("{a}.{b}.{c}.{d}"));
                }
                Ok(Some(network::IpAddress::Ipv6((a, b, c, d, e, f, g, h)))) => {
                    addresses.push(format!("{a:x}:{b:x}:{c:x}:{d:x}:{e:x}:{f:x}:{g:x}:{h:x}"));
                }
                Ok(None) => return Ok(addresses),
                Err(network::ErrorCode::WouldBlock) => stream.subscribe().block(),
                Err(e) => return Err(error(e)),
            }
        }
    }

    fn exchange(port: u16, connected: bool, payload: String) -> Result<String, String> {
        let network = instance_network::instance_network();
        let socket =
            udp_create_socket::create_udp_socket(network::IpAddressFamily::Ipv4).map_err(error)?;
        socket.start_bind(&network, address(0)).map_err(error)?;
        loop {
            match socket.finish_bind() {
                Ok(()) => break,
                Err(network::ErrorCode::WouldBlock) => socket.subscribe().block(),
                Err(e) => return Err(error(e)),
            }
        }
        let remote = address(port);
        let (incoming, outgoing) = socket
            .stream(if connected { Some(remote) } else { None })
            .map_err(error)?;
        let datagram = udp::OutgoingDatagram {
            data: payload.into_bytes(),
            remote_address: if connected { None } else { Some(remote) },
        };
        loop {
            if outgoing.check_send().map_err(error)? == 0 {
                outgoing.subscribe().block();
                continue;
            }
            if outgoing
                .send(std::slice::from_ref(&datagram))
                .map_err(error)?
                == 1
            {
                break;
            }
            outgoing.subscribe().block();
        }
        // Sending may queue work. Observe its completion/error before waiting
        // for a reply, including a denied destination on an unconnected socket.
        outgoing.subscribe().block();
        outgoing.check_send().map_err(error)?;
        loop {
            let packets = incoming.receive(1).map_err(error)?;
            if let Some(packet) = packets.into_iter().next() {
                match packet.remote_address {
                    network::IpSocketAddress::Ipv4(source)
                        if source.address == (127, 0, 0, 1) && source.port == port => {}
                    _ => return Err("unexpected UDP response source".into()),
                }
                return String::from_utf8(packet.data).map_err(|_| "non-UTF8 UDP response".into());
            }
            incoming.subscribe().block();
        }
    }
}

export!(Probe);
