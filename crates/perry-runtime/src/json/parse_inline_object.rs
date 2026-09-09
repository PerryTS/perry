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

/// Decode the canonical one-field form without constructing the eight-entry
/// generic plan. The returned pair is entirely inline and owns no source
/// pointer, so allocation may still happen only after the whole input passes.
#[inline(always)]
pub(super) fn decode_one_field(bytes: &[u8]) -> Option<(JSValue, JSValue)> {
    if bytes.len() < 7 || bytes.first() != Some(&b'{') || bytes.last() != Some(&b'}') {
        return None;
    }
    let key_start = 2;
    if bytes.get(1) != Some(&b'"') {
        return None;
    }
    let key_limit = (key_start + INLINE_BYTES + 1).min(bytes.len() - 1);
    let key_end = bytes[key_start..key_limit]
        .iter()
        .position(|&byte| byte == b'"')?
        + key_start;
    let key_bytes = &bytes[key_start..key_end];
    if key_bytes.iter().any(|&byte| byte < 0x20 || byte == b'\\') {
        return None;
    }
    std::str::from_utf8(key_bytes).ok()?;
    let key = JSValue::try_short_string(key_bytes)?;
    if bytes.get(key_end + 1) != Some(&b':') {
        return None;
    }
    let value_bytes = bytes.get(key_end + 2..bytes.len() - 1)?;
    if value_bytes.is_empty() || value_bytes.iter().any(|&byte| whitespace(byte)) {
        return None;
    }
    let value = match value_bytes {
        [digit @ b'0'..=b'9'] => JSValue::number((digit - b'0') as f64),
        _ => super::parse_scalar::try_parse_scalar(value_bytes)?,
    };
    Some((key, value))
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
    allocate_fields(&plan.fields[..plan.len])
}

#[inline(never)]
pub(super) unsafe fn allocate_one_field(field: (JSValue, JSValue)) -> Option<JSValue> {
    allocate_fields(std::slice::from_ref(&field))
}

unsafe fn allocate_fields(fields: &[(JSValue, JSValue)]) -> Option<JSValue> {
    let cache_index = super::PARSE_SHAPE_CACHE.with(|cache| {
        cache
            .borrow()
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, entry)| {
                if entry.keys.len() != fields.len() {
                    return None;
                }
                if let [(key, _)] = fields {
                    return (entry.one_field_key_bits == key.bits()).then_some(index);
                }
                let mut scratch = [0; INLINE_BYTES];
                for ((key, _), &stored) in fields.iter().zip(&entry.keys) {
                    let len = key.short_string_to_buf(&mut scratch);
                    if (*stored).byte_len as usize != len
                        || std::slice::from_raw_parts(crate::string::string_data(stored), len)
                            != &scratch[..len]
                    {
                        return None;
                    }
                }
                Some(index)
            })
    })?;

    // The index proof above owns no managed pointer. Service pending work
    // before loading the cache entry's GC-rewritten keys pointer, then keep
    // movement suppressed only across final object birth.
    let serviced_pending_gc = crate::gc::gc_collect_pending_suppressed_parse();
    let cached_shape = || {
        super::PARSE_SHAPE_CACHE.with(|cache| {
            let cache = cache.borrow();
            let entry = cache
                .get(cache_index)
                .expect("parse shape cache is stable across collection");
            (entry.keys_array, entry.shape_id)
        })
    };
    let (keys, shape_id) = cached_shape();
    let object = if fields.len() == 1 && !serviced_pending_gc {
        crate::object::try_object_from_prevalidated_one_field(shape_id, fields[0].1)
    } else {
        // The common path bumps the current nursery block without collection.
        // This remains a fully-accounted arena birth, while the short no-move
        // scope keeps the keys and shape stable until the final header exists.
        let _no_move = crate::gc::GcSuppressScope::new();
        crate::object::try_object_from_inline_json_fields(keys, shape_id, fields)
    };
    let object = if let Some(object) = object {
        object
    } else {
        // Preserve the ordinary allocator's block-rollover collection point.
        // The failed try did not allocate or move anything. Run the trigger
        // outside suppression, then re-read the cache's rewritten pointer.
        crate::gc::gc_check_trigger();
        let (keys, shape_id) = cached_shape();
        let _no_move = crate::gc::GcSuppressScope::new();
        crate::object::object_from_inline_json_fields(keys, shape_id, fields)
    };
    // The key edge lives in the shape descriptor; inline slots contain no
    // managed pointers and retain the newborn's pointer-free layout.
    super::parse_scalar::clear_oversized_key_cache();
    crate::gc::gc_schedule_tiny_parse_boundary_collection_if_pressure();
    Some(JSValue::object_ptr(object.cast()))
}

#[cfg(test)]
#[path = "parse_inline_object_tests.rs"]
mod tests;
