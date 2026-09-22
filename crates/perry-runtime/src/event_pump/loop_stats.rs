//! `PERRY_LOOP_STATS=1` wait metrics: where the primary agent's event loop
//! spends its waits.
//!
//! Instruction counts and RSS can stay flat while the waits between Perry and
//! tokio decide a server's latency and CPU. This module makes them directly
//! observable:
//!
//! * per wait kind (a turnloop turn, a tokio tick, a condvar park): count,
//!   total and maximum time parked, from the monotonic clock around the wait;
//! * fast drives (the stdlib's brief tokio drive on the notified path, reported
//!   only when it actually drove): count, total and maximum time;
//! * wake latency: time from a producer's notify (a cross-thread producer, or a
//!   native completion queued inside a tokio tick) to the parked wait
//!   returning, as a five-bucket histogram plus the maximum;
//! * zero-budget returns and #1114 spin-throttle sleeps.
//!
//! Diagnostic only, never a behaviour knob. When the variable is unset every
//! hook costs one relaxed load of [`STATE`]; when set, the hooks use atomics
//! only — no allocation and no lock. Waits are recorded for the primary agent
//! only, so a worker's legacy park cannot blur the A/B comparison.
//!
//! Wake-latency protocol: the waiter clears [`NOTIFY_AT_NS`] and then publishes
//! [`PARKED`] before the wait; a producer that sees `PARKED` stores its
//! timestamp if none is stored yet (the earliest notify wins); the waiter
//! clears `PARKED` after the wait and takes the timestamp. One notify into one
//! parked wait yields exactly one sample; a notify that lands outside a wait
//! yields none. Every producer is covered, because they all fan out through
//! [`super::js_notify_main_thread`]: a cross-thread producer (a blocking-pool
//! job, a `worker_threads` Worker, a child-process reactor) and an in-thread
//! native completion alike (`perry_ffi::notify_main_thread` from ext-http /
//! ext-net / ext-ws, and the stdlib's own resolution sites).
//!
//! Residual window, deliberately not closed: a producer that publishes its
//! notify between the waiter's last `NOTIFIED` re-check and its `PARKED` store
//! records no sample. The waiter is then woken by the ordinary wake path and
//! the wait is still counted, so a *missing* sample never inflates the
//! histogram — it only, very rarely, omits one. Closing it would need the
//! stamp inside the same critical section as the park, which is a behaviour
//! change for a diagnostic.
//!
//! The direction that would matter — a stamp from wait *N* landing on wait
//! *N+1*, where the latency is computed from before that wait began — **is**
//! closed: [`end_wait`] rejects any stamp older than the wait it is ending. The
//! bias is therefore one-sided by construction: this module can under-report
//! wakes, never invent or inflate one.

use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

const UNKNOWN: u8 = 0;
const OFF: u8 = 1;
const ON: u8 = 2;

/// Lazily resolved from `PERRY_LOOP_STATS` (UNKNOWN until first use).
static STATE: AtomicU8 = AtomicU8::new(UNKNOWN);

/// The kind of wait the primary agent is currently parked in (0 = none).
static PARKED: AtomicU8 = AtomicU8::new(0);
per_test_global! {
    /// Monotonic ns of the earliest notify into the current wait (0 = none).
    ///
    /// `per_test_global!` because TEST code reads it (#10944); its siblings
    /// above are not read from tests, which is why they stay bare. The wake
    /// protocol is inherently cross-thread — one thread parks and clears this,
    /// another notifies and sets it — so a test that spawns either side must
    /// share the instance via [`test_shared_wake_key`] / [`test_adopt_wake`].
    /// Outside a test build the macro is the plain `static`, byte for byte.
    static NOTIFY_AT_NS: AtomicU64 = AtomicU64::new(0);
}

/// This thread's `NOTIFY_AT_NS` instance, as an opaque key for
/// [`test_adopt_wake`] on a thread this test spawns.
#[cfg(test)]
pub(crate) fn test_shared_wake_key() -> usize {
    NOTIFY_AT_NS.shared_key()
}

/// Adopt the wake-latency instance from [`test_shared_wake_key`]. Must run
/// before this thread's first park or notify (see `PerThread::adopt`).
#[cfg(test)]
pub(crate) fn test_adopt_wake(key: usize) {
    NOTIFY_AT_NS.adopt(key);
}

/// A kind of parked wait.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitKind {
    /// One `turnloop::Loop::turn` on an `Instant` deadline.
    Turnloop = 1,
    /// One registered tokio tick (`stdlib_wait_driver` → `run_one_tick`).
    TokioTick = 2,
    /// One condvar park (runtime-only binaries in the legacy arm, declined
    /// threads, turn-failure fallback).
    Condvar = 3,
}

struct Kind {
    count: AtomicU64,
    total_ns: AtomicU64,
    max_ns: AtomicU64,
}

