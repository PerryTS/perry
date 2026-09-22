//! Dictionary-mode counters and latch state (#10868 step 2.5 stage 1).
//!
//! Their own file because `gc_runtime_root_holders`' rule B only clears a
//! static integer table in a file that never calls an allocator, and
//! `dictionary.rs` allocates. None of these words ever holds a GC pointer;
//! keeping them somewhere the gate can see that is cheaper than an entry in
//! `gc_runtime_root_holders.json` that a reader has to take on trust.

use std::sync::atomic::{AtomicI8, AtomicU64};

pub(super) static LAYOUT_ID_BUDGET: AtomicU64 = AtomicU64::new(u64::MAX);
pub(super) static EXHAUSTION_LATCHES: AtomicU64 = AtomicU64::new(0);
pub(super) static LATCH_ARMED: AtomicI8 = AtomicI8::new(-1);
pub(super) static LATCH_MIN_KEYS: AtomicU64 = AtomicU64::new(0);
pub(super) static LATCH_CANDIDATES: AtomicU64 = AtomicU64::new(0);
pub(super) static LATCHES: AtomicU64 = AtomicU64::new(0);
pub(super) static PUBLICATIONS: AtomicU64 = AtomicU64::new(0);
pub(super) static REGENERATIONS: AtomicU64 = AtomicU64::new(0);

/// Monotonic within the dictionary generation namespace. Starts at 1 so a
/// drawn generation is never bare `DICTIONARY_GENERATION_TAG`.
pub(super) static DICTIONARY_GENERATION_NEXT: AtomicU64 = AtomicU64::new(1);
