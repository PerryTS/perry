//! Integrity is an immutable shape fact and a weak shape-owned transition.
//! Like a key-add edge, reuse resolves both IDs, checks the exact predecessor,
//! and installs the existing successor. It never owns a GC pointer.
use super::*;

pub(crate) const INTEGRITY_FLAGS: u16 =
    crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND;

fn level(flags: u16) -> usize {
    if flags & crate::gc::OBJ_FLAG_FROZEN != 0 {
        2
    } else if flags & crate::gc::OBJ_FLAG_SEALED != 0 {
        1
    } else {
        0
    }
}

fn edge_matches(from: &ShapeDescriptor, to: &ShapeDescriptor, flags: u16) -> bool {
    to.integrity_flags == flags
        && to.logical_key_count == from.logical_key_count
        && to.live_inline_slot_count == from.live_inline_slot_count
        && to.hole_count == from.hole_count
        && to.semantic_generation == from.semantic_generation
        && to.object_kind == from.object_kind
        && to.proto_id == from.proto_id
        && to.rep == from.rep
        && to.special_constfn_mask == from.special_constfn_mask
        && to.constfn_infos() == from.constfn_infos()
        && shapes_store::brand_lists_equal(to.brands(), from.brands())
}

/// Apply the shared integrity algorithm to an ordinary holder. Header flags
/// remain the existing write-path mirror; the published shape is authority.
/// The cold rebuild uses the same canonical attribute/key-add machinery as
/// descriptor edits. Dictionary holders keep their private list.
///
/// # Safety
/// The receiver is live; an allocating miss runs in a no-move window.
pub(crate) unsafe fn transition_object_integrity(
    obj: *mut crate::object::ObjectHeader,
    requested_flags: u16,
) -> bool {
    if obj.is_null() || !shape_word_is_writable(obj) {
        return false;
    }
    if crate::object::overflow::has_overflow(obj) {
        return transition_owned_integrity(obj, requested_flags);
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    let Some(from) = object_shape_descriptor(obj) else {
        return false;
    };
    let flags = from.integrity_flags | (requested_flags & INTEGRITY_FLAGS);
    let gc = crate::object::gc_header_for(obj);
    if flags == from.integrity_flags {
        (*gc)._reserved |= flags;
        return true;
    }
    let predecessor = object_shape_stamp(obj);
    let edge = level(flags);
    // Owned lists can change under a stable tombstone ID. Only an immutable
    // prefix can authorize an edge learned before such a later mutation.
    let shared = from.keys == 0 || keys_prefix_is_immutable(from.keys as *const ArrayHeader);
    if shared {
        let source = ShapeSlab::agent_record_present(predecessor).unwrap();
        let target = (*source).integrity_target(edge);
        if let Some(to) = shape_descriptor_by_id(target) {
            if edge_matches(&from, &to, flags)
                && install_cached_object_shape_transition(obj, predecessor, target, to.keys_view())
            {
                (*gc)._reserved |= flags;
                if flags & (crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_SEALED) != 0 {
                    (*gc)._reserved |= crate::gc::OBJ_FLAG_HAS_DESCRIPTORS;
                }
                crate::object::prop_plan::prop_plan_epoch_bump_for_owner(obj as usize);
                return true;
            }
        }
    }
    let restrict = flags & (crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_SEALED) != 0;
    let edit = crate::object::key_attrs::AttrsEdit::Integrity {
        freeze: flags & crate::gc::OBJ_FLAG_FROZEN != 0,
    };
    let keys = if restrict {
        let proof = crate::object::canonical_keys::SharedLayout::of_receiver(obj).unwrap();
        crate::object::canonical_keys::rebuild_with_entries(
            &proof,
            from.keys_view(),
            0,
            |_, slot, old| {
                if slot.is_any_string()
                    || slot.is_pointer()
                        && crate::symbol::js_is_symbol(f64::from_bits(slot.bits())) != 0
                {
                    edit.apply(old)
                } else {
                    old
                }
            },
        )
        .view()
    } else {
        from.keys_view()
    };
    // Preserve every representation/body fact: integrity changes no values.
    let target = publish_shape_result(shape_descriptor_intern_with_special(
        keys.arr(),
        keys.count(),
        from.live_inline_slot_count,
        from.semantic_generation,
        from.object_kind,
        from.hole_count,
        from.proto_id,
        receiver_extra_summary(obj)
            | crate::object::key_attrs::keys_summary_checked(keys.arr(), keys.count()),
        from.rep,
        &from.constfn_infos().to_vec(),
        &from.brands().to_vec(),
        None,
        flags,
    ));
    stamp_object_shape_id_with_carrier_note(obj, target);
    (*gc)._reserved |= flags;
    if restrict {
        (*gc)._reserved |= crate::gc::OBJ_FLAG_HAS_DESCRIPTORS;
    }
    crate::object::prop_plan::prop_plan_epoch_bump_for_owner(obj as usize);
    if shared {
        if let Some(source) = ShapeSlab::agent_record_present(predecessor) {
            (*source).note_integrity_target(edge, target);
        }
    }
    debug_assert_object_shape_parity(obj);
    true
}

/// Owned entries and header flags are finalized before one exact publication.
/// Private tables do not get shared integrity edges.
unsafe fn transition_owned_integrity(
    obj: *mut crate::object::ObjectHeader,
    requested_flags: u16,
) -> bool {
    let Some(from) = object_shape_descriptor(obj) else {
        return false;
    };
    if from.object_kind != ShapeObjectKind::Ordinary
        || (*obj).class_id == crate::object::NATIVE_MODULE_CLASS_ID
    {
        return false;
    }
    let meta = (*obj).meta;
    if meta.is_null()
        || (*meta).elements != 0
        || (*meta).flags
            & (crate::object::OBJECT_META_FLAG_IS_PROTOTYPE
                | crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER
                | crate::object::OBJECT_META_FLAG_NATIVE_ALIAS
                | crate::object::meta_flags::OBJECT_META_FLAG_SEALED_INTRINSIC)
            != 0
        || crate::object::exotic_expando::exotic_expando_kind(obj as usize).is_some()
    {
        return false;
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    let flags = from.integrity_flags | (requested_flags & INTEGRITY_FLAGS);
    let gc = crate::object::gc_header_for(obj);
    if flags == from.integrity_flags {
        // GC_STORE_AUDIT(POINTER_FREE): existing scalar header-flag mirror.
        (*gc)._reserved |= flags;
        return true;
    }
    let restrict = flags & (crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_SEALED) != 0;
    if restrict {
        crate::object::key_attrs::restrict_owned_integrity(
            obj,
            flags & crate::gc::OBJ_FLAG_FROZEN != 0,
        );
        // GC_STORE_AUDIT(POINTER_FREE): scalar descriptor classification.
        (*gc)._reserved |= crate::gc::OBJ_FLAG_HAS_DESCRIPTORS;
    }
    // GC_STORE_AUDIT(POINTER_FREE): final scalar integrity-flag mirror.
    (*gc)._reserved |= flags;
    // Mint an exact private successor through the same publication as shared
    // integrity, with the existing semantic generation and every value fact.
    let generation = SHAPE_SEMANTIC_NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if generation == 0 {
        shape_id_exhausted_abort();
    }
    let target = publish_shape_result(shape_descriptor_intern_with_special(
        from.keys as usize as *const ArrayHeader,
        from.logical_key_count,
        from.live_inline_slot_count,
        generation,
        from.object_kind,
        from.hole_count,
        from.proto_id,
        receiver_extra_summary(obj)
            | crate::object::key_attrs::keys_summary_checked(
                from.keys as usize as *const ArrayHeader,
                from.logical_key_count,
            ),
        from.rep,
        &from.constfn_infos().to_vec(),
        &from.brands().to_vec(),
        None,
        flags,
    ));
    stamp_object_shape_id_with_carrier_note(obj, target);
    crate::object::prop_plan::prop_plan_epoch_bump_for_owner(obj as usize);
    debug_assert_object_shape_parity(obj);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn receiver(value: f64) -> *mut crate::object::ObjectHeader {
        let obj = crate::object::js_object_alloc_null_proto(0, 2);
        let key = crate::string::intern_ascii_literal(b"integrity_edge");
        crate::object::js_object_set_field_by_name(obj, key, value);
        obj
    }

    #[test]
    fn same_shape_reuses_integrity_successor_and_preserves_representation() {
        let _global = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            for flags in [
                crate::gc::OBJ_FLAG_NO_EXTEND,
                crate::gc::OBJ_FLAG_NO_EXTEND | crate::gc::OBJ_FLAG_SEALED,
                INTEGRITY_FLAGS,
            ] {
                let a = receiver(3.0);
                let b = receiver(5.0);
                let before = object_shape_descriptor(a).unwrap();
                assert_eq!(object_shape_stamp(a), object_shape_stamp(b));
                assert!(transition_object_integrity(a, flags));
                let target = object_shape_stamp(a);
                let frozen = object_shape_descriptor(a).unwrap();
                assert_eq!(frozen.integrity_flags, flags);
                assert_eq!(frozen.rep, before.rep);
                assert_eq!(frozen.proto_id, before.proto_id);
                assert!(transition_object_integrity(b, flags));
                assert_eq!(object_shape_stamp(b), target);
                assert!(transition_object_integrity(b, flags));
                assert_eq!(object_shape_stamp(b), target);
            }
        }
    }

    #[test]
    fn shared_transition_guard_rejects_a_wrong_integrity_target() {
        let _global = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            let a = receiver(7.0);
            let b = receiver(11.0);
            let predecessor = object_shape_stamp(a);
            let wrong = receiver(13.0);
            transition_object_integrity(wrong, crate::gc::OBJ_FLAG_NO_EXTEND);
            let wrong_id = object_shape_stamp(wrong);
            transition_object_integrity(a, INTEGRITY_FLAGS);
            let source = ShapeSlab::agent_record_present(predecessor).unwrap();
            (*source).note_integrity_target(2, wrong_id);
            transition_object_integrity(b, INTEGRITY_FLAGS);
            let result = object_shape_descriptor(b).unwrap();
            assert_ne!(
                object_shape_stamp(b),
                wrong_id,
                "wrong integrity edge must decline"
            );
            assert_eq!(result.integrity_flags, INTEGRITY_FLAGS);
            assert_eq!(
                crate::object::key_attrs::keys_entry(result.keys as *const ArrayHeader, 0)
                    & (crate::object::key_attrs::ENTRY_NON_WRITABLE
                        | crate::object::key_attrs::ENTRY_NON_CONFIGURABLE),
                crate::object::key_attrs::ENTRY_NON_WRITABLE
                    | crate::object::key_attrs::ENTRY_NON_CONFIGURABLE
            );
        }
    }

    extern "C" fn holder_function(
        _: *const crate::closure::ClosureHeader,
        _: crate::closure::JsThis,
    ) -> f64 {
        1.0
    }

    #[test]
    fn array_and_function_integrity_is_published_on_their_named_holder_shapes() {
        // Array and function bags are born from registered runtime layouts,
        // which the agent state registers when it is created (cfg(test)).
        // Nothing before this body touches the state, so create it first.
        let _agent = crate::state::state();
        let _global = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            for flags in [
                crate::gc::OBJ_FLAG_NO_EXTEND,
                crate::gc::OBJ_FLAG_NO_EXTEND | crate::gc::OBJ_FLAG_SEALED,
                INTEGRITY_FLAGS,
            ] {
                let array = crate::array::js_array_alloc(1);
                let array_bag = crate::array::array_property_bag_ensure(array);
                let function =
                    crate::closure::js_closure_alloc(crate::fn_info!(holder_function, 0), 0);
                let function_bag = crate::closure::props::bag_ensure(function as usize);
                for (owner, bag) in [
                    (array as usize, array_bag),
                    (function as usize, function_bag),
                ] {
                    let value = crate::value::js_nanbox_pointer(owner as i64);
                    if flags & crate::gc::OBJ_FLAG_FROZEN != 0 {
                        crate::object::js_object_freeze(value);
                    } else if flags & crate::gc::OBJ_FLAG_SEALED != 0 {
                        crate::object::js_object_seal(value);
                    } else {
                        crate::object::js_object_prevent_extensions(value);
                    }
                    assert_eq!(
                        object_shape_descriptor(bag).unwrap().integrity_flags,
                        flags,
                        "named stores must use the same integrity authority"
                    );
                }
            }
        }
    }

    #[test]
    fn pruned_integrity_target_is_reminted_before_use() {
        let _global = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            let a = receiver(17.0);
            let b = receiver(19.0);
            let source = ShapeSlab::agent_record_present(object_shape_stamp(a)).unwrap();
            (*source).note_integrity_target(2, u32::MAX);
            assert!(transition_object_integrity(b, INTEGRITY_FLAGS));
            assert!(shape_descriptor_by_id(object_shape_stamp(b)).is_some());
        }
    }
}

