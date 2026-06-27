//! keys_array maintenance helpers shared by the descriptor-define paths:
//! `ensure_key_in_keys_array`, `install_builtin_getter`, `own_key_present`.
use super::super::*;
use super::*;

/// Ensure a key appears in the object's keys_array. Used by `Object.defineProperty`
/// so the property is enumerable-filterable and discoverable by `getOwnPropertyNames`
/// even when the value is undefined or the property is an accessor (no underlying slot).
#[allow(unused_assignments)]
pub(crate) unsafe fn ensure_key_in_keys_array(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
) {
    if obj.is_null() || (obj as usize) < 0x10000 || key.is_null() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let key_handle = scope.root_string_ptr(key);
    let mut obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
    let mut key = key_handle.get_raw_const_ptr::<crate::StringHeader>();
    macro_rules! refresh_define_property_roots {
        () => {{
            obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
            key = key_handle.get_raw_const_ptr::<crate::StringHeader>();
        }};
    }
    // If no keys array exists, create one with this key.
    let keys = (*obj).keys_array;
    if keys.is_null() {
        let new_keys = crate::array::js_array_alloc(4);
        refresh_define_property_roots!();
        let new_keys = crate::array::js_array_push(new_keys, JSValue::string_ptr(key as *mut _));
        refresh_define_property_roots!();
        set_object_keys_array(obj, new_keys);
        if (*obj).field_count == 0 {
            (*obj).field_count = 1;
        }
        return;
    }
    // Validate keys array pointer. The bare high-bits/low-address checks let
    // through values that are non-null and tag-free yet still not real heap
    // pointers (e.g. a stray `0x20_0000_0203` left in a miscompiled object's
    // keys_array slot), which then fault inside `js_array_length`'s GC-header
    // read. Gate on the arena-bounds predicate (same one `js_object_create`
    // uses for prototype validation) so a garbage slot is treated as "no keys
    // array" instead of crashing the process. (#321: defends against the
    // Effect `makeGenericTag` mis-tagged-receiver corruption.)
    let keys_ptr = keys as usize;
    if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 || !is_valid_obj_ptr(keys as *const u8) {
        return;
    }
    // Check if key already exists
    let key_count = crate::array::js_array_length(keys) as usize;
    for i in 0..key_count {
        let stored = crate::array::js_array_get(keys, i as u32);
        // #1781: SSO-aware match — pre-fix an existing inline-SSO key
        // wasn't seen here, so `Object.defineProperty(obj, "id", ...)`
        // on an object that already had `id` as an SSO key
        // double-inserted instead of overwriting.
        if crate::string::js_string_key_matches(stored, key) {
            return; // already present
        }
    }
    // Clone shared keys array if needed, then append.
    let owned_keys = if key_count == (*obj).field_count as usize {
        let cloned = crate::array::js_array_alloc(key_count as u32 + 4);
        refresh_define_property_roots!();
        let keys = (*obj).keys_array;
        let src_data = (keys as *const u8).add(8) as *const f64;
        let dst_data = (cloned as *mut u8).add(8) as *mut f64;
        for i in 0..key_count {
            // GC_STORE_AUDIT(INIT): cloned keys array is unpublished; layout is rebuilt before publication.
            *dst_data.add(i) = *src_data.add(i);
        }
        (*cloned).length = key_count as u32;
        super::super::rebuild_array_layout_from_slots(cloned);
        set_object_keys_array(obj, cloned);
        cloned
    } else {
        keys
    };
    let owned_keys_handle = scope.root_raw_mut_ptr(owned_keys);
    let new_keys = crate::array::js_array_push(owned_keys, JSValue::string_ptr(key as *mut _));
    let _owned_keys = owned_keys_handle.get_raw_mut_ptr::<ArrayHeader>();
    refresh_define_property_roots!();
    set_object_keys_array(obj, new_keys);
    // `field_count` is the inline/overflow boundary consulted by the read path
    // (`js_object_get_field`: index < field_count ⇒ read inline slot, else the
    // overflow map). It must never exceed the object's physically-allocated
    // inline capacity, which is `max(field_count, 8)` (see `js_object_alloc`).
    // Only bump it when this key genuinely lands in an in-bounds inline slot.
    //
    // A keys-only entry — a built-in accessor like `Map.prototype.size`, or a
    // key whose data spilled to the overflow map — must NOT push field_count
    // past the inline region. Doing so reclassifies already-overflowed (or
    // out-of-bounds) slots as inline, so later reads dereference past the
    // allocation into adjacent-heap garbage. That is what made
    // `Map.prototype.set` / `.values` read back as raw non-pointer values and
    // crash the reflective `.call` dispatch (#4099): installing the `size`
    // getter here bumped field_count from 8 (the proto's physical capacity) to
    // 11, exposing the overflowed `values` slot and corrupting the boundary.
    let new_index = key_count as u32;
    let inline_capacity = std::cmp::max((*obj).field_count, 8);
    if new_index < inline_capacity && new_index >= (*obj).field_count {
        (*obj).field_count = new_index + 1;
    }
}

