//! The unmapped-frame verifier — the runtime safety net of RFC deferred
//! collection (§2, "Soundness against the #11522 class", layer 3).
//!
//! The native-root walkers look each frame's return address up in the GC map
//! and, when nothing matches, move on: a Rust runtime frame legitimately has no
//! map. A GENERATED frame with no map at its return address is something else:
//! the frame is suspended at a call codegen marked `gc-leaf-function` (so RS4GC
//! recorded nothing there), and a collection is running anyway. That call's
//! callee did collect. Its caller's roots were not visited, so a moving
//! collection leaves them stale and a non-moving one may free what they name —
//! the #11522 / #11523 failure, reported by nobody.
//!
//! This module makes that frame fail loudly. The question "is this a generated
//! function?" is answered from the function's start address (the unwinder's
//! region start), looked up in the set of statepoint-strategy functions the GC
//! map lists. Functions with records are always listed; functions whose every
//! call is a leaf have no records, and codegen lists them as zero-record
//! entries when the program was compiled with the GC instruments
//! (`PERRY_GC_INSTRUMENTS=1`, `PERRY_GC_VERIFY_FRAMES` or a schedule seed at
//! compile time). A v6 runtime that predates this skips such entries, so the
//! format did not change.
//!
//! Only a PRECISE collection is checked: a conservative one scans the whole
//! native stack, unmapped frames included, and does not move.
//!
//! Armed by `PERRY_GC_VERIFY_FRAMES=1`, by a resolved `PERRY_GC_SCHEDULE_SEED`
//! (the pairing the RFC names: at `RATE=1` every legal collection point is
//! checked), and in `debug_assertions` builds (`gcaudit`). Off in release: it
//! needs the unwinder rather than the x29-chain walk, and the zero-record
//! entries cost map bytes; see the S5 changelog for the numbers.

use super::StackMapIndex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

static FRAMES_CHECKED: AtomicU64 = AtomicU64::new(0);
static GENERATED_UNMAPPED: AtomicU64 = AtomicU64::new(0);

/// Whether the verifier is armed for this process.
pub(in crate::gc) fn active() -> bool {
    static ACTIVE: OnceLock<bool> = OnceLock::new();
    *crate::once_init::get_or_init(&ACTIVE, || {
        cfg!(debug_assertions) || knob_enabled() || crate::gc::schedule::gc_schedule_enabled()
    })
}

fn knob_enabled() -> bool {
    #[cfg(not(feature = "gc-instruments"))]
    return false;
    #[cfg(feature = "gc-instruments")]
    crate::gc::env_flag_enabled("PERRY_GC_VERIFY_FRAMES")
}

/// A walker found no record for the frame whose return address is `ip` and
/// whose function starts at `function_start`. Panics if that function is a
/// generated statepoint-strategy function and this collection is precise.
pub(super) fn unmatched_frame(index: &StackMapIndex, ip: usize, function_start: usize) {
    FRAMES_CHECKED.fetch_add(1, Ordering::Relaxed);
    if function_start == 0 || !index.is_generated_function(function_start) {
        return;
    }
    if matches!(
        crate::gc::conservative_stack_scan_decision(),
        crate::gc::ConservativeStackScanDecision::Scan
    ) {
        return;
    }
    GENERATED_UNMAPPED.fetch_add(1, Ordering::Relaxed);
    unmapped_generated_frame(ip, function_start);
}

#[cold]
#[inline(never)]
fn unmapped_generated_frame(ip: usize, function_start: usize) -> ! {
    panic!(
        "perry GC safety net: a precise collection walked a GENERATED frame with no \
         stack map at its call site (function {function_start:#x}, return address \
         {ip:#x}, offset {:#x}). The call was compiled as one that cannot collect \
         (`gc-leaf-function`), and a collection began inside it, so this frame's \
         roots were not visited. See RFC deferred collection §2 (the #11522 class).",
        ip.wrapping_sub(function_start)
    )
}

/// `(frames_checked, generated_unmapped)` — the verifier's liveness counters.
pub(in crate::gc) fn counters() -> (u64, u64) {
    (
        FRAMES_CHECKED.load(Ordering::Relaxed),
        GENERATED_UNMAPPED.load(Ordering::Relaxed),
    )
}

impl StackMapIndex {
    /// Whether `function_start` is a generated statepoint-strategy function:
    /// one with records, or a zero-record entry codegen listed.
    pub(super) fn is_generated_function(&self, function_start: usize) -> bool {
        self.functions
            .binary_search_by_key(&function_start, |entry| entry.address)
            .is_ok()
            || self.unrecorded_functions.binary_search(&function_start).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sabotage for the classification the verifier rests on: a zero-record
    /// entry must make its function "generated", a Rust frame must not be, and
    /// an unmatched frame in a generated function under a precise collection
    /// must panic.
    #[test]
    fn unmatched_generated_frame_fails_loudly_and_runtime_frames_do_not() {
        let mut index = StackMapIndex::default();
        index.unrecorded_functions = vec![0x1000, 0x2000];
        assert!(index.is_generated_function(0x2000));
        assert!(!index.is_generated_function(0x3000));

        // A runtime frame: ignored.
        unmatched_frame(&index, 0x3010, 0x3000);
        let prev =
            crate::gc::set_conservative_stack_scan_override(Some(crate::gc::ConservativeStackScanMode::Disabled));
        let result = std::panic::catch_unwind(|| unmatched_frame(&index, 0x2010, 0x2000));
        crate::gc::set_conservative_stack_scan_override(prev);
        let message = result.expect_err("an unmapped generated frame must panic");
        let text = message
            .downcast_ref::<String>()
            .map(String::as_str)
            .unwrap_or_default();
        assert!(text.contains("safety net"), "{text}");
        assert!(counters().1 >= 1);
    }

    /// A conservative collection is not checked: it scanned the frame.
    #[test]
    fn a_conservative_collection_tolerates_unmapped_generated_frames() {
        let mut index = StackMapIndex::default();
        index.unrecorded_functions = vec![0x4000];
        let prev =
            crate::gc::set_conservative_stack_scan_override(Some(crate::gc::ConservativeStackScanMode::Full));
        unmatched_frame(&index, 0x4010, 0x4000);
        crate::gc::set_conservative_stack_scan_override(prev);
    }
}
