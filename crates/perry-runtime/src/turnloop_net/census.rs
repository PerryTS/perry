//! `PERRY_LOOP_STATS=1` census of P1 net traffic, by op class and outcome.
//!
//! The `[perry-loop] driver=turnloop` line's `completions=` field counts every
//! completion the driver handed back in a turn — P1 net, P2 process, P3 JS
//! timers and P4 pool alike, because it is summed in `agent_loop` *before*
//! routing. That is the right number for "how much work came back from the
//! loop", and the wrong one for "what did one HTTP request cost": it cannot say
//! whether a request's completions were its read and its write, or a read, a
//! write and two more from a timer handle being destroyed and rebuilt.
//!
//! This module answers that. It counts submissions and completions per op
//! class, splits the timer class into expiry / cancel / close (the three are
//! indistinguishable in the aggregate but have very different meaning), and
//! separates a timer *reset* — which moves a deadline in place and costs
//! nothing — from a timer *create*, which allocates a handle whose eventual
//! destruction costs two completions.
//!
//! Diagnostic only, never a behaviour knob, and gated on the same variable as
//! the rest of the loop stats: when it is unset every hook is one relaxed load
//! of the cached `loop_stats` state and a predictable branch; when it is set
//! the hooks are relaxed atomic adds, with no allocation and no lock.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use turnloop::OpResult;

/// Op classes are 1..=8 (`OP_ACCEPT`..`OP_TIMER`); slot 0 is never used.
const CLASSES: usize = 9;

static SUBMITS: [AtomicU64; CLASSES] = [const { AtomicU64::new(0) }; CLASSES];
static COMPLETIONS: [AtomicU64; CLASSES] = [const { AtomicU64::new(0) }; CLASSES];

/// A deadline that expired and was delivered to its subsystem.
static TIMER_FIRED: AtomicU64 = AtomicU64::new(0);
/// The `Cancelled` a live deadline's operation produces when its handle closes.
static TIMER_CANCELLED: AtomicU64 = AtomicU64::new(0);
/// The `Closed` the handle itself produces after that.
static TIMER_CLOSED: AtomicU64 = AtomicU64::new(0);
/// A deadline moved in place: no handle churn, and no completion at all.
static TIMER_RESETS: AtomicU64 = AtomicU64::new(0);
/// A deadline disarmed by moving it out of reach rather than destroying it.
static TIMER_PARKS: AtomicU64 = AtomicU64::new(0);
/// A fresh deadline handle. Each one costs two completions when it is cancelled.
static TIMER_CREATES: AtomicU64 = AtomicU64::new(0);
/// Completions whose entry had already gone — routed nowhere, pure waste.
static NO_ENTRY: AtomicU64 = AtomicU64::new(0);

fn on() -> bool {
    crate::event_pump::loop_stats::enabled()
}

pub(super) fn note_submit(op_class: u64) {
    if on() {
        if let Some(slot) = SUBMITS.get(op_class as usize) {
            slot.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub(super) fn note_completion(op_class: u64) {
    if on() {
        if let Some(slot) = COMPLETIONS.get(op_class as usize) {
            slot.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Split the timer class, which the aggregate cannot: an expiry is work the
/// program asked for, a cancel/close pair is the cost of having destroyed a
/// deadline instead of moving it.
pub(super) fn note_timer_result(result: &OpResult) {
    if !on() {
        return;
    }
    let slot = match result {
        OpResult::Timer => &TIMER_FIRED,
        OpResult::Cancelled | OpResult::Stopped => &TIMER_CANCELLED,
        OpResult::Closed => &TIMER_CLOSED,
        _ => return,
    };
    slot.fetch_add(1, Ordering::Relaxed);
}

pub(super) fn note_timer_reset() {
    if on() {
        TIMER_RESETS.fetch_add(1, Ordering::Relaxed);
    }
}

pub(super) fn note_timer_park() {
    if on() {
        TIMER_PARKS.fetch_add(1, Ordering::Relaxed);
    }
}

pub(super) fn note_timer_create() {
    if on() {
        TIMER_CREATES.fetch_add(1, Ordering::Relaxed);
    }
}

pub(super) fn note_no_entry() {
    if on() {
        NO_ENTRY.fetch_add(1, Ordering::Relaxed);
    }
}

fn get(table: &[AtomicU64; CLASSES], op_class: usize) -> u64 {
    table[op_class].load(Ordering::Relaxed)
}

/// Print the census once, from the process-exit funnel, when the variable is
/// set. A separate `[perry-loop] p1` line rather than a field on the
/// `driver=turnloop` line, because two instruments parse that one positionally.
pub fn print_once() {
    if !on() {
        return;
    }
    static PRINTED: AtomicBool = AtomicBool::new(false);
    if PRINTED.swap(true, Ordering::AcqRel) {
        return;
    }
    eprintln!(
        "[perry-loop] p1 sub_accept={} sub_read={} sub_write={} sub_shutdown={} sub_connect={} sub_close={} sub_resolve={} timer_creates={} timer_resets={} timer_parks={} timer_closes={}",
        get(&SUBMITS, super::OP_ACCEPT as usize),
        get(&SUBMITS, super::OP_READ as usize),
        get(&SUBMITS, super::OP_WRITE as usize),
        get(&SUBMITS, super::OP_SHUTDOWN as usize),
        get(&SUBMITS, super::OP_CONNECT as usize),
        get(&SUBMITS, super::OP_CLOSE as usize),
        get(&SUBMITS, super::OP_RESOLVE as usize),
        TIMER_CREATES.load(Ordering::Relaxed),
        TIMER_RESETS.load(Ordering::Relaxed),
        TIMER_PARKS.load(Ordering::Relaxed),
        get(&SUBMITS, super::OP_TIMER as usize),
    );
    eprintln!(
        "[perry-loop] p1 comp_accept={} comp_read={} comp_write={} comp_shutdown={} comp_connect={} comp_close={} comp_resolve={} comp_timer={} timer_fired={} timer_cancelled={} timer_closed={} dropped_no_entry={}",
        get(&COMPLETIONS, super::OP_ACCEPT as usize),
        get(&COMPLETIONS, super::OP_READ as usize),
        get(&COMPLETIONS, super::OP_WRITE as usize),
        get(&COMPLETIONS, super::OP_SHUTDOWN as usize),
        get(&COMPLETIONS, super::OP_CONNECT as usize),
        get(&COMPLETIONS, super::OP_CLOSE as usize),
        get(&COMPLETIONS, super::OP_RESOLVE as usize),
        get(&COMPLETIONS, super::OP_TIMER as usize),
        TIMER_FIRED.load(Ordering::Relaxed),
        TIMER_CANCELLED.load(Ordering::Relaxed),
        TIMER_CLOSED.load(Ordering::Relaxed),
        NO_ENTRY.load(Ordering::Relaxed),
    );
}
