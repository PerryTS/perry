//! Map entry identity is proved from canonical storage; the actual receiver
//! brand selects the ordinary operation, with complete semantics on fallback.
use super::*;

#[inline(always)]
pub(super) fn plain_receiver(map: *const MapHeader) -> Option<*const MapHeader> {
    let map = map_receiver_identity(map);
    match unsafe { crate::value::addr_class::try_read_gc_header(map as usize) } {
        Some(header) if header.obj_type == crate::gc::GC_TYPE_MAP => Some(map),
        _ => None,
    }
}

/// Bit identity proves SameValueZero for a live, canonical stored key. A hole
/// must never be admitted against a tombstone before its normalization.
#[inline(always)]
pub(super) unsafe fn identical_entry(map: *const MapHeader, key: f64) -> Option<i32> {
    let used = (*map).used;
    let bits = key.to_bits();
    if used <= SIDE_TABLE_THRESHOLD && bits != MAP_HOLE_KEY_BITS {
        return find_identical_key(entries_ptr(map), used, bits);
    }
    None
}

#[inline(always)]
unsafe fn find_entry(map: *const MapHeader, key: f64) -> i32 {
    if let Some(index) = identical_entry(map, key) {
        return index;
    }
    find_entry_after_identity_miss(map, key)
}

/// The bit-identity pass of [`identical_entry`] already missed, so the small
/// extent is not scanned a second time: a plain non-zero number is then absent,
/// and every other key (content-equal strings, zero, NaN, the hole) goes
/// straight to the general comparison. Past the small extent the indexed
/// lookup owns the decision.
#[inline(always)]
unsafe fn find_entry_after_identity_miss(map: *const MapHeader, key: f64) -> i32 {
    let key = normalize_zero(key);
    if (*map).used <= SIDE_TABLE_THRESHOLD {
        if is_plain_nonzero_number_bits(key.to_bits()) {
            return -1;
        }
        return find_key_index_cold(map, key);
    }
    find_key_index(map, key)
}

#[inline(always)]
pub(super) fn set(map: *mut MapHeader, key: f64, value: f64) -> *mut MapHeader {
    if let Some(map) = plain_receiver(map) {
        return set_resolved(map as *mut MapHeader, key, value);
    }
    set_receiver_fallback(map, key, value)
}

#[inline(never)]
fn set_receiver_fallback(map: *mut MapHeader, key: f64, value: f64) -> *mut MapHeader {
    // Preserve the complete subclass receiver/root contract on the fallback.
    map_op_returning_receiver(map, |map| {
        set_resolved(map, key, value);
    })
}

#[inline(always)]
fn set_resolved(map: *mut MapHeader, key: f64, value: f64) -> *mut MapHeader {
    unsafe {
        let index = find_entry(map, key);
        if index >= 0 {
            let value_slot = entries_ptr_mut(map).add(index as usize * 2 + 1);
            // GC_STORE_AUDIT(BARRIERED): exact slot in the existing owned store.
            crate::gc::runtime_store_gc_jsvalue_slot(
                (*map).store as usize,
                value_slot as usize,
                value.to_bits(),
            );
        } else {
            append_resolved(map, normalize_zero(key), value);
        }
    }
    map
}

#[inline(never)]
pub(super) fn get_receiver_fallback(map: *const MapHeader, key: f64) -> f64 {
    let map = clean_map_ptr(map);
    if map.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    map_get_resolved(map, key)
}

/// Grow a full map with the pending key/value rooted across the grow.
#[inline(never)]
unsafe fn grow_rooted(map: *mut MapHeader, key: f64, value: f64) -> (f64, f64, bool) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let key_handle = scope.root_nanbox_f64(key);
    let value_handle = scope.root_nanbox_f64(value);
    let grew = ensure_capacity(map);
    (
        key_handle.get_nanbox_f64(),
        value_handle.get_nanbox_f64(),
        grew,
    )
}

