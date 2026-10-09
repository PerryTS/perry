//! Shared generated/runtime Eden bumps must feed the pacer exactly once.
use super::*;
use crate::gc::{GC_HEADER_SIZE, GC_TYPE_STRING};

#[test]
fn inline_runtime_bursts_preserve_exact_allocation_accounting() {
    tests::run_with_fresh_arenas(|| unsafe {
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let state = js_inline_arena_state();
        sync_inline_arena_state();
        let before = allocation_totals();
        let initial_offset = (*state).offset;
        let mut expected = 0;
        let mut last_end = (*state).data.add(initial_offset);
        // Odd string payload sizes exercise padding while alternating the
        // collecting and guaranteed-no-collect runtime entries.
        for i in 0..128 {
            let payload = 21 + i % 17;
            let total = (GC_HEADER_SIZE + payload + 7) & !7;
            let p = if i % 2 == 0 {
                arena_alloc_gc(payload, 8, GC_TYPE_STRING)
            } else {
                arena_alloc_gc_no_collect(payload, 8, GC_TYPE_STRING)
            };
            assert!(!p.is_null());
            let raw = p.sub(GC_HEADER_SIZE);
            assert_eq!(raw, last_end, "generated/runtime state must not overlap");
            assert_eq!(p as usize & 7, 0);
            assert_eq!(
                (*crate::gc::header_from_trusted_user_ptr(p)).size as usize,
                total
            );
            assert_eq!(
                classify_heap_generation(p as usize),
                HeapGeneration::Nursery
            );
            expected += total;
            last_end = raw.add(total);
        }
        assert_eq!((*state).offset - initial_offset, expected);
        sync_inline_arena_state();
        let after = allocation_totals();
        assert_eq!(after.0 - before.0, expected);
        assert_eq!(after.1, before.1, "small bumps are nursery churn");
        sync_inline_arena_state();
        assert_eq!(
            allocation_totals(),
            after,
            "a second checkpoint cannot recharge a burst"
        );
        // Large raw requests keep their eager large-allocation attribution.
        let p = arena_alloc(16 * 1024, 8);
        assert!(!p.is_null());
        sync_inline_arena_state();
        let large = allocation_totals();
        assert_eq!(large.0 - after.0, 16 * 1024);
        assert_eq!(large.1 - after.1, 16 * 1024);
    });
}
