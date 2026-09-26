//! `ErrorHeader` layout regressions.
//!
//! Split out of `error.rs` to keep that file under the 2,000-line CI
//! cap (`scripts/check_file_size.sh`). Included from there with
//! `#[cfg(test)] #[path = "error_header_tests.rs"] mod header_unification_tests;`,
//! so `use super::*` still resolves against `error.rs`.

use super::*;

/// #6759 phase 1: an `ErrorHeader` owns a metadata edge, and it is
/// reachable through the SAME accessor an `ObjectHeader` is.
///
/// This is the gate the rest of the migration stands on: while "does this
/// cell own an ObjectMeta?" had no uniform answer, per-error state had
/// nowhere to live but a side table keyed by the error's address.
#[test]
fn error_cell_exposes_a_meta_edge_like_an_object() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let msg = crate::string::js_string_from_bytes(b"boom".as_ptr(), 4);
        let err = js_error_new_with_message(msg);
        assert!(
            (*err).meta.is_null(),
            "a fresh error must start with no metadata record"
        );
        assert!(
            crate::object::cell_has_meta_edge(err as usize),
            "an error cell must be reachable through the uniform meta accessor"
        );
        let obj = crate::object::js_object_alloc(0, 0);
        assert!(
            crate::object::cell_has_meta_edge(obj as usize),
            "an object cell must answer the same accessor"
        );
        // A cell type that has NOT been unified yet must answer `None`
        // rather than mis-reading its own layout as a meta pointer.
        let arr = crate::array::js_array_alloc(0);
        assert!(
            !crate::object::cell_has_meta_edge(arr as usize),
            "a cell without a meta edge must report absence, not garbage"
        );
    }
}

/// #10956 (successor): an Error born in the slot a dead closure vacated must
/// not look like a function. With the magic word gone the question is the
/// closure's ShapeId word at +4 — an ErrorHeader's `error_kind` — and the
/// padding after `flags`, which `alloc_error` must scrub. Arena slots are
/// recycled without zeroing, so both are checked on a slot the test PROVES
/// was a dead closure's (a green run cannot mean "fresh zeroed memory").
#[cfg(target_pointer_width = "64")]
#[test]
fn an_error_born_in_a_dead_closure_slot_does_not_inherit_its_shape() {
    use crate::closure::js_closure_alloc;

    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    assert_eq!(
        crate::closure::closure_payload_size(6),
        std::mem::size_of::<ErrorHeader>(),
        "the fixture needs a closure the exact size of an ErrorHeader"
    );
    unsafe {
        // Several dead slots, not one: `alloc_error` allocates its name and
        // frame strings first, and one of those may take a same-size slot.
        let dead: Vec<*mut u8> = (0..8)
            .map(|_| js_closure_alloc(std::ptr::null(), 6) as *mut u8)
            .collect();
        {
            let mut free_list = crate::gc::hot_arena_free_list().borrow_mut();
            for &slot in &dead {
                let header = crate::value::addr_class::try_read_gc_header(slot as usize)
                    .expect("an arena closure carries a GC header");
                free_list.insert(0, (slot, header.size as usize));
            }
        }
        crate::gc::hot_arena_free_list_nonempty().set(true);

        let err = js_error_new();

        {
            let mut free_list = crate::gc::hot_arena_free_list().borrow_mut();
            free_list.retain(|(slot, _)| !dead.contains(slot));
            crate::gc::hot_arena_free_list_nonempty().set(!free_list.is_empty());
        }
        assert!(
            dead.contains(&(err as *mut u8)),
            "the Error must reuse a dead closure's slot, or this test proves nothing"
        );
        let word = *((err as *const u8).add(crate::closure::CLOSURE_SHAPE_OFFSET) as *const u32);
        assert!(
            !crate::object::shapes::is_shape_id(word),
            "a recycled closure's ShapeId leaked into the Error's +4 word: {word:#x}"
        );
        assert!(!crate::closure::is_closure_ptr(err as usize));
        let pad = *((err as *const u8).add(12) as *const u32);
        assert_eq!(pad, 0, "the padding word must be scrubbed, not left as garbage");
    }
}
