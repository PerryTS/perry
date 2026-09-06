//! Exact-size final output for bounded plain records and primitive-array fields.
//! The plan contains only byte counts, array indices and inline scalar text.
//! Only the parent object is rooted; child pointers are rederived after allocation.

use super::stringify_flat::{emit_piece, scalar_piece, slot, string_piece, Piece};
use super::*;
use crate::string::{init_string_header, string_storage_alloc};

const MAX_FIELDS: usize = 8;
const MAX_ELEMENTS: usize = 16;
const OBJECT_BYTES: usize = std::mem::size_of::<crate::ObjectHeader>();
const ARRAY_BYTES: usize = std::mem::size_of::<crate::ArrayHeader>();

#[derive(Clone, Copy)]
enum Field {
    Scalar(Piece),
    Array { start: usize, len: usize },
}

/// Decline before entering the planning frame on large/exotic receivers.
#[inline]
pub(super) unsafe fn try_object(bits: u64) -> Option<JSValue> {
    if bits & crate::value::TAG_MASK != POINTER_TAG {
        return None;
    }
    let obj = (bits & POINTER_MASK) as *const crate::ObjectHeader;
    let header = crate::value::addr_class::try_read_tracked_gc_header(obj as usize)?.as_ref();
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved & crate::gc::OBJ_FLAG_HAS_DESCRIPTORS != 0
        || (header.size as usize) < crate::gc::GC_HEADER_SIZE + OBJECT_BYTES
        || (*obj).class_id != 0
    {
        return None;
    }
    let keys = crate::object::object_keys_array(obj);
    if keys.is_null() {
        return None;
    }
    let fields = (*keys).length as usize;
    if fields == 0
        || fields > MAX_FIELDS
        || fields > (*keys).capacity as usize
        || (header.size as usize) < crate::gc::GC_HEADER_SIZE + OBJECT_BYTES + fields * 8
        || fields
            > crate::object::object_live_slot_count(obj)
                .max(crate::object::INLINE_SLOT_FLOOR as u32) as usize
        || crate::object::keys_contain_array_index(keys)
    {
        return None;
    }
    emit_record(obj, fields)
}

unsafe fn dense_array(bits: u64) -> Option<(*const crate::ArrayHeader, usize)> {
    if bits & crate::value::TAG_MASK != POINTER_TAG {
        return None;
    }
    let arr = (bits & POINTER_MASK) as *const crate::ArrayHeader;
    let header = crate::value::addr_class::try_read_tracked_gc_header(arr as usize)?.as_ref();
    if header.obj_type != crate::gc::GC_TYPE_ARRAY
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0
        || (header.size as usize) < crate::gc::GC_HEADER_SIZE + ARRAY_BYTES
    {
        return None;
    }
    let len = (*arr).length as usize;
    if len > MAX_ELEMENTS
        || len > (*arr).capacity as usize
        || (header.size as usize) < crate::gc::GC_HEADER_SIZE + ARRAY_BYTES + len * 8
        || crate::array::array_has_named_properties_resolved(arr)
        || crate::object::prototype_chain::object_static_prototype(arr as usize).is_some()
    {
        return None;
    }
    Some((arr, len))
}

#[inline(never)]
unsafe fn emit_record(obj: *const crate::ObjectHeader, fields: usize) -> Option<JSValue> {
    // Every live entry is overwritten before emission. A zero-valued string
    // placeholder lets the compiler clear native plan storage in bulk.
    let empty = Piece::String { bytes: 0, units: 0 };
    let mut key_plan = [empty; MAX_FIELDS];
    let mut value_plan = [Field::Scalar(empty); MAX_FIELDS];
    let mut elements = [empty; MAX_ELEMENTS];
    let mut used = 0;
    let keys = crate::object::object_keys_array(obj);
    let (mut bytes, mut units) = (2u32, 2u32);
    for i in 0..fields {
        let key = string_piece(slot(keys.cast(), ARRAY_BYTES, i))?;
        key_plan[i] = key;
        let (kb, ku) = key.lengths();
        let bits = slot(obj.cast(), OBJECT_BYTES, i);
        let (vb, vu) = if let Some(value) = scalar_piece(bits) {
            value_plan[i] = Field::Scalar(value);
            value.lengths()
        } else {
            let (arr, len) = dense_array(bits)?;
            if used + len > MAX_ELEMENTS {
                return None;
            }
            value_plan[i] = Field::Array { start: used, len };
            let (mut ab, mut au) = (2u32, 2u32);
            for j in 0..len {
                let value = scalar_piece(slot(arr.cast(), ARRAY_BYTES, j))?;
                elements[used + j] = value;
                let (eb, eu) = value.lengths();
                let comma = u32::from(j != 0);
                ab = ab.checked_add(eb)?.checked_add(comma)?;
                au = au.checked_add(eu)?.checked_add(comma)?;
            }
            used += len;
            (ab, au)
        };
        let punctuation = 1 + u32::from(i != 0);
        bytes = bytes
            .checked_add(kb)?
            .checked_add(vb)?
            .checked_add(punctuation)?;
        units = units
            .checked_add(ku)?
            .checked_add(vu)?
            .checked_add(punctuation)?;
    }
    // Root the complete input graph before either prototype initialization or
    // final output allocation. Nothing in the stack plan is a heap pointer.
    let scope = crate::gc::RuntimeHandleScope::new();
    let input = scope.root_raw_const_ptr(obj);
    super::invalidate_object_proto_tojson_state();
    if !super::stringify_tojson_probe::to_json_definitely_absent(obj.cast()) {
        return None;
    }
    let (result, output) = string_storage_alloc(bytes);
    init_string_header(result, units, bytes, bytes, 0, 0);
    let obj = input.get_raw_const_ptr::<crate::ObjectHeader>();
    let keys = crate::object::object_keys_array(obj);
    output.write(b'{');
    let mut at = 1usize;
    for i in 0..fields {
        if i != 0 {
            output.add(at).write(b',');
            at += 1;
        }
        at += emit_piece(
            key_plan[i],
            slot(keys.cast(), ARRAY_BYTES, i),
            output.add(at),
        );
        output.add(at).write(b':');
        at += 1;
        let bits = slot(obj.cast(), OBJECT_BYTES, i);
        match value_plan[i] {
            Field::Scalar(value) => at += emit_piece(value, bits, output.add(at)),
            Field::Array { start, len } => {
                let arr = (bits & POINTER_MASK) as *const crate::ArrayHeader;
                output.add(at).write(b'[');
                at += 1;
                for j in 0..len {
                    if j != 0 {
                        output.add(at).write(b',');
                        at += 1;
                    }
                    at += emit_piece(
                        elements[start + j],
                        slot(arr.cast(), ARRAY_BYTES, j),
                        output.add(at),
                    );
                }
                output.add(at).write(b']');
                at += 1;
            }
        }
    }
    output.add(at).write(b'}');
    debug_assert_eq!(at + 1, bytes as usize);
    Some(JSValue::string_ptr(result))
}

#[cfg(test)]
#[path = "stringify_record_output_tests.rs"]
mod tests;
