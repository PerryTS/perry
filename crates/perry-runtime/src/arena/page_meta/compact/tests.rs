use super::*;

#[test]
fn compact_metadata_preserves_wide_dirty_work_and_epoch() {
    let mut stored = StoredPageMeta {
        allocated_bytes: 4096,
        live_bytes: 2048,
        dead_bytes: 2048,
        object_count: 128,
        live_object_count: 64,
        dead_object_count: 64,
        dirty_slots: usize::MAX,
        dirty_slots_epoch: u64::MAX - 1,
        dirty: true,
        ..StoredPageMeta::default()
    };
    stored.refresh_policy_bits();
    let current = stored.snapshot(123, u64::MAX - 1);
    assert_eq!(
        (current.page_base, current.page_end),
        (123 * 4096, 124 * 4096)
    );
    assert_eq!(
        (
            current.allocated_bytes,
            current.live_bytes,
            current.dead_bytes
        ),
        (4096, 2048, 2048)
    );
    assert_eq!(
        (
            current.object_count,
            current.live_object_count,
            current.dead_object_count
        ),
        (128, 64, 64)
    );
    assert_eq!(current.dirty_slots, usize::MAX);
    assert!(current.evacuation_eligible);
    assert_eq!(stored.snapshot(123, u64::MAX).dirty_slots, 0);
    stored.pinned_bytes = 32;
    stored.pinned_object_count = 1;
    stored.refresh_policy_bits();
    assert!(!stored.snapshot(123, 0).evacuation_eligible);
    stored.reset_cycle_sweep_accounting();
    let reset = stored.snapshot(123, 0);
    assert_eq!(
        (reset.pinned_bytes, reset.live_bytes, reset.dead_bytes),
        (0, 0, 0)
    );
    assert_eq!(
        (
            reset.pinned_object_count,
            reset.live_object_count,
            reset.dead_object_count
        ),
        (0, 0, 0)
    );
    assert_eq!((reset.allocated_bytes, reset.object_count), (4096, 128));
}

#[test]
fn stored_metadata_and_dense_object_lists_are_smaller() {
    #[cfg(target_pointer_width = "64")]
    assert_eq!(
        (size_of::<OldPageMeta>(), size_of::<StoredPageMeta>()),
        (104, 56)
    );
    let page = 100;
    let base = generation_page_base(page);
    let mut list = PageObjects::with_capacity(128);
    for i in 0..128 {
        list.push(page, base + 32 * i);
    }
    assert_eq!(list.len(), 128);
    assert_eq!(list.heap_bytes(), 256);
    for i in 0..128 {
        assert_eq!(list.get(page, i), Some(base + 32 * i));
    }
    assert_eq!(list.get(page, 128), None);
}

#[test]
fn entering_headers_preserve_order_prefix_membership_and_hole_reuse() {
    let page = 100;
    let base = generation_page_base(page);
    let mut list = PageObjects::default();
    let mut oracle = vec![base + 16, base + 4088, base - 8, base];
    list.extend(page, &oracle);
    assert_eq!(list.iter(page).collect::<Vec<_>>(), oracle);
    assert!(!list.contains_prefix(page, base - 8, 2));
    assert!(list.contains_prefix(page, base - 8, 3));
    assert!(!list.contains_prefix(page, base - 16, list.len()));
    assert!(!list.contains_prefix(page, base + 4096, list.len()));
    list.swap_remove(2);
    oracle.swap_remove(2);
    assert_eq!(list.entering_header, 0);
    list.push(page, base - 32);
    oracle.push(base - 32);
    assert_eq!(list.iter(page).collect::<Vec<_>>(), oracle);
    list.retain(page, |h| h >= base && h != base + 16);
    oracle.retain(|&h| h >= base && h != base + 16);
    assert_eq!(list.iter(page).collect::<Vec<_>>(), oracle);
    assert_eq!(list.entering_header, 0);
}

#[test]
fn an_entering_header_is_not_limited_to_a_small_signed_offset() {
    let page = (u32::MAX as usize) >> 12;
    let base = generation_page_base(page);
    let mut list = PageObjects::default();
    list.extend(page, &[8, base + 32, base + 4088]);
    assert_eq!(
        list.iter(page).collect::<Vec<_>>(),
        vec![8, base + 32, base + 4088]
    );
    list.retain(page, |h| h == 8);
    assert_eq!(list.get(page, 0), Some(8));
}

#[test]
fn removal_matches_a_pointer_vector_through_repeated_reuse() {
    let page = 512;
    let base = generation_page_base(page);
    let mut list = PageObjects::default();
    let mut oracle = Vec::new();
    for round in 0..32 {
        for slot in 0..256 {
            let addr = base + slot * 16;
            if !oracle.contains(&addr) {
                list.push(page, addr);
                oracle.push(addr);
            }
        }
        let keep = |h: usize| ((h - base) / 16 + round) % 3 != 0;
        list.retain(page, keep);
        oracle.retain(|&h| keep(h));
        assert_eq!(list.iter(page).collect::<Vec<_>>(), oracle);
        while oracle.len() > 8 {
            let index = (round + oracle.len() / 2) % oracle.len();
            oracle.swap_remove(index);
            list.swap_remove(index);
        }
        assert_eq!(list.iter(page).collect::<Vec<_>>(), oracle);
    }
}

#[test]
#[should_panic(expected = "two different objects enter the same page")]
fn overlapping_entering_objects_are_rejected_instead_of_aliasing() {
    let mut list = PageObjects::default();
    list.push(2, 4096);
    list.push(2, 4104);
}
