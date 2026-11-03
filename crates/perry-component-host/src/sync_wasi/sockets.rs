use super::{
    bindings::wasi::sockets::network::{
        ErrorCode, IpAddressFamily, IpSocketAddress, Ipv4SocketAddress, Ipv6SocketAddress,
    },
    native_io::{Client, Socket as Native},
    network::{SocketError, SocketResult},
};
use perry_abi::component_io as abi;
use socket2::{Domain, Protocol, Socket, Type};
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6},
    sync::Arc,
};

pub fn decode(a: IpSocketAddress) -> SocketAddr {
    match a {
        IpSocketAddress::Ipv4(a) => {
            let (b, c, d, e) = a.address;
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(b, c, d, e)), a.port)
        }
        IpSocketAddress::Ipv6(a) => {
            let (b, c, d, e, f, g, h, i) = a.address;
            SocketAddr::V6(SocketAddrV6::new(
                Ipv6Addr::new(b, c, d, e, f, g, h, i),
                a.port,
                a.flow_info,
                a.scope_id,
            ))
        }
    }
}
pub fn encode(a: SocketAddr) -> IpSocketAddress {
    match a {
        SocketAddr::V4(a) => {
            let [b, c, d, e] = a.ip().octets();
            IpSocketAddress::Ipv4(Ipv4SocketAddress {
                port: a.port(),
                address: (b, c, d, e),
            })
        }
        SocketAddr::V6(a) => {
            let [b, c, d, e, f, g, h, i] = a.ip().segments();
            IpSocketAddress::Ipv6(Ipv6SocketAddress {
                port: a.port(),
                flow_info: a.flowinfo(),
                scope_id: a.scope_id(),
                address: (b, c, d, e, f, g, h, i),
            })
        }
    }
}
pub fn native_address(a: SocketAddr) -> abi::Address {
    let mut out = abi::Address {
        port: a.port(),
        ..Default::default()
    };
    match a {
        SocketAddr::V4(a) => {
            out.family = 4;
            out.octets[..4].copy_from_slice(&a.ip().octets());
        }
        SocketAddr::V6(a) => {
            out.family = 6;
            out.octets = a.ip().octets();
            out.flow_info = a.flowinfo();
            out.scope_id = a.scope_id();
        }
    }
    out
}
pub fn from_native(a: abi::Address) -> SocketAddr {
    if a.family == 4 {
        SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(
                a.octets[0],
                a.octets[1],
                a.octets[2],
                a.octets[3],
            )),
            a.port,
        )
    } else {
        SocketAddr::V6(SocketAddrV6::new(
            Ipv6Addr::from(a.octets),
            a.port,
            a.flow_info,
            a.scope_id,
        ))
    }
}
pub fn validate(a: SocketAddr, family: IpAddressFamily, remote: bool) -> SocketResult<()> {
    let valid = match (a.ip(), family) {
        (IpAddr::V4(_), IpAddressFamily::Ipv4) => true,
        (IpAddr::V6(ip), IpAddressFamily::Ipv6) => {
            ip.to_ipv4_mapped().is_none()
                && !(ip.segments()[..6] == [0; 6] && !ip.is_unspecified() && !ip.is_loopback())
        }
        _ => false,
    };
    if !valid || (remote && (a.port() == 0 || a.ip().is_unspecified())) {
        return Err(ErrorCode::InvalidArgument.into());
    }
    Ok(())
}
pub fn permitted(network: &crate::Network, address: SocketAddr) -> bool {
    network.addresses.iter().any(|grant| grant.allows(address))
}

