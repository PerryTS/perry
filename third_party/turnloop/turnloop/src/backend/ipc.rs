//! AF_UNIX addressing, descriptor transfer and non-socket stream syscalls.
use super::{
    poller::last_error,
    socket::{self, Addr},
    unix::{Detached, Kind},
};
use crate::{Error, ErrorKind, ListenOpts, PipeName, Result};
use std::{
    mem::{size_of, zeroed},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::ffi::OsStrExt,
    },
};

pub(super) fn open(name: &PipeName, listen: Option<ListenOpts>) -> Result<(Detached, Addr)> {
    let bytes = name.0.as_os_str().as_bytes();
    // SAFETY: sockaddr_un is plain C storage valid when zero initialized.
    let mut un: libc::sockaddr_un = unsafe { zeroed() };
    if bytes.is_empty() || bytes.contains(&0) || bytes.len() >= un.sun_path.len() {
        return Err(Error::new(ErrorKind::InvalidInput));
    }
    un.sun_family = libc::AF_UNIX as _;
    for (slot, &b) in un.sun_path.iter_mut().zip(bytes) {
        *slot = b as _;
    }
    let len =
        (std::mem::offset_of!(libc::sockaddr_un, sun_path) + bytes.len() + 1) as libc::socklen_t;
    #[cfg(turnloop_backend = "kqueue")]
    {
        un.sun_len = len as u8;
    }
    let mut addr = Addr::empty();
    addr.len = len;
    // SAFETY: sockaddr_storage is large and aligned enough for sockaddr_un.
    unsafe {
        std::ptr::write(addr.mut_ptr().cast::<libc::sockaddr_un>(), un);
    }
    // SAFETY: valid domain/type constants, no pointer arguments.
    let raw = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if raw < 0 {
        return Err(last_error());
    }
    // SAFETY: socket returned exclusive ownership of a fresh descriptor.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    socket::configure(raw)?;
    if let Some(opts) = listen {
        if opts.backlog > i32::MAX as u32 {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        // A filesystem socket has no port for a second bind to contend for, so
        // there is no SO_REUSEPORT for AF_UNIX on any platform. Report that
        // rather than binding a listener the request was never applied to.
        if opts.reuse_port.is_enabled() {
            return Err(Error::new(ErrorKind::Unsupported));
        }
        // SAFETY: initialized sockaddr of the advertised length, live socket.
        if unsafe { libc::bind(raw, addr.ptr(), len) } < 0 {
            return Err(last_error());
        }
        // SAFETY: bound stream socket and checked integer backlog.
        if unsafe { libc::listen(raw, opts.backlog as i32) } < 0 {
            return Err(last_error());
        }
    }
    Ok((
        Detached::new(
            fd,
            if listen.is_some() {
                Kind::PipeListener
            } else {
                Kind::Pipe
            },
        ),
        addr,
    ))
}

pub(super) fn accept(fd: RawFd) -> Result<Detached> {
    // SAFETY: live listener; null address outputs are permitted by accept.
    let raw = unsafe { libc::accept(fd, std::ptr::null_mut(), std::ptr::null_mut()) };
    if raw < 0 {
        return Err(last_error());
    }
    // SAFETY: accept returned a fresh exclusively owned descriptor.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    socket::configure(raw)?;
    Ok(Detached::new(fd, Kind::Pipe))
}

pub(super) fn stdio(raw: RawFd) -> Result<Detached> {
    // SAFETY: integer fcntl command duplicates the borrowed host fd with CLOEXEC.
    let dup = unsafe { libc::fcntl(raw, libc::F_DUPFD_CLOEXEC, 3) };
    if dup < 0 {
        return Err(last_error());
    }
    // SAFETY: successful fcntl returns a fresh owned descriptor.
    let fd = unsafe { OwnedFd::from_raw_fd(dup) };
    let (lease, mode) = super::stdio::Lease::acquire(raw)?;
    let mut transport = classify(fd)?;
    transport.share_stdio(lease, mode);
    Ok(transport)
}

pub(super) fn classify(fd: OwnedFd) -> Result<Detached> {
    classify_hint(fd, false)
}
/// Adopt a descriptor the caller says is a listening stream socket, and refuse
/// anything that is provably not one.
///
/// epoll asks the kernel with `SO_ACCEPTCONN`. kqueue platforms do not support
/// that option (macOS answers `ENOPROTOOPT`), so there the caller's word is
/// taken - except that a socket with a peer is connected, and a listener never
/// has one, so that case is still refused.
pub(super) fn classify_listener(fd: OwnedFd) -> Result<Detached> {
    #[cfg(turnloop_backend = "kqueue")]
    {
        let mut peer = Addr::empty();
        // SAFETY: initialized sockaddr output storage and length on a live fd.
        if unsafe { libc::getpeername(fd.as_raw_fd(), peer.mut_ptr(), &mut peer.len) } == 0 {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
    }
    let transport = classify_hint(fd, true)?;
    if !matches!(transport.kind, Kind::Listener | Kind::PipeListener) {
        return Err(Error::new(ErrorKind::InvalidInput));
    }
    Ok(transport)
}
fn classify_hint(fd: OwnedFd, listener: bool) -> Result<Detached> {
    // SAFETY: stat is plain output storage and all-zero is a valid initial value.
    let mut stat: libc::stat = unsafe { zeroed() };
    // SAFETY: writable stat storage and a live owned fd.
    if unsafe { libc::fstat(fd.as_raw_fd(), &mut stat) } < 0 {
        return Err(last_error());
    }
    let kind = if stat.st_mode & libc::S_IFMT == libc::S_IFSOCK {
        let addr = Addr::empty();
        let mut addr = addr;
        // SAFETY: initialized sockaddr output storage and length.
        if unsafe { libc::getsockname(fd.as_raw_fd(), addr.mut_ptr(), &mut addr.len) } < 0 {
            return Err(last_error());
        }
        let socket_type = get_option(fd.as_raw_fd(), libc::SO_TYPE)?;
        #[cfg(turnloop_backend = "epoll")]
        let listening = get_option(fd.as_raw_fd(), libc::SO_ACCEPTCONN)? != 0;
        #[cfg(turnloop_backend = "kqueue")]
        let listening = listener;
        #[cfg(turnloop_backend = "epoll")]
        let _ = listener;
        match (addr.storage.ss_family as i32, socket_type, listening) {
            (libc::AF_UNIX, libc::SOCK_STREAM, false) => Kind::Pipe,
            (libc::AF_UNIX, libc::SOCK_STREAM, true) => Kind::PipeListener,
            (libc::AF_INET | libc::AF_INET6, libc::SOCK_STREAM, false) => Kind::Tcp,
            (libc::AF_INET | libc::AF_INET6, libc::SOCK_STREAM, true) => Kind::Listener,
            (libc::AF_INET | libc::AF_INET6, libc::SOCK_DGRAM, _) => Kind::Udp,
            _ => return Err(Error::new(ErrorKind::Unsupported)),
        }
    } else if stat.st_mode & libc::S_IFMT == libc::S_IFREG {
        Kind::File
    } else if stat.st_mode & libc::S_IFMT == libc::S_IFCHR {
        // SAFETY: isatty only inspects the live owned descriptor.
        if unsafe { libc::isatty(fd.as_raw_fd()) } == 1 {
            Kind::Stream
        } else {
            Kind::File
        }
    } else {
        Kind::Stream
    };
    // SAFETY: live descriptor and integer-only fcntl command.
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 {
        return Err(last_error());
    }
    // SAFETY: live descriptor and valid status flags.
    if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(last_error());
    }
    if matches!(
        kind,
        Kind::Tcp | Kind::Listener | Kind::Udp | Kind::Pipe | Kind::PipeListener
    ) {
        socket::configure(fd.as_raw_fd())?;
    }
    let mut transport = Detached::new(fd, kind);
    if matches!(kind, Kind::Stream | Kind::File) {
        transport.original_flags = Some(flags);
    }
    Ok(transport)
}
fn get_option(fd: RawFd, name: i32) -> Result<i32> {
    let mut value: i32 = 0;
    let mut len = size_of::<i32>() as libc::socklen_t;
    // SAFETY: initialized integer output and corresponding length.
    if unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            name,
            (&mut value as *mut i32).cast(),
            &mut len,
        )
    } < 0
    {
        return Err(last_error());
    }
    Ok(value)
}

