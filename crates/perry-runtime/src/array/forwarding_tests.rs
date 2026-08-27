//! Array growth-forwarding tests.
//!
//! Split out of `tests.rs` (2000-line-per-file cap). Pure relocation --
//! covers `install_array_growth_forwarding_*` and the `clean_arr_ptr`
//! chain walk (cycle rejection, multi-hop compression, untracked
//! targets).

use std::ptr;

use super::*;

#[test]
fn growth_of_old_array_keeps_forwarding_target_out_of_copying_nursery() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let capacity = MIN_ARRAY_CAPACITY;
    let initial = crate::arena::arena_alloc_gc_old_born_tenured(
        array_byte_size(capacity as usize),
        8,
        crate::gc::GC_TYPE_ARRAY,
    ) as *mut ArrayHeader;

    unsafe {
        (*initial).length = 0;
        (*initial).capacity = capacity;
        let elements = (initial as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut u64;
        for i in 0..capacity as usize {
            // GC_STORE_AUDIT(INIT): initialize unpublished fresh array storage
            // with the non-pointer hole sentinel before exposing the array.
            ptr::write(elements.add(i), crate::value::TAG_HOLE);
        }
        set_array_numeric_layout(initial, NumericArrayLayout::RawF64);
        crate::gc::layout_init_pointer_free(initial as *mut u8);
    }

    let mut head = initial;
    for i in 0..=capacity {
        head = js_array_push_f64(head, i as f64);
    }

    assert_ne!(head, initial, "the capacity-crossing push must grow");
    assert_eq!(clean_arr_ptr_mut(initial), head);
    assert_eq!(
        crate::arena::classify_heap_generation(head as usize),
        crate::arena::HeapGeneration::Old,
        "an old forwarding stub must not point into resetting copying-nursery space"
    );
}

#[test]
fn growth_transfers_array_descriptor_side_tables() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let initial = js_array_alloc_literal(1);
    let first = js_array_grow(initial, 2);
    crate::object::set_accessor_descriptor(
        first as usize,
        "1".to_string(),
        crate::object::AccessorDescriptor { get: 42, set: 84 },
    );
    crate::object::set_property_attrs(
        first as usize,
        "1".to_string(),
        crate::object::PropertyAttrs::new(false, false, true),
    );

    let second = js_array_grow(first, 3);

    let accessor = crate::object::get_accessor_descriptor(second as usize, "1")
        .expect("accessor descriptor must follow array growth");
    assert_eq!(accessor.get, 42);
    assert_eq!(accessor.set, 84);
    let attrs = crate::object::get_property_attrs(second as usize, "1")
        .expect("property attributes must follow array growth");
    assert!(!attrs.writable());
    assert!(!attrs.enumerable());
    assert!(attrs.configurable());
}

#[test]
fn install_array_growth_forwarding_with_installs_stub_for_injected_header() {
    // Actual low-address classification is covered by
    // value::addr_class::tests::tracked_gc_classifier_accepts_injected_low_arena_membership.
    // This test proves the install path uses the injected tracked header.
    const LOW_USER: usize = 0x1_0000_0008;
    const { assert!(LOW_USER < 0x200_0000_0000) };
    let old = js_array_alloc(0);
    let new = js_array_alloc(0);
    unsafe {
        let old_header =
            crate::value::addr_class::try_read_tracked_gc_header(old as usize).unwrap();
        let header_ptr = old_header.as_ptr();
        let flags = (*header_ptr).gc_flags;
        let payload = *(old as *const u64);

        let installed = super::push_pop::install_array_growth_forwarding_with(
            LOW_USER,
            new as *mut u8,
            |candidate| {
                assert_eq!(candidate, LOW_USER);
                Some(old_header)
            },
        );
        let resolved = clean_arr_ptr(old);

        *(old as *mut u64) = payload;
        (*header_ptr).gc_flags = flags;
        assert!(installed);
        assert_eq!(resolved, new);
    }
}

#[test]
fn clean_arr_ptr_rejects_forwarding_cycle() {
    let first = js_array_alloc(0);
    let second = js_array_alloc(0);
    unsafe {
        let first_header = crate::value::addr_class::try_read_tracked_gc_header(first as usize)
            .unwrap()
            .as_ptr();
        let second_header = crate::value::addr_class::try_read_tracked_gc_header(second as usize)
            .unwrap()
            .as_ptr();
        let first_flags = (*first_header).gc_flags;
        let second_flags = (*second_header).gc_flags;
        let first_payload = *(first as *const u64);
        let second_payload = *(second as *const u64);
        crate::gc::set_forwarding_address(first_header, second as *mut u8);
        crate::gc::set_forwarding_address(second_header, first as *mut u8);

        let resolved = clean_arr_ptr(first);

        *(first as *mut u64) = first_payload;
        *(second as *mut u64) = second_payload;
        (*first_header).gc_flags = first_flags;
        (*second_header).gc_flags = second_flags;
        assert!(resolved.is_null());
    }
}

#[test]
fn clean_arr_ptr_compresses_multi_hop_forwarding_chain() {
    let first = js_array_alloc(0);
    let second = js_array_alloc(0);
    let live = js_array_alloc(0);
    unsafe {
        let first_header = crate::value::addr_class::try_read_tracked_gc_header(first as usize)
            .unwrap()
            .as_ptr();
        let second_header = crate::value::addr_class::try_read_tracked_gc_header(second as usize)
            .unwrap()
            .as_ptr();
        let first_flags = (*first_header).gc_flags;
        let second_flags = (*second_header).gc_flags;
        let first_payload = *(first as *const u64);
        let second_payload = *(second as *const u64);
        crate::gc::set_forwarding_address(first_header, second as *mut u8);
        crate::gc::set_forwarding_address(second_header, live as *mut u8);

        let resolved = clean_arr_ptr(first);
        let compressed_target = crate::gc::forwarding_address(first_header);

        *(first as *mut u64) = first_payload;
        *(second as *mut u64) = second_payload;
        (*first_header).gc_flags = first_flags;
        (*second_header).gc_flags = second_flags;
        assert_eq!(resolved, live);
        assert_eq!(
            compressed_target, live as *mut u8,
            "the original stub must point directly at the validated live head"
        );
    }
}

#[test]
fn clean_arr_ptr_rejects_untracked_forwarding_target_without_deref() {
    let array = js_array_alloc(0);
    let unrelated = 0x20_0000usize as *mut u8;
    unsafe {
        let header = crate::value::addr_class::try_read_tracked_gc_header(array as usize)
            .unwrap()
            .as_ptr();
        let flags = (*header).gc_flags;
        let payload = *(array as *const u64);
        crate::gc::set_forwarding_address(header, unrelated);

        let resolved = clean_arr_ptr(array);

        *(array as *mut u64) = payload;
        (*header).gc_flags = flags;
        assert!(resolved.is_null());
    }
}
