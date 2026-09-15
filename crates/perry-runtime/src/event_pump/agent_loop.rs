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

use turnloop::{Completions, Config, Handle, Loop, Notifier, Timeout, Token};

/// The token of the single timer this agent arms for its JS timer heap.
///
/// `turnloop_net` builds its tokens as `(op_class << 56) | id` with op classes
/// 1..=7 and a debug-asserted non-zero id below `ID_MASK`, so `u64::MAX`
/// (op class 255) can never collide with one.
pub(crate) const TIMER_TOKEN: Token = Token(u64::MAX);

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

/// How much loop the program has asked for. `Config` is fixed at
/// `Loop::new`, and the default preallocates 256 × 16 KiB read buffers and
/// 4096 operation slots — megabytes of RSS for a process that only waits. A
/// timer-only program must not pay that, and a server must not be capped at
/// 16 handles, so the loop is created at the profile in force and *upgraded*
/// (recreated) the first time a net submission needs the larger one. The
/// upgrade is only ever Wait → Net, and only while the loop owns no handles,
/// which is exactly the state P0 leaves it in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Profile {
    /// P0: the loop is a wait primitive. No operation is ever submitted.
    Wait,
    /// P1: sockets live on the loop.
    Net,
}

/// P0 submits no operations, so the loop needs no real capacity.
fn wait_config() -> Config {
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

/// Sized for a server: a listener, its connections, and their in-flight reads
/// and writes. `pooled_buffers` matches `events_per_turn` on purpose — a read
/// lease is released inside the same dispatch pass that produced it, so the
/// pool only has to cover one turn's worth of concurrently delivered reads.
/// Under-provisioning it would not lose data (turnloop leaves the read
/// pending, which is backpressure), but it would cost an extra turn per read.
fn net_config() -> Config {
    Config {
        max_handles: 4096,
        max_operations: 8192,
        events_per_turn: 64,
        pooled_buffers: 64,
        pooled_buffer_size: 16 * 1024,
        post_capacity: 256,
        ..Config::default()
    }
}

fn config_for(profile: Profile) -> Config {
    match profile {
        Profile::Wait => wait_config(),
        Profile::Net => net_config(),
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
    /// Completions dispatched to a P1 net subsystem. Zero means turnloop
    /// carried no I/O for this process, whatever the turn count says.
    pub completions: u64,
    /// JS timer deadlines that expired as a turnloop timer completion (P3).
    /// Zero on a program with timers means the heap's deadline never reached
    /// the loop — the arming is decorative and the stats line says so.
    pub timer_expiries: u64,
    /// Times the armed deadline was created, moved or cancelled.
    pub timer_arms: u64,
}

pub(super) struct AgentLoop {
    id: u64,
    profile: Profile,
    driver: Loop,
    completions: Completions,
    stats: LoopStats,
    /// The single timer handle carrying this agent's JS timer deadline, and the
    /// deadline it currently holds.
    timer: Option<(Handle, Instant)>,
}

impl AgentLoop {
    fn new(profile: Profile) -> turnloop::Result<Self> {
        let config = config_for(profile);
        let capacity = config.events_per_turn.max(1);
        let driver = Loop::new(config)?;
        Ok(Self {
            id: NEXT_LOOP_ID.fetch_add(1, Ordering::Relaxed),
            profile,
            driver,
            completions: Completions::with_capacity(capacity),
            stats: LoopStats::default(),
            timer: None,
        })
    }

    /// Account for one turn and move its completions into the staging buffer.
    ///
    /// The completions are *moved*, not dispatched: dispatch runs host code
    /// (a JS `'data'` listener) that re-enters this module to submit more
    /// work, so it must happen after the borrow on [`AGENT_LOOP`] is released
    /// (DESIGN D1 — the driver never calls host code, and neither does this).
    fn record(&mut self, info: &turnloop::TurnInfo) {
        self.stats.turns += 1;
        self.stats.os_waits += u64::from(info.os_waits);
        self.stats.zero_event_waits += u64::from(info.zero_event_waits);
        if self.completions.is_empty() {
            return;
        }
        self.stats.completions += self.completions.len() as u64;
        STAGED.with(|staged| staged.borrow_mut().extend(self.completions.drain()));
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
    /// Completions moved out of the driver by [`AgentLoop::record`] and not
    /// yet routed. Owned by this thread, drained in FIFO order by
    /// [`dispatch_staged`] once no borrow on `AGENT_LOOP` is held.
    static STAGED: RefCell<Vec<turnloop::Completion>> = const { RefCell::new(Vec::new()) };
}

/// Route every staged completion to its subsystem.
///
/// Runs outside any `AGENT_LOOP` borrow, because a sink legitimately submits
/// new operations (a `'data'` listener that writes a reply) and would
/// otherwise re-enter a live `RefCell` borrow. Re-entry is still possible —
/// a submission can drive `fast_turn` — so the batch is taken before any of
/// it runs; a nested call then finds an empty buffer and does nothing.
fn dispatch_staged() {
    let mut batch = STAGED.with(|staged| std::mem::take(&mut *staged.borrow_mut()));
    if batch.is_empty() {
        return;
    }
    for completion in batch.drain(..) {
        if completion.token == TIMER_TOKEN {
            // The JS timer heap's deadline. Nothing to deliver: the expiry IS
            // the wake, and the timers phase reads the heap. Counted so a
            // `PERRY_LOOP_STATS` line can say the arming was live.
            if matches!(completion.result, turnloop::OpResult::Timer) {
                note_timer_expiry();
            }
            continue;
        }
        // One router, three token spaces. P1's classes are 1..=7, P2's are
        // 0x10..=0x1F and P3 owns TIMER_TOKEN above, so `owns` is a range test
        // and no module can be handed another's completion
        // (`turnloop_proc`'s module note).
        if crate::turnloop_proc::owns(completion.token) {
            crate::turnloop_proc::dispatch(completion);
        } else {
            crate::turnloop_net::dispatch(completion);
        }
    }
    // Give the emptied allocation back so steady-state dispatch allocates
    // nothing (DESIGN §10 rule 1).
    STAGED.with(|staged| {
        let mut slot = staged.borrow_mut();
        if slot.is_empty() && slot.capacity() < batch.capacity() {
            *slot = batch;
        }
    });
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
    ensure_loop_with(Profile::Wait)
}

/// Create — or upgrade — this thread's loop for `profile`.
///
/// An upgrade recreates the loop, which is sound only while it owns no
/// handles. That is asserted rather than assumed: P0's loop owns none by
/// construction, and the first net submission is what triggers the upgrade,
/// so a loop that already carries sockets is never rebuilt under them.
pub(super) fn ensure_loop_with(profile: Profile) -> bool {
    match STATE.with(Cell::get) {
        LoopState::Owner => return upgrade_profile(profile),
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
    let agent = match AgentLoop::new(profile) {
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

/// Rebuild this thread's loop at a larger profile, if it is not there yet.
///
/// Returns false only if the rebuild failed, in which case the old loop is
/// gone and the thread falls back to the legacy park — the same outcome as a
/// loop that never got created, and the stats line still says so.
fn upgrade_profile(profile: Profile) -> bool {
    let needs_upgrade = AGENT_LOOP.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|agent| agent.profile < profile)
    });
    if !needs_upgrade {
        return true;
    }
    debug_assert_eq!(
        crate::turnloop_net::live_handles() + crate::turnloop_proc::live_handles(),
        0,
        "the loop profile is upgraded before the first handle, never under one"
    );
    let previous = AGENT_LOOP.with(|slot| slot.borrow_mut().take());
    let carried = previous.as_ref().map(|agent| agent.stats);
    drop(previous);
    // `AgentLoop::drop` cleared the route; install the replacement's.
    let mut agent = match AgentLoop::new(profile) {
        Ok(agent) => agent,
        Err(_) => {
            STATE.with(|s| s.set(LoopState::Declined));
            return false;
        }
    };
    if let Some(stats) = carried {
        agent.stats = stats;
    }
    let mut route = PRIMARY_ROUTE
        .notifier
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    *route = Some((agent.id, agent.driver.notifier()));
    drop(route);
    AGENT_LOOP.with(|slot| *slot.borrow_mut() = Some(agent));
    // The replaced loop took its timer handle with it; re-arm on the new one
    // from the store, outside the borrow above.
    crate::timer::resync_loop_timer();
    true
}

/// Run `f` against this agent's driver, creating or upgrading the loop to the
/// net profile first. `None` means this thread has no loop and the caller must
/// keep its legacy transport.
pub(super) fn with_net_driver<R>(f: impl FnOnce(&mut Loop) -> R) -> Option<R> {
    if !ensure_loop_with(Profile::Net) {
        return None;
    }
    AGENT_LOOP.with(|slot| slot.borrow_mut().as_mut().map(|agent| f(&mut agent.driver)))
}

/// Give the calling thread a loop at `profile` WITHOUT taking the process-wide
/// route, so a test that only exercises turns and completions cannot race
/// another test thread for route ownership.
#[cfg(test)]
pub(super) fn install_unrouted_for_test(profile: Profile) -> bool {
    match AgentLoop::new(profile) {
        Ok(agent) => {
            AGENT_LOOP.with(|slot| *slot.borrow_mut() = Some(agent));
            STATE.with(|s| s.set(LoopState::Owner));
            true
        }
        Err(_) => false,
    }
}

/// One bounded turn plus its completion dispatch, for tests that need the loop
/// driven without the surrounding event pump.
#[cfg(test)]
pub(super) fn turn_for_test(budget: std::time::Duration) {
    AGENT_LOOP.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(agent) = slot.as_mut() {
            if let Ok(info) = agent
                .driver
                .turn(turnloop::Timeout::After(budget), &mut agent.completions)
            {
                agent.record(&info);
            }
        }
    });
    dispatch_staged();
}

/// Drop this thread's loop and any staged completions, so the next test starts
/// from a clean slate even though it runs on the same process.
#[cfg(test)]
pub(super) fn reset_for_test() {
    crate::turnloop_net::reset_for_test();
    crate::turnloop_proc::reset_for_test();
    AGENT_LOOP.with(|slot| *slot.borrow_mut() = None);
    STAGED.with(|staged| staged.borrow_mut().clear());
    STATE.with(|s| s.set(LoopState::Unset));
}

/// Whether the loop has referenced handles, operations or queued results —
/// `Loop::alive()`, O(1). Used to decide whether a park must service turnloop
/// as well as the transitional tokio tick.
pub(super) fn has_outstanding_work() -> bool {
    if STATE.with(Cell::get) != LoopState::Owner {
        return false;
    }
    AGENT_LOOP.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|agent| agent.driver.alive())
    })
}

