//! Instrument-liveness counters (#7604): did the run's GC stress actually
//! exercise anything?
//!
//! They are not properties of any one stress mode — they count what the
//! COLLECTOR did (copying minors completed, objects relocated, loop back-edge
//! polls reached) regardless of what forced it. That is why they live here
//! rather than inside `schedule.rs`: a counter that lives in the knob it
//! measures dies with the knob.
//!
//! Process-global rather than thread-local: the report is about the run.

use std::sync::atomic::{AtomicU64, Ordering};

static COPYING_MINORS: AtomicU64 = AtomicU64::new(0);
static MOVED_OBJECTS: AtomicU64 = AtomicU64::new(0);

/// Called once per COMPLETED copying minor, with what it relocated.
///
/// `copied + promoted`, not `copied` alone: #7657 made the explicit-`gc()` path
/// precise, which lets `gc/tenuring.rs` seed the adaptive threshold from these
/// cycles, and on two ratchet probes survivors are now promoted on first copy
/// rather than copied into survivor space. A `copied_objects > 0` liveness
/// assertion would have been pinned permanently false on exactly those probes.
#[inline]
pub(crate) fn note_copying_minor_moved(copied_objects: usize, promoted_objects: usize) {
    COPYING_MINORS.fetch_add(1, Ordering::Relaxed);
    MOVED_OBJECTS.fetch_add(
        (copied_objects + promoted_objects) as u64,
        Ordering::Relaxed,
    );
}

/// How many COPYING minors have completed in this process.
pub fn copying_minor_cycles() -> u64 {
    COPYING_MINORS.load(Ordering::Relaxed)
}

/// `copied_objects + promoted_objects` summed over every copying minor.
pub fn moved_objects_total() -> u64 {
    MOVED_OBJECTS.load(Ordering::Relaxed)
}

static LOOP_POLLS: AtomicU64 = AtomicU64::new(0);

/// Every `js_gc_loop_safepoint` that got past the compile-time/runtime opt-in.
///
/// This is the counter that answers "was the COMPILE-TIME half live", and it
/// exists because the obvious external check does not work: `nm`/`objdump
/// -d BIN | grep -c js_gc_loop_safepoint` reports **0** on a binary whose polls
/// then fire 20069 times, so an operator following that advice concludes the
/// polls are absent when they are not. Measured, not assumed.
#[inline]
pub(crate) fn note_loop_poll_reached() {
    LOOP_POLLS.fetch_add(1, Ordering::Relaxed);
}

/// How many loop back-edge polls this run reached.
pub fn loop_polls_reached() -> u64 {
    LOOP_POLLS.load(Ordering::Relaxed)
}
