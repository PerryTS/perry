//! The crate's async runtime: a turnloop-backed `block_on` plus the few leaf
//! primitives the compose engine awaits — child processes, timers, a shutdown
//! signal and an async mutex.
//!
//! Everything above this module (`ContainerBackend`, the compose engine, the
//! workload graph, the CLI) is plain executor-agnostic `async` Rust. Only the
//! leaves need an event source, and they get it from the
//! [`turnloop::Loop`] that the innermost enclosing [`block_on`] owns on this
//! thread:
//!
//! * [`Command`] spawns the container CLI (`docker`, `podman`, `container`,
//!   …) through turnloop's native child-process support, reads its stdout and
//!   stderr pipes to EOF with multishot reads, and completes on the child's
//!   reaped exit status. Dropping an unfinished `output()` / `status()` future
//!   closes the process handle, which terminates the child — so a timed-out
//!   CLI invocation is killed rather than left running.
//! * [`sleep`] / [`timeout`] are one-shot turnloop timers.
//! * [`shutdown_signal`] subscribes the loop to SIGINT / SIGTERM (console
//!   Ctrl-C on Windows).
//! * [`Mutex`] is `async-lock`'s executor-agnostic mutex; its wakers may fire
//!   from any thread, which [`block_on`] turns into a turnloop notification.
//!
//! # Driving it
//!
//! [`block_on`] creates one loop per call, polls the future, and turns the
//! loop whenever the future is pending — waking on a completion one of its
//! leaves submitted, on a timer, or on a cross-thread [`std::task::Waker`].
//! It is the executor for the standalone `perry-compose` binary, for this
//! crate's tests, and — one call per operation, on a turnloop pool worker —
//! for perry-stdlib's `perry/container`, `perry/compose` and
//! `perry/workloads` bindings. Nested calls are allowed: the inner call gets
//! its own loop and the outer one resumes when it returns.
//!
//! The leaf futures hold no loop reference, only plain ids, so they are
//! `Send` and fit `#[async_trait]`'s boxed `Send` futures. The price is the
//! same rule tokio has: a leaf must be polled inside the `block_on` that first
//! polled it, and polling one outside any `block_on` panics.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::io;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use turnloop::{Completions, Config, Loop, Notifier, OpResult, Timeout, Token};

mod process;
mod signal;
mod time;

#[cfg(test)]
mod tests;

pub use async_lock::{Mutex, MutexGuard};
pub use process::{Command, ExitStatus, Output, OutputFuture, StatusFuture};
pub use signal::{shutdown_signal, ShutdownSignal, ShutdownSignalFuture};
pub use time::{sleep, timeout, Elapsed, Sleep, Timeout as TimeoutFuture};

/// Close token for handles whose `Closed` completion nobody waits for. Slot
/// tokens start at 1, so completions carrying it are dropped by dispatch.
const UNROUTED: Token = Token(0);

static NEXT_REACTOR_ID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    /// The reactor of the innermost `block_on` running on this thread.
    static CURRENT: RefCell<Option<Rc<RefCell<Reactor>>>> = const { RefCell::new(None) };
}

/// One routed completion, copied out of turnloop's output buffer.
#[derive(Debug)]
pub(crate) enum Event {
    Timer,
    Read(Vec<u8>),
    Eof,
    Exited(turnloop::ExitStatus),
    Signal,
    Failed(turnloop::Error),
}

#[derive(Default)]
struct Slot {
    waker: Option<Waker>,
    events: VecDeque<Event>,
}

/// A `block_on` call's loop plus the per-token event queues its leaves poll.
pub(crate) struct Reactor {
    id: u64,
    pub(crate) driver: Loop,
    next_token: u64,
    slots: HashMap<u64, Slot>,
}

