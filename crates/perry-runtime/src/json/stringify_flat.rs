//! Exact-size output for small plain objects with primitive fields.
//! The plan holds only lengths and inline bytes; the object is the sole GC
//! root. Keys and values are re-read after allocating the final output.

use super::*;
use crate::string::{
    init_string_header, json_output_storage_alloc, string_storage_alloc,
    JSON_MALLOC_OUTPUT_THRESHOLD, STRING_FLAG_JSON_ESCAPE_FREE,
};

const MAX_FIELDS: usize = 4;
const JSON_OUTPUT_SWEEP_BUDGET: usize = 32 * 1024 * 1024;

crate::perry_thread_local! {
    /// Bytes of malloc-backed exact output completed since the last boundary
    /// sweep. This is scheduling debt only and never owns a managed pointer.
    static JSON_OUTPUT_BYTES_SINCE_SWEEP: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[inline]
fn service_json_output_sweep_boundary() {
    JSON_OUTPUT_BYTES_SINCE_SWEEP.with(|bytes| {
        if bytes.get() >= JSON_OUTPUT_SWEEP_BUDGET {
            // The caller has rooted its input and has not allocated output.
            // A collection here can reclaim prior results without observing a
            // partially initialized string.
            crate::gc::gc_check_trigger();
            bytes.set(0);
        }
    });
}

#[inline]
fn note_completed_malloc_json_output(bytes: u32) {
    JSON_OUTPUT_BYTES_SINCE_SWEEP.with(|debt| {
        let total = debt.get().saturating_add(bytes as usize);
        debt.set(total);
        if total >= JSON_OUTPUT_SWEEP_BUDGET {
            crate::gc::gc_schedule_malloc_sweep_after_json_output();
        }
    });
}

#[derive(Clone, Copy)]
pub(super) enum Piece {
    String { bytes: u32, units: u32 },
    Escaped(super::stringify_escaped_output::Plan),
    Inline { bytes: [u8; 32], len: u32 },
}

impl Piece {
    pub(super) fn inline(text: &[u8]) -> Self {
        let mut bytes = [0; 32];
        bytes[..text.len()].copy_from_slice(text);
        Self::Inline {
            bytes,
            len: text.len() as u32,
        }
    }

    pub(super) fn lengths(self) -> (u32, u32) {
        match self {
            Self::String { bytes, units } => (bytes + 2, units + 2),
            Self::Escaped(plan) => (plan.bytes, plan.units),
            Self::Inline { len, .. } => (len, len),
        }
    }
}

#[inline]
pub(super) unsafe fn slot(base: *const u8, header_size: usize, i: usize) -> u64 {
    base.add(header_size).cast::<u64>().add(i).read()
}

pub(super) unsafe fn string_piece(bits: u64) -> Option<Piece> {
    string_piece_for::<false>(bits)
}

/// Plan a key and prove that insertion order and the own-key toJSON miss
/// are safe in the same pass. No callback or allocation may separate this
/// proof from the remaining prototype probe.
pub(super) unsafe fn key_piece(bits: u64) -> Option<Piece> {
    string_piece_for::<true>(bits)
}

unsafe fn string_piece_for<const KEY: bool>(bits: u64) -> Option<Piece> {
    let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
    let (ptr, len) = crate::string::str_bytes_from_jsvalue(f64::from_bits(bits), &mut scratch)?;
    if ptr.is_null() || len > u32::MAX - 2 {
        return None;
    }
    let bytes = std::slice::from_raw_parts(ptr, len as usize);
    if KEY {
        if bytes.first().is_some_and(u8::is_ascii_digit)
            && std::str::from_utf8(bytes)
                .ok()
                .and_then(crate::object::canonical_array_index)
                .is_some()
        {
            return None;
        }
        if super::stringify_tojson_probe::key_bytes_may_carry_to_json(bytes) {
            return None;
        }
    }
    let escaped = super::simd::short_string_needs_escape(bytes);
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
    if escaped {
        return super::stringify_escaped_output::Plan::new(bytes, units).map(Piece::Escaped);
    }
    if super::stringify_string::has_incomplete_tail(bytes) {
        return None;
    }
    units.checked_add(2)?;
    Some(Piece::String { bytes: len, units })
}

pub(super) unsafe fn scalar_piece(bits: u64) -> Option<Piece> {
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
    if number.abs() < crate::builtins::INT_EXACT_FASTPATH_LIMIT {
        let integer = number as i64;
        if integer as f64 == number {
            // Below 2^53 an exact integer has the ECMAScript decimal spelling.
            // Converting either signed zero to i64 also emits JSON's "0".
            let mut digits = itoa::Buffer::new();
            return Some(Piece::inline(digits.format(integer).as_bytes()));
        }
    }
    let mut digits = ryu_js::Buffer::new();
    Some(Piece::inline(digits.format_finite(number).as_bytes()))
}

pub(super) unsafe fn emit_piece(piece: Piece, bits: u64, output: *mut u8) -> usize {
    match piece {
        Piece::Escaped(plan) => {
            let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
            let (source, _) =
                crate::string::str_bytes_from_jsvalue(f64::from_bits(bits), &mut scratch)
                    .expect("prevalidated escaped string slot");
            plan.write(source, output)
        }
        Piece::Inline { bytes, len } => {
            super::stringify_copy::copy_short(bytes.as_ptr(), output, len as usize);
            len as usize
        }
        Piece::String { bytes, .. } => {
            let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
            let (source, _) =
                crate::string::str_bytes_from_jsvalue(f64::from_bits(bits), &mut scratch)
                    .expect("prevalidated string slot");
            // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
            output.write(b'"');
            super::stringify_copy::copy_bytes(source, output.add(1), bytes as usize);
            // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
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
    if fields != 0 && !bounded_keys_are_dense(keys, fields) {
        return None;
    }
    if fields == 0 {
        // The inline result needs neither an output allocation nor a stack
        // plan. Decline the allocating first lookup so the rooted general
        // serializer initializes the signature cache.
        return super::stringify_tojson_probe::to_json_definitely_absent_without_gc(obj.cast())
            .then(|| JSValue::short_string_unchecked(b"{}"));
    }
    if fields == 1 {
        return emit_one_field_object(obj);
    }
    if fields == 2 {
        for i in 0..2 {
            let value_bits = slot(obj.cast(), std::mem::size_of::<crate::ObjectHeader>(), i);
            if let Some(value) = parsed_plain_string_piece(value_bits) {
                if let Some(result) = emit_two_field_parsed_string_object(obj, i, value) {
                    return Some(result);
                }
                break;
            }
        }
    }
    emit_object(obj, fields)
}

/// Exact output for the smallest non-empty object. Keeping this straight-line
/// avoids constructing and walking the generic four-entry plan for `{a: 1}`
/// style payloads while retaining the same own-key and prototype checks.
#[inline]
unsafe fn emit_one_field_object(obj: *const crate::ObjectHeader) -> Option<JSValue> {
    let keys = crate::object::object_keys_array(obj);
    let key_bits = slot(keys.cast(), std::mem::size_of::<crate::ArrayHeader>(), 0);
    let value_bits = slot(obj.cast(), std::mem::size_of::<crate::ObjectHeader>(), 0);
    let key = key_piece(key_bits)?;
    let value = scalar_piece(value_bits)?;
    let (key_bytes, key_units) = key.lengths();
    let (value_bytes, value_units) = value.lengths();
    let bytes = 3u32.checked_add(key_bytes)?.checked_add(value_bytes)?;
    let units = 3u32.checked_add(key_units)?.checked_add(value_units)?;

    let scope = crate::gc::RuntimeHandleScope::new();
    let input = scope.root_raw_const_ptr(obj);
    if !super::stringify_tojson_probe::to_json_definitely_absent_after_own_keys(obj.cast()) {
        return None;
    }
    let (result, output) = string_storage_alloc(bytes);
    input.with_const_ptr(|obj: *const crate::ObjectHeader| {
        let keys = crate::object::object_keys_array(obj);
        init_string_header(result, units, bytes, bytes, 0, 0);
        // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
        output.write(b'{');
        let at = 1 + emit_piece(
            key,
            slot(keys.cast(), std::mem::size_of::<crate::ArrayHeader>(), 0),
            output.add(1),
        );
        // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
        output.add(at).write(b':');
        let at = at
            + 1
            + emit_piece(
                value,
                slot(obj.cast(), std::mem::size_of::<crate::ObjectHeader>(), 0),
                output.add(at + 1),
            );
        // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
        output.add(at).write(b'}');
        debug_assert_eq!(at + 1, bytes as usize);
        Some(JSValue::string_ptr(result))
    })
}

/// The direct parser marks strings borrowed from unescaped JSON tokens. That
/// proof is stronger than rescanning the payload here and remains valid until
/// a string-producing mutation creates a new header without the flag.
#[inline]
unsafe fn parsed_plain_string_piece(bits: u64) -> Option<Piece> {
    if bits & crate::value::TAG_MASK != STRING_TAG {
        return None;
    }
    let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
    let (_, len) = crate::string::str_bytes_from_jsvalue(f64::from_bits(bits), &mut scratch)?;
    if len < 64 || len > u32::MAX - 2 {
        return None;
    }
    let header = (bits & POINTER_MASK) as *const StringHeader;
    if (*header).flags & STRING_FLAG_JSON_ESCAPE_FREE == 0 {
        return None;
    }
    (*header).utf16_len.checked_add(2)?;
    Some(Piece::String {
        bytes: len,
        units: (*header).utf16_len,
    })
}

/// Exact output for the common `{ id, text }`-shaped large JSON object. The
/// caller supplies the one value whose parser provenance avoids a payload
/// scan; the other scalar and both keys keep their ordinary semantic checks.
#[inline(never)]
unsafe fn emit_two_field_parsed_string_object(
    obj: *const crate::ObjectHeader,
    proven_index: usize,
    proven_value: Piece,
) -> Option<JSValue> {
    let keys = crate::object::object_keys_array(obj);
    let empty = Piece::String { bytes: 0, units: 0 };
    let mut key_plan = [empty; 2];
    let mut value_plan = [empty; 2];
    let mut bytes = 2u32;
    let mut units = 2u32;
    for i in 0..2 {
        key_plan[i] = key_piece(slot(
            keys.cast(),
            std::mem::size_of::<crate::ArrayHeader>(),
            i,
        ))?;
        value_plan[i] = if i == proven_index {
            proven_value
        } else {
            scalar_piece(slot(
                obj.cast(),
                std::mem::size_of::<crate::ObjectHeader>(),
                i,
            ))?
        };
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

    let scope = crate::gc::RuntimeHandleScope::new();
    let input = scope.root_raw_const_ptr(obj);
    service_json_output_sweep_boundary();
    if !input.with_const_ptr(|obj: *const crate::ObjectHeader| {
        super::stringify_tojson_probe::to_json_definitely_absent_after_own_keys(obj.cast())
    }) {
        return None;
    }

    let large_output = bytes >= JSON_MALLOC_OUTPUT_THRESHOLD;
    let construction = large_output.then(crate::gc::GcSuppressScope::new);
    let (result, output) = json_output_storage_alloc(bytes);
    let value = input.with_const_ptr(|obj: *const crate::ObjectHeader| {
        let keys = crate::object::object_keys_array(obj);
        init_string_header(result, units, bytes, bytes, 0, 0);
        // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
        output.write(b'{');
        let mut at = 1usize;
        for i in 0..2 {
            if i != 0 {
                // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
                output.add(at).write(b',');
                at += 1;
            }
            at += emit_piece(
                key_plan[i],
                slot(keys.cast(), std::mem::size_of::<crate::ArrayHeader>(), i),
                output.add(at),
            );
            // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
            output.add(at).write(b':');
            at += 1;
            at += emit_piece(
                value_plan[i],
                slot(obj.cast(), std::mem::size_of::<crate::ObjectHeader>(), i),
                output.add(at),
            );
        }
        // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
        output.add(at).write(b'}');
        debug_assert_eq!(at + 1, bytes as usize);
        Some(JSValue::string_ptr(result))
    });
    drop(construction);
    if large_output {
        note_completed_malloc_json_output(bytes);
    }
    value
}

/// The fused key loop replaces the own-key probe's array validation as well
/// as its scan. Its callers have already bounded the number of live slots.
pub(super) unsafe fn bounded_keys_are_dense(
    keys: *const crate::ArrayHeader,
    fields: usize,
) -> bool {
    if keys as usize & 7 != 0 {
        return false;
    }
    let Some(header) = crate::value::addr_class::try_read_gc_header(keys as usize) else {
        return false;
    };
    header.obj_type == crate::gc::GC_TYPE_ARRAY
        && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
        && header.size as usize
            >= crate::gc::GC_HEADER_SIZE + std::mem::size_of::<crate::ArrayHeader>() + fields * 8
}

#[inline(never)]
unsafe fn emit_object(obj: *const crate::ObjectHeader, fields: usize) -> Option<JSValue> {
    // Every live entry is overwritten before emission. This initialized
    // placeholder has a smaller active payload than an inline-text piece.
    let empty = Piece::String { bytes: 0, units: 0 };
    let mut key_plan = [empty; MAX_FIELDS];
    let mut value_plan = [empty; MAX_FIELDS];
    let keys = crate::object::object_keys_array(obj);
    let mut bytes = 2u32;
    let mut units = 2u32;
    for i in 0..fields {
        key_plan[i] = key_piece(slot(
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
    // The first prototype lookup initializes globalThis and allocates its
    // lookup key. Root before that lookup as well as before output allocation;
    // the stack plan contains lengths and inline bytes, never heap pointers.
    let scope = crate::gc::RuntimeHandleScope::new();
    let input = scope.root_raw_const_ptr(obj);
    // This early path can run outside a serializer frame. The prototype probe
    // validates its live signature before reusing a prior call's verdict.
    if !super::stringify_tojson_probe::to_json_definitely_absent_after_own_keys(obj.cast()) {
        return None;
    }
    let (result, output) = string_storage_alloc(bytes);
    input.with_const_ptr(|obj: *const crate::ObjectHeader| {
        let keys = crate::object::object_keys_array(obj);
        init_string_header(result, units, bytes, bytes, 0, 0);
        // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
        output.write(b'{');
        let mut at = 1;
        for i in 0..fields {
            if i != 0 {
                // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
                output.add(at).write(b',');
                at += 1;
            }
            at += emit_piece(
                key_plan[i],
                slot(keys.cast(), std::mem::size_of::<crate::ArrayHeader>(), i),
                output.add(at),
            );
            // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
            output.add(at).write(b':');
            at += 1;
            at += emit_piece(
                value_plan[i],
                slot(obj.cast(), std::mem::size_of::<crate::ObjectHeader>(), i),
                output.add(at),
            );
        }
        // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
        output.add(at).write(b'}');
        debug_assert_eq!(at + 1, bytes as usize);
        Some(JSValue::string_ptr(result))
    })
}

#[cfg(test)]
#[path = "stringify_flat_tests.rs"]
mod tests;