pub fn permit(network: &crate::Network, a: SocketAddr) -> SocketResult<()> {
    if permitted(network, a) {
        Ok(())
    } else {
        Err(ErrorCode::AccessDenied.into())
    }
}
pub fn new_socket(family: IpAddressFamily, udp: bool) -> SocketResult<Socket> {
    let domain = if matches!(family, IpAddressFamily::Ipv4) {
        Domain::IPV4
    } else {
        Domain::IPV6
    };
    let socket = Socket::new(
        domain,
        if udp { Type::DGRAM } else { Type::STREAM },
        Some(if udp { Protocol::UDP } else { Protocol::TCP }),
    )?;
    socket.set_nonblocking(true)?;
    if matches!(family, IpAddressFamily::Ipv6) {
        socket.set_only_v6(true)?;
    }
    Ok(socket)
}
#[cfg(unix)]
pub fn into_raw(socket: Socket) -> usize {
    use std::os::fd::IntoRawFd;
    socket.into_raw_fd() as usize
}
#[cfg(windows)]
pub fn into_raw(socket: Socket) -> usize {
    use std::os::windows::io::IntoRawSocket;
    socket.into_raw_socket() as usize
}
#[cfg(unix)]
unsafe fn from_raw(raw: usize) -> Socket {
    use std::os::fd::FromRawFd;
    unsafe { Socket::from_raw_fd(raw as i32) }
}
#[cfg(windows)]
unsafe fn from_raw(raw: usize) -> Socket {
    use std::os::windows::io::FromRawSocket;
    unsafe { Socket::from_raw_socket(raw as _) }
}
/// The borrowed native identity is queried only; this wrapper never drops it.
pub fn query<T>(socket: &Native, f: impl FnOnce(&Socket) -> std::io::Result<T>) -> SocketResult<T> {
    let raw = socket.raw()?;
    // SAFETY: the native owner remains borrowed, and ManuallyDrop prevents close.
    // Every caller performs a read-only endpoint or socket-option query.
    let borrowed = std::mem::ManuallyDrop::new(unsafe { from_raw(raw) });
    Ok(f(&borrowed)?)
}
pub enum SocketOwner {
    Owned(Socket),
    Driven(Arc<Native>),
    Closed,
}
impl SocketOwner {
    pub fn query<T>(&self, f: impl FnOnce(&Socket) -> std::io::Result<T>) -> SocketResult<T> {
        match self {
            Self::Owned(s) => Ok(f(s)?),
            Self::Driven(s) => query(s, f),
            Self::Closed => Err(ErrorCode::InvalidState.into()),
        }
    }
    pub fn owned(&mut self) -> SocketResult<&Socket> {
        if matches!(self, Self::Driven(_)) {
            let Self::Driven(s) = std::mem::replace(self, Self::Closed) else {
                unreachable!()
            };
            let s = Arc::try_unwrap(s).map_err(|_| {
                SocketError::Trap(wasmtime::format_err!(
                    "socket streams still own native operations"
                ))
            })?;
            let raw = s.release()?;
            // SAFETY: successful detach transferred exclusive ownership.
            *self = Self::Owned(unsafe { from_raw(raw) });
        }
        match self {
            Self::Owned(s) => Ok(s),
            _ => Err(ErrorCode::InvalidState.into()),
        }
    }
    pub fn drive(&mut self, client: Client) -> SocketResult<Arc<Native>> {
        if let Self::Driven(s) = self {
            return Ok(s.clone());
        }
        let Self::Owned(s) = std::mem::replace(self, Self::Closed) else {
            return Err(ErrorCode::InvalidState.into());
        };
        // SAFETY: adoption consumes this uniquely owned socket.
        let (s, _) = unsafe { client.adopt(into_raw(s), false, None) }?;
        *self = Self::Driven(s.clone());
        Ok(s)
    }
    pub fn option(&self, tag: u32, set: Option<u64>, family: IpAddressFamily) -> SocketResult<u64> {
        let value = self.raw_option(tag, set, family)?;
        Ok(
            if set.is_none()
                && cfg!(target_os = "linux")
                && matches!(tag, abi::RECEIVE_BUFFER | abi::SEND_BUFFER)
            {
                value / 2
            } else {
                value
            },
        )
    }
    fn raw_option(&self, tag: u32, set: Option<u64>, family: IpAddressFamily) -> SocketResult<u64> {
        if set == Some(0) {
            return Err(ErrorCode::InvalidArgument.into());
        }
        let value = set.map(|v| v.min(i32::MAX as u64));
        match self {
            Self::Driven(s) => {
                match s.option(tag, value.is_some(), [value.unwrap_or(0), 0, 0, 0]) {
                    Err(e)
                        if value.is_some()
                            && matches!(tag, abi::RECEIVE_BUFFER | abi::SEND_BUFFER)
                            && no_buffers(e.0.os) =>
                    {
                        self.raw_option(tag, None, family)
                    }
                    result => Ok(result?[0]),
                }
            }
            Self::Owned(s) => match tag {
                abi::RECEIVE_BUFFER => {
                    if let Some(v) = value {
                        buffer_hint(s.set_recv_buffer_size(v as usize))?;
                    }
                    Ok(s.recv_buffer_size()? as u64)
                }
                abi::SEND_BUFFER => {
                    if let Some(v) = value {
                        buffer_hint(s.set_send_buffer_size(v as usize))?;
                    }
                    Ok(s.send_buffer_size()? as u64)
                }
                abi::HOP_LIMIT => match family {
                    IpAddressFamily::Ipv4 => {
                        if let Some(v) = value {
                            s.set_ttl_v4(v as u32)?;
                        }
                        Ok(s.ttl_v4()? as u64)
                    }
                    IpAddressFamily::Ipv6 => {
                        if let Some(v) = value {
                            s.set_unicast_hops_v6(v as u32)?;
                        }
                        Ok(s.unicast_hops_v6()? as u64)
                    }
                },
                _ => Err(ErrorCode::InvalidArgument.into()),
            },
            Self::Closed => Err(ErrorCode::InvalidState.into()),
        }
    }
}
pub fn endpoint(owner: &SocketOwner, remote: bool) -> SocketResult<SocketAddr> {
    owner
        .query(|s| {
            if remote {
                s.peer_addr()
            } else {
                s.local_addr()
            }
        })?
        .as_socket()
        .ok_or(ErrorCode::Unknown.into())
}
pub fn disconnect(s: &Socket) -> SocketResult<()> {
    let storage = socket2::SockAddrStorage::zeroed();
    let len = storage.size_of();
    // SAFETY: zeroed storage has AF_UNSPEC on all supported native platforms.
    // The full initialized storage length is supplied to connect.
    let address = unsafe { socket2::SockAddr::new(storage, len) };
    match s.connect(&address) {
        #[cfg(target_os = "macos")]
        Err(e) if matches!(e.raw_os_error(), Some(libc::EINVAL | libc::EAFNOSUPPORT)) => Ok(()),
        result => Ok(result?),
    }
}

