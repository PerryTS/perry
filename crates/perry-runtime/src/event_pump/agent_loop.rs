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
//! Thread model (DESIGN §5a; P9 made it true for every agent):
//! - The loop is thread-local, and **every JS agent may own one** — the
//!   primary agent, a `node:worker_threads` Worker, a `perry/thread` worker.
//!   It is created lazily by that agent's first park or first net submission.
//! - Exactly ONE thread owns an agent's loop. A second thread acting for the
//!   same agent — Android's UI thread pumping on behalf of `perry-native`, an
//!   embedder's host thread — is declined and keeps the legacy park, which is
//!   the behaviour it has today. The tie-break is "first to ask wins", and the
//!   thread that asks first is the one running that agent's event loop.
//! - The cross-thread piece is therefore a route *table* keyed by [`AgentId`]
//!   ([`ROUTES`]): each entry is one agent's `Notifier` (a cloneable wake
//!   endpoint, not the loop) plus a flag saying whether its owner is inside
//!   `turn`. [`PARKED_LOOPS`] keeps a producer's fast path at the single
//!   atomic load the one-route design had.
//! - `js_notify_main_thread` is a *broadcast*, not a point-to-point send: the
//!   flag it sets (`event_pump::NOTIFIED`) and the condvar it signals are both
//!   process-global and `notify_all`-shaped, so the turnloop wake has to reach
//!   every parked agent or a Worker waiting on a `postMessage`-driven
//!   resolution would never wake. Only agents actually inside a turn are
//!   poked, so an idle agent costs nothing.
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
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::ThreadId;
use std::time::Instant;

use turnloop::{Completions, Config, Handle, Loop, Notifier, Payload, Poster, Timeout, Token};

use crate::agent::AgentId;

/// The token of the single timer this agent arms for its JS timer heap.
///
/// `turnloop_net` builds its tokens as `(op_class << 56) | id` with op classes
/// 1..=7 and a debug-asserted non-zero id below `ID_MASK`, so `u64::MAX`
/// (op class 255) can never collide with one.
pub(crate) const TIMER_TOKEN: Token = Token(u64::MAX);

/// One agent's cross-thread wake route.
///
/// The entry is created by the *claim* (before any loop exists), so a thread
/// can answer "may I use turnloop?" authoritatively without paying for a loop
/// it may not use — which is the predicate `c13372cc70` had to fix after
/// `net_available()` and `ensure_loop_with` disagreed on a Worker.
struct Route {
    /// The agent this route belongs to.
    agent: AgentId,
    /// The thread that claimed it. Only this thread may own the agent's loop.
    owner: ThreadId,
    /// Identity of the loop currently behind the route, or 0 while the slot is
    /// claimed but no loop has been built. Lets a dropped loop clear only its
    /// own endpoint, and lets a profile upgrade replace it without the slot
    /// changing hands.
    loop_id: u64,
    /// True exactly while the owner is inside `Loop::turn`. Shared with the
    /// owner (which keeps a clone in its [`AgentLoop`]) so a producer can read
    /// it under the registry lock without touching the owner's TLS.
    in_turn: Arc<AtomicBool>,
    /// The loop's wake endpoint. `None` while the slot is merely claimed.
    notifier: Option<Notifier>,
    /// The loop's submission endpoint, published with `notifier` and cleared
    /// with it. `Notifier` lets another thread *wake* this agent; `Poster`
    /// lets it hand the agent *work*. That is the whole difference between a
    /// thread that must decline to tokio and one that can serve the agent it
    /// is already acting for.
    poster: Option<Poster>,
}

/// Every claimed agent route, one entry per agent. A `Vec` rather than a map:
/// the population is the number of JS agents that have asked for a loop (one,
/// in almost every program), the list is only walked on the cold wake path,
/// and a `Vec` needs no allocation to look up.
static ROUTES: Mutex<Vec<Route>> = Mutex::new(Vec::new());

