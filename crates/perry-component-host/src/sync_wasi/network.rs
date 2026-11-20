use super::{
    bindings::wasi::sockets::{instance_network, ip_name_lookup, network},
    native_io::{self, Operation},
    streams::Pollable,
    State,
};
use network::{ErrorCode, IpAddress};
use std::{
    collections::VecDeque,
    net::{IpAddr, Ipv6Addr},
};
use wasmtime::component::{Resource, ResourceTableError};

pub struct Network(pub crate::Network);
pub enum ResolveState {
    Pending(Operation),
    Ready(VecDeque<IpAddr>),
    Failed(ErrorCode),
}
pub struct ResolveStream(pub ResolveState);
#[derive(Debug)]
pub enum SocketError {
    Code(ErrorCode),
    Trap(wasmtime::Error),
}
pub type SocketResult<T> = Result<T, SocketError>;
impl From<ErrorCode> for SocketError {
    fn from(e: ErrorCode) -> Self {
        Self::Code(e)
    }
}
impl From<ResourceTableError> for SocketError {
    fn from(e: ResourceTableError) -> Self {
        Self::Trap(e.into())
    }
}
impl From<wasmtime::Error> for SocketError {
    fn from(e: wasmtime::Error) -> Self {
        Self::Trap(e)
    }
}
impl From<std::io::Error> for SocketError {
    fn from(e: std::io::Error) -> Self {
        Self::Code(io_error(&e))
    }
}
impl From<native_io::Error> for SocketError {
    fn from(e: native_io::Error) -> Self {
        use perry_abi::component_io as a;
        if e.0.os != 0 {
            return std::io::Error::from_raw_os_error(e.0.os).into();
        }
        Self::Code(match e.0.kind {
            a::ERR_UNSUPPORTED => ErrorCode::NotSupported,
            a::ERR_INVALID => ErrorCode::InvalidArgument,
            a::ERR_WOULD_BLOCK => ErrorCode::WouldBlock,
            a::ERR_TIMEOUT => ErrorCode::Timeout,
            a::ERR_REFUSED => ErrorCode::ConnectionRefused,
            a::ERR_RESET => ErrorCode::ConnectionReset,
            a::ERR_PIPE => ErrorCode::Unknown,
            a::ERR_CANCELLED => ErrorCode::ConnectionAborted,
            a::ERR_LIMIT => ErrorCode::NewSocketLimit,
            a::ERR_ACCESS => ErrorCode::AccessDenied,
            _ => ErrorCode::Unknown,
        })
    }
}
impl std::fmt::Display for SocketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for SocketError {}
pub fn io_error(error: &std::io::Error) -> ErrorCode {
    use std::io::ErrorKind as K;
    if let Some(os) = error.raw_os_error() {
        #[cfg(unix)]
        {
            let special = match os {
                libc::EALREADY => Some(ErrorCode::ConcurrencyConflict),
                libc::EISCONN | libc::ENOTCONN | libc::EDESTADDRREQ => {
                    Some(ErrorCode::InvalidState)
                }
                libc::EMFILE | libc::ENFILE => Some(ErrorCode::NewSocketLimit),
                libc::EMSGSIZE => Some(ErrorCode::DatagramTooLarge),
                libc::ENOBUFS => Some(ErrorCode::OutOfMemory),
                libc::EHOSTDOWN | libc::ENETDOWN => Some(ErrorCode::RemoteUnreachable),
                libc::ENOPROTOOPT
                | libc::EPFNOSUPPORT
                | libc::EPROTONOSUPPORT
                | libc::EPROTOTYPE
                | libc::ESOCKTNOSUPPORT
                | libc::EAFNOSUPPORT => Some(ErrorCode::NotSupported),
                #[cfg(target_os = "linux")]
                libc::ENONET => Some(ErrorCode::RemoteUnreachable),
                _ => None,
            };
            if let Some(code) = special {
                return code;
            }
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::Networking::WinSock::*;
            let special = match os {
                WSAEALREADY => Some(ErrorCode::ConcurrencyConflict),
                WSAEISCONN | WSAENOTCONN | WSAEDESTADDRREQ => Some(ErrorCode::InvalidState),
                WSAEMFILE => Some(ErrorCode::NewSocketLimit),
                WSAEMSGSIZE => Some(ErrorCode::DatagramTooLarge),
                WSAENOBUFS => Some(ErrorCode::OutOfMemory),
                WSAEHOSTDOWN | WSAENETDOWN => Some(ErrorCode::RemoteUnreachable),
                WSAENOPROTOOPT | WSAEPFNOSUPPORT | WSAEPROTONOSUPPORT | WSAEPROTOTYPE
                | WSAESOCKTNOSUPPORT | WSAEAFNOSUPPORT => Some(ErrorCode::NotSupported),
                _ => None,
            };
            if let Some(code) = special {
                return code;
            }
        }
    }
    match error.kind() {
        K::PermissionDenied => ErrorCode::AccessDenied,
        K::Unsupported => ErrorCode::NotSupported,
        K::InvalidInput => ErrorCode::InvalidArgument,
        K::NotConnected => ErrorCode::InvalidState,
        K::WouldBlock | K::Interrupted => ErrorCode::WouldBlock,
        K::OutOfMemory => ErrorCode::OutOfMemory,
        K::AddrInUse => ErrorCode::AddressInUse,
        K::AddrNotAvailable => ErrorCode::AddressNotBindable,
        K::NetworkUnreachable | K::HostUnreachable => ErrorCode::RemoteUnreachable,
        K::TimedOut => ErrorCode::Timeout,
        K::ConnectionRefused => ErrorCode::ConnectionRefused,
        K::ConnectionReset => ErrorCode::ConnectionReset,
        K::ConnectionAborted => ErrorCode::ConnectionAborted,
        _ => ErrorCode::Unknown,
    }
}
fn ip(addr: IpAddr) -> IpAddress {
    match addr {
        IpAddr::V4(ip) => {
            let [a, b, c, d] = ip.octets();
            IpAddress::Ipv4((a, b, c, d))
        }
        IpAddr::V6(ip) => {
            let [a, b, c, d, e, f, g, h] = ip.segments();
            IpAddress::Ipv6((a, b, c, d, e, f, g, h))
        }
    }
}
impl network::Host for State {
    fn network_error_code(
        &mut self,
        error: Resource<super::errors::IoError>,
    ) -> wasmtime::Result<Option<ErrorCode>> {
        let code = io_error(&self.table.get(&error)?.0);
        Ok(Some(code))
    }
    fn convert_error_code(&mut self, error: SocketError) -> wasmtime::Result<ErrorCode> {
        match error {
            SocketError::Code(code) => Ok(code),
            SocketError::Trap(error) => Err(error),
        }
    }
}
impl network::HostNetwork for State {
    fn drop(&mut self, network: Resource<Network>) -> wasmtime::Result<()> {
        self.table.delete(network)?;
        Ok(())
    }
}
impl instance_network::Host for State {
    fn instance_network(&mut self) -> wasmtime::Result<Resource<Network>> {
        Ok(self.table.push(Network(self.options.network.clone()))?)
    }
}
impl ip_name_lookup::Host for State {
    fn resolve_addresses(
        &mut self,
        network: Resource<Network>,
        name: String,
    ) -> SocketResult<Resource<ResolveStream>> {
        if !self.table.get(&network)?.0.dns {
            return Err(ErrorCode::PermanentResolverFailure.into());
        }
        let state = match url::Host::parse(&name) {
            Ok(url::Host::Domain(domain)) => ResolveState::Pending(self.io.resolve(&domain)?),
            Ok(url::Host::Ipv4(addr)) => ResolveState::Ready(VecDeque::from([IpAddr::V4(addr)])),
            Ok(url::Host::Ipv6(addr)) => ResolveState::Ready(VecDeque::from([IpAddr::V6(addr)])),
            Err(_) => match name.parse::<Ipv6Addr>() {
                Ok(addr) => ResolveState::Ready(VecDeque::from([IpAddr::V6(addr)])),
                Err(_) => return Err(ErrorCode::InvalidArgument.into()),
            },
        };
        Ok(self.table.push(ResolveStream(state))?)
    }
}
impl ip_name_lookup::HostResolveAddressStream for State {
    fn drop(&mut self, stream: Resource<ResolveStream>) -> wasmtime::Result<()> {
        self.table.delete(stream)?;
        Ok(())
    }
    fn subscribe(
        &mut self,
        stream: Resource<ResolveStream>,
    ) -> wasmtime::Result<Resource<Pollable>> {
        Ok(self
            .table
            .push_child(Pollable::Resolve(stream.rep()), &stream)?)
    }
    fn resolve_next_address(
        &mut self,
        stream: Resource<ResolveStream>,
    ) -> SocketResult<Option<IpAddress>> {
        if let ResolveState::Pending(op) = &self.table.get(&stream)?.0 {
            self.io.poll(&[op], false)?;
            if !op.ready() {
                return Err(ErrorCode::WouldBlock.into());
            }
        }
        let stream = self.table.get_mut(&stream)?;
        if matches!(stream.0, ResolveState::Pending(_)) {
            let ResolveState::Pending(op) =
                std::mem::replace(&mut stream.0, ResolveState::Ready(VecDeque::new()))
            else {
                unreachable!()
            };
            stream.0 = match op.finish() {
                Ok(result) if result.error.kind == 0 => ResolveState::Ready(
                    result
                        .addresses
                        .into_iter()
                        .map(|addr| {
                            if addr.family == 4 {
                                IpAddr::from([
                                    addr.octets[0],
                                    addr.octets[1],
                                    addr.octets[2],
                                    addr.octets[3],
                                ])
                            } else {
                                IpAddr::from(addr.octets).to_canonical()
                            }
                        })
                        .collect(),
                ),
                _ => ResolveState::Failed(ErrorCode::NameUnresolvable),
            };
        }
        match &mut stream.0 {
            ResolveState::Ready(addresses) => Ok(addresses.pop_front().map(ip)),
            ResolveState::Failed(error) => Err((*error).into()),
            ResolveState::Pending(_) => unreachable!(),
        }
    }
}
impl SocketError {
    #[cfg(test)]
    pub(crate) fn downcast(self) -> Result<ErrorCode, wasmtime::Error> {
        match self {
            Self::Code(code) => Ok(code),
            Self::Trap(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::bindings::wasi::io::poll::HostPollable;
    use super::*;
    use instance_network::Host as _;
    use ip_name_lookup::{Host, HostResolveAddressStream};
    #[test]
    fn dns_permission_literals_and_live_resolution_without_tokio() {
        let mut denied = State::new(&crate::WasiOptions::default()).unwrap();
        let net = denied.instance_network().unwrap();
        assert!(matches!(
            denied.resolve_addresses(net, "localhost".into()),
            Err(SocketError::Code(ErrorCode::PermanentResolverFailure))
        ));
        let mut state = State::new(&crate::WasiOptions {
            network: crate::Network {
                dns: true,
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
        let network = state.instance_network().unwrap();
        for name in ["127.0.0.1", "::1", "localhost"] {
            let stream = state
                .resolve_addresses(Resource::new_borrow(network.rep()), name.into())
                .unwrap();
            let pollable = state.subscribe(Resource::new_borrow(stream.rep())).unwrap();
            state.block(Resource::new_borrow(pollable.rep())).unwrap();
            let mut count = 0;
            while state
                .resolve_next_address(Resource::new_borrow(stream.rep()))
                .unwrap()
                .is_some()
            {
                count += 1;
            }
            assert!(count > 0);
            assert!(state.ready(Resource::new_borrow(pollable.rep())).unwrap());
            HostPollable::drop(&mut state, pollable).unwrap();
            HostResolveAddressStream::drop(&mut state, stream).unwrap();
        }
    }
    #[test]
    fn ipv4_mapped_ipv6_literals_keep_their_literal_address_family() {
        let mut state = State::new(&crate::WasiOptions {
            network: crate::Network {
                dns: true,
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
        let network = state.instance_network().unwrap();
        for name in ["::ffff:127.0.0.1", "[::ffff:127.0.0.1]"] {
            let stream = state
                .resolve_addresses(Resource::new_borrow(network.rep()), name.into())
                .unwrap();
            let address = state
                .resolve_next_address(Resource::new_borrow(stream.rep()))
                .unwrap();
            assert!(matches!(
                address,
                Some(IpAddress::Ipv6((0, 0, 0, 0, 0, 0xffff, 0x7f00, 1)))
            ));
            assert!(state
                .resolve_next_address(Resource::new_borrow(stream.rep()))
                .unwrap()
                .is_none());
            HostResolveAddressStream::drop(&mut state, stream).unwrap();
        }
    }
    #[test]
    fn network_error_resources_preserve_unknown_io_errors() {
        let mut state = State::new(&crate::WasiOptions::default()).unwrap();
        for (kind, expected) in [
            (std::io::ErrorKind::Other, ErrorCode::Unknown),
            (
                std::io::ErrorKind::PermissionDenied,
                ErrorCode::AccessDenied,
            ),
        ] {
            let error = state
                .table
                .push(super::super::errors::IoError(kind.into()))
                .unwrap();
            assert_eq!(
                network::Host::network_error_code(&mut state, Resource::new_borrow(error.rep()))
                    .unwrap(),
                Some(expected)
            );
            state.table.delete(error).unwrap();
        }
    }
}
