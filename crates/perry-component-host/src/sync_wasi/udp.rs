use super::{
    bindings::wasi::sockets::{
        network::{ErrorCode, IpAddressFamily, IpSocketAddress},
        udp, udp_create_socket,
    },
    native_io::{Client, Operation, Socket},
    network::{Network, SocketError, SocketResult},
    sockets::*,
    streams::Pollable,
    State,
};
use perry_abi::component_io as abi;
use std::sync::Arc;
use wasmtime::component::Resource;

enum Phase {
    Unbound,
    Binding,
    Bound,
}
pub struct UdpSocket {
    owner: SocketOwner,
    family: IpAddressFamily,
    phase: Phase,
    network: crate::Network,
}
pub enum Receive {
    Idle,
    Pending(Operation),
    Ready(Result<udp::IncomingDatagram, ErrorCode>),
}
pub struct Incoming {
    socket: Arc<Socket>,
    network: crate::Network,
    pub state: Receive,
}
pub struct Outgoing {
    socket: Arc<Socket>,
    network: crate::Network,
    family: IpAddressFamily,
    pub pending: Option<Operation>,
    permit: u64,
}
fn code(e: SocketError) -> ErrorCode {
    match e {
        SocketError::Code(c) => c,
        _ => ErrorCode::Unknown,
    }
}
fn completion_error(error: abi::Error) -> ErrorCode {
    code(super::native_io::Error(error).into())
}
impl Incoming {
    pub fn prepare(&mut self, io: Client) -> wasmtime::Result<()> {
        for _ in 0..16 {
            if matches!(self.state, Receive::Idle) {
                self.state =
                    match io.submit(Some(self.socket.clone()), abi::RECEIVE, &[], 65535, None, 0) {
                        Ok(op) => Receive::Pending(op),
                        Err(e) => Receive::Ready(Err(code(e.into()))),
                    };
            }
            let Receive::Pending(op) = &self.state else {
                return Ok(());
            };
            io.poll(&[op], false)?;
            if !op.ready() {
                return Ok(());
            }
            let Receive::Pending(op) = std::mem::replace(&mut self.state, Receive::Idle) else {
                unreachable!()
            };
            let result = match op.finish() {
                Err(e) => Err(code(e.into())),
                Ok(c) if c.error.kind != 0 => Err(completion_error(c.error)),
                Ok(c) => {
                    let from = from_native(c.address);
                    if !permitted(&self.network, from) {
                        continue;
                    }
                    Ok(udp::IncomingDatagram {
                        data: c.bytes,
                        remote_address: encode(from),
                    })
                }
            };
            self.state = Receive::Ready(result);
            return Ok(());
        }
        // Leave one operation armed even after a burst of denied packets.
        if matches!(self.state, Receive::Idle) {
            self.state =
                match io.submit(Some(self.socket.clone()), abi::RECEIVE, &[], 65535, None, 0) {
                    Ok(op) => Receive::Pending(op),
                    Err(e) => Receive::Ready(Err(code(e.into()))),
                };
        }
        Ok(())
    }
}
impl Outgoing {
    fn finish(&mut self, io: Client) -> SocketResult<bool> {
        if let Some(op) = &self.pending {
            io.poll(&[op], false)?;
            if !op.ready() {
                return Ok(false);
            }
        } else {
            return Ok(true);
        }
        let c = self.pending.take().unwrap().finish()?;
        if c.error.kind != 0 {
            return Err(completion_error(c.error).into());
        }
        Ok(true)
    }
}
impl udp::Host for State {}
impl udp_create_socket::Host for State {
    fn create_udp_socket(&mut self, family: IpAddressFamily) -> SocketResult<Resource<UdpSocket>> {
        if !self.options.network.udp {
            return Err(ErrorCode::AccessDenied.into());
        }
        Ok(self.table.push(UdpSocket {
            owner: SocketOwner::Owned(new_socket(family, true)?),
            family,
            phase: Phase::Unbound,
            network: self.options.network.clone(),
        })?)
    }
}
impl udp::HostUdpSocket for State {
    fn start_bind(
        &mut self,
        socket: Resource<UdpSocket>,
        network: Resource<Network>,
        address: IpSocketAddress,
    ) -> SocketResult<()> {
        self.table.get(&network)?;
        let s = self.table.get_mut(&socket)?;
        if matches!(s.phase, Phase::Binding) {
            return Err(ErrorCode::ConcurrencyConflict.into());
        }
        if !matches!(s.phase, Phase::Unbound) {
            return Err(ErrorCode::InvalidState.into());
        }
        let address = decode(address);
        validate(address, s.family, false)?;
        permit(&s.network, address)?;
        s.owner.owned()?.bind(&address.into())?;
        s.phase = Phase::Binding;
        Ok(())
    }
    fn finish_bind(&mut self, socket: Resource<UdpSocket>) -> SocketResult<()> {
        let s = self.table.get_mut(&socket)?;
        if !matches!(s.phase, Phase::Binding) {
            return Err(ErrorCode::NotInProgress.into());
        }
        s.phase = Phase::Bound;
        Ok(())
    }
    fn stream(
        &mut self,
        socket: Resource<UdpSocket>,
        remote: Option<IpSocketAddress>,
    ) -> SocketResult<(Resource<Incoming>, Resource<Outgoing>)> {
        if self
            .table
            .iter_children(&socket)?
            .any(|c| c.is::<Incoming>() || c.is::<Outgoing>())
        {
            return Err(SocketError::Trap(wasmtime::format_err!(
                "UDP streams not dropped yet"
            )));
        }
        let s = self.table.get_mut(&socket)?;
        if matches!(s.phase, Phase::Unbound) {
            return Err(ErrorCode::InvalidState.into());
        }
        let remote = remote.map(decode);
        if let Some(a) = remote {
            validate(a, s.family, true)?;
            permit(&s.network, a)?;
        }
        let owned = s.owner.owned()?;
        if let Some(a) = remote {
            match owned.connect(&a.into()) {
                #[cfg(target_os = "linux")]
                Err(e) if e.raw_os_error() == Some(libc::EINVAL) => {
                    disconnect(owned)?;
                    owned.connect(&a.into())?;
                }
                result => result?,
            }
        } else if owned.peer_addr().is_ok() {
            disconnect(owned)?;
        }
        let native = s.owner.drive(self.io)?;
        let network = s.network.clone();
        let family = s.family;
        Ok((
            self.table.push_child(
                Incoming {
                    socket: native.clone(),
                    network: network.clone(),
                    state: Receive::Idle,
                },
                &socket,
            )?,
            self.table.push_child(
                Outgoing {
                    socket: native,
                    network,
                    family,
                    pending: None,
                    permit: 0,
                },
                &socket,
            )?,
        ))
    }
    fn local_address(&mut self, socket: Resource<UdpSocket>) -> SocketResult<IpSocketAddress> {
        let s = self.table.get(&socket)?;
        if matches!(s.phase, Phase::Unbound) {
            return Err(ErrorCode::InvalidState.into());
        }
        Ok(encode(endpoint(&s.owner, false)?))
    }
    fn remote_address(&mut self, socket: Resource<UdpSocket>) -> SocketResult<IpSocketAddress> {
        let s = self.table.get(&socket)?;
        endpoint(&s.owner, true)
            .map(encode)
            .map_err(|_| ErrorCode::InvalidState.into())
    }
    fn address_family(&mut self, socket: Resource<UdpSocket>) -> wasmtime::Result<IpAddressFamily> {
        Ok(self.table.get(&socket)?.family)
    }
    fn unicast_hop_limit(&mut self, socket: Resource<UdpSocket>) -> SocketResult<u8> {
        let s = self.table.get(&socket)?;
        Ok(s.owner.option(abi::HOP_LIMIT, None, s.family)? as u8)
    }
    fn set_unicast_hop_limit(
        &mut self,
        socket: Resource<UdpSocket>,
        value: u8,
    ) -> SocketResult<()> {
        let s = self.table.get(&socket)?;
        s.owner
            .option(abi::HOP_LIMIT, Some(value as u64), s.family)?;
        Ok(())
    }
    fn receive_buffer_size(&mut self, socket: Resource<UdpSocket>) -> SocketResult<u64> {
        let s = self.table.get(&socket)?;
        s.owner.option(abi::RECEIVE_BUFFER, None, s.family)
    }
    fn set_receive_buffer_size(
        &mut self,
        socket: Resource<UdpSocket>,
        value: u64,
    ) -> SocketResult<()> {
        let s = self.table.get(&socket)?;
        s.owner.option(abi::RECEIVE_BUFFER, Some(value), s.family)?;
        Ok(())
    }
    fn send_buffer_size(&mut self, socket: Resource<UdpSocket>) -> SocketResult<u64> {
        let s = self.table.get(&socket)?;
        s.owner.option(abi::SEND_BUFFER, None, s.family)
    }
    fn set_send_buffer_size(
        &mut self,
        socket: Resource<UdpSocket>,
        value: u64,
    ) -> SocketResult<()> {
        let s = self.table.get(&socket)?;
        s.owner.option(abi::SEND_BUFFER, Some(value), s.family)?;
        Ok(())
    }
    fn subscribe(&mut self, socket: Resource<UdpSocket>) -> wasmtime::Result<Resource<Pollable>> {
        Ok(self.table.push_child(Pollable::Always, &socket)?)
    }
    fn drop(&mut self, socket: Resource<UdpSocket>) -> wasmtime::Result<()> {
        self.table.delete(socket)?;
        Ok(())
    }
}
impl udp::HostIncomingDatagramStream for State {
    fn receive(
        &mut self,
        stream: Resource<Incoming>,
        max: u64,
    ) -> SocketResult<Vec<udp::IncomingDatagram>> {
        let incoming = self.table.get_mut(&stream)?;
        let mut results = Vec::new();
        for _ in 0..max.min(16) {
            incoming.prepare(self.io)?;
            if !matches!(incoming.state, Receive::Ready(_)) {
                break;
            }
            let Receive::Ready(result) = std::mem::replace(&mut incoming.state, Receive::Idle)
            else {
                unreachable!()
            };
            match result {
                Ok(d) => results.push(d),
                Err(e) if results.is_empty() => return Err(e.into()),
                Err(_) => break,
            }
        }
        Ok(results)
    }
    fn subscribe(&mut self, stream: Resource<Incoming>) -> wasmtime::Result<Resource<Pollable>> {
        Ok(self
            .table
            .push_child(Pollable::UdpReceive(stream.rep()), &stream)?)
    }
    fn drop(&mut self, stream: Resource<Incoming>) -> wasmtime::Result<()> {
        self.table.delete(stream)?;
        Ok(())
    }
}
impl udp::HostOutgoingDatagramStream for State {
    fn check_send(&mut self, stream: Resource<Outgoing>) -> SocketResult<u64> {
        let s = self.table.get_mut(&stream)?;
        s.permit = 0;
        if s.finish(self.io)? {
            s.permit = 1
        }
        Ok(s.permit)
    }
    fn send(
        &mut self,
        stream: Resource<Outgoing>,
        datagrams: Vec<udp::OutgoingDatagram>,
    ) -> SocketResult<u64> {
        let s = self.table.get_mut(&stream)?;
        if datagrams.is_empty() {
            return Ok(0);
        }
        if datagrams.len() as u64 > s.permit {
            return Err(SocketError::Trap(wasmtime::format_err!(
                "send exceeds check-send permit"
            )));
        }
        s.permit = 0;
        let datagram = datagrams.into_iter().next().unwrap();
        let connected = query(&s.socket, |s| s.peer_addr())
            .ok()
            .and_then(|a| a.as_socket());
        let remote = match (datagram.remote_address.map(decode), connected) {
            (Some(a), Some(b)) if a != b => return Err(ErrorCode::InvalidArgument.into()),
            (Some(a), _) | (_, Some(a)) => a,
            _ => return Err(ErrorCode::InvalidArgument.into()),
        };
        validate(remote, s.family, true)?;
        permit(&s.network, remote)?;
        if datagram.data.len() > 65535 {
            return Err(ErrorCode::DatagramTooLarge.into());
        }
        s.pending = Some(self.io.submit(
            Some(s.socket.clone()),
            abi::SEND,
            &datagram.data,
            datagram.data.len(),
            Some(native_address(remote)),
            0,
        )?);
        // Submission owns the entire datagram; completion or its error is
        // observed by check-send and the outgoing stream's pollable.
        Ok(1)
    }
    fn subscribe(&mut self, stream: Resource<Outgoing>) -> wasmtime::Result<Resource<Pollable>> {
        Ok(self
            .table
            .push_child(Pollable::UdpSend(stream.rep()), &stream)?)
    }
    fn drop(&mut self, stream: Resource<Outgoing>) -> wasmtime::Result<()> {
        let mut stream = self.table.delete(stream)?;
        if let Some(op) = stream.pending.take() {
            while !op.ready() {
                self.io.poll(&[&op], true)?;
            }
            let _ = op.finish();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::bindings::wasi::{
        io::poll::{Host as P, HostPollable as H},
        sockets::instance_network::Host as N,
    };
    use super::*;
    use std::{net::UdpSocket as StdUdp, time::Duration};
    use udp::{
        HostIncomingDatagramStream as I, HostOutgoingDatagramStream as O, HostUdpSocket as U,
    };
    use udp_create_socket::Host as C;
    fn borrow<T>(r: &Resource<T>) -> Resource<T> {
        Resource::new_borrow(r.rep())
    }
    fn wait(s: &mut State, p: &Resource<Pollable>) {
        let deadline = s
            .table
            .push(Pollable::Operation(s.io.timer(5_000_000_000).unwrap()))
            .unwrap();
        let ready = P::poll(s, vec![borrow(p), borrow(&deadline)]).unwrap();
        assert!(ready.contains(&0), "UDP operation timed out");
        H::drop(s, deadline).unwrap();
    }
    #[test]
    fn udp_grants_datagrams_permits_and_reassociation() {
        let peer = StdUdp::bind("127.0.0.1:0").unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let denied = StdUdp::bind("127.0.0.1:0").unwrap();
        let mut s = State::new(&crate::WasiOptions {
            network: crate::Network {
                udp: true,
                addresses: vec![
                    crate::Address {
                        address: "127.0.0.1".parse().unwrap(),
                        port: Some(0),
                    },
                    crate::Address {
                        address: "127.0.0.1".parse().unwrap(),
                        port: Some(peer.local_addr().unwrap().port()),
                    },
                ],
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
        let socket = C::create_udp_socket(&mut s, IpAddressFamily::Ipv4).unwrap();
        let net = N::instance_network(&mut s).unwrap();
        assert!(matches!(
            U::local_address(&mut s, borrow(&socket)),
            Err(SocketError::Code(ErrorCode::InvalidState))
        ));
        U::start_bind(
            &mut s,
            borrow(&socket),
            net,
            encode("127.0.0.1:0".parse().unwrap()),
        )
        .unwrap();
        U::finish_bind(&mut s, borrow(&socket)).unwrap();
        let address = decode(U::local_address(&mut s, borrow(&socket)).unwrap());
        let (input, output) = U::stream(&mut s, borrow(&socket), None).unwrap();
        assert!(
            U::stream(&mut s, borrow(&socket), None).is_err(),
            "live streams must prevent reassociation"
        );
        assert!(
            O::send(
                &mut s,
                borrow(&output),
                vec![udp::OutgoingDatagram {
                    data: vec![1],
                    remote_address: Some(encode(peer.local_addr().unwrap()))
                }]
            )
            .is_err(),
            "check-send permit required"
        );
        denied.send_to(b"denied", address).unwrap();
        assert!(I::receive(&mut s, borrow(&input), 1).unwrap().is_empty());
        let readable = I::subscribe(&mut s, borrow(&input)).unwrap();
        assert!(
            !H::ready(&mut s, borrow(&readable)).unwrap(),
            "forbidden packet is not readable"
        );
        peer.send_to(b"allowed", address).unwrap();
        wait(&mut s, &readable);
        let packets = I::receive(&mut s, borrow(&input), 16).unwrap();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].data, b"allowed");
        assert_eq!(
            decode(packets[0].remote_address),
            peer.local_addr().unwrap()
        );
        assert_eq!(O::check_send(&mut s, borrow(&output)).unwrap(), 1);
        O::send(
            &mut s,
            borrow(&output),
            vec![udp::OutgoingDatagram {
                data: b"reply".to_vec(),
                remote_address: Some(encode(peer.local_addr().unwrap())),
            }],
        )
        .unwrap();
        let writable = O::subscribe(&mut s, borrow(&output)).unwrap();
        wait(&mut s, &writable);
        assert_eq!(O::check_send(&mut s, borrow(&output)).unwrap(), 1);
        let mut bytes = [0; 32];
        let (n, source) = peer.recv_from(&mut bytes).unwrap();
        assert_eq!(&bytes[..n], b"reply");
        assert_eq!(source, address);
        H::drop(&mut s, readable).unwrap();
        H::drop(&mut s, writable).unwrap();
        I::drop(&mut s, input).unwrap();
        O::drop(&mut s, output).unwrap();
        let (input, output) = U::stream(
            &mut s,
            borrow(&socket),
            Some(encode(peer.local_addr().unwrap())),
        )
        .unwrap();
        assert_eq!(
            decode(U::remote_address(&mut s, borrow(&socket)).unwrap()),
            peer.local_addr().unwrap()
        );
        I::drop(&mut s, input).unwrap();
        O::drop(&mut s, output).unwrap();
        let (input, output) = U::stream(&mut s, borrow(&socket), None).unwrap();
        assert!(matches!(
            U::remote_address(&mut s, borrow(&socket)),
            Err(SocketError::Code(ErrorCode::InvalidState))
        ));
        U::set_unicast_hop_limit(&mut s, borrow(&socket), 42).unwrap();
        assert_eq!(U::unicast_hop_limit(&mut s, borrow(&socket)).unwrap(), 42);
        I::drop(&mut s, input).unwrap();
        O::drop(&mut s, output).unwrap();
        U::drop(&mut s, socket).unwrap();
    }
}
