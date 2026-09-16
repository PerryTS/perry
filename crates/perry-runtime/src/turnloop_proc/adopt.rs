//! Handing an already-created descriptor to the loop.
//!
//! P2 does not re-create the descriptors it moves. A dgram socket carries
//! Node's bind-time `SO_REUSEADDR`/`SO_REUSEPORT` choice and, afterwards, its
//! multicast membership and interface state; a child's pipes come out of a
//! `std::process::Command` whose `pre_exec` hooks turnloop's `ProcessSpec` has
//! no equivalent for. Re-creating either would mean re-deriving syscalls the
//! existing code already gets right, on the same commit that moves the wait.
//!
//! So the descriptor is created exactly as before and *adopted*:
//! `Detached::from_fd` on Unix, `from_socket` / `from_handle` on Windows,
//! then [`turnloop::Loop::attach`]. turnloop classifies it itself (socket
//! family and type via `getsockname` + `SO_TYPE`, otherwise `fstat` +
//! `isatty`), which is why nothing here has to tell it what kind of
//! descriptor it is getting.
//!
//! # Ownership
//!
//! Adoption **moves** the descriptor. The `OwnedFd`/`OwnedSocket`/`OwnedHandle`
//! is consumed, and from then on the driver closes it — so a caller that still
//! needs the descriptor for something the loop does not expose (dgram's
//! `setsockopt` surface) must hand over a **duplicate** and keep the original,
//! which is what [`crate::dgram_reactor`] does. `dup(2)` shares one open file
//! description, so an option set through the retained copy is the same socket
//! the driver is receiving on; `getsockname` through it answers about the same
//! binding. What it must *not* do is read or write there: turnloop sets
//! `O_NONBLOCK` on the description it adopts, and the duplicate sees that too.

use turnloop::Result as TlResult;

/// A descriptor on its way to the loop, in the form its platform names.
pub(crate) enum Transport {
    /// A Unix descriptor of any kind: pipe, socket, tty or file.
    #[cfg(unix)]
    Fd(std::os::fd::OwnedFd),
    /// A Windows kernel object: pipe, console or file.
    #[cfg(windows)]
    Handle(std::os::windows::io::OwnedHandle),
    /// A Windows socket, which is not a kernel object and has its own adopter.
    #[cfg(windows)]
    Socket(std::os::windows::io::OwnedSocket),
}

impl Transport {
    pub(crate) fn into_detached(self) -> TlResult<turnloop::Detached> {
        match self {
            #[cfg(unix)]
            Transport::Fd(fd) => turnloop::Detached::from_fd(fd),
            #[cfg(windows)]
            Transport::Handle(h) => turnloop::Detached::from_handle(h),
            #[cfg(windows)]
            Transport::Socket(s) => turnloop::Detached::from_socket(s),
        }
    }
}

/// Duplicate a borrowed descriptor so the loop can own one copy while the
/// caller keeps the other.
///
/// The duplicate shares the open file description, which is exactly what makes
/// it useful and exactly what makes it dangerous: options and the binding are
/// shared (wanted), and so is `O_NONBLOCK` (which is why the retained copy is
/// for `setsockopt`/`getsockname` only, never for I/O).
// Datagram-side surface of the P2 handle table: real, exercised by
// `turnloop_proc::tests`, and consumed in production only by
// `dgram_reactor`, which is `#[cfg(feature = "mod-dgram")]`. The gate
// stays LIVE in the configuration that has the consumer -- if
// `dgram_reactor` ever stops calling this, a `mod-dgram` build goes red.
#[cfg(unix)]
#[cfg_attr(not(feature = "mod-dgram"), allow(dead_code))]
pub(crate) fn duplicate_fd(fd: std::os::fd::BorrowedFd<'_>) -> std::io::Result<Transport> {
    use std::os::fd::AsRawFd;
    // SAFETY: `fd` is a live borrowed descriptor for the duration of the call,
    // and F_DUPFD_CLOEXEC returns a new owned descriptor or -1.
    let raw = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) };
    if raw < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `raw` is a fresh descriptor this call owns and nothing else holds.
    Ok(Transport::Fd(unsafe {
        <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(raw)
    }))
}

/// Windows counterpart: duplicate a socket into a second owned handle on the
/// same underlying socket, for the same "options here, I/O on the loop" split.
#[cfg(windows)]
pub(crate) fn duplicate_socket(
    socket: std::os::windows::io::BorrowedSocket<'_>,
) -> std::io::Result<Transport> {
    use std::os::windows::io::{AsRawSocket, FromRawSocket, OwnedSocket};
    use windows_sys::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    let mut duplicate: HANDLE = std::ptr::null_mut();
    // SAFETY: the source socket is live for the call and `duplicate` is
    // writable output storage; the pseudo-handle from GetCurrentProcess needs
    // no release.
    let ok = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            socket.as_raw_socket() as HANDLE,
            GetCurrentProcess(),
            &mut duplicate,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `duplicate` is a fresh handle this call owns.
    Ok(Transport::Socket(unsafe {
        OwnedSocket::from_raw_socket(duplicate as std::os::windows::raw::SOCKET)
    }))
}
