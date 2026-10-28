//! Existing-entry operations keep numeric normalization and allocation frames
//! off the identity hit. The entry payload, not a mirrored key cache, proves it.
use super::*;

/// A bit-identical live entry is SameValueZero-equal. Normalization is needed
/// only when that proof fails: -0 and noncanonical NaNs then take the complete
/// path. A hole must never match an entry tombstone before normalization.
#[inline(always)]
pub(super) unsafe fn find_entry(map: *const MapHeader, key: f64) -> i32 {
    let used = (*map).used;
    let bits = key.to_bits();
    if used <= SIDE_TABLE_THRESHOLD && bits != MAP_HOLE_KEY_BITS {
        if let Some(index) = find_identical_key(entries_ptr(map), used, bits) {
            return index;
        }
    }
    find_key_index(map, normalize_zero(key))
}

/// Resolve the real brand once, then replace without an allocation-capable
/// insertion/receiver-root frame. The ordinary exact slot barrier is retained.
#[inline(always)]
pub(super) fn set(map: *mut MapHeader, key: f64, value: f64) -> *mut MapHeader {
    let receiver = map_receiver_identity_mut(map);
    let resolved = clean_map_ptr_mut(map);
    if resolved.is_null() {
        return receiver;
    }
    unsafe {
        let index = find_entry(resolved, key);
        if index >= 0 {
            let value_slot = entries_ptr_mut(resolved).add(index as usize * 2 + 1);
            // GC_STORE_AUDIT(BARRIERED): exact slot in the existing owned store.
            crate::gc::runtime_store_gc_jsvalue_slot(
                (*resolved).store as usize,
                value_slot as usize,
                value.to_bits(),
            );
            // Replacement does not collect. A subclass retains its identity
            // without opening the handle scope that an insertion can need.
            return receiver;
        }
    }
    insert_returning_receiver(receiver, resolved, normalize_zero(key), value)
}

#[inline(never)]
fn insert_returning_receiver(
    receiver: *mut MapHeader,
    resolved: *mut MapHeader,
    key: f64,
    value: f64,
) -> *mut MapHeader {
    if std::ptr::eq(resolved, receiver) {
        unsafe { append_resolved(resolved, key, value) };
        return receiver;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = scope.root_raw_mut_ptr(receiver);
    let ((), receiver) =
        handle.across_mut::<MapHeader, _>(|| unsafe { append_resolved(resolved, key, value) });
    receiver
}

/// The key is normalized and absent; caller has resolved/rooted the receiver.
unsafe fn append_resolved(map: *mut MapHeader, key: f64, value: f64) {
    // Key doesn't exist, append a new entry. `ensure_capacity` can fire a
    // MOVING minor (via gc_note_external_side_alloc) — its "conservative +
    // non-moving" comment is false under evacuation. `key`/`value` are held
    // only in these native-stack params (a freshly-built, not-yet-inserted
    // object is reachable via nothing else), which an evacuating minor does
    // not scan, so root them across the grow and re-derive after. gh #6206.
    let scope = crate::gc::RuntimeHandleScope::new();
    let key_handle = scope.root_nanbox_f64(key);
    let value_handle = scope.root_nanbox_f64(value);
    let grew = ensure_capacity(map);
    let key = key_handle.get_nanbox_f64();
    let value = value_handle.get_nanbox_f64();
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
