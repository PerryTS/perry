//! The `gc_check_trigger` fast path must resolve its thread-locals through the
//! hot cache, not through `_tlv_get_addr`.
//!
//! `gc_check_trigger` runs on **every** `gc_malloc`. Measured with `sample` on
//! the compiled claude-code TUI streaming a 3300-char reply (14,580 main-thread
//! samples), `_tlv_get_addr` was 380 main-thread leaf samples and **85 of them
//! sat under `gc_budgeted_due_trigger < gc_check_trigger < gc_malloc`** — the
//! trigger predicate resolving `GC_OLD_RECLAIM_PENDING`,
//! `GC_LAST_OLD_RECLAIM_IN_USE_BYTES`, `GC_NEXT_MALLOC_TRIGGER`,
//! `GC_TRIGGER_ARMED`, `GC_NEXT_TRIGGER_BYTES`, `GC_EXTERNAL_SIDE_LIVE_BYTES`,
//! `OLD_GEN_IN_USE_BYTES`, `ARENA_TOTAL_BYTES`, `OLD_FREE_BYTES`,
//! `MALLOC_STATE` and the survivor cells one out-of-line call at a time.
//!
//! `scripts/check_thread_locals.py` cannot express this: it ratchets on the
//! number of raw `thread_local!` *blocks* per file, so `gc/policy.rs` read as
//! "6" while declaring 28 cold thread-locals, and the block count does not move
//! when a declaration is converted alongside a split (`arena/block.rs` went
//! from 11 cold declarations to 0 with its recorded count unchanged at 2). The
//! declaration-denominated ratchet added in the same change closes the *policy*
//! hole; this test is the *runtime* half — it asserts the mechanism is live on
//! the path the profile named.

use crate::tls_hot::{published_slots, HOT_SLOT_CAPACITY};

/// Every trigger-path declaration named above must own a hot slot, and
/// driving `gc_check_trigger` must actually populate slots on the calling
/// thread.
///
/// Two assertions, because they fail to different sabotage. Reverting one
/// declaration to a raw `thread_local!` removes `slot_index` and breaks the
/// build at the named line; making the slot allocator hand out its overflow
/// sentinel keeps the build green and trips the bound check below.
#[test]
fn gc_check_trigger_resolves_its_thread_locals_through_the_hot_cache() {
    // A fresh thread so the "how many slots did this path publish" reading is
    // this path's, not the harness's.
    std::thread::spawn(|| {
        let before = published_slots();
        crate::gc::gc_check_trigger();
        let after = published_slots();

        for (name, idx) in crate::gc::policy::trigger_path_hot_slot_indices() {
            assert!(
                (idx as usize) < HOT_SLOT_CAPACITY,
                "{name} did not claim a hot TLS slot (index {idx}, capacity \
                 {HOT_SLOT_CAPACITY}); it is paying `_tlv_get_addr` per read on \
                 every `gc_malloc`",
            );
        }

        // The predicate reads at least the ten declarations listed in the
        // module docs. A lower bound, deliberately: `gc_check_trigger` reaches
        // other hot declarations too, and this must not fail when an unrelated
        // one is added or removed. It DOES fail if the trigger path stops
        // going through `HotKey` at all, which is the regression #7469 has
        // already suffered three times.
        let published = after - before;
        assert!(
            published >= 11,
            "gc_check_trigger published only {published} hot TLS slots on a \
             fresh thread ({before} -> {after}); the trigger path's \
             thread-locals are resolving through `_tlv_get_addr` again",
        );
    })
    .join()
    .expect("trigger-path TLS probe thread panicked");
}
