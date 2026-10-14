//! Named [[Set]] answered by the receiver shape, independent of a store site.

use super::{shapes, ObjectHeader};

/// Traced Array/Function named-property holder; ordinary-band ids already prove ObjectHeaders.
pub(crate) unsafe fn named_bag(
    receiver: *const ObjectHeader,
    key_bits: u64,
) -> Option<*const ObjectHeader> {
    let header = crate::value::addr_class::try_read_gc_header(receiver as usize)?;
    let bag = match header.obj_type {
        crate::gc::GC_TYPE_ARRAY => {
            let bag = crate::array::array_property_bag(receiver.cast());
            if bag.is_null() {
                return None;
            }
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let bytes = crate::string::js_string_key_bytes(
                crate::JSValue::from_bits(key_bits),
                &mut scratch,
            )?;
            let name = std::str::from_utf8(bytes).ok()?;
            if name == "length" || super::canonical_array_index(name).is_some() {
                return None;
            }
            bag
        }
        crate::gc::GC_TYPE_CLOSURE => {
            if header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
                return None;
            }
            (*(receiver as *const crate::closure::ClosureHeader)).props
        }
        _ => return None,
    };
    (!bag.is_null()).then_some(bag.cast_const())
}

/// A POINTER-tag receiver's ordinary stamp proves kind (#10828 rule 3).
/// Forwarding writes a high half below the band; the slab needs no allocator classification.
#[inline(always)]
unsafe fn ordinary_receiver_record(obj: *const ObjectHeader) -> Option<shapes::ShapeRecordRef> {
    if !crate::value::addr_class::is_plausible_heap_addr(obj as usize)
        || !(obj as usize).is_multiple_of(std::mem::align_of::<crate::gc::GcHeader>())
    {
        return None;
    }
    let id = shapes::object_shape_stamp(obj);
    if id.wrapping_sub(shapes::SHAPE_ID_BASE)
        >= shapes::EXOTIC_SHAPE_ID_BASE - shapes::SHAPE_ID_BASE
    {
        return None;
    }
    shapes::shape_record_by_id(id)
}

/// The store twin of the read front's positional shape answer. The receiver
/// kind is a shape fact; integrity changes retire the id or its key attributes.
pub(crate) unsafe fn try_own_store(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> bool {
    let record = if let Some(record) = ordinary_receiver_record(obj) {
        record
    } else {
        let Some(header) = crate::value::addr_class::try_read_gc_header(obj as usize) else {
            return false;
        };
        if header.obj_type == crate::gc::GC_TYPE_CLOSURE {
            if header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
                || header._reserved & crate::gc::OBJ_FLAG_FROZEN != 0
                || crate::closure::shape::is_class_info(
                    (*(obj as *const crate::closure::ClosureHeader)).info,
                )
            {
                return false;
            }
            return named_bag(obj, crate::JSValue::string_ptr(key as *mut _).bits())
                .is_some_and(|bag| try_own_store(bag.cast_mut(), key, value));
        }
        if header.obj_type == crate::gc::GC_TYPE_ARRAY {
            let arr = crate::array::clean_arr_ptr_mut(obj.cast());
            if crate::array::array_object_flags_resolved(arr) & crate::gc::OBJ_FLAG_FROZEN != 0 {
                return false;
            }
            // Array keys are classified once. An existing dense index is
            // an own data binding too; the shared strict dense setter proves
            // descriptors, integrity, presence and the value barrier. A
            // refusal still needs ordinary Set (holes, growth or accessors).
            let Some(name) = crate::string::header_str_checked(key) else {
                return false;
            };
            if let Some(index) = super::canonical_array_index(name) {
                return crate::array::try_strict_dense_index_set(arr, index, value).is_some();
            }
            if name == "length" {
                return false;
            }
            let bag = crate::array::array_property_bag(arr);
            return !bag.is_null() && try_own_store(bag, key, value);
        }
        if header.obj_type != crate::gc::GC_TYPE_OBJECT
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return false;
        }
        let Some(record) = shapes::object_shape_record(obj) else {
            return false;
        };
        record
    };
    if !record.object_kind().is_ordinary_layout() {
        return false;
    }
    if record.object_kind() == shapes::ShapeObjectKind::Ordinary {
        if let Some(slot) = record.own_store_slot(key) {
            let Some(slot) = slot else {
                // Shape-proven absence cannot become an overwrite on another own-key lookup.
                return false;
            };
            // Attributes belong to the matched binding. An accessor,
            // readonly intrinsic or private slot elsewhere on this shape
            // cannot invalidate an ordinary writable property here.
            if record.summary()
                & (super::key_attrs::SUMMARY_BLOCKS_STORE | super::key_attrs::SUMMARY_PRIVATE)
                != 0
                && !super::key_attrs::entry_is_plain_writable_data(super::key_attrs::keys_entry(
                    record.keys() as usize as *const crate::ArrayHeader,
                    slot as u32,
                ))
            {
                return false;
            }
            if store_proven_own_slot(obj, record, slot, value) {
                return true;
            }
        }
    }
    super::try_existing_own_data_overwrite(obj, key, value)
}

/// Proven writable ordinary own slot: shared representation/barrier funnel for Set and Define.
#[inline(always)]
pub(crate) unsafe fn store_proven_own_slot(
    obj: *mut ObjectHeader,
    record: shapes::ShapeRecordRef,
    slot: usize,
    value: f64,
) -> bool {
    let bits = if value.to_bits() == crate::value::POINTER_TAG {
        crate::value::TAG_UNDEFINED
    } else {
        value.to_bits()
    };
    if slot < record.live_inline_slot_count() as usize {
        let rep = super::field_rep::slot_rep(record.rep(), slot as u32);
        let address = obj
            .cast::<u8>()
            .add(std::mem::size_of::<ObjectHeader>())
            .cast::<u64>()
            .add(slot);
        if bits & 0x7ff0_0000_0000_0000 != 0x7ff0_0000_0000_0000
            && rep != super::field_rep::REP_SPECIAL
        {
            // GC_STORE_AUDIT(POINTER_FREE): a canonical finite Number.
            address.write(bits);
            return true;
        }
        if rep == super::field_rep::REP_ANY {
            // Existing named-Set barrier: pointer traffic needs no extra slow call.
            crate::gc::runtime_store_jsvalue_slot_layout_deferred(
                obj as usize,
                address as usize,
                slot,
                bits,
            );
            return true;
        }
    }
    store_proven_own_slot_slow(obj, record, slot, f64::from_bits(bits))
}

#[cold]
#[inline(never)]
unsafe fn store_proven_own_slot_slow(
    obj: *mut ObjectHeader,
    record: shapes::ShapeRecordRef,
    slot: usize,
    value: f64,
) -> bool {
    let bits = if value.to_bits() == crate::value::POINTER_TAG {
        crate::value::TAG_UNDEFINED
    } else {
        value.to_bits()
    };
    if slot < record.live_inline_slot_count() as usize {
        super::store_object_field_slot(obj, slot, bits);
        return true;
    }
    if super::spill_store_would_be_in_capacity(obj as usize, slot) {
        // Existing physical storage makes the ordinary spill barrier a nonmoving leaf.
        super::spill::spill_set(obj as usize, slot, bits);
        return true;
    }
    false
}

/// Null and realm-default chains already have an authoritative answer. Reuse
/// that answer rather than resolving and recording a second chain fact for
/// each plain-object predecessor. Custom and class chains use record facts.
#[inline]
unsafe fn inherited_store_blocked(
    record: shapes::ShapeRecordRef,
    key: *const crate::StringHeader,
    bytes: &[u8],
) -> Option<bool> {
    // The prototype identity also covers anonymous closed-literal class ids.
    // Header class_id alone cannot distinguish their DEFAULT link from a
    // declaration's CLASS link; shape minting already made that distinction.
    match record.proto_id() {
        shapes::PROTO_ID_NULL => return Some(false),
        shapes::PROTO_ID_DEFAULT => {
            return Some(super::object_proto_may_intercept_key(
                crate::value::js_nanbox_string(key as i64),
            ));
        }
        _ => {}
    }
    record.store_chain_blocks(bytes, super::proto_validity::proto_validity())
}

/// A warm shape proof needs no outer PutValue scope or key conversion. The
/// existing transition funnel owns its roots and repairs declined operands.
pub(crate) unsafe fn try_warm_transition(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
    refresh: &mut Option<(f64, f64, f64)>,
) -> Option<f64> {
    let record = if let Some(record) = ordinary_receiver_record(obj) {
        record
    } else {
        let header = crate::value::addr_class::try_read_gc_header(obj as usize)?;
        if header.obj_type == crate::gc::GC_TYPE_ARRAY {
            return try_warm_array_transition(obj.cast(), key, value, refresh);
        }
        if header.obj_type != crate::gc::GC_TYPE_OBJECT
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return None;
        }
        shapes::object_shape_record(obj)?
    };
    if !record.object_kind().is_ordinary_layout()
        || (*obj).class_id == super::NATIVE_MODULE_CLASS_ID
    {
        return None;
    }
    let bytes = crate::string::header_str_checked(key)?.as_bytes();
    if bytes == b"__proto__"
        || bytes
            .first()
            .is_some_and(|b| *b == b'#' || b.is_ascii_digit())
        || inherited_store_blocked(record, key, bytes) != Some(false)
    {
        return None;
    }
    super::object_set_field_by_name_transition_chain_proven_value(obj, key, value, refresh)
}