/// The key is normalized and absent; caller has resolved/rooted the receiver.
#[inline(never)]
unsafe fn append_resolved(map: *mut MapHeader, key: f64, value: f64) {
    // Key doesn't exist, append a new entry. `ensure_capacity` can fire a
    // MOVING minor (via gc_note_external_side_alloc) — its "conservative +
    // non-moving" comment is false under evacuation. `key`/`value` are held
    // only in these native-stack params (a freshly-built, not-yet-inserted
    // object is reachable via nothing else), which an evacuating minor does
    // not scan, so root them across the grow and re-derive after. gh #6206.
    // Only a full map grows (and may move): the common append roots nothing.
    let (key, value, grew) = if (*map).used < (*map).capacity {
        (key, value, false)
    } else {
        grow_rooted(map, key, value)
    };
    let size = (*map).size;
    let used = (*map).used;
    let entries = entries_ptr_mut(map);
    if grew && used > 0 {
        crate::gc::replay_old_parent_slot_range_barriers(
            (*map).store as usize,
            entries as *mut u64,
            used as usize * 2,
        );
    }

    let key_slot = entries.add((used as usize) * 2);
    let value_slot = entries.add((used as usize) * 2 + 1);
    // GC_STORE_AUDIT(EXTERNAL_BARRIERED): map append key/value slots use the shared external-slot helper.
    crate::gc::runtime_store_gc_jsvalue_slot(
        (*map).store as usize,
        key_slot as usize,
        key.to_bits(),
    );
    crate::gc::runtime_store_gc_jsvalue_slot(
        (*map).store as usize,
        value_slot as usize,
        value.to_bits(),
    );

    (*map).size = size + 1;
    (*map).used = used + 1;
    (*(*map).store).used = (*map).used as usize;

    // Update the O(1) side-tables: numeric keys by bits, string keys by
    // content hash, pointer keys (objects/symbols/bigints) in the
    // GC-rebuilt pointer index (#6084).
    let key_bits = key.to_bits();
    if is_safe_numeric_key(key_bits) {
        if let Some(index) = (*map).store.as_mut().map(|store| &mut store.numeric) {
            index.insert(NumericKey(key_bits), used);
        }
    } else if is_string_like(key_bits) {
        // String key: content-hashed index bypasses the gen-GC stale-bits
        // constraint by storing entry indexes (not pointers) keyed by
        // Map-local word digest of the bytes.
        if let Some(h) = string_content_hash(key_bits) {
            (*(*map).store).strings.insert(h, used);
        }
    } else {
        let index = &mut (*(*map).store).pointers;
        let before = hash_index_bytes(index);
        index.insert(MapPtrKey(key), used);
        note_index_bytes_changed(before, hash_index_bytes(index));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_replacement_preserves_normalized_zero_nan_and_tombstones() {
        let map = js_map_alloc(8);
        js_map_set(map, -0.0, 1.0);
        js_map_set(map, 0.0, 2.0);
        assert_eq!(js_map_size(map), 1);
        assert_eq!(js_map_get(map, -0.0), 2.0);
        assert_eq!(js_map_get(map, 0.0), 2.0);
        for bits in [
            0x7ff0_0000_0000_0001,
            0xfff0_0000_0000_0001,
            0x7ff8_0000_0000_0011,
        ] {
            js_map_set(map, f64::from_bits(bits), 3.0);
            assert_eq!(js_map_get(map, f64::NAN), 3.0);
        }
        assert_eq!(js_map_size(map), 2);
        js_map_set(map, 1.0, 9.0);
        js_map_delete(map, 1.0);
        assert!(
            unsafe { (*map).used > (*map).size },
            "the tombstone must be live"
        );
        let hole = f64::from_bits(MAP_HOLE_KEY_BITS);
        js_map_set(map, hole, 7.0);
        assert_eq!(js_map_size(map), 3);
        assert_eq!(js_map_get(map, hole), 7.0);
        assert_eq!(js_map_get(map, f64::from_bits(TAG_UNDEFINED)), 7.0);
        js_map_set(map, f64::from_bits(TAG_UNDEFINED), 8.0);
        assert_eq!(js_map_get(map, hole), 8.0);
        assert_eq!(js_map_size(map), 3);
    }

    #[test]
    fn identity_and_content_replacement_preserve_original_key_and_order() {
        let a = crate::string::js_string_from_bytes(b"same".as_ptr(), 4);
        let b = crate::string::js_string_from_bytes(b"same".as_ptr(), 4);
        assert_ne!(a, b);
        let map = js_map_alloc(8);
        let key = boxed_heap_string_key(a);
        js_map_set(map, key, 1.0);
        js_map_set(map, 2.0, 2.0);
        js_map_set(map, key, 3.0);
        js_map_set(map, boxed_heap_string_key(b), 4.0);
        assert_eq!(js_map_size(map), 2);
        assert_eq!(
            unsafe { map_entry_key_raw(map, 0) }.to_bits(),
            key.to_bits()
        );
        assert_eq!(unsafe { map_entry_key_raw(map, 1) }, 2.0);
        assert_eq!(js_map_get(map, key), 4.0);
        assert_eq!(js_map_get(map, boxed_heap_string_key(b)), 4.0);
    }

    #[test]
    fn large_map_replacement_preserves_indexes_and_entry_order() {
        let map = js_map_alloc(32);
        for i in 0..32 {
            js_map_set(map, i as f64, i as f64);
        }
        for i in 0..32 {
            assert_eq!(js_map_set(map, i as f64, (100 + i) as f64), map);
        }
        assert_eq!(js_map_size(map), 32);
        for i in 0..32 {
            assert_eq!(js_map_get(map, i as f64), (100 + i) as f64);
            assert_eq!(unsafe { map_entry_key_raw(map, i as u32) }, i as f64);
        }
    }
}
