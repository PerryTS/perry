//! #6759 C3: what a `delete` does to an object's SHAPE IDENTITY, pinned for
//! both object representations — because rung 1 made them AGREE, and that
//! agreement is the entry gate for the header shrink (#7916).
//!
//! `perry-codegen`'s `class_field_inline_guard` speculates that a receiver's
//! packed slot layout is its class's canonical one. `delete` breaks that
//! (slots after the deleted key shift down one) while PRESERVING
//! `class_id`, so the guard compares the live `keys_array` POINTER against
//! the class's `@perry_class_keys_*` token. Replacing that pointer compare
//! with a one-word ShapeId compare — which is what makes the header shrink
//! a load *removal* instead of a load-for-probe trade — requires a class
//! instance to HAVE a shape word.
//!
//! Before rung 1 it did not: `parent_class_id` was the shape word only when
//! `class_id == 0`, so the sibling test below asserted the ABSENCE of one
//! and named its own replacement. Rung 1 removed the `class_id` gate, so
//! both representations now mint a fresh id across a delete and these tests
//! state the same property twice, once per representation.
//!
//! Rung 1 is deliberately runtime-only: the guard has NOT switched to the
//! id compare (that is rung 3), and codegen's inline `new C()` still writes
//! a constant `parent_cid`, so an instance is stamped LAZILY at its first
//! by-name resolve rather than at birth (rung 2). The tests below therefore
//! resolve a field before reading the stamp — a fresh instance legitimately
//! reads as unstamped.
use super::*;
use crate::object::shapes::is_shape_id;

fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

/// A plain object's `delete` mints a new authoritative ShapeId eagerly. The
/// stale slot index is dropped, the stamp is cleared, then the compacted
/// facts install a genuinely fresh id rather than reviving the old one.
/// Ids are never reused, so "different" is the whole property.
#[test]
fn delete_mints_a_fresh_shape_id_for_a_plain_object() {
    // This test is specifically about the identity transition caused by
    // slot compaction. Default-on tombstones intentionally preserve slot
    // placement, so pin the compacting lane instead of making the shape
    // assertion vacuous against a different structural operation.
    let _tombstones = test_scope_tombstone_deletes(false);
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = crate::object::js_object_alloc(0, 8);
        for (name, value) in [("del6759_a", 1.0), ("del6759_b", 2.0), ("del6759_c", 3.0)] {
            crate::object::js_object_set_field_by_name(obj, key(name), value);
        }
        let _ = crate::object::js_object_get_field_by_name(obj, key("del6759_b"));
        let before = (*obj).parent_class_id;
        assert!(
            is_shape_id(before),
            "fixture is vacuous — no shape stamp to transition (got {before:#x})"
        );

        assert_eq!(js_object_delete_field(obj, key("del6759_a")), 1);
        assert_eq!(
            f64::from_bits(js_object_get_field(obj, 1).bits()),
            3.0,
            "test premise: the delete did not compact the slots"
        );

        // The compacted descriptor is installed before delete returns.
        let after = (*obj).parent_class_id;
        assert!(
            is_shape_id(after),
            "no shape id re-minted after the delete (got {after:#x})"
        );
        assert_ne!(
            after, before,
            "the delete re-used the pre-delete ShapeId — a shape-id compare \
             would accept a compacted object as its own class's shape"
        );
        let descriptor = crate::object::shapes::shape_descriptor_by_id(after)
            .expect("delete must publish a by-id descriptor");
        assert_eq!(
            descriptor.keys,
            crate::object::object_keys(obj).arr() as u64
        );
        assert_eq!(descriptor.logical_key_count, 2);
        assert_eq!(
            descriptor.live_inline_slot_count,
            crate::object::object_live_slot_count(obj)
        );
    }
}

