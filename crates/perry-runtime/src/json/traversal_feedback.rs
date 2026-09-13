//! Per-thread feedback: do this thread's lazy JSON arrays get fully traversed?
//!
//! A top-level JSON array parses onto a validating tape and materializes its
//! elements on demand. That wins whenever the caller touches a few elements --
//! `parse` and `sparse` shapes run at ~0.4x the better of Node and Bun -- but a
//! program that then walks EVERY element pays for two tokenizations: the tape
//! build, and the per-record reparse the scan flip hands to the direct parser.
//! Profiled on records_array_1m:scan: 26.7% tape build, 43.1% record reparse,
//! 1.13x the better engine; on records_array_16k:scan, 1.23x. The same inputs
//! parsed eagerly run at 0.97x and 0.76x.
//!
//! No size threshold can pick between the two, because the direct parser is
//! also the WRONG choice for some arrays nobody traverses: heterogeneous_1m's
//! 32 record shapes cost it 2.89x CPU and 3.31x RSS where the tape takes 0.35x
//! and 0.66x. So the decision follows behaviour instead. Every lazy array
//! created costs a point of evidence; every traversal flip earns two. Once the
//! score shows traversal is the norm, eligible parses go eagerly, and one in
//! [`RESAMPLE_EVERY`] still goes lazily so a program that stops traversing
//! drifts back. Parse-only and sparse-access programs never flip, so they never
//! leave the tape.
//!
//! Measured across all 50 JSON matrix cells in one binary, against the tape-only
//! route: records_array_16k:scan 1.23x -> 0.78x, records_array_1m:scan
//! 1.11x -> 0.98x, records_array_8m:scan CPU 0.93x -> 0.77x and RSS 190 ->
//! 163 MiB; every other cell unchanged.
//!
//! Only element-by-element reads in `lazy_get_rooted` count (the flip, or an
//! in-order read of the last element). Stringify,
//! revivers, array methods and mutation also force materialization, but none of
//! them is evidence that a scan would have been cheaper eagerly.

use std::cell::Cell;

/// Evidence at or above which an eligible parse goes eagerly.
const PREFER_EAGER_AT: u8 = 4;
/// Ceiling on accumulated evidence, so a long traversal history is unlearned
/// within a bounded number of untraversed parses.
const SCORE_MAX: u8 = 8;
/// While eager is preferred, one parse in this many still takes the tape, so
/// the evidence keeps being refreshed.
const RESAMPLE_EVERY: u8 = 16;

// Two byte counters read once per parse: plain `thread_local!`, not the hot-TLS
// macro, and no heap pointer can live in either.
thread_local! {
    static SCORE: Cell<u8> = const { Cell::new(0) };
    static EAGER_RUN: Cell<u8> = const { Cell::new(0) };
}

/// A lazy array was created: one point of evidence against traversal.
pub(crate) fn note_lazy_array_created() {
    SCORE.with(|s| s.set(s.get().saturating_sub(1)));
}

/// Called after every cold element read of a lazy array.
///
/// `flip` is the existing adaptive threshold (cumulative walk or scan streak):
/// materialize, and count it as traversal evidence. `completed_scan` means this
/// read finished an in-order walk of the WHOLE array. That has to count on its
/// own, because the flip deliberately never fires for an array too small for
/// its streak to be proportional evidence (a 120-row array reaches the 64-read
/// streak with over half its elements already cached) -- and a program that
/// walks every one of those small arrays is exactly the one that pays for the
/// tape twice.
///
/// # Safety
///
/// `hdr` must be a live `LazyArrayHeader`, as for `force_materialize_lazy`.
pub(crate) unsafe fn after_cold_read(
    hdr: *mut crate::json_tape::LazyArrayHeader,
    flip: bool,
    completed_scan: bool,
) {
    if flip || completed_scan {
        SCORE.with(|s| s.set(s.get().saturating_add(2).min(SCORE_MAX)));
    }
    if flip {
        crate::json_tape::force_materialize_lazy(hdr);
    }
}

/// Should an otherwise tape-eligible parse go eagerly instead?
pub(crate) fn prefer_eager() -> bool {
    if SCORE.with(Cell::get) < PREFER_EAGER_AT {
        EAGER_RUN.with(|r| r.set(0));
        return false;
    }
    EAGER_RUN.with(|r| {
        let next = r.get() + 1;
        if next >= RESAMPLE_EVERY {
            r.set(0);
            false
        } else {
            r.set(next);
            true
        }
    })
}

#[cfg(test)]
pub(crate) fn reset_for_tests() {
    SCORE.with(|s| s.set(0));
    EAGER_RUN.with(|r| r.set(0));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn created_then(flipped: bool) {
        note_lazy_array_created();
        if flipped {
            SCORE.with(|s| s.set(s.get().saturating_add(2).min(SCORE_MAX)));
        }
    }

    #[test]
    fn traversal_evidence_switches_to_eager_and_keeps_resampling() {
        reset_for_tests();
        // A scan loop: every lazy array is fully traversed.
        let mut lazy = 0;
        let mut eager = 0;
        for _ in 0..200 {
            if prefer_eager() {
                eager += 1;
            } else {
                lazy += 1;
                created_then(true);
            }
        }
        assert!(
            eager > 150,
            "a traversing program must mostly parse eagerly"
        );
        assert!(
            lazy >= 200 / RESAMPLE_EVERY as usize,
            "it must keep resampling lazily"
        );
        reset_for_tests();
    }

    #[test]
    fn untraversed_arrays_never_leave_the_tape() {
        reset_for_tests();
        for _ in 0..200 {
            assert!(!prefer_eager(), "parse-only programs must stay lazy");
            created_then(false);
        }
        reset_for_tests();
    }

    #[test]
    fn a_program_that_stops_traversing_drifts_back_to_the_tape() {
        reset_for_tests();
        for _ in 0..64 {
            if !prefer_eager() {
                created_then(true);
            }
        }
        assert!(SCORE.with(Cell::get) >= PREFER_EAGER_AT);
        let mut went_lazy_for_good = false;
        for _ in 0..200 {
            if !prefer_eager() {
                created_then(false);
                if SCORE.with(Cell::get) < PREFER_EAGER_AT {
                    went_lazy_for_good = true;
                    break;
                }
            }
        }
        assert!(
            went_lazy_for_good,
            "evidence must be unlearned once traversal stops"
        );
        reset_for_tests();
    }
}
