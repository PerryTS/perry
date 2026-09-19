//! Indexed WeakMap operations, also shared by WeakSet.
use super::*;

/// TEMPORARY diagnostic for #10660 -- guarded by PERRY_DEBUG_WEAKMAP=1, prints
/// to stderr whenever a WeakMap/WeakSet lookup misses, distinguishing a
/// zero-pointer receiver (map itself isn't a real weak collection at this
/// call site) from a genuine "key not present" miss. Remove before landing.
fn debug_weakmap_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("PERRY_DEBUG_WEAKMAP").is_ok())
}

fn debug_weakmap_seq() -> u64 {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

fn debug_weakmap_miss(op: &str, map: f64, key: f64, reason: &str) {
    if !debug_weakmap_enabled() {
        return;
    }
    let map_ptr = js_nanbox_get_pointer(map);
    let key_ptr = js_nanbox_get_pointer(key);
    let map_class = if map_ptr != 0 {
        crate::object::js_object_get_class_id(map_ptr as *mut ObjectHeader)
    } else {
        0
    };
    let key_class = if key_ptr != 0 {
        crate::object::js_object_get_class_id(key_ptr as *mut ObjectHeader)
    } else {
        0
    };
    let (entries_len, entry_keys) = if map_ptr != 0 {
        unsafe {
            let entries = entries_array(map_ptr as *mut ObjectHeader);
            if entries.is_null() {
                (-1i64, String::new())
            } else {
                let len = crate::array::js_array_length(entries);
                let mut keys = String::new();
                for slot in 0..len.min(16) {
                    let entry = weak_entry_at(entries, slot as usize);
                    if entry.is_null() {
                        keys.push_str("null,");
                    } else {
                        let kbits = object_field_bits(entry, WEAK_ENTRY_KEY_FIELD);
                        keys.push_str(&format!("{:#x},", kbits));
                    }
                }
                (len as i64, keys)
            }
        }
    } else {
        (-2i64, String::new())
    };
    let seq = debug_weakmap_seq();
    eprintln!(
        "[weakmap-debug] seq={seq} {op} MISS reason={reason} map_bits={:#x} map_ptr={:#x} map_class_id={map_class} key_bits={:#x} key_ptr={:#x} key_class_id={key_class} entries_len={entries_len} entry_keys=[{entry_keys}]",
        map.to_bits(), map_ptr, key.to_bits(), key_ptr
    );
}

fn debug_weakmap_set(map: f64, key: f64) {
    if !debug_weakmap_enabled() {
        return;
    }
    let map_ptr = js_nanbox_get_pointer(map);
    let key_ptr = js_nanbox_get_pointer(key);
    let key_class = if key_ptr != 0 {
        crate::object::js_object_get_class_id(key_ptr as *mut ObjectHeader)
    } else {
        0
    };
    let seq = debug_weakmap_seq();
    eprintln!(
        "[weakmap-debug] seq={seq} SET map_bits={:#x} map_ptr={:#x} key_bits={:#x} key_ptr={:#x} key_class_id={key_class}",
        map.to_bits(), map_ptr, key.to_bits(), key_ptr
    );
}

fn debug_weakmap_hit(op: &str, map: f64, key: f64) {
    if !debug_weakmap_enabled() {
        return;
    }
    let map_ptr = js_nanbox_get_pointer(map);
    let key_ptr = js_nanbox_get_pointer(key);
    let seq = debug_weakmap_seq();
    eprintln!(
        "[weakmap-debug] seq={seq} {op} HIT map_bits={:#x} map_ptr={:#x} key_bits={:#x} key_ptr={:#x}",
        map.to_bits(), map_ptr, key.to_bits(), key_ptr
    );
}

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
    debug_weakmap_set(map, key);
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
        let (entries, entry) =
            entry.across_mut::<ObjectHeader, _>(|| entries_array(map_pointer(map)));
        let entry_value = f64::from_bits(JSValue::pointer(entry as *const u8).bits());
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
        debug_weakmap_miss("get", map, key, "map_pointer_zero");
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
            debug_weakmap_hit("get", map.get_nanbox_f64(), key.get_nanbox_f64());
            return read_barrier::weak_read_barrier_f64(object_field_bits(
                entry,
                WEAK_ENTRY_VALUE_FIELD,
            ));
        }
    }
    debug_weakmap_miss("get", map.get_nanbox_f64(), key.get_nanbox_f64(), "key_not_found");
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_weakmap_has(map: f64, key: f64) -> f64 {
    if let Some(v) = crate::object::delegate_if_not_weak_collection(map, "has", &[key]) {
        if debug_weakmap_enabled() {
            eprintln!("[weakmap-debug] has DELEGATED (not a weak collection) map_bits={:#x}", map.to_bits());
        }
        return v;
    }
    if js_nanbox_get_pointer(map) == 0 {
        debug_weakmap_miss("has", map, key, "map_pointer_zero");
        return f64::from_bits(TAG_FALSE);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let map = scope.root_nanbox_f64(map);
    let key = scope.root_nanbox_f64(key);
    unsafe {
        if let Some(entry) = find_entry(map, key) {
            read_barrier::weak_read_barrier(object_field_bits(entry, WEAK_ENTRY_KEY_FIELD));
            debug_weakmap_hit("has", map.get_nanbox_f64(), key.get_nanbox_f64());
            return f64::from_bits(TAG_TRUE);
        }
    }
    debug_weakmap_miss("has", map.get_nanbox_f64(), key.get_nanbox_f64(), "key_not_found");
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
