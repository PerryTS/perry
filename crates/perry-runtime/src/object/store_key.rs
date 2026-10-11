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

/// The receiver's ShapeId word before a slow store, so the miss handler can
/// tell afterwards whether the store changed the receiver's layout (the
/// pre-shape a key-add memo is keyed on). 0 for anything that is not a heap
/// object.
///
/// # Safety
/// `target` is a live value.
#[inline]
pub(crate) unsafe fn pre_store_shape(target: f64) -> u32 {
    let bits = target.to_bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return 0;
    }
    let obj = (bits & crate::value::POINTER_MASK) as *const crate::ObjectHeader;
    if !crate::value::addr_class::is_above_handle_band(obj as usize) {
        return 0;
    }
    match crate::value::addr_class::try_read_gc_header(obj as usize) {
        Some(h) if h.obj_type == crate::gc::GC_TYPE_OBJECT => {
            crate::object::shapes::object_shape_stamp(obj)
        }
        _ => 0,
    }
}

/// Walk `obj`'s prototype chain exactly as `class_instance_set_may_intercept`
/// does (`js_object_get_prototype_of`) and mark every hop as a prototype, so
/// a later change to any hop moves `proto_validity`. `false`
/// when a hop is anything but an ordinary heap object — a Proxy, a function
/// object, a handle — or the chain does not end within 64 hops; the caller
/// then records nothing that depends on the chain.
///
/// # Safety
/// `receiver` is a live ordinary object the caller holds rooted.
pub(crate) unsafe fn mark_chain_hops(scope: &crate::gc::RuntimeHandleScope, receiver: f64) -> bool {
    let mut hop = scope.root_nanbox_f64(crate::object::js_object_get_prototype_of(receiver));
    for _ in 0..64 {
        let bits = hop.get_nanbox_f64().to_bits();
        if bits == crate::value::TAG_NULL {
            return true;
        }
        if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
            return false;
        }
        let addr = (bits & crate::value::POINTER_MASK) as usize;
        if !crate::value::addr_class::is_above_handle_band(addr)
            || !crate::value::addr_class::try_read_gc_header(addr)
                .is_some_and(|h| h.obj_type == crate::gc::GC_TYPE_OBJECT)
        {
            return false;
        }
        if !crate::object::proto_validity::object_is_marked_prototype(addr) {
            let _ = crate::object::proto_validity::mark_object_as_prototype(addr);
        }
        let next = crate::object::js_object_get_prototype_of(hop.get_nanbox_f64());
        hop = scope.root_nanbox_f64(next);
    }
    false
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

    #[test]
    fn a_non_object_target_has_no_pre_store_shape() {
        unsafe {
            assert_eq!(pre_store_shape(2.0), 0);
            assert_eq!(pre_store_shape(f64::from_bits(crate::value::TAG_NULL)), 0);
        }
    }
}