/// Whether this thread can own the primary agent's loop at all.
///
/// Answers without creating one: a caller asking "may I use turnloop?" on a
/// worker agent must not pay for a loop it will never park in.
pub(super) fn net_available() -> bool {
    match STATE.with(Cell::get) {
        LoopState::Owner => true,
        LoopState::Declined | LoopState::ShutDown => false,
        LoopState::Unset => crate::agent::current_agent() == crate::agent::PRIMARY_AGENT,
    }
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
    let outcome = park_turn(deadline);
    // Outside the borrow: a sink may submit, and a submission may turn.
    dispatch_staged();
    outcome
}

fn park_turn(deadline: Instant) -> Park {
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

/// Count one expiry of the armed JS-timer deadline.
fn note_timer_expiry() {
    AGENT_LOOP.with(|slot| {
        if let Ok(mut slot) = slot.try_borrow_mut() {
            if let Some(agent) = slot.as_mut() {
                agent.stats.timer_expiries += 1;
                // A one-shot timer's expiry is terminal: its operation retired,
                // so the handle can no longer be reset and must be closed
                // before the next deadline is armed.
                if let Some((handle, _)) = agent.timer.take() {
                    let _ = agent.driver.close(handle, TIMER_TOKEN);
                }
            }
        }
    });
}

/// Arm — or move, or cancel — this agent's single JS-timer deadline.
///
/// turnloop P3 (DESIGN §9): the JS timer heap's earliest deadline becomes a
/// real turnloop timer, so a park that ends at a timer ends on an
/// `OpResult::Timer` completion rather than on a timeout Perry computed for
/// itself, and `Loop::next_deadline()` answers for Perry's timers too.
///
/// The handle is deliberately **unreferenced**: Perry's own keep-alive counters
/// decide whether the loop lives, and an armed deadline must never make
/// `Loop::alive()` true by itself. A referenced timer operation counts toward
/// `refs`, so the `set_ref(false)` below is load-bearing, not hygiene — there is
/// a unit test that arms a timer and asserts `alive()` stays false.
pub(crate) fn arm_timer(at: Option<Instant>) {
    if STATE.with(Cell::get) != LoopState::Owner {
        return;
    }
    AGENT_LOOP.with(|slot| {
        // `try_borrow_mut` fails only under re-entry from a completion sink
        // that is already inside this module; that pass re-arms on its way out.
        let Ok(mut slot) = slot.try_borrow_mut() else {
            return;
        };
        let Some(agent) = slot.as_mut() else {
            return;
        };
        match (agent.timer, at) {
            (Some((_, armed)), Some(at)) if armed == at => {}
            (Some((handle, _)), Some(at)) if agent.driver.timer_reset(handle, at) => {
                agent.timer = Some((handle, at));
                agent.stats.timer_arms += 1;
            }
            (previous, at) => {
                if let Some((handle, _)) = previous {
                    let _ = agent.driver.close(handle, TIMER_TOKEN);
                    agent.timer = None;
                }
                if let Some(at) = at {
                    match agent.driver.timer(at, None, TIMER_TOKEN) {
                        Ok(handle) => {
                            // Must not hold the loop alive on its own.
                            let _ = agent.driver.set_ref(handle, false);
                            agent.timer = Some((handle, at));
                            agent.stats.timer_arms += 1;
                        }
                        // Resource limit or a closing loop: the park still has
                        // Perry's own deadline, so this costs precision in the
                        // stats line, not correctness.
                        Err(_) => agent.timer = None,
                    }
                }
            }
        }
    });
}

/// The loop's own earliest deadline (DESIGN §9: the deadline provider becomes
/// `next_deadline()` where the loop owns deadlines). Since P3 this includes the
/// armed JS timer deadline.
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
    let turned = AGENT_LOOP.with(|slot| {
        let mut slot = slot.borrow_mut();
        // `borrow_mut` fails only under re-entry from a sink, which is
        // already inside a dispatch pass: skipping is correct, not a lost
        // wake, because that pass turns again on its way out.
        let Some(agent) = slot.as_mut() else {
            return false;
        };
        if !agent.driver.alive() {
            return false;
        }
        if let Ok(info) = agent.driver.turn(Timeout::Now, &mut agent.completions) {
            agent.record(&info);
        }
        true
    });
    if turned {
        dispatch_staged();
    }
}