pub fn keepalive_parameter(
    socket: &Socket,
    field: usize,
    value: Option<u64>,
) -> std::io::Result<u64> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        #[cfg(target_vendor = "apple")]
        let idle = libc::TCP_KEEPALIVE;
        #[cfg(not(target_vendor = "apple"))]
        let idle = libc::TCP_KEEPIDLE;
        let name = match field {
            1 => idle,
            2 => libc::TCP_KEEPINTVL,
            _ => libc::TCP_KEEPCNT,
        };
        if let Some(value) = value {
            let value = if field == 3 {
                value
            } else {
                value / 1_000_000_000
            };
            let value = i32::try_from(value).map_err(|_| std::io::ErrorKind::InvalidInput)?;
            // SAFETY: this owned socket and initialized integer remain live.
            let rc = unsafe {
                libc::setsockopt(
                    socket.as_raw_fd(),
                    libc::IPPROTO_TCP,
                    name,
                    std::ptr::from_ref(&value).cast(),
                    std::mem::size_of::<i32>() as _,
                )
            };
            if rc != 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        let mut value = 0i32;
        let mut length = std::mem::size_of::<i32>() as libc::socklen_t;
        // SAFETY: writable integer output with exact capacity on a live socket.
        let rc = unsafe {
            libc::getsockopt(
                socket.as_raw_fd(),
                libc::IPPROTO_TCP,
                name,
                std::ptr::from_mut(&mut value).cast(),
                &mut length,
            )
        };
        if rc != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(value.max(0) as u64 * if field == 3 { 1 } else { 1_000_000_000 })
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawSocket;
        use windows_sys::Win32::Networking::WinSock::{
            getsockopt, setsockopt, WSAGetLastError, IPPROTO_TCP, TCP_KEEPCNT, TCP_KEEPIDLE,
            TCP_KEEPINTVL,
        };
        let name = match field {
            1 => TCP_KEEPIDLE,
            2 => TCP_KEEPINTVL,
            _ => TCP_KEEPCNT,
        };
        if let Some(value) = value {
            let value = if field == 3 {
                value
            } else {
                value / 1_000_000_000
            };
            let value = i32::try_from(value).map_err(|_| std::io::ErrorKind::InvalidInput)?;
            // SAFETY: this owned socket and initialized integer remain live.
            let rc = unsafe {
                setsockopt(
                    socket.as_raw_socket() as _,
                    IPPROTO_TCP,
                    name,
                    std::ptr::from_ref(&value).cast(),
                    std::mem::size_of::<i32>() as _,
                )
            };
            if rc != 0 {
                // SAFETY: thread-local Winsock error retrieval has no arguments.
                return Err(std::io::Error::from_raw_os_error(unsafe {
                    WSAGetLastError()
                }));
            }
        }
        let mut value = 0i32;
        let mut length = std::mem::size_of::<i32>() as i32;
        // SAFETY: writable integer output with exact capacity on a live socket.
        let rc = unsafe {
            getsockopt(
                socket.as_raw_socket() as _,
                IPPROTO_TCP,
                name,
                std::ptr::from_mut(&mut value).cast(),
                &mut length,
            )
        };
        if rc != 0 {
            // SAFETY: thread-local Winsock error retrieval has no arguments.
            return Err(std::io::Error::from_raw_os_error(unsafe {
                WSAGetLastError()
            }));
        }
        Ok(value.max(0) as u64 * if field == 3 { 1 } else { 1_000_000_000 })
    }
}

