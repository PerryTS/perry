//! RS-free young-leaf liveness: every evidence class must be exercised, the
//! precise oracle must catch a planted missed escape, and the escape bit that
//! shares HAS_SURVIVED must be set, kept and cleared where the design says.
use super::super::*;
use super::support::*;
use crate::arena::HeapGeneration;

unsafe fn leaf() -> usize {
    let bytes = LARGE_OBJECT_THRESHOLD_BYTES + 8;
    let p = crate::arena::arena_alloc_gc(bytes, 8, GC_TYPE_STRING);
    p.write_bytes(0, bytes);
    p as usize
}
fn sbits(p: usize) -> u64 {
    STRING_TAG | p as u64
}
unsafe fn flags(p: usize) -> u8 {
    (*header_from_user_ptr(p as *const u8)).gc_flags
}
unsafe fn small_young() -> usize {
    crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize
}
fn service() {
    super::super::leaf_service::collect();
}
fn panic_text(r: Result<(), Box<dyn std::any::Any + Send>>) -> String {
    let e = r.expect_err("the oracle must reject a dropped leaf");
    e.downcast_ref::<String>()
        .cloned()
        .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default()
}

#[test]
fn barrier_sets_the_escape_bit_before_the_cached_dirty_page_exit() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let (old, fields) = alloc_old_test_object(4);
        let small = small_young();
        *fields = ptr_bits(small);
        js_write_barrier_slot(ptr_bits(old as usize), fields as u64, ptr_bits(small));
        let page = crate::arena::generation_page_for_addr(fields as usize);
        assert!(
            super::super::dirty_page_cache::dirty_old_page_already_marked(page),
            "the first store must leave its page in the dirty-page cache"
        );
        let l = leaf();
        assert_eq!(flags(l) & GC_FLAG_HAS_SURVIVED, 0);
        *fields.add(1) = sbits(l);
        js_write_barrier_slot_validated_parent(old as u64, fields.add(1) as u64, sbits(l));
        assert_ne!(flags(l) & GC_FLAG_HAS_SURVIVED, 0, "cached-dirty store must still escape");
        service();
        assert_eq!(crate::arena::classify_heap_generation(l), HeapGeneration::Old);
        assert_ne!(flags(l) & GC_FLAG_TENURED, 0);
        assert_eq!(flags(l) & GC_FLAG_HAS_SURVIVED, 0, "promotion clears the shared bit");
        assert_eq!(flags(l) & GC_FLAG_MARKED, 0);
    })
    .join()
    .unwrap();
}

#[test]
fn replay_sets_the_bit_for_slots_the_dirty_page_skip_passes_over() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let (old, fields) = alloc_old_test_object(4);
        let small = small_young();
        let l = leaf();
        *fields = ptr_bits(small);
        *fields.add(1) = sbits(l);
        crate::gc::barrier::replay_old_parent_slot_range(old as usize, fields, 2);
        assert_ne!(flags(l) & GC_FLAG_HAS_SURVIVED, 0, "slot 1 shares slot 0's dirty page");
    })
    .join()
    .unwrap();
}

#[test]
fn young_cell_naming_a_leaf_retains_it_even_when_the_cell_is_dead() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let held = leaf();
        let loose = leaf();
        let arr = crate::array::js_array_alloc(2);
        crate::array::js_array_set_f64_extend(arr, 0, f64::from_bits(sbits(held)));
        service();
        assert_eq!(
            crate::arena::classify_heap_generation(held),
            HeapGeneration::Old,
            "a young referrer is census evidence (conservative: it is not rooted)"
        );
        assert_eq!(
            crate::arena::classify_heap_space(loose),
            crate::arena::HeapSpace::Unknown,
            "a leaf with no root, escape or young referrer is freed"
        );
    })
    .join()
    .unwrap();
}

#[test]
fn precise_root_retains_a_leaf_without_any_other_evidence() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let l = leaf();
        js_shadow_slot_set(0, sbits(l));
        service();
        assert_eq!(crate::arena::classify_heap_generation(l), HeapGeneration::Old);
    })
    .join()
    .unwrap();
}

#[test]
fn planted_missed_escape_is_caught_by_the_precise_oracle() {
    let r = std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let (old, fields) = alloc_old_test_object(2);
        let l = leaf();
        let _sabotage = super::super::leaf_escape::SkipEscapeBit::arm();
        *fields = sbits(l);
        js_write_barrier_slot(ptr_bits(old as usize), fields as u64, sbits(l));
        assert_eq!(flags(l) & GC_FLAG_HAS_SURVIVED, 0, "the sabotage must drop the bit");
        service();
    })
    .join();
    let text = panic_text(r);
    assert!(text.contains("precise service proves live"), "{text}");
}

#[test]
fn a_writer_that_clears_the_shared_bit_is_caught() {
    let r = std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let (old, fields) = alloc_old_test_object(2);
        let l = leaf();
        *fields = sbits(l);
        js_write_barrier_slot(ptr_bits(old as usize), fields as u64, sbits(l));
        assert_ne!(flags(l) & GC_FLAG_HAS_SURVIVED, 0);
        (*header_from_user_ptr(l as *const u8)).gc_flags &= !GC_FLAG_HAS_SURVIVED;
        service();
    })
    .join();
    let text = panic_text(r);
    assert!(text.contains("precise service proves live"), "{text}");
}

#[test]
fn store_in_the_unarmed_window_is_recovered_by_the_arming_rebuild() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        struct Reopen;
        impl Drop for Reopen {
            fn drop(&mut self) {
                close_barrier_arming_window_for_tests();
            }
        }
        reset_barrier_arming_for_tests();
        let _reopen = Reopen;
        let (old, fields) = alloc_old_test_object(2);
        let l = leaf();
        *fields = sbits(l);
        js_write_barrier_slot(ptr_bits(old as usize), fields as u64, sbits(l));
        assert_eq!(flags(l) & GC_FLAG_HAS_SURVIVED, 0, "unarmed barrier records nothing");
        service();
        assert_eq!(
            crate::arena::classify_heap_generation(l),
            HeapGeneration::Old,
            "the arming rebuild must have applied the escape test to the old slot"
        );
    })
    .join()
    .unwrap();
}

#[test]
fn only_the_exact_extent_base_is_a_leaf_header() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let l = leaf();
        assert_eq!(l & 4095, 8, "the page-aligned leaf test");
        assert!(crate::arena::exact_young_leaf_header(l).is_some());
        assert!(
            crate::arena::exact_young_leaf_header(l + 4096).is_none(),
            "an interior address with the same page offset is payload, not a header"
        );
        assert!(crate::arena::exact_young_leaf_header(small_young()).is_none());
    })
    .join()
    .unwrap();
}

