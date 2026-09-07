//! Completed plain-record construction in the JSON suppression window.
use super::*;

pub(crate) unsafe fn object_from_json_fields(
    batch: &mut Option<crate::arena::ConstructionBatch>,
    keys: *mut ArrayHeader,
    values: &[JSValue],
) -> *mut ObjectHeader {
    let count = values.len();
    let capacity = count.max(INLINE_SLOT_FLOOR);
    let size = std::mem::size_of::<ObjectHeader>() + capacity * 8;
    let raw = batch.as_mut().map_or(ptr::null_mut(), |b| {
        b.try_alloc(size, crate::gc::GC_TYPE_OBJECT)
    });
    let obj = if raw.is_null() {
        js_object_alloc_class_inline_keys(0, 0, count as u32, keys)
    } else {
        let obj = raw.cast::<ObjectHeader>();
        (*obj).class_id = 0;
        (*obj).parent_class_id = 0;
        // GC_STORE_AUDIT(INIT): fresh record has no metadata edge.
        (*obj).meta = ptr::null_mut();
        // Keep shape publication in the existing mint-and-stamp funnel.
        let id = shapes::shape_id_for_keys_ensure(keys, count as u32);
        if !shapes::try_birth_stamp_preinstalled_shape(obj, id, keys, count as u32) {
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
