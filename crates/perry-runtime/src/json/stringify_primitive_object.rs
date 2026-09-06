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

/// # Safety
/// The caller has resolved object/toJSON handling, rejected descriptors and
/// class instances, and proved that every logical field is inline and passes
/// `field_is_primitive`. Keys and field storage remain live, without callbacks,
/// managed allocation or a safepoint from that validation through this return.
/// `order`, if present, is the existing ECMA own-key order for this keys array.
pub(super) unsafe fn emit_validated(
    obj: *const crate::ObjectHeader,
    keys: *const crate::ArrayHeader,
    order: Option<&[u32]>,
    buf: &mut String,
) {
    let fields = (obj as *const u8)
        .add(std::mem::size_of::<crate::ObjectHeader>())
        .cast::<u64>();
    let key_slots = (keys as *const u8)
        .add(std::mem::size_of::<crate::ArrayHeader>())
        .cast::<u64>();
    buf.push('{');
    let mut first = true;
    for j in 0..(*keys).length as usize {
        let f = order.map_or(j, |indices| indices[j] as usize);
        let key_bits = *key_slots.add(f);
        if key_bits == crate::value::TAG_HOLE {
            continue;
        }
        let bits = *fields.add(f);
        if bits == TAG_UNDEFINED {
            continue;
        }
        debug_assert!(field_is_primitive(bits));
        if !first {
            buf.push(',');
        }
        first = false;
        let mut key_sso = [0; crate::value::SHORT_STRING_MAX_LEN];
        if let Some(key) = super::stringify::object_key_str(key_bits, &mut key_sso) {
            write_escaped_string(buf, key);
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
}

#[cfg(test)]
#[path = "stringify_primitive_object_tests.rs"]
mod tests;