/// Agent loops currently inside `Loop::turn`.
///
/// The wake producer's fast path is one atomic load of this, exactly as it was
/// one load of the single route's `in_turn` flag before P9. A process with no
/// parked loop — the common case, because the notifying thread is usually the
/// one that would be parked — never takes the registry lock.
static PARKED_LOOPS: AtomicI64 = AtomicI64::new(0);

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
        // 4096 handles meant a server refused the 2,049th connection: a
        // connection costs two handles, and the refusal was flat rather than
        // backpressure (perry#10351). The number was small because turnloop
        // used to ALLOCATE it -- `Table::new` built every slot and the whole
        // free list up front, so the ceiling was paid whether or not it was
        // used. Since turnloop 0.1.0-alpha.5 the slot tables are paged
        // (turnloop#75): a loop's idle cost is one page and is byte-identical
        // from 1K to 1M handles, so the ceiling is free and only the
        // high-water mark costs anything. 64K handles is ~32K connections.
        max_handles: 65_536,
        // NOT simply "two per connection". `max_operations` sizes two very
        // different things in turnloop: the paged `ops` table (free at any
        // ceiling) and the blocking `WorkPort`'s lock-free ring, which is
        // eagerly allocated because its capacity IS its backpressure bound.
        // Setting this to 131_072 by reflex cost 19 MB of resident memory per
        // net loop, measured, for a ring a server holding idle connections
        // never fills. 32_768 covers one armed read per connection at the
        // 65_536-handle ceiling.
        //
        // WINDOWS COSTS A THIRD THING, and it is the biggest (#10385). The
        // IOCP backend's `kernel` slab — one `OVERLAPPED`+addr+wire block per
        // operation, `sizeof` 1096 bytes — is deliberately NOT paged: mapping
        // a completion packet's pointer back to an op index is pointer
        // arithmetic over one allocation, so it must stay contiguous, and
        // `Iocp::new` builds and zeroes every slot up front. That makes the
        // ceiling cost real resident memory on Windows while it stays free on
        // epoll/kqueue. Measured here, idle HTTP server, `perry-dev`:
        //
        //     max_operations   idle working set   idle private
        //     32_768                 69.0 MB          84.7 MB
        //      2_048                 32.9 MB          48.0 MB
        //     (no net loop at all)   10.3 MB          21.6 MB
        //
        // i.e. ~36 MB of the idle footprint is this ceiling alone, matching
        // (32_768 - 2_048) * 1096 B = 33.7 MB plus the per-slot `bridges`.
        //
        // Deliberately NOT lowered on Windows. Trading the slab for a smaller
        // ceiling just reinstates the refusal perry#10351 removed — a
        // connection costs two handles and an armed read, so a low
        // `max_operations` is a connection ceiling wearing a different name.
        // The fix belongs in turnloop: the slab needs to be contiguous, not
        // committed, so reserving the address range and committing to the
        // high-water mark would keep the pointer arithmetic and drop the idle
        // cost to the paged backends' level. Tracked in the #10385 writeup.
        max_operations: 32_768,
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
    /// Every completion the driver returned, summed across ALL classes before
    /// routing — P1 net, P2 process, P3 JS timers and P4 pool together.
    ///
    /// NOT "net completions", which this comment used to claim and which an
    /// instrument then quoted: `scripts/turnloop/server_ab.py` justified its
    /// "turnloop really carried the I/O" check with those words, so a server
    /// that had declined its listener to hyper but armed a keep-alive deadline
    /// would still have shown a non-zero count and passed. Nonzero here means
    /// the loop did *something*, not that it did I/O.
    ///
    /// For a per-class answer use `turnloop_net::census`, whose
    /// `[perry-loop] p1 comp_*` line counts net completions by operation.
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
    /// The agent this loop belongs to. Carried so the stats line can name it
    /// and so teardown can clear the right route entry.
    agent: AgentId,
    profile: Profile,
    driver: Loop,
    completions: Completions,
    stats: LoopStats,
    /// This loop's half of its route's `in_turn` flag (see [`Route`]).
    in_turn: Arc<AtomicBool>,
    /// The single timer handle carrying this agent's JS timer deadline, and the
    /// deadline it currently holds.
    timer: Option<(Handle, Instant)>,
}

