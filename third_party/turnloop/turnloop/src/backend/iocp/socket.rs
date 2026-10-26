use super::{Detached, Kind, Native, invalid, unsupported};
use crate::{Error, Result};
use std::{
    mem::size_of,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    os::windows::io::{AsRawSocket, FromRawSocket, OwnedSocket},
    ptr,
    sync::OnceLock,
};
use windows_sys::{Win32::Networking::WinSock::*, core::GUID};

pub(super) fn startup() -> Result<()> {
    static STARTED: OnceLock<Result<()>> = OnceLock::new();
    *STARTED.get_or_init(|| {
        // SAFETY: WSADATA is writable C output; retain Winsock for process lifetime,
        // including Detached sockets that outlive every loop.
        let mut data = unsafe { std::mem::zeroed() };
        // SAFETY: valid output and supported version. WSAStartup returns its error.
        let code = unsafe { WSAStartup(0x202, &mut data) };
        if code == 0 { Ok(()) } else { Err(error(code)) }
    })
}
pub(super) fn error(code: i32) -> Error {
    use crate::ErrorKind;
    use windows_sys::Win32::Foundation::*;
    let mut error: Error = std::io::Error::from_raw_os_error(code).into();
    error.kind = match code as u32 {
        ERROR_CONNECTION_REFUSED => ErrorKind::ConnectionRefused,
        ERROR_NETNAME_DELETED | ERROR_CONNECTION_ABORTED => ErrorKind::ConnectionReset,
        ERROR_BROKEN_PIPE | ERROR_NO_DATA => ErrorKind::BrokenPipe,
        ERROR_SEM_TIMEOUT | ERROR_TIMEOUT => ErrorKind::TimedOut,
        ERROR_OPERATION_ABORTED => ErrorKind::Cancelled,
        ERROR_NOT_ENOUGH_MEMORY | ERROR_NO_SYSTEM_RESOURCES => ErrorKind::ResourceLimit,
        _ => error.kind,
    };
    error
}
pub(super) fn last_error() -> Error {
    // SAFETY: thread-local error query.
    error(unsafe { WSAGetLastError() })
}
pub(super) fn check(code: i32) -> Result<()> {
    if code == SOCKET_ERROR {
        Err(last_error())
    } else {
        Ok(())
    }
}
pub(super) fn create(v6: bool, udp: bool) -> Result<OwnedSocket> {
    startup()?;
    // SAFETY: initialized Winsock; no provider pointer; exclusively owned socket.
    let raw = unsafe {
        WSASocketW(
            if v6 { AF_INET6 } else { AF_INET } as i32,
            if udp { SOCK_DGRAM } else { SOCK_STREAM },
            if udp { IPPROTO_UDP } else { IPPROTO_TCP },
            ptr::null(),
            0,
            WSA_FLAG_OVERLAPPED | WSA_FLAG_NO_HANDLE_INHERIT,
        )
    };
    if raw == INVALID_SOCKET {
        return Err(last_error());
    }
    // SAFETY: successful creation transferred exclusive ownership.
    Ok(unsafe { OwnedSocket::from_raw_socket(raw as _) })
}

