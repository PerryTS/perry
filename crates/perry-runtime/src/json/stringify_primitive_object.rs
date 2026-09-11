//! Emit a prevalidated object's inline primitive fields without callbacks.
//!
//! The general object walker owns the input root, prototype checks, layout
//! validation and property ordering. This leaf borrows that input until it
//! finishes; only the Rust output buffer can grow during the walk.

use super::*;
use std::fmt::Write;

#[inline]
pub(super) fn field_is_primitive(bits: u64) -> bool {
    bits != crate::value::TAG_HOLE
        && bits & crate::value::TAG_MASK != POINTER_TAG
        && bits & crate::value::TAG_MASK != BIGINT_TAG
        // Conservatively decline pointer-shaped subnormals too. The general
        // walker distinguishes them from tracked raw pointers; this leaf
        // needs only a cheap proof that neither case requires inspection.
        && !untagged_pointer_bits(bits)
}

/// The caller has proved that `count` logical slots fit the live inline
/// allocation. Checking these slots cannot call user code or collect, so the
/// field base can be borrowed once for the whole preflight too.
#[cfg(test)]
pub(super) unsafe fn fields_are_primitive(obj: *const crate::ObjectHeader, count: u32) -> bool {
    let fields = (obj as *const u8)
        .add(std::mem::size_of::<crate::ObjectHeader>())
        .cast::<u64>();
    std::slice::from_raw_parts(fields, count as usize)
        .iter()
        .all(|&bits| field_is_primitive(bits))
}

/// # Safety
/// The caller has resolved object/toJSON handling, rejected descriptors and
/// class instances, and proved that every logical field is inline and passes
/// `field_is_primitive`. Keys and field storage remain live, without callbacks,
/// managed allocation or a safepoint from that validation through this return.
/// `order`, if present, is the existing ECMA own-key order for this keys array.
#[cfg(test)]
pub(super) unsafe fn emit_validated(
    obj: *const crate::ObjectHeader,
    keys: *const crate::ArrayHeader,
    order: Option<&[u32]>,
    buf: &mut String,
) {
    let emitted = emit::<false>(obj, keys, order, buf);
    debug_assert!(emitted);
}

/// Validate and emit in one pass. A late complex field rolls the native output
/// buffer back to its entry length so the general object walker can take over.
/// No managed allocation, callback or safepoint occurs during the attempt.
pub(super) unsafe fn try_emit(
    obj: *const crate::ObjectHeader,
    keys: *const crate::ArrayHeader,
    buf: &mut String,
) -> bool {
    emit::<true>(obj, keys, None, buf)
}

#[inline(always)]
fn key_needs_ecma_reordering(key: &str) -> bool {
    key.as_bytes().first().is_some_and(u8::is_ascii_digit)
        && crate::object::canonical_array_index(key).is_some()
}

unsafe fn emit<const VALIDATE: bool>(
    obj: *const crate::ObjectHeader,
    keys: *const crate::ArrayHeader,
    order: Option<&[u32]>,
    buf: &mut String,
) -> bool {
    let saved_len = buf.len();
    let fields = (obj as *const u8)
        .add(std::mem::size_of::<crate::ObjectHeader>())
        .cast::<u64>();
    let key_slots =
        crate::array::array_elements_ptr(keys as *const crate::ArrayHeader).cast::<u64>();
    buf.push('{');
    let mut first = true;
    for j in 0..(*keys).length as usize {
        let f = order.map_or(j, |indices| indices[j] as usize);
        let key_bits = *key_slots.add(f);
        if key_bits == crate::value::TAG_HOLE {
            continue;
        }
        let bits = *fields.add(f);
        if VALIDATE && !field_is_primitive(bits) {
            buf.truncate(saved_len);
            return false;
        }
        if bits == TAG_UNDEFINED {
            continue;
        }
        debug_assert!(field_is_primitive(bits));
        if !first {
            buf.push(',');
        }
        first = false;
        let mut key_sso = [0; crate::value::SHORT_STRING_MAX_LEN];
        let key_written = if key_bits & crate::value::TAG_MASK == STRING_TAG {
            let ptr = (key_bits & POINTER_MASK) as *const StringHeader;
            if VALIDATE && str_from_header(ptr).is_some_and(key_needs_ecma_reordering) {
                buf.truncate(saved_len);
                return false;
            }
            write_heap_string(buf, ptr)
        } else if let Some(key) = super::stringify::object_key_str(key_bits, &mut key_sso) {
            if VALIDATE && key_needs_ecma_reordering(key) {
                buf.truncate(saved_len);
                return false;
            }
            write_escaped_string(buf, key);
            true
        } else {
            false
        };
        if key_written {
            buf.push(':');
        } else {
            let _ = write!(buf, "\"field{}\":", f);
        }
        match bits {
            TAG_NULL => buf.push_str("null"),
            TAG_TRUE => buf.push_str("true"),
            TAG_FALSE => buf.push_str("false"),
            _ => match bits & crate::value::TAG_MASK {
                STRING_TAG => {
                    let ptr = (bits & POINTER_MASK) as *const StringHeader;
                    if let Some(text) = str_from_header(ptr) {
                        write_escaped_string(buf, text);
                    } else {
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
#[path = "stringify_primitive_object_tests.rs"]
mod tests;
