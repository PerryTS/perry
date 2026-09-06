//! Exact-size output for small plain objects with primitive fields.
//! The plan holds only lengths and inline bytes; the object is the sole GC
//! root. Keys and values are re-read after allocating the final output.

use super::*;
use crate::string::{init_string_header, string_storage_alloc};

const MAX_FIELDS: usize = 4;

#[derive(Clone, Copy)]
enum Piece {
    String { bytes: u32, units: u32 },
    Inline { bytes: [u8; 32], len: u32 },
}

impl Piece {
    fn inline(text: &[u8]) -> Self {
        let mut bytes = [0; 32];
        bytes[..text.len()].copy_from_slice(text);
        Self::Inline {
            bytes,
            len: text.len() as u32,
        }
    }

    fn lengths(self) -> (u32, u32) {
        match self {
            Self::String { bytes, units } => (bytes + 2, units + 2),
            Self::Inline { len, .. } => (len, len),
        }
    }
}

#[inline]
unsafe fn slot(base: *const u8, header_size: usize, i: usize) -> u64 {
    base.add(header_size).cast::<u64>().add(i).read()
}

unsafe fn string_piece(bits: u64) -> Option<Piece> {
    let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
    let (ptr, len) = crate::string::str_bytes_from_jsvalue(f64::from_bits(bits), &mut scratch)?;
    if ptr.is_null() || len > u32::MAX - 2 {
        return None;
    }
    let bytes = std::slice::from_raw_parts(ptr, len as usize);
    if super::simd::find_string_escape(bytes).is_some()
        || super::stringify_string::has_incomplete_tail(bytes)
    {
        return None;
    }
    let units = if bits & crate::value::TAG_MASK == STRING_TAG {
        (*((bits & POINTER_MASK) as *const StringHeader)).utf16_len
    } else {
        // The inline representation's length is a byte count. Non-ASCII
        // synthetic SSO values use the general path's decoding semantics.
        if !bytes.is_ascii() {
            return None;
        }
        len
    };
    units.checked_add(2)?;
    Some(Piece::String { bytes: len, units })
}

unsafe fn scalar_piece(bits: u64) -> Option<Piece> {
    match bits {
        TAG_NULL => return Some(Piece::inline(b"null")),
        TAG_TRUE => return Some(Piece::inline(b"true")),
        TAG_FALSE => return Some(Piece::inline(b"false")),
        TAG_UNDEFINED | crate::value::TAG_HOLE => return None,
        _ => {}
    }
    match bits & crate::value::TAG_MASK {
        STRING_TAG | crate::value::SHORT_STRING_TAG => return string_piece(bits),
        POINTER_TAG | BIGINT_TAG => return None,
        INT32_TAG => {
            let mut digits = itoa::Buffer::new();
            return Some(Piece::inline(
                digits.format((bits & INT32_MASK) as u32 as i32).as_bytes(),
            ));
        }
        _ => {}
    }
    if is_raw_pointer(bits) {
        return None;
    }
    let number = f64::from_bits(bits);
    if !number.is_finite() {
        return Some(Piece::inline(b"null"));
    }
    let mut digits = ryu_js::Buffer::new();
    Some(Piece::inline(digits.format_finite(number).as_bytes()))
}

unsafe fn emit_piece(piece: Piece, bits: u64, output: *mut u8) -> usize {
    match piece {
        Piece::Inline { bytes, len } => {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), output, len as usize);
            len as usize
        }
        Piece::String { bytes, .. } => {
            let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
            let (source, _) =
                crate::string::str_bytes_from_jsvalue(f64::from_bits(bits), &mut scratch)
                    .expect("prevalidated string slot");
            output.write(b'"');
            std::ptr::copy_nonoverlapping(source, output.add(1), bytes as usize);
            output.add(bytes as usize + 1).write(b'"');
            bytes as usize + 2
        }
    }
}

