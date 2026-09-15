//! turnloop P0: one `turnloop::Loop` per native JS agent, used as that agent's
//! event-loop wait primitive (DESIGN §9 "Wiring for P0").
//!
//! What P0 hands to turnloop: the *wait*. Perry still owns timers, microtasks,
//! nextTick, every pump and every keep-alive decision; P0 submits no turnloop
//! operation. A turn therefore does exactly one thing: block in one OS wait
//! (`kevent` / `epoll_pwait2` / `GetQueuedCompletionStatusEx`) until the exact
//! `Instant` deadline `js_wait_for_event` computed, or until a producer wakes
//! the loop through its `Notifier`.
//!
//! Thread model (DESIGN §5a):
//! - The loop is thread-local. There is no process-global loop. The primary
//!   agent's loop is created lazily by that agent's first real park.
//! - `js_notify_main_thread` addresses the primary agent, so the only
//!   process-global piece is [`PRIMARY_ROUTE`]: that agent's `Notifier` (a
//!   cloneable wake endpoint, not the loop) plus a flag saying whether the
//!   owning thread is inside `turn`.
//! - Worker agents have no loop in P0 and keep the legacy park unchanged
//!   (`perry/thread` workers cannot `await`; a `worker_threads` Worker that
//!   awaits parks on the condvar or on the legacy registered driver exactly as
//!   before). P3/P4 give every agent its own loop, poster and timer heap.
//!
//! Wake protocol (no lost wake, no hot-path syscall, no hot-path lock):
//! the owner sets `in_turn` and then re-reads the runtime's `NOTIFIED` flag
//! (and the native in-flight predicate) before turning; a producer publishes
//! its work (stores `NOTIFIED`, or makes work visible to that predicate) and
//! then reads `in_turn` (both `SeqCst`). Either the owner sees the work and
//! skips the wait, or the
//! producer sees `in_turn` and calls `Notifier::notify`, whose own
//! RUNNING/PARKED/NOTIFIED handshake covers the window before the OS wait.
//! Outside a turn a notify is a single atomic load: the owner is running JS
//! and observes `NOTIFIED` on its next `js_wait_for_event` fast path, so
//! notifying turnloop too would only leave a stale bit that costs one
//! zero-event poll.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

use turnloop::{Completions, Config, Loop, Notifier, Timeout};

/// Cross-thread route to the primary agent's loop.
struct PrimaryRoute {
    /// True exactly while the owning thread is inside `Loop::turn`.
    in_turn: AtomicBool,
    /// `(loop id, notifier)` of the thread that currently owns the route.
    notifier: Mutex<Option<(u64, Notifier)>>,
}

static PRIMARY_ROUTE: PrimaryRoute = PrimaryRoute {
    in_turn: AtomicBool::new(false),
    notifier: Mutex::new(None),
};

/// Identity for route ownership; lets a dropped loop clear only its own route.
static NEXT_LOOP_ID: AtomicU64 = AtomicU64::new(1);

/// P0 submits no operations, so the loop needs no real capacity. The default
/// `Config` preallocates 256 × 16 KiB read buffers and 4096 operation slots —
/// megabytes of RSS for a process that only waits. P1 must size this for the
/// handles it actually moves onto the loop.
fn p0_config() -> Config {
    Config {
        max_handles: 16,
        max_operations: 16,
        events_per_turn: 16,
        pooled_buffers: 0,
        pooled_buffer_size: 1,
        post_capacity: 16,
        ..Config::default()
    }
}

/// Diagnostic counters for the `PERRY_LOOP_STATS=1` exit line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoopStats {
    /// `Loop::turn` calls.
    pub turns: u64,
    /// Native OS waits turnloop reported (at most one per turn).
    pub os_waits: u64,
    /// OS waits that returned with no I/O or notifier event (a timed-out
    /// deadline wait is one of these).
    pub zero_event_waits: u64,
    /// P0-transitional parks that drove the legacy tokio tick instead of a
    /// turn, because tokio-owned native work was in flight.
    pub native_ticks: u64,
    /// Turns that returned an error; the park fell back to the condvar.
    pub turn_errors: u64,
}

pub(super) struct AgentLoop {
    id: u64,
    driver: Loop,
    completions: Completions,
    stats: LoopStats,
}

