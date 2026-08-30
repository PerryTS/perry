//! Non-rooting fast paths for the dynamic `obj[key] = value` write:
//! the existing-own-data overwrite and the shape-transition-cache
//! entry point. Split out of `object/field_set_by_name.rs` (issue
//! #7402) — pure relocation, no logic changes.

use super::*;

#[cfg(test)]
thread_local! {
    static TEST_TRANSITION_FAST_HITS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn test_reset_transition_fast_hits() {
    TEST_TRANSITION_FAST_HITS.with(|hits| hits.set(0));
}

#[cfg(test)]
pub(crate) fn test_transition_fast_hits() -> u64 {
    TEST_TRANSITION_FAST_HITS.with(std::cell::Cell::get)
}

/// Non-allocating-in-the-GC-heap overwrite for an existing own data field.
///
/// This is the common assignment case for ordinary objects.  It is deliberately
/// conservative: anything with per-object semantics (descriptors, URL backing
/// state, a changed prototype, frozen-family flags, or a special object class)
/// falls through to the complete `[[Set]]` implementation.
///
/// The key must already be the canonical interned heap string emitted by
/// codegen.  No arena allocation occurs here, so callers may use this before
/// opening a `RuntimeHandleScope`.
#[inline]
pub(crate) unsafe fn try_existing_own_data_overwrite(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> bool {
    let obj_addr = obj as usize;
    let key_addr = key as usize;
    if obj.is_null() || key.is_null() {
        return false;
    }

    let Some(obj_gc) = crate::value::addr_class::try_read_gc_header(obj_addr) else {
        return false;
    };
    const BLOCKING_FLAGS: u16 = crate::gc::OBJ_FLAG_FROZEN
        | crate::gc::OBJ_FLAG_SEALED
        | crate::gc::OBJ_FLAG_NO_EXTEND
        | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
        | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO;
    if obj_gc.obj_type != crate::gc::GC_TYPE_OBJECT
        || obj_gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || obj_gc._reserved & BLOCKING_FLAGS != 0
        // A per-evaluation class object can carry dynamic static accessors in
        // the class registry while retaining an ordinary backing slot with the
        // same key. Overwriting that slot directly bypasses the accessor
        // setter, so class constructors must always take the full exotic
        // `[[Set]]` path.
        || crate::object::class_registry::is_class_object_ptr(obj.cast())
        || (*obj).class_id == NATIVE_MODULE_CLASS_ID
        || crate::array::object_prototype_addr_matches(obj_addr)
        // URL's visible fields are live views over one backing URL. An own
        // slot exists for e.g. `pathname`, but its setter must also rebuild
        // `href`/`origin`; do not mistake that slot for ordinary data.
        || ((*obj).class_id == 0 && crate::url::is_url_object_shape(obj))
    {
        return false;
    }
    // The header probe above already established a live, non-forwarded
    // `GC_TYPE_OBJECT`. Resolve its immutable descriptor once for both the
    // ordinary-object discriminator and the live-slot bound used below.
    // `object_is_regular` followed by `object_live_slot_count` repeated both
    // the allocator classification and this ShapeId table lookup.
    let Some(shape) = crate::object::shapes::object_shape_descriptor(obj) else {
        return false;
    };
    if shape.object_kind != crate::object::shapes::ShapeObjectKind::Ordinary {
        return false;
    }
    let live_slots = shape.live_inline_slot_count;

    let Some(key_gc) = crate::value::addr_class::try_read_gc_header(key_addr) else {
        return false;
    };
    if key_gc.obj_type != crate::gc::GC_TYPE_STRING
        || key_gc.gc_flags & (crate::gc::GC_FLAG_FORWARDED | crate::gc::GC_FLAG_INTERNED)
            != crate::gc::GC_FLAG_INTERNED
    {
        return false;
    }

    let keys = crate::object::object_keys_array(obj);
    let keys_addr = keys as usize;
    if keys.is_null() || (keys_addr as u64) >> 48 != 0 {
        return false;
    }
    let Some(keys_gc) = crate::value::addr_class::try_read_gc_header(keys_addr) else {
        return false;
    };
    if keys_gc.obj_type != crate::gc::GC_TYPE_ARRAY
        || keys_gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return false;
    }

    let mut own_idx = super::prop_plan::read_plan_lookup(keys_addr, key_addr);
    if own_idx.is_none() {
        let key_count = crate::array::keys_array_len_capped_to_capacity(keys);
        if key_count > 4096 {
            return false;
        }
        // The write twin of the read lane's resolver: shape hash index first,
        // raw dense-slot scan as its own fallback. The open-coded walk this
        // replaces ran `js_array_get` (which additionally probes for per-index
        // accessors) plus a string compare per key, in full, every time the
        // epoch-guarded read plan was flushed — the same miss-path cost #8936
        // and #8950 removed from their sides of the property paths.
        own_idx = crate::object::keys_find_slot_by_key_ptr(keys, key_count as u32, key);
        if let Some(i) = own_idx {
            super::prop_plan::read_plan_record(keys_addr, key_addr, i);
        }
    }
    let Some(idx) = own_idx else {
        return false;
    };

    let vbits = value.to_bits();
    let vbits = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
        crate::value::TAG_UNDEFINED
    } else {
        vbits
    };
    super::mark_object_dynamic_shape_unknown(obj);
    let alloc_limit = std::cmp::max(live_slots, crate::object::INLINE_SLOT_FLOOR as u32) as usize;
    if (idx as usize) < alloc_limit {
        if idx >= live_slots {
            set_object_live_slot_count(obj, idx + 1);
        }
        store_object_field_slot(obj, idx as usize, vbits);
    } else {
        overflow_set(obj_addr, idx as usize, vbits);
    }
    true
}

