//! Callback-free emission of inline records with primitive array fields.
//!
//! Validate before output, then keep field snapshots on the stack. The emit
//! interval only grows a Rust output buffer: no managed allocation, user code,
//! collection or temporary managed roots. Other records use the rooted walker.

use super::*;

const MAX_FIELDS: usize = 32;

#[inline]
unsafe fn primitive(bits: u64) -> bool {
    bits != crate::value::TAG_HOLE
        && bits & crate::value::TAG_MASK != POINTER_TAG
        && bits & crate::value::TAG_MASK != BIGINT_TAG
        && !is_raw_pointer(bits)
}

unsafe fn array_pointer(bits: u64) -> Option<*const crate::ArrayHeader> {
    let ptr = if bits & crate::value::TAG_MASK == POINTER_TAG {
        (bits & POINTER_MASK) as *const crate::ArrayHeader
    } else if is_raw_pointer(bits) {
        bits as *const crate::ArrayHeader
    } else {
        return None;
    };
    let header = crate::value::addr_class::try_read_tracked_gc_header(ptr as usize)?;
    let header = header.as_ref();
    if header.obj_type != crate::gc::GC_TYPE_ARRAY
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || (header.size as usize)
            < crate::gc::GC_HEADER_SIZE + std::mem::size_of::<crate::ArrayHeader>()
    {
        return None;
    }
    // The ordinary array walker resolves forwarding. A live, already-resolved
    // dense head lets this path avoid both that resolver and any lazy forcing.
    Some(ptr)
}

unsafe fn inline_fields<'a>(obj: *const crate::ObjectHeader, count: u32) -> Option<&'a [u64]> {
    inline_fields_with_live(obj, count, crate::object::object_live_slot_count(obj))
}

unsafe fn inline_fields_with_live<'a>(
    obj: *const crate::ObjectHeader,
    count: u32,
    live_inline_slots: u32,
) -> Option<&'a [u64]> {
    let count = count as usize;
    // Both callers receive `obj` from the shape-template path after it has
    // validated the tracked object type. Re-reading the global address maps
    // for every record duplicates that proof in the hottest loop.
    let header = &*crate::gc::header_from_trusted_user_ptr(obj.cast());
    if count == 0
        || count > MAX_FIELDS
        || header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header._reserved & crate::gc::OBJ_FLAG_HAS_DESCRIPTORS != 0
        || (header.size as usize)
            < crate::gc::GC_HEADER_SIZE + std::mem::size_of::<crate::ObjectHeader>() + count * 8
        || (*obj).class_id != 0
        || count > live_inline_slots as usize
    {
        return None;
    }
    Some(std::slice::from_raw_parts(
        (obj as *const u8)
            .add(std::mem::size_of::<crate::ObjectHeader>())
            .cast(),
        count,
    ))
}

/// A shape hint, not a promise about later elements. Avoid speculative work
/// on heterogeneous records whose pointer fields are nested objects.
pub(super) unsafe fn template_candidate(obj: *const crate::ObjectHeader, count: u32) -> bool {
    let Some(fields) = inline_fields(obj, count) else {
        return false;
    };
    let mut has_array = false;
    for &bits in fields {
        if bits == TAG_UNDEFINED {
            return false;
        }
        if !primitive(bits) {
            if array_pointer(bits).is_none() {
                return false;
            }
            has_array = true;
        }
    }
    has_array
}

/// Prove the existing primitive-array emitter's preconditions without output.
/// Reject named properties (including toJSON), descriptors, holes, sparse and
/// lazy layouts, and any element that could recurse or invoke BigInt.toJSON.
unsafe fn primitive_array(arr: *const crate::ArrayHeader) -> bool {
    // `array_pointer` has already validated this exact tracked header.
    let header = &*crate::gc::header_from_trusted_user_ptr(arr.cast());
    if header.obj_type != crate::gc::GC_TYPE_ARRAY
        || (header.size as usize)
            < crate::gc::GC_HEADER_SIZE + std::mem::size_of::<crate::ArrayHeader>()
        || (*arr).length > (*arr).capacity
        || header._reserved & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0
        || (header.size as usize)
            < crate::gc::GC_HEADER_SIZE
                + std::mem::size_of::<crate::ArrayHeader>()
                + (*arr).length as usize * 8
    {
        return false;
    }
    let elements = std::slice::from_raw_parts(
        crate::array::array_elements_ptr(arr as *const crate::ArrayHeader).cast::<u64>(),
        (*arr).length as usize,
    );
    // Even a raw-f64 array is checked for tag collisions before borrowing it
    // across the callback-free interval. Ordinary numeric arrays pass.
    elements.iter().all(|&bits| primitive(bits))
}

