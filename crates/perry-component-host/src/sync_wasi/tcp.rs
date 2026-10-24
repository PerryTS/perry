use super::{
    bindings::wasi::sockets::{
        network::{ErrorCode, IpAddressFamily, IpSocketAddress},
        tcp, tcp_create_socket,
    },
    native_io::{Client, Operation},
    network::{Network, SocketError, SocketResult},
    sockets::*,
    streams::{Input, Output, Pollable},
    tcp_io::{Connection, Read, Write},
    State,
};
use perry_abi::component_io as abi;
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};
use wasmtime::component::Resource;

enum Phase {
    Initial(u32),
    Binding(u32),
    Bound(u32),
    ListeningStart,
    Listening(Option<Operation>),
    Connecting(Result<Operation, ErrorCode>),
    Connected(Arc<Connection>),
    Closed,
}
pub struct TcpSocket {
    owner: SocketOwner,
    family: IpAddressFamily,
    network: crate::Network,
    phase: Phase,
}
fn code(error: SocketError) -> ErrorCode {
    match error {
        SocketError::Code(c) => c,
        _ => ErrorCode::Unknown,
    }
}
fn unicast(a: SocketAddr) -> SocketResult<()> {
    if a.ip().is_multicast() || matches!(a.ip(),IpAddr::V4(a) if a.is_broadcast()) {
        Err(ErrorCode::InvalidArgument.into())
    } else {
        Ok(())
    }
}
impl TcpSocket {
    pub fn operation(&self) -> Option<&Operation> {
        match &self.phase {
            Phase::Connecting(Ok(op)) => Some(op),
            Phase::Listening(op) => op.as_ref(),
            _ => None,
        }
    }
    pub fn prepare(&mut self, io: Client) -> wasmtime::Result<()> {
        if matches!(self.phase, Phase::Listening(None)) {
            let SocketOwner::Driven(socket) = &self.owner else {
                unreachable!()
            };
            self.phase = Phase::Listening(Some(io.submit(
                Some(socket.clone()),
                abi::ACCEPT,
                &[],
                0,
                None,
                0,
            )?));
        }
        Ok(())
    }
    fn keepalive_enabled(&self) -> SocketResult<bool> {
        self.owner.query(|s| s.keepalive())
    }
    fn set_keepalive_enabled(&self, value: bool) -> SocketResult<()> {
        match &self.owner {
            SocketOwner::Owned(s) => Ok(s.set_keepalive(value)?),
            SocketOwner::Driven(s) => {
                s.option(abi::KEEPALIVE, true, [value as u64, 0, 0, 0])?;
                Ok(())
            }
            SocketOwner::Closed => Err(ErrorCode::InvalidState.into()),
        }
    }
    fn parameter(&self, field: usize) -> SocketResult<u64> {
        self.owner.query(|s| keepalive_parameter(s, field, None))
    }