/// Fast transition-cache-backed dynamic property write.
///
/// This is intentionally narrower than `js_object_set_field_by_name`: it only
/// handles plain object-shape transitions that have already been learned by
/// the runtime transition cache. In addition to class-id-zero objects, HIR's
/// registered anonymous-shape classes qualify: they are the runtime backing
/// for source-level object literals and have ordinary `Object.prototype`
/// semantics. User class instances, accessors/descriptors, frozen/sealed
/// objects, prototype overrides, closures, native handles, arrays, strings,
/// and cache misses return 0 so callers preserve the full setter semantics by
/// falling back to `js_object_set_field_by_name`.
#[no_mangle]
pub extern "C" fn js_object_set_field_by_name_transition_fast(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> i32 {
    object_set_field_by_name_transition_fast_impl(obj, key, value, true)
}

/// Transition-only form for callers that already own a complete semantic
/// fallback. If `key` is an existing property, no append edge can exist for
/// `(current_keys, key)`, so the lookup returns 0 and the caller performs the
/// ordinary write. Skipping the up-front linear overwrite scan is important
/// for repeated computed-key object construction.
pub(crate) fn object_set_field_by_name_transition_only_fast(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> i32 {
    object_set_field_by_name_transition_fast_impl(obj, key, value, false)
}

/// Re-add a deleted property on a receiver whose ShapeId deliberately stayed
/// stable across the tombstone delete (#9064).
///
/// The write stub has already identified the key's former slot and observed
/// `TAG_HOLE`; this helper completes the absent-property transition without
/// repeating the full `OrdinarySet` ladder. It remains conservative: the key
/// must still be absent, the keys array must still be private, and the normal
/// prototype-interception proof must clear. The key is appended rather than
/// resurrected in its old slot, preserving ECMAScript enumeration order.
/// Returns the new IC slot word (including the overflow tag), the post-GC
/// receiver, and the post-GC value on success.
pub(crate) fn try_readd_stable_tombstone(
    obj: *mut ObjectHeader,
    key: f64,
    value: f64,
) -> Option<(u32, *mut ObjectHeader, f64)> {
    let key_value = JSValue::from_bits(key.to_bits());
    if obj.is_null() || (!key_value.is_short_string() && !key_value.is_string()) {
        return None;
    }

    // The small-object churn case overwhelmingly has spare capacity: the
    // private keys array grows geometrically, then accepts several SSO names
    // before the next squeeze. Appending there cannot collect, so avoid three
    // runtime handles and the general Array.push classifier on that lane.
    // Every semantic gate from the rooted path is repeated inside the helper;
    // allocation/growth still falls through unchanged.
    if key_value.is_short_string() {
        if let Some(result) =
            unsafe { try_readd_stable_tombstone_sso_no_grow(obj, key_value, value) }
        {
            return Some(result);
        }
    }

    let initial_gc = unsafe { crate::value::addr_class::try_read_gc_header(obj as usize)? };
    if initial_gc.obj_type != crate::gc::GC_TYPE_OBJECT
        || initial_gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || initial_gc._reserved & crate::gc::OBJ_FLAG_STABLE_TOMBSTONES == 0
    {
        return None;
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let key_handle = scope.root_nanbox_f64(key);
    let value_handle = scope.root_nanbox_f64(value);

    unsafe {
        let mut key = key_handle.get_nanbox_f64();
        const BLOCKING_FLAGS: u16 = crate::gc::OBJ_FLAG_FROZEN
            | crate::gc::OBJ_FLAG_SEALED
            | crate::gc::OBJ_FLAG_NO_EXTEND
            | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
            | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO
            | crate::gc::GC_OBJ_TYPED_LAYOUT_INTACT;
        let eligible = obj_handle.with_mut_ptr(|obj: *mut ObjectHeader| {
            let gc = crate::value::addr_class::try_read_gc_header(obj as usize)?;
            Some(
                gc.obj_type == crate::gc::GC_TYPE_OBJECT
                    && gc.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
                    && gc._reserved & BLOCKING_FLAGS == 0
                    && gc._reserved & crate::gc::OBJ_FLAG_STABLE_TOMBSTONES != 0
                    && crate::object::object_is_regular(obj)
                    && !crate::array::object_prototype_addr_matches(obj as usize)
                    && ((*obj).class_id != 0 || !crate::url::is_url_object_shape(obj)),
            )
        })?;
        if !eligible {
            return None;
        }

        if obj_handle.with_mut_ptr(|obj: *mut ObjectHeader| {
            super::plain_data_write_may_intercept(obj as usize, 0, key)
        }) {
            return None;
        }

        key = key_handle.get_nanbox_f64();
        let (shape, keys) = obj_handle.with_mut_ptr(|obj: *mut ObjectHeader| {
            let shape = crate::object::shapes::object_shape_descriptor(obj)?;
            Some((shape, shape.keys as usize as *mut ArrayHeader))
        })?;
        if keys.is_null() || shape.logical_key_count > 4096 {
            return None;
        }
        let mut key_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let key_bytes =
            crate::string::js_string_key_bytes(JSValue::from_bits(key.to_bits()), &mut key_buf)?;
        let key_hash = key_bytes_hash(key_bytes.as_ptr(), key_bytes.len());
        let keys_gc = crate::value::addr_class::try_read_gc_header(keys as usize)?;
        if keys_gc.obj_type != crate::gc::GC_TYPE_ARRAY
            || keys_gc.gc_flags & (crate::gc::GC_FLAG_FORWARDED | crate::gc::GC_FLAG_SHAPE_SHARED)
                != 0
            || crate::object::keys_find_slot_by_bytes(keys, shape.logical_key_count, key_bytes)
                .is_some()
        {
            return None;
        }

        let new_index = shape.logical_key_count;
        let live_slots = shape.live_inline_slot_count;
        let alloc_limit = live_slots.max(crate::object::INLINE_SLOT_FLOOR as u32);
        let old_keys_handle = scope.root_raw_mut_ptr(keys);
        let ((new_keys, old_keys), obj) = obj_handle.across_mut::<ObjectHeader, _>(|| {
            old_keys_handle.across_mut::<ArrayHeader, _>(|| {
                crate::array::js_array_push(keys, JSValue::from_bits(key.to_bits()))
            })
        });
        let value = value_handle.get_nanbox_f64();
        set_object_keys_array(obj, new_keys);
        super::mark_object_dynamic_shape_unknown(obj);
        if old_keys != new_keys {
            super::shapes::shape_keys_grown(old_keys as usize, new_keys);
        }

        let mut value_bits = value.to_bits();
        if (value_bits >> 48) == 0x7FFD && (value_bits & 0x0000_FFFF_FFFF_FFFF) == 0 {
            value_bits = crate::value::TAG_UNDEFINED;
        }
        let slot_word = if new_index < alloc_limit {
            if new_index >= live_slots {
                set_object_live_slot_count(obj, new_index + 1);
            }
            store_object_field_slot(obj, new_index as usize, value_bits);
            new_index
        } else {
            overflow_set(obj as usize, new_index as usize, value_bits);
            new_index | crate::proxy::IC_SLOT_OVERFLOW_BIT
        };
        keys_index_insert(new_keys, new_index + 1, key_hash, new_index);
        Some((slot_word, obj, value))
    }
}

/// Non-allocating SSO append for a private stable-tombstone keys array that
/// already has capacity for one more entry.
unsafe fn try_readd_stable_tombstone_sso_no_grow(
    obj: *mut ObjectHeader,
    key: JSValue,
    value: f64,
) -> Option<(u32, *mut ObjectHeader, f64)> {
    let gc = crate::value::addr_class::try_read_gc_header(obj as usize)?;
    const BLOCKING_FLAGS: u16 = crate::gc::OBJ_FLAG_FROZEN
        | crate::gc::OBJ_FLAG_SEALED
        | crate::gc::OBJ_FLAG_NO_EXTEND
        | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
        | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO
        | crate::gc::GC_OBJ_TYPED_LAYOUT_INTACT;
    // Stable-tombstone admission already excludes real class/prototype
    // receivers; only class-less and registered anonymous-shape ordinary
    // objects can carry the flag into this append lane.
    if gc.obj_type != crate::gc::GC_TYPE_OBJECT
        || gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || gc._reserved & BLOCKING_FLAGS != 0
        || gc._reserved & crate::gc::OBJ_FLAG_STABLE_TOMBSTONES == 0
        || !crate::object::object_is_regular(obj)
        || crate::array::object_prototype_addr_matches(obj as usize)
        || ((*obj).class_id == 0 && crate::url::is_url_object_shape(obj))
    {
        return None;
    }

    let key_f64 = f64::from_bits(key.bits());
    if super::plain_data_write_may_intercept(obj as usize, 0, key_f64) {
        return None;
    }
    let shape = crate::object::shapes::object_shape_descriptor(obj)?;
    if shape.object_kind != crate::object::shapes::ShapeObjectKind::Ordinary
        || shape.logical_key_count >= 16
    {
        return None;
    }
    let keys = shape.keys as usize as *mut ArrayHeader;
    if keys.is_null() {
        return None;
    }
    let keys_gc = crate::value::addr_class::try_read_gc_header(keys as usize)?;
    if keys_gc.obj_type != crate::gc::GC_TYPE_ARRAY
        || keys_gc.gc_flags & (crate::gc::GC_FLAG_FORWARDED | crate::gc::GC_FLAG_SHAPE_SHARED) != 0
        || (*keys).length != shape.logical_key_count
        || (*keys).length >= (*keys).capacity
    {
        return None;
    }

    let mut key_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let key_bytes = crate::string::js_string_key_bytes(key, &mut key_buf)?;
    // All-holes is a constructive absence proof and is the steady state of a
    // one-live-key receiver immediately after delete.
    if shape.hole_count != shape.logical_key_count
        && crate::object::keys_find_slot_by_bytes(keys, shape.logical_key_count, key_bytes)
            .is_some()
    {
        return None;
    }
    let new_index = shape.logical_key_count;
    let alloc_limit = shape
        .live_inline_slot_count
        .max(crate::object::INLINE_SLOT_FLOOR as u32);
    let next_live = if new_index < alloc_limit {
        shape.live_inline_slot_count.max(new_index + 1)
    } else {
        shape.live_inline_slot_count
    };

    let elements = (keys as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
    crate::gc::runtime_store_external_jsvalue_slot(
        keys as usize,
        elements.add(new_index as usize) as usize,
        key.bits(),
    );
    (*keys).length = new_index + 1;
    if crate::object::shapes::try_update_stable_tombstone_shape_cached(
        obj,
        shape,
        new_index + 1,
        next_live,
        shape.hole_count,
    )
    .or_else(|| {
        crate::object::shapes::try_update_stable_tombstone_shape(
            obj,
            keys,
            new_index + 1,
            next_live,
            shape.hole_count,
        )
    })
    .is_none()
    {
        (*keys).length = new_index;
        crate::gc::runtime_store_external_jsvalue_slot(
            keys as usize,
            elements.add(new_index as usize) as usize,
            crate::value::TAG_HOLE,
        );
        return None;
    }

    super::mark_object_dynamic_shape_unknown(obj);
    let mut value_bits = value.to_bits();
    if (value_bits >> 48) == 0x7FFD && (value_bits & 0x0000_FFFF_FFFF_FFFF) == 0 {
        value_bits = crate::value::TAG_UNDEFINED;
    }
    let slot_word = if new_index < alloc_limit {
        store_object_field_slot(obj, new_index as usize, value_bits);
        new_index
    } else {
        overflow_set(obj as usize, new_index as usize, value_bits);
        new_index | crate::proxy::IC_SLOT_OVERFLOW_BIT
    };
    let key_hash = key_bytes_hash(key_bytes.as_ptr(), key_bytes.len());
    keys_index_insert(keys, new_index + 1, key_hash, new_index);
    Some((slot_word, obj, value))
}

fn object_set_field_by_name_transition_fast_impl(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
    try_overwrite: bool,
) -> i32 {
    if key.is_null() || (key as usize) < 0x10000 {
        return 0;
    }

    let obj = {
        let bits = obj as u64;
        let top16 = bits >> 48;
        if top16 >= 0x7FF8 {
            if top16 != 0x7FFD {
                // Not a POINTER-tagged heap receiver (SSO string payload,
                // UNDEFINED/NULL remnant, INT32, BIGINT…). The old catch-all
                // masked these to 48 bits — a 2–5-char SSO payload lands in
                // the 2–5.5TB range, passes the macOS heap floor, and the
                // GcHeader read below deref'd unmapped memory (write-side
                // #5429 twin, 2026-07-02 audit). Return 0 = defer to the
                // full dynamic path, which triages by tag.
                return 0;
            }
            let raw = (bits & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader;
            if raw.is_null() || crate::value::addr_class::is_small_handle(raw as usize) {
                return 0;
            }
            raw
        } else {
            obj
        }
    };

    if obj.is_null() || (obj as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return 0;
    }

    if try_overwrite && unsafe { try_existing_own_data_overwrite(obj, key, value) } {
        return 1;
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let key_handle = scope.root_string_ptr(key);
    let value_handle = scope.root_nanbox_f64(value);

    unsafe {
        let mut obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
        let key = key_handle.get_raw_const_ptr::<crate::StringHeader>();

        // Validated header probe (rejects the handle band, implausible
        // addresses, and slab allocations without touching memory) instead
        // of the bare floor + raw deref.
        let gc_header = match crate::value::addr_class::try_read_gc_header(obj as usize) {
            Some(h) => h as *const crate::gc::GcHeader,
            None => return 0,
        };
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT
            || (*gc_header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return 0;
        }
        let object_flags = (*gc_header)._reserved;
        if object_flags
            & (crate::gc::OBJ_FLAG_FROZEN
                | crate::gc::OBJ_FLAG_SEALED
                | crate::gc::OBJ_FLAG_NO_EXTEND
                // #6084 item 6: an own descriptor on THIS object (accessor or
                // non-writable) must route through the full setter semantics.
                | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
                | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO)
            != 0
        {
            return 0;
        }
        if !crate::object::object_is_regular(obj) || (*obj).class_id == NATIVE_MODULE_CLASS_ID {
            return 0;
        }

        let key_gc =
            (key as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;

        // Closed source-level object literals are represented as synthetic
        // `__AnonShape_*` classes so their static fields can use the same
        // ShapeId machinery as class instances. Semantically they are still
        // plain objects: codegen registers their class ids at module init and
        // prototype/constructor dispatch already treats them as having
        // ordinary Object semantics. Admit exactly that registered population
        // alongside genuinely class-id-zero objects; a real user class must
        // retain the full inherited-setter/prototype walk.
        let class_id = (*obj).class_id;
        if class_id != 0 && !crate::object::is_anon_shape_class_id(class_id) {
            return 0;
        }

        // #6084 item 6: this used to be a `GLOBAL_DESCRIPTORS_IN_USE` check at
        // the top of the function — one `Object.freeze` anywhere in the process
        // (even on an unrelated object) permanently disabled this fast path for
        // every object. Vet the receiver's own flag (above) and its prototype
        // chain (here) instead. Pass semantic class id zero for an anon shape:
        // its nonzero runtime id is an implementation detail, not a JS class
        // whose vtable/prototype chain can carry instance accessors.
        let key_f64 = f64::from_bits(JSValue::string_ptr(key as *mut _).bits());
        if super::plain_data_write_may_intercept(obj as usize, 0, key_f64) {
            return 0;
        }

        if (*key_gc).obj_type != crate::gc::GC_TYPE_STRING {
            return 0;
        }
        let interned_key = if (*key_gc).gc_flags & crate::gc::GC_FLAG_INTERNED != 0 {
            key
        } else {
            let hash = key_content_hash(key);
            crate::string::js_string_intern(key, hash)
        };
        if interned_key.is_null() {
            return 0;
        }

        obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
        let value = value_handle.get_nanbox_f64();

        let prev_shape_id = super::shapes::object_shape_stamp(obj);
        let Some((next_keys, slot_idx, target_shape_id)) =
            transition_cache_lookup(prev_shape_id, interned_key)
        else {
            return 0;
        };
        if next_keys == 0 {
            return 0;
        }

        // `Object.prototype[<index>]` must reach the ordinary setter so it can
        // invalidate array hole/OOB guards through
        // `note_object_prototype_index_write`. A canonical index must start
        // with an ASCII digit, so named transitions avoid the prototype TLS
        // lookup entirely. Probe only after a cache hit; ordinary misses
        // already take the semantic fallback.
        let key_starts_with_digit =
            (*key).byte_len != 0 && (*crate::string::string_data(key)).is_ascii_digit();
        if key_starts_with_digit && crate::array::object_prototype_addr_matches(obj as usize) {
            return 0;
        }

        if !super::shapes::install_cached_object_shape_transition(
            obj,
            prev_shape_id,
            target_shape_id,
            next_keys as *mut ArrayHeader,
        ) {
            set_object_keys_array(obj, next_keys as *mut ArrayHeader);
        }

        // #8113: one bound probe, reused.
        let live_slots = crate::object::object_live_slot_count(obj);
        let alloc_limit =
            std::cmp::max(live_slots, crate::object::INLINE_SLOT_FLOOR as u32) as usize;
        let slot_usize = slot_idx as usize;
        let vbits = value.to_bits();
        let vbits = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
            crate::value::TAG_UNDEFINED
        } else {
            vbits
        };

        if slot_usize < alloc_limit {
            if slot_idx >= live_slots {
                set_object_live_slot_count(obj, slot_idx + 1);
            }
            store_object_field_slot(obj, slot_usize, vbits);
        } else {
            overflow_set(obj as usize, slot_usize, vbits);
        }

        #[cfg(test)]
        TEST_TRANSITION_FAST_HITS.with(|hits| hits.set(hits.get() + 1));
    }

    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_fast_rejects_object_prototype_even_with_a_cached_edge() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let prototype = crate::array::object_prototype_addr() as *mut ObjectHeader;
        assert!(!prototype.is_null(), "test premise: Object.prototype");
        let prototype_handle = scope.root_raw_mut_ptr(prototype);

        let raw_key = crate::string::js_string_from_bytes(b"879400001".as_ptr(), 9);
        let raw_key_handle = scope.root_string_ptr(raw_key);
        let raw_key = raw_key_handle.get_raw_const_ptr::<crate::StringHeader>();
        let key = crate::string::js_string_intern(raw_key, key_content_hash(raw_key));
        let key_handle = scope.root_string_ptr(key);

        let prototype = prototype_handle.get_raw_mut_ptr::<ObjectHeader>();
        let predecessor = unsafe { super::super::shapes::object_shape_stamp(prototype) };
        assert!(
            super::super::shapes::is_shape_id(predecessor),
            "test premise: Object.prototype has a resolvable ShapeId"
        );
        let old_keys = unsafe { super::super::object_keys_array(prototype) };
        let next_keys = crate::array::js_array_clone(old_keys);
        let slot = crate::array::js_array_length(next_keys);
        let next_keys = crate::array::js_array_push(
            next_keys,
            crate::JSValue::string_ptr(key_handle.get_raw_mut_ptr()),
        );
        let next_keys_handle = scope.root_raw_mut_ptr(next_keys);
        let next_keys = next_keys_handle.get_raw_mut_ptr::<ArrayHeader>();
        let target = super::super::shapes::shape_descriptor_ensure(
            next_keys,
            slot + 1,
            unsafe { super::super::object_live_slot_count(prototype) }.max(slot + 1),
        )
        .expect("shape range unexpectedly exhausted");
        let key = key_handle.get_raw_const_ptr::<crate::StringHeader>();
        super::super::transition_cache_insert(
            std::ptr::null(),
            predecessor,
            key,
            next_keys as usize,
            slot,
            target,
        );
        assert!(
            super::super::transition_cache_lookup(predecessor, key).is_some(),
            "test premise: the synthetic transition must be cache-resident"
        );

        test_reset_transition_fast_hits();
        assert_eq!(
            object_set_field_by_name_transition_only_fast(prototype, key, 42.0),
            0,
            "Object.prototype must use the setter that records indexed writes"
        );
        assert_eq!(test_transition_fast_hits(), 0);
        super::super::test_clear_transition_cache_root();
    }
}
