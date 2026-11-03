//! Native helper or single-agent service for host conditions and exact deadlines.
use crate::{Error, ErrorKind, Instant, Result};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// Shared host wait condition. Hosts may provide their own atomic storage and call
/// `notify` after changing it. No predicate callback runs inside a loop turn.
#[derive(Clone)]
pub struct WaitCondition {
    value: Arc<AtomicU64>,
    generation: Arc<AtomicU64>,
}
impl WaitCondition {
    /// Create shared storage and initialize the platform wait service if needed.
    pub fn new(value: u64) -> Result<Self> {
        Self::from_atomic(Arc::new(AtomicU64::new(value)))
    }
    /// Register host-owned atomic storage. Call `notify` after external mutations.
    pub fn from_atomic(value: Arc<AtomicU64>) -> Result<Self> {
        service::initialize()?;
        Ok(Self {
            value,
            generation: Arc::new(AtomicU64::new(0)),
        })
    }
    /// Read the current value with acquire ordering.
    pub fn load(&self) -> u64 {
        self.value.load(Ordering::Acquire)
    }
    /// Store a new value and wake registered waiters for this condition.
    pub fn store(&self, value: u64) {
        self.value.store(value, Ordering::Release);
        self.notify();
    }
    /// Wake current registrations, including when the value remains unchanged.
    pub fn notify(&self) {
        service::notify(self);
    }
}
/// Why an external wait completed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaitResult {
    /// The value already differed from the expected value at registration.
    NotEqual,
    /// The host notified this condition after registration.
    Notified,
    /// The exact monotonic deadline expired.
    TimedOut,
}