impl Kind {
    const fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            total_ns: AtomicU64::new(0),
            max_ns: AtomicU64::new(0),
        }
    }
    fn add(&self, ns: u64) {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.total_ns.fetch_add(ns, Ordering::Relaxed);
        self.max_ns.fetch_max(ns, Ordering::Relaxed);
    }
    fn snapshot(&self) -> KindStats {
        KindStats {
            count: self.count.load(Ordering::Relaxed),
            total_ns: self.total_ns.load(Ordering::Relaxed),
            max_ns: self.max_ns.load(Ordering::Relaxed),
        }
    }
}

static TURNLOOP: Kind = Kind::new();
static TOKIO_TICK: Kind = Kind::new();
static CONDVAR: Kind = Kind::new();
static FAST_DRIVE: Kind = Kind::new();
static ZERO_BUDGET: AtomicU64 = AtomicU64::new(0);
static THROTTLE_SLEEPS: AtomicU64 = AtomicU64::new(0);

/// Upper bounds (exclusive) of the wake-latency buckets; the last is open.
pub const WAKE_BUCKET_BOUNDS_NS: [u64; 4] = [50_000, 200_000, 1_000_000, 5_000_000];
static WAKE_BUCKETS: [AtomicU64; 5] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];
static WAKE_MAX_NS: AtomicU64 = AtomicU64::new(0);

/// Count, total and maximum of one measured quantity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KindStats {
    pub count: u64,
    pub total_ns: u64,
    pub max_ns: u64,
}

/// Snapshot of every wait metric.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoopWaitStats {
    pub turnloop: KindStats,
    pub tokio_tick: KindStats,
    pub condvar: KindStats,
    pub fast_drive: KindStats,
    pub zero_budget: u64,
    pub throttle_sleeps: u64,
    /// Wake-latency histogram: `<50µs, <200µs, <1ms, <5ms, ≥5ms`.
    pub wake_buckets: [u64; 5],
    pub wake_max_ns: u64,
}

impl LoopWaitStats {
    pub fn wake_samples(&self) -> u64 {
        self.wake_buckets.iter().sum()
    }
}

/// Whether `PERRY_LOOP_STATS=1`. One relaxed load once resolved.
#[inline]
pub fn enabled() -> bool {
    match STATE.load(Ordering::Relaxed) {
        ON => true,
        OFF => false,
        _ => resolve(),
    }
}

#[cold]
fn resolve() -> bool {
    let on = std::env::var_os("PERRY_LOOP_STATS").is_some_and(|v| v == "1");
    STATE.store(if on { ON } else { OFF }, Ordering::Relaxed);
    on
}

/// Test hook: turn recording on regardless of the environment.
///
/// Public because perry-stdlib's tests drive the real tokio tick through this
/// module, and a test cannot rely on `PERRY_LOOP_STATS` having been in the
/// environment before the first [`enabled`] call resolved [`STATE`]. It turns a
/// diagnostic on and nothing else; no production caller exists.
#[doc(hidden)]
pub fn enable_for_tests() {
    STATE.store(ON, Ordering::Relaxed);
}

#[cfg(test)]
pub(crate) fn force_enable_for_test() {
    enable_for_tests();
}

/// Monotonic nanoseconds since the first stats clock read, never 0.
fn now_ns() -> u64 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    let epoch = *EPOCH.get_or_init(Instant::now);
    (epoch.elapsed().as_nanos() as u64).saturating_add(1)
}

#[inline]
fn recording_thread() -> bool {
    crate::agent::current_agent() == crate::agent::PRIMARY_AGENT
}

/// Start timing a wait. `None` when stats are off or this is not the primary
/// agent; pass the result to [`end_wait`].
#[inline]
pub fn begin_wait(kind: WaitKind) -> Option<u64> {
    if !enabled() || !recording_thread() {
        return None;
    }
    Some(begin_wait_recorded(kind))
}

fn begin_wait_recorded(kind: WaitKind) -> u64 {
    // Clear the stamp BEFORE publishing `PARKED`: a producer that observes
    // `PARKED` can only be stamping for this wait, never for the previous one.
    NOTIFY_AT_NS.store(0, Ordering::SeqCst);
    PARKED.store(kind as u8, Ordering::SeqCst);
    now_ns()
}

/// Finish timing a wait started by [`begin_wait`].
#[inline]
pub fn end_wait(kind: WaitKind, started: Option<u64>) {
    if let Some(started) = started {
        end_wait_recorded(kind, started);
    }
}

fn end_wait_recorded(kind: WaitKind, started: u64) {
    let now = now_ns();
    PARKED.store(0, Ordering::SeqCst);
    let notified_at = NOTIFY_AT_NS.swap(0, Ordering::SeqCst);
    let slot = match kind {
        WaitKind::Turnloop => &TURNLOOP,
        WaitKind::TokioTick => &TOKIO_TICK,
        WaitKind::Condvar => &CONDVAR,
    };
    slot.add(now.saturating_sub(started));
    // `notified_at >= started` rejects a stamp that belongs to an EARLIER wait:
    // a producer preempted between reading its clock and its compare-exchange
    // can land that stamp on the next wait, where the latency would be computed
    // from a moment before the wait even began. A rejected stamp costs one
    // sample; an accepted stale one would invent a multi-millisecond wake.
    // (`started` is never 0, so this also covers "no stamp".)
    if notified_at >= started {
        record_wake_latency(now.saturating_sub(notified_at));
    }
}

