//! Dirty scanning and re-remembering share the descriptor's custody verdict.
use super::super::*;
use super::support::*;

fn custody_disagreements(negative_control: bool) -> usize {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    unsafe {
        let (old, _) = alloc_old_test_array(8);
        let captures = vec![crate::value::TAG_UNDEFINED; LARGE_OBJECT_THRESHOLD_BYTES / 8 + 64];
        let malloc = crate::closure::js_closure_alloc_init(
            std::ptr::null(),
            captures.len() as u32,
            captures.as_ptr(),
        );
        // A separate OLD cache: its leaf header cannot rediscover the slots
        // that the lazy owner's descriptor enumerates.
        let lazy = crate::arena::arena_alloc_gc_old(
            std::mem::size_of::<crate::json_tape::LazyArrayHeader>(),
            8,
            GC_TYPE_LAZY_ARRAY,
        )
        .cast::<crate::json_tape::LazyArrayHeader>();
        std::ptr::write_bytes(lazy, 0, 1);
        let cache = crate::arena::arena_alloc_gc_old(32, 8, GC_TYPE_STRING).cast::<u64>();
        let bitmap = crate::arena::arena_alloc_gc_old(32, 8, GC_TYPE_STRING).cast::<u64>();
        std::ptr::write_bytes(cache, 0, 4);
        std::ptr::write_bytes(bitmap, 0, 4);
        *bitmap = 1;
        (*lazy).magic = crate::json_tape::LAZY_ARRAY_MAGIC;
        (*lazy).cached_length = 1;
        (*lazy).materialized_elements = cache.cast();
        (*lazy).materialized_bitmap = bitmap;
        assert!(!GcMutableSlot::new(cache, None).external());

        let mut disagreements = 0;
        for (user, expected_external) in [
            (old.cast::<u8>(), false),
            (malloc as *mut u8, true),
            (lazy.cast::<u8>(), true),
        ] {
            let header = header_from_user_ptr(user);
            let mut pages = crate::fast_hash::new_ptr_hash_set();
            visit_gc_rewrite_slots(header, |slot| {
                pages.insert(crate::arena::generation_page_for_addr(slot.slot as usize));
            });
            let mut slots = 0;
            barrier::scan_dirty_object_slots(
                header,
                &pages,
                &mut Default::default(),
                &mut |slot, external, _| {
                    slots += 1;
                    let verdict = if negative_control {
                        slot.external()
                    } else {
                        external
                    };
                    disagreements += usize::from(verdict != expected_external);
                },
            );
            assert!(slots > 0, "each parent must exercise descriptor custody");
        }
        disagreements
    }
}

#[test]
fn dirty_scan_uses_parent_and_descriptor_custody() {
    assert_eq!(custody_disagreements(false), 0);
}

#[test]
fn negative_control_generation_only_custody_misses_the_old_side_buffer() {
    assert!(custody_disagreements(true) > 0);
}