impl AgentLoop {
    fn new(profile: Profile, agent: AgentId, in_turn: Arc<AtomicBool>) -> turnloop::Result<Self> {
        let config = config_for(profile);
        let capacity = config.events_per_turn.max(1);
        let driver = Loop::new(config)?;
        Ok(Self {
            id: NEXT_LOOP_ID.fetch_add(1, Ordering::Relaxed),
            agent,
            profile,
            driver,
            completions: Completions::with_capacity(capacity),
            stats: LoopStats::default(),
            in_turn,
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
        // Thread exit is a teardown path too (unit-test threads, embedders),
        // and so is a profile upgrade, which drops this loop and installs a
        // replacement in the same slot. Clear the endpoint only if it is still
        // this loop's, and leave the CLAIM alone: the slot belongs to the
        // thread, not to the loop, and [`ClaimGuard`] releases it at thread
        // exit or at an explicit shutdown.
        self.in_turn.store(false, Ordering::SeqCst);
        let mut routes = ROUTES.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(route) = routes.iter_mut().find(|route| route.loop_id == self.id) {
            route.loop_id = 0;
            route.notifier = None;
            route.poster = None;
        }
        // `Loop::drop` closes the notifier, poster and native backend.
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoopState {
    /// This thread has not asked whether it may own its agent's loop.
    Unset,
    /// This thread holds its agent's route slot but has not built a loop yet.
    /// `net_available()` is true here: the slot is what makes the answer
    /// authoritative, so a submission accepted now cannot be refused later.
    Claimed,
    /// This thread owns its agent's route slot AND its loop.
    Owner,
    /// Not eligible: another thread already owns this agent's loop. Parks use
    /// the legacy path.
    ///
    /// This is the P1 coexistence rule and nothing else. It is decided by
    /// [`claim_route`] *before* any loop is built, so a thread that reaches
    /// `AgentLoop::new` has already won its agent's slot and a failure there
    /// is not a decline — it is [`loop_creation_failed`], which aborts. The
    /// only other writers are the two `claimed_flag()` arms below, which are
    /// unreachable by construction and carry a `debug_assert!` saying so.
    Declined,
    /// `shutdown_current_thread` ran; parks use the legacy path from now on.
    ShutDown,
}

/// Releases this thread's route slot when the thread goes away.
///
/// Held in TLS rather than dropped by [`AgentLoop`]: the slot is claimed
/// *before* the loop exists and must outlive a profile upgrade (which drops
/// one loop and builds another), so its lifetime is the thread's, not the
/// loop's. Without this, a program that spawns Workers in a sequence would
/// leak one `Route` per retired agent.
struct ClaimGuard {
    in_turn: Arc<AtomicBool>,
}

impl Drop for ClaimGuard {
    fn drop(&mut self) {
        // A parked owner cannot be dropping its own claim, so the flag can only
        // be false here; clearing it is belt-and-braces against a wake that
        // races the teardown and finds a stale `true` with no notifier.
        self.in_turn.store(false, Ordering::SeqCst);
        let flag = &self.in_turn;
        let mut routes = ROUTES.lock().unwrap_or_else(PoisonError::into_inner);
        routes.retain(|route| !Arc::ptr_eq(&route.in_turn, flag));
    }
}

crate::perry_thread_local! {
    static STATE: Cell<LoopState> = const { Cell::new(LoopState::Unset) };
    static AGENT_LOOP: RefCell<Option<AgentLoop>> = const { RefCell::new(None) };
    /// This thread's route slot, from the claim to thread exit. See
    /// [`ClaimGuard`].
    static CLAIM: RefCell<Option<ClaimGuard>> = const { RefCell::new(None) };
    /// Completions moved out of the driver by [`AgentLoop::record`] and not
    /// yet routed. Owned by this thread, drained in FIFO order by
    /// [`dispatch_staged`] once no borrow on `AGENT_LOOP` is held.
    static STAGED: RefCell<Vec<turnloop::Completion>> = const { RefCell::new(Vec::new()) };
}

/// Take this agent's route slot for this thread, without building a loop.
///
/// This is the whole admission decision, and it is taken **once per thread**:
/// after it, [`eligible`] and [`net_available`] are a single TLS read. Exactly
/// one thread owns an agent's loop — the first to ask, which is the thread
/// running that agent's event loop — and a second thread acting for the same
/// agent keeps the legacy park it has today (Android's UI thread pumping for
/// `perry-native`, an embedder's host thread).
///
/// Answering here rather than from agent identity is what keeps
/// `net_available()` and `ensure_loop_with()` in lockstep. They disagreed once
/// (`c13372cc70`): a `worker_threads` Worker reported `PRIMARY_AGENT`, the
/// submit guard accepted its `fetch()`, and `ensure_loop_with` refused a moment
/// later — so the request failed *after acceptance* instead of taking the
/// fallback. A claimed slot cannot be taken away, so that class is gone.
fn claim_route() -> bool {
    match STATE.with(Cell::get) {
        LoopState::Claimed | LoopState::Owner => return true,
        LoopState::Declined | LoopState::ShutDown => return false,
        LoopState::Unset => {}
    }
    let agent = crate::agent::current_agent();
    let me = std::thread::current().id();
    let flag = {
        let mut routes = ROUTES.lock().unwrap_or_else(PoisonError::into_inner);
        match routes.iter().find(|route| route.agent == agent) {
            // Someone already speaks for this agent. If it is us the slot is
            // reusable (a shutdown that left the claim behind); if not, decline
            // for the life of this thread.
            Some(route) if route.owner == me => route.in_turn.clone(),
            Some(_) => {
                drop(routes);
                STATE.with(|s| s.set(LoopState::Declined));
                return false;
            }
            None => {
                let flag = Arc::new(AtomicBool::new(false));
                routes.push(Route {
                    agent,
                    owner: me,
                    loop_id: 0,
                    in_turn: flag.clone(),
                    notifier: None,
                    poster: None,
                });
                flag
            }
        }
    };
    CLAIM.with(|slot| {
        *slot.borrow_mut() = Some(ClaimGuard {
            in_turn: flag.clone(),
        })
    });
    STATE.with(|s| s.set(LoopState::Claimed));
    true
}

/// This thread's claimed `in_turn` flag, if it holds a slot.
fn claimed_flag() -> Option<Arc<AtomicBool>> {
    CLAIM.with(|slot| slot.borrow().as_ref().map(|c| c.in_turn.clone()))
}

/// Give up this thread's route slot at an explicit shutdown.
fn release_route() {
    CLAIM.with(|slot| *slot.borrow_mut() = None);
}

/// Publish a freshly built loop's wake endpoint into its route slot.
fn publish_route(agent: &AgentLoop) {
    let mut routes = ROUTES.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(route) = routes
        .iter_mut()
        .find(|route| Arc::ptr_eq(&route.in_turn, &agent.in_turn))
    {
        route.loop_id = agent.id;
        route.notifier = Some(agent.driver.notifier());
        route.poster = Some(agent.driver.poster());
    }
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
        // One router, five token spaces. P1's classes are 1..=7, P2's are
        // 0x10..=0x1F, P4's are 0x20..=0x2F, P10's posted host jobs are
        // 0x30..=0x3F and P3 owns TIMER_TOKEN above, so `owns` is a range test
        // and no module can be handed another's completion (`turnloop_proc`'s
        // module note). P1 is the fall-through, so every other class must be
        // branched on here or its completions land in the net subsystem.
        if crate::turnloop_proc::owns(completion.token) {
            crate::turnloop_proc::dispatch(completion);
        } else if crate::turnloop_pool::owns(completion.token) {
            crate::turnloop_pool::dispatch(completion);
        } else if crate::turnloop_post::owns(completion.token) {
            crate::turnloop_post::dispatch(completion);
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
/// slot is claimed; the claim itself is taken once, on the first ask.
#[inline]
pub(super) fn eligible() -> bool {
    match STATE.with(Cell::get) {
        LoopState::Claimed | LoopState::Owner => true,
        LoopState::Declined | LoopState::ShutDown => false,
        LoopState::Unset => claim_route(),
    }
}

/// `turnloop::BACKEND_NAME == name`, answerable in const context.
const fn backend_name_is(name: &[u8]) -> bool {
    let actual = turnloop::BACKEND_NAME.as_bytes();
    if actual.len() != name.len() {
        return false;
    }
    let mut i = 0;
    while i < actual.len() {
        if actual[i] != name[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Compile-time proof that this build HAS a turnloop backend. That is the
/// whole platform gate behind [`loop_creation_failed`] being fatal.
///
/// turnloop ships no fallback backend: `turnloop::Loop` is
/// `Driver<backend::Platform>`, and `backend::Platform` exists only under
/// `turnloop_backend = kqueue | epoll | iocp | wasi_p2 | wasi_p3 | web`
/// (turnloop's `build.rs` maps every other target to `"unsupported"`). A host
/// turnloop cannot serve therefore fails to COMPILE here — it never reaches
/// `Loop::new` to fail at run time. So "an unsupported host" is not a runtime
/// cause in any binary that exists, and making the failure fatal needs no
/// `cfg` arm keeping a legacy park for one.
///
/// HarmonyOS is worth naming because it looks like the exception and is not.
/// Perry builds it as `{aarch64,x86_64}-unknown-linux-ohos`, whose rustc cfg is
/// `target_os = "linux"` + `target_env = "ohos"` — which is why every HarmonyOS
/// `cfg` in this crate spells `target_env = "ohos"` — so turnloop's `build.rs`
/// selects the epoll backend there exactly as for any other Linux target.
///
/// What this assertion does NOT say is that `Loop::new` cannot fail on ohos.
/// It can, for the same reasons it can on any Linux: `Epoll::new` opens an
/// epoll fd and an eventfd, and probes `epoll_pwait2`, treating only ENOSYS as
/// "old kernel". A sandbox that answers EPERM instead fails every call, on
/// every device. That is a real environment fault and belongs in
/// [`loop_creation_failed`]'s message (it names it, and the errno tells it
/// apart from a descriptor ceiling) — but it is not a missing backend, so it
/// is not a reason to keep a silent legacy fallback.
///
/// If turnloop ever gains a no-op backend for unsupported hosts, this stops
/// holding. The assertion then fails the *build* on the affected target
/// instead of letting a user's program abort at run time, and the legacy
/// fallback should be restored here under a `cfg` as a deliberate choice. Note
/// where that lands: no CI job cross-compiles this crate for `*-linux-ohos` —
/// `harmonyos-smoke` only runs `perry-codegen-arkts` host tests — so the ohos
/// build that would trip it is the one `perry compile --target harmonyos`
/// drives (`perry/src/commands/compile/optimized_libs/driver.rs`), on the
/// machine of whoever is packaging the app.
const _: () = assert!(
    !backend_name_is(b"unsupported"),
    "turnloop reports no backend for this target: perry-runtime must keep the legacy park here \
     rather than let loop_creation_failed abort a user's program"
);

/// `turnloop::Loop::new` failed on a thread that had already won its agent's
/// route. Fatal, deliberately.
///
/// This used to set [`LoopState::Declined`] and keep the legacy tokio park.
/// But `STATE` is `perry_thread_local!` and neither [`net_available`] nor
/// [`eligible`] ever retries a decline, so ONE transient failure pinned that
/// thread to the legacy transport for the rest of its life — an fd-ceiling bug
/// presenting as an unexplained throughput and RSS regression on a single
/// thread, and only under `PERRY_LOOP_STATS`, which nobody sets in production.
/// Nor can a caller recover: every caller's fallback *is* that degradation.
///
/// `abort` rather than a panic or a warning. perry-runtime ships
/// `panic = "abort"` but is built `panic = "unwind"` under `cargo test`, and a
/// panic on a `perry/thread` or `worker_threads` agent kills only that thread —
/// so a panic is swallowable exactly where this bug lives. A printed warning
/// that lets the program continue is the silent degradation with extra output.
#[cold]
#[inline(never)]
fn loop_creation_failed(profile: Profile, agent: AgentId, error: turnloop::Error) -> ! {
    eprintln!(
        "[PERRY ABORT] turnloop Loop::new failed for agent {agent} at the {profile:?} profile: \
         {error} (kind={:?} os_error={:?} backend={}). Perry's event loop cannot be created on \
         this thread, and `os_error` above is what tells the causes apart. (1) FILE DESCRIPTOR \
         EXHAUSTION — EMFILE (24) or ENFILE (23). Every agent loop needs a kqueue/epoll/IOCP \
         descriptor of its own, plus an eventfd on epoll, so a process that has run out cannot \
         open another; raise the limit (`ulimit -n`, or `LimitNOFILE=` in a systemd unit) and \
         re-run. (2) A SANDBOX DENYING A SYSCALL — EPERM (1) or EACCES (13). The epoll backend \
         probes `epoll_pwait2` at construction and only treats ENOSYS as 'old kernel, use \
         timerfd'; a seccomp filter that answers EPERM instead makes this fail on every attempt, \
         deterministically. Relevant on sandboxed Linux/Android/HarmonyOS app processes: check the \
         policy for epoll_pwait2, eventfd2 and timerfd_create. (3) AN UNSUPPORTED HOST — \
         `backend=unsupported` above would say so; perry-runtime does not compile in that state, \
         so it cannot be this unless turnloop has gained a no-op backend. Perry used to degrade \
         this thread to the legacy tokio park instead, which turned every one of these into an \
         invisible per-thread throughput and memory regression; it is fatal now.",
        error.kind,
        error.os,
        turnloop::BACKEND_NAME,
    );
    std::process::abort()
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
        LoopState::Claimed => {}
        LoopState::Unset => {
            if !claim_route() {
                return false;
            }
        }
    }
    let Some(in_turn) = claimed_flag() else {
        // Unreachable: `LoopState::Claimed` and a missing claim cannot coexist.
        // Decline rather than assert, so a future refactor degrades to the
        // legacy park instead of aborting a user's program.
        debug_assert!(false, "claimed state without a claim guard");
        STATE.with(|s| s.set(LoopState::Declined));
        return false;
    };
    let id = crate::agent::current_agent();
    let agent = match AgentLoop::new(profile, id, in_turn) {
        Ok(agent) => agent,
        // Descriptor exhaustion, or a sandbox refusing one of the backend's
        // syscalls. Fatal: see `loop_creation_failed` for why a silent fall
        // back to the legacy park is worse than stopping.
        Err(error) => loop_creation_failed(profile, id, error),
    };
    publish_route(&agent);
    AGENT_LOOP.with(|slot| *slot.borrow_mut() = Some(agent));
    STATE.with(|s| s.set(LoopState::Owner));
    true
}

/// Rebuild this thread's loop at a larger profile, if it is not there yet.
///
/// The rebuild itself cannot fail softly: it drops the old loop first, so a
/// failure would leave the thread with no loop at all, and that is exactly the
/// silent degradation [`loop_creation_failed`] now aborts on. The only `false`
/// left is the unreachable missing-claim arm below.
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
    // P4: the same rule for jobs, which have no handle. A recreated loop takes
    // its blocking-pool `WorkPort` with it, so a job outstanding across an
    // upgrade would complete into a closed port and never be delivered. The
    // pool submits at the net profile precisely so this cannot happen
    // (`event_pump::with_pool_driver`); the assertion is what keeps that true.
    debug_assert_eq!(
        crate::turnloop_pool::outstanding(),
        0,
        "the loop profile is upgraded before the first pool job, never under one"
    );
    let previous = AGENT_LOOP.with(|slot| slot.borrow_mut().take());
    let carried = previous.as_ref().map(|agent| agent.stats);
    let owner = previous.as_ref().map(|agent| agent.agent);
    drop(previous);
    let Some(in_turn) = claimed_flag() else {
        debug_assert!(false, "an owned loop without a claim guard");
        STATE.with(|s| s.set(LoopState::Declined));
        return false;
    };
    // `AgentLoop::drop` cleared the endpoint but kept the slot; install the
    // replacement's into the same slot.
    let id = owner.unwrap_or_else(crate::agent::current_agent);
    let mut agent = match AgentLoop::new(profile, id, in_turn) {
        Ok(agent) => agent,
        Err(error) => loop_creation_failed(profile, id, error),
    };
    if let Some(stats) = carried {
        agent.stats = stats;
    }
    publish_route(&agent);
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

/// Give the calling thread a loop at `profile` WITHOUT taking a route slot, so
/// a test that only exercises turns and completions cannot race another test
/// thread for its agent's route.
#[cfg(test)]
pub(super) fn install_unrouted_for_test(profile: Profile) -> bool {
    let in_turn = Arc::new(AtomicBool::new(false));
    match AgentLoop::new(profile, crate::agent::current_agent(), in_turn) {
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
    crate::turnloop_pool::reset_for_test();
    AGENT_LOOP.with(|slot| *slot.borrow_mut() = None);
    STAGED.with(|staged| staged.borrow_mut().clear());
    release_route();
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

/// Whether this thread can own its agent's loop at all.
///
/// Answers without creating one — a caller asking "may I use turnloop?" must
/// not pay for a loop it may not use — but **authoritatively**: a `true` here
/// means the route slot is this thread's, so the `ensure_loop_with` that
/// follows the submission cannot refuse for want of ownership. That lockstep
/// is the whole point; see [`claim_route`] for the bug that taught it.
///
/// It no longer mentions `PRIMARY_AGENT`. Every JS agent may have a loop, so
/// the question is "do I have (or may I take) one", which is true on every
/// thread that runs an agent's event loop.
pub(super) fn net_available() -> bool {
    match STATE.with(Cell::get) {
        LoopState::Claimed | LoopState::Owner => true,
        LoopState::Declined | LoopState::ShutDown => false,
        LoopState::Unset => claim_route(),
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
        // Publish "parked" BEFORE re-reading the work flags, and in this
        // order: the per-route flag first (it is what a producer reads to pick
        // a target), then the global count (it is what a producer reads to
        // decide whether to look at all). A producer stores its work and then
        // loads the count, both `SeqCst`, so in the single total order either
        // it sees this increment — and then also the flag, which precedes it —
        // or this thread's load below sees the producer's store. No lost wake.
        agent.in_turn.store(true, Ordering::SeqCst);
        PARKED_LOOPS.fetch_add(1, Ordering::SeqCst);
        if super::NOTIFIED.load(Ordering::SeqCst) || super::precise_wait::native_inflight() {
            // A notify landed after the fast path (leave the flag for the next
            // `js_wait_for_event` fast path to consume), or tokio-owned native
            // work appeared after the caller chose this wait
            // (`js_native_work_submitted`). Either way, go back around the loop.
            agent.in_turn.store(false, Ordering::SeqCst);
            PARKED_LOOPS.fetch_sub(1, Ordering::SeqCst);
            return Park::Notified;
        }
        let started = super::loop_stats::begin_wait(super::loop_stats::WaitKind::Turnloop);
        let result = agent
            .driver
            .turn(Timeout::Until(deadline), &mut agent.completions);
        super::loop_stats::end_wait(super::loop_stats::WaitKind::Turnloop, started);
        agent.in_turn.store(false, Ordering::SeqCst);
        PARKED_LOOPS.fetch_sub(1, Ordering::SeqCst);
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

/// Wake every agent loop currently inside a turn. `js_notify_main_thread`
/// calls this after storing `NOTIFIED`; `js_native_work_submitted` after new
/// tokio-owned work became visible to the in-flight predicate.
///
/// A broadcast, deliberately. The two things it mirrors are both broadcasts:
/// `NOTIFIED` is one process-global flag every JS thread consumes, and the
/// legacy park's `PUMP.cvar` is signalled for whoever is waiting on it. Before
/// P9 only the primary agent could be inside a turn, so "wake the route" and
/// "wake everyone parked" were the same thing; now a Worker parks in its own
/// turn instead of on that condvar, and a point-to-point wake addressed to the
/// primary agent would leave it asleep on a `postMessage`-driven resolution —
/// P6's failure mode, a hang rather than an error.
///
/// The fast path is one atomic load, exactly as it was before. Only agents
/// actually inside a turn are poked, so an idle agent costs nothing and a
/// program with one agent behaves identically.
#[inline]
pub(super) fn wake_parked_agents() {
    if PARKED_LOOPS.load(Ordering::SeqCst) > 0 {
        wake_parked_agents_slow();
    }
}

#[cold]
fn wake_parked_agents_slow() {
    let routes = ROUTES.lock().unwrap_or_else(PoisonError::into_inner);
    for route in routes.iter() {
        if !route.in_turn.load(Ordering::SeqCst) {
            continue;
        }
        if let Some(notifier) = route.notifier.as_ref() {
            // Err means that loop is closing: there is no waiter left to wake.
            let _ = notifier.notify();
        }
    }
}

/// Why a post to another thread's agent loop could not be delivered.
///
/// Deliberately distinct from "declined": a caller that cannot post needs to
/// know *why*, because the answers differ. No route at all means this agent has
/// no loop and the caller must use its own fallback; a closed or full loop is a
/// transient condition on a loop that does exist.
#[derive(Debug)]
pub enum PostToAgentError {
    /// No thread has claimed a loop for this agent, so there is nothing to post
    /// to. The caller's own fallback is the correct answer here.
    NoRoute,
    /// The agent has a loop, but its slot is claimed and the loop is not built
    /// yet. Transient: the owner is between `claim_route` and `publish_route`.
    NotPublished,
    /// The loop refused the post. `payload` is returned when the caller may
    /// retry; `None` means the post was accepted and must not be retried (a
    /// wake error after enqueue), per `Poster::post`'s contract.
    Refused { payload: Option<Payload> },
}

/// Hand work to the loop of an agent **another thread owns**.
///
/// This is what a host pump thread needs. Since P9 every JS agent has its own
/// loop, but a second thread may still act *for* an agent another thread owns —
/// a host pump, Android's UI thread for `perry-native`. Such a thread cannot
/// own the loop, and until now its only option was to decline to tokio, which
/// is why the tokio implementations are still live code.
///
/// Posting is sound precisely because both threads serve the **same agent's
/// heap**: the completion is delivered on the owner, which is where that
/// agent's JS values live. This is not the rejected "route completions between
/// agents" idea — nothing crosses an agent boundary.
///
/// The owner is woken by `Poster::post` itself, so a parked loop picks the work
/// up without a separate `notify`.
///
/// `pub` because the callers are in other crates — perry#10395 step 2 converts
/// the declining bindings one at a time, and the ext crates reach this through
/// perry-stdlib or a `turnloop_net::abi` export rather than from inside here.
pub fn post_to_agent(
    agent: AgentId,
    token: Token,
    payload: Payload,
) -> Result<(), PostToAgentError> {
    let poster = {
        let routes = ROUTES.lock().unwrap_or_else(PoisonError::into_inner);
        match routes.iter().find(|route| route.agent == agent) {
            None => return Err(PostToAgentError::NoRoute),
            Some(route) => match route.poster.as_ref() {
                None => return Err(PostToAgentError::NotPublished),
                // Cloned out so the post happens without the registry lock
                // held: `post` can wake the owner, and waking under this lock
                // would put a cross-thread wake inside a mutex every producer
                // takes.
                Some(poster) => poster.clone(),
            },
        }
    };
    poster
        .post(token, payload)
        .map_err(|err| PostToAgentError::Refused {
            payload: err.payload,
        })
}

/// Whether a post to `agent` would reach a loop, asked without building a job.
///
/// A binding has to decide which transport a connection lives on *before* it
/// has any work to post, and [`post_to_agent`] consumes its payload on the way
/// in — so "would this land?" cannot be answered by trying. True means a route
/// exists **and its loop is published**, i.e. exactly the two cases
/// [`PostToAgentError::NoRoute`] and [`PostToAgentError::NotPublished`] rule
/// out; a post can still be refused afterwards by a full postbox, which is
/// transient and which the caller retries or falls back on.
///
/// Deliberately says nothing about whether the *calling* thread owns that loop.
/// A caller that owns it should submit directly instead of posting to itself,
/// and it already knows that from `net_available()`.
pub fn has_route(agent: AgentId) -> bool {
    ROUTES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .any(|route| route.agent == agent && route.poster.is_some())
}

/// How many agents hold a route slot. A leak check for tests: a program that
/// spawns and retires Workers must not grow this.
#[cfg(test)]
pub(super) fn routed_agents() -> usize {
    ROUTES.lock().unwrap_or_else(PoisonError::into_inner).len()
}

/// This thread's loop counters, if it owns a loop.
pub fn loop_statistics() -> Option<LoopStats> {
    AGENT_LOOP.with(|slot| slot.borrow().as_ref().map(|agent| agent.stats))
}

/// Destroy this thread's loop at the process-exit funnel — or, for a worker
/// agent, at [`crate::agent::retire_agent`] — and print the
/// `PERRY_LOOP_STATS=1` line once. Idempotent; later parks use the legacy path.
///
/// Running this on a worker agent is not optional. The settle sequence below
/// is the only thing that turns an outstanding operation into a completion the
/// binding can see, and P5/P6/P7's engines learn about teardown *only* through
/// those completions. Skipping it on a worker would strand every promise those
/// engines owe — which presents as a hang, not an error.
pub fn shutdown_current_thread() {
    if STATE.with(Cell::get) == LoopState::Owner {
        // Close P1's sockets while the loop is still here, then run one
        // nonblocking turn so their `Closed` completions reach the binding
        // (exactly-once release, DESIGN D4). `Loop::drop` would free the
        // descriptors either way; this is what lets a binding's own
        // bookkeeping see the close rather than inferring it from teardown.
        crate::turnloop_net::shutdown_current_thread();
        crate::turnloop_proc::shutdown_current_thread();
        // P4: settle every outstanding job before the loop goes away, so a job
        // the pool is still running cannot complete into a closed port and
        // silently skip its delivery (DESIGN D4).
        crate::turnloop_pool::shutdown_current_thread();
        fast_turn();
    }
    let previous = STATE.with(|s| s.replace(LoopState::ShutDown));
    if previous == LoopState::ShutDown {
        return;
    }
    let agent = AGENT_LOOP.with(|slot| slot.borrow_mut().take());
    STAGED.with(|staged| staged.borrow_mut().clear());
    let id = agent
        .as_ref()
        .map(|agent| agent.agent)
        .unwrap_or_else(crate::agent::current_agent);
    if stats_enabled() {
        match (&agent, previous) {
            (Some(agent), _) => print_stats(id, agent.stats),
            // Since loop-creation failure aborts, `Declined` can only mean the
            // P1 coexistence rule: another thread owns this agent's loop and
            // this one pumped on the legacy path all along. That is normal.
            (None, LoopState::Declined) => eprintln!("[perry-loop] driver=legacy agent={id}"),
            // A worker agent that never parked and never submitted is the
            // ordinary case for `parallelMap` over 64 cores. Saying so once per
            // core would bury the primary agent's line, so stay quiet unless
            // this thread actually reached the driver.
            (None, _) if id != crate::agent::PRIMARY_AGENT => {}
            (None, _) => eprintln!("[perry-loop] driver=turnloop parked=0 agent={id}"),
        }
    }
    drop(agent);
    // After the loop, so a wake that races teardown finds the endpoint gone
    // rather than the slot gone and the endpoint live.
    release_route();
}

fn stats_enabled() -> bool {
    super::loop_stats::enabled()
}

/// A `PERRY_LOOP_STATS` line produced by a crate that depends on this one.
///
/// perry-stdlib owns P6's outbound-client counters and depends on perry-runtime
/// rather than the other way round, so it installs a reporter here instead of
/// this module reaching into it. One slot: a second consumer adds its own.
pub type StatsReporter = extern "C" fn();

static EXTRA_STATS: std::sync::atomic::AtomicPtr<()> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

/// Install the extra reporter. Idempotent for the same pointer.
pub fn register_stats_reporter(reporter: StatsReporter) {
    EXTRA_STATS.store(reporter as *mut (), std::sync::atomic::Ordering::Release);
}

fn print_extra_stats() {
    let p = EXTRA_STATS.load(std::sync::atomic::Ordering::Acquire);
    if p.is_null() {
        return;
    }
    // SAFETY: the slot only ever holds a `StatsReporter` stored above.
    let f: StatsReporter = unsafe { std::mem::transmute(p) };
    f();
}

fn print_stats(id: AgentId, stats: LoopStats) {
    // `agent=` is APPENDED, never inserted. Two instruments parse this line
    // positionally — `scripts/turnloop/server_ab.py` matches the literal
    // prefix `[perry-loop] driver=turnloop` as its arm marker, and
    // `scripts/turnloop_p0_loop_stats.py` has a regex anchored on
    // `driver=turnloop turns=… turn_errors=…` — so a new field in the middle
    // would make the A/B harness reject every sample as "wrong arm", which is
    // exactly the check that stops it comparing a tree against itself.
    eprintln!(
        "[perry-loop] driver=turnloop turns={} os_waits={} zero_event_waits={} native_ticks={} turn_errors={} completions={} timer_arms={} timer_expiries={} agent={id}",
        stats.turns,
        stats.os_waits,
        stats.zero_event_waits,
        stats.native_ticks,
        stats.turn_errors,
        stats.completions,
        stats.timer_arms,
        stats.timer_expiries
    );
    // Everything below is a PROCESS-wide lifetime total, not this agent's, so
    // it is printed once — by the primary agent, whose shutdown is the
    // process-exit funnel and therefore the last one to run. A worker agent
    // retiring mid-program would otherwise print a partial copy of each.
    if id != crate::agent::PRIMARY_AGENT {
        return;
    }
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
    // P4's own "the subject ran" line. `completions` above cannot distinguish a
    // socket P1 carried from a blocking job P4 carried, and a program that ran
    // one bcrypt hash exits with every live count at zero — so the lifetime
    // totals are what an A/B or an acceptance test reads to know the work
    // really left the JS thread. `refused` is separate on purpose: a refused
    // submission ran the caller's own fallback, so a nonzero value means the
    // pool was NOT the transport for that work.
    eprintln!(
        "[perry-loop] p4 pool_submitted={} completed={} cancelled={} failed={} refused={}",
        crate::turnloop_pool::submitted_total(),
        crate::turnloop_pool::completed_total(),
        crate::turnloop_pool::cancelled_total(),
        crate::turnloop_pool::failed_total(),
        crate::turnloop_pool::refused_total(),
    );
    // P6's own line, when perry-stdlib is linked and its client engine
    // registered one (see `register_stats_reporter`).
    print_extra_stats();
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
