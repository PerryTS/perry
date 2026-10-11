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
    if key.is_null() {
        return false;
    }
    existing_own_data_overwrite(obj, OverwriteKey::Interned(key), value)
}

/// [`try_existing_own_data_overwrite`] for a key that is not interned: a key
/// list holds its keys by content, so a key it lists is found by the key's
/// bytes as well as by the interned word. Nothing is interned and no read
/// plan is recorded (a plan is keyed by the interned word). `key_value` is
/// the key as a value, `key_bytes` its content.
///
/// # Safety
/// As [`try_existing_own_data_overwrite`]; `key_value` is a live string value
/// whose content is `key_bytes`.
#[inline]
pub(crate) unsafe fn try_existing_own_data_overwrite_by_content(
    obj: *mut ObjectHeader,
    key_value: f64,
    key_bytes: &[u8],
    value: f64,
) -> bool {
    existing_own_data_overwrite(obj, OverwriteKey::Content(key_value, key_bytes), value)
}

/// How an overwrite names its key.
#[derive(Clone, Copy)]
enum OverwriteKey<'a> {
    /// The canonical interned heap string.
    Interned(*const crate::StringHeader),
    /// A key value and its content.
    Content(f64, &'a [u8]),
}

#[inline(always)]
unsafe fn existing_own_data_overwrite(
    obj: *mut ObjectHeader,
    key: OverwriteKey<'_>,
    value: f64,
) -> bool {
    let obj_addr = obj as usize;
    if obj.is_null() {
        return false;
    }
    let key_value = match key {
        OverwriteKey::Interned(key) => f64::from_bits(JSValue::string_ptr(key as *mut _).bits()),
        OverwriteKey::Content(key_value, _) => key_value,
    };

    let Some(obj_gc) = crate::value::addr_class::try_read_gc_header(obj_addr) else {
        return false;
    };
    const BLOCKING_FLAGS: u16 = crate::gc::OBJ_FLAG_FROZEN
        | crate::gc::OBJ_FLAG_SEALED
        | crate::gc::OBJ_FLAG_NO_EXTEND
        | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO;
    if obj_gc.obj_type != crate::gc::GC_TYPE_OBJECT
        || obj_gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || obj_gc._reserved & BLOCKING_FLAGS != 0
        // #10287: an own descriptor is vetted per KEY. Overwriting a key the
        // summary proves uncovered cannot hit an own accessor or a
        // non-writable data property, which is what the object-wide flag
        // stood in for.
        || (obj_gc._reserved & crate::gc::OBJ_FLAG_HAS_DESCRIPTORS != 0
            && !crate::object::own_descriptors_skip_key(obj_addr, key_value))
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
    if !shape.object_kind.is_ordinary_layout() {
        return false;
    }
    // #10868 step 2.5 stage 1: this path takes its bound from the descriptor
    // and its keys pointer from `object_keys_array`; for a dictionary-mode
    // receiver those name different arrays.
    if crate::object::dictionary::is_dictionary(obj) {
        return false;
    }
    let live_slots = shape.live_inline_slot_count;

    if let OverwriteKey::Interned(key) = key {
        let Some(key_gc) = crate::value::addr_class::try_read_gc_header(key as usize) else {
            return false;
        };
        if key_gc.obj_type != crate::gc::GC_TYPE_STRING
            || key_gc.gc_flags & (crate::gc::GC_FLAG_FORWARDED | crate::gc::GC_FLAG_INTERNED)
                != crate::gc::GC_FLAG_INTERNED
        {
            return false;
        }
    }

    let keys_view = crate::object::object_keys(obj);
    let keys = keys_view.arr();
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

    // A read plan is keyed by the keys ARRAY, which lists on one growth chain
    // share: a slot learned on a longer list is not this receiver's unless it
    // is below this receiver's count.
    let own_idx = match key {
        OverwriteKey::Interned(key) => {
            let key_addr = key as usize;
            let mut own_idx =
                super::prop_plan::read_plan_lookup(keys_addr, key_addr, keys_view.count());
            if own_idx.is_none() {
                let key_count = keys_view.count() as usize;
                if key_count > 4096 {
                    return false;
                }
                // The write twin of the read lane's resolver: shape hash index
                // first, raw dense-slot scan as its own fallback. The
                // open-coded walk this replaces ran `js_array_get` (which
                // additionally probes for per-index accessors) plus a string
                // compare per key, in full, every time the epoch-guarded read
                // plan was flushed — the same miss-path cost #8936 and #8950
                // removed from their sides of the property paths.
                own_idx = if shape.summary & crate::object::key_attrs::SUMMARY_PRIVATE == 0 {
                    crate::object::keys_find_slot_by_key_ptr(keys, key_count as u32, key)
                } else {
                    crate::object::keys_find_property_slot_by_key_ptr(keys, key_count as u32, key)
                };
                if let Some(i) = own_idx {
                    super::prop_plan::read_plan_record(keys_addr, key_addr, i);
                }
            }
            own_idx
        }
        OverwriteKey::Content(_, bytes) => {
            let key_count = keys_view.count();
            if key_count > 4096 {
                return false;
            }
            // The key list's own keying: content, most-derived declaration
            // first (#10595), exactly as the interned lookup resolves it.
            if shape.summary & crate::object::key_attrs::SUMMARY_PRIVATE == 0 {
                crate::object::keys_find_slot_by_bytes(keys, key_count, bytes)
            } else {
                crate::object::keys_find_property_slot_by_bytes(keys, key_count, bytes)
            }
        }
    };
    let Some(idx) = own_idx else {
        return false;
    };
    if shape.summary
        & (crate::object::key_attrs::SUMMARY_BLOCKS_STORE
            | crate::object::key_attrs::SUMMARY_PRIVATE)
        != 0
        && !crate::object::key_attrs::entry_is_plain_writable_data(
            crate::object::key_attrs::keys_entry(keys, idx),
        )
    {
        return false;
    }

    let vbits = value.to_bits();
    let vbits = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
        crate::value::TAG_UNDEFINED
    } else {
        vbits
    };
    let alloc_limit = std::cmp::max(live_slots, crate::object::INLINE_SLOT_FLOOR as u32) as usize;
    if idx < live_slots {
        // An overwrite of a key the shape already places in a live inline
        // slot keeps the receiver's ShapeId, the layout a class-field read
        // guard compares. What the VALUE may change (a non-Number into an
        // `F64` lane) is decided per slot by the store check that
        // `store_object_field_slot` runs, which generalizes that lane and
        // restamps; the owner store funnel likewise retires a numeric-proof
        // ShapeId before publishing the new value.
        if crate::hot_diag::recv_routes_armed()
            && crate::object::field_rep::f64_lane_slots(crate::object::field_rep_store::shape_rep(
                crate::object::shapes::object_shape_stamp(obj),
            )) != 0
        {
            crate::hot_diag::recv_route_note_runtime(
                crate::hot_diag::RT_ROUTE_OVERWRITE_KEPT_TYPED,
            );
        }
        store_object_field_slot(obj, idx as usize, vbits);
    } else if (idx as usize) < alloc_limit {
        // The store widens the shape-visible live bound.
        set_object_live_slot_count(obj, idx + 1);
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
// #9287 moved the production caller to the value-returning form; this i32
// wrapper survives only as the shape the transition tests assert against.
#[cfg(test)]
pub(crate) fn object_set_field_by_name_transition_only_fast(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> i32 {
    object_set_field_by_name_transition_fast_impl(obj, key, value, false)
}

/// Value-returning transition-only entry (#9287) — callable with unrooted
/// operands; see `object_set_field_by_name_transition_fast_impl_value`.
pub(crate) fn object_set_field_by_name_transition_only_fast_value(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
    refresh: &mut Option<(f64, f64, f64)>,
) -> Option<f64> {
    object_set_field_by_name_transition_fast_impl_value::<false>(obj, key, value, false, refresh)
}

/// [`object_set_field_by_name_transition_only_fast_value`] for a CLASS
/// instance whose store site has already proved the prototype chain does not
/// intercept this key (`object::chain_store`). The plain-object lane refuses
/// class instances because it cannot prove that itself; with the proof in
/// hand the append is the same one a plain object takes. Every other check —
/// receiver kind and flags, an own descriptor for the key, the transition
/// edge (which exists only for a shape that lacks the key) — still runs.
pub(crate) fn object_set_field_by_name_transition_chain_proven_value(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
    refresh: &mut Option<(f64, f64, f64)>,
) -> Option<f64> {
    object_set_field_by_name_transition_fast_impl_value::<true>(obj, key, value, false, refresh)
}

/// Add `key` to a class-less ordinary object born with a null
/// `[[Prototype]]` (`Object.create(null)`, `{ __proto__: null }`, an
/// emitter's `_events`) through the learned key-add edge, with none of the
/// `[[Set]]` vet [`object_set_field_by_name_transition_only_fast_value`]
/// runs, because the caller has proved what that vet establishes: the
/// receiver is a live, unforwarded `GC_TYPE_OBJECT` of class 0 whose header
/// carries no frozen, sealed, non-extensible or descriptor flag, it was born
/// null-prototype and never relinked (so no chain can intercept the add), its
/// own list lacks `key`, and `key` is an interned heap string. `false`, having
/// changed nothing, on a cache miss or an edge the cache refuses.
///
/// # Safety
/// As above. Nothing between the caller's proof and this call allocates.
pub(crate) unsafe fn add_absent_key_to_null_proto_object(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> bool {
    let prev_shape_id = super::shapes::object_shape_stamp(obj);
    let Some(hit) = transition_cache_lookup(prev_shape_id, key) else {
        return false;
    };
    if hit.0.is_null() {
        return false;
    }
    let Some(edge) =
        super::constfn_key_add::admit_or_store(obj, prev_shape_id, hit, value.to_bits())
    else {
        return false;
    };
    let Some((next_keys, slot_idx, target_shape_id)) = edge.transition() else {
        return true;
    };
    if !super::shapes::install_cached_object_shape_transition(
        obj,
        prev_shape_id,
        target_shape_id,
        next_keys,
    ) {
        set_object_keys(obj, next_keys);
    }
    let live_slots = crate::object::object_live_slot_count(obj);
    let alloc_limit = std::cmp::max(live_slots, crate::object::INLINE_SLOT_FLOOR as u32) as usize;
    let vbits = value.to_bits();
    let vbits = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
        crate::value::TAG_UNDEFINED
    } else {
        vbits
    };
    if (slot_idx as usize) < alloc_limit {
        if slot_idx >= live_slots {
            set_object_live_slot_count(obj, slot_idx + 1);
        }
        store_object_field_slot(obj, slot_idx as usize, vbits);
    } else {
        overflow_set(obj as usize, slot_idx as usize, vbits);
    }
    true
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
            | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO;
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
            || crate::object::keys_find_property_slot_by_bytes(
                keys,
                shape.logical_key_count,
                key_bytes,
            )
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
        // The stable-tombstone list is private (not shape-shared, checked
        // above), so its header length is its count.
        set_object_keys(obj, crate::object::ObjectKeys::owned(new_keys));
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
        | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO;
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
    if !shape.object_kind.is_ordinary_layout() || shape.logical_key_count >= 16 {
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
        && crate::object::keys_find_property_slot_by_bytes(keys, shape.logical_key_count, key_bytes)
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

    let elements = crate::array::array_elements_ptr(keys as *const ArrayHeader) as *mut f64;
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
    object_set_field_by_name_transition_fast_impl_value::<false>(
        obj,
        key,
        value,
        try_overwrite,
        &mut None,
    )
    .is_some() as i32
}

/// Value-returning form (#9287): the returned f64 is re-read from this
/// function's OWN root after every internal allocation point (key interning,
/// spill growth), so a caller may invoke it with UNROOTED operands. The
/// pre-scope shortcut in `js_put_value_set_dyn_ic_miss` relies on that,
/// saving a RuntimeHandleScope + three roots on the hot
/// fresh-object-construction lane.
fn object_set_field_by_name_transition_fast_impl_value<const CHAIN_PROVEN: bool>(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
    try_overwrite: bool,
    refresh: &mut Option<(f64, f64, f64)>,
) -> Option<f64> {
    if key.is_null() || (key as usize) < 0x10000 {
        return None;
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
                return None;
            }
            let raw = (bits & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader;
            if raw.is_null() || crate::value::addr_class::is_small_handle(raw as usize) {
                return None;
            }
            raw
        } else {
            obj
        }
    };

    if obj.is_null() || (obj as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }

    if try_overwrite && unsafe { try_existing_own_data_overwrite(obj, key, value) } {
        // The overwrite scan allocates nothing, so the caller's value bits
        // are still live exactly as passed.
        return Some(value);
    }

    unsafe {
        let mut obj = obj;

        // Validated header probe (rejects the handle band, implausible
        // addresses, and slab allocations without touching memory) instead
        // of the bare floor + raw deref.
        let gc_header = match crate::value::addr_class::try_read_gc_header(obj as usize) {
            Some(h) => h as *const crate::gc::GcHeader,
            None => return None,
        };
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT
            || (*gc_header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return None;
        }
        let object_flags = (*gc_header)._reserved;
        if object_flags
            & (crate::gc::OBJ_FLAG_FROZEN
                | crate::gc::OBJ_FLAG_SEALED
                | crate::gc::OBJ_FLAG_NO_EXTEND
                | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO)
            != 0
        {
            return None;
        }
        // #6084 item 6: an own descriptor on THIS object (accessor or
        // non-writable) must route through the full setter semantics —
        // #10287: per KEY, so a receiver carrying one descriptor keeps the
        // transition lane for every other key. The append below targets a key
        // this shape does not have, so only an own descriptor recorded for
        // that absent key can matter.
        if object_flags & crate::gc::OBJ_FLAG_HAS_DESCRIPTORS != 0
            && !crate::object::own_descriptors_skip_key(
                obj as usize,
                f64::from_bits(JSValue::string_ptr(key as *mut _).bits()),
            )
        {
            return None;
        }
        // The header probe above already established a live ordinary heap
        // allocation; ask only its shape for the layout kind here.
        let object_kind =
            super::shapes::shape_object_kind_by_id(super::shapes::object_shape_stamp(obj))?;
        if !object_kind.is_ordinary_layout() || (*obj).class_id == NATIVE_MODULE_CLASS_ID {
            return None;
        }

        let key_gc =
            (key as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*key_gc).obj_type != crate::gc::GC_TYPE_STRING {
            return None;
        }
        let already_interned = (*key_gc).gc_flags & crate::gc::GC_FLAG_INTERNED != 0;
        // A chain verdict and a definition's lattice probe name interned
        // words. Reject anything else without touching the receiver, so the
        // semantic fallback retains interning and its roots. Specializing
        // this existing helper removes all optional-root work from that lane.
        if CHAIN_PROVEN && !already_interned {
            return None;
        }
        // A proven-chain append with an interned key does no JS or GC-heap work
        // until spill growth. Keep the same receiver/edge vet below, but create
        // handles only for chain inspection, interning or a collecting store.
        let scope = (!CHAIN_PROVEN).then(crate::gc::RuntimeHandleScope::new);
        let roots = scope.as_ref().map(|scope| {
            (
                scope.root_raw_mut_ptr(obj),
                scope.root_string_ptr(key),
                scope.root_nanbox_f64(value),
            )
        });

        // Closed source-level object literals are represented as synthetic
        // `__AnonShape_*` classes so their static fields can use the same
        // ShapeId machinery as class instances. Semantically they are still
        // plain objects: codegen registers their class ids at module init and
        // prototype/constructor dispatch already treats them as having
        // ordinary Object semantics. Admit exactly that registered population
        // alongside genuinely class-id-zero objects; a real user class must
        // retain the full inherited-setter/prototype walk.
        let class_id = (*obj).class_id;
        if !CHAIN_PROVEN && class_id != 0 && !crate::object::is_anon_shape_class_id(class_id) {
            return None;
        }

        // #6084 item 6: this used to be a `GLOBAL_DESCRIPTORS_IN_USE` check at
        // the top of the function — one `Object.freeze` anywhere in the process
        // (even on an unrelated object) permanently disabled this fast path for
        // every object. Vet the receiver's own flag (above) and its prototype
        // chain (here) instead. Pass semantic class id zero for an anon shape:
        // its nonzero runtime id is an implementation detail, not a JS class
        // whose vtable/prototype chain can carry instance accessors.
        // A chain-proven store skips this: the site's verdict IS this
        // question, answered for the receiver's real class chain rather than
        // the class-id-zero chain this call assumes.
        let key_f64 = f64::from_bits(JSValue::string_ptr(key as *mut _).bits());
        if !CHAIN_PROVEN && super::plain_data_write_may_intercept(obj as usize, 0, key_f64) {
            return None;
        }

        let key = roots
            .as_ref()
            .map_or(key, |(_, key, _)| key.get_raw_const_ptr());
        let interned_key = if already_interned {
            key
        } else {
            let hash = key_content_hash(key);
            crate::string::js_string_intern(key, hash)
        };
        if interned_key.is_null() {
            return None;
        }

        // Chain inspection or interning may have moved the operands. A
        // rootless miss collected nothing and leaves the caller's copies live.
        let (key, value) = if let Some((obj_root, key_root, value_root)) = &roots {
            obj = obj_root.get_raw_mut_ptr::<ObjectHeader>();
            let key = key_root.get_raw_const_ptr::<crate::StringHeader>();
            let value = value_root.get_nanbox_f64();
            *refresh = Some((
                crate::value::js_nanbox_pointer(obj as i64),
                crate::value::js_nanbox_string(key as i64),
                value,
            ));
            (key, value)
        } else {
            (key, value)
        };

        let prev_shape_id = super::shapes::object_shape_stamp(obj);
        let hit = transition_cache_lookup(prev_shape_id, interned_key)?;
        if hit.0.is_null() {
            return None;
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
            return None;
        }

        let edge =
            super::constfn_key_add::admit_or_store(obj, prev_shape_id, hit, value.to_bits())?;
        let Some((next_keys, slot_idx, target_shape_id, admitted_bits)) =
            edge.transition_slot_bits()
        else {
            return Some(value);
        };

        let cached_install = super::shapes::install_cached_object_shape_transition(
            obj,
            prev_shape_id,
            target_shape_id,
            next_keys,
        );
        if !cached_install {
            // A remint is not covered by the non-collecting cached stamp.
            // Decline without changing the receiver; the rooted miss owns it.
            if roots.is_none() {
                return None;
            }
            set_object_keys(obj, next_keys);
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
            // A default definition still publishes its key above. If birth
            // already filled this ordinary slot with undefined, its value
            // needs no second store, representation change, alias demotion or
            // barrier. Numeric-proof receivers retain the complete funnel.
            let fields = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
            if !(cached_install
                && object_kind == super::shapes::ShapeObjectKind::Ordinary
                && vbits == crate::value::TAG_UNDEFINED
                && *fields.add(slot_usize) == crate::value::TAG_UNDEFINED)
            {
                if cached_install && object_kind == super::shapes::ShapeObjectKind::Ordinary {
                    // An append preserves the ordinary kind. There is no
                    // numeric-prefix proof to retire, including after a rep
                    // generalization. T2 above already checked this exact
                    // target's representation and canonicalized numeric bits.
                    // Reuse its result with the same alias and mark barriers.
                    debug_assert_eq!(
                        super::shapes::shape_object_kind_by_id(super::shapes::object_shape_stamp(
                            obj
                        )),
                        Some(super::shapes::ShapeObjectKind::Ordinary)
                    );
                    let slot_bits = admitted_bits;
                    let slot_bits = if slot_bits == crate::value::POINTER_TAG {
                        crate::value::TAG_UNDEFINED
                    } else {
                        slot_bits
                    };
                    let _ = crate::gc::runtime_store_jsvalue_slot_layout_deferred(
                        obj as usize,
                        fields.add(slot_usize) as usize,
                        slot_usize,
                        slot_bits,
                    );
                } else {
                    store_object_field_slot(obj, slot_usize, vbits);
                }
            }
        } else if roots.is_some() {
            overflow_set(obj as usize, slot_usize, vbits);
        } else {
            // The successor is already authoritative. Protect its receiver
            // and the return value across the spill allocation; the key is
            // no longer read and is owned by that shape.
            let spill_scope = crate::gc::RuntimeHandleScope::new();
            let obj_root = spill_scope.root_raw_mut_ptr(obj);
            let value_root = spill_scope.root_nanbox_f64(value);
            overflow_set(
                obj_root.get_raw_mut_ptr::<ObjectHeader>() as usize,
                slot_usize,
                vbits,
            );
            #[cfg(test)]
            TEST_TRANSITION_FAST_HITS.with(|hits| hits.set(hits.get() + 1));
            return Some(value_root.get_nanbox_f64());
        }

        #[cfg(test)]
        TEST_TRANSITION_FAST_HITS.with(|hits| hits.set(hits.get() + 1));
        // Success: value may be a heap pointer that moved during interning or
        // spill growth; re-read it through this function's own root so the
        // caller needs none.
        return Some(
            roots
                .as_ref()
                .map_or(value, |(_, _, value)| value.get_nanbox_f64()),
        );
    }

    #[allow(unreachable_code)]
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cached_numeric_append_stores_canonical_bits_and_returns_the_input() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _ = crate::object::js_get_global_this();
        let scope = crate::gc::RuntimeHandleScope::new();
        unsafe {
            let name = b"canonical_append_number";
            let raw = scope.root_string_ptr(crate::string::js_string_from_bytes(
                name.as_ptr(),
                name.len() as u32,
            ));
            let key = scope.root_string_ptr(crate::string::js_string_intern(
                raw.get_raw_const_ptr(),
                key_content_hash(raw.get_raw_const_ptr()),
            ));
            let warm = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 1));
            crate::object::js_object_set_field_by_name(
                warm.get_raw_mut_ptr(),
                key.get_raw_const_ptr(),
                1.0,
            );
            assert_eq!(
                super::super::field_rep_store::object_slot_rep(warm.get_raw_mut_ptr(), 0),
                super::super::field_rep::REP_F64,
                "the Set warmup must establish a numeric transition"
            );
            test_reset_transition_fast_hits();
            for proven in [false, true] {
                for bits in [
                    crate::value::INT32_TAG | 17,
                    (-0.0_f64).to_bits(),
                    f64::NAN.to_bits(),
                ] {
                    let fresh = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 1));
                    let obj = fresh.get_raw_mut_ptr::<ObjectHeader>();
                    let value = f64::from_bits(bits);
                    let stored = if proven {
                        object_set_field_by_name_transition_chain_proven_value(
                            obj,
                            key.get_raw_const_ptr(),
                            value,
                            &mut None,
                        )
                    } else {
                        object_set_field_by_name_transition_fast_impl_value::<false>(
                            obj,
                            key.get_raw_const_ptr(),
                            value,
                            false,
                            &mut None,
                        )
                    }
                    .expect("the warmed numeric append must use its cached edge");
                    assert_eq!(
                        stored.to_bits(),
                        bits,
                        "the assignment returns the input value"
                    );
                    assert_eq!(
                        super::super::field_rep_store::object_slot_rep(obj, 0),
                        super::super::field_rep::REP_F64
                    );
                    let fields =
                        (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
                    assert_eq!(
                        *fields,
                        super::super::field_rep::f64_slot_bits(bits).unwrap()
                    );
                    assert_eq!(crate::object::object_keys(obj).count(), 1);
                }
            }
            assert_eq!(test_transition_fast_hits(), 6);
        }
    }

    #[test]
    fn cached_undefined_definition_publishes_the_key_and_clears_a_reserved_value() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        const CID: u32 = 0x0C12_3275;
        unsafe {
            crate::object::js_register_class_name(CID, b"UndefinedField".as_ptr(), 14);
            let raw =
                scope.root_string_ptr(crate::string::js_string_from_bytes(b"field".as_ptr(), 5));
            let key = scope.root_string_ptr(crate::string::js_string_intern(
                raw.get_raw_const_ptr(),
                key_content_hash(raw.get_raw_const_ptr()),
            ));
            let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
            let warm = scope.root_raw_mut_ptr(crate::object::js_object_alloc(CID, 1));
            crate::object::js_class_field_add(
                crate::value::js_nanbox_pointer(warm.get_raw_mut_ptr::<ObjectHeader>() as i64),
                crate::value::js_nanbox_string(
                    key.get_raw_const_ptr::<crate::StringHeader>() as i64
                ),
                undefined,
            );
            test_reset_transition_fast_hits();
            for old in [crate::value::TAG_UNDEFINED, 43.0_f64.to_bits()] {
                let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(CID, 1));
                // A reserved slot is not an own key. A prior internal value
                // must still be replaced when the definition first names it.
                store_object_field_slot(obj.get_raw_mut_ptr(), 0, old);
                assert_eq!(crate::object::object_keys(obj.get_raw_mut_ptr()).count(), 0);
                object_set_field_by_name_transition_chain_proven_value(
                    obj.get_raw_mut_ptr(),
                    key.get_raw_const_ptr(),
                    undefined,
                    &mut None,
                )
                .expect("the warmed definition must append through its cached edge");
                assert_eq!(crate::object::object_keys(obj.get_raw_mut_ptr()).count(), 1);
                assert_eq!(
                    crate::object::js_object_get_field_by_name(
                        obj.get_raw_mut_ptr(),
                        key.get_raw_const_ptr(),
                    )
                    .bits(),
                    crate::value::TAG_UNDEFINED
                );
            }
            assert_eq!(test_transition_fast_hits(), 2);
        }
    }

    #[test]
    fn a_cached_class_definition_keeps_pointer_values_inline_and_in_spill() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        const CID: u32 = 0x0C12_3274;
        unsafe {
            crate::object::js_register_class_name(CID, b"DeferredFields".as_ptr(), 14);
            let keys: Vec<_> = (0..crate::object::INLINE_SLOT_FLOOR + 2)
                .map(|i| {
                    let name = format!("deferred_pointer_{i}");
                    let raw = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                    let raw = scope.root_string_ptr(raw);
                    let key = crate::string::js_string_intern(
                        raw.get_raw_const_ptr(),
                        key_content_hash(raw.get_raw_const_ptr()),
                    );
                    scope.root_string_ptr(key)
                })
                .collect();
            let warm = scope.root_raw_mut_ptr(crate::object::js_object_alloc(CID, 0));
            for key_root in &keys {
                let child = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
                crate::object::js_class_field_add(
                    crate::value::js_nanbox_pointer(warm.get_raw_mut_ptr::<ObjectHeader>() as i64),
                    crate::value::js_nanbox_string(
                        key_root.get_raw_const_ptr::<crate::StringHeader>() as i64,
                    ),
                    crate::value::js_nanbox_pointer(child.get_raw_mut_ptr::<ObjectHeader>() as i64),
                );
            }
            let fresh = scope.root_raw_mut_ptr(crate::object::js_object_alloc(CID, 0));
            test_reset_transition_fast_hits();
            for key_root in &keys {
                let child = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
                let obj = fresh.get_raw_mut_ptr::<ObjectHeader>();
                let key = key_root.get_raw_const_ptr::<crate::StringHeader>();
                assert!(
                    transition_cache_lookup(super::super::shapes::object_shape_stamp(obj), key)
                        .is_some(),
                    "the warmed definition must have a live append edge"
                );
                let stored = object_set_field_by_name_transition_chain_proven_value(
                    obj,
                    key,
                    crate::value::js_nanbox_pointer(child.get_raw_mut_ptr::<ObjectHeader>() as i64),
                    &mut None,
                )
                .expect("cached inline and spill definitions must both be handled");
                let expected =
                    crate::value::js_nanbox_pointer(child.get_raw_mut_ptr::<ObjectHeader>() as i64);
                assert_eq!(stored.to_bits(), expected.to_bits());
                assert_eq!(
                    crate::object::js_object_get_field_by_name(
                        fresh.get_raw_mut_ptr::<ObjectHeader>(),
                        key_root.get_raw_const_ptr::<crate::StringHeader>()
                    )
                    .bits(),
                    expected.to_bits()
                );
            }
            assert_eq!(test_transition_fast_hits(), keys.len() as u64);
            assert_eq!(
                crate::object::object_live_slot_count(fresh.get_raw_mut_ptr::<ObjectHeader>()),
                crate::object::INLINE_SLOT_FLOOR as u32,
                "the last two definitions must use spill storage"
            );
            assert_eq!(
                crate::object::object_keys(fresh.get_raw_mut_ptr::<ObjectHeader>()).count(),
                keys.len() as u32
            );
        }
    }

    #[test]
    fn transition_fast_rejects_object_prototype_even_with_a_cached_edge() {
        let _lock = crate::gc::global_side_table_test_lock();
        // Establish the premise instead of assuming it. `Object.prototype` does
        // not exist until the realm global is built, and since #10836's fix
        // `object_prototype_addr()` correctly answers 0 rather than
        // materializing it as a side effect. Without this the test passed only
        // when some EARLIER test in the same process had built the global —
        // i.e. it passed in the full suite and failed run alone, which is a
        // pass that depends on test order rather than on the code under test.
        let _ = crate::object::js_get_global_this();
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
        let old_keys_view = unsafe { super::super::object_keys(prototype) };
        let old_keys = old_keys_view.arr();
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
