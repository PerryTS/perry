//! The weak reverse edge of a canonical key add, owned by its target shape.
//! A last-key delete reinstalls the exact recorded parent, including its live
//! bound and representation. A pruned parent makes this a miss; the edge is
//! neither a side table nor an independent GC carrier.

use super::*;

/// Learn the reverse of an admitted canonical key-add edge. Prefix equality
/// is checked once at the write; all uses still validate the live records.
#[cold]
#[inline(never)]
pub(crate) fn note_last_key_parent(from: u32, parent: u32) {
    unsafe {
        let (Some(from_record), Some(parent_record)) = (
            ShapeSlab::agent_record_present(from),
            ShapeSlab::agent_record_present(parent),
        ) else {
            return;
        };
        let child = &*from_record;
        let predecessor = &*parent_record;
        if !rollback_pair_matches(child, predecessor) {
            return;
        }
        let keys = child.keys as usize as *const ArrayHeader;
        let Some(header) = crate::value::addr_class::try_read_gc_header(keys as usize) else {
            return;
        };
        if header.gc_flags & crate::gc::GC_FLAG_SHAPE_SHARED == 0 {
            return;
        }
        let count = predecessor.logical_key_count as usize;
        if count != 0 {
            let (child_keys, child_len) = crate::object::keys_array_dense_slots_resolved(keys);
            let (parent_keys, parent_len) = crate::object::keys_array_dense_slots_resolved(
                predecessor.keys as usize as *const ArrayHeader,
            );
            if child_keys.is_null()
                || parent_keys.is_null()
                || child_len < count
                || parent_len < count
            {
                return;
            }
            for i in 0..count {
                if (*child_keys.add(i)).to_bits() != (*parent_keys.add(i)).to_bits() {
                    return;
                }
            }
        }
        // Exact-facts interning can converge append edges from two different
        // live parents (for example different birth widths). Such a child
        // has no unique reverse edge. Decline instead of choosing the last
        // writer's parent for another receiver.
        let recorded = (*from_record).rollback_parent();
        let ambiguous = recorded == u32::MAX
            || (recorded != 0
                && recorded != parent
                && ShapeSlab::agent_record_present(recorded).is_some());
        (*from_record).note_rollback_parent(if ambiguous { u32::MAX } else { parent });
    }
}

/// Facts a reverse key-add edge must preserve. In particular, an integer
/// target with the right key count but a different prototype or attributes
/// never authorizes a rollback. Only default data-property lists qualify.
fn rollback_pair_matches(child: &ShapeRecord, parent: &ShapeRecord) -> bool {
    child.logical_key_count > 0
        && parent.logical_key_count + 1 == child.logical_key_count
        && child.object_kind().is_ordinary_layout()
        && child.object_kind() == parent.object_kind()
        && child.proto_id == parent.proto_id
        && child.semantic_generation == parent.semantic_generation
        && child.summary() == 0
        && parent.summary() == 0
        && child.hole_count == 0
        && parent.hole_count == 0
        && parent.live_inline_slot_count <= child.live_inline_slot_count
        && shapes_store::brand_lists_equal(child.brands(), parent.brands())
}