/// One nonblocking turn plus its dispatch, *without* the `alive()` gate.
///
/// [`fast_turn`] deliberately skips a loop with no outstanding work, which is
/// right on the hot promise path. A close that must be observable by the next
/// statement is the opposite case: the caller has just submitted a `close` and
/// needs its terminal completion now, and the handle may already be unref'd
/// (an `unref()`'d socket being closed), so `alive()` would say there is
/// nothing to do and the descriptor would stay open.
pub(super) fn settle_turn() {
    if STATE.with(Cell::get) != LoopState::Owner {
        return;
    }
    let turned = AGENT_LOOP.with(|slot| {
        let Ok(mut slot) = slot.try_borrow_mut() else {
            // Re-entry from inside a dispatch pass: that pass turns again on
            // its way out, so skipping is correct rather than a lost wake.
            return false;
        };
        let Some(agent) = slot.as_mut() else {
            return false;
        };
        if let Ok(info) = agent.driver.turn(Timeout::Now, &mut agent.completions) {
            agent.record(&info);
        }
        true
    });
    if turned {
        dispatch_staged();
    }
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
    if STATE.with(Cell::get) == LoopState::Owner {
        // Close P1's sockets while the loop is still here, then run one
        // nonblocking turn so their `Closed` completions reach the binding
        // (exactly-once release, DESIGN D4). `Loop::drop` would free the
        // descriptors either way; this is what lets a binding's own
        // bookkeeping see the close rather than inferring it from teardown.
        crate::turnloop_net::shutdown_current_thread();
        crate::turnloop_proc::shutdown_current_thread();
        fast_turn();
    }
    let previous = STATE.with(|s| s.replace(LoopState::ShutDown));
    if previous == LoopState::ShutDown {
        return;
    }
    let agent = AGENT_LOOP.with(|slot| slot.borrow_mut().take());
    STAGED.with(|staged| staged.borrow_mut().clear());
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
        "[perry-loop] driver=turnloop turns={} os_waits={} zero_event_waits={} native_ticks={} turn_errors={} completions={} timer_arms={} timer_expiries={}",
        stats.turns,
        stats.os_waits,
        stats.zero_event_waits,
        stats.native_ticks,
        stats.turn_errors,
        stats.completions,
        stats.timer_arms,
        stats.timer_expiries
    );
    // P2's own "the subject ran" line. `completions` above cannot distinguish
    // a socket P1 carried from a child pipe P2 carried, and every live count
    // is zero by the time a process exits — so the lifetime adoption count is
    // what an A/B or an acceptance test reads to know the threads really were
    // replaced rather than merely not used.
    eprintln!(
        "[perry-loop] p2 adopted={} live={} dgram_sockets={} signals={}",
        crate::turnloop_proc::adopted_total(),
        crate::turnloop_proc::live_handles(),
        dgram_sockets_on_turnloop(),
        crate::os::signal::signals_on_turnloop(),
    );
}

#[cfg(feature = "mod-dgram")]
fn dgram_sockets_on_turnloop() -> u64 {
    crate::dgram_reactor::turnloop_sockets()
}

#[cfg(not(feature = "mod-dgram"))]
fn dgram_sockets_on_turnloop() -> u64 {
    0
}

#[cfg(test)]
#[path = "agent_loop_tests.rs"]
mod tests;