impl AgentLoop {
    fn new() -> turnloop::Result<Self> {
        let driver = Loop::new(p0_config())?;
        Ok(Self {
            id: NEXT_LOOP_ID.fetch_add(1, Ordering::Relaxed),
            driver,
            completions: Completions::with_capacity(16),
            stats: LoopStats::default(),
        })
    }

    fn record(&mut self, info: &turnloop::TurnInfo) {
        self.stats.turns += 1;
        self.stats.os_waits += u64::from(info.os_waits);
        self.stats.zero_event_waits += u64::from(info.zero_event_waits);
        // P0 submits nothing, so nothing can complete. P1 dispatches
        // completions here, after `turn` returned, before releasing any root
        // associated with a token (DESIGN D1/D4).
        debug_assert!(
            self.completions.is_empty(),
            "P0 agent loop produced a completion without a submitted operation"
        );
    }
}

impl Drop for AgentLoop {
    fn drop(&mut self) {
        // Thread exit is a teardown path too (unit-test threads, embedders).
        // Clear the route only if it is still this loop's.
        let mut route = PRIMARY_ROUTE
            .notifier
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if route.as_ref().is_some_and(|(id, _)| *id == self.id) {
            *route = None;
        }
        // `Loop::drop` closes the notifier, poster and native backend. P0 owns
        // no handles and never touched the blocking pool, so nothing joins.
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoopState {
    /// This thread has not parked through the precise path yet.
    Unset,
    /// This thread owns the primary agent's loop and route.
    Owner,
    /// Not eligible: a worker agent, the route is held by another thread, or
    /// loop creation failed. Parks use the legacy path.
    Declined,
    /// `shutdown_current_thread` ran; parks use the legacy path from now on.
    ShutDown,
}

thread_local! {
    static STATE: Cell<LoopState> = const { Cell::new(LoopState::Unset) };
    static AGENT_LOOP: RefCell<Option<AgentLoop>> = const { RefCell::new(None) };
}

/// Whether this thread may take the precise park path. One TLS read once the
/// loop exists; a worker agent is declined for its whole life.
#[inline]
pub(super) fn eligible() -> bool {
    match STATE.with(Cell::get) {
        LoopState::Owner => true,
        LoopState::Unset => crate::agent::current_agent() == crate::agent::PRIMARY_AGENT,
        LoopState::Declined | LoopState::ShutDown => false,
    }
}

/// Create this thread's loop on first use. Returns whether the thread owns one.
pub(super) fn ensure_loop() -> bool {
    match STATE.with(Cell::get) {
        LoopState::Owner => return true,
        LoopState::Declined | LoopState::ShutDown => return false,
        LoopState::Unset => {}
    }
    if crate::agent::current_agent() != crate::agent::PRIMARY_AGENT {
        STATE.with(|s| s.set(LoopState::Declined));
        return false;
    }
    let mut route = PRIMARY_ROUTE
        .notifier
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if route.is_some() {
        // A second thread acting for the primary agent (a host pump thread).
        // Exactly one thread owns the route; this one keeps the legacy park.
        STATE.with(|s| s.set(LoopState::Declined));
        return false;
    }
    let agent = match AgentLoop::new() {
        Ok(agent) => agent,
        Err(_) => {
            // Descriptor exhaustion or an unsupported host. Keep the legacy
            // park rather than failing the program; the stats line says so.
            STATE.with(|s| s.set(LoopState::Declined));
            return false;
        }
    };
    *route = Some((agent.id, agent.driver.notifier()));
    drop(route);
    AGENT_LOOP.with(|slot| *slot.borrow_mut() = Some(agent));
    STATE.with(|s| s.set(LoopState::Owner));
    true
}

/// Outcome of [`park_until`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Park {
    /// A turn ran: it waited until the deadline or a wake.
    Waited,
    /// A notify was already pending; no wait happened.
    Notified,
    /// No loop on this thread, or the turn failed; use the fallback park.
    Failed,
}

/// Block until `deadline` or a wake, in one turn.
pub(super) fn park_until(deadline: Instant) -> Park {
    AGENT_LOOP.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(agent) = slot.as_mut() else {
            return Park::Failed;
        };
        PRIMARY_ROUTE.in_turn.store(true, Ordering::SeqCst);
        if super::NOTIFIED.load(Ordering::SeqCst) || super::precise_wait::native_inflight() {
            // A notify landed after the fast path (leave the flag for the next
            // `js_wait_for_event` fast path to consume), or tokio-owned native
            // work appeared after the caller chose this wait
            // (`js_native_work_submitted`). Either way, go back around the loop.
            PRIMARY_ROUTE.in_turn.store(false, Ordering::SeqCst);
            return Park::Notified;
        }
        let started = super::loop_stats::begin_wait(super::loop_stats::WaitKind::Turnloop);
        let result = agent
            .driver
            .turn(Timeout::Until(deadline), &mut agent.completions);
        super::loop_stats::end_wait(super::loop_stats::WaitKind::Turnloop, started);
        PRIMARY_ROUTE.in_turn.store(false, Ordering::SeqCst);
        match result {
            Ok(info) => {
                agent.record(&info);
                Park::Waited
            }
            Err(_) => {
                agent.stats.turns += 1;
                agent.stats.turn_errors += 1;
                Park::Failed
            }
        }
    })
}

