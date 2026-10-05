/// Per-key refinement of `OBJ_FLAG_HAS_DESCRIPTORS` for the store fast paths
/// (#10287). `true` proves no OWN string-keyed descriptor (attr or accessor)
/// covers `key` on `addr`, so a store of `key` meets the same own-property
/// preconditions as one on a receiver that never had a descriptor. Prototype
/// vetting stays with the caller.
///
/// zod v4 opens every schema constructor with
/// `Object.defineProperty(inst, "_zod", …)` and then installs ~60 methods by
/// assignment; the object-wide flag sent every one of those stores down the
/// full `OrdinarySet` walk.
///
/// Index-shaped keys stay on the slow walk: a boxed `String` wrapper
/// synthesizes non-writable index attributes that the summary never records.
#[inline(always)]
pub(crate) unsafe fn own_descriptors_skip_key(addr: usize, key: f64) -> bool {
    // An Ordinary shape proves the store layout, including numeric keys.
    // Unmarked native layouts still take the synthetic-property checks.
    if super::shapes::object_shape_record(addr as *const ObjectHeader).is_some_and(|shape| {
        shape.object_kind() == super::shapes::ShapeObjectKind::Ordinary && shape.summary() == 0
    }) && crate::value::JSValue::from_bits(key.to_bits()).is_any_string()
    {
        return true;
    }
    own_descriptors_skip_key_slow(addr, key)
}

#[cold]
#[inline(never)]
unsafe fn own_descriptors_skip_key_slow(addr: usize, key: f64) -> bool {
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(bytes) = crate::string::js_string_key_bytes(
        crate::value::JSValue::from_bits(key.to_bits()),
        &mut sso,
    ) else {
        return false;
    };
    if bytes.first().is_some_and(u8::is_ascii_digit) {
        return false;
    }
    // Charter step 3: exact from the keys for an ordinary object.
    if super::key_attrs::attrs_live_in_keys(addr) {
        return super::key_attrs::object_key_entry(addr as *const ObjectHeader, bytes) == 0;
    }
    // Exact when this owner carries descriptors for one key only (zod's
    // `_zod`): a full-width hash compare, no Bloom, no table probe.
    if let Some(meta) = descriptor_summary_meta(addr) {
        if !meta.is_null() && (*meta).descriptor_key_count == 1 {
            return super::key_bytes_hash(bytes.as_ptr(), bytes.len())
                != (*meta).descriptor_key_hash;
        }
    }
    if !own_descriptor_may_cover_key(addr, key) {
        // Clear summary bits are authoritative: no entry can exist.
        return true;
    }
    // A SET bit is a maybe — the summary is 64 bits wide, so roughly one key
    // in 64 collides with a descriptor key. Confirm against the tables rather
    // than surrendering the fast path.
    //
    // This matters far more than the collision rate suggests: a store that
    // takes the slow path appends to a PRIVATE keys array, which takes the
    // receiver off the shared transition chain for good. Every later store on
    // it then misses the lane too. With one descriptor (`_zod`) and keys
    // `p0..p39`, `p17` collides — so every receiver derailed at the same
    // store and lost the chain for its remaining 23 properties.
    let Ok(name) = std::str::from_utf8(bytes) else {
        return false;
    };
    get_accessor_descriptor(addr, name).is_none() && get_property_attrs(addr, name).is_none()
}