/// A projected array bag already names its prototype chain. Reuse its
/// learned edge without repeating the collecting generic Set setup.
unsafe fn try_warm_array_transition(
    arr: *mut crate::ArrayHeader,
    key: *const crate::StringHeader,
    value: f64,
    refresh: &mut Option<(f64, f64, f64)>,
) -> Option<f64> {
    let arr = crate::array::clean_arr_ptr_mut(arr);
    if crate::array::array_object_flags_resolved(arr)
        & (crate::gc::GC_ARRAY_CUSTOM_PROTO
            | crate::gc::OBJ_FLAG_FROZEN
            | crate::gc::OBJ_FLAG_SEALED
            | crate::gc::OBJ_FLAG_NO_EXTEND)
        != 0
    {
        return None;
    }
    let bytes = crate::string::header_str_checked(key)?.as_bytes();
    if bytes == b"length"
        || bytes == b"__proto__"
        || bytes
            .first()
            .is_some_and(|b| *b == b'#' || b.is_ascii_digit())
    {
        return None;
    }
    let bag = crate::array::array_property_bag(arr);
    if bag.is_null() {
        return None;
    }
    let proto = crate::array::array_prototype_addr() as *mut crate::ArrayHeader;
    if proto.is_null()
        || arr == crate::array::clean_arr_ptr_mut(proto)
        || crate::array::array_object_flags_resolved(proto) & crate::gc::GC_ARRAY_CUSTOM_PROTO != 0
    {
        return None;
    }
    let proto_bag = crate::array::array_property_bag(proto);
    // A freshly allocated bag still has Object.prototype. Its proof cannot
    // stand in for the outer array's chain until the projection is installed.
    if proto_bag.is_null()
        || super::prototype_chain::object_static_prototype(bag as usize)
            != Some(crate::value::js_nanbox_pointer(proto_bag as i64).to_bits())
    {
        return None;
    }
    if shapes::object_shape_record(bag)?
        .store_chain_blocks(bytes, super::proto_validity::proto_validity())
        != Some(false)
    {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(arr);
    let key_root = scope.root_string_ptr(key);
    let value_root = scope.root_nanbox_f64(value);
    let stored =
        super::object_set_field_by_name_transition_chain_proven_value(bag, key, value, &mut None);
    // A declined append may have interned a key and collected. Repair the
    // outer receiver, rather than returning the bag as the receiver.
    *refresh = Some((
        crate::value::js_nanbox_pointer(owner.get_raw_mut_ptr::<crate::ArrayHeader>() as i64),
        crate::value::js_nanbox_string(key_root.get_raw_const_ptr::<crate::StringHeader>() as i64),
        value_root.get_nanbox_f64(),
    ));
    stored.map(|_| value_root.get_nanbox_f64())
}

/// Array named properties use the same shaped bag as customized descriptors.
/// Its prototype edge names the array chain; elements and length stay exotic.
unsafe fn try_array_store(
    target: f64,
    key: f64,
    value: f64,
    name: &str,
    may_move: &mut bool,
) -> Option<f64> {
    let key_ptr = (key.to_bits() & crate::value::POINTER_MASK) as *const crate::StringHeader;
    if name == "length"
        || name == "__proto__"
        || name.starts_with('#')
        || super::canonical_array_index(name).is_some()
    {
        return None;
    }
    let arr = crate::array::clean_arr_ptr_mut(
        (target.to_bits() & crate::value::POINTER_MASK) as *mut crate::ArrayHeader,
    );
    let flags = crate::array::array_object_flags_resolved(arr);
    if flags & (crate::gc::GC_ARRAY_CUSTOM_PROTO | crate::gc::OBJ_FLAG_FROZEN) != 0 {
        return None;
    }
    let bag = crate::array::array_property_bag(arr);
    let existing = if bag.is_null() {
        crate::array::array_named_property_has(arr, key_ptr)
    } else {
        super::obj_value_has_own_key(crate::value::js_nanbox_pointer(bag as i64), key)
    };
    if !existing && flags & (crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND) != 0 {
        return None;
    }
    *may_move = true;
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(target);
    let key_root = scope.root_string_ptr(key_ptr);
    let value_root = scope.root_nanbox_f64(value);
    let bag = crate::array::array_property_bag_ensure(arr);
    let bag_root = scope.root_raw_mut_ptr(bag);
    if bag.is_null() {
        return None;
    }
    // This internal holder has ordinary data/descriptor semantics; it is
    // not one of the runtime-born special objects the unmarked kind covers.
    if shapes::object_shape_record(bag)?.object_kind() == shapes::ShapeObjectKind::OrdinaryUnmarked
    {
        super::alloc_plain::mark_object_plain_ordinary(bag);
    }
    if !existing {
        // Array.prototype is itself an array. Its named-property bag is
        // the ordinary shape projection of this named-only chain. Marking
        // that bag makes subsequent named descriptor edits invalidate the
        // proof through the existing shape-stamp hook.
        let mut array_proto = crate::array::array_prototype_addr();
        if array_proto == 0 {
            let _ = super::js_get_global_this();
            array_proto = crate::array::array_prototype_addr();
        }
        if array_proto == 0 {
            return None;
        }
        let owner_arr = crate::array::clean_arr_ptr_mut(
            (owner.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK)
                as *mut crate::ArrayHeader,
        );
        let array_proto = crate::array::clean_arr_ptr_mut(array_proto as *mut crate::ArrayHeader);
        let prototype = if owner_arr == array_proto {
            crate::value::js_nanbox_pointer(crate::array::object_prototype_addr() as i64)
        } else {
            crate::value::js_nanbox_pointer(array_proto as i64)
        };
        let proto_root = scope.root_nanbox_f64(prototype);
        let proto_addr = (prototype.to_bits() & crate::value::POINTER_MASK) as usize;
        let header = crate::value::addr_class::try_read_gc_header(proto_addr)?;
        let prototype = if header.obj_type == crate::gc::GC_TYPE_ARRAY {
            let proto_arr = crate::array::clean_arr_ptr_mut(proto_addr as *mut crate::ArrayHeader);
            if crate::array::array_object_flags_resolved(proto_arr)
                & crate::gc::GC_ARRAY_CUSTOM_PROTO
                != 0
            {
                return None;
            }
            let proto_bag = crate::array::array_property_bag_ensure(proto_arr);
            if proto_bag.is_null() {
                return None;
            }
            let proto_bag_root = scope.root_raw_mut_ptr(proto_bag);
            let object_proto_addr = crate::array::object_prototype_addr();
            if object_proto_addr == 0 {
                return None;
            }
            let object_proto = crate::value::js_nanbox_pointer(object_proto_addr as i64);
            if super::prototype_chain::object_static_prototype(proto_bag as usize)
                != Some(object_proto.to_bits())
            {
                // Internal construction of `%Array.prototype%`'s bag, not a
                // modification of it (#11915, `builtin_proto`).
                super::own_override::as_builtin_definition(|| {
                    super::prototype_chain::object_link_created_prototype(
                        proto_bag as usize,
                        object_proto.to_bits(),
                    )
                });
            }
            crate::value::js_nanbox_pointer(proto_bag_root.get_raw_mut_ptr::<ObjectHeader>() as i64)
        } else if header.obj_type == crate::gc::GC_TYPE_OBJECT {
            proto_root.get_nanbox_f64()
        } else {
            return None;
        };
        let bag = bag_root.get_raw_mut_ptr::<ObjectHeader>();
        if super::prototype_chain::object_static_prototype(bag as usize)
            != Some(prototype.to_bits())
        {
            // Internal construction, not observable prototype surgery. The
            // new shape names the edge; no global plan invalidation is needed.
            super::prototype_chain::object_link_created_prototype(
                bag as usize,
                prototype.to_bits(),
            );
        }
    }
    let bag = bag_root.get_raw_mut_ptr::<ObjectHeader>();
    let key = key_root.get_raw_const_ptr::<crate::StringHeader>();
    let key_value = crate::value::js_nanbox_string(key as i64);
    let result = try_store_with_refresh(
        crate::value::js_nanbox_pointer(bag as i64),
        key_value,
        value_root.get_nanbox_f64(),
        may_move,
    );
    if result.is_some() {
        return Some(value_root.get_nanbox_f64());
    }
    if !existing {
        // The first transition has no edge yet. A proven chain can create it
        // through the ordinary bag store; later receivers reuse that edge.
        let bag = bag_root.get_raw_mut_ptr::<ObjectHeader>();
        let key = key_root.get_raw_const_ptr::<crate::StringHeader>();
        let record = shapes::object_shape_record(bag)?;
        let bytes = crate::string::header_str_checked(key)?.as_bytes();
        if record.store_chain_blocks(bytes, super::proto_validity::proto_validity()) == Some(false)
        {
            super::object_ops::define_property_force_store_value(
                bag,
                key,
                value_root.get_nanbox_f64(),
            );
            return Some(value_root.get_nanbox_f64());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn function_recreated_data_key_clears_the_deletion_marker() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let owner = fresh_test_function();
        let ptr = (owner.to_bits() & crate::value::POINTER_MASK) as usize;
        let key = crate::string::canonical_key(b"recreatedFunctionData");
        let boxed_key = crate::value::js_nanbox_string(key as i64);
        assert_eq!(unsafe { try_store(owner, boxed_key, 43.0) }, Some(43.0));
        crate::closure::closure_mark_key_deleted(ptr, "recreatedFunctionData");
        assert!(crate::closure::closure_is_key_deleted(
            ptr,
            "recreatedFunctionData"
        ));
        assert_eq!(unsafe { try_store(owner, boxed_key, 107.0) }, Some(107.0));
        assert!(!crate::closure::closure_is_key_deleted(
            ptr,
            "recreatedFunctionData"
        ));
        assert!(super::super::obj_value_has_own_key(owner, boxed_key));
        let descriptor = super::super::js_object_get_own_property_descriptor(owner, boxed_key);
        assert_ne!(descriptor.to_bits(), crate::value::TAG_UNDEFINED);
    }

    #[test]
    fn plain_add_uses_the_existing_chain_answer_without_recording_another_fact() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _ = super::super::js_get_global_this();
        unsafe {
            let key = crate::string::canonical_key(b"plainShapeAnswerOnly");
            let boxed_key = crate::value::js_nanbox_string(key as i64);
            const ANON: u32 = 0x63C2;
            super::super::class_registry::js_register_anon_shape_class_id(ANON);
            for obj in [
                super::super::js_object_alloc(0, 0),
                super::super::js_object_alloc_null_proto(0, 0),
                super::super::js_object_alloc(ANON, 0),
            ] {
                super::super::alloc_plain::mark_object_plain_ordinary(obj);
                let before = shapes::object_shape_record(obj).unwrap();
                assert_eq!(
                    inherited_store_blocked(before, key, b"plainShapeAnswerOnly"),
                    Some(false)
                );
                let owner = crate::value::js_nanbox_pointer(obj as i64);
                assert_eq!(try_store(owner, boxed_key, 17.0), Some(17.0));
                assert_eq!(
                    before.store_chain_blocks(
                        b"plainShapeAnswerOnly",
                        super::super::proto_validity::proto_validity()
                    ),
                    None
                );
                assert_eq!(
                    super::super::js_object_get_field_by_name_f64(obj, key),
                    17.0
                );
            }
            let proto = crate::array::object_prototype_addr() as *mut ObjectHeader;
            super::super::js_object_set_field_by_name(proto, key, 29.0);
            super::super::set_property_attrs(
                proto as usize,
                "plainShapeAnswerOnly".into(),
                super::super::PropertyAttrs::new(false, true, true),
            );
            let fresh = super::super::js_object_alloc(0, 0);
            super::super::alloc_plain::mark_object_plain_ordinary(fresh);
            let owner = crate::value::js_nanbox_pointer(fresh as i64);
            let result = try_store(owner, boxed_key, 23.0);
            let present = super::super::obj_value_has_own_key(owner, boxed_key);
            super::super::js_object_delete_dynamic_value(
                crate::value::js_nanbox_pointer(proto as i64),
                boxed_key,
            );
            assert_eq!(
                result, None,
                "the default prototype's current attributes remain authoritative"
            );
            assert!(!present);
        }
    }

    #[test]
    fn any_own_store_preserves_string_alias_demotion() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_gc = crate::gc::GcSuppressScope::new();
        unsafe {
            let obj = super::super::js_object_alloc(0, 1);
            super::super::alloc_plain::mark_object_plain_ordinary(obj);
            let key = crate::string::canonical_key(b"ownedStringValue");
            super::super::js_object_set_field_by_name(
                obj,
                key,
                f64::from_bits(crate::value::TAG_UNDEFINED),
            );
            assert_eq!(
                shapes::object_shape_record(obj).unwrap().rep() & 3,
                super::super::field_rep::REP_ANY
            );
            let text = b"retained string snapshot";
            let string = crate::string::js_string_from_bytes_with_capacity(
                text.as_ptr(),
                text.len() as u32,
                64,
            );
            (*string).refcount = 1;
            let value = crate::value::js_nanbox_string(string as i64);
            assert!(try_own_store(obj, key, value));
            assert_eq!((*string).refcount, 0, "a heap slot makes the string shared");
            let suffix = crate::string::js_string_from_bytes(b" suffix".as_ptr(), 7);
            assert_ne!(crate::string::js_string_append(string, suffix), string);
            let stored = super::super::js_object_get_field_by_name_f64(obj, key);
            assert_eq!(stored.to_bits(), value.to_bits());
            assert_eq!(
                crate::string::header_str_checked(string)
                    .unwrap()
                    .as_bytes(),
                text
            );
        }
    }

    #[test]
    fn string_index_own_store_uses_dense_presence_and_descriptor_proof() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_gc = crate::gc::GcSuppressScope::new();
        let values = [11.0, 22.0];
        let arr = crate::array::js_array_from_f64(values.as_ptr(), 2);
        let index = crate::string::canonical_key(b"1");
        unsafe {
            assert!(try_own_store(arr.cast(), index, 33.0));
            assert_eq!(crate::array::js_array_get_f64(arr, 1), 33.0);
            for name in [b"01".as_slice(), b"-0", b"2", b"4294967295", b"length"] {
                assert!(!try_own_store(
                    arr.cast(),
                    crate::string::canonical_key(name),
                    44.0
                ));
            }
            let hole = crate::array::js_array_alloc_with_length(2);
            assert!(!try_own_store(hole.cast(), index, 44.0));
            let header = (arr as *mut u8)
                .sub(crate::gc::GC_HEADER_SIZE)
                .cast::<crate::gc::GcHeader>();
            let saved = (*header)._reserved;
            (*header)._reserved |= crate::gc::OBJ_FLAG_FROZEN;
            assert!(!try_own_store(arr.cast(), index, 44.0));
            (*header)._reserved = saved;
            // An element descriptor is a key of the array's named layout: its
            // attribute summary sends every own element store to the exact path.
            crate::object::set_property_attrs(
                arr as usize,
                "0".to_string(),
                crate::object::PropertyAttrs::new(false, true, true),
            );
            assert!(crate::array::array_has_element_descriptors(arr));
            assert!(!try_own_store(arr.cast(), index, 44.0));
            assert_eq!(crate::array::js_array_get_f64(arr, 1), 33.0);
        }
    }

    #[test]
    fn own_number_store_preserves_lanes_and_generalizes_a_function_slot() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _ = super::super::js_get_global_this();
        unsafe {
            let obj = super::super::js_object_alloc(0, 4);
            super::super::alloc_plain::mark_object_plain_ordinary(obj);
            let atom = crate::string::canonical_key(b"shapeNumberStore");
            super::super::js_object_set_field_by_name(obj, atom, 1.0);
            assert!(try_own_store(obj, atom, -17.5));
            assert_eq!(
                super::super::js_object_get_field_by_name_f64(obj, atom),
                -17.5
            );
            assert!(try_own_store(obj, atom, f64::INFINITY));
            assert_eq!(
                super::super::js_object_get_field_by_name_f64(obj, atom),
                f64::INFINITY
            );
            assert!(try_own_store(
                obj,
                atom,
                f64::from_bits(crate::JSValue::int32(42).bits())
            ));
            assert_eq!(
                super::super::js_object_get_field_by_name_f64(obj, atom),
                42.0
            );

            extern "C" fn body(
                _c: *const crate::closure::ClosureHeader,
                _this: crate::closure::JsThis,
            ) -> f64 {
                1.0
            }
            let info = crate::fn_info!(body, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE));
            let function =
                crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(info, 0) as i64);
            let method = crate::string::canonical_key(b"shapeNumberMethod");
            super::super::js_object_set_field_by_name(obj, method, function);
            let record = shapes::object_shape_record(obj).unwrap();
            let slot = record.own_store_slot(method).unwrap().unwrap();
            assert_eq!(
                super::super::field_rep::slot_rep(record.rep(), slot as u32),
                super::super::field_rep::REP_SPECIAL
            );
            assert!(try_own_store(obj, method, 7.0));
            let record = shapes::object_shape_record(obj).unwrap();
            assert_eq!(
                super::super::field_rep::slot_rep(record.rep(), slot as u32),
                super::super::field_rep::REP_ANY
            );
            assert_eq!(
                super::super::js_object_get_field_by_name_f64(obj, method),
                7.0
            );
        }
    }

    #[test]
    fn short_string_put_value_reuses_the_shape_transition() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _ = super::super::js_get_global_this();
        let atom = crate::string::canonical_key(b"sPos");
        let objects = [0; 2].map(|_| {
            let obj = super::super::js_object_alloc(0, 0);
            unsafe { super::super::alloc_plain::mark_object_plain_ordinary(obj) };
            crate::value::js_nanbox_pointer(obj as i64)
        });
        let heap_key = crate::value::js_nanbox_string(atom as i64);
        crate::proxy::js_put_value_set(objects[0], heap_key, 1.0, objects[0], 0);
        let short_key = f64::from_bits(crate::JSValue::try_short_string(b"sPos").unwrap().bits());
        super::super::test_reset_transition_fast_hits();
        assert_eq!(
            crate::proxy::js_put_value_set(objects[1], short_key, 2.0, objects[1], 0),
            2.0
        );
        assert!(
            super::super::test_transition_fast_hits() > 0,
            "short and heap representations must take the same learned edge"
        );
        assert_eq!(
            super::super::js_object_get_field_by_name_f64(
                (objects[1].to_bits() & crate::value::POINTER_MASK) as *mut ObjectHeader,
                atom
            ),
            2.0
        );
    }

    #[test]
    fn function_own_store_uses_bag_attributes_and_outer_integrity() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let function = fresh_test_function();
        let owner = (function.to_bits() & crate::value::POINTER_MASK) as *mut ObjectHeader;
        let atom = crate::string::canonical_key(b"functionStorePos");
        let key = crate::value::js_nanbox_string(atom as i64);
        assert_eq!(unsafe { try_store(function, key, 1.0) }, Some(1.0));
        assert!(unsafe { try_own_store(owner, atom, 2.0) });
        let bag = unsafe { crate::closure::props::bag_of(owner as usize) };
        assert_eq!(
            super::super::js_object_get_field_by_name_f64(bag, atom),
            2.0
        );
        unsafe {
            let header = (owner as *mut u8)
                .sub(crate::gc::GC_HEADER_SIZE)
                .cast::<crate::gc::GcHeader>();
            let saved = (*header)._reserved;
            (*header)._reserved |= crate::gc::OBJ_FLAG_FROZEN;
            assert!(!try_own_store(owner, atom, 3.0));
            (*header)._reserved = saved;
        }
        super::super::set_property_attrs(
            bag as usize,
            "functionStorePos".into(),
            super::super::PropertyAttrs::new(false, true, true),
        );
        assert!(!unsafe { try_own_store(owner, atom, 4.0) });
        assert_eq!(
            super::super::js_object_get_field_by_name_f64(bag, atom),
            2.0
        );
    }

    #[test]
    fn shape_add_retires_its_slot_proof_at_dictionary_conversion() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _ = super::super::js_get_global_this();
        struct Restore(Option<u64>);
        impl Drop for Restore {
            fn drop(&mut self) {
                super::super::dictionary::test_arm_latch(self.0);
            }
        }
        let _restore = Restore(super::super::dictionary::test_arm_latch(Some(3)));
        let obj = super::super::js_object_alloc(0, 0);
        unsafe { super::super::alloc_plain::mark_object_plain_ordinary(obj) };
        let target = crate::value::js_nanbox_pointer(obj as i64);
        for i in 0..8 {
            let name = format!("dictionaryAppendWitness{i}");
            let key = crate::string::canonical_key(name.as_bytes());
            crate::proxy::js_put_value_set(
                target,
                crate::value::js_nanbox_string(key as i64),
                i as f64,
                target,
                0,
            );
        }
        assert!(
            unsafe { super::super::dictionary::is_dictionary(obj) },
            "the conversion must actually run"
        );
        let dictionary_record = unsafe { shapes::object_shape_record(obj).unwrap() };
        assert_eq!(
            unsafe {
                dictionary_record
                    .own_store_slot(crate::string::canonical_key(b"dictionaryAppendWitness0"))
            },
            None,
            "a dictionary record cannot prove own absence"
        );
        for i in 0..8 {
            let name = format!("dictionaryAppendWitness{i}");
            let key = crate::string::canonical_key(name.as_bytes());
            assert_eq!(
                super::super::js_object_get_field_by_name_f64(obj, key),
                i as f64
            );
        }
        let readonly = crate::string::canonical_key(b"dictionaryAppendWitness0");
        super::super::set_property_attrs(
            obj as usize,
            "dictionaryAppendWitness0".into(),
            super::super::PropertyAttrs::new(false, true, true),
        );
        let child = super::super::js_object_alloc(0, 0);
        unsafe { super::super::alloc_plain::mark_object_plain_ordinary(child) };
        let child_value = crate::value::js_nanbox_pointer(child as i64);
        super::super::js_object_set_prototype_of(child_value, target);
        let readonly_value = crate::value::js_nanbox_string(readonly as i64);
        crate::proxy::js_put_value_set(child_value, readonly_value, 99.0, child_value, 0);
        assert!(
            !super::super::obj_value_has_own_key(child_value, readonly_value),
            "dictionary prototype readonly keys must remain inherited"
        );
        assert_eq!(
            super::super::js_object_get_field_by_name_f64(obj, readonly),
            0.0
        );
        let key = crate::value::js_nanbox_string(crate::string::canonical_key(
            b"dictionaryAbsentWitness",
        ) as i64);
        let mut may_move = false;
        assert_eq!(
            unsafe { try_store_with_refresh(target, key, 9.0, &mut may_move) },
            None
        );
        assert!(!may_move);
        // Conversion during the value-bearing publication also retires the
        // slot proof; the dictionary setter must write the pending value.
        super::super::dictionary::test_arm_latch(Some(1));
        let fresh = super::super::js_object_alloc(0, 0);
        unsafe { super::super::alloc_plain::mark_object_plain_ordinary(fresh) };
        let fresh_value = crate::value::js_nanbox_pointer(fresh as i64);
        let atom = crate::string::canonical_key(b"dictionaryNumberPublicationWitness");
        crate::proxy::js_put_value_set(
            fresh_value,
            crate::value::js_nanbox_string(atom as i64),
            37.0,
            fresh_value,
            0,
        );
        assert!(unsafe { super::super::dictionary::is_dictionary(fresh) });
        assert_eq!(
            super::super::js_object_get_field_by_name_f64(fresh, atom),
            37.0
        );
    }

    #[test]
    fn cold_shape_add_writes_the_proven_append_slot_in_a_wide_object() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _ = super::super::js_get_global_this();
        let obj = super::super::js_object_alloc(0, 0);
        unsafe { super::super::alloc_plain::mark_object_plain_ordinary(obj) };
        for i in 0..48 {
            let name = format!("coldWideAppend{i}");
            let key = crate::string::canonical_key(name.as_bytes());
            super::super::js_object_set_field_by_name(obj, key, i as f64);
        }
        let key = crate::string::canonical_key(b"coldWideAppendWitness");
        assert_eq!(
            unsafe {
                shapes::object_shape_record(obj)
                    .unwrap()
                    .logical_key_count()
            },
            48
        );
        assert_eq!(
            unsafe {
                try_store(
                    crate::value::js_nanbox_pointer(obj as i64),
                    crate::value::js_nanbox_string(key as i64),
                    91.0,
                )
            },
            Some(91.0)
        );
        assert_eq!(
            super::super::js_object_get_field_by_name_f64(obj, key),
            91.0
        );
        assert_eq!(
            super::super::js_object_get_field_by_name_f64(
                obj,
                crate::string::canonical_key(b"coldWideAppend0")
            ),
            0.0
        );
    }

    #[test]
    fn cold_shape_proof_mints_the_first_edge_without_complete_set() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _ = super::super::js_get_global_this();
        let key = crate::string::canonical_key(b"coldShapeMintWitness");
        let objects = [0; 2].map(|_| {
            let obj = super::super::js_object_alloc(0, 0);
            unsafe { super::super::alloc_plain::mark_object_plain_ordinary(obj) };
            obj
        });
        let predecessor = unsafe { shapes::object_shape_stamp(objects[0]) };
        assert!(super::super::transition_cache_lookup(predecessor, key).is_none());
        assert_eq!(
            unsafe {
                try_store(
                    crate::value::js_nanbox_pointer(objects[0] as i64),
                    crate::value::js_nanbox_string(key as i64),
                    41.0,
                )
            },
            Some(41.0),
            "a cold, proven absent key must create its first edge"
        );
        assert_eq!(
            unsafe { super::super::field_rep_store::object_slot_rep(objects[0], 0) },
            super::super::field_rep::REP_F64,
            "a cold Number addition must preserve the value publication's lane"
        );
        super::super::test_reset_transition_fast_hits();
        let private_key = crate::string::js_string_from_bytes(
            b"coldShapeMintWitness".as_ptr(),
            b"coldShapeMintWitness".len() as u32,
        );
        assert_ne!(private_key.cast_const(), key);
        assert_eq!(
            unsafe { try_warm_transition(objects[1], private_key, 42.0, &mut None) },
            Some(42.0)
        );
        assert!(super::super::test_transition_fast_hits() > 0);
        assert_eq!(
            super::super::js_object_get_field_by_name_f64(objects[1], key),
            42.0
        );
        assert_eq!(
            unsafe { super::super::field_rep_store::object_slot_rep(objects[1], 0) },
            super::super::field_rep::REP_F64,
            "the warm edge must retain the cold Number publication's lane"
        );
    }

    #[test]
    fn small_store_binding_keeps_a_later_noncanonical_declaration() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let key = crate::string::canonical_key(b"pos");
        let other = crate::string::canonical_key(b"other");
        let duplicate = crate::string::js_string_from_bytes(b"pos".as_ptr(), 3);
        assert_ne!(key, duplicate);
        let mut keys = crate::array::js_array_alloc(4);
        for name in [key, other, duplicate, other] {
            keys = crate::array::js_array_push(keys, crate::JSValue::string_ptr(name));
        }
        let obj = super::super::alloc_plain::alloc_plain_record_with_keys(4, unsafe {
            super::super::ObjectKeys::owned(keys)
        });
        let record = unsafe { shapes::object_shape_record(obj).unwrap() };
        assert_eq!(unsafe { record.own_store_slot(key) }, Some(Some(2)));
        assert_eq!(unsafe { record.own_store_slot(other) }, Some(Some(3)));
    }

    #[test]
    fn declined_store_refreshes_only_after_an_allocating_path() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let function = fresh_test_function();
        let length = crate::value::js_nanbox_string(crate::string::canonical_key(b"length") as i64);
        let mut may_move = false;
        assert_eq!(
            unsafe { try_store_with_refresh(function, length, 3.0, &mut may_move) },
            None
        );
        assert!(!may_move, "an intrinsic-key decline is a nonmoving leaf");
        let obj = super::super::js_object_alloc(0, 0);
        unsafe { super::super::alloc_plain::mark_object_plain_ordinary(obj) };
        let key =
            crate::value::js_nanbox_string(crate::string::canonical_key(b"freshStoreProof") as i64);
        let receiver = crate::value::js_nanbox_pointer(obj as i64);
        unsafe { try_store_with_refresh(receiver, key, 3.0, &mut may_move) };
        assert!(
            may_move,
            "learning a cold chain proof can allocate even on decline"
        );
    }

    #[test]
    fn own_store_uses_complete_binding_after_read_position_proof_retires() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let obj = super::super::js_object_alloc(0, 0);
        unsafe { super::super::alloc_plain::mark_object_plain_ordinary(obj) };
        let key = crate::string::canonical_key(b"pos");
        super::super::js_object_set_field_by_name(obj, key, 1.0);
        super::super::js_object_set_field_by_name(obj, crate::string::canonical_key(b"temp"), 0.0);
        // A semantic transition retires read guesses while retaining these
        // own data bindings, as a prototype/accessor replacement can do.
        unsafe { shapes::transition_object_shape_semantics(obj) };
        assert_eq!(shapes::test_position_bound_of(obj), Some(0));
        assert_eq!(
            unsafe {
                shapes::object_shape_record(obj)
                    .unwrap()
                    .own_store_slot(key)
            },
            Some(Some(0))
        );
        assert!(
            unsafe { try_own_store(obj, key, 7.0) },
            "an authoritative binding survives retirement of the read guess proof"
        );
        assert_eq!(super::super::js_object_get_field_by_name_f64(obj, key), 7.0);
    }

    #[test]
    fn warm_array_transition_uses_the_projected_shape_and_retires_after_edit() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let first = crate::string::canonical_key(b"warmArrayPos");
        let key = crate::string::canonical_key(b"warmArrayEnd");
        let key_value = crate::value::js_nanbox_string(key as i64);
        let arrays = [0; 3].map(|_| crate::array::js_array_alloc(4));
        for arr in arrays {
            let receiver = crate::value::js_nanbox_pointer(arr as i64);
            assert_eq!(
                unsafe { try_store(receiver, crate::value::js_nanbox_string(first as i64), 1.0) },
                Some(1.0)
            );
        }
        assert_eq!(
            unsafe {
                try_store(
                    crate::value::js_nanbox_pointer(arrays[0] as i64),
                    key_value,
                    2.0,
                )
            },
            Some(2.0)
        );
        super::super::test_reset_transition_fast_hits();
        assert_eq!(
            unsafe { try_warm_array_transition(arrays[1], key, 7.0, &mut None) },
            Some(7.0),
            "the projected bag proof must serve another array"
        );
        assert!(super::super::test_transition_fast_hits() > 0);
        let proto = crate::array::array_prototype_addr() as *const crate::ArrayHeader;
        let bag = unsafe { crate::array::array_property_bag(proto) };
        super::super::js_object_set_field_by_name(bag, key, 9.0);
        super::super::set_property_attrs(
            bag as usize,
            "warmArrayEnd".into(),
            super::super::PropertyAttrs::new(false, true, true),
        );
        let answer = unsafe { try_warm_array_transition(arrays[2], key, 8.0, &mut None) };
        super::super::js_object_delete_dynamic_value(
            crate::value::js_nanbox_pointer(bag as i64),
            key_value,
        );
        assert_eq!(answer, None, "a descriptor edit must retire the bag proof");
    }

    fn fresh_test_function() -> f64 {
        extern "C" fn body(
            _c: *const crate::closure::ClosureHeader,
            _this: crate::closure::JsThis,
        ) -> f64 {
            1.0
        }
        static INFO: crate::closure::JsFunctionInfo = crate::closure::JsFunctionInfo::of(
            body as crate::codegen_abi::JsBody0<crate::closure::ClosureHeader>,
        );
        crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(&INFO, 0) as i64)
    }

    #[test]
    fn own_slot_plan_cannot_hide_a_later_declaration() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let key = crate::string::canonical_key(b"pos");
        let keys = crate::array::js_array_alloc(4);
        let keys = crate::array::js_array_push(keys, crate::JSValue::string_ptr(key as *mut _));
        let keys = crate::array::js_array_push(keys, crate::JSValue::string_ptr(key as *mut _));
        let obj = super::super::alloc_plain::alloc_plain_record_with_keys(2, unsafe {
            super::super::ObjectKeys::owned(keys)
        });
        let record = unsafe { shapes::object_shape_record(obj).unwrap() };
        let actual_keys = record.keys() as usize;
        super::super::prop_plan::read_plan_record(actual_keys, key as usize, 0);
        assert_eq!(
            super::super::prop_plan::read_plan_lookup(actual_keys, key as usize, 2),
            Some(0),
            "the stale prefix hint must be present before testing the guard"
        );
        assert_eq!(
            unsafe {
                shapes::object_shape_record(obj)
                    .unwrap()
                    .own_store_slot(key)
            },
            Some(Some(1)),
            "the earlier prefix plan must not hide the subclass declaration"
        );
    }

    #[test]
    fn function_dictionary_adds_reuse_the_named_bag_transition() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let a = fresh_test_function();
        let b = fresh_test_function();
        let prototype = super::super::js_object_alloc(0, 0);
        for owner in [a, b] {
            super::super::js_object_set_prototype_of(
                owner,
                crate::value::js_nanbox_pointer(prototype as i64),
            );
        }
        let first =
            crate::value::js_nanbox_string(crate::string::canonical_key(b"rangeEnd") as i64);
        let second =
            crate::value::js_nanbox_string(crate::string::canonical_key(b"rangePos") as i64);
        for owner in [a, b] {
            assert_eq!(unsafe { try_store(owner, first, 1.0) }, Some(1.0));
        }
        super::super::test_reset_transition_fast_hits();
        for owner in [a, b] {
            assert_eq!(unsafe { try_store(owner, second, 2.0) }, Some(2.0));
        }
        assert!(
            super::super::test_transition_fast_hits() > 0,
            "a second named add must reuse the edge after the FunctionDictionary transition"
        );
        unsafe {
            let bags = [a, b].map(|v| {
                crate::closure::props::bag_of((v.to_bits() & crate::value::POINTER_MASK) as usize)
            });
            assert_eq!(
                shapes::object_shape_stamp(bags[0]),
                shapes::object_shape_stamp(bags[1])
            );
            assert_eq!(
                super::super::js_object_get_prototype_of(crate::value::js_nanbox_pointer(
                    bags[0] as i64
                ))
                .to_bits(),
                crate::value::TAG_NULL
            );
        }
    }

    #[test]
    fn function_dictionary_asks_its_actual_prototype_own_attributes() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let a = fresh_test_function();
        let b = fresh_test_function();
        let first =
            crate::value::js_nanbox_string(crate::string::canonical_key(b"rangeEnd") as i64);
        let key = crate::string::canonical_key(b"rangePos");
        let key_value = crate::value::js_nanbox_string(key as i64);
        for owner in [a, b] {
            assert_eq!(unsafe { try_store(owner, first, 1.0) }, Some(1.0));
        }
        let proto = super::super::js_object_alloc(0, 0);
        super::super::js_object_set_field_by_name(proto, key, 9.0);
        super::super::set_property_attrs(
            proto as usize,
            "rangePos".into(),
            super::super::PropertyAttrs::new(false, true, true),
        );
        super::super::js_object_set_prototype_of(a, crate::value::js_nanbox_pointer(proto as i64));
        let other_proto = super::super::js_object_alloc(0, 0);
        super::super::js_object_set_prototype_of(
            b,
            crate::value::js_nanbox_pointer(other_proto as i64),
        );
        unsafe {
            let id = |v: f64| {
                (*((v.to_bits() & crate::value::POINTER_MASK)
                    as *const crate::closure::ClosureHeader))
                    .shape_id
            };
            assert_eq!(
                id(a),
                id(b),
                "FunctionDictionary is shared across these distinct prototype chains"
            );
        }
        assert_eq!(
            unsafe { try_store(a, key_value, 2.0) },
            None,
            "the actual prototype's own readonly key must decline"
        );
        assert!(!super::super::obj_value_has_own_key(a, key_value));
        assert_eq!(
            unsafe { try_store(b, key_value, 2.0) },
            Some(2.0),
            "the same FunctionDictionary id with a different prototype must remain writable"
        );
    }

    #[test]
    fn warm_transition_uses_the_shape_proof_before_put_value_roots() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let proto = super::super::js_object_alloc(0, 0);
        let a = super::super::js_object_alloc(0, 0);
        let b = super::super::js_object_alloc(0, 0);
        let c = super::super::js_object_alloc(0, 0);
        for obj in [a, b, c] {
            unsafe { super::super::alloc_plain::mark_object_plain_ordinary(obj) };
            super::super::js_object_set_prototype_of(
                crate::value::js_nanbox_pointer(obj as i64),
                crate::value::js_nanbox_pointer(proto as i64),
            );
        }
        let key = crate::string::canonical_key(b"rangePos");
        let a_value = crate::value::js_nanbox_pointer(a as i64);
        crate::proxy::js_put_value_set(
            a_value,
            crate::value::js_nanbox_string(key as i64),
            1.0,
            a_value,
            0,
        );
        super::super::test_reset_transition_fast_hits();
        assert_eq!(
            unsafe { try_warm_transition(b, key, 7.0, &mut None) },
            Some(7.0)
        );
        assert!(super::super::test_transition_fast_hits() > 0);
        super::super::js_object_set_field_by_name(proto, key, 9.0);
        super::super::set_property_attrs(
            proto as usize,
            "rangePos".into(),
            super::super::PropertyAttrs::new(false, true, true),
        );
        assert_eq!(unsafe { try_warm_transition(c, key, 8.0, &mut None) }, None);
        let c_value = crate::value::js_nanbox_pointer(c as i64);
        assert!(!super::super::obj_value_has_own_key(
            c_value,
            crate::value::js_nanbox_string(key as i64)
        ));
    }

    #[test]
    fn own_slot_lookup_answers_empty_and_wide_layouts() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let obj = super::super::js_object_alloc(0, 0);
        unsafe { super::super::alloc_plain::mark_object_plain_ordinary(obj) };
        let key = crate::string::canonical_key(b"pos");
        let absent = crate::string::canonical_key(b"absent");
        unsafe {
            assert_eq!(
                shapes::object_shape_record(obj)
                    .unwrap()
                    .own_store_slot(key),
                Some(None)
            );
        }
        super::super::js_object_set_field_by_name(obj, key, 1.0);
        unsafe {
            let record = shapes::object_shape_record(obj).unwrap();
            assert_eq!(record.own_store_slot(key), Some(Some(0)));
            assert_eq!(record.own_store_slot(absent), Some(None));
        }
        for i in 0..super::super::KEYS_INDEX_THRESHOLD {
            let name = format!("field{i}");
            super::super::js_object_set_field_by_name(
                obj,
                crate::string::canonical_key(name.as_bytes()),
                1.0,
            );
        }
        unsafe {
            let record = shapes::object_shape_record(obj).unwrap();
            assert_eq!(record.own_store_slot(key), Some(Some(0)));
            assert_eq!(record.own_store_slot(absent), Some(None));
        }
        super::super::set_property_attrs(
            obj as usize,
            "pos".into(),
            super::super::PropertyAttrs::new(false, true, true),
        );
        unsafe {
            assert_eq!(
                shapes::object_shape_record(obj)
                    .unwrap()
                    .own_store_slot(key),
                Some(Some(0))
            );
            assert!(!try_own_store(obj, key, 2.0));
        }
    }

    #[test]
    fn own_spill_store_answers_from_the_shape() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let obj = super::super::js_object_alloc(0, 0);
        unsafe { super::super::alloc_plain::mark_object_plain_ordinary(obj) };
        for name in [b"pos".as_slice(), b"end", b"parent", b"transformFlags"] {
            super::super::js_object_set_field_by_name(obj, crate::string::canonical_key(name), 1.0);
        }
        let key = crate::string::canonical_key(b"transformFlags");
        assert!(
            unsafe { try_own_store(obj, key, 37.0) },
            "an existing spill slot must take the shape answer"
        );
        assert_eq!(
            super::super::js_object_get_field_by_name_f64(obj, key),
            37.0
        );
        super::super::set_property_attrs(
            obj as usize,
            "transformFlags".into(),
            super::super::PropertyAttrs::new(false, true, true),
        );
        assert!(!unsafe { try_own_store(obj, key, 38.0) });
        assert_eq!(
            super::super::js_object_get_field_by_name_f64(obj, key),
            37.0
        );
    }

    #[test]
    fn function_named_add_reuses_the_bag_transition() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        extern "C" fn body(
            _c: *const crate::closure::ClosureHeader,
            _this: crate::closure::JsThis,
        ) -> f64 {
            1.0
        }
        static INFO: crate::closure::JsFunctionInfo = crate::closure::JsFunctionInfo::of(
            body as crate::codegen_abi::JsBody0<crate::closure::ClosureHeader>,
        );
        let key = crate::string::canonical_key(b"rangeEnd");
        let key_value = crate::value::js_nanbox_string(key as i64);
        let a = crate::closure::js_closure_alloc(&INFO, 0);
        let b = crate::closure::js_closure_alloc(&INFO, 0);
        assert_eq!(
            unsafe { try_store(crate::value::js_nanbox_pointer(a as i64), key_value, 1.0) },
            Some(1.0)
        );
        super::super::test_reset_transition_fast_hits();
        assert_eq!(
            unsafe { try_store(crate::value::js_nanbox_pointer(b as i64), key_value, 2.0) },
            Some(2.0)
        );
        assert!(
            super::super::test_transition_fast_hits() > 0,
            "the second function must take the learned edge"
        );
    }

    #[test]
    fn generic_set_uses_indexed_prototype_attributes() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let proto = super::super::js_object_alloc(0, 0);
        let obj = super::super::js_object_alloc(0, 0);
        let key = crate::string::canonical_key(b"rangeEnd");
        super::super::js_object_set_field_by_name(proto, key, 9.0);
        super::super::set_property_attrs(
            proto as usize,
            "rangeEnd".into(),
            super::super::PropertyAttrs::new(false, true, true),
        );
        let target = crate::value::js_nanbox_pointer(obj as i64);
        super::super::js_object_set_prototype_of(
            target,
            crate::value::js_nanbox_pointer(proto as i64),
        );
        assert!(unsafe {
            super::super::proto_validity::object_is_marked_prototype(proto as usize)
        });
        let key_value = crate::value::js_nanbox_string(key as i64);
        let result =
            crate::proxy::reflect_ordinary_set_with_receiver(target, key_value, 3.0, target);
        assert_eq!(crate::value::js_is_truthy(result), 0);
        assert!(!super::super::obj_value_has_own_key(target, key_value));
    }

    #[test]
    fn own_store_uses_the_shape_and_never_accepts_a_frozen_slot() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let obj = super::super::js_object_alloc(0, 0);
        unsafe { super::super::alloc_plain::mark_object_plain_ordinary(obj) };
        let key = crate::string::canonical_key(b"pos");
        super::super::js_object_set_field_by_name(obj, key, 1.0);
        for name in [b"end".as_slice(), b"parent", b"transformFlags"] {
            super::super::js_object_set_field_by_name(obj, crate::string::canonical_key(name), 0.0);
        }
        assert!(
            unsafe { try_own_store(obj, key, 2.0) },
            "shape store must be live"
        );
        assert_eq!(super::super::js_object_get_field_by_name_f64(obj, key), 2.0);
        super::super::set_property_attrs(
            obj as usize,
            "pos".into(),
            super::super::PropertyAttrs::new(false, true, true),
        );
        assert!(!unsafe { try_own_store(obj, key, 3.0) });
        assert_eq!(super::super::js_object_get_field_by_name_f64(obj, key), 2.0);
    }

    #[test]
    fn array_named_chain_includes_object_prototype_after_warmup() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _ = super::super::js_get_global_this();
        let key = crate::string::canonical_key(b"arrayGrandparentRange");
        let key_value = crate::value::js_nanbox_string(key as i64);
        let arr = crate::array::js_array_alloc(4);
        let receiver = crate::value::js_nanbox_pointer(arr as i64);
        assert_eq!(unsafe { try_store(receiver, key_value, 1.0) }, Some(1.0));
        let proto = crate::array::object_prototype_addr() as *mut ObjectHeader;
        super::super::js_object_set_field_by_name(proto, key, 9.0);
        super::super::set_property_attrs(
            proto as usize,
            "arrayGrandparentRange".into(),
            super::super::PropertyAttrs::new(false, true, true),
        );
        let fresh = crate::array::js_array_alloc(4);
        let fresh_value = crate::value::js_nanbox_pointer(fresh as i64);
        let answer = unsafe { try_store(fresh_value, key_value, 2.0) };
        super::super::js_object_delete_dynamic_value(
            crate::value::js_nanbox_pointer(proto as i64),
            key_value,
        );
        assert_eq!(answer, None, "Object.prototype is part of an array's chain");
        assert!(!super::super::obj_value_has_own_key(fresh_value, key_value));
    }

    #[test]
    fn array_chain_projection_declines_a_changed_prototype_after_warmup() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let key = crate::string::canonical_key(b"rangeEnd");
        let key_value = crate::value::js_nanbox_string(key as i64);
        let arr = crate::array::js_array_alloc(4);
        let receiver = crate::value::js_nanbox_pointer(arr as i64);
        assert_eq!(unsafe { try_store(receiver, key_value, 1.0) }, Some(1.0));
        let prototype = super::super::js_object_get_prototype_of(receiver);
        let original = super::super::js_object_get_prototype_of(prototype);
        let readonly = super::super::js_object_alloc(0, 0);
        super::super::js_object_set_field_by_name(readonly, key, 9.0);
        super::super::set_property_attrs(
            readonly as usize,
            "rangeEnd".into(),
            super::super::PropertyAttrs::new(false, true, true),
        );
        super::super::js_object_set_prototype_of(
            prototype,
            crate::value::js_nanbox_pointer(readonly as i64),
        );
        let fresh = crate::array::js_array_alloc(4);
        let fresh_value = crate::value::js_nanbox_pointer(fresh as i64);
        let answer = unsafe { try_store(fresh_value, key_value, 2.0) };
        super::super::js_object_set_prototype_of(prototype, original);
        assert_eq!(answer, None, "a custom array chain needs the complete Set");
    }

    #[test]
    fn array_named_bags_share_store_shapes_and_read_back_inline_and_spill() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let key = crate::string::canonical_key(b"pos");
        let key_value = crate::value::js_nanbox_string(key as i64);
        let mut bags = Vec::new();
        for value in [11.0, 19.0] {
            let arr = crate::array::js_array_alloc_with_length(4);
            let receiver = crate::value::js_nanbox_pointer(arr as i64);
            assert_eq!(
                unsafe { try_store(receiver, key_value, value) },
                Some(value),
                "named add must take the shaped path"
            );
            let bag = unsafe { crate::array::array_property_bag(arr) };
            assert!(!bag.is_null());
            assert_eq!(
                unsafe { shapes::object_shape_record(bag).unwrap().object_kind() },
                shapes::ShapeObjectKind::Ordinary
            );
            bags.push(bag);
        }
        assert_eq!(unsafe { shapes::object_shape_stamp(bags[0]) }, unsafe {
            shapes::object_shape_stamp(bags[1])
        });
        let scope = crate::gc::RuntimeHandleScope::new();
        // Read shaped stores back through the ordinary array getter, for
        // both inline and spill storage.
        let arr = crate::array::js_array_alloc_with_length(4);
        let receiver = crate::value::js_nanbox_pointer(arr as i64);
        assert_eq!(unsafe { try_store(receiver, key_value, 23.0) }, Some(23.0));
        let arr_root = scope.root_raw_mut_ptr(arr);
        let mut cache: crate::object::PicCache = [0; crate::object::PIC_CACHE_WORDS];
        let mut cache_slot = &mut cache as *mut _;
        let packed = std::sync::atomic::AtomicU64::new(0xFFFF_FFFF);
        assert_eq!(
            crate::object::field_get_set::js_object_get_field_ic_miss_packed(
                arr_root.get_raw_mut_ptr::<crate::ArrayHeader>().cast(),
                key,
                &mut cache_slot,
                &packed
            ),
            23.0
        );
        for name in [b"end".as_slice(), b"hasTrailingComma", b"transformFlags"] {
            let key = crate::string::canonical_key(name);
            let receiver = crate::value::js_nanbox_pointer(
                arr_root.get_raw_mut_ptr::<crate::ArrayHeader>() as i64,
            );
            assert_eq!(
                unsafe { try_store(receiver, crate::value::js_nanbox_string(key as i64), 29.0) },
                Some(29.0)
            );
        }
        let spill_key = crate::string::canonical_key(b"transformFlags");
        assert_eq!(
            crate::object::field_get_set::js_object_get_field_ic_miss_packed(
                arr_root.get_raw_mut_ptr::<crate::ArrayHeader>().cast(),
                spill_key,
                &mut cache_slot,
                &packed
            ),
            29.0
        );
    }
    #[test]
    fn chain_fact_is_shape_owned_and_rejected_after_mutation() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let proto = super::super::js_object_alloc(0, 0);
        let a = super::super::js_object_alloc(0, 0);
        let b = super::super::js_object_alloc(0, 0);
        let proto_value = crate::value::js_nanbox_pointer(proto as i64);
        for obj in [a, b] {
            super::super::js_object_set_prototype_of(
                crate::value::js_nanbox_pointer(obj as i64),
                proto_value,
            );
        }
        let shape = unsafe { shapes::object_shape_record(a) }.unwrap();
        let key = crate::string::js_string_from_bytes(b"rangePos".as_ptr(), 8);
        let key_value = crate::value::js_nanbox_string(key as i64);
        let a_value = crate::value::js_nanbox_pointer(a as i64);
        crate::proxy::js_put_value_set(a_value, key_value, 1.0, a_value, 0);
        let validity = super::super::proto_validity::proto_validity();
        assert_eq!(
            shape.store_chain_blocks(b"rangePos", validity),
            Some(false),
            "the proof must be live"
        );
        let b_shape = unsafe { shapes::object_shape_record(b) }.unwrap();
        assert_eq!(
            b_shape.store_chain_blocks(b"rangePos", validity),
            Some(false),
            "a different receiver uses its shape's fact"
        );
        super::super::set_property_attrs(
            proto as usize,
            "rangePos".into(),
            super::super::PropertyAttrs::new(false, true, true),
        );
        let new_validity = super::super::proto_validity::proto_validity();
        assert_ne!(new_validity, validity);
        assert_eq!(b_shape.store_chain_blocks(b"rangePos", new_validity), None);
        let b_value = crate::value::js_nanbox_pointer(b as i64);
        crate::proxy::js_put_value_set(b_value, key_value, 2.0, b_value, 0);
        assert!(!super::super::obj_value_has_own_key(b_value, key_value));
    }
}

