//! Completed plain-record construction in the JSON suppression window.
use super::*;

/// Build the pointer-free object used by the bounded inline JSON parser.
///
/// The caller has already serviced any pending parse-boundary collection and
/// holds a [`crate::gc::GcSuppressScope`].  That makes the shape id stable
/// across the ordinary, fully-accounted arena allocation below.  We can
/// therefore publish the final shape at birth and initialize every physical
/// slot exactly once, instead of first publishing a temporary keyless shape
/// and overwriting its initialized live slots afterwards.
///
/// Unlike [`crate::arena::ConstructionBatch`], this retains the ordinary arena
/// allocation path: GC headers, object-start indexing, birth flags, pressure
/// accounting, and block rollover all remain identical to `js_object_alloc`.
#[inline(always)]
unsafe fn finish_inline_json_object(
    object: *mut ObjectHeader,
    keys: *mut ArrayHeader,
    shape_id: u32,
    fields: &[(JSValue, JSValue)],
) -> *mut ObjectHeader {
    debug_assert!(crate::gc::gc_is_suppressed());
    debug_assert!(fields
        .iter()
        .all(|(_, value)| !value.is_pointer() && !value.is_string()));

    let count = fields.len();
    let capacity = count.max(INLINE_SLOT_FLOOR);
    (*object).class_id = 0;
    (*object).parent_class_id = 0;
    // GC_STORE_AUDIT(INIT): fresh inline JSON objects have no metadata edge.
    (*object).meta = ptr::null_mut();
    assert!(shapes::try_birth_stamp_preinstalled_shape(
        object,
        shape_id,
        keys,
        count as u32,
    ));
    mark_object_plain_ordinary(object);

    let slots = object
        .cast::<u8>()
        .add(std::mem::size_of::<ObjectHeader>())
        .cast::<JSValue>();
    for (index, &(_, value)) in fields.iter().enumerate() {
        // GC_STORE_AUDIT(INIT): the bounded decoder accepts pointer-free
        // scalars and short strings only.
        slots.add(index).write(value);
    }
    for index in count..capacity {
        // GC_STORE_AUDIT(INIT): initialize physical inline slack for safe reuse.
        slots.add(index).write(JSValue::undefined());
    }
    crate::gc::layout_init_pointer_free(object.cast());
    object
}

#[inline(always)]
fn inline_json_object_size(field_count: usize) -> usize {
    let capacity = field_count.max(INLINE_SLOT_FLOOR);
    std::mem::size_of::<ObjectHeader>() + capacity * std::mem::size_of::<JSValue>()
}

/// Try the current nursery block without permitting collection or reserving a
/// block. A miss has no side effect and lets the caller service the ordinary
/// rollover trigger before retrying with rooted operands.
#[inline(never)]
pub(crate) unsafe fn try_object_from_inline_json_fields(
    keys: *mut ArrayHeader,
    shape_id: u32,
    fields: &[(JSValue, JSValue)],
) -> Option<*mut ObjectHeader> {
    let raw = crate::arena::arena_alloc_gc_no_collect(
        inline_json_object_size(fields.len()),
        8,
        crate::gc::GC_TYPE_OBJECT,
    );
    if raw.is_null() {
        return None;
    }
    Some(finish_inline_json_object(
        raw.cast(),
        keys,
        shape_id,
        fields,
    ))
}

/// Retry after the caller has serviced the ordinary block-rollover trigger.
/// GC remains suppressed only while the newborn lacks its final shape.
#[inline(never)]
pub(crate) unsafe fn object_from_inline_json_fields(
    keys: *mut ArrayHeader,
    shape_id: u32,
    fields: &[(JSValue, JSValue)],
) -> *mut ObjectHeader {
    let raw = crate::arena::arena_alloc_gc(
        inline_json_object_size(fields.len()),
        8,
        crate::gc::GC_TYPE_OBJECT,
    );
    finish_inline_json_object(raw.cast(), keys, shape_id, fields)
}

pub(crate) unsafe fn object_from_json_fields_preinstalled(
    batch: &mut Option<crate::arena::ConstructionBatch>,
    keys: *mut ArrayHeader,
    shape_id: u32,
    values: &[JSValue],
) -> *mut ObjectHeader {
    let count = values.len();
    let capacity = count.max(INLINE_SLOT_FLOOR);
    let size = std::mem::size_of::<ObjectHeader>() + capacity * 8;
    let raw = batch.as_mut().map_or(ptr::null_mut(), |b| {
        b.try_alloc(size, crate::gc::GC_TYPE_OBJECT)
    });
    let obj = if raw.is_null() {
        js_object_alloc_class_inline_keys_stamped(0, 0, count as u32, keys, shape_id)
    } else {
        let obj = raw.cast::<ObjectHeader>();
        (*obj).class_id = 0;
        (*obj).parent_class_id = 0;
        // GC_STORE_AUDIT(INIT): fresh record has no metadata edge.
        (*obj).meta = ptr::null_mut();
        // Keep shape publication in the existing mint-and-stamp funnel.
        if !shapes::try_birth_stamp_preinstalled_shape(obj, shape_id, keys, count as u32) {
            let id = shapes::shape_id_for_keys_ensure(keys, count as u32);
            set_object_keys_array_with_live(obj, keys, count as u32);
            shapes::birth_stamp_object_shape(obj, id, count as u32);
        }
        crate::gc::layout_init_pointer_free(raw);
        obj
    };
    mark_object_plain_ordinary(obj);
    let slots = obj
        .cast::<u8>()
        .add(std::mem::size_of::<ObjectHeader>())
        .cast::<JSValue>();
    let mut saw_pointer = false;
    for (index, &value) in values.iter().enumerate() {
        if batch.is_none() {
            // Allocate-black births retain their normal remembering and
            // shading. Suppression alone cannot elide those.
            saw_pointer |= store_object_field_slot_layout_deferred(obj, index, value.bits());
        } else {
            // GC_STORE_AUDIT(INIT): unpublished record, no marking or callbacks;
            // final layout and old-to-young pages are published below.
            slots.add(index).write(value);
            let tag = value.bits() & crate::value::TAG_MASK;
            saw_pointer |= tag == crate::value::POINTER_TAG || tag == crate::value::STRING_TAG;
        }
    }
    // Initialize only physical slack, not fields that we immediately replace.
    for index in count..capacity {
        // GC_STORE_AUDIT(INIT): physical slack is not a live shape field.
        slots.add(index).write(JSValue::undefined());
    }
    crate::gc::layout_finish_deferred_boxed_object(obj as usize, saw_pointer);
    if saw_pointer {
        if let Some(batch) = batch {
            batch.finish_json_slots(obj.cast(), slots.cast(), count);
        }
    }
    obj
}
