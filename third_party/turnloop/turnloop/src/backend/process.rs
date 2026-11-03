//! Unix child-descriptor plumbing: the extra descriptors a spec asks for, and
//! the relocation that makes the child hook's dup2 sequence order-independent.
use super::poller::last_error;
use crate::{Error, ErrorKind, Result};
use std::os::{
    fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
    unix::net::UnixStream,
};

/// A descriptor on the platform null device, opened for reading and writing.
pub(super) fn null() -> Result<OwnedFd> {
    // SAFETY: constant NUL-terminated device path and integer flags only.
    let raw = unsafe { libc::open(c"/dev/null".as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
    if raw < 0 {
        return Err(last_error());
    }
    // SAFETY: open returned a fresh exclusively owned descriptor.
    Ok(unsafe { OwnedFd::from_raw_fd(raw) })
}

/// A connected bidirectional stream pair as `(parent end, child end)`.
///
/// libuv creates every child pipe this way, so a child's extra descriptor
/// behaves the same here as it does under Node, including descriptor passing
/// over an IPC channel.
pub(super) fn stream_pair() -> Result<(OwnedFd, OwnedFd)> {
    let (parent, child) = UnixStream::pair().map_err(Error::from)?;
    Ok((OwnedFd::from(parent), OwnedFd::from(child)))
}

/// A one-way pipe as `(parent read end, child write end)`.
///
/// A real pipe, not a half-shut socket pair: Darwin refuses `SHUT_RD` on a
/// `socketpair` end with `ENOTCONN`, so the direction has to come from the
/// object rather than from a later call.
pub(super) fn one_way() -> Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0 as RawFd; 2];
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "freebsd"))]
    // SAFETY: writable pair of descriptor outputs and an integer flag.
    let created = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "freebsd")))]
    // SAFETY: writable pair of descriptor outputs; close-on-exec is set below.
    let created = unsafe { libc::pipe(fds.as_mut_ptr()) };
    if created < 0 {
        return Err(last_error());
    }
    // SAFETY: pipe transferred ownership of two fresh descriptors.
    let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "freebsd")))]
    for fd in [&read, &write] {
        // SAFETY: live owned descriptor and integer-only fcntl arguments.
        if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return Err(last_error());
        }
    }
    Ok((read, write))
}

/// Move a child-side descriptor to at least `floor`, keeping close-on-exec.
///
/// Every source then sits above every target, so the child hook can dup2 them
/// into place in any order without a later source having been overwritten by an
/// earlier target. The original descriptor is closed by the returned value's
/// replacement, never leaked.
pub(super) fn lift(fd: OwnedFd, floor: RawFd) -> Result<OwnedFd> {
    if fd.as_raw_fd() >= floor {
        return Ok(fd);
    }
    // SAFETY: live owned descriptor and an integer-only fcntl command.
    let raw = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, floor) };
    if raw < 0 {
        return Err(last_error());
    }
    // SAFETY: F_DUPFD_CLOEXEC returned a fresh exclusively owned descriptor.
    Ok(unsafe { OwnedFd::from_raw_fd(raw) })
}

/// Hold every free target number in the parent until the child has forked.
///
/// `Command::spawn` creates its exec-error pipe after the standard streams and
/// before the fork, and the kernel gives it the lowest free numbers — which is
/// exactly what the sources vacate when they are lifted above the targets. If
/// that pipe landed on a target number, the child hook would `dup2` over it, and
/// a failed `exec` would be reported to the parent as a successful spawn. A
/// number the parent is already using cannot be handed out either, so only the
/// free ones need holding, and each is released as soon as the child exists.
pub(super) fn reserve(donor: RawFd, numbers: impl Iterator<Item = RawFd>) -> Result<Vec<OwnedFd>> {
    let mut held = Vec::new();
    for number in numbers {
        // SAFETY: integer-only query of one descriptor's flags.
        if unsafe { libc::fcntl(number, libc::F_GETFD) } >= 0 {
            continue;
        }
        if last_error().os != Some(libc::EBADF) {
            return Err(last_error());
        }
        // SAFETY: live donor descriptor, lifted above every target, and a
        // number this process is not using.
        if unsafe { libc::dup2(donor, number) } < 0 {
            return Err(last_error());
        }
        // SAFETY: dup2 created this descriptor and nothing else owns it.
        held.push(unsafe { OwnedFd::from_raw_fd(number) });
        // SAFETY: live owned descriptor and integer-only fcntl arguments.
        if unsafe { libc::fcntl(number, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return Err(last_error());
        }
    }
    Ok(held)
}

/// Reject a spec whose extra descriptors this platform cannot place.
pub(super) fn checked_floor(numbers: impl Iterator<Item = u32>) -> Result<RawFd> {
    let mut floor = 0;
    for number in numbers {
        let number = RawFd::try_from(number).map_err(|_| Error::new(ErrorKind::InvalidInput))?;
        floor = floor.max(number);
    }
    floor
        .checked_add(1)
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput))
}
