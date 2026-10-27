use super::*;
use futures_util::io::{AsyncRead, AsyncWrite};
use std::{future::poll_fn, pin::Pin};

// AsyncIo has one completion slot per direction. Serializing access prevents
// one caller from taking another caller's bytes or replacing its wakeup.
struct Direction {
    local: perry_component_io::LocalResource<NativeTcp>,
    write: bool,
    complete: bool,
}
impl Direction {
    fn acquire(
        local: perry_component_io::LocalResource<NativeTcp>,
        write: bool,
    ) -> Result<Self, Failure> {
        {
            let mut state = local.borrow_mut();
            if if write { state.writing } else { state.reading } {
                return Err(Failure::Socket(ErrorCode::ConcurrencyConflict));
            }
            if write && state.write_cancelled {
                return Err(Failure::Socket(ErrorCode::InvalidState));
            }
            if write {
                state.writing = true;
            } else {
                state.reading = true;
            }
        }
        Ok(Self {
            local,
            write,
            complete: false,
        })
    }
}
impl Drop for Direction {
    fn drop(&mut self) {
        let mut state = self.local.borrow_mut();
        if self.write {
            state.writing = false;
            // A partial write may already have reached the peer. A new caller
            // must not mistake its completion for acceptance of different bytes.
            state.write_cancelled |= !self.complete;
        } else {
            state.reading = false;
        }
    }
}

/// Native socket failures are typed WASI errors; loss of the driver is a trap.
#[derive(Clone, Debug)]
pub enum Failure {
    Socket(ErrorCode),
    // Preserve the existing P2 code while allowing P3 to report connection-broken.
    BrokenPipe(ErrorCode),
    Driver(perry_component_io::Error),
}
impl Failure {
    pub fn socket(self) -> SocketError {
        match self {
            Self::Socket(code) | Self::BrokenPipe(code) => code.into(),
            Self::Driver(error) => SocketError::trap(error),
        }
    }
}
impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        if let Some(error) = error
            .get_ref()
            .and_then(|e| e.downcast_ref::<turnloop::Error>())
        {
            return Self::from(*error);
        }
        if error.kind() == std::io::ErrorKind::BrokenPipe {
            Self::BrokenPipe(ErrorCode::from(error))
        } else {
            Self::Socket(ErrorCode::from(error))
        }
    }
}
impl From<turnloop::Error> for Failure {
    fn from(error: turnloop::Error) -> Self {
        use turnloop::ErrorKind as K;
        if matches!(error.kind, K::ResourceLimit | K::Cancelled) {
            return Self::Driver(if error.kind == K::ResourceLimit {
                perry_component_io::Error::Capacity
            } else {
                perry_component_io::Error::Cancelled
            });
        }
        if let Some(code) = error.os {
            return std::io::Error::from_raw_os_error(code).into();
        }
        if error.kind == K::BrokenPipe {
            return Self::BrokenPipe(ErrorCode::ConnectionReset);
        }
        Self::Socket(match error.kind {
            K::PermissionDenied => ErrorCode::AccessDenied,
            K::Unsupported => ErrorCode::NotSupported,
            K::InvalidInput => ErrorCode::InvalidArgument,
            K::WouldBlock => ErrorCode::WouldBlock,
            K::TimedOut => ErrorCode::Timeout,
            K::ConnectionRefused => ErrorCode::ConnectionRefused,
            K::ConnectionReset => ErrorCode::ConnectionReset,
            _ => ErrorCode::Unknown,
        })
    }
}