/// #6759 C3 rung 1 — the replacement the pre-rung-1 test named.
///
/// This is the assertion `delete_leaves_a_class_instance_with_no_shape_word_to_transition`
/// asked to be replaced by. A class instance now HAS a shape word, so a
/// `delete` on it transitions the same way a plain object's does: the
/// compaction clears the stamp, then installs a genuinely fresh id before
/// returning (ids are never reused) rather than reviving the class's
/// canonical one.
///
/// The old test's other half — that `class_id` is preserved and the keys
/// POINTER moves — is kept below, because both are still true and both are
/// still what `class_field_inline_guard` compares until rung 3.
#[test]
fn delete_mints_a_fresh_shape_id_for_a_class_instance() {
    // Keep the compacting precondition explicit: the default tombstone
    // path also mints a successor id for class instances, but deliberately
    // leaves surviving fields in their original slots.
    let _tombstones = test_scope_tombstone_deletes(false);
    let _lock = crate::gc::global_side_table_test_lock();
    const CID: u32 = 0x0C3C_6760;
    const PARENT: u32 = 0x0C3C_6761;
    let packed = b"del6759_x\0del6759_y\0del6759_z";
    unsafe {
        let obj = crate::object::js_object_alloc_class_with_keys(
            CID,
            PARENT,
            3,
            packed.as_ptr(),
            packed.len() as u32,
        );
        for (i, v) in [10.0f64, 20.0, 30.0].iter().enumerate() {
            js_object_set_field(obj, i as u32, JSValue::from_bits(v.to_bits()));
        }
        let keys_before_view = crate::object::object_keys(obj);
        let keys_before = keys_before_view.arr();
        assert_eq!((*obj).class_id, CID, "test premise: a class instance");
        let before = (*obj).parent_class_id;
        assert!(
            is_shape_id(before),
            "test premise: a class instance is stamped AT BIRTH (got \
             {before:#x}). Rung 2 (#8009 for the compiled path, and the \
             runtime allocators alongside it) exists because a LAZY stamp \
             splits the shape's population — see \
             `shapes::birth_stamp_object_shape`"
        );
        // A by-name resolve must not change it: the birth stamp is already
        // the id every later resolve would have minted.
        let _ = crate::object::js_object_get_field_by_name(obj, key("del6759_y"));
        assert_eq!(
            (*obj).parent_class_id,
            before,
            "a resolve re-stamped an already-stamped instance with a \
             DIFFERENT id — every site holding the birth token would miss"
        );

        assert_eq!(js_object_delete_field(obj, key("del6759_x")), 1);

        // The compaction really happened: `z` moved from slot 2 to slot 1.
        assert_eq!(
            f64::from_bits(js_object_get_field(obj, 1).bits()),
            30.0,
            "test premise: the delete did not compact the slots"
        );
        // The compacted descriptor is installed before delete returns.
        let after = (*obj).parent_class_id;
        assert!(
            is_shape_id(after),
            "no shape id re-minted after the delete (got {after:#x})"
        );
        assert_ne!(
            after, before,
            "the delete re-used the pre-delete ShapeId — a shape-id compare \
             would accept a compacted class instance as its own class's shape"
        );
        let descriptor = crate::object::shapes::shape_descriptor_by_id(after)
            .expect("class delete must publish a by-id descriptor");
        assert_eq!(
            descriptor.keys,
            crate::object::object_keys(obj).arr() as u64
        );
        assert_eq!(descriptor.logical_key_count, 2);
        assert_eq!(
            descriptor.live_inline_slot_count,
            crate::object::object_live_slot_count(obj)
        );

        // Still true, and still what the guard compares until rung 3.
        assert_ne!(
            crate::object::object_keys(obj).arr(),
            keys_before,
            "the keys pointer is the guard's compaction evidence and it did not change"
        );
        assert_eq!(
            (*obj).class_id,
            CID,
            "class_id must survive a delete — it is the vtable/instanceof identity"
        );
    }
}

