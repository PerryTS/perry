wit_bindgen::generate!({ path: "..", world: "plugin", generate_all });

use exports::perry::network_probe::probe::Guest;
use wasi::sockets::{instance_network, ip_name_lookup, network, udp, udp_create_socket};

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