#[cfg(test)]
mod owned_tests {
    use super::*;
    #[test]
    fn owned_integrity_accepts_final_facts_and_is_idempotent() {
        crate::gc::initialize_runtime_fixture();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            for flags in [
                crate::gc::OBJ_FLAG_NO_EXTEND,
                crate::gc::OBJ_FLAG_NO_EXTEND | crate::gc::OBJ_FLAG_SEALED,
                INTEGRITY_FLAGS,
            ] {
                let object = crate::object::js_object_alloc(0, 2);
                crate::object::mark_object_plain_ordinary(object);
                let key = crate::string::js_string_pool_atom(
                    b"integrity_owned".as_ptr(),
                    15,
                    crate::object::key_bytes_hash(b"integrity_owned".as_ptr(), 15),
                    0,
                );
                crate::object::js_object_set_field_by_name(object, key, 7.0);
                let proto = crate::object::js_object_alloc(0, 0);
                crate::object::js_object_set_prototype_of(
                    crate::value::js_nanbox_pointer(object as i64),
                    crate::value::js_nanbox_pointer(proto as i64),
                );
                transition_object_shape_add_brand(object, (1u64 << 63) | 90271);
                let table = crate::object::overflow::ensure(object);
                assert!(!table.is_null());
                let before = object_shape_descriptor(object).unwrap();
                assert_eq!(before.object_kind, ShapeObjectKind::Ordinary);
                assert_eq!(before.brands().len(), 1);
                assert_ne!(before.proto_id, PROTO_ID_DEFAULT);
                let brands = before.brands().to_vec();
                assert!(
                    transition_object_integrity(object, flags),
                    "the owned-table operation must accept, rather than the legacy full path"
                );
                let after = object_shape_descriptor(object).unwrap();
                assert_eq!(after.integrity_flags, flags);
                assert_eq!(after.live_inline_slot_count, before.live_inline_slot_count);
                assert_eq!(after.rep, before.rep);
                assert_eq!(after.proto_id, before.proto_id);
                assert_eq!(after.brands(), brands.as_slice());
                let table = crate::object::overflow::table(object);
                let atom = crate::string::property_atom(key);
                let i = crate::object::overflow::find(
                    table,
                    crate::JSValue::string_ptr(atom.cast_mut()).bits(),
                )
                .unwrap();
                let attrs = crate::object::overflow::entry_attrs(table.cast(), i);
                assert_eq!(
                    attrs & crate::object::key_attrs::ENTRY_NON_CONFIGURABLE != 0,
                    flags & crate::gc::OBJ_FLAG_SEALED != 0
                );
                assert_eq!(
                    attrs & crate::object::key_attrs::ENTRY_NON_WRITABLE != 0,
                    flags & crate::gc::OBJ_FLAG_FROZEN != 0
                );
                assert_eq!(
                    crate::object::overflow::get(object, i),
                    Some(7.0f64.to_bits())
                );
                let stamp = object_shape_stamp(object);
                assert!(transition_object_integrity(object, flags));
                assert_eq!(
                    object_shape_stamp(object),
                    stamp,
                    "repeating integrity is a no-op"
                );
            }
        }
    }
}