/// No user code runs on a successful own store or transition. An unproven
/// chain, exotic receiver, or exceptional key retains the complete [[Set]].
#[cfg(test)]
pub(crate) unsafe fn try_store(target: f64, key: f64, value: f64) -> Option<f64> {
    try_store_with_refresh(target, key, value, &mut false)
}

/// Report whether this attempt entered an allocating path. A decline before
/// that point keeps the caller's already refreshed operands valid.
pub(crate) unsafe fn try_store_with_refresh(
    target: f64,
    key: f64,
    value: f64,
    may_move: &mut bool,
) -> Option<f64> {
    use crate::value::{POINTER_MASK, POINTER_TAG, STRING_TAG, TAG_MASK};
    if target.to_bits() & TAG_MASK != POINTER_TAG || key.to_bits() & TAG_MASK != STRING_TAG {
        return None;
    }
    let key_ptr = (key.to_bits() & POINTER_MASK) as *const crate::StringHeader;
    let name = crate::string::header_str_checked(key_ptr)?;
    let bytes = name.as_bytes();
    if bytes
        .first()
        .is_some_and(|b| *b == b'#' || b.is_ascii_digit())
    {
        return None;
    }
    let addr = (target.to_bits() & POINTER_MASK) as usize;
    let header = crate::value::addr_class::try_read_gc_header(addr)?;
    if header.obj_type == crate::gc::GC_TYPE_ARRAY {
        return try_array_store(target, key, value, name, may_move);
    }
    if header.obj_type == crate::gc::GC_TYPE_CLOSURE {
        if header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
            return None;
        }
        return try_function_store(target, key, value, name, may_move);
    }
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    let obj = addr as *mut ObjectHeader;
    let shape = shapes::object_shape_descriptor(obj)?;
    if !shape.object_kind.is_ordinary_layout()
        || (*obj).class_id == super::NATIVE_MODULE_CLASS_ID
        || super::dictionary::is_dictionary(obj)
    {
        return None;
    }
    if super::try_existing_own_data_overwrite_by_content(obj, key, bytes, value) {
        return Some(value);
    }
    if header._reserved
        & (crate::gc::OBJ_FLAG_FROZEN
            | crate::gc::OBJ_FLAG_SEALED
            | crate::gc::OBJ_FLAG_NO_EXTEND
            | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO)
        != 0
        || !super::own_descriptors_skip_key(addr, key)
        || bytes == b"__proto__"
    {
        return None;
    }
    // A transition edge only exists for an absent own key. The prototype
    // fact is attached to its predecessor shape, never to the call site.
    let record = shape.record_ref()?;
    let blocked = match inherited_store_blocked(record, key_ptr, bytes) {
        Some(blocked) => blocked,
        None => {
            *may_move = true;
            let scope = crate::gc::RuntimeHandleScope::new();
            let obj_root = scope.root_raw_mut_ptr(obj);
            let key_root = scope.root_string_ptr(key_ptr);
            let value_root = scope.root_nanbox_f64(value);
            if !super::store_key::mark_chain_hops(&scope, target) {
                return None;
            }
            let obj = obj_root.get_raw_mut_ptr::<ObjectHeader>();
            let key_ptr = key_root.get_raw_const_ptr::<crate::StringHeader>();
            let key = crate::value::js_nanbox_string(key_ptr as i64);
            let blocked = super::plain_data_write_may_intercept(obj as usize, (*obj).class_id, key);
            let record = shapes::object_shape_record(obj)?;
            record.note_store_chain(
                crate::string::header_str_checked(key_ptr)?.as_bytes(),
                super::proto_validity::proto_validity(),
                blocked,
            );
            if blocked {
                return None;
            }
            return apply_proven_add(&obj_root, &key_root, &value_root);
        }
    };
    if blocked {
        return None;
    }
    *may_move = true;
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_root = scope.root_raw_mut_ptr(obj);
    let key_root = scope.root_string_ptr(key_ptr);
    let value_root = scope.root_nanbox_f64(value);
    apply_proven_add(&obj_root, &key_root, &value_root)
}