pub(super) struct Addr {
    pub storage: SOCKADDR_STORAGE,
    pub len: i32,
}
impl Addr {
    pub fn new(addr: SocketAddr) -> Self {
        // SAFETY: plain C address storage; each address below fits with correct alignment.
        let mut storage: SOCKADDR_STORAGE = unsafe { std::mem::zeroed() };
        let len = match addr {
            SocketAddr::V4(addr) => {
                // SAFETY: SOCKADDR_STORAGE is large and aligned for SOCKADDR_IN.
                let out = unsafe { &mut *ptr::from_mut(&mut storage).cast::<SOCKADDR_IN>() };
                out.sin_family = AF_INET;
                out.sin_port = addr.port().to_be();
                out.sin_addr.S_un.S_addr = u32::from_ne_bytes(addr.ip().octets());
                size_of::<SOCKADDR_IN>()
            }
            SocketAddr::V6(addr) => {
                // SAFETY: SOCKADDR_STORAGE is large and aligned for SOCKADDR_IN6.
                let out = unsafe { &mut *ptr::from_mut(&mut storage).cast::<SOCKADDR_IN6>() };
                out.sin6_family = AF_INET6;
                out.sin6_port = addr.port().to_be();
                out.sin6_flowinfo = addr.flowinfo();
                out.Anonymous.sin6_scope_id = addr.scope_id();
                out.sin6_addr.u.Byte = addr.ip().octets();
                size_of::<SOCKADDR_IN6>()
            }
        } as i32;
        Self { storage, len }
    }
    pub fn as_ptr(&self) -> *const SOCKADDR {
        ptr::from_ref(&self.storage).cast()
    }
}
pub(super) fn decode(storage: &SOCKADDR_STORAGE, len: i32) -> Result<SocketAddr> {
    match storage.ss_family {
        AF_INET if len >= size_of::<SOCKADDR_IN>() as i32 => {
            // SAFETY: validated family and size; storage has sufficient alignment.
            let addr = unsafe { &*ptr::from_ref(storage).cast::<SOCKADDR_IN>() };
            // SAFETY: IPv4 address union was initialized by Winsock.
            let ip = unsafe { addr.sin_addr.S_un.S_addr }.to_ne_bytes();
            Ok((Ipv4Addr::from(ip), u16::from_be(addr.sin_port)).into())
        }
        AF_INET6 if len >= size_of::<SOCKADDR_IN6>() as i32 => {
            // SAFETY: validated family and size; initialized Winsock output unions.
            let addr = unsafe { &*ptr::from_ref(storage).cast::<SOCKADDR_IN6>() };
            // SAFETY: initialized IPv6 union members.
            let (ip, scope) = unsafe { (addr.sin6_addr.u.Byte, addr.Anonymous.sin6_scope_id) };
            Ok(std::net::SocketAddrV6::new(
                Ipv6Addr::from(ip),
                u16::from_be(addr.sin6_port),
                addr.sin6_flowinfo,
                scope,
            )
            .into())
        }
        _ => Err(invalid()),
    }
}
pub(super) fn address(socket: usize, peer: bool) -> Result<SocketAddr> {
    // SAFETY: plain C writable address storage.
    let mut addr: SOCKADDR_STORAGE = unsafe { std::mem::zeroed() };
    let mut len = size_of::<SOCKADDR_STORAGE>() as i32;
    // SAFETY: live socket and bounded output pointers.
    check(unsafe {
        if peer {
            getpeername(socket, ptr::from_mut(&mut addr).cast(), &mut len)
        } else {
            getsockname(socket, ptr::from_mut(&mut addr).cast(), &mut len)
        }
    })?;
    decode(&addr, len)
}
pub(super) fn bind_to(socket: usize, addr: SocketAddr) -> Result<()> {
    let addr = Addr::new(addr);
    // SAFETY: live socket; address pointer matches its byte length.
    check(unsafe { bind(socket, addr.as_ptr(), addr.len) })
}
pub(super) fn option(socket: usize, name: i32, value: i32) -> Result<()> {
    // SAFETY: live socket, option takes a four-byte integer.
    check(unsafe {
        setsockopt(
            socket,
            SOL_SOCKET,
            name,
            ptr::from_ref(&value).cast(),
            size_of::<i32>() as i32,
        )
    })
}
pub(super) fn nonblocking(socket: usize) -> Result<()> {
    let mut value = 1;
    // SAFETY: live socket and valid FIONBIO input.
    check(unsafe { ioctlsocket(socket, FIONBIO, &mut value) })
}
pub(super) fn loopback_connect(socket: usize) {
    // Loopback delivery does not lose SYN packets. Avoid Windows retrying a
    // refused local connection for ~2 seconds before reporting ECONNREFUSED.
    // https://learn.microsoft.com/en-us/windows/win32/api/mstcpip/ns-mstcpip-tcp_initial_rto_parameters
    let options = TCP_INITIAL_RTO_PARAMETERS {
        Rtt: u16::MAX, // TCP_INITIAL_RTO_UNSPECIFIED_RTT (absent from windows-sys)
        MaxSynRetransmissions: TCP_INITIAL_RTO_NO_SYN_RETRANSMISSIONS as u8,
    };
    let mut bytes = 0;
    // This is a best-effort latency optimization, not a prerequisite for connect.
    // SAFETY: documented synchronous IOCTL input layout; Winsock validates the
    // socket value and retains none of these stack pointers after returning.
    let _ = unsafe {
        WSAIoctl(
            socket,
            SIO_TCP_INITIAL_RTO,
            ptr::from_ref(&options).cast(),
            size_of::<TCP_INITIAL_RTO_PARAMETERS>() as u32,
            ptr::null_mut(),
            0,
            &mut bytes,
            ptr::null_mut(),
            None,
        )
    };
}
pub(super) fn ifs(socket: usize) -> Result<bool> {
    let mut info = WSAPROTOCOL_INFOW::default();
    let mut len = size_of::<WSAPROTOCOL_INFOW>() as i32;
    // SAFETY: option output has its documented layout and exact length.
    check(unsafe {
        getsockopt(
            socket,
            SOL_SOCKET,
            SO_PROTOCOL_INFOW,
            ptr::from_mut(&mut info).cast(),
            &mut len,
        )
    })?;
    Ok(info.dwServiceFlags1 & XP1_IFS_HANDLES != 0)
}
/// Resolve per-provider extension pointers rather than assuming one Winsock provider.
pub(super) unsafe fn extension<T: Copy>(socket: usize, guid: &GUID) -> Result<T> {
    let mut out = std::mem::MaybeUninit::<T>::uninit();
    let mut bytes = 0;
    // SAFETY: caller supplies the nullable function-pointer type matching this GUID;
    // synchronous WSAIoctl writes exactly the provided output storage.
    check(unsafe {
        WSAIoctl(
            socket,
            SIO_GET_EXTENSION_FUNCTION_POINTER,
            ptr::from_ref(guid).cast(),
            size_of::<GUID>() as u32,
            out.as_mut_ptr().cast(),
            size_of::<T>() as u32,
            &mut bytes,
            ptr::null_mut(),
            None,
        )
    })?;
    if bytes as usize != size_of::<T>() {
        return Err(unsupported());
    }
    // SAFETY: successful ioctl initialized the entire documented pointer type.
    Ok(unsafe { out.assume_init() })
}
pub(super) fn duplicate(socket: &OwnedSocket) -> Result<OwnedSocket> {
    let mut info = WSAPROTOCOL_INFOW::default();
    // SAFETY: source is owned; target is this process; output layout is correct.
    check(unsafe {
        WSADuplicateSocketW(
            socket.as_raw_socket() as usize,
            std::process::id(),
            &mut info,
        )
    })?;
    reconstruct(&info)
}
pub(super) fn reconstruct(info: &WSAPROTOCOL_INFOW) -> Result<OwnedSocket> {
    startup()?;
    // SAFETY: protocol information obtained from cooperative WSADuplicateSocketW.
    let raw = unsafe {
        WSASocketW(
            FROM_PROTOCOL_INFO,
            FROM_PROTOCOL_INFO,
            FROM_PROTOCOL_INFO,
            info,
            0,
            WSA_FLAG_OVERLAPPED | WSA_FLAG_NO_HANDLE_INHERIT,
        )
    };
    if raw == INVALID_SOCKET {
        return Err(last_error());
    }
    // SAFETY: successful reconstruction owns one independent socket reference.
    Ok(unsafe { OwnedSocket::from_raw_socket(raw as _) })
}
impl Detached {
    /// Adopt an owned, quiescent socket. Existing IOCP associations are tolerated
    /// by routing overlapped-event completions to the receiving loop.
    pub fn from_socket(socket: OwnedSocket) -> Result<Self> {
        startup()?;
        let raw = socket.as_raw_socket() as usize;
        let mut typ = 0i32;
        let mut len = size_of::<i32>() as i32;
        // SAFETY: correct writable integer option buffer.
        check(unsafe {
            getsockopt(
                raw,
                SOL_SOCKET,
                SO_TYPE,
                ptr::from_mut(&mut typ).cast(),
                &mut len,
            )
        })?;
        let mut accepting = 0i32;
        // SAFETY: same valid output layout for SO_ACCEPTCONN.
        check(unsafe {
            getsockopt(
                raw,
                SOL_SOCKET,
                SO_ACCEPTCONN,
                ptr::from_mut(&mut accepting).cast(),
                &mut len,
            )
        })?;
        let kind = if typ == SOCK_DGRAM {
            Kind::Udp
        } else if typ == SOCK_STREAM {
            if accepting != 0 {
                Kind::Listener
            } else {
                Kind::Tcp
            }
        } else {
            return Err(unsupported());
        };
        Ok(Self::new(Native::Socket(socket), kind, true))
    }
}

#[cfg(all(test, not(loom)))]
mod tests {
    use super::*;
    #[test]
    fn loopback_rto_failure_is_best_effort() {
        startup().expect("Winsock");
        loopback_connect(INVALID_SOCKET);
        assert_eq!(last_error().os, Some(WSAENOTSOCK), "the failing ioctl ran");
        let socket = create(false, false).expect("valid socket after failure");
        bind_to(socket.as_raw_socket() as usize, ([127, 0, 0, 1], 0).into()).expect("bind");
        assert_ne!(
            address(socket.as_raw_socket() as usize, false)
                .expect("address")
                .port(),
            0
        );
    }
}
