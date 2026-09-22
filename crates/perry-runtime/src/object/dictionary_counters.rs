//! Dictionary-mode counters and latch state (#10868 step 2.5 stage 1).
//!
//! Their own file because `gc_runtime_root_holders`' rule B only clears a
//! static integer table in a file that never calls an allocator, and
//! `dictionary.rs` allocates. None of these words ever holds a GC pointer;
//! keeping them somewhere the gate can see that is cheaper than an entry in
//! `gc_runtime_root_holders.json` that a reader has to take on trust.

use std::sync::atomic::{AtomicI8, AtomicU64};

// #10868 step 2.5: the LATCH STATE is `per_test_global!`, the counters below
// are not.
//
// The state is what a test MUTATES and a later test READS, which is exactly
// L16.11's class. It became load-bearing when step 2.5 armed the latch by
// default: every test that arms or disarms it now leaves a different value
// behind than it found, and `own_key_membership_crosses_65536_without_a_cutoff`
// — whose key list is unique to it, so it is the k(k+1)/2 cliff — runs later
// in the same binary. It PASSED STANDALONE AND WAS OOM-KILLED IN THE SUITE,
// which is that class's signature.
//
// Three save/restore fixes did not close it, and there were four disarm sites
// to chase. Patching them one at a time is the whack-a-mole `per_test_global!`
// exists to end: per-thread in a test build, the plain `static` outside one,
// so a test cannot reach another test's arming and a new disarm site cannot
// reintroduce the hazard.
per_test_global! {
    pub(super) static LATCH_ARMED: AtomicI8 = AtomicI8::new(-1);
    pub(super) static LATCH_MIN_KEYS: AtomicU64 = AtomicU64::new(0);
    pub(super) static LAYOUT_ID_BUDGET: AtomicU64 = AtomicU64::new(u64::MAX);
}

pub(super) static EXHAUSTION_LATCHES: AtomicU64 = AtomicU64::new(0);
pub(super) static LATCH_CANDIDATES: AtomicU64 = AtomicU64::new(0);
pub(super) static LATCHES: AtomicU64 = AtomicU64::new(0);
pub(super) static PUBLICATIONS: AtomicU64 = AtomicU64::new(0);
pub(super) static REGENERATIONS: AtomicU64 = AtomicU64::new(0);

/// Monotonic within the dictionary generation namespace. Starts at 1 so a
/// drawn generation is never bare `DICTIONARY_GENERATION_TAG`.
pub(super) static DICTIONARY_GENERATION_NEXT: AtomicU64 = AtomicU64::new(1);
