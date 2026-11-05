use crate::{
    Error, ErrorKind, Payload, Result, Token,
    backend::Wake,
    queue::Queue,
    sync::{Arc, AtomicBool, AtomicUsize, Ordering},
};
const RUNNING: usize = 0;
const PARKED: usize = 1;
const NOTIFIED: usize = 2;
const CLOSED: usize = 4;
struct State {
    bits: AtomicUsize,
    wake: std::sync::Arc<dyn Wake>,
}
#[derive(Clone)]
/// Thread-safe, cloneable wake endpoint using the loop parking handshake.
pub struct Notifier {
    state: Arc<State>,
}
impl Notifier {
    pub(crate) fn new(wake: std::sync::Arc<dyn Wake>) -> Self {
        Self {
            state: Arc::new(State {
                bits: AtomicUsize::new(RUNNING),
                wake,
            }),
        }
    }
    /// Coalesced notification. The only syscall path is the transition from
    /// PARKED to PARKED|NOTIFIED. A failed wake is returned to the producer.
    pub fn notify(&self) -> Result<()> {
        let old = self.state.bits.fetch_or(NOTIFIED, Ordering::AcqRel);
        if old & CLOSED != 0 {
            return Err(Error::new(ErrorKind::NotFound));
        }
        if old & PARKED != 0
            && old & NOTIFIED == 0
            && let Err(e) = self.state.wake.wake()
        {
            // Permit another producer to retry the failed wake.
            self.state.bits.fetch_and(!NOTIFIED, Ordering::AcqRel);
            return Err(e);
        }
        Ok(())
    }
    /// Return native wake syscall attempts for instrumentation.
    pub fn wake_syscalls(&self) -> u64 {
        self.state.wake.syscall_count()
    }
    /// Observe whether this loop is currently prepared to receive a native wake.
    pub fn is_parked(&self) -> bool {
        self.state.bits.load(Ordering::Acquire) & PARKED != 0
    }
    /// Consume an earlier notification; it forces this turn to be nonblocking.
    pub(crate) fn begin(&self) -> bool {
        self.state.bits.swap(RUNNING, Ordering::AcqRel) & NOTIFIED != 0
    }
    pub(crate) fn park(&self) -> bool {
        self.state
            .bits
            .compare_exchange(RUNNING, PARKED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
    pub(crate) fn running(&self) {
        self.state.bits.fetch_and(!PARKED, Ordering::AcqRel);
    }
    pub(crate) fn external_park(&self, work: bool) -> Result<()> {
        let old = self.state.bits.swap(PARKED, Ordering::AcqRel);
        if work || old & NOTIFIED != 0 {
            self.notify()?;
        }
        Ok(())
    }
    pub(crate) fn close(&self) {
        self.state.bits.store(CLOSED, Ordering::Release);
    }
}
struct Postbox {
    queue: Queue<Post>,
    notifier: Notifier,
    closed: AtomicBool,
}
pub(crate) struct Post {
    pub token: Token,
    pub payload: Payload,
}
#[derive(Clone)]
/// Bounded thread-safe payload queue routed to exactly one owning loop.
pub struct Poster {
    inner: Arc<Postbox>,
}
#[derive(Debug)]
/// Posting failure with ownership returned only when the payload was not accepted.
pub struct PostError {
    /// The queue or wake failure.
    pub error: Error,
    /// Opaque host routing token preserved from submission.
    pub token: Token,
    /// Returned payload if rejected; None means accepted despite a wake error, so do not retry.
    pub payload: Option<Payload>,
}
impl Poster {
    pub(crate) fn new(capacity: usize, notifier: Notifier) -> Self {
        Self {
            inner: Arc::new(Postbox {
                queue: Queue::new(capacity.max(2).next_power_of_two()),
                notifier,
                closed: AtomicBool::new(false),
            }),
        }
    }
    /// Returns ownership on full/closed. On Ok the payload is owned by this loop.
    /// A wake error after enqueue returns `payload: None`: the post was accepted
    /// and must not be retried. It stays queued for the next host turn.
    pub fn post(&self, token: Token, payload: Payload) -> std::result::Result<(), PostError> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(PostError {
                error: Error::new(ErrorKind::NotFound),
                token,
                payload: Some(payload),
            });
        }
        self.inner
            .queue
            .push(Post { token, payload })
            .map_err(|p| PostError {
                error: Error::new(ErrorKind::WouldBlock),
                token: p.token,
                payload: Some(p.payload),
            })?;
        self.inner.notifier.notify().map_err(|error| PostError {
            error,
            token,
            payload: None,
        })
    }
    pub(crate) fn pop(&self) -> Option<Post> {
        self.inner.queue.pop()
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.inner.queue.is_empty()
    }
    pub(crate) fn close(&self) {
        self.inner.closed.store(true, Ordering::Release);
    }
}

#[cfg(all(test, loom))]
mod models {
    use super::*;
    struct W(AtomicUsize);
    impl Wake for W {
        fn wake(&self) -> Result<()> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        fn syscall_count(&self) -> u64 {
            self.0.load(Ordering::Relaxed) as u64
        }
    }
    #[test]
    fn notify_racing_with_park_is_not_lost() {
        loom::model(|| {
            let wake = std::sync::Arc::new(W(AtomicUsize::new(0)));
            let n = Notifier::new(wake.clone());
            let other = n.clone();
            let t = loom::thread::spawn(move || other.notify().expect("wake"));
            let parked = n.park();
            t.join().expect("producer");
            if parked {
                assert_eq!(wake.syscall_count(), 1);
            } else {
                assert!(n.begin());
                assert_eq!(wake.syscall_count(), 0);
            }
        });
    }
    #[test]
    fn running_notifications_make_no_syscalls() {
        loom::model(|| {
            let wake = std::sync::Arc::new(W(AtomicUsize::new(0)));
            let n = Notifier::new(wake.clone());
            let other = n.clone();
            let t = loom::thread::spawn(move || {
                other.notify().expect("notify");
                other.notify().expect("notify");
            });
            n.notify().expect("notify");
            t.join().expect("producer");
            assert!(n.begin());
            assert_eq!(wake.syscall_count(), 0);
        });
    }
}