pub(super) fn detach(socket: Socket, listener: bool) -> Result<turnloop::Detached, Failure> {
    #[cfg(unix)]
    let result = if listener {
        turnloop::Detached::from_listener_fd(socket.into())
    } else {
        turnloop::Detached::from_fd(socket.into())
    };
    #[cfg(windows)]
    let result = {
        let _ = listener;
        turnloop::Detached::from_socket(socket.into())
    };
    result.map_err(Into::into)
}
pub(super) fn local(socket: &Resource<NativeTcp>) -> SocketResult<SocketAddr> {
    let task = socket
        .call(|io, local| async move { io.driver().local_addr(local.borrow_mut().stream.handle()) })
        .map_err(SocketError::trap)?;
    futures_lite::future::block_on(task)
        .map_err(SocketError::trap)?
        .map_err(|e| Failure::from(e).socket())
}
pub(super) fn peer(socket: &Resource<NativeTcp>) -> SocketResult<SocketAddr> {
    let task = socket
        .call(|io, local| async move {
            let local = local.borrow_mut();
            let driver = io.driver();
            let raw = driver
                .raw_transport(local.stream.handle())
                .map_err(Failure::from)?;
            #[cfg(unix)]
            let address = {
                use std::os::fd::BorrowedFd;
                let turnloop::RawTransport::Fd(fd) = raw else {
                    return Err(Failure::Socket(ErrorCode::InvalidState));
                };
                // SAFETY: this driver borrow and native owner keep the descriptor
                // live. SockRef only queries peer_addr; no ownership, I/O or mutation.
                let fd = unsafe { BorrowedFd::borrow_raw(fd) };
                socket2::SockRef::from(&fd).peer_addr()?
            };
            #[cfg(windows)]
            let address = {
                use std::os::windows::io::BorrowedSocket;
                let turnloop::RawTransport::Socket(socket) = raw else {
                    return Err(Failure::Socket(ErrorCode::InvalidState));
                };
                // SAFETY: live socket protected by the driver borrow and native
                // owner. The borrowed socket is used only for a read-only query.
                let socket = unsafe { BorrowedSocket::borrow_raw(socket) };
                socket2::SockRef::from(&socket).peer_addr()?
            };
            address
                .as_socket()
                .ok_or(Failure::Socket(ErrorCode::InvalidState))
        })
        .map_err(SocketError::trap)?;
    futures_lite::future::block_on(task)
        .map_err(SocketError::trap)?
        .map_err(Failure::socket)
}
pub(super) fn get_option(
    socket: &Resource<NativeTcp>,
    kind: SocketOptionKind,
) -> SocketResult<SocketOption> {
    let task = socket
        .call(move |io, local| async move {
            io.driver()
                .get_option(local.borrow_mut().stream.handle(), kind)
        })
        .map_err(SocketError::trap)?;
    futures_lite::future::block_on(task)
        .map_err(SocketError::trap)?
        .map_err(|e| Failure::from(e).socket())
}
pub(super) fn set_option(socket: &Resource<NativeTcp>, option: SocketOption) -> SocketResult<()> {
    let task = socket
        .call(move |io, local| async move {
            let mut native = local.borrow_mut();
            io.driver().set_option(native.stream.handle(), option)?;
            remember_option(&mut native.inherited, option);
            Ok::<_, turnloop::Error>(())
        })
        .map_err(SocketError::trap)?;
    futures_lite::future::block_on(task)
        .map_err(SocketError::trap)?
        .map_err(|e| Failure::from(e).socket())
}
pub(super) fn read(
    socket: &Resource<NativeTcp>,
    size: usize,
) -> Result<ResourceRequest<Result<Vec<u8>, Failure>>, perry_component_io::Error> {
    socket.call(move |_, local| async move {
        let _direction = Direction::acquire(local.clone(), false)?;
        let mut bytes = vec![0; size.min(crate::MAX_READ_SIZE_ALLOC)];
        let count =
            poll_fn(|cx| Pin::new(&mut local.borrow_mut().stream).poll_read(cx, &mut bytes))
                .await?;
        bytes.truncate(count);
        Ok(bytes)
    })
}
pub(super) fn write(
    socket: &Resource<NativeTcp>,
    bytes: Vec<u8>,
) -> Result<ResourceRequest<Result<(), Failure>>, perry_component_io::Error> {
    socket.call(move |_, local| async move {
        let mut direction = Direction::acquire(local.clone(), true)?;
        let mut offset = 0;
        while offset < bytes.len() {
            let count = poll_fn(|cx| {
                Pin::new(&mut local.borrow_mut().stream).poll_write(cx, &bytes[offset..])
            })
            .await?;
            if count == 0 {
                return Err(std::io::Error::from(std::io::ErrorKind::WriteZero).into());
            }
            offset += count;
        }
        poll_fn(|cx| Pin::new(&mut local.borrow_mut().stream).poll_flush(cx)).await?;
        direction.complete = true;
        Ok(())
    })
}
