//! #10941: a zero-slot test fixture must still have room for a named store.
//!
//! `alloc_{nursery,old}_test_object(0)` allocated exactly an `ObjectHeader`
//! and left the receiver unstamped, on the reasoning that "the derived bound
//! is 0 either way". A named-property write does not respect that bound: the
//! inline/overflow boundary is
//! `max(object_live_slot_count(obj), INLINE_SLOT_FLOOR)` and the floor is 2,
//! so the first two keys stored into a zero-slot fixture land in inline slots
//! 0 and 1 of an object that has none — THE NEXT CELL.
//!
//! Every caller before PR #10938 only ever set a `[[Prototype]]` on one, so
//! nothing had written a named property to one and the hazard was invisible.
//! It presents as a wrong read now and a SIGSEGV inside an unrelated
//! collection later, which is why this is pinned structurally: the assertions
//! below are about the ALLOCATION, and they cannot pass vacuously.
//!
//! An end-to-end pin — write six named properties to a zero-slot fixture and
//! read them back — was written and then deliberately dropped: without the
//! fix it does not fail, it DUMPS CORE, which under `--test-threads=1` takes
//! the other ~4,200 results in the process with it. The structural assertions
//! redden with a message instead, and they redden for the same reason.

use super::support::{alloc_nursery_test_object, alloc_old_test_object};

unsafe fn payload_bytes(obj: *mut crate::object::ObjectHeader) -> usize {
    let header = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    (*header).size as usize - crate::gc::GC_HEADER_SIZE
}

fn floor_bytes() -> usize {
    std::mem::size_of::<crate::object::ObjectHeader>() + crate::object::INLINE_SLOT_FLOOR * 8
}

#[test]
fn a_zero_slot_nursery_fixture_has_room_for_the_named_store_floor() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let (obj, _) = alloc_nursery_test_object(0);
        let payload = payload_bytes(obj);
        assert!(
            payload >= floor_bytes(),
            "a named-property write on this fixture stores into inline slots 0 \
             and 1 — the store path's floor is \
             `max(object_live_slot_count(obj), INLINE_SLOT_FLOOR)` and never \
             zero — but the allocation carries {payload} payload bytes against \
             the {} it would need. Those words are the NEXT CELL (#10941).",
            floor_bytes()
        );
    }
}

#[test]
fn a_zero_slot_old_fixture_has_room_for_the_named_store_floor() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let (obj, _) = alloc_old_test_object(0);
        let payload = payload_bytes(obj);
        assert!(
            payload >= floor_bytes(),
            "the old-generation twin of the nursery fixture has the same hole: \
             {payload} payload bytes against the {} a named store needs \
             (#10941).",
            floor_bytes()
        );
    }
}