/// Producer side: `js_notify_main_thread` calls this after publishing its
/// notify and before waking the waiter.
#[inline]
pub fn note_notify() {
    if enabled() && PARKED.load(Ordering::SeqCst) != 0 {
        let _ = NOTIFY_AT_NS.compare_exchange(0, now_ns(), Ordering::SeqCst, Ordering::SeqCst);
    }
}

pub(crate) fn record_wake_latency(ns: u64) {
    let bucket = WAKE_BUCKET_BOUNDS_NS
        .iter()
        .position(|&bound| ns < bound)
        .unwrap_or(WAKE_BUCKET_BOUNDS_NS.len());
    WAKE_BUCKETS[bucket].fetch_add(1, Ordering::Relaxed);
    WAKE_MAX_NS.fetch_max(ns, Ordering::Relaxed);
}

/// A zero-budget return (a deadline was already due).
#[inline]
pub fn note_zero_budget(throttled: bool) {
    if enabled() && recording_thread() {
        ZERO_BUDGET.fetch_add(1, Ordering::Relaxed);
        if throttled {
            THROTTLE_SLEEPS.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// perry-stdlib: a fast drive is about to run tokio. Returns 0 when not
/// recording; pass the result to [`end_fast_drive`].
///
/// A plain Rust call (perry-stdlib links perry-runtime as an rlib): the stats
/// hooks add no `extern "C"` symbol and no FFI contract to maintain.
#[inline]
pub fn begin_fast_drive() -> u64 {
    if enabled() && recording_thread() {
        now_ns()
    } else {
        0
    }
}

/// perry-stdlib: the fast drive started by [`begin_fast_drive`] ended.
#[inline]
pub fn end_fast_drive(started: u64) {
    if started != 0 {
        FAST_DRIVE.add(now_ns().saturating_sub(started));
    }
}

pub fn snapshot() -> LoopWaitStats {
    let mut wake_buckets = [0; 5];
    for (out, bucket) in wake_buckets.iter_mut().zip(WAKE_BUCKETS.iter()) {
        *out = bucket.load(Ordering::Relaxed);
    }
    LoopWaitStats {
        turnloop: TURNLOOP.snapshot(),
        tokio_tick: TOKIO_TICK.snapshot(),
        condvar: CONDVAR.snapshot(),
        fast_drive: FAST_DRIVE.snapshot(),
        zero_budget: ZERO_BUDGET.load(Ordering::Relaxed),
        throttle_sleeps: THROTTLE_SLEEPS.load(Ordering::Relaxed),
        wake_buckets,
        wake_max_ns: WAKE_MAX_NS.load(Ordering::Relaxed),
    }
}

/// The `[perry-loop-waits]` exit line (one line of `key=value` pairs).
pub fn format_line(arm: &str, s: &LoopWaitStats) -> String {
    let b = s.wake_buckets;
    format!(
        "[perry-loop-waits] arm={arm} \
         turnloop_waits={} turnloop_wait_ns={} turnloop_wait_max_ns={} \
         tokio_ticks={} tokio_tick_ns={} tokio_tick_max_ns={} \
         condvar_waits={} condvar_wait_ns={} condvar_wait_max_ns={} \
         fast_drives={} fast_drive_ns={} fast_drive_max_ns={} \
         zero_budget={} throttle_sleeps={} \
         wake_samples={} wake_lt50us={} wake_lt200us={} wake_lt1ms={} wake_lt5ms={} wake_ge5ms={} \
         wake_max_ns={}",
        s.turnloop.count,
        s.turnloop.total_ns,
        s.turnloop.max_ns,
        s.tokio_tick.count,
        s.tokio_tick.total_ns,
        s.tokio_tick.max_ns,
        s.condvar.count,
        s.condvar.total_ns,
        s.condvar.max_ns,
        s.fast_drive.count,
        s.fast_drive.total_ns,
        s.fast_drive.max_ns,
        s.zero_budget,
        s.throttle_sleeps,
        s.wake_samples(),
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
        s.wake_max_ns,
    )
}

/// Print the wait-metrics line once per process.
pub fn print_once(arm: &str) {
    static PRINTED: AtomicU8 = AtomicU8::new(0);
    if enabled() && PRINTED.swap(1, Ordering::Relaxed) == 0 {
        eprintln!("{}", format_line(arm, &snapshot()));
    }
}

#[cfg(test)]
#[path = "loop_stats_tests.rs"]
mod tests;
