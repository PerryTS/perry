//! `delete obj[key]` of the key a plain object added LAST: the rollback edge
//! (`shapes::publish_object_shape_last_key_rollback`) and a cleared slot,
//! without the exotic-receiver ladder of `js_object_delete_field`.
//!
//! Only a receiver for which that ladder has nothing to say is admitted: a
//! class-less ordinary object with no meta record (so not a prototype, no
//! accessor or element record, no spill buffer), no descriptor, tombstone,
//! freeze or seal flag, and none of the class-less exotics (`process.env`, an
//! arguments object, a URL record, `Object.prototype`). For such an object
//! every own key is a configurable data property, and deleting the last one
//! is exactly the rollback. Everything else, and every other key, returns
//! `None` for the full path.

use super::ObjectHeader;

/// Delete own key `key_bits` (a heap or SSO string, text `key_bytes`) from
/// `obj` when it is the last key of an admitted receiver (module docs).
/// `Some(1)` when deleted; `None`, having changed nothing, otherwise.
///
/// # Safety
/// `obj` is a candidate object address (validated here before any read).
pub(crate) unsafe fn try_delete_last_added_key(
    obj: *mut ObjectHeader,
    key_bits: u64,
    key_bytes: &[u8],
) -> Option<i32> {
    let record = admitted_receiver(obj)?;
    let slot = record.own_data_slot_of_value(key_bits, key_bytes)??;
    delete_admitted_last_key(obj, record, slot)
}

/// [`try_delete_last_added_key`] for a caller that already resolved the
/// key's slot on `obj`'s current shape, with nothing allocated since.
///
/// # Safety
/// As [`try_delete_last_added_key`]; `slot` is the key's slot on the shape
/// `obj` carries now.
pub(crate) unsafe fn try_delete_last_added_key_at(
    obj: *mut ObjectHeader,
    slot: u32,
) -> Option<i32> {
    let record = admitted_receiver(obj)?;
    delete_admitted_last_key(obj, record, slot)
}

/// `obj`'s shape record when `obj` is a receiver the fast delete admits (the
/// module docs), else `None`.
unsafe fn admitted_receiver(obj: *mut ObjectHeader) -> Option<super::shapes::ShapeRecordRef> {
    let addr = obj as usize;
    let header = crate::value::addr_class::try_read_gc_header(addr)?;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved
            & (crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
                | crate::gc::OBJ_FLAG_STABLE_TOMBSTONES
                | crate::gc::OBJ_FLAG_FROZEN
                | crate::gc::OBJ_FLAG_SEALED
                | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO)
            != 0
        || (*obj).class_id != 0
        || !(*obj).meta.is_null()
    {
        return None;
    }
    let id = (*obj).parent_class_id;
    if !super::shapes::is_site_matchable_shape_id(id) {
        return None;
    }
    let record = super::shapes::shape_record_by_id(id)?;
    if crate::process::is_process_env_ptr(addr)
        || super::is_arguments_object(obj)
        || crate::url::is_url_object_shape(obj)
        || crate::array::object_prototype_addr_matches(addr)
    {
        return None;
    }
    Some(record)
}

/// Delete the key at `slot` of an admitted receiver when it is the last key
/// of `record`, its shape.
unsafe fn delete_admitted_last_key(
    obj: *mut ObjectHeader,
    record: super::shapes::ShapeRecordRef,
    slot: u32,
) -> Option<i32> {
    if slot + 1 != record.logical_key_count() {
        return None;
    }
    // As `js_object_delete_field`: a delete retires the read plans keyed by
    // this receiver's keys array.
    super::prop_plan::prop_plan_epoch_bump_for_owner(obj as usize);
    rollback(obj, slot).then_some(1)
}

/// `delete obj[key]` with a dynamic key value: [`try_delete_last_added_key`]
/// for a string key.
///
/// # Safety
/// As [`try_delete_last_added_key`].
pub(crate) unsafe fn try_delete_last_added_dynamic(
    obj: *mut ObjectHeader,
    key: f64,
) -> Option<i32> {
    let key = crate::value::JSValue::from_bits(key.to_bits());
    let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let bytes = crate::string::js_string_key_bytes(key, &mut buf)?;
    try_delete_last_added_key(obj, key.bits(), bytes)
}

/// `delete obj.key` with a heap string key: [`try_delete_last_added_key`].
///
/// # Safety
/// As [`try_delete_last_added_key`]; `key` is a live string or null.
pub(crate) unsafe fn try_delete_last_added_field(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<i32> {
    if key.is_null() {
        return None;
    }
    let bytes =
        std::slice::from_raw_parts(crate::string::string_data(key), (*key).byte_len as usize);
    let bits = crate::value::JSValue::string_ptr(key as *mut crate::StringHeader).bits();
    try_delete_last_added_key(obj, bits, bytes)
}

/// Delete the key at `slot`, the last of `obj`'s list: move `obj` to the
/// shape of its list without it (the rollback edge) and clear the vacated
/// slot. `false`, having changed nothing, when the rollback is not admitted.
///
/// # Safety
/// `obj` is a live `ObjectHeader` whose shape lists `slot + 1` keys, and the
/// caller has established that the key may be deleted.
pub(crate) unsafe fn rollback(obj: *mut ObjectHeader, slot: u32) -> bool {
    let inline = (super::object_live_slot_count(obj) as usize).max(super::INLINE_SLOT_FLOOR);
    if super::shapes::publish_object_shape_last_key_rollback(obj, slot + 1) == 0 {
        return false;
    }
    let slot = slot as usize;
    if slot < inline {
        let fields = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
        let at = fields.add(slot) as usize;
        crate::gc::runtime_store_object_jsvalue_slot(
            obj as usize,
            at,
            slot,
            crate::value::TAG_UNDEFINED,
        );
    } else {
        super::overflow_set(obj as usize, slot, crate::value::TAG_UNDEFINED);
    }
    true
}

#[cfg(test)]
#[path = "delete_last_key_tests.rs"]
mod tests;
