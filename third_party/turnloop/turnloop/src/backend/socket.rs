use super::poller::last_error;
use crate::{Error, ErrorKind, Result, ReusePort};
use std::{
    mem::{size_of, zeroed},
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
};

pub(crate) struct Addr {
    pub storage: libc::sockaddr_storage,
    pub len: libc::socklen_t,
}
impl Addr {
    pub fn new(addr: SocketAddr) -> Self {
        // SAFETY: sockaddr_storage is plain C integer/padding storage; zero is valid.
        let mut storage: libc::sockaddr_storage = unsafe { zeroed() };
        let len;
        match addr {
            SocketAddr::V4(a) => {
                // SAFETY: zero is a valid initial sockaddr_in representation.
                let mut s: libc::sockaddr_in = unsafe { zeroed() };
                s.sin_family = libc::AF_INET as _;
                s.sin_port = a.port().to_be();
                s.sin_addr.s_addr = u32::from_ne_bytes(a.ip().octets());
                len = size_of::<libc::sockaddr_in>() as libc::socklen_t;
                #[cfg(turnloop_backend = "kqueue")]
                {
                    s.sin_len = len as u8;
                }
                // SAFETY: storage is sufficiently large/aligned for sockaddr_in.
                unsafe {
                    std::ptr::write(
                        (&mut storage as *mut libc::sockaddr_storage).cast::<libc::sockaddr_in>(),
                        s,
                    );
                }
            }
            SocketAddr::V6(a) => {
                // SAFETY: zero is a valid initial sockaddr_in6 representation.
                let mut s: libc::sockaddr_in6 = unsafe { zeroed() };
                s.sin6_family = libc::AF_INET6 as _;
                s.sin6_port = a.port().to_be();
                s.sin6_flowinfo = a.flowinfo().to_be();
                s.sin6_scope_id = a.scope_id();
                s.sin6_addr.s6_addr = a.ip().octets();
                len = size_of::<libc::sockaddr_in6>() as libc::socklen_t;
                #[cfg(turnloop_backend = "kqueue")]
                {
                    s.sin6_len = len as u8;
                }
                // SAFETY: storage is sufficiently large/aligned for sockaddr_in6.
                unsafe {
                    std::ptr::write(
                        (&mut storage as *mut libc::sockaddr_storage).cast::<libc::sockaddr_in6>(),
                        s,
                    );
                }
            }
        }
        Self { storage, len }
    }
    pub fn empty() -> Self {
        Self {
            // SAFETY: sockaddr_storage can be zero-initialized as output storage.
            storage: unsafe { zeroed() },
            len: size_of::<libc::sockaddr_storage>() as _,
        }
    }
    pub fn ptr(&self) -> *const libc::sockaddr {
        (&self.storage as *const libc::sockaddr_storage).cast()
    }
    pub fn mut_ptr(&mut self) -> *mut libc::sockaddr {
        (&mut self.storage as *mut libc::sockaddr_storage).cast()
    }
    pub fn decode(&self) -> Result<SocketAddr> {
        match self.storage.ss_family as i32 {
            libc::AF_INET if self.len as usize >= size_of::<libc::sockaddr_in>() => {
                // SAFETY: family/length were checked; storage is aligned for sockaddr_in.
                let s = unsafe { &*self.ptr().cast::<libc::sockaddr_in>() };
                Ok(SocketAddr::V4(SocketAddrV4::new(
                    Ipv4Addr::from(s.sin_addr.s_addr.to_ne_bytes()),
                    u16::from_be(s.sin_port),
                )))
            }
            libc::AF_INET6 if self.len as usize >= size_of::<libc::sockaddr_in6>() => {
                // SAFETY: family/length were checked; storage is aligned for sockaddr_in6.
                let s = unsafe { &*self.ptr().cast::<libc::sockaddr_in6>() };
                Ok(SocketAddr::V6(SocketAddrV6::new(
                    Ipv6Addr::from(s.sin6_addr.s6_addr),
                    u16::from_be(s.sin6_port),
                    u32::from_be(s.sin6_flowinfo),
                    s.sin6_scope_id,
                )))
            }
            _ => Err(Error::new(ErrorKind::InvalidInput)),
        }
    }
}
/// The `SOL_SOCKET` option that honours a [`ReusePort`] request on this target,
/// or `None` when nothing needs to be set.
///
/// This is the single place the platform split lives. `SO_REUSEPORT` is spelled
/// identically on Linux and the BSDs and means different things:
///
/// * **Linux and Android** distribute incoming connections across every listener
///   bound with `SO_REUSEPORT`, selecting one by hashing the connection's
///   4-tuple. One option satisfies both requests.
/// * **FreeBSD** keeps `SO_REUSEPORT` at its original BSD meaning — duplicate
///   binding, with new connections going to the socket that bound last — and
///   added a separate `SO_REUSEPORT_LB` in 12.0 that distributes. So the two
///   requests are two different options here.
/// * **macOS and the other Apple platforms, NetBSD, OpenBSD and DragonFly** have
///   only the original option. A [`ReusePort::Distribute`] request is refused
///   rather than silently answered with `SO_REUSEPORT`: measured on macOS 26.5,
///   two loops binding one port with `SO_REUSEPORT` split 32 connections
///   `[0, 32]` — the first listener is not merely under-served, it is never
///   given anything at all.
pub(crate) fn reuse_port_option(reuse: ReusePort) -> Result<Option<i32>> {
    match reuse {
        ReusePort::No => Ok(None),
        ReusePort::Share => Ok(Some(libc::SO_REUSEPORT)),
        #[cfg(any(target_os = "linux", target_os = "android"))]
        ReusePort::Distribute => Ok(Some(libc::SO_REUSEPORT)),
        #[cfg(target_os = "freebsd")]
        ReusePort::Distribute => Ok(Some(libc::SO_REUSEPORT_LB)),
        #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "freebsd")))]
        ReusePort::Distribute => Err(Error::new(ErrorKind::Unsupported)),
    }
}
pub(crate) fn option(fd: RawFd, level: i32, name: i32, value: i32) -> Result<()> {
    // SAFETY: value is an initialized integer of the size passed to setsockopt.
    if unsafe {
        libc::setsockopt(
            fd,
            level,
            name,
            (&value as *const i32).cast(),
            size_of::<i32>() as _,
        )
    } < 0
    {
        return Err(last_error());
    }
    Ok(())
}
pub(crate) fn configure(fd: RawFd) -> Result<()> {
    // SAFETY: F_GETFL uses a valid descriptor and no pointer argument.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(last_error());
    }
    // SAFETY: F_SETFL uses integer flags and a valid descriptor.
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(last_error());
    }
    // SAFETY: F_SETFD uses integer flags and a valid descriptor.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(last_error());
    }
    #[cfg(turnloop_backend = "kqueue")]
    option(fd, libc::SOL_SOCKET, libc::SO_NOSIGPIPE, 1)?;
    Ok(())
}
pub(crate) fn create(addr: SocketAddr, udp: bool) -> Result<OwnedFd> {
    let domain = if addr.is_ipv4() {
        libc::AF_INET
    } else {
        libc::AF_INET6
    };
    let kind = if udp {
        libc::SOCK_DGRAM
    } else {
        libc::SOCK_STREAM
    };
    #[cfg(turnloop_backend = "epoll")]
    let kind = kind | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK;
    // SAFETY: socket arguments are valid constants and no pointer is supplied.
    let fd = unsafe { libc::socket(domain, kind, 0) };
    if fd < 0 {
        return Err(last_error());
    }
    // SAFETY: socket returned a new exclusively owned descriptor.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    configure(fd.as_raw_fd())?;
    Ok(fd)
}
pub(crate) fn local_addr(fd: RawFd) -> Result<SocketAddr> {
    let mut a = Addr::empty();
    // SAFETY: output address storage and initialized capacity are valid for the call.
    if unsafe { libc::getsockname(fd, a.mut_ptr(), &mut a.len) } < 0 {
        return Err(last_error());
    }
    a.decode()
}
pub(crate) fn accept(fd: RawFd) -> Result<(OwnedFd, SocketAddr)> {
    let mut a = Addr::empty();
    // SAFETY: valid listener; output address storage and length are writable.
    let conn = unsafe { libc::accept(fd, a.mut_ptr(), &mut a.len) };
    if conn < 0 {
        return Err(last_error());
    }
    // SAFETY: accept transferred exclusive ownership of the new descriptor.
    let conn = unsafe { OwnedFd::from_raw_fd(conn) };
    configure(conn.as_raw_fd())?;
    Ok((conn, a.decode()?))
}
