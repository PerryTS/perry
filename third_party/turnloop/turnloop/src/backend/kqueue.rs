//! EV_CLEAR readiness and an EVFILT_USER wake registered on the same kqueue.
use crate::{
    Result,
    backend::{
        PollInfo, Wake,
        poller::{Poller, Ready, last_error, timespec},
    },
};
use std::{
    os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
pub struct KqueueWake {
    fd: OwnedFd,
    calls: AtomicU64,
}
impl Wake for KqueueWake {
    fn wake(&self) -> Result<()> {
        let ev = event(0, libc::EVFILT_USER, 0, libc::NOTE_TRIGGER, 0);
        loop {
            self.calls.fetch_add(1, Ordering::Relaxed);
            // SAFETY: kqueue is owned by this Arc; the change is valid for the call.
            let n = unsafe {
                libc::kevent(
                    self.fd.as_raw_fd(),
                    &ev,
                    1,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null(),
                )
            };
            if n >= 0 {
                return Ok(());
            }
            let e = std::io::Error::last_os_error();
            if e.kind() != std::io::ErrorKind::Interrupted {
                return Err(e.into());
            }
        }
    }
    fn syscall_count(&self) -> u64 {
        self.calls.load(Ordering::Relaxed)
    }
}
pub(crate) struct Kqueue {
    wake: Arc<KqueueWake>,
    events: Vec<libc::kevent>,
}
fn event(ident: usize, filter: i16, flags: u16, fflags: u32, key: u64) -> libc::kevent {
    libc::kevent {
        ident,
        filter,
        flags,
        fflags,
        data: 0,
        udata: key as usize as *mut libc::c_void,
    }
}
impl Poller for Kqueue {
    type W = KqueueWake;
    fn new(capacity: usize) -> Result<Self> {
        // SAFETY: kqueue takes no pointers and returns a new owned descriptor.
        let fd = unsafe { libc::kqueue() };
        if fd < 0 {
            return Err(last_error());
        }
        // SAFETY: successful kqueue returned exclusive ownership of fd.
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        // SAFETY: valid owned descriptor, integer fcntl argument.
        if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return Err(last_error());
        }
        let ev = event(0, libc::EVFILT_USER, libc::EV_ADD | libc::EV_CLEAR, 0, 0);
        // SAFETY: valid change record; no event output and therefore no wait.
        if unsafe {
            libc::kevent(
                fd.as_raw_fd(),
                &ev,
                1,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
            )
        } < 0
        {
            return Err(last_error());
        }
        Ok(Self {
            wake: Arc::new(KqueueWake {
                fd,
                calls: AtomicU64::new(0),
            }),
            events: vec![event(0, 0, 0, 0, 0); capacity],
        })
    }
    fn waker(&self) -> Arc<Self::W> {
        self.wake.clone()
    }
    fn register(&mut self, fd: RawFd, key: u64) -> Result<()> {
        let changes = [
            event(
                fd as usize,
                libc::EVFILT_READ,
                libc::EV_ADD | libc::EV_CLEAR,
                0,
                key,
            ),
            event(
                fd as usize,
                libc::EVFILT_WRITE,
                libc::EV_ADD | libc::EV_CLEAR,
                0,
                key,
            ),
        ];
        // SAFETY: live descriptor, integer-only query of its access mode.
        let mode = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if mode < 0 {
            return Err(last_error());
        }
        let (offset, count) = match mode & libc::O_ACCMODE {
            libc::O_RDONLY => (0, 1),
            libc::O_WRONLY => (1, 1),
            _ => (0, 2),
        };
        // SAFETY: descriptors and selected initialized input records are valid.
        if unsafe {
            libc::kevent(
                self.fd(),
                changes[offset..].as_ptr(),
                count,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
            )
        } < 0
        {
            return Err(last_error());
        }
        Ok(())
    }
    fn deregister(&mut self, fd: RawFd) -> Result<()> {
        // SAFETY: live descriptor, integer-only access-mode query.
        let mode = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if mode < 0 {
            return Err(last_error());
        }
        for filter in [libc::EVFILT_READ, libc::EVFILT_WRITE] {
            if (filter == libc::EVFILT_READ && mode & libc::O_ACCMODE == libc::O_WRONLY)
                || (filter == libc::EVFILT_WRITE && mode & libc::O_ACCMODE == libc::O_RDONLY)
            {
                continue;
            }
            let ev = event(fd as usize, filter, libc::EV_DELETE, 0, 0);
            // SAFETY: live kqueue and initialized deletion, with no wait/output.
            if unsafe { libc::kevent(self.fd(), &ev, 1, std::ptr::null_mut(), 0, std::ptr::null()) }
                < 0
            {
                let e = last_error();
                if e.os != Some(libc::ENOENT) {
                    return Err(e);
                }
            }
        }
        Ok(())
    }
    fn process(&mut self, pid: u32, key: u64) -> Result<Option<OwnedFd>> {
        let ev = event(
            pid as usize,
            libc::EVFILT_PROC,
            libc::EV_ADD | libc::EV_ONESHOT,
            libc::NOTE_EXIT,
            key,
        );
        // SAFETY: live kqueue, initialized process filter, no output wait.
        if unsafe { libc::kevent(self.fd(), &ev, 1, std::ptr::null_mut(), 0, std::ptr::null()) } < 0
        {
            return Err(last_error());
        }
        Ok(None)
    }
    fn remove_process(&mut self, pid: u32, _fd: Option<RawFd>) {
        let ev = event(pid as usize, libc::EVFILT_PROC, libc::EV_DELETE, 0, 0);
        // SAFETY: live kqueue; deleting an expired one-shot may return ENOENT.
        unsafe {
            libc::kevent(self.fd(), &ev, 1, std::ptr::null_mut(), 0, std::ptr::null());
        }
    }
    fn wait(&mut self, timeout: Option<Duration>, out: &mut Vec<Ready>) -> Result<PollInfo> {
        let ts = timeout.map(timespec);
        // SAFETY: output storage holds events.len() initialized kevents. Timeout
        // pointer is either null or points to a live timespec for this call.
        let n = unsafe {
            libc::kevent(
                self.fd(),
                std::ptr::null(),
                0,
                self.events.as_mut_ptr(),
                self.events.len() as i32,
                ts.as_ref().map_or(std::ptr::null(), |v| v),
            )
        };
        if n < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::Interrupted {
                return Ok(PollInfo::native(timeout, true));
            }
            return Err(e.into());
        }
        for e in &self.events[..n as usize] {
            if e.filter == libc::EVFILT_USER {
                continue;
            }
            out.push(Ready {
                key: e.udata as usize as u64,
                read: e.filter == libc::EVFILT_READ,
                write: e.filter == libc::EVFILT_WRITE,
                vnode: if e.filter == libc::EVFILT_VNODE {
                    e.fflags
                } else {
                    0
                },
            });
        }
        Ok(PollInfo::native(timeout, n == 0))
    }
    fn fd(&self) -> RawFd {
        self.wake.fd.as_raw_fd()
    }
}

#[cfg(all(test, not(loom)))]
mod tests {
    use super::*;

    #[test]
    fn blocking_and_discovery_calls_keep_raw_empty_accounting() {
        let mut poller = Kqueue::new(4).expect("kqueue");
        let mut ready = Vec::with_capacity(4);
        let mut calls = 0;
        for timeout in [Some(Duration::ZERO), Some(Duration::from_millis(2)), None] {
            // Infinite calls must be woken; finite/zero calls also exercise empty results.
            for wake in [false, true] {
                if timeout.is_none() && !wake {
                    continue;
                }
                if wake {
                    poller.waker().wake().expect("wake");
                }
                let info = poller.wait(timeout, &mut ready).expect("native call");
                let discovery = timeout == Some(Duration::ZERO);
                assert_eq!(
                    (info.waits, info.discovery_polls),
                    (u32::from(!discovery), u32::from(discovery))
                );
                assert_eq!(info.zero_event_waits, u32::from(!wake));
                assert!(
                    ready.is_empty(),
                    "notifier is native work without user readiness"
                );
                calls += 1;
            }
        }
        assert_eq!(calls, 5);
    }
}