/// A proven absent key can mint its first edge through the same own-data
/// funnel that later transition hits reuse. A missing edge is not a reason
/// to repeat [[Set]]'s descriptor and prototype walk.
unsafe fn apply_proven_add(
    owner: &crate::gc::RuntimeHandle<'_>,
    key: &crate::gc::RuntimeHandle<'_>,
    value: &crate::gc::RuntimeHandle<'_>,
) -> Option<f64> {
    if super::object_set_field_by_name_transition_chain_proven_value(
        owner.get_raw_mut_ptr::<ObjectHeader>(),
        key.get_raw_const_ptr::<crate::StringHeader>(),
        value.get_nanbox_f64(),
        &mut None,
    )
    .is_some()
    {
        return Some(value.get_nanbox_f64());
    }
    // A keys-only claim cannot publish the body-specific successor of a
    // permanent-image function. Its first add stays in the existing value
    // publication funnel; subsequent adds use that learned transition above.
    if super::field_rep_store::constfn_store_info(value.get_nanbox_f64().to_bits()).is_some() {
        return None;
    }
    let obj = owner.get_raw_mut_ptr::<ObjectHeader>();
    let key_ptr = key.get_raw_const_ptr::<crate::StringHeader>();
    let record = shapes::object_shape_record(obj)?;
    if record.object_kind() != shapes::ShapeObjectKind::Ordinary
        || record.summary() & super::key_attrs::SUMMARY_PRIVATE != 0
        || super::class_registry::is_class_object_ptr(obj.cast())
        || record.own_store_slot(key_ptr) != Some(None)
        || crate::value::addr_class::try_read_gc_header(obj as usize)?._reserved
            & (crate::gc::OBJ_FLAG_FROZEN
                | crate::gc::OBJ_FLAG_SEALED
                | crate::gc::OBJ_FLAG_NO_EXTEND
                | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO)
            != 0
    {
        return None;
    }
    // Absence and the complete ordered key count prove where this append
    // lands. Reuse the existing key-add funnel, then write that slot; the
    // general descriptor helper would scan the whole list again.
    let slot = record.logical_key_count() as usize;
    let predecessor = shapes::object_shape_stamp(obj);
    let predecessor_rep = record.rep();
    let number_lane = slot < super::field_rep::REP_SLOTS as usize
        && slot < super::object_inline_capacity(obj) as usize
        && super::field_rep::f64_slot_bits(value.get_nanbox_f64().to_bits()).is_some();
    if number_lane {
        // Publish the value-bearing edge directly, as the named-store tail
        // does. A keys-only Any intermediary would incorrectly count as a
        // non-Number sibling and deprecate this new F64 lane at convergence.
        let atom = crate::string::js_string_intern(key_ptr, super::key_content_hash(key_ptr));
        if atom.is_null() {
            return None;
        }
        let obj = owner.get_raw_mut_ptr::<ObjectHeader>();
        let proof = super::canonical_keys::SharedLayout::of_receiver(obj)?;
        let keys = super::object_keys(obj);
        let parent = super::canonical_keys::canonicalize(&proof, keys.arr(), slot as u32);
        let next = super::canonical_keys::extend_key(
            &proof,
            parent,
            key.get_raw_const_ptr::<crate::StringHeader>(),
        );
        super::field_rep_store::publish_key_add_edge(
            owner.get_raw_mut_ptr::<ObjectHeader>(),
            next.view(),
            predecessor_rep,
            slot as u32,
            Some(value.get_nanbox_f64().to_bits()),
            true,
        );
        let obj = owner.get_raw_mut_ptr::<ObjectHeader>();
        if super::dictionary::is_dictionary(obj) {
            return None;
        }
        super::js_object_set_field(
            obj,
            slot as u32,
            crate::JSValue::from_bits(value.get_nanbox_f64().to_bits()),
        );
        let obj = owner.get_raw_mut_ptr::<ObjectHeader>();
        if let Some(atom) = super::store_key::interned_key_for_store(
            crate::value::js_nanbox_string(key.get_raw_const_ptr::<crate::StringHeader>() as i64),
        ) {
            super::transition_cache_insert(
                std::ptr::null(),
                predecessor,
                atom,
                super::object_keys(obj).arr() as usize,
                slot as u32,
                shapes::object_shape_stamp(obj),
            );
        }
        return Some(value.get_nanbox_f64());
    }
    super::object_ops::ensure_key_in_keys_array_for_value(
        owner.get_raw_mut_ptr::<ObjectHeader>(),
        key.get_raw_const_ptr::<crate::StringHeader>(),
        value.get_nanbox_f64(),
    );
    let obj = owner.get_raw_mut_ptr::<ObjectHeader>();
    // Appending can transfer the list to the receiver's dictionary. That
    // store no longer has a shape-owned slot; the complete setter writes it.
    if super::dictionary::is_dictionary(obj) {
        return None;
    }
    let obj = owner.get_raw_mut_ptr::<ObjectHeader>();
    let bits = value.get_nanbox_f64().to_bits();
    if slot < super::object_inline_capacity(obj) as usize {
        super::js_object_set_field(obj, slot as u32, crate::JSValue::from_bits(bits));
    } else {
        super::overflow_set(obj as usize, slot, bits);
    }
    Some(value.get_nanbox_f64())
}