/// The full entry restricts replacer and spacer to inert arguments before
/// calling this helper. Any semantic uncertainty declines before output.
#[inline]
pub(super) unsafe fn try_object(bits: u64) -> Option<JSValue> {
    if bits & crate::value::TAG_MASK != POINTER_TAG {
        return None;
    }
    let obj = (bits & POINTER_MASK) as *const crate::ObjectHeader;
    let header = crate::value::addr_class::try_read_tracked_gc_header(obj as usize)?;
    let header = header.as_ref();
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header._reserved & crate::gc::OBJ_FLAG_HAS_DESCRIPTORS != 0
        || (header.size as usize)
            < crate::gc::GC_HEADER_SIZE + std::mem::size_of::<crate::ObjectHeader>()
        || (*obj).class_id != 0
    {
        return None;
    }
    let keys = crate::object::object_keys_array(obj);
    let fields = if keys.is_null() {
        0
    } else {
        (*keys).length as usize
    };
    if fields > MAX_FIELDS {
        return None;
    }
    if !keys.is_null() && fields > (*keys).capacity as usize {
        return None;
    }
    if (header.size as usize)
        < crate::gc::GC_HEADER_SIZE + std::mem::size_of::<crate::ObjectHeader>() + fields * 8
    {
        return None;
    }
    if fields
        > crate::object::object_live_slot_count(obj).max(crate::object::INLINE_SLOT_FLOOR as u32)
            as usize
    {
        return None;
    }
    if !keys.is_null() && crate::object::keys_contain_array_index(keys) {
        return None;
    }
    emit_object(obj, fields)
}

#[inline(never)]
unsafe fn emit_object(obj: *const crate::ObjectHeader, fields: usize) -> Option<JSValue> {
    let empty = Piece::inline(b"");
    let mut key_plan = [empty; MAX_FIELDS];
    let mut value_plan = [empty; MAX_FIELDS];
    let keys = crate::object::object_keys_array(obj);
    let mut bytes = 2u32;
    let mut units = 2u32;
    for i in 0..fields {
        key_plan[i] = string_piece(slot(
            keys.cast(),
            std::mem::size_of::<crate::ArrayHeader>(),
            i,
        ))?;
        value_plan[i] = scalar_piece(slot(
            obj.cast(),
            std::mem::size_of::<crate::ObjectHeader>(),
            i,
        ))?;
        let (kb, ku) = key_plan[i].lengths();
        let (vb, vu) = value_plan[i].lengths();
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
    // This early path can run outside a serializer frame. Refresh the
    // prototype verdict rather than inheriting a previous call's cache.
    super::invalidate_object_proto_tojson_state();
    if !super::stringify_tojson_probe::to_json_definitely_absent(obj.cast()) {
        return None;
    }
    if fields == 0 {
        return Some(JSValue::short_string_unchecked(b"{}"));
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let input = scope.root_raw_const_ptr(obj);
    let (result, output) = string_storage_alloc(bytes);
    let obj = input.get_raw_const_ptr::<crate::ObjectHeader>();
    let keys = crate::object::object_keys_array(obj);
    init_string_header(result, units, bytes, bytes, 0, 0);
    output.write(b'{');
    let mut at = 1;
    for i in 0..fields {
        if i != 0 {
            output.add(at).write(b',');
            at += 1;
        }
        at += emit_piece(
            key_plan[i],
            slot(keys.cast(), std::mem::size_of::<crate::ArrayHeader>(), i),
            output.add(at),
        );
        output.add(at).write(b':');
        at += 1;
        at += emit_piece(
            value_plan[i],
            slot(obj.cast(), std::mem::size_of::<crate::ObjectHeader>(), i),
            output.add(at),
        );
    }
    output.add(at).write(b'}');
    debug_assert_eq!(at + 1, bytes as usize);
    Some(JSValue::string_ptr(result))
}

#[cfg(test)]
#[path = "stringify_flat_tests.rs"]
mod tests;
