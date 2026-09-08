//! Decode bounded flat objects without managed intermediates. A warm schema
//! reuses the existing bounded canonical-key cache; values are always decoded
//! again. Unsupported input and cold schemas keep the direct parser.

use crate::JSValue;

pub(super) const MAX_BYTES: usize = 64;
const MAX_FIELDS: usize = 8;
const INLINE_BYTES: usize = crate::value::SHORT_STRING_MAX_LEN;

pub(super) struct Plan {
    fields: [(JSValue, JSValue); MAX_FIELDS],
    len: usize,
}

fn whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

fn skip_space(bytes: &[u8], at: &mut usize) {
    while bytes.get(*at).copied().is_some_and(whitespace) {
        *at += 1;
    }
}

fn inline_string(bytes: &[u8], at: &mut usize) -> Option<JSValue> {
    if bytes.get(*at) != Some(&b'"') {
        return None;
    }
    *at += 1;
    let start = *at;
    loop {
        let byte = *bytes.get(*at)?;
        if byte == b'"' {
            let text = &bytes[start..*at];
            std::str::from_utf8(text).ok()?;
            *at += 1;
            return JSValue::try_short_string(text);
        }
        if *at - start == INLINE_BYTES || byte < 0x20 || byte == b'\\' {
            return None;
        }
        *at += 1;
    }
}

/// A successful plan owns only inline bits. No input borrow or managed edge
/// escapes this function, including for negative zero and inline UTF-8.
#[inline(never)]
pub(super) fn decode(bytes: &[u8]) -> Option<Plan> {
    if bytes.len() > MAX_BYTES {
        return None;
    }
    let mut at = 0;
    skip_space(bytes, &mut at);
    if bytes.get(at) != Some(&b'{') {
        return None;
    }
    at += 1;
    let mut plan = Plan {
        fields: [(JSValue::undefined(), JSValue::undefined()); MAX_FIELDS],
        len: 0,
    };
    loop {
        skip_space(bytes, &mut at);
        let key = inline_string(bytes, &mut at)?;
        skip_space(bytes, &mut at);
        if bytes.get(at) != Some(&b':') {
            return None;
        }
        at += 1;
        skip_space(bytes, &mut at);
        let value = if bytes.get(at) == Some(&b'"') {
            inline_string(bytes, &mut at)?
        } else {
            let start = at;
            while bytes
                .get(at)
                .is_some_and(|&b| b != b',' && b != b'}' && !whitespace(b))
            {
                at += 1;
            }
            // The scalar parser owns the numerical grammar and rounding.
            // It can only return a number, boolean, null, or inline string.
            super::parse_scalar::try_parse_scalar(&bytes[start..at])?
        };
        if let Some(index) = plan.fields[..plan.len]
            .iter()
            .position(|(k, _)| k.bits() == key.bits())
        {
            // JSON duplicate names keep their first position and last value.
            plan.fields[index].1 = value;
        } else {
            if plan.len == MAX_FIELDS {
                return None;
            }
            plan.fields[plan.len] = (key, value);
            plan.len += 1;
        }
        skip_space(bytes, &mut at);
        match bytes.get(at) {
            Some(b',') => at += 1,
            Some(b'}') => {
                at += 1;
                skip_space(bytes, &mut at);
                return (at == bytes.len()).then_some(plan);
            }
            _ => return None,
        }
    }
}

/// Return None only before any collection or allocation. Thus callers can
/// safely retain their original input pointer on the fallback route.
#[inline(never)]
pub(super) unsafe fn allocate(plan: &Plan) -> Option<JSValue> {
    let keys = super::PARSE_SHAPE_CACHE.with(|cache| {
        cache.borrow().iter().rev().find_map(|entry| {
            if entry.keys.len() != plan.len {
                return None;
            }
            let mut scratch = [0; INLINE_BYTES];
            for ((key, _), &stored) in plan.fields[..plan.len].iter().zip(&entry.keys) {
                let len = key.short_string_to_buf(&mut scratch);
                if (*stored).byte_len as usize != len
                    || std::slice::from_raw_parts(crate::string::string_data(stored), len)
                        != &scratch[..len]
                {
                    return None;
                }
            }
            Some(entry.keys_array)
        })
    })?;

    let scope = crate::gc::RuntimeHandleScope::new();
    let keys = scope.root_raw_mut_ptr(keys);
    crate::gc::gc_collect_pending_suppressed_parse();
    let object = {
        // The common path bumps the current nursery block without collection.
        // This remains a fully-accounted arena birth, while the short no-move
        // scope keeps the keys and shape stable until the final header exists.
        let _no_move = crate::gc::GcSuppressScope::new();
        keys.with_mut_ptr(|key_ptr: *mut crate::ArrayHeader| {
            let id = crate::object::shapes::shape_id_for_keys_ensure(key_ptr, plan.len as u32);
            crate::object::try_object_from_inline_json_fields(key_ptr, id, &plan.fields[..plan.len])
        })
    };
    let object = if let Some(object) = object {
        object
    } else {
        // Preserve the ordinary allocator's block-rollover collection point.
        // The failed try did not allocate or move anything. Run the trigger
        // outside suppression, then re-read every movable fact from its handle.
        crate::gc::gc_check_trigger();
        let _no_move = crate::gc::GcSuppressScope::new();
        keys.with_mut_ptr(|key_ptr: *mut crate::ArrayHeader| {
            let id = crate::object::shapes::shape_id_for_keys_ensure(key_ptr, plan.len as u32);
            crate::object::object_from_inline_json_fields(key_ptr, id, &plan.fields[..plan.len])
        })
    };
    // The key edge lives in the shape descriptor; inline slots contain no
    // managed pointers and retain the newborn's pointer-free layout.
    super::parse_scalar::clear_oversized_key_cache();
    crate::gc::gc_schedule_parse_boundary_collection_if_pressure();
    Some(JSValue::object_ptr(object.cast()))
}

#[cfg(test)]
#[path = "parse_inline_object_tests.rs"]
mod tests;