/// Array named properties use the same shaped bag as customized descriptors.
/// Its prototype edge names the array chain; elements and length stay exotic.
unsafe fn try_function_store(
    target: f64,
    key: f64,
    value: f64,
    name: &str,
    may_move: &mut bool,
) -> Option<f64> {
    let ptr = (target.to_bits() & crate::value::POINTER_MASK) as usize;
    let key_ptr = (key.to_bits() & crate::value::POINTER_MASK) as *const crate::StringHeader;
    // Intrinsic function keys retain their own descriptor/materialization rules.
    if matches!(
        name,
        "name" | "length" | "prototype" | "caller" | "arguments" | "__proto__"
    ) || name.starts_with('#')
        || super::canonical_array_index(name).is_some()
    {
        return None;
    }
    let closure = ptr as *const crate::closure::ClosureHeader;
    let descriptor = shapes::shape_descriptor_by_id((*closure).shape_id)?;
    if crate::closure::shape::is_class_info((*closure).info) {
        return None;
    }
    let flags = crate::value::addr_class::try_read_gc_header(ptr)?._reserved;
    if flags & crate::gc::OBJ_FLAG_FROZEN != 0 {
        return None;
    }
    let bag = (*closure).props;
    if !bag.is_null() && crate::closure::props::bag_has_own(ptr, name.as_bytes()) {
        return super::try_existing_own_data_overwrite_by_content(bag, key, name.as_bytes(), value)
            .then_some(value);
    }
    if !matches!(
        descriptor.object_kind,
        shapes::ShapeObjectKind::Function | shapes::ShapeObjectKind::FunctionDictionary
    ) || flags & (crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND) != 0
    {
        return None;
    }
    *may_move = true;
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(target);
    let key_root = scope.root_string_ptr(key_ptr);
    let value_root = scope.root_nanbox_f64(value);
    let prototype =
        scope.root_nanbox_f64(super::js_object_get_prototype_of(owner.get_nanbox_f64()));
    if prototype.get_nanbox_f64().to_bits() != crate::value::TAG_NULL {
        let bits = prototype.get_nanbox_f64().to_bits();
        if bits & crate::value::TAG_MASK != crate::value::POINTER_TAG {
            return None;
        }
        let addr = (bits & crate::value::POINTER_MASK) as usize;
        let header = crate::value::addr_class::try_read_gc_header(addr)?;
        if header.obj_type != crate::gc::GC_TYPE_OBJECT
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return None;
        }
        let proto = addr as *const ObjectHeader;
        let record = shapes::object_shape_record(proto)?;
        if !record.object_kind().is_ordinary_layout() {
            return None;
        }
        let name =
            crate::string::header_str_checked(key_root.get_raw_const_ptr::<crate::StringHeader>())?;
        // A FunctionDictionary's prototype is per-object, so its shared
        // ShapeId cannot authorize a chain. Ask the actual prototype shape's
        // own attributes plus its inherited-chain fact. The named bag retains
        // its null prototype for all existing DefineOwnProperty writers.
        if super::key_attrs::object_key_blocks_plain_store(proto, name.as_bytes()) {
            return None;
        }
        let validity = super::proto_validity::proto_validity();
        match record.store_chain_blocks(name.as_bytes(), validity) {
            Some(true) => return None,
            Some(false) => {}
            None => {
                if !super::store_key::mark_chain_hops(&scope, owner.get_nanbox_f64()) {
                    return None;
                }
                let key = key_root.get_raw_const_ptr::<crate::StringHeader>();
                let name = crate::string::header_str_checked(key)?;
                let blocked = chain_blocks(prototype.get_nanbox_f64(), name.as_bytes());
                let proto = (prototype.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK)
                    as *const ObjectHeader;
                shapes::object_shape_record(proto)?.note_store_chain(
                    name.as_bytes(),
                    super::proto_validity::proto_validity(),
                    blocked,
                );
                if blocked {
                    return None;
                }
            }
        }
    }
    let ptr = (owner.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK) as usize;
    let bag = crate::closure::props::bag_ensure(ptr);
    let bag_root = scope.root_raw_mut_ptr(bag);
    if shapes::object_shape_record(bag)?.object_kind() == shapes::ShapeObjectKind::OrdinaryUnmarked
    {
        super::alloc_plain::mark_object_plain_ordinary(bag);
    }
    let key_ptr = key_root.get_raw_const_ptr::<crate::StringHeader>();
    let mut refresh = None;
    if super::object_set_field_by_name_transition_chain_proven_value(
        bag,
        key_ptr,
        value_root.get_nanbox_f64(),
        &mut refresh,
    )
    .is_none()
    {
        super::object_ops::define_property_force_store_value(
            bag_root.get_raw_mut_ptr::<ObjectHeader>(),
            key_root.get_raw_const_ptr::<crate::StringHeader>(),
            value_root.get_nanbox_f64(),
        );
    }
    let ptr = (owner.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK) as usize;
    // The ordinary function writer also revives a deleted configurable key.
    // The bag's new data binding and the function's deletion state must agree.
    let name =
        crate::string::header_str_checked(key_root.get_raw_const_ptr::<crate::StringHeader>())?;
    crate::closure::props::state_clear_deleted(ptr, name);
    crate::closure::shape::refresh_closure_shape(ptr);
    Some(value_root.get_nanbox_f64())
}

