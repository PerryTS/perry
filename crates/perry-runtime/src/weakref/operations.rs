//! Indexed WeakMap operations, also shared by WeakSet.
use super::*;

#[inline]
fn map_pointer(map: crate::gc::RuntimeHandle<'_>) -> *mut ObjectHeader {
    js_nanbox_get_pointer(map.get_nanbox_f64()) as *mut ObjectHeader
}

/// The by-name entries lookup on a subclass can allocate. Re-read both roots
/// afterwards, and never keep a cache borrow across a collecting operation.
unsafe fn find_entry(
    map: crate::gc::RuntimeHandle<'_>,
    key: crate::gc::RuntimeHandle<'_>,
) -> Option<*mut ObjectHeader> {
    let entries = entries_array(map_pointer(map));
    if entries.is_null() {
        return None;
    }
    let slot = index::find(map_pointer(map), entries, key.get_nanbox_f64().to_bits())?;
    Some(weak_entry_at(entries, slot as usize))
}

#[no_mangle]
pub extern "C" fn js_weakmap_set(map: f64, key: f64, value: f64) -> f64 {
    // #7948: name-based HIR folds can reach these helpers for foreign objects.
    if let Some(v) = crate::object::delegate_if_not_weak_collection(map, "set", &[key, value]) {
        return v;
    }
    if !is_valid_weak_target(key) {
        throw_invalid_weakmap_key();
    }
    if js_nanbox_get_pointer(map) == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let map = scope.root_nanbox_f64(map);
    let key = scope.root_nanbox_f64(key);
    let value = scope.root_nanbox_f64(value);
    unsafe {
        let entries = entries_array(map_pointer(map));
        if entries.is_null() {
            return f64::from_bits(TAG_UNDEFINED);
        }
        let owner = map_pointer(map);
        if let Some(slot) = index::find(owner, entries, key.get_nanbox_f64().to_bits()) {
            let entry = weak_entry_at(entries, slot as usize);
            // #7154: overwriting an old entry must remember its young value.
            js_object_set_field(
                entry,
                WEAK_ENTRY_VALUE_FIELD as u32,
                JSValue::from_bits(value.get_nanbox_f64().to_bits()),
            );
            return map.get_nanbox_f64();
        }
        let free = index::take_free(owner, entries);
        let slot = free.unwrap_or_else(|| js_array_length(entries));
        let entry = weak_entry_new(key.get_nanbox_f64(), value.get_nanbox_f64());
        let entry = scope.root_raw_mut_ptr(entry);
        let entries = entries_array(map_pointer(map));
        let entry_value = f64::from_bits(
            JSValue::pointer(entry.get_raw_mut_ptr::<ObjectHeader>() as *const u8).bits(),
        );
        if free.is_some() {
            js_array_set_f64(entries, slot, entry_value);
        } else {
            let grown = js_array_push_f64(entries, entry_value);
            js_object_set_field(map_pointer(map), 0, JSValue::array_ptr(grown));
        }
        // All allocation is finished. A collection may have discarded the
        // index, moved the owner/key, and tombstoned other weak entries.
        let entries = entries_array(map_pointer(map));
        index::inserted(
            map_pointer(map),
            entries,
            key.get_nanbox_f64().to_bits(),
            slot,
        );
    }
    map.get_nanbox_f64()
}

#[no_mangle]
pub extern "C" fn js_weakmap_get(map: f64, key: f64) -> f64 {
    if let Some(v) = crate::object::delegate_if_not_weak_collection(map, "get", &[key]) {
        return v;
    }
    if js_nanbox_get_pointer(map) == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let map = scope.root_nanbox_f64(map);
    let key = scope.root_nanbox_f64(key);
    unsafe {
        if let Some(entry) = find_entry(map, key) {
            // #7900: keep the key alive through pending weak slices and shade
            // the value handed back to compiled code.
            read_barrier::weak_read_barrier(object_field_bits(entry, WEAK_ENTRY_KEY_FIELD));
            return read_barrier::weak_read_barrier_f64(object_field_bits(
                entry,
                WEAK_ENTRY_VALUE_FIELD,
            ));
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_weakmap_has(map: f64, key: f64) -> f64 {
    if let Some(v) = crate::object::delegate_if_not_weak_collection(map, "has", &[key]) {
        return v;
    }
    if js_nanbox_get_pointer(map) == 0 {
        return f64::from_bits(TAG_FALSE);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let map = scope.root_nanbox_f64(map);
    let key = scope.root_nanbox_f64(key);
    unsafe {
        if let Some(entry) = find_entry(map, key) {
            read_barrier::weak_read_barrier(object_field_bits(entry, WEAK_ENTRY_KEY_FIELD));
            return f64::from_bits(TAG_TRUE);
        }
    }
    f64::from_bits(TAG_FALSE)
}

#[no_mangle]
pub extern "C" fn js_weakmap_delete(map: f64, key: f64) -> f64 {
    if let Some(v) = crate::object::delegate_if_not_weak_collection(map, "delete", &[key]) {
        return v;
    }
    if js_nanbox_get_pointer(map) == 0 {
        return f64::from_bits(TAG_FALSE);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let map = scope.root_nanbox_f64(map);
    let key = scope.root_nanbox_f64(key);
    unsafe {
        let entries = entries_array(map_pointer(map));
        if entries.is_null() {
            return f64::from_bits(TAG_FALSE);
        }
        let owner = map_pointer(map);
        let key = key.get_nanbox_f64().to_bits();
        if let Some(slot) = index::find(owner, entries, key) {
            let entry = weak_entry_at(entries, slot as usize);
            // Clearing introduces no pointer and cannot collect, like the
            // GC's tombstone write. Keep offsets stable instead of compacting
            // the entire entries array on every delete.
            write_object_field_bits_raw(entry, WEAK_ENTRY_KEY_FIELD, TAG_UNDEFINED);
            write_object_field_bits_raw(entry, WEAK_ENTRY_VALUE_FIELD, TAG_UNDEFINED);
            index::deleted(owner, entries, key, slot);
            return f64::from_bits(TAG_TRUE);
        }
    }
    f64::from_bits(TAG_FALSE)
}