/// Two pristine instances of the same class share ONE ShapeId (their
/// canonical keys array is shared), and a delete on one moves only that
/// one. This is the property a rung-3 id compare rests on, stated
/// separately from the mint test so a regression names which half broke.
#[test]
fn class_siblings_share_one_shape_id_until_one_is_deleted_from() {
    let _lock = crate::gc::global_side_table_test_lock();
    const CID: u32 = 0x0C3C_6762;
    let packed = b"sib6759_a\0sib6759_b\0sib6759_c";
    unsafe {
        let mk = || {
            crate::object::js_object_alloc_class_with_keys(
                CID,
                0,
                3,
                packed.as_ptr(),
                packed.len() as u32,
            )
        };
        let a = mk();
        let b = mk();
        let _ = crate::object::js_object_get_field_by_name(a, key("sib6759_b"));
        let _ = crate::object::js_object_get_field_by_name(b, key("sib6759_b"));
        let id_a = (*a).parent_class_id;
        let id_b = (*b).parent_class_id;
        assert!(
            is_shape_id(id_a) && is_shape_id(id_b),
            "both must be stamped"
        );
        assert_eq!(
            id_a, id_b,
            "same-class siblings must share one ShapeId — otherwise an \
             id-comparing PIC is monomorphic-per-OBJECT and never hits"
        );

        assert_eq!(js_object_delete_field(a, key("sib6759_a")), 1);
        let _ = crate::object::js_object_get_field_by_name(a, key("sib6759_c"));
        assert_ne!(
            (*a).parent_class_id,
            id_b,
            "the compacted instance kept its siblings' ShapeId"
        );
        assert_eq!(
            (*b).parent_class_id,
            id_b,
            "the untouched sibling's stamp moved — a delete is not a \
             class-wide shape transition"
        );
    }
}

/// #6759 C3 rung 1 risk check: stamping OVERWRITES the header's
/// `parent_class_id`, so the class-parent chain must be served entirely by
/// the class-id-keyed registry (rung 0 / #7981 removed the last header
/// reader). A 3-level chain must still resolve after every level's header
/// word has been clobbered by a stamp.
#[test]
fn a_stamped_class_instance_still_resolves_a_three_level_parent_chain() {
    let _lock = crate::gc::global_side_table_test_lock();
    const BASE: u32 = 0x0C3C_6770;
    const MID: u32 = 0x0C3C_6771;
    const LEAF: u32 = 0x0C3C_6772;
    let packed = b"chain6759_p\0chain6759_q";
    unsafe {
        let leaf = crate::object::js_object_alloc_class_with_keys(
            LEAF,
            MID,
            2,
            packed.as_ptr(),
            packed.len() as u32,
        );
        // The registry edges are registered by the allocator (codegen's
        // inline `new C()` path registers them from the module-init
        // prelude instead); MID→BASE has no instance here, so register it
        // the way that prelude does.
        crate::object::register_class(MID, BASE);
        // Rung 2: the word is a ShapeId from BIRTH, so the parent edge is
        // already only in the registry before anything below runs. That
        // makes this test stronger than when the word still started as
        // inheritance data — there is no window in which it was correct.
        assert!(
            is_shape_id((*leaf).parent_class_id),
            "test premise: the newborn's header word was not clobbered by a \
             birth stamp, so the chain is not actually being stressed"
        );

        let boxed = crate::value::js_nanbox_pointer(leaf as i64);
        let truthy = |v: f64| crate::value::js_is_truthy(v) != 0;
        assert!(truthy(crate::object::js_instanceof(boxed, LEAF)));
        assert!(truthy(crate::object::js_instanceof(boxed, MID)));
        assert!(truthy(crate::object::js_instanceof(boxed, BASE)));

        // …and it stays clobbered across a resolve.
        let _ = crate::object::js_object_get_field_by_name(leaf, key("chain6759_p"));
        assert!(
            is_shape_id((*leaf).parent_class_id),
            "test premise: the word stopped being a stamp, so nothing is being tested"
        );

        assert!(
            truthy(crate::object::js_instanceof(boxed, LEAF)),
            "own class lost after stamping"
        );
        assert!(
            truthy(crate::object::js_instanceof(boxed, MID)),
            "direct parent lost after the header word was overwritten — the \
             parent edge is not coming from the registry"
        );
        assert!(
            truthy(crate::object::js_instanceof(boxed, BASE)),
            "grandparent lost after the header word was overwritten"
        );
    }
}