/// Prototype key summaries are the proof: ordinary writable data cannot
/// intercept a named add. Unknown kinds and Proxy hops keep the full Set.
unsafe fn chain_blocks(receiver: f64, key: &[u8]) -> bool {
    let mut hop = super::js_object_get_prototype_of(receiver);
    for _ in 0..64 {
        if hop.to_bits() == crate::value::TAG_NULL {
            return false;
        }
        if hop.to_bits() & crate::value::TAG_MASK != crate::value::POINTER_TAG {
            return true;
        }
        let addr = (hop.to_bits() & crate::value::POINTER_MASK) as usize;
        let Some(header) = crate::value::addr_class::try_read_gc_header(addr) else {
            return true;
        };
        if header.obj_type != crate::gc::GC_TYPE_OBJECT {
            return true;
        }
        let obj = addr as *const ObjectHeader;
        let Some(shape) = shapes::object_shape_descriptor(obj) else {
            return true;
        };
        if !shape.object_kind.is_ordinary_layout() {
            return true;
        }
        if shape.summary & super::key_attrs::SUMMARY_BLOCKS_STORE != 0
            && super::key_attrs::object_key_blocks_plain_store(obj, key)
        {
            return true;
        }
        hop = super::js_object_get_prototype_of(hop);
    }
    true
}