#[cfg(not(target_arch = "wasm32"))]
mod service {
    use super::*;
    use crate::{
        OpId,
        blocking::{WorkOutput, WorkPort, WorkResult},
    };
    use std::sync::{Condvar, Mutex, OnceLock};
    const CAPACITY: usize = 16_384;
    struct Wait {
        op: OpId,
        port: Arc<WorkPort>,
        condition: WaitCondition,
        generation: u64,
        expected: u64,
        deadline: Option<Instant>,
    }
    struct Service {
        waits: Mutex<Vec<Option<Wait>>>,
        changed: Condvar,
    }
    static SERVICE: OnceLock<Result<Arc<Service>>> = OnceLock::new();
    fn get() -> Result<&'static Arc<Service>> {
        SERVICE
            .get_or_init(|| {
                let service = Arc::new(Service {
                    waits: Mutex::new((0..CAPACITY).map(|_| None).collect()),
                    changed: Condvar::new(),
                });
                let s = service.clone();
                std::thread::Builder::new()
                    .name("turnloop-external-wait".into())
                    .spawn(move || s.run())
                    .map_err(Error::from)?;
                Ok(service)
            })
            .as_ref()
            .map_err(|&e| e)
    }
    pub(super) fn initialize() -> Result<()> {
        get().map(|_| ())
    }
    pub(super) fn notify(condition: &WaitCondition) {
        if let Ok(s) = get() {
            // Holding the same mutex as registration and parking closes lost-wake
            // windows even when the host changed its atomic outside this service.
            let _waits = s
                .waits
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            condition.generation.fetch_add(1, Ordering::Release);
            s.changed.notify_one();
        }
    }
    impl Service {
        fn run(&self) {
            let mut waits = self
                .waits
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            loop {
                let now = Instant::now();
                let mut earliest: Option<Instant> = None;
                for slot in waits.iter_mut() {
                    let Some(w) = slot else {
                        continue;
                    };
                    let result = if w.condition.load() != w.expected
                        || w.condition.generation.load(Ordering::Acquire) != w.generation
                    {
                        Some(WaitResult::Notified)
                    } else if w.deadline.is_some_and(|d| d <= now) {
                        Some(WaitResult::TimedOut)
                    } else {
                        None
                    };
                    if let Some(result) = result {
                        let w = slot.take().expect("ready wait");
                        w.port.complete(WorkResult {
                            op: w.op,
                            result: Ok(WorkOutput::ExternalWait(result)),
                        });
                    } else if let Some(at) = w.deadline {
                        earliest = Some(earliest.map_or(at, |old| old.min(at)));
                    }
                }
                waits = if let Some(at) = earliest {
                    self.changed
                        .wait_timeout(waits, at.saturating_duration_since(Instant::now()))
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .0
                } else {
                    self.changed
                        .wait(waits)
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                };
            }
        }
    }
    pub(crate) fn submit(
        op: OpId,
        port: Arc<WorkPort>,
        condition: &WaitCondition,
        expected: u64,
        deadline: Option<Instant>,
    ) -> Result<()> {
        let s = get()?;
        let mut waits = s
            .waits
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if condition.load() != expected {
            port.complete(WorkResult {
                op,
                result: Ok(WorkOutput::ExternalWait(WaitResult::NotEqual)),
            });
            return Ok(());
        }
        let slot = waits
            .iter_mut()
            .find(|w| w.is_none())
            .ok_or(Error::new(ErrorKind::ResourceLimit))?;
        *slot = Some(Wait {
            op,
            port,
            condition: condition.clone(),
            generation: condition.generation.load(Ordering::Acquire),
            expected,
            deadline,
        });
        s.changed.notify_one();
        Ok(())
    }
    pub(crate) fn cancel(op: OpId) {
        if let Ok(s) = get() {
            let mut waits = s
                .waits
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(slot) = waits
                .iter_mut()
                .find(|slot| slot.as_ref().is_some_and(|w| w.op == op))
            {
                let w = slot.take().expect("cancelled wait");
                w.port.complete(WorkResult {
                    op,
                    result: Err(Error::new(ErrorKind::Cancelled)),
                });
                s.changed.notify_one();
            }
        }
    }
    pub(crate) fn close(owner: u64) {
        if let Some(Ok(s)) = SERVICE.get() {
            let mut waits = s
                .waits
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for slot in waits.iter_mut() {
                if slot.as_ref().is_some_and(|w| w.op.owner() == owner) {
                    *slot = None;
                }
            }
            s.changed.notify_one();
        }
    }
}
// Component guests and browser instances have one owning agent. No helper
// thread or callback is run from a driver turn; host notifications publish into
// the same reserved completion queue as native service results.
#[cfg(target_arch = "wasm32")]
mod service {
    use super::*;
    use crate::{
        OpId,
        blocking::{WorkOutput, WorkPort, WorkResult},
    };
    use std::cell::RefCell;
    const CAPACITY: usize = 16_384;
    struct Wait {
        op: OpId,
        port: Arc<WorkPort>,
        condition: WaitCondition,
        expected: u64,
        deadline: Option<Instant>,
    }
    thread_local! {
        static WAITS: RefCell<Vec<Option<Wait>>> = const { RefCell::new(Vec::new()) };
    }
    pub(super) fn initialize() -> Result<()> {
        WAITS.with(|waits| {
            let mut waits = waits.borrow_mut();
            if waits.capacity() == 0 {
                waits.reserve_exact(CAPACITY);
            }
        });
        Ok(())
    }
    fn complete(slot: &mut Option<Wait>, result: Result<WaitResult>, wake: bool) {
        let w = slot.take().expect("active wait");
        let result = WorkResult {
            op: w.op,
            result: result.map(WorkOutput::ExternalWait),
        };
        if wake {
            w.port.complete(result);
        } else {
            w.port.complete_during_turn(result);
        }
    }
    pub(super) fn notify(condition: &WaitCondition) {
        condition.generation.fetch_add(1, Ordering::Release);
        WAITS.with(|waits| {
            for slot in waits.borrow_mut().iter_mut() {
                if slot
                    .as_ref()
                    .is_some_and(|w| Arc::ptr_eq(&w.condition.generation, &condition.generation))
                {
                    complete(slot, Ok(WaitResult::Notified), true);
                }
            }
        });
    }
    pub(crate) fn submit(
        op: OpId,
        port: Arc<WorkPort>,
        condition: &WaitCondition,
        expected: u64,
        deadline: Option<Instant>,
    ) -> Result<()> {
        // Same-agent registration and notification cannot interleave here.
        if condition.load() != expected {
            port.complete(WorkResult {
                op,
                result: Ok(WorkOutput::ExternalWait(WaitResult::NotEqual)),
            });
            return Ok(());
        }
        WAITS.with(|waits| {
            let mut waits = waits.borrow_mut();
            let index = if let Some(i) = waits.iter().position(Option::is_none) {
                i
            } else if waits.len() < CAPACITY {
                waits.push(None); // Capacity was reserved at condition construction.
                waits.len() - 1
            } else {
                return Err(Error::new(ErrorKind::ResourceLimit));
            };
            waits[index] = Some(Wait {
                op,
                port,
                condition: condition.clone(),
                expected,
                deadline,
            });
            Ok(())
        })
    }
    pub(crate) fn deadline(owner: u64) -> Option<Instant> {
        WAITS.with(|waits| {
            waits
                .borrow()
                .iter()
                .flatten()
                .filter(|w| w.op.owner() == owner)
                .filter_map(|w| w.deadline)
                .min()
        })
    }
    pub(crate) fn poll(owner: u64, now: Instant) {
        WAITS.with(|waits| {
            for slot in waits.borrow_mut().iter_mut() {
                let Some(w) = slot else { continue };
                if w.op.owner() != owner {
                    continue;
                }
                if w.condition.load() != w.expected {
                    complete(slot, Ok(WaitResult::Notified), false);
                } else if w.deadline.is_some_and(|d| d <= now) {
                    complete(slot, Ok(WaitResult::TimedOut), false);
                }
            }
        });
    }
    pub(crate) fn cancel(op: OpId) {
        WAITS.with(|waits| {
            if let Some(slot) = waits
                .borrow_mut()
                .iter_mut()
                .find(|w| w.as_ref().is_some_and(|w| w.op == op))
            {
                complete(slot, Err(Error::new(ErrorKind::Cancelled)), true);
            }
        });
    }
    pub(crate) fn close(owner: u64) {
        WAITS.with(|waits| {
            for slot in waits.borrow_mut().iter_mut() {
                if slot.as_ref().is_some_and(|w| w.op.owner() == owner) {
                    *slot = None;
                }
            }
        });
    }
}
pub(crate) use service::{cancel, close, submit};
#[cfg(target_arch = "wasm32")]
pub(crate) use service::{deadline, poll};
