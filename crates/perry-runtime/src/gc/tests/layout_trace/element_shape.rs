//! #7480: the element-shape invariant has to survive a real copying minor.
//!
//! The header bit rides `_reserved`, which the collector copies verbatim —
//! that is the whole reason the storage mirrors Phase 4a's dense bit. The
//! *record*, though, is keyed by the array's user address, so it only
//! survives because `transfer_element_shape` hangs off `layout_transfer`.
//! These tests assert the pair, and they assert the collector actually MOVED
//! something first: a copying minor that never ran would make both green
//! while proving nothing (CLAUDE.md's "the gate runs but its subject never
//! did").

use super::*;
use crate::array::{test_element_shape_record_exists, test_serialize};

const CLASS_ELEM: u32 = 0x0007_4810;

fn shaped_instance() -> f64 {
    let obj = crate::object::js_object_alloc(CLASS_ELEM, 2);
    crate::value::js_nanbox_pointer(obj as i64)
}

/// `const rows = []; rows.push(new C()); …` built through the real push
/// funnel, so the invariant is established the way production establishes it.
fn proven_array(count: usize) -> *mut crate::array::ArrayHeader {
    let mut arr = crate::array::js_array_alloc(count as u32);
    for _ in 0..count {
        arr = crate::array::js_array_push_f64(arr, shaped_instance());
    }
    assert_eq!(
        crate::array::js_array_element_shape_class(arr),
        CLASS_ELEM as i32,
        "the push funnel must establish the invariant before the GC test starts"
    );
    arr
}

#[test]
fn test_element_shape_invariant_survives_a_copying_minor() {
    let _serialized = test_serialize();
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let arr = proven_array(4);
    js_shadow_slot_set(0, ptr_bits(arr as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;

    // The subject has to have been live: a run with zero copying minors, or
    // one where the array did not move, proves nothing about the transfer.
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(after, arr as usize, "the array must actually have moved");
    assert!(crate::arena::pointer_in_nursery(after));

    let moved = after as *mut crate::array::ArrayHeader;
    unsafe {
        let header = header_from_user_ptr(after as *const u8);
        assert_ne!(
            (*header)._reserved & crate::gc::GC_ARRAY_ELEMENT_SHAPE,
            0,
            "the header bit rides `_reserved` across the copy"
        );
    }
    assert_eq!(
        crate::array::js_array_element_shape_class(moved),
        CLASS_ELEM as i32,
        "the address-keyed record must have followed the move via layout_transfer"
    );
    assert!(
        test_element_shape_record_exists(after),
        "the record must be keyed by the post-move address"
    );
    assert!(
        !test_element_shape_record_exists(arr as usize),
        "and must not be left addressable under the pre-move key, where a \
         recycled allocation could inherit its identity"
    );
    // A caller still holding the pre-move head must reach the SAME proof, not
    // a second one: the query resolves the forwarding chain before it looks
    // the record up, exactly as the 4a element tiers do
    // (`js_array_refresh_local_head`). If it did not, a stale head would read
    // as unproven and the consumer would deopt for the rest of the loop.
    assert_eq!(
        crate::array::js_array_element_shape_class(arr as *const _),
        CLASS_ELEM as i32,
        "a stale head must resolve through forwarding to the moved proof"
    );
}

#[test]
fn test_element_shape_invariant_keeps_growing_after_a_copying_minor() {
    let _serialized = test_serialize();
    // A moved array must still be *maintainable*, not merely readable: the
    // record has to be reachable at the new key for the store funnel to find
    // it, or the next matching push would clear the proof instead of
    // extending it.
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let arr = proven_array(3);
    js_shadow_slot_set(0, ptr_bits(arr as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);

    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize as *mut crate::array::ArrayHeader;
    assert_ne!(moved as usize, arr as usize);

    let grown = crate::array::js_array_push_f64(moved, shaped_instance());
    assert_eq!(
        crate::array::js_array_element_shape_class(grown),
        CLASS_ELEM as i32,
        "a matching push after the move must keep the proof, not retire it"
    );
    let mismatched = crate::object::js_object_alloc(CLASS_ELEM + 1, 1);
    let grown =
        crate::array::js_array_push_f64(grown, crate::value::js_nanbox_pointer(mismatched as i64));
    assert_eq!(
        crate::array::js_array_element_shape_class(grown),
        0,
        "and a mismatched push after the move must still retire it"
    );
}