impl Reactor {
    fn new() -> io::Result<Self> {
        // Sized for a handful of concurrent CLI invocations, not a server:
        // each child costs at most three handles (process, stdout, stderr).
        // `blocking_pool` keeps its default because it is process-wide and
        // must match every other loop's; nothing here submits to it.
        let config = Config {
            max_handles: 256,
            max_operations: 1024,
            events_per_turn: 64,
            pooled_buffers: 32,
            ..Config::default()
        };
        let driver = Loop::new(config).map_err(io_error)?;
        Ok(Self {
            id: NEXT_REACTOR_ID.fetch_add(1, Ordering::Relaxed),
            driver,
            next_token: 1,
            slots: HashMap::new(),
        })
    }

    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    /// Mint a routing token and the event queue its completions land in.
    pub(crate) fn register(&mut self) -> u64 {
        let token = self.next_token;
        self.next_token += 1;
        self.slots.insert(token, Slot::default());
        token
    }

    /// Stop routing `token`; later completions for it are dropped.
    pub(crate) fn forget(&mut self, token: u64) {
        self.slots.remove(&token);
    }

    /// Pop the next event for `token`, or remember `cx`'s waker for it.
    pub(crate) fn take_event(&mut self, token: u64, cx: &Context<'_>) -> Option<Event> {
        let slot = self.slots.get_mut(&token)?;
        let event = slot.events.pop_front();
        if event.is_none() {
            match &slot.waker {
                Some(w) if w.will_wake(cx.waker()) => {}
                _ => slot.waker = Some(cx.waker().clone()),
            }
        }
        event
    }

    /// Close `handle`, ignoring a handle that is already closing or gone.
    pub(crate) fn close(&mut self, handle: turnloop::Handle) {
        let _ = self.driver.close(handle, UNROUTED);
    }

    fn turn(this: &RefCell<Self>, timeout: Timeout, completions: &mut Completions) {
        let mut wake = Vec::new();
        {
            let mut reactor = this.borrow_mut();
            if let Err(e) = reactor.driver.turn(timeout, completions) {
                panic!("perry_container_compose::rt: turnloop turn failed: {e}");
            }
            for completion in completions.drain() {
                let event = match completion.result {
                    OpResult::Timer => Event::Timer,
                    OpResult::Read { n, lease } => {
                        let bytes = lease
                            .as_ref()
                            .map(|l| {
                                let s = l.as_slice();
                                s[..n.min(s.len())].to_vec()
                            })
                            .unwrap_or_default();
                        Event::Read(bytes)
                    }
                    OpResult::Eof => Event::Eof,
                    OpResult::Exited(status) => Event::Exited(status),
                    OpResult::Signal(_) => Event::Signal,
                    OpResult::Err(e) => Event::Failed(e),
                    // Cancelled / Closed / Stopped acknowledge teardown the
                    // leaf already did; nothing waits for them.
                    _ => continue,
                };
                if let Some(slot) = reactor.slots.get_mut(&completion.token.0) {
                    slot.events.push_back(event);
                    if let Some(w) = slot.waker.take() {
                        wake.push(w);
                    }
                }
            }
        }
        // Wake outside the borrow: a waker is foreign code.
        for w in wake {
            w.wake();
        }
    }
}

/// Run `f` against the current thread's innermost reactor.
///
/// # Panics
/// Outside [`block_on`].
pub(crate) fn with_current<R>(f: impl FnOnce(&mut Reactor) -> R) -> R {
    CURRENT.with(|current| {
        let reactor = current.borrow().clone().expect(
            "perry_container_compose::rt primitive polled outside rt::block_on \
             (it needs the turnloop loop that block_on owns)",
        );
        let mut reactor = reactor.borrow_mut();
        f(&mut reactor)
    })
}

/// Run `f` against the reactor `id`, which must be the current one.
///
/// # Panics
/// When the current reactor is a different one: the leaf was first polled by
/// another `block_on`, whose loop owns its handles.
pub(crate) fn with_reactor<R>(id: u64, f: impl FnOnce(&mut Reactor) -> R) -> R {
    with_current(|reactor| {
        assert_eq!(
            reactor.id, id,
            "perry_container_compose::rt primitive polled by a different rt::block_on than the \
             one that started it"
        );
        f(reactor)
    })
}

