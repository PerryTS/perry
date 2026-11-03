//! One lazy process-wide signal dispatcher, with weak per-loop subscriptions.
use super::poller::last_error;
use super::services::ReadyQueue;
use crate::{Error, ErrorKind, Handle, Notifier, Result, Signal};
use std::{
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

pub(super) fn number(signal: Signal) -> Result<i32> {
    Ok(match signal {
        Signal::Int => libc::SIGINT,
        Signal::Term => libc::SIGTERM,
        Signal::Kill => libc::SIGKILL,
        Signal::Hup => libc::SIGHUP,
        Signal::Chld => libc::SIGCHLD,
        Signal::WinCh => libc::SIGWINCH,
        Signal::Usr1 => libc::SIGUSR1,
        Signal::Usr2 => libc::SIGUSR2,
        Signal::Break => return Err(Error::new(ErrorKind::Unsupported)),
    })
}
pub(super) struct Ticket {
    signal: Signal,
    pending: AtomicBool,
    notifier: Notifier,
    handle: Handle,
    ready: Arc<ReadyQueue>,
}
impl Ticket {
    pub fn take(&self) -> bool {
        self.pending.swap(false, Ordering::AcqRel)
    }
    pub fn ready(&self) -> bool {
        self.pending.load(Ordering::Acquire)
    }
}
struct Record {
    number: i32,
    original: libc::sigaction,
    subscribers: Vec<Weak<Ticket>>,
}
struct Dispatcher {
    fd: OwnedFd,
    records: Mutex<Vec<Record>>,
    #[cfg(turnloop_backend = "epoll")]
    _writer: OwnedFd,
}
static DISPATCHER: OnceLock<Result<Arc<Dispatcher>>> = OnceLock::new();
#[cfg(turnloop_backend = "epoll")]
static WRITE_FD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);
#[cfg(turnloop_backend = "epoll")]
static PENDING: [AtomicBool; 128] = [const { AtomicBool::new(false) }; 128];
extern "C" fn handler(_signal: i32) {
    #[cfg(turnloop_backend = "epoll")]
    {
        if let Some(pending) = PENDING.get(_signal as usize) {
            pending.store(true, Ordering::Release);
            let fd = WRITE_FD.load(Ordering::Acquire);
            let byte = 1u8;
            // SAFETY: process-lifetime nonblocking pipe, stack byte and thread-local
            // errno. write is async-signal-safe; pipe saturation already wakes dispatch.
            unsafe {
                let saved = *super::ipc::errno();
                libc::write(fd, (&byte as *const u8).cast(), 1);
                *super::ipc::errno() = saved;
            }
        }
    }
}
#[cfg(turnloop_backend = "kqueue")]
fn change(fd: i32, signal: i32, flags: u16) -> Result<()> {
    let event = libc::kevent {
        ident: signal as usize,
        filter: libc::EVFILT_SIGNAL,
        flags,
        fflags: 0,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    // SAFETY: valid kqueue and initialized signal change, no event output or wait.
    if unsafe { libc::kevent(fd, &event, 1, std::ptr::null_mut(), 0, std::ptr::null()) } < 0 {
        return Err(last_error());
    }
    Ok(())
}
fn start() -> Result<Arc<Dispatcher>> {
    #[cfg(turnloop_backend = "kqueue")]
    let fd = {
        // SAFETY: kqueue returns a fresh owned descriptor without pointer arguments.
        let raw = unsafe { libc::kqueue() };
        if raw < 0 {
            return Err(last_error());
        }
        // SAFETY: successful kqueue transferred exclusive ownership.
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        // SAFETY: live fd and valid integer fcntl flags.
        if unsafe { libc::fcntl(raw, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return Err(last_error());
        }
        fd
    };
    #[cfg(turnloop_backend = "epoll")]
    let (fd, writer) = {
        let mut pair = [-1; 2];
        // SAFETY: two writable fd slots and valid flags.
        if unsafe { libc::pipe2(pair.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) } < 0 {
            return Err(last_error());
        }
        // SAFETY: successful pipe2 returned two exclusively owned descriptors.
        let (reader, writer) =
            unsafe { (OwnedFd::from_raw_fd(pair[0]), OwnedFd::from_raw_fd(pair[1])) };
        WRITE_FD.store(writer.as_raw_fd(), Ordering::Release);
        (reader, writer)
    };
    let dispatcher = Arc::new(Dispatcher {
        fd,
        records: Mutex::new(Vec::new()),
        #[cfg(turnloop_backend = "epoll")]
        _writer: writer,
    });
    let d = dispatcher.clone();
    std::thread::Builder::new()
        .name("turnloop-signals".into())
        .spawn(move || d.run())
        .map_err(Error::from)?;
    Ok(dispatcher)
}
impl Dispatcher {
    fn dispatch(&self, number: i32) {
        let records = self
            .records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(record) = records.iter().find(|r| r.number == number) {
            for ticket in &record.subscribers {
                if let Some(t) = ticket.upgrade() {
                    t.pending.store(true, Ordering::Release);
                    t.ready.push(t.handle);
                    let _ = t.notifier.notify();
                }
            }
        }
    }
    fn run(&self) {
        loop {
            #[cfg(turnloop_backend = "kqueue")]
            {
                // SAFETY: kevent is plain C output storage.
                let mut event: libc::kevent = unsafe { std::mem::zeroed() };
                // SAFETY: live kqueue and one writable output event; unbounded helper wait.
                let n = unsafe {
                    libc::kevent(
                        self.fd.as_raw_fd(),
                        std::ptr::null(),
                        0,
                        &mut event,
                        1,
                        std::ptr::null(),
                    )
                };
                if n == 1 {
                    self.dispatch(event.ident as i32);
                } else if n < 0 && last_error().os != Some(libc::EINTR) {
                    return;
                }
            }
            #[cfg(turnloop_backend = "epoll")]
            {
                let mut fd = libc::pollfd {
                    fd: self.fd.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                // SAFETY: helper-owned pollfd, one record, unbounded wait.
                let n = unsafe { libc::poll(&mut fd, 1, -1) };
                if n < 0 {
                    if last_error().os == Some(libc::EINTR) {
                        continue;
                    }
                    return;
                }
                let mut bytes = [0u8; 256];
                // SAFETY: writable stack buffer and nonblocking pipe.
                while unsafe { libc::read(fd.fd, bytes.as_mut_ptr().cast(), bytes.len()) } > 0 {}
                for (sig, pending) in PENDING.iter().enumerate() {
                    if pending.swap(false, Ordering::AcqRel) {
                        self.dispatch(sig as i32);
                    }
                }
            }
        }
    }
}
pub(super) fn subscribe(
    signal: Signal,
    notifier: Notifier,
    handle: Handle,
    ready: Arc<ReadyQueue>,
) -> Result<Subscription> {
    let number = number(signal)?;
    if signal == Signal::Kill {
        return Err(Error::new(ErrorKind::InvalidInput));
    }
    let dispatcher = DISPATCHER.get_or_init(start).as_ref().map_err(|&e| e)?;
    let mut records = dispatcher
        .records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let ticket = Arc::new(Ticket {
        signal,
        pending: AtomicBool::new(false),
        notifier,
        handle,
        ready,
    });
    if let Some(record) = records.iter_mut().find(|r| r.number == number) {
        record.subscribers.push(Arc::downgrade(&ticket));
    } else {
        // SAFETY: sigaction is plain C storage initialized before installation.
        let (mut action, mut original): (libc::sigaction, libc::sigaction) =
            unsafe { std::mem::zeroed() };
        action.sa_sigaction = handler as *const () as usize;
        #[cfg(turnloop_backend = "kqueue")]
        {
            // EVFILT_SIGNAL observes generation even for SIG_IGN. A no-op
            // handler instead leaves ordinary delivery pending on another
            // thread: all loops can consume the kevent and restore SIG_DFL
            // before that thread runs, killing the process on the old signal.
            // SIG_IGN discards that pending delivery. For SIGCHLD, SIG_DFL also
            // discards ordinary delivery but preserves wait status; SIG_IGN
            // would auto-reap children. Kqueue observes generation in both cases.
            action.sa_sigaction = if signal == Signal::Chld {
                libc::SIG_DFL
            } else {
                libc::SIG_IGN
            };
        }
        action.sa_flags = libc::SA_RESTART
            | if signal == Signal::Chld {
                libc::SA_NOCLDSTOP
            } else {
                0
            };
        // SAFETY: valid action pointers and initialized signal set.
        unsafe {
            libc::sigemptyset(&mut action.sa_mask);
        }
        // SAFETY: supported catchable signal and valid action storage.
        if unsafe { libc::sigaction(number, &action, &mut original) } < 0 {
            return Err(last_error());
        }
        #[cfg(turnloop_backend = "kqueue")]
        if let Err(e) = change(
            dispatcher.fd.as_raw_fd(),
            number,
            libc::EV_ADD | libc::EV_CLEAR,
        ) {
            // SAFETY: restore precisely the action captured above on registration failure.
            unsafe {
                libc::sigaction(number, &original, std::ptr::null_mut());
            }
            return Err(e);
        }
        records.push(Record {
            number,
            original,
            subscribers: vec![Arc::downgrade(&ticket)],
        });
    }
    Ok(Subscription(ticket))
}
pub(super) struct Subscription(Arc<Ticket>);
impl std::ops::Deref for Subscription {
    type Target = Ticket;
    fn deref(&self) -> &Ticket {
        &self.0
    }
}
impl Drop for Subscription {
    fn drop(&mut self) {
        let Some(Ok(dispatcher)) = DISPATCHER.get() else {
            return;
        };
        let Ok(number) = number(self.signal) else {
            return;
        };
        let mut records = dispatcher
            .records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(i) = records.iter().position(|r| r.number == number) {
            records[i]
                .subscribers
                .retain(|w| w.as_ptr() != Arc::as_ptr(&self.0));
            if records[i].subscribers.is_empty() {
                let record = records.swap_remove(i);
                #[cfg(turnloop_backend = "kqueue")]
                let _ = change(dispatcher.fd.as_raw_fd(), number, libc::EV_DELETE);
                // SAFETY: restore the exact disposition saved when first subscribed.
                unsafe {
                    libc::sigaction(number, &record.original, std::ptr::null_mut());
                }
            }
        }
    }
}
