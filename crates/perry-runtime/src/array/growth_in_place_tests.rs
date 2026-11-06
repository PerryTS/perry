//! Growth in place (`push_pop/growth.rs::try_extend_in_place`) and the
//! no-collect grow the inline push tier calls (`js_array_grow_for_push`).
//! Every test holds `GrowInPlaceTestGuard` (unit tests copy by default).
use super::*;

fn header_of(arr: *const ArrayHeader) -> *mut crate::gc::GcHeader {
    unsafe { super::header::array_gc_header(arr) }.expect("a live array head")
}

/// Fill `arr` to its capacity with `base + i` and return the (possibly new) head.
fn fill_to_capacity(mut arr: *mut ArrayHeader, base: f64) -> *mut ArrayHeader {
    unsafe {
        while (*arr).length < (*arr).capacity {
            let i = (*arr).length;
            arr = js_array_push_f64(arr, base + f64::from(i));
        }
    }
    arr
}

#[test]
fn the_last_nursery_birth_grows_where_it_stands() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _in_place = GrowInPlaceTestGuard::on();
    let arr = fill_to_capacity(js_array_alloc(16), 100.0);
    unsafe {
        let old_capacity = (*arr).capacity;
        let old_elements = array_elements_ptr(arr);
        let grown = js_array_grow_for_push(arr);
        assert_eq!(grown, arr, "the most recent birth extends in place");
        assert!((*arr).capacity > old_capacity);
        assert_eq!(
            array_elements_ptr(arr),
            old_elements,
            "the element base does not move"
        );
        assert_eq!(array_physical_capacity(arr), (*arr).capacity as usize);
        assert_eq!((*header_of(arr)).gc_flags & crate::gc::GC_FLAG_FORWARDED, 0);
        for i in 0..old_capacity {
            assert_eq!(js_array_get_f64(arr, i), 100.0 + f64::from(i));
        }
        for i in old_capacity..(*arr).capacity {
            assert_eq!(*old_elements.add(i as usize), crate::value::TAG_HOLE);
        }
        // The next birth starts after the widened cell, never inside it.
        let cell_end = header_of(arr) as usize + (*header_of(arr)).size as usize;
        let next = js_array_alloc(4);
        assert!(header_of(next) as usize >= cell_end);
        // Writing the whole new capacity leaves the next cell intact.
        let filled = fill_to_capacity(arr, 100.0);
        assert_eq!(filled, arr);
        assert_eq!((*next).length, 0);
        assert_eq!((*next).capacity, 4);
        assert_eq!(js_array_length(arr), (*arr).capacity);
    }
}

#[test]
fn a_birth_with_a_later_neighbour_moves_and_its_alias_follows() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _in_place = GrowInPlaceTestGuard::on();
    let arr = fill_to_capacity(js_array_alloc(16), 0.0);
    let _neighbour = js_array_alloc(4);
    let grown = js_array_grow_for_push(arr);
    assert!(!grown.is_null());
    assert_ne!(grown, arr, "not the last birth: the copy path");
    unsafe {
        assert_ne!((*header_of(arr)).gc_flags & crate::gc::GC_FLAG_FORWARDED, 0);
        assert!((*grown).capacity > (*grown).length);
        // The stale head resolves to the replacement, and a push through it
        // lands in the one array.
        assert_eq!(clean_arr_ptr(arr), grown as *const ArrayHeader);
        js_array_push_f64(arr, 77.0);
        assert_eq!(js_array_length(grown), 17);
        assert_eq!(js_array_get_f64(grown, 16), 77.0);
        assert_eq!(js_array_get_f64(arr, 3), 3.0);
    }
}

#[test]
fn grow_for_push_declines_what_the_inline_tier_did_not_admit() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _in_place = GrowInPlaceTestGuard::on();
    // Room left: nothing to do, so the inline tier never asks; decline.
    let roomy = js_array_alloc(16);
    js_array_push_f64(roomy, 1.0);
    assert!(js_array_grow_for_push(roomy).is_null());
    unsafe {
        assert_eq!((*roomy).capacity, 16);
    }
    // Frozen / sealed / non-extensible: the complete push must throw.
    for flag in [
        crate::gc::OBJ_FLAG_FROZEN,
        crate::gc::OBJ_FLAG_SEALED,
        crate::gc::OBJ_FLAG_NO_EXTEND,
    ] {
        let locked = fill_to_capacity(js_array_alloc(4), 0.0);
        unsafe {
            let header = header_of(locked);
            (*header)._reserved |= flag;
            assert!(js_array_grow_for_push(locked).is_null());
            assert_eq!((*locked).capacity, 4);
            (*header)._reserved &= !flag;
        }
    }
    // A growth stub is not an admitted head.
    let moved = fill_to_capacity(js_array_alloc(4), 0.0);
    let _neighbour = js_array_alloc(4);
    let replacement = js_array_grow(moved, 5);
    assert_ne!(replacement, moved);
    assert!(js_array_grow_for_push(moved).is_null());
    // Not an array at all.
    assert!(js_array_grow_for_push(std::ptr::null_mut()).is_null());
}

#[test]
fn an_old_source_is_declined_by_the_no_collect_grow() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _in_place = GrowInPlaceTestGuard::on();
    let old = crate::arena::arena_alloc_gc_old_born_tenured(
        array_byte_size(4),
        8,
        crate::gc::GC_TYPE_ARRAY,
    ) as *mut ArrayHeader;
    unsafe {
        (*old).length = 4;
        (*old).capacity = 4;
        // GC_STORE_AUDIT(INIT): unpublished old array has no named layout.
        (*old).named = std::ptr::null_mut();
        for i in 0..4 {
            // GC_STORE_AUDIT(INIT): unpublished old array, numeric elements.
            array_elements_ptr(old)
                .add(i)
                .write(f64::from(i as u32).to_bits());
        }
        crate::gc::layout_init_pointer_free(old.cast());
        assert!(js_array_grow_for_push(old).is_null());
        assert_eq!((*old).capacity, 4);
        // The complete grow still serves it (old-born replacement).
        let grown = js_array_grow(old, 5);
        assert!((*grown).capacity >= 5);
        assert_eq!(js_array_get_f64(grown, 3), 3.0);
    }
}

#[test]
fn without_the_test_opt_in_growth_copies_as_stub_fixtures_expect() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let arr = fill_to_capacity(js_array_alloc(16), 0.0);
    let grown = js_array_grow_for_push(arr);
    assert!(!grown.is_null());
    assert_ne!(grown, arr);
    unsafe {
        assert_ne!((*header_of(arr)).gc_flags & crate::gc::GC_FLAG_FORWARDED, 0);
    }
}
