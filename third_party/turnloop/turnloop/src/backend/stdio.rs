//! Process stdio duplicates share open-file descriptions, even across loops.
//! Snapshot all three before changing any: stdout and stderr may be aliases.
use super::poller::last_error;
use crate::{Error, ErrorKind, Result};
use std::{
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    sync::Mutex,
};

static STATE: Mutex<Option<State>> = Mutex::new(None);

struct Saved {
    fd: OwnedFd,
    flags: i32,
    mode: Option<libc::termios>,
}
struct State {
    users: usize,
    saved: [Option<Saved>; 3],
}

/// Held by the native resource until all its operations have quiesced.
pub(super) struct Lease;
impl Lease {
    pub(super) fn acquire(raw: i32) -> Result<(Self, Option<libc::termios>)> {
        let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(state) = state.as_mut() {
            state.users = state
                .users
                .checked_add(1)
                .ok_or_else(|| Error::new(ErrorKind::ResourceLimit))?;
        } else {
            let mut saved = [None, None, None];
            for (raw, slot) in saved.iter_mut().enumerate() {
                // SAFETY: duplicates a borrowed descriptor. Start above stdio so
                // a closed standard descriptor is never accidentally populated.
                let dup = unsafe { libc::fcntl(raw as i32, libc::F_DUPFD_CLOEXEC, 3) };
                if dup < 0 {
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() == Some(libc::EBADF) {
                        continue;
                    }
                    return Err(error.into());
                }
                // SAFETY: successful fcntl returned a fresh owned descriptor.
                let fd = unsafe { OwnedFd::from_raw_fd(dup) };
                // SAFETY: integer-only command on a live descriptor.
                let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
                if flags < 0 {
                    return Err(last_error());
                }
                // SAFETY: termios is plain C output storage.
                let mut mode = unsafe { std::mem::zeroed() };
                // SAFETY: live descriptor and writable termios storage.
                let mode =
                    (unsafe { libc::tcgetattr(fd.as_raw_fd(), &mut mode) } == 0).then_some(mode);
                *slot = Some(Saved { fd, flags, mode });
            }
            *state = Some(State { users: 1, saved });
        }
        let mode = state
            .as_ref()
            .and_then(|s| s.saved[raw as usize].as_ref())
            .and_then(|s| s.mode);
        Ok((Self, mode))
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        let Some(active) = state.as_mut() else {
            return;
        };
        active.users -= 1;
        if active.users != 0 {
            return;
        }
        // Serialize final restoration with the next acquisition. An Arc/Weak
        // registry alone would let a new cohort snapshot flags while the last
        // old owner is still restoring them in its destructor.
        for saved in active.saved.iter().flatten() {
            if let Some(mode) = &saved.mode {
                // SAFETY: retained descriptor and previously captured termios.
                unsafe { libc::tcsetattr(saved.fd.as_raw_fd(), libc::TCSANOW, mode) };
            }
            // SAFETY: retained descriptor and flags from F_GETFL.
            unsafe { libc::fcntl(saved.fd.as_raw_fd(), libc::F_SETFL, saved.flags) };
        }
        *state = None;
    }
}