// Enough aligned storage to parse and close every descriptor in a truncated packet.
#[repr(C)]
struct Control {
    _align: [libc::cmsghdr; 0],
    bytes: [u8; 256],
}

pub(super) fn send(fd: RawFd, passed: &Detached) -> Result<()> {
    let mut byte = if matches!(passed.kind, Kind::Listener | Kind::PipeListener) {
        0x55u8
    } else {
        0x54u8
    };
    let mut iov = libc::iovec {
        iov_base: (&mut byte as *mut u8).cast(),
        iov_len: 1,
    };
    let mut control = Control {
        _align: [],
        bytes: [0; 256],
    };
    // SAFETY: zero initializes msghdr; its referenced stack storage lives for sendmsg.
    let mut msg: libc::msghdr = unsafe { zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.bytes.as_mut_ptr().cast();
    // SAFETY: CMSG_SPACE/LEN compute sizes for one int; storage has sufficient alignment/capacity.
    unsafe {
        msg.msg_controllen = libc::CMSG_SPACE(size_of::<i32>() as _) as _;
        let c = libc::CMSG_FIRSTHDR(&msg);
        (*c).cmsg_level = libc::SOL_SOCKET;
        (*c).cmsg_type = libc::SCM_RIGHTS;
        (*c).cmsg_len = libc::CMSG_LEN(size_of::<i32>() as _) as _;
        std::ptr::write_unaligned(libc::CMSG_DATA(c).cast::<i32>(), passed.fd.as_raw_fd());
    }
    #[cfg(turnloop_backend = "epoll")]
    let flags = libc::MSG_NOSIGNAL;
    #[cfg(turnloop_backend = "kqueue")]
    let flags = 0;
    // SAFETY: all msghdr regions are initialized, stack-owned and live for this call.
    let n = unsafe { libc::sendmsg(fd, &msg, flags) };
    if n < 0 {
        return Err(last_error());
    }
    if n != 1 {
        return Err(Error::new(ErrorKind::BrokenPipe));
    }
    Ok(())
}
pub(super) fn receive(fd: RawFd) -> Result<Detached> {
    let mut byte = 0u8;
    let mut iov = libc::iovec {
        iov_base: (&mut byte as *mut u8).cast(),
        iov_len: 1,
    };
    let mut control = Control {
        _align: [],
        bytes: [0; 256],
    };
    // SAFETY: zeroed msghdr is valid before its buffer fields are populated.
    let mut msg: libc::msghdr = unsafe { zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.bytes.as_mut_ptr().cast();
    msg.msg_controllen = control.bytes.len() as _;
    #[cfg(turnloop_backend = "epoll")]
    let flags = libc::MSG_CMSG_CLOEXEC;
    #[cfg(turnloop_backend = "kqueue")]
    let flags = 0;
    // SAFETY: exclusive writable message, payload and ancillary output storage.
    let n = unsafe { libc::recvmsg(fd, &mut msg, flags) };
    if n < 0 {
        return Err(last_error());
    }
    let mut received = None;
    let mut count = 0;
    // SAFETY: kernel initialized headers inside msg_controllen. CMSG traversal
    // bounds each header; descriptor integers are read unaligned as ABI permits.
    unsafe {
        let mut c = libc::CMSG_FIRSTHDR(&msg);
        while !c.is_null() {
            if (*c).cmsg_level == libc::SOL_SOCKET && (*c).cmsg_type == libc::SCM_RIGHTS {
                let size = ((*c).cmsg_len as usize).saturating_sub(libc::CMSG_LEN(0) as usize);
                for i in 0..size / size_of::<i32>() {
                    let raw = std::ptr::read_unaligned(libc::CMSG_DATA(c).cast::<i32>().add(i));
                    let owned = OwnedFd::from_raw_fd(raw);
                    count += 1;
                    if received.is_none() {
                        received = Some(owned);
                    }
                }
            }
            c = libc::CMSG_NXTHDR(&msg, c);
        }
    }
    if n != 1 || !matches!(byte, 0x54 | 0x55) || count != 1 || msg.msg_flags & libc::MSG_CTRUNC != 0
    {
        return Err(Error::new(ErrorKind::InvalidInput));
    }
    let received = received.ok_or(Error::new(ErrorKind::InvalidInput))?;
    // SAFETY: received fd is owned; CLOEXEC prevents subsequent child inheritance.
    if unsafe { libc::fcntl(received.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(last_error());
    }
    let transport = classify_hint(received, byte == 0x55)?;
    if matches!(transport.kind, Kind::Stream | Kind::File) {
        return Err(Error::new(ErrorKind::Unsupported));
    }
    Ok(transport)
}

unsafe fn without_sigpipe(f: impl FnOnce() -> isize) -> isize {
    // SAFETY: zeroed signal sets are initialized with sigemptyset before use.
    let (mut block, mut old, mut pending): (libc::sigset_t, libc::sigset_t, libc::sigset_t) =
        unsafe { zeroed() };
    // SAFETY: all signal set pointers are writable and initialized; pthread_sigmask
    // changes only this thread. Consume only a newly generated SIGPIPE on EPIPE.
    unsafe {
        libc::sigemptyset(&mut block);
        libc::sigaddset(&mut block, libc::SIGPIPE);
        let error = libc::pthread_sigmask(libc::SIG_BLOCK, &block, &mut old);
        if error != 0 {
            *errno() = error;
            return -1;
        }
        libc::sigpending(&mut pending);
        let had = libc::sigismember(&pending, libc::SIGPIPE) == 1;
        let n = f();
        let saved = *errno();
        if n < 0 && saved == libc::EPIPE && !had {
            libc::sigpending(&mut pending);
            if libc::sigismember(&pending, libc::SIGPIPE) == 1 {
                let mut sig = 0;
                libc::sigwait(&block, &mut sig);
            }
        }
        libc::pthread_sigmask(libc::SIG_SETMASK, &old, std::ptr::null_mut());
        *errno() = saved;
        n
    }
}
pub(super) unsafe fn errno() -> *mut i32 {
    #[cfg(turnloop_backend = "kqueue")]
    // SAFETY: libc returns this thread's live errno storage.
    unsafe {
        libc::__error()
    }
    #[cfg(all(turnloop_backend = "epoll", not(target_os = "android")))]
    // SAFETY: libc returns this thread's live errno storage.
    unsafe {
        libc::__errno_location()
    }
    #[cfg(target_os = "android")]
    // SAFETY: Bionic returns this thread's live errno storage.
    unsafe {
        libc::__errno()
    }
}
pub(super) unsafe fn write(fd: RawFd, ptr: *const libc::c_void, len: usize) -> isize {
    // SAFETY: caller guarantees a readable buffer for the synchronous write.
    unsafe { without_sigpipe(|| libc::write(fd, ptr, len)) }
}
pub(super) unsafe fn writev(fd: RawFd, iov: *const libc::iovec, count: i32) -> isize {
    // SAFETY: caller guarantees all iovecs and their data live through writev.
    unsafe { without_sigpipe(|| libc::writev(fd, iov, count)) }
}