    fn set_parameter(&self, field: usize, value: u64) -> SocketResult<()> {
        if value == 0 {
            return Err(ErrorCode::InvalidArgument.into());
        }
        let value = if field == 3 {
            value.min(127)
        } else {
            value.clamp(1_000_000_000, 32_767_000_000_000) / 1_000_000_000 * 1_000_000_000
        };
        match &self.owner {
            SocketOwner::Driven(s) => {
                let mut values = [0; 4];
                values[0] = 1 << (field - 1);
                values[field] = value;
                s.option(abi::KEEPALIVE_PARAMETERS, true, values)?;
                Ok(())
            }
            SocketOwner::Owned(s) => {
                keepalive_parameter(s, field, Some(value))?;
                Ok(())
            }
            SocketOwner::Closed => Err(ErrorCode::InvalidState.into()),
        }
    }
}
impl State {
    fn tcp_streams(
        &mut self,
        socket: &Resource<TcpSocket>,
        connection: Arc<Connection>,
    ) -> SocketResult<(Resource<Input>, Resource<Output>)> {
        Ok((
            self.table
                .push_child(Input::Tcp(connection.clone()), socket)?,
            self.table.push_child(Output::Tcp(connection), socket)?,
        ))
    }
}
impl tcp::Host for State {}
impl tcp_create_socket::Host for State {
    fn create_tcp_socket(&mut self, family: IpAddressFamily) -> SocketResult<Resource<TcpSocket>> {
        if !self.options.network.tcp {
            return Err(ErrorCode::AccessDenied.into());
        }
        Ok(self.table.push(TcpSocket {
            owner: SocketOwner::Owned(new_socket(family, false)?),
            family,
            network: self.options.network.clone(),
            phase: Phase::Initial(128),
        })?)
    }
}
impl tcp::HostTcpSocket for State {
    fn start_bind(
        &mut self,
        socket: Resource<TcpSocket>,
        network: Resource<Network>,
        address: IpSocketAddress,
    ) -> SocketResult<()> {
        self.table.get(&network)?;
        let s = self.table.get_mut(&socket)?;
        if matches!(
            s.phase,
            Phase::Binding(_) | Phase::Connecting(_) | Phase::ListeningStart
        ) {
            return Err(ErrorCode::ConcurrencyConflict.into());
        }
        let Phase::Initial(backlog) = s.phase else {
            return Err(ErrorCode::InvalidState.into());
        };
        let address = decode(address);
        validate(address, s.family, false)?;
        unicast(address)?;
        permit(&s.network, address)?;
        let owned = s.owner.owned()?;
        #[cfg(not(windows))]
        {
            let _ = owned.set_reuse_address(true);
        }
        owned.bind(&address.into())?;
        s.phase = Phase::Binding(backlog);
        Ok(())
    }
    fn finish_bind(&mut self, socket: Resource<TcpSocket>) -> SocketResult<()> {
        let s = self.table.get_mut(&socket)?;
        let Phase::Binding(backlog) = s.phase else {
            return Err(ErrorCode::NotInProgress.into());
        };
        s.phase = Phase::Bound(backlog);
        Ok(())
    }
    fn start_connect(
        &mut self,
        socket: Resource<TcpSocket>,
        network: Resource<Network>,
        address: IpSocketAddress,
    ) -> SocketResult<()> {
        self.table.get(&network)?;
        let s = self.table.get_mut(&socket)?;
        if matches!(
            s.phase,
            Phase::Binding(_) | Phase::Connecting(_) | Phase::ListeningStart
        ) {
            return Err(ErrorCode::ConcurrencyConflict.into());
        }
        if !matches!(s.phase, Phase::Initial(_) | Phase::Bound(_)) {
            return Err(ErrorCode::InvalidState.into());
        }
        let address = decode(address);
        validate(address, s.family, true)?;
        unicast(address)?;
        let implicit = match s.family {
            IpAddressFamily::Ipv4 => "0.0.0.0:0".parse::<SocketAddr>().unwrap(),
            IpAddressFamily::Ipv6 => "[::]:0".parse().unwrap(),
        };
        let allowed = (|| {
            if matches!(s.phase, Phase::Initial(_)) {
                permit(&s.network, implicit)?;
            }
            permit(&s.network, address)
        })();
        if let Err(e) = allowed {
            s.phase = Phase::Connecting(Err(code(e)));
            return Ok(());
        }
        let SocketOwner::Owned(owned) = std::mem::replace(&mut s.owner, SocketOwner::Closed) else {
            return Err(ErrorCode::InvalidState.into());
        };
        // SAFETY: connect adoption consumes the owned socket and preserves its binding.
        match unsafe {
            self.io
                .adopt(into_raw(owned), false, Some(native_address(address)))
        } {
            Ok((native, Some(operation))) => {
                s.owner = SocketOwner::Driven(native);
                s.phase = Phase::Connecting(Ok(operation));
            }
            Ok(_) => unreachable!(),
            Err(e) => s.phase = Phase::Connecting(Err(code(e.into()))),
        }
        Ok(())
    }
    fn finish_connect(
        &mut self,
        socket: Resource<TcpSocket>,
    ) -> SocketResult<(Resource<Input>, Resource<Output>)> {
        let s = self.table.get_mut(&socket)?;
        match &s.phase {
            Phase::Connecting(Ok(op)) => {
                self.io.poll(&[op], false)?;
                if !op.ready() {
                    return Err(ErrorCode::WouldBlock.into());
                }
            }
            Phase::Connecting(Err(_)) => {}
            _ => return Err(ErrorCode::NotInProgress.into()),
        }
        let Phase::Connecting(result) = std::mem::replace(&mut s.phase, Phase::Closed) else {
            unreachable!()
        };
        let complete = result.map_err(SocketError::Code)?.finish()?;
        if complete.error.kind != 0 {
            return Err(super::native_io::Error(complete.error).into());
        }
        let SocketOwner::Driven(native) = &s.owner else {
            unreachable!()
        };
        let connection = Connection::new(native.clone());
        s.phase = Phase::Connected(connection.clone());
        self.tcp_streams(&socket, connection)
    }
    fn start_listen(&mut self, socket: Resource<TcpSocket>) -> SocketResult<()> {
        let s = self.table.get_mut(&socket)?;
        if matches!(
            s.phase,
            Phase::Binding(_) | Phase::Connecting(_) | Phase::ListeningStart
        ) {
            return Err(ErrorCode::ConcurrencyConflict.into());
        }
        let Phase::Bound(backlog) = s.phase else {
            return Err(ErrorCode::InvalidState.into());
        };
        s.owner.owned()?.listen(backlog as i32)?;
        let SocketOwner::Owned(owned) = std::mem::replace(&mut s.owner, SocketOwner::Closed) else {
            unreachable!()
        };
        // SAFETY: this transfers the owned socket just placed in listen state.
        let (native, _) = unsafe { self.io.adopt(into_raw(owned), true, None) }?;
        s.owner = SocketOwner::Driven(native);
        s.phase = Phase::ListeningStart;
        Ok(())
    }
    fn finish_listen(&mut self, socket: Resource<TcpSocket>) -> SocketResult<()> {
        let s = self.table.get_mut(&socket)?;
        if !matches!(s.phase, Phase::ListeningStart) {
            return Err(ErrorCode::NotInProgress.into());
        }
        s.phase = Phase::Listening(None);
        Ok(())
    }
    fn accept(
        &mut self,
        socket: Resource<TcpSocket>,
    ) -> SocketResult<(Resource<TcpSocket>, Resource<Input>, Resource<Output>)> {
        let s = self.table.get_mut(&socket)?;
        if !matches!(s.phase, Phase::Listening(_) | Phase::ListeningStart) {
            return Err(ErrorCode::InvalidState.into());
        }
        s.prepare(self.io)?;
        let Phase::Listening(Some(op)) = &s.phase else {
            return Err(ErrorCode::WouldBlock.into());
        };
        self.io.poll(&[op], false)?;
        if !op.ready() {
            return Err(ErrorCode::WouldBlock.into());
        }
        let Phase::Listening(Some(op)) = std::mem::replace(&mut s.phase, Phase::Listening(None))
        else {
            unreachable!()
        };
        let complete = op.finish()?;
        if complete.error.kind != 0 {
            return Err(super::native_io::Error(complete.error).into());
        }
        let native = complete.socket.ok_or(ErrorCode::Unknown)?;
        let family = s.family;
        let network = s.network.clone();
        #[cfg(target_os = "macos")]
        {
            // macOS does not inherit these options. Read the listener at accept;
            // the accepted resource receives its actual current kernel settings.
            let idle = s.parameter(1)?;
            native.option(abi::KEEPALIVE_PARAMETERS, true, [1, idle, 0, 0])?;
            for tag in [abi::HOP_LIMIT, abi::RECEIVE_BUFFER, abi::SEND_BUFFER] {
                native.option(tag, true, [s.owner.option(tag, None, family)?, 0, 0, 0])?;
            }
        }
        let connection = Connection::new(native.clone());
        let accepted = self.table.push(TcpSocket {
            owner: SocketOwner::Driven(native),
            family,
            network,
            phase: Phase::Connected(connection.clone()),
        })?;
        let (input, output) = self.tcp_streams(&accepted, connection)?;
        Ok((accepted, input, output))
    }
    fn local_address(&mut self, socket: Resource<TcpSocket>) -> SocketResult<IpSocketAddress> {
        let s = self.table.get(&socket)?;
        if matches!(s.phase, Phase::Initial(_)) {
            return Err(ErrorCode::InvalidState.into());
        }
        Ok(encode(endpoint(&s.owner, false)?))
    }
    fn remote_address(&mut self, socket: Resource<TcpSocket>) -> SocketResult<IpSocketAddress> {
        let s = self.table.get(&socket)?;
        if !matches!(s.phase, Phase::Connected(_)) {
            return Err(ErrorCode::InvalidState.into());
        }
        Ok(encode(endpoint(&s.owner, true)?))
    }
    fn is_listening(&mut self, socket: Resource<TcpSocket>) -> wasmtime::Result<bool> {
        Ok(matches!(
            self.table.get(&socket)?.phase,
            Phase::Listening(_) | Phase::ListeningStart
        ))
    }
    fn address_family(&mut self, socket: Resource<TcpSocket>) -> wasmtime::Result<IpAddressFamily> {
        Ok(self.table.get(&socket)?.family)
    }
    fn set_listen_backlog_size(
        &mut self,
        socket: Resource<TcpSocket>,
        value: u64,
    ) -> SocketResult<()> {
        if value == 0 {
            return Err(ErrorCode::InvalidArgument.into());
        }
        let value = value.min(i32::MAX as u64) as u32;
        let s = self.table.get_mut(&socket)?;
        match &mut s.phase {
            Phase::Initial(backlog) | Phase::Binding(backlog) | Phase::Bound(backlog) => {
                *backlog = value;
                Ok(())
            }
            Phase::Listening(_) | Phase::ListeningStart => {
                let SocketOwner::Driven(native) = &s.owner else {
                    unreachable!()
                };
                native
                    .option(abi::LISTEN_BACKLOG, true, [value as u64, 0, 0, 0])
                    .map_err(|_| SocketError::Code(ErrorCode::NotSupported))?;
                Ok(())
            }
            _ => Err(ErrorCode::InvalidState.into()),
        }
    }
    fn keep_alive_enabled(&mut self, socket: Resource<TcpSocket>) -> SocketResult<bool> {
        self.table.get(&socket)?.keepalive_enabled()
    }
    fn set_keep_alive_enabled(
        &mut self,
        socket: Resource<TcpSocket>,
        value: bool,
    ) -> SocketResult<()> {
        self.table.get(&socket)?.set_keepalive_enabled(value)
    }
    fn keep_alive_idle_time(&mut self, socket: Resource<TcpSocket>) -> SocketResult<u64> {
        self.table.get(&socket)?.parameter(1)
    }
    fn set_keep_alive_idle_time(
        &mut self,
        socket: Resource<TcpSocket>,
        value: u64,
    ) -> SocketResult<()> {
        self.table.get(&socket)?.set_parameter(1, value)
    }
    fn keep_alive_interval(&mut self, socket: Resource<TcpSocket>) -> SocketResult<u64> {
        self.table.get(&socket)?.parameter(2)
    }
    fn set_keep_alive_interval(
        &mut self,
        socket: Resource<TcpSocket>,
        value: u64,
    ) -> SocketResult<()> {
        self.table.get(&socket)?.set_parameter(2, value)
    }
    fn keep_alive_count(&mut self, socket: Resource<TcpSocket>) -> SocketResult<u32> {
        Ok(self.table.get(&socket)?.parameter(3)? as u32)
    }
    fn set_keep_alive_count(
        &mut self,
        socket: Resource<TcpSocket>,
        value: u32,
    ) -> SocketResult<()> {
        self.table.get(&socket)?.set_parameter(3, value as u64)
    }
    fn hop_limit(&mut self, socket: Resource<TcpSocket>) -> SocketResult<u8> {
        let s = self.table.get(&socket)?;
        Ok(s.owner.option(abi::HOP_LIMIT, None, s.family)? as u8)
    }
    fn set_hop_limit(&mut self, socket: Resource<TcpSocket>, value: u8) -> SocketResult<()> {
        let s = self.table.get(&socket)?;
        s.owner
            .option(abi::HOP_LIMIT, Some(value as u64), s.family)?;
        Ok(())
    }
    fn receive_buffer_size(&mut self, socket: Resource<TcpSocket>) -> SocketResult<u64> {
        let s = self.table.get(&socket)?;
        s.owner.option(abi::RECEIVE_BUFFER, None, s.family)
    }
    fn set_receive_buffer_size(
        &mut self,
        socket: Resource<TcpSocket>,
        value: u64,
    ) -> SocketResult<()> {
        let s = self.table.get(&socket)?;
        s.owner.option(abi::RECEIVE_BUFFER, Some(value), s.family)?;
        Ok(())
    }
    fn send_buffer_size(&mut self, socket: Resource<TcpSocket>) -> SocketResult<u64> {
        let s = self.table.get(&socket)?;
        s.owner.option(abi::SEND_BUFFER, None, s.family)
    }
    fn set_send_buffer_size(
        &mut self,
        socket: Resource<TcpSocket>,
        value: u64,
    ) -> SocketResult<()> {
        let s = self.table.get(&socket)?;
        s.owner.option(abi::SEND_BUFFER, Some(value), s.family)?;
        Ok(())
    }
    fn subscribe(&mut self, socket: Resource<TcpSocket>) -> wasmtime::Result<Resource<Pollable>> {
        Ok(self
            .table
            .push_child(Pollable::TcpSocket(socket.rep()), &socket)?)
    }
    fn shutdown(
        &mut self,
        socket: Resource<TcpSocket>,
        how: tcp::ShutdownType,
    ) -> SocketResult<()> {
        let s = self.table.get(&socket)?;
        let Phase::Connected(c) = &s.phase else {
            return Err(ErrorCode::InvalidState.into());
        };
        if matches!(how, tcp::ShutdownType::Receive | tcp::ShutdownType::Both) {
            *c.read.lock().unwrap() = Read::Closed;
            c.socket.option(abi::SHUTDOWN_READ, true, [0; 4])?;
        }
        if matches!(how, tcp::ShutdownType::Send | tcp::ShutdownType::Both)
            && !matches!(*c.write.lock().unwrap(), Write::Closed)
        {
            c.wait_write(self.io)
                .map_err(|e| SocketError::Trap(e.into()))?;
            let op = self
                .io
                .submit(Some(c.socket.clone()), abi::SHUTDOWN, &[], 0, None, 0)?;
            while !op.ready() {
                self.io.poll(&[&op], true)?;
            }
            let done = op.finish()?;
            if done.error.kind != 0 {
                return Err(super::native_io::Error(done.error).into());
            }
            *c.write.lock().unwrap() = Write::Closed;
        }
        Ok(())
    }
    fn drop(&mut self, socket: Resource<TcpSocket>) -> wasmtime::Result<()> {
        self.table.delete(socket)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::bindings::wasi::{
        io::{
            poll::{Host as P, HostPollable as H},
            streams::{HostInputStream as I, HostOutputStream as O},
        },
        sockets::instance_network::Host as N,
    };
    use super::*;
    use std::{
        io::{Read as _, Write as _},
        net::{TcpListener, TcpStream},
        time::Duration,
    };
    use tcp::HostTcpSocket as T;
    use tcp_create_socket::Host as C;
    fn borrow<T>(r: &Resource<T>) -> Resource<T> {
        Resource::new_borrow(r.rep())
    }
    fn wait(s: &mut State, p: &Resource<Pollable>) {
        let deadline = s
            .table
            .push(Pollable::Operation(s.io.timer(5_000_000_000).unwrap()))
            .unwrap();
        let ready = P::poll(s, vec![borrow(p), borrow(&deadline)]).unwrap();
        assert!(ready.contains(&0), "TCP operation timed out");
        H::drop(s, deadline).unwrap();
    }
    fn state() -> State {
        State::new(&crate::WasiOptions {
            network: crate::Network {
                tcp: true,
                addresses: vec![crate::Address {
                    address: "127.0.0.1".parse().unwrap(),
                    port: None,
                }],
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap()
    }
    #[test]
    fn tcp_bound_connect_streams_and_independent_shutdown() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut s = state();
        let network = N::instance_network(&mut s).unwrap();
        let socket = C::create_tcp_socket(&mut s, IpAddressFamily::Ipv4).unwrap();
        assert!(!T::keep_alive_enabled(&mut s, borrow(&socket)).unwrap());
        T::set_keep_alive_idle_time(&mut s, borrow(&socket), 7_000_000_000).unwrap();
        assert_eq!(
            T::keep_alive_idle_time(&mut s, borrow(&socket)).unwrap(),
            7_000_000_000
        );
        assert!(!T::keep_alive_enabled(&mut s, borrow(&socket)).unwrap());
        let socket_poll = T::subscribe(&mut s, borrow(&socket)).unwrap();
        T::start_bind(
            &mut s,
            borrow(&socket),
            borrow(&network),
            encode("127.0.0.1:0".parse().unwrap()),
        )
        .unwrap();
        wait(&mut s, &socket_poll);
        T::finish_bind(&mut s, borrow(&socket)).unwrap();
        let local = decode(T::local_address(&mut s, borrow(&socket)).unwrap());
        T::start_connect(
            &mut s,
            borrow(&socket),
            network,
            encode(listener.local_addr().unwrap()),
        )
        .unwrap();
        wait(&mut s, &socket_poll);
        let (input, output) = T::finish_connect(&mut s, borrow(&socket)).unwrap();
        let (mut peer, remote) = listener.accept().unwrap();
        assert_eq!(local, remote);
        peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        peer.set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        assert_eq!(
            decode(T::local_address(&mut s, borrow(&socket)).unwrap()),
            local
        );
        T::set_keep_alive_interval(&mut s, borrow(&socket), 3_000_000_000).unwrap();
        assert_eq!(
            T::keep_alive_interval(&mut s, borrow(&socket)).unwrap(),
            3_000_000_000
        );
        assert!(!T::keep_alive_enabled(&mut s, borrow(&socket)).unwrap());
        let readable = I::subscribe(&mut s, borrow(&input)).unwrap();
        assert!(I::blocking_read(&mut s, borrow(&input), 0)
            .unwrap()
            .is_empty());
        peer.write_all(b"abc").unwrap();
        wait(&mut s, &readable);
        assert_eq!(I::read(&mut s, borrow(&input), 2).unwrap(), b"ab");
        assert!(H::ready(&mut s, borrow(&readable)).unwrap());
        assert_eq!(I::read(&mut s, borrow(&input), 2).unwrap(), b"c");
        assert!(I::read(&mut s, borrow(&input), 2).unwrap().is_empty());
        T::shutdown(&mut s, borrow(&socket), tcp::ShutdownType::Receive).unwrap();
        assert!(matches!(
            I::read(&mut s, borrow(&input), 2),
            Err(super::super::streams::StreamError::Closed)
        ));
        O::blocking_write_and_flush(&mut s, borrow(&output), b"reply".to_vec()).unwrap();
        let mut reply = [0; 5];
        peer.read_exact(&mut reply).unwrap();
        assert_eq!(&reply, b"reply");
        O::drop(&mut s, output).unwrap();
        assert_eq!(peer.read(&mut [0; 1]).unwrap(), 0, "output drop sends FIN");
        H::drop(&mut s, readable).unwrap();
        I::drop(&mut s, input).unwrap();
        H::drop(&mut s, socket_poll).unwrap();
        T::drop(&mut s, socket).unwrap();
    }
    #[test]
    fn tcp_unbound_connect_uses_both_grants_and_preserves_options() {
        for (local, wildcard, family) in [
            ("127.0.0.1:0", "0.0.0.0:0", IpAddressFamily::Ipv4),
            ("[::1]:0", "[::]:0", IpAddressFamily::Ipv6),
        ] {
            let listener = TcpListener::bind(local).unwrap();
            let remote = listener.local_addr().unwrap();
            let wildcard: SocketAddr = wildcard.parse().unwrap();
            let mut s = State::new(&crate::WasiOptions {
                network: crate::Network {
                    tcp: true,
                    addresses: vec![
                        crate::Address {
                            address: wildcard.ip(),
                            port: Some(0),
                        },
                        crate::Address {
                            address: remote.ip(),
                            port: Some(remote.port()),
                        },
                    ],
                    ..Default::default()
                },
                ..Default::default()
            })
            .unwrap();
            let network = N::instance_network(&mut s).unwrap();
            let socket = C::create_tcp_socket(&mut s, family).unwrap();
            T::set_keep_alive_enabled(&mut s, borrow(&socket), true).unwrap();
            T::set_keep_alive_idle_time(&mut s, borrow(&socket), 7_000_000_000).unwrap();
            let poll = T::subscribe(&mut s, borrow(&socket)).unwrap();
            T::start_connect(&mut s, borrow(&socket), network, encode(remote)).unwrap();
            #[cfg(unix)]
            assert_eq!(
                decode(T::local_address(&mut s, borrow(&socket)).unwrap()).port(),
                0
            );
            wait(&mut s, &poll);
            let (input, output) = T::finish_connect(&mut s, borrow(&socket)).unwrap();
            assert!(T::keep_alive_enabled(&mut s, borrow(&socket)).unwrap());
            assert_eq!(
                T::keep_alive_idle_time(&mut s, borrow(&socket)).unwrap(),
                7_000_000_000
            );
            let (mut peer, endpoint) = listener.accept().unwrap();
            peer.set_nonblocking(false).unwrap();
            peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            peer.set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            assert_eq!(
                decode(T::local_address(&mut s, borrow(&socket)).unwrap()),
                endpoint
            );
            peer.write_all(b"x").unwrap();
            assert_eq!(I::blocking_read(&mut s, borrow(&input), 1).unwrap(), b"x");
            O::blocking_write_and_flush(&mut s, borrow(&output), b"y".to_vec()).unwrap();
            let mut byte = [0; 1];
            peer.read_exact(&mut byte).unwrap();
            assert_eq!(&byte, b"y");
            O::drop(&mut s, output).unwrap();
            I::drop(&mut s, input).unwrap();
            H::drop(&mut s, poll).unwrap();
            T::drop(&mut s, socket).unwrap();
        }
    }
    #[test]
    fn tcp_listening_pollable_tracks_successive_accepts() {
        let mut s = state();
        let net = N::instance_network(&mut s).unwrap();
        let socket = C::create_tcp_socket(&mut s, IpAddressFamily::Ipv4).unwrap();
        T::start_bind(
            &mut s,
            borrow(&socket),
            net,
            encode("127.0.0.1:0".parse().unwrap()),
        )
        .unwrap();
        T::finish_bind(&mut s, borrow(&socket)).unwrap();
        T::start_listen(&mut s, borrow(&socket)).unwrap();
        T::finish_listen(&mut s, borrow(&socket)).unwrap();
        T::set_listen_backlog_size(&mut s, borrow(&socket), 4).unwrap();
        let endpoint = decode(T::local_address(&mut s, borrow(&socket)).unwrap());
        let poll = T::subscribe(&mut s, borrow(&socket)).unwrap();
        assert!(!H::ready(&mut s, borrow(&poll)).unwrap());
        for message in [b"x", b"y"] {
            let mut peer = TcpStream::connect(endpoint).unwrap();
            peer.write_all(message).unwrap();
            wait(&mut s, &poll);
            let (accepted, input, output) = T::accept(&mut s, borrow(&socket)).unwrap();
            let readable = I::subscribe(&mut s, borrow(&input)).unwrap();
            wait(&mut s, &readable);
            assert_eq!(I::read(&mut s, borrow(&input), 1).unwrap(), message);
            H::drop(&mut s, readable).unwrap();
            I::drop(&mut s, input).unwrap();
            O::drop(&mut s, output).unwrap();
            T::drop(&mut s, accepted).unwrap();
            assert!(!H::ready(&mut s, borrow(&poll)).unwrap());
        }
        H::drop(&mut s, poll).unwrap();
        T::drop(&mut s, socket).unwrap();
    }
    #[test]
    fn tcp_permission_denial_finishes_the_connect_operation() {
        let mut denied = State::new(&crate::WasiOptions::default()).unwrap();
        assert!(matches!(
            C::create_tcp_socket(&mut denied, IpAddressFamily::Ipv4),
            Err(SocketError::Code(ErrorCode::AccessDenied))
        ));
        let mut s = state();
        let socket = C::create_tcp_socket(&mut s, IpAddressFamily::Ipv4).unwrap();
        let net = N::instance_network(&mut s).unwrap();
        // The implicit wildcard bind is ungranted; start/finish retains the P2 split.
        T::start_connect(
            &mut s,
            borrow(&socket),
            net,
            encode("127.0.0.1:80".parse().unwrap()),
        )
        .unwrap();
        assert!(matches!(
            T::finish_connect(&mut s, borrow(&socket)),
            Err(SocketError::Code(ErrorCode::AccessDenied))
        ));
        assert!(matches!(
            T::finish_connect(&mut s, borrow(&socket)),
            Err(SocketError::Code(ErrorCode::NotInProgress))
        ));
        T::drop(&mut s, socket).unwrap();
    }
}