/// Install a built-in *getter-only* accessor on a prototype object so that
/// `Object.getOwnPropertyDescriptor(proto, key)` reflects it as a real
/// accessor descriptor `{ get, set: undefined, enumerable, configurable }`.
///
/// `getter_bits` is the NaN-boxed `f64` bits of the getter closure (0 = none).
/// The descriptor is non-enumerable and configurable, matching the ECMA-262
/// shape for `%TypedArray%.prototype` accessors like `length` / `byteLength` /
/// `byteOffset` / `buffer`. Reflection-only: this does NOT flip the hot-path
/// descriptor gate (see `set_builtin_accessor_descriptor`). #2060.
pub(crate) unsafe fn install_builtin_getter(proto: *mut ObjectHeader, key: &str, getter_bits: u64) {
    if proto.is_null() || (proto as usize) < 0x10000 {
        return;
    }
    let key_str = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    if key_str.is_null() {
        return;
    }
    // Make the key discoverable by `own_key_present` / `getOwnPropertyNames`.
    ensure_key_in_keys_array(proto, key_str);
    // Spec: an accessor getter's `.name` is `"get " + key` (e.g.
    // `Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,"byteLength").get.name
    // === "get byteLength"`). Register it against the getter closure's func_ptr;
    // without this the `.name` read returned `""`.
    let getter_ptr = (getter_bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    if getter_ptr >= 0x1000 && crate::closure::is_closure_ptr(getter_ptr) {
        let func_ptr = (*(getter_ptr as *const crate::closure::ClosureHeader)).func_ptr as usize;
        crate::builtins::register_function_name_if_absent(func_ptr, &format!("get {key}"));
    }
    set_builtin_accessor_descriptor(
        proto as usize,
        key.to_string(),
        AccessorDescriptor {
            get: getter_bits,
            set: 0,
        },
        // writable is N/A for an accessor; enumerable=false, configurable=true.
        PropertyAttrs::new(true, false, true),
    );
}

/// Helper: does `key` appear in `obj.keys_array`?
pub(crate) unsafe fn own_key_present(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
) -> bool {
    // Every GC allocation is `align.max(8)`-aligned, so a real object pointer
    // has its low 3 bits clear. Rejecting misaligned `obj` keeps a non-object
    // value (e.g. a native-module namespace sentinel reaching `hasOwnProperty`
    // via a caller that didn't route through `extract_obj_ptr`) from being
    // dereferenced as an ObjectHeader. (#3527)
    if obj.is_null() || (obj as usize) < 0x10000 || (obj as usize) & 0x7 != 0 || key.is_null() {
        return false;
    }
    let keys = (*obj).keys_array;
    if keys.is_null() {
        return false;
    }
    let keys_ptr = keys as usize;
    // Same alignment invariant for the keys_array pointer: when `obj` is not a
    // genuine object its `keys_array` field holds garbage that may land in the
    // address range yet be misaligned. Without this guard the `[keys-8]`
    // GcHeader read below SIGBUSes on that garbage. (#3527)
    if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 || keys_ptr & 0x7 != 0 {
        return false;
    }
    // Validate keys_array GC header
    let keys_gc = (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*keys_gc).obj_type != crate::gc::GC_TYPE_ARRAY {
        return false;
    }
    let key_count = crate::array::js_array_length(keys) as usize;
    if key_count > 65536 {
        return false;
    }
    // #5736: a wide object — e.g. a barrel `export *` namespace with thousands
    // of re-exported bindings — made this an O(n) keys_array scan, so callers
    // that re-check every own key in a loop (`Object.values` / `Object.entries`,
    // which call `own_key_present` per key) ran O(n²). Probe the shared
    // wide-object key→slot index first: a hit is O(1). A miss falls through to
    // the linear scan below, so an absent key — or a present key whose index
    // entry was dropped as stale — is still answered correctly. The index is an
    // accelerator, never authoritative (it revalidates every hit against the
    // live slot via `js_string_key_matches`), and is the same map the read-path
    // getter maintains for these objects.
    if key_count >= super::super::field_get_set::WIDE_KEY_INDEX_MIN_KEYS {
        let key_bytes_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let key_len = (*key).byte_len as usize;
        let key_bytes = std::slice::from_raw_parts(key_bytes_ptr, key_len);
        if super::super::field_get_set::wide_key_index_lookup(
            keys as usize,
            key_bytes,
            key,
            keys,
            key_count,
        )
        .is_some()
        {
            return true;
        }
    }
    for i in 0..key_count {
        let stored = crate::array::js_array_get(keys, i as u32);
        // #1781: SSO-aware match — `hasOwnProperty("id")` previously
        // returned false when "id" lived as an inline SSO key.
        if crate::string::js_string_key_matches(stored, key) {
            return true;
        }
    }
    false
}