/// [`with_reactor`] for `Drop`: does nothing when `id` is not the current
/// reactor (its loop is gone, or dropping it will release the handles) or
/// the thread-local is already torn down.
pub(crate) fn try_with_reactor(id: u64, f: impl FnOnce(&mut Reactor)) {
    let _ = CURRENT.try_with(|current| {
        let Ok(current) = current.try_borrow() else {
            return;
        };
        if let Some(reactor) = current.as_ref() {
            if let Ok(mut reactor) = reactor.try_borrow_mut() {
                if reactor.id == id {
                    f(&mut reactor);
                }
            }
        }
    });
}

/// Whether this thread is inside a [`block_on`] — the analogue of
/// `tokio::runtime::Handle::try_current().is_ok()`.
pub fn in_context() -> bool {
    CURRENT
        .try_with(|current| current.borrow().is_some())
        .unwrap_or(false)
}

/// The `block_on` waker: records the wake and nudges the loop, which only
/// costs a syscall when the loop is actually parked (a wake from another
/// thread).
struct WakeFlag {
    woken: AtomicBool,
    notifier: Notifier,
}

impl Wake for WakeFlag {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.woken.store(true, Ordering::Release);
        // `Err` means the loop is gone, i.e. the block_on already returned.
        let _ = self.notifier.notify();
    }
}

/// Restores the enclosing `block_on`'s reactor (if any) on exit, including
/// on unwind.
struct RestoreCurrent(Option<Rc<RefCell<Reactor>>>);

impl Drop for RestoreCurrent {
    fn drop(&mut self) {
        let previous = self.0.take();
        let _ = CURRENT.try_with(|current| *current.borrow_mut() = previous);
    }
}

/// Drive `future` to completion on this thread, on a turnloop loop created
/// for the call. Returns an error only when the loop cannot be created.
pub fn try_block_on<F: Future>(future: F) -> io::Result<F::Output> {
    let reactor = Rc::new(RefCell::new(Reactor::new()?));
    let notifier = reactor.borrow().driver.notifier();
    let previous = CURRENT.with(|current| current.replace(Some(Rc::clone(&reactor))));
    // Declared after `reactor` and before `future`: the future — and every
    // leaf in it — drops first, while this reactor is still current, so leaf
    // destructors can close their handles; then the enclosing reactor comes
    // back; then this loop drops, terminating anything still alive in it.
    let _restore = RestoreCurrent(previous);
    let flag = Arc::new(WakeFlag {
        woken: AtomicBool::new(true),
        notifier,
    });
    let waker = Waker::from(Arc::clone(&flag));
    let mut cx = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    let mut completions = Completions::with_capacity(64);
    loop {
        if flag.woken.swap(false, Ordering::AcqRel) {
            if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
                return Ok(output);
            }
        }
        let timeout = if flag.woken.load(Ordering::Acquire) {
            Timeout::Now
        } else {
            Timeout::Forever
        };
        Reactor::turn(&reactor, timeout, &mut completions);
    }
}

/// Drive `future` to completion on this thread — see [`try_block_on`].
///
/// # Panics
/// When the turnloop loop cannot be created (descriptor exhaustion).
pub fn block_on<F: Future>(future: F) -> F::Output {
    try_block_on(future).unwrap_or_else(|e| {
        panic!("perry_container_compose::rt::block_on: cannot create a turnloop loop: {e}")
    })
}

/// Convert a turnloop error to `std::io::Error`, keeping the OS code.
pub(crate) fn io_error(e: turnloop::Error) -> io::Error {
    use turnloop::ErrorKind as K;
    if let Some(code) = e.os {
        return io::Error::from_raw_os_error(code);
    }
    let kind = match e.kind {
        K::NotFound => io::ErrorKind::NotFound,
        K::PermissionDenied => io::ErrorKind::PermissionDenied,
        K::InvalidInput => io::ErrorKind::InvalidInput,
        K::Unsupported => io::ErrorKind::Unsupported,
        K::TimedOut => io::ErrorKind::TimedOut,
        K::WouldBlock => io::ErrorKind::WouldBlock,
        K::BrokenPipe => io::ErrorKind::BrokenPipe,
        K::ConnectionRefused => io::ErrorKind::ConnectionRefused,
        K::ConnectionReset => io::ErrorKind::ConnectionReset,
        K::AlreadyExists => io::ErrorKind::AlreadyExists,
        K::Cancelled => io::ErrorKind::Interrupted,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, e)
}