fn no_buffers(code: i32) -> bool {
    #[cfg(unix)]
    {
        code == libc::ENOBUFS
    }
    #[cfg(windows)]
    {
        code == windows_sys::Win32::Networking::WinSock::WSAENOBUFS
    }
}
fn buffer_hint(result: std::io::Result<()>) -> std::io::Result<()> {
    match result {
        Err(e) if e.raw_os_error().is_some_and(no_buffers) => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn socket_buffer_hints_report_kernel_values_with_wasi_normalization() {
        let family = IpAddressFamily::Ipv4;
        let mut socket = SocketOwner::Owned(new_socket(family, true).unwrap());
        socket
            .owned()
            .unwrap()
            .bind(&"127.0.0.1:0".parse::<SocketAddr>().unwrap().into())
            .unwrap();
        for driven in [false, true] {
            if driven {
                socket.drive(Client::current()).unwrap();
            }
            for tag in [abi::RECEIVE_BUFFER, abi::SEND_BUFFER] {
                assert!(socket.option(tag, Some(0), family).is_err());
                socket.option(tag, Some(32 * 1024), family).unwrap();
                let kernel = socket
                    .query(|s| {
                        if tag == abi::RECEIVE_BUFFER {
                            s.recv_buffer_size()
                        } else {
                            s.send_buffer_size()
                        }
                    })
                    .unwrap() as u64;
                let visible = socket.option(tag, None, family).unwrap();
                assert_eq!(
                    visible,
                    if cfg!(target_os = "linux") {
                        kernel / 2
                    } else {
                        kernel
                    }
                );
                assert!(
                    socket.option(tag, Some(u64::MAX), family).is_ok(),
                    "buffer sizes are performance hints"
                );
            }
        }
    }
}
