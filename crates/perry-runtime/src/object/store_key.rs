//! Allocation-free helpers for a store that may ADD a key: name the key by
//! its interned pointer, read the receiver's ShapeId before the store, and
//! mark the receiver's prototype chain before a verdict that depends on it is
//! recorded (`proxy::put_value::packed_add`, `object::shape_store`).

/// Is `key` an interned heap-string key a named-store transition can use?
///
/// # Safety
/// `key` is null or a live `StringHeader`.
#[inline]
unsafe fn eligible_key(key: *const crate::StringHeader) -> bool {
    if key.is_null() || !crate::value::addr_class::is_above_handle_band(key as usize) {
        return false;
    }
    let Some(gc) = crate::value::addr_class::try_read_gc_header(key as usize) else {
        return false;
    };
    if gc.obj_type != crate::gc::GC_TYPE_STRING
        || gc.gc_flags & (crate::gc::GC_FLAG_FORWARDED | crate::gc::GC_FLAG_INTERNED)
            != crate::gc::GC_FLAG_INTERNED
    {
        return false;
    }
    let len = (*key).byte_len as usize;
    if len == 0 {
        return false;
    }
    // Private members (`#x`) are not ordinary properties, and a canonical
    // index (digit-leading) can reach Array-subclass / `Object.prototype`
    // index bookkeeping the append does not perform.
    let first = *crate::string::string_data(key);
    first != b'#' && !first.is_ascii_digit()
}

/// The interned key pointer for a store's key VALUE, without allocating: a
/// heap string that is already interned, or an SSO immediate whose content
/// is in the intern table. `None` for anything else — the caller takes the
/// ordinary path, which interns it, so the next store of the same key finds
/// it.
///
/// # Safety
/// `key` is a live key value.
#[inline]
pub(crate) unsafe fn interned_key_for_store(key: f64) -> Option<*const crate::StringHeader> {
    let bits = key.to_bits();
    let jv = crate::value::JSValue::from_bits(bits);
    if bits & !crate::value::POINTER_MASK == crate::value::STRING_TAG {
        let ptr = (bits & crate::value::POINTER_MASK) as *const crate::StringHeader;
        if eligible_key(ptr) {
            return Some(ptr);
        }
        // A string LITERAL is allocated at module init with
        // `js_string_from_bytes`, not interned, so a key longer than an SSO
        // immediate usually arrives here as a private copy. Its interned twin
        // — the pointer the transition table and every other site use — is a
        // read-only table probe away.
        if !crate::value::addr_class::is_above_handle_band(ptr as usize)
            || !crate::value::addr_class::try_read_gc_header(ptr as usize)
                .is_some_and(|h| h.obj_type == crate::gc::GC_TYPE_STRING)
        {
            return None;
        }
        let len = (*ptr).byte_len as usize;
        if len == 0 || len > (*ptr).capacity as usize {
            return None;
        }
        let bytes = std::slice::from_raw_parts(crate::string::string_data(ptr), len);
        let twin = crate::string::intern_lookup_bytes(bytes)?;
        return eligible_key(twin).then_some(twin);
    }
    if jv.is_short_string() {
        let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let bytes = crate::string::js_string_key_bytes(jv, &mut buf)?;
        // The same admission as eligible_key, before probing the atom table:
        // an index or private name cannot use a named-store transition.
        if bytes
            .first()
            .is_none_or(|b| *b == b'#' || b.is_ascii_digit())
        {
            return None;
        }
        let ptr = crate::string::intern_lookup_bytes(bytes)?;
        return eligible_key(ptr).then_some(ptr);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_non_string_key_has_no_interned_pointer() {
        unsafe {
            assert!(interned_key_for_store(1.5).is_none());
            assert!(interned_key_for_store(f64::from_bits(crate::value::TAG_UNDEFINED)).is_none());
        }
    }
}
