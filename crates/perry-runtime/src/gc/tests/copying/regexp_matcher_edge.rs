//! The `[[RegExpMatcher]]` private slot is an ordinary object field holding
//! the RegExpData edge. Every birth places it inline; a private add after the
//! inline slots are full lands it in the object's spill buffer. Both
//! placements must be traced and rewritten when the instance and the data
//! cell move. Run under `PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1`
//! as well as the default mode.
use super::*;

fn matcher_slot_bits(obj: usize, spilled: bool) -> u64 {
    if spilled {
        {
            let bits = crate::object::test_overflow_field_bits(obj, 2);
            assert_ne!(bits, 0, "matcher must sit in the spill buffer");
            bits
        }
    } else {
        unsafe { *((obj + std::mem::size_of::<crate::ObjectHeader>()) as *const u64) }
    }
}

fn matcher_edge_moves_with(spilled: bool) {
    let _guard = CopyingNurseryTestGuard::new(2);
    let re = crate::regex::test_alloc_nursery_regexp_for_move("edge/source", "gy");
    let data = crate::regex::regexp_data_of(crate::value::js_nanbox_pointer(re as i64))
        .expect("a RegExp birth carries its matcher") as usize;
    let obj = if spilled {
        js_shadow_slot_set(1, ptr_bits(re as usize));
        let obj = crate::object::object_alloc_plain(2);
        js_shadow_slot_set(0, ptr_bits(obj as usize));
        for (i, key) in [b"a" as &[u8], b"b"].into_iter().enumerate() {
            let obj = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::ObjectHeader;
            crate::object::js_object_set_field_by_name(
                obj,
                crate::string::intern_ascii_literal(key),
                i as f64,
            );
        }
        let re = (js_shadow_slot_get(1) & POINTER_MASK) as usize;
        let data = crate::regex::regexp_data_of(crate::value::js_nanbox_pointer(re as i64)).unwrap()
            as usize;
        crate::object::intrinsic_private_add(
            f64::from_bits(js_shadow_slot_get(0)),
            crate::regex::REGEXP_MATCHER,
            crate::value::js_nanbox_pointer(data as i64),
        );
        js_shadow_slot_set(1, 0);
        (js_shadow_slot_get(0) & POINTER_MASK) as usize
    } else {
        js_shadow_slot_set(0, ptr_bits(re as usize));
        re as usize
    };
    let data = crate::regex::regexp_data_of(crate::value::js_nanbox_pointer(obj as i64))
        .map_or(data, |d| d as usize);
    assert_eq!(matcher_slot_bits(obj, spilled) & POINTER_MASK, data as u64);
    assert!(
        crate::arena::pointer_in_nursery(obj),
        "test premise: young instance"
    );
    assert!(
        crate::arena::pointer_in_nursery(data),
        "test premise: young data cell"
    );

    let cycles = crate::gc::copying_minor_cycles();
    let _ = gc_collect_minor();
    assert!(
        crate::gc::copying_minor_cycles() > cycles,
        "test premise: a copying minor ran"
    );

    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(moved, obj, "the instance must be evacuated");
    let moved_data = matcher_slot_bits(moved, spilled) & POINTER_MASK;
    assert_ne!(
        moved_data, data as u64,
        "the RegExpData cell must be evacuated"
    );
    assert_eq!(
        crate::regex::regexp_data_of(crate::value::js_nanbox_pointer(moved as i64))
            .map(|d| d as u64),
        Some(moved_data),
        "the matcher edge must be rewritten to the moved data cell"
    );
    let check = |addr: usize| {
        let source = crate::regex::js_regexp_get_source(addr as *const _);
        assert_eq!(crate::regex::string_as_str(source), r"edge\/source");
        let addr = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
        let flags = crate::regex::original_flags(addr as *const _);
        assert_eq!(crate::regex::string_as_str(flags), "gy");
    };
    check(moved);

    // A full collection (which evacuates old pages under FORCE_EVACUATE)
    // must keep the edge exact as well.
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    let after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let after_data = matcher_slot_bits(after, spilled) & POINTER_MASK;
    assert_eq!(
        crate::regex::regexp_data_of(crate::value::js_nanbox_pointer(after as i64))
            .map(|d| d as u64),
        Some(after_data)
    );
    check(after);
}

#[test]
fn test_regexp_matcher_edge_inline_is_traced_and_rewritten_on_evacuation() {
    matcher_edge_moves_with(false);
}

#[test]
fn test_regexp_matcher_edge_in_spill_is_traced_and_rewritten_on_evacuation() {
    matcher_edge_moves_with(true);
}