/// The loop's own earliest deadline (DESIGN §9: the deadline provider becomes
/// `next_deadline()` where the loop owns deadlines). P0 arms no turnloop timer,
/// so this is `None` until P3 moves JS timers into the loop's heap.
pub(super) fn loop_deadline() -> Option<Instant> {
    if STATE.with(Cell::get) != LoopState::Owner {
        return None;
    }
    AGENT_LOOP.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|agent| agent.driver.next_deadline())
    })
}

/// DESIGN §9 `fast()`: a nonblocking turn, only when turnloop has outstanding
/// work. P0 submits no operation, so `alive()` is false and this makes no OS
/// call on the hot promise path.
#[inline]
pub(super) fn fast_turn() {
    if STATE.with(Cell::get) != LoopState::Owner {
        return;
    }
    AGENT_LOOP.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(agent) = slot.as_mut() {
            if agent.driver.alive() {
                if let Ok(info) = agent.driver.turn(Timeout::Now, &mut agent.completions) {
                    agent.record(&info);
                }
            }
        }
    });
}

/// Count a transitional tokio tick taken instead of a turn.
pub(super) fn note_native_tick() {
    AGENT_LOOP.with(|slot| {
        if let Some(agent) = slot.borrow_mut().as_mut() {
            agent.stats.native_ticks += 1;
        }
    });
}

/// Wake the primary agent's loop if it is inside a turn. `js_notify_main_thread`
/// calls this after storing `NOTIFIED`; `js_native_work_submitted` after new
/// tokio-owned work became visible to the in-flight predicate.
#[inline]
pub(super) fn wake_primary() {
    if PRIMARY_ROUTE.in_turn.load(Ordering::SeqCst) {
        wake_primary_slow();
    }
}

#[cold]
fn wake_primary_slow() {
    let route = PRIMARY_ROUTE
        .notifier
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some((_, notifier)) = route.as_ref() {
        // Err means the loop is closing: there is no waiter left to wake.
        let _ = notifier.notify();
    }
}

/// This thread's loop counters, if it owns a loop.
pub fn loop_statistics() -> Option<LoopStats> {
    AGENT_LOOP.with(|slot| slot.borrow().as_ref().map(|agent| agent.stats))
}

/// Destroy this thread's loop at the process-exit funnel and print the
/// `PERRY_LOOP_STATS=1` line once. Idempotent; later parks use the legacy path.
pub fn shutdown_current_thread() {
    let previous = STATE.with(|s| s.replace(LoopState::ShutDown));
    if previous == LoopState::ShutDown {
        return;
    }
    let agent = AGENT_LOOP.with(|slot| slot.borrow_mut().take());
    if stats_enabled() {
        match (&agent, previous) {
            (Some(agent), _) => print_stats(agent.stats),
            (None, LoopState::Declined) => eprintln!("[perry-loop] driver=legacy"),
            (None, _) => eprintln!("[perry-loop] driver=turnloop parked=0"),
        }
    }
    drop(agent);
}

fn stats_enabled() -> bool {
    super::loop_stats::enabled()
}

fn print_stats(stats: LoopStats) {
    eprintln!(
        "[perry-loop] driver=turnloop turns={} os_waits={} zero_event_waits={} native_ticks={} turn_errors={}",
        stats.turns, stats.os_waits, stats.zero_event_waits, stats.native_ticks, stats.turn_errors
    );
}

#[cfg(test)]
#[path = "agent_loop_tests.rs"]
mod tests;
