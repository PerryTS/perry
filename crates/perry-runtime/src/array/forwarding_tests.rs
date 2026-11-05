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
        let elements = crate::array::array_elements_ptr(initial as *const ArrayHeader) as *mut u64;
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
fn growth_preserves_the_owned_array_accessor_pair_and_attributes() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let initial = js_array_alloc_literal(1);
    let first = js_array_grow(initial, 2);
    extern "C" fn get_value(
        _closure: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
    ) -> f64 {
        42.0
    }
    extern "C" fn set_value(
        _closure: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
        _value: f64,
    ) -> f64 {
        84.0
    }
    let get = crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(
        crate::fn_info!(get_value, 0),
        0,
    ) as i64)
    .to_bits();
    let set = crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(
        crate::fn_info!(set_value, 1),
        0,
    ) as i64)
    .to_bits();
    crate::object::set_accessor_descriptor(
        first as usize,
        "1".to_string(),
        crate::object::AccessorDescriptor { get, set },
    );
    crate::object::set_property_attrs(
        first as usize,
        "1".to_string(),
        crate::object::PropertyAttrs::new(false, false, true),
    );

    let second = js_array_grow(first, 3);

    let accessor = crate::object::get_accessor_descriptor(second as usize, "1")
        .expect("accessor descriptor must follow array growth");
    assert_eq!(accessor.get, get);
    assert_eq!(accessor.set, set);
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

#[test]
fn growth_preserves_the_custom_prototype_record_when_its_header_bit_is_set() {
    let _latch = crate::test_support::ArrayPrototypeLatchGuard::new();
    let _global = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let proto = crate::object::js_object_alloc(0, 0);
    let initial = js_array_alloc_literal(1);
    let boxed = |p: usize| crate::value::js_nanbox_pointer(p as i64);
    crate::object::js_object_set_prototype_of(boxed(initial as usize), boxed(proto as usize));
    let grown = js_array_grow(initial, 64);
    assert_ne!(grown, initial, "the fixture must replace the array backing");
    assert_eq!(
        crate::object::js_object_get_prototype_of(boxed(grown as usize)).to_bits(),
        boxed(proto as usize).to_bits(),
        "the header ownership bit must carry the actual prototype edge"
    );
}

#[test]
fn retired_growth_payload_pages_are_dead_but_alias_and_gc_reachability_survive() {
    crate::gc::initialize_runtime_fixture();
    let _global = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let scope = crate::gc::RuntimeHandleScope::new();
    let capacity = 8192;
    let original = js_array_alloc(capacity);
    let alias = scope.root_raw_mut_ptr(original);
    let mut head = original;
    for i in 0..capacity {
        head = js_array_push_f64(head, i as f64 + 0.25);
    }
    let grown = js_array_grow(head, capacity + 1);
    assert_ne!(grown, original);
    assert_eq!(clean_arr_ptr_mut(original), grown);
    assert_eq!(
        js_array_get_f64(original, capacity - 1),
        capacity as f64 - 0.75
    );
    #[cfg(target_os = "linux")]
    unsafe {
        let page = libc::sysconf(libc::_SC_PAGESIZE) as usize;
        let dead_start = original as usize + std::mem::size_of::<u64>();
        let first = dead_start.div_ceil(page) * page;
        let end = (original as usize + array_byte_size(capacity as usize)) / page * page;
        assert!(end > first, "fixture contains complete dead payload pages");
        let mut resident = vec![0u8; (end - first) / page];
        assert_eq!(
            libc::mincore(
                first as *mut libc::c_void,
                end - first,
                resident.as_mut_ptr()
            ),
            0
        );
        assert!(
            resident.iter().all(|v| v & 1 == 0),
            "the kernel must release the actual retired payload pages"
        );
    }
    let (_, live) = alias.across_mut(crate::gc::test_collect_synchronous_full_trace);
    let live = clean_arr_ptr_mut(live);
    for i in 0..capacity {
        assert_eq!(js_array_get_f64(live, i), i as f64 + 0.25);
    }
    // A second growth keeps a multi-hop alias usable, with all slot values.
    let newer = js_array_grow(live, unsafe { (*live).capacity + 1 });
    assert_eq!(clean_arr_ptr_mut(live), newer);
    assert_eq!(js_array_get_f64(live, capacity - 1), capacity as f64 - 0.75);
}
