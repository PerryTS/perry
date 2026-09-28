//! Function-entry polls (RFC deferred collection S5, decision 2).
//!
//! Codegen places one at the indirect entry of closures, methods and value
//! wrappers, and one per recursive SCC (`perry-codegen/src/entry_polls.rs`).
//! Each is the back-edge poll under another name: the inline armed-word check,
//! then [`super::policy::js_gc_loop_safepoint_armed`]. So it is a declared
//! safepoint with exactly the loop poll's semantics — it drains a deferred
//! nursery collection, serves a parked budgeted root phase or an owed
//! collection, and is a candidate for the seeded schedule (paced by
//! `PERRY_GC_SCHEDULE_ALLOC_KB` like every poll). The distinct symbol exists
//! only so codegen's leaf analysis can tell an entry poll from a loop poll.

use std::sync::atomic::{AtomicU64, Ordering};

static ENTRY_POLLS: AtomicU64 = AtomicU64::new(0);

/// How many armed entry polls this run reached (a liveness counter; the
/// unarmed fast path is not counted, exactly like `loop_polls_reached`).
pub fn entry_polls_reached() -> u64 {
    ENTRY_POLLS.load(Ordering::Relaxed)
}

#[no_mangle]
pub extern "C" fn js_gc_entry_safepoint() {
    if !super::poll_armed() {
        return;
    }
    ENTRY_POLLS.fetch_add(1, Ordering::Relaxed);
    super::policy::js_gc_loop_safepoint_armed();
}

/// The entry poll of a `__perry_wrap_*` forwarder, whose arguments live only
/// in its registers. The wrapper spills them to `args` (`count` NaN-boxed
/// words; slot 0 is the closure pointer boxed as an object), and this roots
/// every one across the collection and writes the possibly-relocated values
/// back for the wrapper to reload.
///
/// # Safety
/// `args` must point at `count` writable, initialised 8-byte words.
#[no_mangle]
pub unsafe extern "C" fn js_gc_entry_safepoint_args(args: *mut u64, count: u32) {
    if !super::poll_armed() || args.is_null() {
        return;
    }
    ENTRY_POLLS.fetch_add(1, Ordering::Relaxed);
    let words = unsafe { std::slice::from_raw_parts_mut(args, count as usize) };
    let scope = super::RuntimeHandleScope::new();
    let handles: Vec<_> = words
        .iter()
        .map(|bits| scope.root_nanbox_f64(f64::from_bits(*bits)))
        .collect();
    super::policy::js_gc_loop_safepoint_armed();
    for (word, handle) in words.iter_mut().zip(&handles) {
        *word = handle.get_nanbox_f64().to_bits();
    }
}