/// Reinstall the exact parent of `obj`'s last canonical key add. The caller
/// has checked configurability and clears the vacated value after success.
///
/// # Safety
/// `obj` is a live object and no allocation intervenes before the clear.
pub(crate) unsafe fn publish_object_shape_last_key_rollback(
    obj: *mut crate::object::ObjectHeader,
    count: u32,
) -> u32 {
    if obj.is_null() || count == 0 || !shape_word_is_writable(obj) {
        return 0;
    }
    let Some(header) = crate::value::addr_class::try_read_gc_header(obj as usize) else {
        return 0;
    };
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header._reserved
            & (crate::gc::OBJ_FLAG_STABLE_TOMBSTONES
                | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
                | crate::gc::OBJ_FLAG_FROZEN
                | crate::gc::OBJ_FLAG_SEALED
                | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO)
            != 0
        || crate::object::dictionary::is_dictionary(obj)
    {
        return 0;
    }
    let Some(current) = ShapeSlab::agent_record_present(object_shape_stamp(obj)) else {
        return 0;
    };
    let to = (*current).rollback_parent();
    let Some(parent) = ShapeSlab::agent_record_present(to) else {
        return 0;
    };
    if (*current).logical_key_count != count || !rollback_pair_matches(&*current, &*parent) {
        return 0;
    }
    // A key add may have generalized a surviving ConstFn lane to Any. Its
    // value can subsequently change without moving the child's shape. Never
    // reinstall the parent's stronger lane claim without checking that value.
    let fields =
        (obj as *const u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *const u64;
    for slot in 0..(*parent)
        .live_inline_slot_count
        .min(crate::object::field_rep::REP_SLOTS)
    {
        let rep = crate::object::field_rep::slot_rep((*parent).rep, slot);
        let bits = *fields.add(slot as usize);
        match rep {
            crate::object::field_rep::REP_ANY => {}
            crate::object::field_rep::REP_F64 | crate::object::field_rep::REP_F64_DEPRECATED => {
                if crate::object::field_rep::f64_slot_bits(bits) != Some(bits) {
                    return 0;
                }
            }
            crate::object::field_rep::REP_SPECIAL => {
                let expected = (*parent)
                    .constfn_infos()
                    .iter()
                    .find(|info| u32::from(info.slot) == slot)
                    .map(|info| info.info);
                if expected.is_none()
                    || crate::object::field_rep_store::constfn_store_info(bits) != expected
                {
                    return 0;
                }
            }
            _ => return 0,
        }
    }
    crate::array::clear_array_subclass_named_prefix_token(obj);
    stamp_object_shape_id_with_carrier_note(obj, to);
    debug_assert_object_shape_parity(obj);
    to
}

#[cfg(test)]
mod tests {
    use super::*;

    // Deliberately replace a learned edge with a similarly sized parent that
    // carries a different prototype or attributes. The cached use MUST vet
    // those facts too, rather than trusting two integers and the key count.
    #[test]
    fn sabotage_wrong_prototype_or_attributes_cannot_authorize_rollback() {
        let _global = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            let obj = crate::object::js_object_alloc_null_proto(0, 2);
            let a = crate::string::intern_ascii_literal(b"edge_guard_a");
            let b = crate::string::intern_ascii_literal(b"edge_guard_b");
            crate::object::js_object_set_field_by_name(obj, a, 1.0);
            let parent = object_shape_stamp(obj);
            crate::object::js_object_set_field_by_name(obj, b, 2.0);
            let child = object_shape_stamp(obj);
            let child_record = ShapeSlab::agent_record_present(child).unwrap();
            assert_eq!(
                (*child_record).rollback_parent(),
                parent,
                "live learned edge"
            );
            let original = *ShapeSlab::agent_record_present(parent).unwrap();
            for attributes in [false, true] {
                let bad = if attributes {
                    original.with_summary(crate::object::key_attrs::SUMMARY_ACCESSOR)
                } else {
                    original.with_proto_id(original.proto_id ^ 1)
                };
                assert!(
                    !rollback_pair_matches(&*child_record, &bad),
                    "sabotage must be detected"
                );
                // Plant the same corrupt facts on the actual target record,
                // so this also exercises the production cached-use guard.
                let target = ShapeSlab::agent_record_present(parent).unwrap();
                *target = bad;
                let result = publish_object_shape_last_key_rollback(obj, 2);
                *target = original;
                assert_eq!(
                    result, 0,
                    "a corrupt parent must refuse, not change the receiver"
                );
                assert_eq!(object_shape_stamp(obj), child);
            }
            assert_eq!(publish_object_shape_last_key_rollback(obj, 2), parent);
        }
    }
}

#[cfg(test)]
mod representation_tests {
    use super::*;

    extern "C" fn body(_: *const crate::closure::ClosureHeader, _: crate::closure::JsThis) -> f64 {
        1.0
    }

    #[test]
    fn rollback_cannot_restore_a_constfn_claim_after_its_surviving_value_changed() {
        let _global = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            let obj = crate::object::js_object_alloc_null_proto(0, 2);
            let a = crate::string::intern_ascii_literal(b"surviving_constfn");
            let b = crate::string::intern_ascii_literal(b"later_key");
            let info = crate::fn_info!(body, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE));
            let closure = crate::closure::js_closure_alloc(info, 0);
            crate::object::js_object_set_field_by_name(
                obj,
                a,
                crate::value::js_nanbox_pointer(closure as i64),
            );
            let parent = object_shape_stamp(obj);
            assert_eq!(
                shape_descriptor_by_id(parent).unwrap().special_constfn_mask,
                1
            );
            crate::object::js_object_set_field_by_name(obj, b, 2.0);
            let child = object_shape_stamp(obj);
            assert_eq!(
                (*ShapeSlab::agent_record_present(child).unwrap()).rollback_parent(),
                parent
            );
            crate::object::js_object_set_field_by_name(obj, a, 17.0);
            assert_eq!(
                object_shape_stamp(obj),
                child,
                "Any survivor overwrite keeps the child"
            );
            assert_eq!(
                publish_object_shape_last_key_rollback(obj, 2),
                0,
                "the parent's old body is no longer valid"
            );
            assert_eq!(object_shape_stamp(obj), child);
        }
    }
}
