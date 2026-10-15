//! Presence consumes key membership and prototype cells, never property values.
use super::*;
use crate::object::shapes::{self, ShapeObjectKind};

/// One candidate lookup in the owner's key index. Dictionary keys belong to
/// the owner and never authorize a reusable shape answer.
#[inline(never)]
pub(crate) unsafe fn own_position(
    obj: *const ObjectHeader,
    name: &[u8],
) -> Option<(Option<u32>, u64, ShapeObjectKind, bool)> {
    // A live JS pointer's honest +4 word naming an authoritative ShapeId
    // already proves an object (shape rule 3), as in hasOwn's register gate.
    // Do not rediscover allocator ownership on every presence candidate.
    if !crate::value::addr_class::is_above_handle_band(obj as usize) {
        return None;
    }
    let shape = shapes::object_shape_descriptor(obj)?;
    let header = &*((obj as usize - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader);
    if header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
    {
        return None;
    }
    let table_owned = crate::object::overflow::has_overflow(obj);
    if (!table_owned && !shape.object_kind.is_ordinary_layout())
        || matches!(
            shape.object_kind,
            ShapeObjectKind::NativeNamespace | ShapeObjectKind::OrdinaryNativeAlias
        )
        || shape.proto_id == shapes::PROTO_ID_PER_OBJECT
    {
        return None;
    }
    let class_id = (*obj).class_id;
    if class_id >= 0x7FFF_FF00
        && !(crate::object::class_registry::SYNTHETIC_CLASS_ID_BASE..0xFFFF_0000)
            .contains(&class_id)
    {
        return None;
    }
    let meta = (*obj).meta;
    if !meta.is_null()
        && ((*meta).elements != 0
            || (*meta).flags
                & (crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER
                    | crate::object::OBJECT_META_FLAG_NATIVE_ALIAS)
                != 0)
    {
        return None;
    }
    let view = if table_owned {
        crate::object::object_keys(obj)
    } else {
        shape.keys_view()
    };
    let position = if table_owned {
        crate::object::keys_find_slot_by_bytes_resolved(view.arr(), view.count(), name)
    } else if view.count() >= crate::object::KEYS_INDEX_THRESHOLD {
        // Presence may build the owner's existing key index once. A complete
        // miss is authoritative, including on a mutable table_owned list.
        let hash = crate::object::key_bytes_hash(name.as_ptr(), name.len());
        match shapes::shape_slot_lookup_verdict(view.arr(), name, hash, view.count(), true) {
            shapes::KeysIndexVerdict::Found(slot) => Some(slot),
            shapes::KeysIndexVerdict::Absent => None,
            shapes::KeysIndexVerdict::Unindexed => {
                crate::object::keys_find_slot_by_bytes_resolved(view.arr(), view.count(), name)
            }
        }
    } else {
        crate::object::keys_find_slot_by_bytes_resolved(view.arr(), view.count(), name)
    }
    .and_then(|slot| {
        if crate::object::key_attrs::entry_is_private(crate::object::key_attrs::keys_entry(
            view.arr(),
            slot,
        )) {
            // Private and public strings may have the same diagnostic
            // spelling. Ask the existing property namespace lookup rather
            // than treating the private candidate as proof of absence.
            crate::object::keys_find_property_slot_by_bytes(view.arr(), view.count(), name)
        } else {
            Some(slot)
        }
    });
    // A layout that can have holes: an optional key whose slot holds the
    // hole is absent (`key_view`), and the answer is this object's alone.
    let holey = !table_owned && crate::object::key_view::summary_may_have_holes(shape.summary);
    let position = position
        .filter(|&p| !holey || !crate::object::key_view::position_is_hole(obj, view, p));

    if position.is_some() {
        if class_id != 0 && super::super::is_internal_runtime_key_bytes(name) {
            return None;
        }
        if shape.object_kind != ShapeObjectKind::Ordinary
            && obj as usize == crate::array::function_prototype_addr_if_resolved()
        {
            return None;
        }
    } else if class_id != 0
        && (crate::object::class_value::class_decl_prototype_link(class_id) as usize
            == obj as usize
            || super::super::class_evaluation_prototype_class_id(obj as usize).is_some())
    {
        // Declared prototype methods/constructor can be virtual own keys.
        return None;
    }
    let reusable = !table_owned
        && !holey
        && shape.hole_count == 0
        && shapes::is_site_matchable_shape_id(shapes::object_shape_stamp(obj));
    Some((position, shape.proto_id, shape.object_kind, reusable))
}

#[inline(never)]
pub(crate) unsafe fn try_shape_has_property(receiver: f64, key: f64) -> Option<bool> {
    let value = JSValue::from_bits(receiver.to_bits());
    let key_value = JSValue::from_bits(key.to_bits());
    if !value.is_pointer() || !key_value.is_any_string() {
        return None;
    }
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let name = crate::string::js_string_key_bytes(key_value, &mut scratch)?;
    let mut obj = value.as_pointer::<ObjectHeader>() as *const ObjectHeader;
    for _ in 0..32 {
        if !crate::value::addr_class::is_above_handle_band(obj as usize) {
            return None;
        }
        let stamp = shapes::object_shape_stamp(obj);
        // The ordinary boolean operation needs membership, not a slot or a
        // holder proof. Reuse hasOwn's existing shape consumer; it declines
        // private, table_owned, holey and per-object-prototype records.
        let ordinary = shapes::shape_record_by_id(stamp)
            .filter(|record| {
                let kind = record.object_kind();
                kind.is_ordinary_layout()
                    && !matches!(
                        kind,
                        ShapeObjectKind::NativeNamespace | ShapeObjectKind::OrdinaryNativeAlias
                    )
                    && (*obj).class_id == 0
                    && (kind == ShapeObjectKind::Ordinary
                        || obj as usize != crate::array::function_prototype_addr_if_resolved())
            })
            .and_then(|record| {
                shapes::plain_own_key_present(
                    shapes::ordinary_dir_addr(),
                    stamp,
                    key_value.bits(),
                    name,
                )
                .map(|(present, _)| (present, record.proto_id()))
            });
        let (present, pid) = if let Some((present, pid)) = ordinary {
            // The authoritative record admitted the object. Receiver-only
            // exotic flags still belong to the complete shared operation.
            let header =
                &*((obj as usize - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader);
            let meta = (*obj).meta;
            if header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
                || header._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
                || (!meta.is_null()
                    && ((*meta).elements != 0
                        || (*meta).flags
                            & (crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER
                                | crate::object::OBJECT_META_FLAG_NATIVE_ALIAS)
                            != 0))
            {
                return None;
            }
            if present {
                return Some(true);
            }
            (false, pid)
        } else {
            let (position, pid, _kind, _reusable) = own_position(obj, name)?;
            (position.is_some(), pid)
        };
        if present {
            return Some(true);
        }
        if pid == shapes::PROTO_ID_NULL {
            return Some(false);
        }
        let next = if pid == shapes::PROTO_ID_DEFAULT {
            let prototype = crate::array::object_prototype_addr_if_resolved();
            if prototype == 0 {
                return None;
            }
            if prototype == obj as usize {
                return Some(false);
            }
            prototype as *const ObjectHeader
        } else {
            let word = shapes::shape_record_by_id(stamp)?.prototype_word();
            let prototype = JSValue::from_bits(word);
            if !prototype.is_pointer() {
                return None;
            }
            prototype.as_pointer::<ObjectHeader>() as *const ObjectHeader
        };
        obj = next;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presence_shape_answer_does_not_repeat_allocator_admission() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_gc = crate::gc::GcSuppressScope::new();
        unsafe {
            let obj = crate::object::object_alloc_plain(1);
            let key = crate::string::canonical_key(b"shapePresence");
            crate::object::js_object_set_field_by_name(obj, key, 42.0);
            let probes = crate::value::addr_class::tracked_header_probe_count_for_tests;
            let before = probes();
            let answer = own_position(obj, b"shapePresence").expect("subject: shape answer");
            assert!(answer.0.is_some());
            assert_eq!(
                try_shape_has_property(
                    crate::value::js_nanbox_pointer(obj as i64),
                    crate::value::js_nanbox_string(key as i64),
                ),
                Some(true)
            );
            assert_eq!(
                probes(),
                before,
                "shape authority already admits the object"
            );
            assert!(own_position(
                perry_abi::RECEIVER_HANDLE_FLOOR.wrapping_sub(1) as *const ObjectHeader,
                b"shapePresence"
            )
            .is_none());
        }
    }

    #[test]
    fn presence_keeps_same_spelling_public_and_private_names_separate() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_gc = crate::gc::GcSuppressScope::new();
        crate::object::ensure_object_intrinsics();
        unsafe {
            let obj = crate::object::object_alloc_plain(1);
            let key = crate::string::canonical_key(b"namespacePresence");
            let receiver = crate::value::js_nanbox_pointer(obj as i64);
            let boxed_key = crate::value::js_nanbox_string(key as i64);
            crate::object::js_object_set_field_by_name(
                obj,
                key,
                f64::from_bits(crate::value::TAG_UNDEFINED),
            );
            crate::object::key_attrs::apply_edits(
                obj,
                &[crate::object::key_attrs::AttrsEdit::Private(
                    b"namespacePresence",
                )],
            );
            assert_eq!(
                crate::object::object_keys(obj).count(),
                2,
                "subject: two namespaces share one spelling"
            );
            assert_eq!(try_shape_has_property(receiver, boxed_key), Some(true));
            assert_eq!(
                crate::object::js_object_has_own(receiver, boxed_key).to_bits(),
                crate::value::TAG_TRUE
            );
            crate::object::js_object_delete_field(obj, key);
            // Object model step 4: the delete went through the overflow; the
            // public entry is a tombstone and only the private element is
            // still a key.
            let (slots, len) = crate::object::object_keys(obj).dense_slots();
            assert_eq!(
                (0..len)
                    .filter(|&i| (*slots.add(i)).to_bits() != crate::value::TAG_HOLE)
                    .count(),
                1,
                "only the private element remains"
            );
            assert_eq!(try_shape_has_property(receiver, boxed_key), Some(false));
            assert_eq!(
                crate::object::js_object_has_own(receiver, boxed_key).to_bits(),
                crate::value::TAG_FALSE
            );
        }
    }

    #[test]
    fn table_owned_presence_uses_owned_keys_without_shape_memo() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_gc = crate::gc::GcSuppressScope::new();
        unsafe {
            crate::object::ensure_object_intrinsics();
            let obj = crate::object::object_alloc_plain(0);
            for i in 0..6 {
                let name = format!("table_owned_presence_{i}");
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                crate::object::js_object_set_field_by_name(obj, key, i as f64);
            }
            let key = crate::string::js_string_from_bytes(b"x".as_ptr(), 1);
            let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
            crate::object::js_object_set_field_by_name(obj, key, undefined);
            assert!(crate::object::overflow::establish_order(obj));
            // Object model step 4: the layout keeps its keys; the overflow
            // is the receiver's key view and answers presence.
            assert!(crate::object::overflow::is_table(
                crate::object::object_keys(obj).arr()
            ));
            let receiver = crate::value::js_nanbox_pointer(obj as i64);
            let boxed_key = crate::value::js_nanbox_string(key as i64);
            let child = crate::object::object_alloc_plain(0);
            let child_value = crate::value::js_nanbox_pointer(child as i64);
            crate::object::js_object_set_prototype_of(child_value, receiver);
            for value in [receiver, child_value] {
                assert_eq!(try_shape_has_property(value, boxed_key), Some(true));
                assert_ne!(
                    crate::value::js_is_truthy(js_in_operator(value, boxed_key)),
                    0
                );
                assert_ne!(
                    crate::value::js_is_truthy(js_object_has_property(value, boxed_key)),
                    0
                );
            }
            crate::object::js_object_delete_field(obj, key);
            for value in [receiver, child_value] {
                assert_eq!(
                    crate::value::js_is_truthy(js_in_operator(value, boxed_key)),
                    0
                );
            }
            crate::object::js_object_set_field_by_name(obj, key, undefined);
            for value in [receiver, child_value] {
                assert_ne!(
                    crate::value::js_is_truthy(js_in_operator(value, boxed_key)),
                    0
                );
            }
        }
    }

    #[test]
    fn implicit_prototype_presence_does_not_read_undefined_or_invoke_getters() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_gc = crate::gc::GcSuppressScope::new();
        let proto = crate::object::ensure_object_intrinsics().1;
        let obj = crate::object::object_alloc_plain(0);
        let key = crate::string::js_string_from_bytes(b"presence10497".as_ptr(), 13);
        let receiver = crate::value::js_nanbox_pointer(obj as i64);
        let boxed_key = crate::value::js_nanbox_string(key as i64);
        crate::object::js_object_set_field_by_name(
            proto,
            key,
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        assert_ne!(
            crate::value::js_is_truthy(js_object_has_property(receiver, boxed_key)),
            0
        );

        static GETS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        extern "C" fn getter(
            _closure: *const crate::closure::ClosureHeader,
            _this: crate::closure::JsThis,
        ) -> f64 {
            GETS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        let get = crate::closure::js_closure_alloc(crate::fn_info!(getter, 0), 0);
        crate::object::set_builtin_accessor_descriptor(
            proto as usize,
            "presence10497".to_string(),
            crate::object::AccessorDescriptor {
                get: crate::value::js_nanbox_pointer(get as i64).to_bits(),
                set: 0,
            },
            crate::object::PropertyAttrs::new(true, false, true),
        );
        GETS.store(0, std::sync::atomic::Ordering::Relaxed);
        assert_ne!(
            crate::value::js_is_truthy(js_in_operator(receiver, boxed_key)),
            0
        );
        assert_eq!(GETS.load(std::sync::atomic::Ordering::Relaxed), 0);
        crate::object::js_object_delete_field(proto, key);
        assert_eq!(
            crate::value::js_is_truthy(js_object_has_property(receiver, boxed_key)),
            0
        );
    }
}
