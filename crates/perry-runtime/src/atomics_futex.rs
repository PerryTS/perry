//! Futex-style park/wake table backing `Atomics.wait` / `Atomics.notify` /
//! `Atomics.waitAsync` across `perry/thread` agents (#4913 Stage 2).
//!
//! The table is keyed by the **absolute physical byte address** of the atomic
//! slot. Because a `SharedArrayBuffer` aliases the same backing bytes on every
//! thread (see [`crate::shared_sab`]), two agents viewing the same SAB index —
//! through any combination of element kinds or byte offsets — compute the same
//! key, so a `notify` issued on one OS thread wakes a `wait` parked on another.
//!
//! Correctness (no lost wakeups): the value re-check and the enqueue both
//! happen while the global table lock is held. A writer agent stores the new
//! value and then calls `notify`, which must acquire that same lock. So either
//! the waiter observes the new value and never parks (`NotEqual`), or it
//! enqueues first and the serialized `notify` finds and wakes it. The final
//! "did I get notified or did I time out" decision is likewise settled under
//! the table lock, so a `notify` racing a timeout resolves to exactly one
//! outcome and the notifier's wake count stays accurate.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// One parked waiter. Shared (`Arc`) between the parking thread (which blocks
/// on `cv`) and the table entry the notifier walks.
struct Waiter {
    /// Set to `true` by `notify` to mark this waiter as woken (vs. timed out).
    notified: Mutex<bool>,
    cv: Condvar,
}

type WaitTable = HashMap<usize, Vec<Arc<Waiter>>>;

static WAIT_TABLE: OnceLock<Mutex<WaitTable>> = OnceLock::new();

fn table() -> &'static Mutex<WaitTable> {
    WAIT_TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_table() -> std::sync::MutexGuard<'static, WaitTable> {
    table().lock().unwrap_or_else(|e| e.into_inner())
}

/// Outcome of a parked wait.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitOutcome {
    /// The slot value no longer matched at the atomic check — never parked.
    NotEqual,
    /// Woken by a matching `notify`.
    Ok,
    /// The timeout elapsed without a `notify`.
    TimedOut,
}

/// Handle returned by [`enqueue`]; pass to [`block`] to park on it.
pub struct WaitHandle {
    addr: usize,
    waiter: Arc<Waiter>,
}

/// Atomically (under the table lock) re-check the slot and, if it still matches,
/// register a waiter for `addr`. Returns `None` if the value already changed
/// (caller should report `"not-equal"`), otherwise a handle to park on.
///
/// `still_equal` runs while the table lock is held, so it is serialized against
/// any concurrent `notify` (which also takes the lock after its value store).
pub fn enqueue(addr: usize, still_equal: impl FnOnce() -> bool) -> Option<WaitHandle> {
    let mut tbl = lock_table();
    if !still_equal() {
        return None;
    }
    let waiter = Arc::new(Waiter {
        notified: Mutex::new(false),
        cv: Condvar::new(),
    });
    tbl.entry(addr).or_default().push(waiter.clone());
    Some(WaitHandle { addr, waiter })
}

/// Park on a previously [`enqueue`]d handle until a matching `notify` or until
/// `timeout` elapses (`None` = block forever). Removes the waiter from the table
/// before returning. Returns [`WaitOutcome::Ok`] or [`WaitOutcome::TimedOut`].
pub fn block(handle: WaitHandle, timeout: Option<Duration>) -> WaitOutcome {
    let WaitHandle { addr, waiter } = handle;
    let deadline = timeout.map(|d| Instant::now().checked_add(d));

    {
        let mut g = waiter.notified.lock().unwrap_or_else(|e| e.into_inner());
        while !*g {
            match deadline {
                // No timeout: block until notified.
                None => {
                    g = waiter.cv.wait(g).unwrap_or_else(|e| e.into_inner());
                }
                // Timeout that overflowed `Instant` (effectively infinite).
                Some(None) => {
                    g = waiter.cv.wait(g).unwrap_or_else(|e| e.into_inner());
                }
                Some(Some(dl)) => {
                    let now = Instant::now();
                    if now >= dl {
                        break;
                    }
                    let (ng, _to) = waiter
                        .cv
                        .wait_timeout(g, dl - now)
                        .unwrap_or_else(|e| e.into_inner());
                    g = ng;
                }
            }
        }
    }

    // Settle under the table lock so a `notify` racing this timeout resolves to
    // exactly one outcome (and is counted exactly once by the notifier).
    let mut tbl = lock_table();
    let notified = *waiter.notified.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(list) = tbl.get_mut(&addr) {
        list.retain(|w| !Arc::ptr_eq(w, &waiter));
        if list.is_empty() {
            tbl.remove(&addr);
        }
    }
    if notified {
        WaitOutcome::Ok
    } else {
        WaitOutcome::TimedOut
    }
}

/// Synchronous park: [`enqueue`] then [`block`]. Returns `NotEqual` without
/// parking if the slot value changed at the atomic check.
pub fn wait(
    addr: usize,
    timeout: Option<Duration>,
    still_equal: impl FnOnce() -> bool,
) -> WaitOutcome {
    match enqueue(addr, still_equal) {
        None => WaitOutcome::NotEqual,
        Some(handle) => block(handle, timeout),
    }
}

/// Wake up to `count` waiters parked on `addr`. Returns the number actually
/// woken. `count == usize::MAX` wakes all of them (`Atomics.notify` with an
/// `undefined` / `+Infinity` count).
pub fn notify(addr: usize, count: usize) -> usize {
    let mut tbl = lock_table();
    let Some(list) = tbl.get_mut(&addr) else {
        return 0;
    };
    let n = count.min(list.len());
    let mut woken = 0;
    for waiter in list.drain(..n) {
        {
            let mut g = waiter.notified.lock().unwrap_or_else(|e| e.into_inner());
            *g = true;
        }
        waiter.cv.notify_one();
        woken += 1;
    }
    if list.is_empty() {
        tbl.remove(&addr);
    }
    woken
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notify_wakes_a_parked_waiter() {
        // A fresh isolated address (a stack slot) avoids collisions with other
        // tests sharing the process-global table.
        let cell = 0u64;
        let addr = &cell as *const u64 as usize;

        let handle = enqueue(addr, || true).expect("enqueued");
        let waker = std::thread::spawn(move || {
            // Give the parker time to block, then wake it.
            std::thread::sleep(Duration::from_millis(20));
            notify(addr, 1)
        });
        let outcome = block(handle, Some(Duration::from_secs(5)));
        let woken = waker.join().unwrap();
        assert_eq!(outcome, WaitOutcome::Ok);
        assert_eq!(woken, 1);
    }

    #[test]
    fn wait_times_out_with_no_notifier() {
        let cell = 0u64;
        let addr = &cell as *const u64 as usize;
        let outcome = wait(addr, Some(Duration::from_millis(10)), || true);
        assert_eq!(outcome, WaitOutcome::TimedOut);
    }

    #[test]
    fn wait_returns_not_equal_without_parking() {
        let cell = 0u64;
        let addr = &cell as *const u64 as usize;
        let outcome = wait(addr, None, || false);
        assert_eq!(outcome, WaitOutcome::NotEqual);
    }

    #[test]
    fn notify_empty_address_wakes_nobody() {
        let cell = 0u64;
        let addr = &cell as *const u64 as usize;
        assert_eq!(notify(addr, usize::MAX), 0);
    }
}