#[cfg(test)]
unsafe fn try_emit(
    obj: *const crate::ObjectHeader,
    template: &ShapeTemplate,
    buf: &mut String,
    depth: u32,
) -> bool {
    try_emit_with_live(
        obj,
        template,
        buf,
        depth,
        crate::object::object_live_slot_count(obj),
        super::stringify_tojson_probe::data_record_global_to_json_absent_without_gc(),
    )
}

pub(super) unsafe fn try_emit_with_live(
    obj: *const crate::ObjectHeader,
    template: &ShapeTemplate,
    buf: &mut String,
    depth: u32,
    live_inline_slots: u32,
    global_to_json_absent: bool,
) -> bool {
    if depth as usize >= super::stringify::MAX_STRINGIFY_NESTING_DEPTH || !global_to_json_absent {
        return false;
    }
    let Some(fields) = inline_fields_with_live(obj, template.shape_fields, live_inline_slots)
    else {
        return false;
    };
    if !template.own_keys_exclude_to_json
        || crate::object::prototype_chain::object_static_prototype(obj as usize).is_some()
    {
        return false;
    }
    // Only the `fields.len()` prefix is read below. Keeping each slot wrapped
    // in `MaybeUninit` avoids clearing both full 32-entry arrays for every
    // short record while preserving the stable snapshots used during output.
    let mut values: [std::mem::MaybeUninit<u64>; MAX_FIELDS] =
        [const { std::mem::MaybeUninit::uninit() }; MAX_FIELDS];
    let mut arrays: [std::mem::MaybeUninit<*const crate::ArrayHeader>; MAX_FIELDS] =
        [const { std::mem::MaybeUninit::uninit() }; MAX_FIELDS];
    for (i, &bits) in fields.iter().enumerate() {
        if bits == TAG_UNDEFINED {
            return false;
        }
        values[i].write(bits);
        let arr = if !primitive(bits) {
            let Some(arr) = array_pointer(bits) else {
                return false;
            };
            if !primitive_array(arr) {
                return false;
            }
            arr
        } else {
            std::ptr::null()
        };
        arrays[i].write(arr);
    }
    for i in 0..fields.len() {
        super::stringify_copy::push_str(buf, &template.prefixes[i]);
        let arr = arrays[i].assume_init();
        if !arr.is_null() {
            // No callbacks or managed allocation can invalidate validation.
            super::stringify_primitive_array::emit_validated(arr, buf);
            continue;
        }
        let bits = values[i].assume_init();
        match bits {
            TAG_NULL => buf.push_str("null"),
            TAG_TRUE => buf.push_str("true"),
            TAG_FALSE => buf.push_str("false"),
            _ => match bits & crate::value::TAG_MASK {
                STRING_TAG => {
                    let ptr = (bits & POINTER_MASK) as *const StringHeader;
                    if !write_heap_string(buf, ptr) {
                        buf.push_str("null");
                    }
                }
                crate::value::SHORT_STRING_TAG => {
                    let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
                    let len = JSValue::from_bits(bits).short_string_to_buf(&mut scratch);
                    if let Ok(text) = std::str::from_utf8(&scratch[..len]) {
                        write_escaped_string(buf, text);
                    } else {
                        buf.push_str("null");
                    }
                }
                _ => write_number(buf, f64::from_bits(bits)),
            },
        }
    }
    buf.push('}');
    true
}

#[cfg(test)]
#[path = "stringify_data_record_tests.rs"]
mod tests;
