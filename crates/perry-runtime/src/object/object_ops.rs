//! `Object.*` static methods and descriptor machinery:
//! `Object.fromEntries`/`groupBy`/`is`/`hasOwn`/`create`/`freeze`/`seal`/
//! `defineProperty`/`getOwnPropertyDescriptor`/`getPrototypeOf`/... plus
//! the `js_object_*` helpers backing them.
//!
//! Split out of `object.rs` (issue #1103). Pure relocation. The
//! `globalThis` singleton subsystem stays in the parent module because
//! it is also consumed by class/builtin-resolution code there.

use super::*;

/// Object.fromEntries(entries) — build an object from an array of [key, value] pairs or a Map.
/// `entries` is an array of arrays, or a Map. Returns a NaN-boxed pointer to a new object.
#[no_mangle]
pub extern "C" fn js_object_from_entries(entries_value: f64) -> f64 {
    // Extract pointer from NaN-boxed value
    let bits = entries_value.to_bits();
    let raw_ptr = if (bits & 0xFFFF_0000_0000_0000) == 0x7FFD_0000_0000_0000 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as *const u8
    } else if bits != 0 && bits <= 0x0000_FFFF_FFFF_FFFF {
        bits as *const u8
    } else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    if raw_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

    unsafe {
        // Check GcHeader to see if this is a Map
        let gc_header = (raw_ptr).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type == crate::gc::GC_TYPE_MAP {
            // It's a Map — convert via js_map_entries first
            let map_ptr = raw_ptr as *const crate::map::MapHeader;
            let entries_arr = crate::map::js_map_entries(map_ptr);
            // Recursively call ourselves with the entries array (NaN-boxed pointer)
            let arr_boxed = crate::value::js_nanbox_pointer(entries_arr as i64);
            return js_object_from_entries(arr_boxed);
        }

        // It's an array of [key, value] pairs
        let arr_ptr = raw_ptr as *const ArrayHeader;
        let length = (*arr_ptr).length as usize;
        // Allocate empty object — class_id 0 = generic object
        let obj = js_object_alloc(0, length as u32);
        if obj.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        // Iterate entries: each entry is itself an array [key, value]
        let entries_data =
            (arr_ptr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        for i in 0..length {
            let entry_val = *entries_data.add(i);
            // Get the inner entry array
            let entry_bits = entry_val.to_bits();
            let entry_arr = if (entry_bits & 0xFFFF_0000_0000_0000) == 0x7FFD_0000_0000_0000 {
                (entry_bits & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader
            } else if entry_bits != 0 && entry_bits <= 0x0000_FFFF_FFFF_FFFF {
                entry_bits as *const ArrayHeader
            } else {
                continue;
            };
            if entry_arr.is_null() || (*entry_arr).length < 2 {
                continue;
            }
            let entry_data =
                (entry_arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
            let key_val = *entry_data;
            let val_val = *entry_data.add(1);
            // Convert key to string
            let key_str = crate::builtins::js_string_coerce(key_val);
            if key_str.is_null() {
                continue;
            }
            js_object_set_field_by_name(obj, key_str, val_val);
        }
        // Return as NaN-boxed pointer
        let bits = (obj as u64) | 0x7FFD_0000_0000_0000;
        f64::from_bits(bits)
    }
}

/// `Object.groupBy(items, callback)` — Node 22+ static method.
/// Walks `items` (an array), calls `callback(item, index)` to compute a
/// string key per item, and returns a new object whose keys are the
/// distinct callback results and whose values are arrays of the items
/// that produced each key.
///
/// `items_value` is the NaN-boxed array pointer; `callback` is the
/// closure to invoke per element. Returns the result object as a
/// NaN-boxed POINTER_TAG f64 so codegen can pass it through the normal
/// f64 plumbing.
#[no_mangle]
pub extern "C" fn js_object_group_by(
    items_value: f64,
    callback: *const crate::closure::ClosureHeader,
) -> f64 {
    // Strip NaN-box and validate the array pointer.
    let bits = items_value.to_bits();
    let raw = if (bits >> 48) == 0x7FFD {
        (bits & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader
    } else if bits != 0 && bits <= 0x0000_FFFF_FFFF_FFFF {
        bits as *const ArrayHeader
    } else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    if raw.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

    unsafe {
        let length = (*raw).length as usize;
        let elements = (raw as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        // Build a side table: key (UTF-8 String) -> Vec<f64> of group elements.
        // We materialize the result object only at the end so we don't have to
        // worry about per-push reallocation invalidating an array stored
        // inside the object's field slot.
        use std::collections::BTreeMap;
        let mut groups: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        // Preserve insertion order for the keys array (Node iterates the
        // result object in insertion order, not sorted order).
        let mut order: Vec<String> = Vec::new();

        for i in 0..length {
            let item = *elements.add(i);
            let key_val = crate::closure::js_closure_call2(callback, item, i as f64);
            // Coerce the key to a UTF-8 String.
            let key_ptr = crate::builtins::js_string_coerce(key_val);
            let key_string = if key_ptr.is_null() {
                "undefined".to_string()
            } else {
                let len = (*key_ptr).byte_len as usize;
                let data =
                    (key_ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
                let bytes = std::slice::from_raw_parts(data, len);
                std::str::from_utf8(bytes).unwrap_or("").to_string()
            };

            if !groups.contains_key(&key_string) {
                order.push(key_string.clone());
            }
            groups.entry(key_string).or_default().push(item);
        }

        // Materialize the result object. Allocate with the right field count
        // up front so the keys_array is sized correctly.
        let obj = js_object_alloc(0, order.len() as u32);
        if obj.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        for key in &order {
            // Build the JS string for the key.
            let key_str_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
            // Build the per-group Array<f64> from the materialized Vec.
            let items_for_key = groups.get(key).unwrap();
            let arr = crate::array::js_array_alloc(items_for_key.len() as u32);
            (*arr).length = items_for_key.len() as u32;
            let arr_data = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            for (i, v) in items_for_key.iter().enumerate() {
                std::ptr::write(arr_data.add(i), *v);
            }
            super::rebuild_array_layout_from_slots(arr);
            // NaN-box the array pointer with POINTER_TAG before storing.
            let arr_boxed = f64::from_bits((arr as u64) | 0x7FFD_0000_0000_0000);
            js_object_set_field_by_name(obj, key_str_ptr, arr_boxed);
        }
        // Return the result object NaN-boxed.
        f64::from_bits((obj as u64) | 0x7FFD_0000_0000_0000)
    }
}

/// Object.is(a, b) — SameValue algorithm
/// Like ===, except: NaN === NaN (true) and +0 !== -0 (false).
/// Returns NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_object_is(a: f64, b: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let a_bits = a.to_bits();
    let b_bits = b.to_bits();

    // Handle NaN: SameValue treats NaN as equal to NaN
    let a_jsval = crate::JSValue::from_bits(a_bits);
    let b_jsval = crate::JSValue::from_bits(b_bits);

    if a_jsval.is_number() && b_jsval.is_number() {
        let an = a_jsval.as_number();
        let bn = b_jsval.as_number();
        if an.is_nan() && bn.is_nan() {
            return f64::from_bits(TAG_TRUE);
        }
        // Distinguish +0 / -0 by bit pattern
        if an == 0.0 && bn == 0.0 {
            if a_bits == b_bits {
                return f64::from_bits(TAG_TRUE);
            }
            return f64::from_bits(TAG_FALSE);
        }
        if an == bn {
            return f64::from_bits(TAG_TRUE);
        }
        return f64::from_bits(TAG_FALSE);
    }

    // For strings, do content comparison
    if a_jsval.is_string() && b_jsval.is_string() {
        let result = crate::string::js_string_equals(
            a_jsval.as_string_ptr() as *const crate::StringHeader,
            b_jsval.as_string_ptr() as *const crate::StringHeader,
        );
        if result != 0 {
            return f64::from_bits(TAG_TRUE);
        }
        return f64::from_bits(TAG_FALSE);
    }

    // For everything else, bit-pattern equality
    if a_bits == b_bits {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

/// Object.hasOwn(obj, key) — check if obj has its own property `key`.
/// Returns NaN-boxed boolean. Checks via `keys_array` membership (not via
/// "value != undefined") so properties that legitimately hold `undefined` and
/// accessor descriptors with no backing slot still report true.
#[no_mangle]
pub extern "C" fn js_object_has_own(obj_value: f64, key_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    unsafe {
        // Symbol-keyed lookup: route through SYMBOL_PROPERTIES side table.
        // drizzle's `is(value, type)` checks `entityKind` which is a Symbol;
        // string-coercion would yield null and the check would always fail.
        // Refs #420.
        if crate::symbol::js_is_symbol(key_value) != 0 {
            // ClassRef receivers are NaN-boxed as INT32_TAG (top16 = 0x7FFE)
            // with the class_id in the low 32 bits. Consult the
            // class-static-symbol side table populated by
            // `js_class_register_static_symbol`. Refs #420 (drizzle's
            // `Object.prototype.hasOwnProperty.call(Table, entityKind)`).
            let bits = obj_value.to_bits();
            if (bits >> 48) == 0x7FFE {
                let class_id = (bits & 0xFFFF_FFFF) as u32;
                let present =
                    crate::symbol::class_static_symbol_lookup(class_id, key_value).is_some();
                return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
            }
            let present = crate::symbol::js_object_has_own_symbol(obj_value, key_value);
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) < 0x1000000 {
            return f64::from_bits(TAG_FALSE);
        }
        let key_str = crate::builtins::js_string_coerce(key_value);
        if key_str.is_null() {
            return f64::from_bits(TAG_FALSE);
        }
        if own_key_present(obj, key_str) {
            f64::from_bits(TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        }
    }
}

/// Helper: extract object pointer from NaN-boxed f64. Returns null on failure.
unsafe fn extract_obj_ptr(value: f64) -> *mut ObjectHeader {
    let jsval = crate::JSValue::from_bits(value.to_bits());
    if jsval.is_pointer() {
        jsval.as_pointer::<ObjectHeader>() as *mut ObjectHeader
    } else {
        let bits = value.to_bits();
        if bits != 0 && bits <= 0x0000_FFFF_FFFF_FFFF && bits > 0x10000 {
            bits as *mut ObjectHeader
        } else {
            ptr::null_mut()
        }
    }
}

/// Helper: get GcHeader for an object pointer
unsafe fn gc_header_for(obj: *const ObjectHeader) -> *mut crate::gc::GcHeader {
    (obj as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader
}

/// Object.defineProperty(obj, key, descriptor) — set the value AND record the
/// `writable` / `enumerable` / `configurable` attribute flags in the side table.
/// Returns the object (NaN-boxed pointer).
///
/// IMPORTANT: writes the value via `js_object_set_field_by_name` BEFORE recording
/// the descriptor — otherwise a `writable: false` descriptor would block its own
/// initial value from being stored.
#[no_mangle]
pub extern "C" fn js_object_define_property(
    obj_value: f64,
    key_value: f64,
    descriptor_value: f64,
) -> f64 {
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() {
            return obj_value;
        }
        // Extract key string
        let key_str = crate::builtins::js_string_coerce(key_value);
        if key_str.is_null() {
            return obj_value;
        }
        // Extract the key as a Rust string for the descriptor side-table lookup.
        let key_rust: Option<String> = {
            let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key_str).byte_len as usize;
            let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
            std::str::from_utf8(name_bytes).ok().map(|s| s.to_string())
        };
        // Extract descriptor object
        let desc_ptr = extract_obj_ptr(descriptor_value);
        if desc_ptr.is_null() {
            return obj_value;
        }

        // Detect accessor descriptor (has `get` and/or `set`) vs. data descriptor (has `value`).
        // JS disallows mixing them, but we only check for `get`/`set` presence.
        let get_key = crate::string::js_string_from_bytes(b"get".as_ptr(), 3);
        let set_key = crate::string::js_string_from_bytes(b"set".as_ptr(), 3);
        let get_field = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, get_key);
        let set_field = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, set_key);
        let has_accessor = !get_field.is_undefined() || !set_field.is_undefined();

        if has_accessor {
            // Store the accessor closures in the side table. Ensure the key is present
            // in the object's keys_array so lookups (hasOwn, getOwnPropertyDescriptor,
            // keys) can see it.
            ensure_key_in_keys_array(obj, key_str);
            if let Some(k) = key_rust.clone() {
                // Issue #450: spec says the getter/setter runs with `this === obj`
                // (the property access target). The user's descriptor literal
                // `{ get() {...}, set() {...} }` was lowered with `captures_this: true`
                // and had its reserved `this` slot patched to point to the *descriptor*
                // object at construction time — that's what every other object-literal
                // method does. Clone the closure once at defineProperty time and
                // rebind `this` to `obj`, so every subsequent get/set call sees the
                // correct receiver. Closures without CAPTURES_THIS_FLAG (e.g. arrow-form
                // `get: () => this._backing` written as a field rather than a method
                // shorthand) pass through unchanged.
                let recv_box = crate::value::js_nanbox_pointer(obj as i64);
                let get_bits = if get_field.is_undefined() {
                    0u64
                } else {
                    crate::closure::clone_closure_rebind_this(get_field.bits(), recv_box)
                };
                let set_bits = if set_field.is_undefined() {
                    0u64
                } else {
                    crate::closure::clone_closure_rebind_this(set_field.bits(), recv_box)
                };
                set_accessor_descriptor(
                    obj as usize,
                    k,
                    AccessorDescriptor {
                        get: get_bits,
                        set: set_bits,
                    },
                );
            }
        } else {
            // Data descriptor: look for "value" field and store it.
            let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
            let value_field =
                js_object_get_field_by_name(desc_ptr as *const ObjectHeader, value_key);
            // Clear any existing accessor for this key so the write doesn't fire the setter.
            if let Some(ref k) = key_rust {
                ACCESSOR_DESCRIPTORS.with(|m| {
                    m.borrow_mut().remove(&(obj as usize, k.clone()));
                });
            }
            // Ensure the key exists even if the descriptor's value is undefined —
            // the property still "exists" per JS semantics.
            if value_field.is_undefined() {
                ensure_key_in_keys_array(obj, key_str);
            } else {
                // Store via runtime path. Any existing descriptor attrs are NOT yet set,
                // so writability defaults to true and the write goes through.
                js_object_set_field_by_name(obj, key_str, f64::from_bits(value_field.bits()));
            }
        }

        // Read attribute flags from descriptor. JS defaults when omitted in
        // `Object.defineProperty` are `false` (NOT `true` like for direct assignment).
        let read_bool = |name: &[u8]| -> Option<bool> {
            let k = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            let v = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, k);
            if v.is_undefined() {
                None
            } else {
                Some(crate::value::js_is_truthy(f64::from_bits(v.bits())) != 0)
            }
        };
        // Accessor descriptors don't have `writable`; we leave it true so data
        // lookups that happen before the accessor override don't accidentally
        // reject a legitimate fallthrough write. Attrs default to false when
        // omitted (JS spec).
        let writable = read_bool(b"writable").unwrap_or(has_accessor);
        let enumerable = read_bool(b"enumerable").unwrap_or(false);
        let configurable = read_bool(b"configurable").unwrap_or(false);

        if let Some(k) = key_rust {
            set_property_attrs(
                obj as usize,
                k,
                PropertyAttrs::new(writable, enumerable, configurable),
            );
        }
        // Return the object
        obj_value
    }
}

/// Ensure a key appears in the object's keys_array. Used by `Object.defineProperty`
/// so the property is enumerable-filterable and discoverable by `getOwnPropertyNames`
/// even when the value is undefined or the property is an accessor (no underlying slot).
#[allow(unused_assignments)]
unsafe fn ensure_key_in_keys_array(obj: *mut ObjectHeader, key: *const crate::StringHeader) {
    if obj.is_null() || (obj as usize) < 0x1000000 || key.is_null() {
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
    // Validate keys array pointer
    let keys_ptr = keys as usize;
    if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
        return;
    }
    // Check if key already exists
    let key_count = crate::array::js_array_length(keys) as usize;
    for i in 0..key_count {
        let stored = crate::array::js_array_get(keys, i as u32);
        if stored.is_string() {
            let stored_key = stored.as_string_ptr();
            if crate::string::js_string_equals(key, stored_key) != 0 {
                return; // already present
            }
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
            *dst_data.add(i) = *src_data.add(i);
        }
        (*cloned).length = key_count as u32;
        super::rebuild_array_layout_from_slots(cloned);
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
    let new_index = key_count as u32;
    if new_index >= (*obj).field_count {
        (*obj).field_count = new_index + 1;
    }
}

/// Object.getOwnPropertyDescriptor(obj, key) — returns a data descriptor
/// `{ value, writable, enumerable, configurable }` for data properties, or an
/// accessor descriptor `{ get, set, enumerable, configurable }` for properties
/// installed via `Object.defineProperty(obj, key, { get, set })`. Returns
/// TAG_UNDEFINED if the property doesn't exist.
#[no_mangle]
pub extern "C" fn js_object_get_own_property_descriptor(obj_value: f64, key_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    unsafe {
        if let Some(class_id) = class_ref_id(obj_value) {
            let method_name = metadata_key_to_string(key_value);
            if let Some(method_name) = method_name {
                if method_name == "constructor" || class_has_own_method(class_id, &method_name) {
                    let value = if method_name == "constructor" {
                        obj_value
                    } else {
                        class_prototype_method_value_for_name(class_id, &method_name)
                    };
                    let packed = b"value\0writable\0enumerable\0configurable";
                    let desc = js_object_alloc_with_shape(
                        0x0D_E5_C2,
                        4,
                        packed.as_ptr(),
                        packed.len() as u32,
                    );
                    let header_size = std::mem::size_of::<ObjectHeader>();
                    let fields = (desc as *mut u8).add(header_size) as *mut f64;
                    *fields = value;
                    *fields.add(1) = f64::from_bits(TAG_TRUE);
                    *fields.add(2) = f64::from_bits(TAG_FALSE);
                    *fields.add(3) = f64::from_bits(TAG_TRUE);
                    super::rebuild_object_field_layout(desc, 4);
                    return f64::from_bits((desc as u64) | 0x7FFD_0000_0000_0000);
                }
            }
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }

        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        // Extract key string
        let key_str = crate::builtins::js_string_coerce(key_value);
        if key_str.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        // Extract key as a Rust string for descriptor lookup.
        let key_rust: Option<String> = {
            let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key_str).byte_len as usize;
            let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
            std::str::from_utf8(name_bytes).ok().map(|s| s.to_string())
        };

        // Check whether the key is actually present on the object. A property can
        // legitimately hold `undefined`, and accessor descriptors have no value slot,
        // so we check the keys_array directly instead of relying on "value != undefined".
        let present = own_key_present(obj, key_str);
        if !present {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }

        // Look up descriptor flags (default: all true).
        let attrs = key_rust
            .as_ref()
            .and_then(|k| get_property_attrs(obj as usize, k))
            .unwrap_or(PropertyAttrs::new(true, true, true));
        let bool_to_f64 = |b: bool| f64::from_bits(if b { TAG_TRUE } else { TAG_FALSE });

        // Accessor descriptor path.
        if let Some(acc) = key_rust
            .as_ref()
            .and_then(|k| get_accessor_descriptor(obj as usize, k))
        {
            let packed = b"get\0set\0enumerable\0configurable";
            let desc =
                js_object_alloc_with_shape(0x0D_E5_C1, 4, packed.as_ptr(), packed.len() as u32);
            let header_size = std::mem::size_of::<ObjectHeader>();
            let fields = (desc as *mut u8).add(header_size) as *mut f64;
            *fields = if acc.get != 0 {
                f64::from_bits(acc.get)
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            *fields.add(1) = if acc.set != 0 {
                f64::from_bits(acc.set)
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            *fields.add(2) = bool_to_f64(attrs.enumerable());
            *fields.add(3) = bool_to_f64(attrs.configurable());
            super::rebuild_object_field_layout(desc, 4);
            return f64::from_bits((desc as u64) | 0x7FFD_0000_0000_0000);
        }

        // Data descriptor path.
        let value = js_object_get_field_by_name(obj, key_str);
        let packed = b"value\0writable\0enumerable\0configurable";
        let desc = js_object_alloc_with_shape(
            0x0D_E5_C0, // unique shape_id for property descriptors
            4,
            packed.as_ptr(),
            packed.len() as u32,
        );
        let header_size = std::mem::size_of::<ObjectHeader>();
        let fields = (desc as *mut u8).add(header_size) as *mut f64;
        *fields = f64::from_bits(value.bits()); // value
        *fields.add(1) = bool_to_f64(attrs.writable()); // writable
        *fields.add(2) = bool_to_f64(attrs.enumerable()); // enumerable
        *fields.add(3) = bool_to_f64(attrs.configurable()); // configurable
        super::rebuild_object_field_layout(desc, 4);
        f64::from_bits((desc as u64) | 0x7FFD_0000_0000_0000)
    }
}

/// Helper: does `key` appear in `obj.keys_array`?
unsafe fn own_key_present(obj: *mut ObjectHeader, key: *const crate::StringHeader) -> bool {
    if obj.is_null() || (obj as usize) < 0x1000000 || key.is_null() {
        return false;
    }
    let keys = (*obj).keys_array;
    if keys.is_null() {
        return false;
    }
    let keys_ptr = keys as usize;
    if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
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
    for i in 0..key_count {
        let stored = crate::array::js_array_get(keys, i as u32);
        if stored.is_string() {
            let stored_key = stored.as_string_ptr();
            if !stored_key.is_null() && crate::string::js_string_equals(key, stored_key) != 0 {
                return true;
            }
        }
    }
    false
}

/// Issue #620: returns the OWN-property value at `name` if one exists in the
/// receiver's own keys_array (a string-keyed data property), otherwise
/// returns TAG_UNDEFINED. Used by class-method dispatch to detect override
/// patterns like `this.method = X` (hono's SmartRouter.match rebinds itself
/// on first call). Distinct from `js_object_get_field_by_name` because it
/// does NOT walk the class vtable's getter chain — we only want a raw own
/// data-property read, not a side-effecting getter invocation.
#[no_mangle]
pub extern "C" fn js_object_get_own_field_or_undef(
    obj_value: f64,
    name_ptr: *const u8,
    name_len: usize,
) -> f64 {
    const TAG_UNDEF: u64 = 0x7FFC_0000_0000_0001;
    if name_ptr.is_null() || name_len == 0 {
        return f64::from_bits(TAG_UNDEF);
    }
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) < 0x1000000 {
            return f64::from_bits(TAG_UNDEF);
        }
        if (obj as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
            return f64::from_bits(TAG_UNDEF);
        }
        if !is_valid_obj_ptr(obj as *const u8) {
            return f64::from_bits(TAG_UNDEF);
        }
        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
            return f64::from_bits(TAG_UNDEF);
        }
        // Skip closures sharing the GC_TYPE_OBJECT slot (CLOSURE_MAGIC at +12).
        let type_tag_at_12 = *((obj as *const u8).add(12) as *const u32);
        if type_tag_at_12 == crate::closure::CLOSURE_MAGIC {
            return f64::from_bits(TAG_UNDEF);
        }
        let keys = (*obj).keys_array;
        if keys.is_null() {
            return f64::from_bits(TAG_UNDEF);
        }
        let keys_ptr = keys as usize;
        if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
            return f64::from_bits(TAG_UNDEF);
        }
        let keys_gc =
            (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*keys_gc).obj_type != crate::gc::GC_TYPE_ARRAY {
            return f64::from_bits(TAG_UNDEF);
        }
        let key_bytes = std::slice::from_raw_parts(name_ptr, name_len);
        let key_count = crate::array::js_array_length(keys) as usize;
        if key_count > 65536 {
            return f64::from_bits(TAG_UNDEF);
        }
        let alloc_limit = std::cmp::max((*obj).field_count, 8) as usize;
        for i in 0..key_count {
            let key_val = crate::array::js_array_get(keys, i as u32);
            if key_val.is_string() {
                let stored_key = key_val.as_string_ptr();
                if !stored_key.is_null() {
                    let stored_data =
                        (stored_key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let stored_len = (*stored_key).byte_len as usize;
                    let stored_bytes = std::slice::from_raw_parts(stored_data, stored_len);
                    if stored_bytes == key_bytes {
                        let val = if i < alloc_limit {
                            js_object_get_field(obj, i as u32)
                        } else {
                            match overflow_get(obj as usize, i) {
                                Some(bits) => crate::JSValue::from_bits(bits),
                                None => return f64::from_bits(TAG_UNDEF),
                            }
                        };
                        return f64::from_bits(val.bits());
                    }
                }
            }
        }
        f64::from_bits(TAG_UNDEF)
    }
}

/// Look up the canonical NaN-boxed value of a built-in constructor /
/// namespace stored on `globalThis` (the singleton populated by
/// `populate_global_this_builtins`). Used by `instance.constructor`
/// reads and by bare `Date`/`Array`/`Object` identifier resolution so
/// both forms produce the same closure-pointer value — that's what
/// `instance.constructor === Date` (date-fns's `constructFrom`,
/// drizzle's `is(value, ctor)` duck checks, ...) hinges on.
///
/// Returns NaN-boxed undefined if the name isn't one of the populated
/// built-ins or the singleton hasn't been initialized yet.
#[no_mangle]
pub extern "C" fn js_get_global_this_builtin_value(name_ptr: *const u8, name_len: usize) -> f64 {
    if name_ptr.is_null() || name_len == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let name_bytes = unsafe { std::slice::from_raw_parts(name_ptr, name_len) };
    let name = match std::str::from_utf8(name_bytes) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(crate::value::TAG_UNDEFINED),
    };
    // Force the singleton init the first time so the lookup below has
    // a populated field bag.
    let global_this_f64 = js_get_global_this();
    let global_obj = crate::value::js_nanbox_get_pointer(global_this_f64) as *const ObjectHeader;
    if global_obj.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = js_object_get_field_by_name(global_obj, key);
    let bits = value.bits();
    f64::from_bits(bits)
}

/// Object.getOwnPropertyNames(obj) — returns all own property names (including non-enumerable).
/// Takes a NaN-boxed f64 object pointer, returns a NaN-boxed f64 array pointer.
#[no_mangle]
pub extern "C" fn js_object_get_own_property_names(obj_value: f64) -> f64 {
    unsafe {
        if let Some(class_id) = class_ref_id(obj_value) {
            let mut names: Vec<String> = vec!["constructor".to_string()];
            if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                if let Some(reg) = registry.as_ref() {
                    if let Some(vtable) = reg.get(&class_id) {
                        let mut methods: Vec<String> = vtable.methods.keys().cloned().collect();
                        methods.sort();
                        names.extend(methods);
                    }
                }
            }
            let result = crate::array::js_array_alloc(names.len() as u32);
            for name in names {
                let str_ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                crate::array::js_array_push(result, JSValue::string_ptr(str_ptr));
            }
            return f64::from_bits((result as u64) | 0x7FFD_0000_0000_0000);
        }

        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() {
            let empty = crate::array::js_array_alloc(0);
            return f64::from_bits((empty as u64) | 0x7FFD_0000_0000_0000);
        }
        let keys = (*obj).keys_array;
        if keys.is_null() {
            let empty = crate::array::js_array_alloc(0);
            return f64::from_bits((empty as u64) | 0x7FFD_0000_0000_0000);
        }
        // Clone the keys array — Object.getOwnPropertyNames includes ALL keys (even non-enumerable).
        let len = crate::array::js_array_length(keys) as usize;
        let result = crate::array::js_array_alloc(len as u32);
        for i in 0..len {
            let key_val = crate::array::js_array_get(keys, i as u32);
            crate::array::js_array_push_f64(result, f64::from_bits(key_val.bits()));
        }
        f64::from_bits((result as u64) | 0x7FFD_0000_0000_0000)
    }
}

/// Object.create(proto) — create empty object. Perry ignores prototype; Object.create(null) returns {}.
#[no_mangle]
pub extern "C" fn js_object_create(proto_value: f64) -> f64 {
    // #809: actually wire up the prototype. Pre-fix this ignored its
    // argument entirely, so `Object.create(Proto)` returned a bare empty
    // object — `inst.method()` / `inst.prop` saw nothing and threw
    // `TypeError: <m> is not a function`. Reuse the #711 prototype-object
    // machinery: allocate a synthetic class_id, map it to `proto` in
    // CLASS_PROTOTYPE_OBJECTS, and stamp the new object with that id. The
    // chain walk in `js_object_get_field_by_name` (the `class_id != 0`
    // branch) then resolves missing own props/methods off `proto`.
    //
    // `Object.create(null)` (or a non-object proto / a builtin-backed
    // Set/Map/Regex source Perry can't model as a prototype) falls back
    // to the original behavior: a plain prototype-less object.
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    let mut class_id: u32 = 0;
    let proto_bits = proto_value.to_bits();
    if (proto_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG {
        let proto_ptr = crate::value::js_nanbox_get_pointer(proto_value) as *mut ObjectHeader;
        if !proto_ptr.is_null() && (proto_ptr as usize) > 0x10000 {
            let proto_addr = proto_ptr as usize;
            let modellable = !(crate::set::is_registered_set(proto_addr)
                || crate::map::is_registered_map(proto_addr)
                || crate::regex::is_regex_pointer(proto_ptr as *const u8));
            let valid = modellable && is_valid_obj_ptr(proto_ptr as *const u8);
            if valid {
                let cid =
                    NEXT_SYNTHETIC_CLASS_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                {
                    let mut write = CLASS_PROTOTYPE_OBJECTS.write().unwrap();
                    if write.is_none() {
                        *write = Some(HashMap::new());
                    }
                    write.as_mut().unwrap().insert(cid, proto_ptr as usize);
                }
                unsafe { js_register_class_id(cid) };
                class_id = cid;
            }
        }
    }
    let obj = js_object_alloc(class_id, 0);
    // Return NaN-boxed pointer
    f64::from_bits((obj as u64) | 0x7FFD_0000_0000_0000)
}

/// Object.freeze(obj) — sets the frozen flag and drops `writable` +
/// `configurable` on every existing key so per-key descriptor lookups report
/// the post-freeze state. Returns the object.
#[no_mangle]
pub extern "C" fn js_object_freeze(obj_value: f64) -> f64 {
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if !obj.is_null() && (obj as usize) > 0x10000 {
            let gc = gc_header_for(obj);
            (*gc)._reserved |= crate::gc::OBJ_FLAG_FROZEN
                | crate::gc::OBJ_FLAG_SEALED
                | crate::gc::OBJ_FLAG_NO_EXTEND;
            // Drop writable + configurable for every existing key.
            mark_all_keys(
                obj, /*drop_writable=*/ true, false, /*drop_configurable=*/ true,
            );
        }
    }
    obj_value
}

/// Object.seal(obj) — sets the sealed flag and drops `configurable` on every
/// existing key. Writable is preserved (sealed ≠ frozen). Returns the object.
#[no_mangle]
pub extern "C" fn js_object_seal(obj_value: f64) -> f64 {
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if !obj.is_null() && (obj as usize) > 0x10000 {
            let gc = gc_header_for(obj);
            (*gc)._reserved |= crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND;
            // Drop configurable for every existing key (but leave writable intact).
            mark_all_keys(
                obj, /*drop_writable=*/ false, false, /*drop_configurable=*/ true,
            );
        }
    }
    obj_value
}

/// Object.preventExtensions(obj) — sets the no-extend flag. Returns the object.
#[no_mangle]
pub extern "C" fn js_object_prevent_extensions(obj_value: f64) -> f64 {
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if !obj.is_null() && (obj as usize) > 0x10000 {
            let gc = gc_header_for(obj);
            (*gc)._reserved |= crate::gc::OBJ_FLAG_NO_EXTEND;
        }
    }
    obj_value
}

/// Object.isFrozen(obj) — returns NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_object_is_frozen(obj_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) <= 0x10000 {
            return f64::from_bits(TAG_TRUE); // non-objects are vacuously frozen
        }
        let gc = gc_header_for(obj);
        if (*gc)._reserved & crate::gc::OBJ_FLAG_FROZEN != 0 {
            f64::from_bits(TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        }
    }
}

/// Object.isSealed(obj) — returns NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_object_is_sealed(obj_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) <= 0x10000 {
            return f64::from_bits(TAG_TRUE); // non-objects are vacuously sealed
        }
        let gc = gc_header_for(obj);
        if (*gc)._reserved & crate::gc::OBJ_FLAG_SEALED != 0 {
            f64::from_bits(TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        }
    }
}

/// Object.isExtensible(obj) — returns NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_object_is_extensible(obj_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) <= 0x10000 {
            return f64::from_bits(TAG_FALSE); // non-objects are not extensible
        }
        let gc = gc_header_for(obj);
        if (*gc)._reserved & crate::gc::OBJ_FLAG_NO_EXTEND != 0 {
            f64::from_bits(TAG_FALSE)
        } else {
            f64::from_bits(TAG_TRUE)
        }
    }
}

/// Object.getPrototypeOf(obj):
/// - For an INT32-tagged class ref (top16 == 0x7FFE) — return the parent
///   class ref via CLASS_REGISTRY's parent_class_id chain, or null at
///   the root. Drizzle's `is(value, type)` chain walks this.
/// - For an object instance with a registered class_id — return the
///   class ref. Conceptually JS returns `Class.prototype`; Perry doesn't
///   maintain prototype objects, but drizzle's chain consumes
///   `Object.getPrototypeOf(value).constructor`, and class_ref's
///   `.constructor` synthesizes back to the same class ref via the
///   constructor intercept (v0.5.746). So returning the class ref here
///   makes that chain produce `value.constructor` as Node would.
/// - Other receivers — null.
/// Refs #420 / #618 followup.
#[no_mangle]
pub extern "C" fn js_object_get_prototype_of(obj_value: f64) -> f64 {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let bits = obj_value.to_bits();
    let top16 = bits >> 48;
    if top16 == 0x7FFE {
        let class_id = (bits & 0xFFFF_FFFF) as u32;
        if let Some(parent_id) = get_parent_class_id(class_id) {
            if parent_id != 0 {
                let parent_bits = 0x7FFE_0000_0000_0000u64 | (parent_id as u64);
                return f64::from_bits(parent_bits);
            }
        }
        return f64::from_bits(TAG_NULL);
    }
    // Heap-pointer receiver — return the input value itself. For
    // class-id-tagged instances, `.constructor` then returns the class
    // ref (via the constructor intercept in js_object_get_field_by_name,
    // v0.5.746), making `getPrototypeOf(v).constructor === v.constructor`.
    // For object literals / arrays / other non-class-tagged heap values,
    // `.constructor` returns undefined, which collapses drizzle's
    // `if (cls)` chain to false safely (instead of throwing on
    // `null.constructor` if we returned null). Drizzle's
    // `is(value, type)` chain calls this on every chunk including
    // arrays of values, so the array case is load-bearing.
    //
    // Two NaN-shapes cover the heap-pointer case:
    //  - top16 == 0x7FFD: NaN-boxed POINTER_TAG (typical function-local).
    //  - top16 == 0x0000 with raw_addr large enough: module-level object
    //    literals get stored as raw I64 pointers (no NaN-boxing) per the
    //    "Module-level variables" note in CLAUDE.md, so we accept that
    //    form here too.
    if top16 == 0x7FFD {
        let raw_addr = bits & 0x0000_FFFF_FFFF_FFFF;
        if raw_addr != 0 && raw_addr >= (crate::gc::GC_HEADER_SIZE as u64) + 0x1000 {
            return obj_value;
        }
    }
    if top16 == 0 {
        if bits >= (crate::gc::GC_HEADER_SIZE as u64) + 0x1000 {
            return obj_value;
        }
    }
    f64::from_bits(TAG_NULL)
}

/// `Object.defineProperties(target, descriptors)` — iterate the descriptor
/// object's own keys and invoke `js_object_define_property` for each one.
/// Used by chalk's `Object.defineProperties(createChalk.prototype, styles)`
/// where `styles` is built via `Object.create(null)` + dynamic assignment,
/// so the static `Object(...)` literal desugar in the HIR lowering can't
/// fire and we fall here.
///
/// Returns the target. Spec also returns target — Perry's lowering relies
/// on that so `const x = Object.defineProperties(...)` still binds `x`.
#[no_mangle]
pub extern "C" fn js_object_define_properties(target: f64, descriptors: f64) -> f64 {
    let desc_obj = unsafe { extract_obj_ptr(descriptors) };
    if desc_obj.is_null() || !is_valid_obj_ptr(desc_obj as *const u8) {
        return target;
    }
    // Snapshot the descriptor object's own keys array. We collect into a
    // Vec<f64> first so adding properties via `js_object_define_property`
    // (which can resize the target's keys_array) can't perturb iteration
    // — descriptors and target are usually different objects, but a
    // defensive copy costs ~ngc and protects against a user who passes
    // `Object.defineProperties(obj, obj)` aliasing.
    let key_array_ptr: *const crate::array::ArrayHeader = unsafe { (*desc_obj).keys_array };
    if key_array_ptr.is_null() {
        return target;
    }
    let len = unsafe { crate::array::js_array_length(key_array_ptr) } as usize;
    let mut keys: Vec<f64> = Vec::with_capacity(len);
    for i in 0..len {
        let k = unsafe { crate::array::js_array_get(key_array_ptr, i as u32) };
        keys.push(f64::from_bits(k.bits()));
    }
    for k in keys {
        let descriptor = unsafe {
            js_object_get_field_by_name_f64(desc_obj as *const ObjectHeader, str_from_value(k))
        };
        if descriptor.to_bits() == TAG_UNDEFINED_LOCAL {
            continue;
        }
        js_object_define_property(target, k, descriptor);
    }
    target
}

const TAG_UNDEFINED_LOCAL: u64 = 0x7FFC_0000_0000_0001;

/// Coerce an arbitrary key value (f64 — usually a STRING_TAG NaN-box) to a
/// `*const StringHeader` for use with `js_object_get_field_by_name_f64`.
/// Returns null if the value isn't string-like.
fn str_from_value(v: f64) -> *const crate::string::StringHeader {
    let bits = v.to_bits();
    let top = bits >> 48;
    if top == 0x7FFF {
        (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::string::StringHeader
    } else {
        // Try to coerce (handles number keys, etc.).
        crate::builtins::js_string_coerce(v) as *const crate::string::StringHeader
    }
}

/// `Object.setPrototypeOf(obj, proto)` — chalk's callable-with-getter-bag
/// foundation. Perry's runtime bakes class IDs at allocation time (it
/// walks `parent_class_id` for INT32-tagged class refs), so we cannot
/// mutate an existing object's prototype chain in a fully observable
/// way. What we *can* do is satisfy the spec's "return target" contract
/// so callers like
///
/// ```text
/// const chalk = (...s) => s.join(' ');
/// Object.setPrototypeOf(chalk, Foo.prototype);
/// ```
///
/// don't crash with `TypeError: value is not a function` (which is what
/// the generic `(Object).setPrototypeOf(...)` PropertyGet → Call fallback
/// used to produce — the property lookup returned undefined and the call
/// dispatched a non-callable). chalk's module init invokes this exact
/// pattern; ms / express decorate functions with `Object.assign` instead,
/// which is already a fast path.
///
/// Pragmatically: today this returns the target and otherwise no-ops.
/// chalk's getters on `createChalk.prototype` won't actually fire under
/// Perry, but the rest of the program keeps running and chalk's
/// call-without-properties form (the most common usage) keeps working.
/// A future change can register the (obj → proto) mapping in a
/// thread-local side-table so a downstream `Object.getPrototypeOf(obj)`
/// + inherited property dispatch can consult it.
#[no_mangle]
pub extern "C" fn js_object_set_prototype_of(obj_value: f64, _proto: f64) -> f64 {
    // Spec: `Object.setPrototypeOf(O, proto)` returns O. We deliberately
    // do nothing else — see the function doc above for the trade-off.
    obj_value
}

/// Issue #100: build a module-namespace object (the value an `await
/// import("./foo.ts")` resolves to) from parallel arrays of keys and
/// values.
///
/// Keys are length-prefixed UTF-8 (Perry strings are not guaranteed
/// null-terminated), passed as parallel `*const *const u8` (data
/// pointers) and `*const i32` (byte lengths). Values are the already
/// NaN-boxed `f64` representations passed as a flat `f64` array.
///
/// The returned f64 is a NaN-boxed POINTER_TAG `ObjectHeader` with its
/// `keys_array` populated so `Object.keys(ns)`/iteration and property
/// dispatch work the same as on any other JS object. Caller is
/// responsible for pinning the object as a GC root if it stores the
/// result in a long-lived slot — codegen does this by writing the
/// result into the module-scoped `__perry_ns_<prefix>` global which is
/// already registered with `js_gc_register_global_root`.
///
/// Empty namespace (`n == 0`) returns a fresh empty object.
///
/// Returns an `f64` directly (not `JSValue`) so the LLVM ABI signature
/// `double js_create_namespace(...)` declared in `runtime_decls.rs`
/// matches: NaN-boxed values use float-register-return on AArch64 /
/// SysV-x86_64. A `JSValue` return would route through integer
/// registers (`#[repr(transparent)]` over `u64`) and the call site's
/// `%xmm0` read would observe stale bits.
#[no_mangle]
pub extern "C" fn js_create_namespace(
    n: i32,
    keys: *const *const u8,
    key_lens: *const i32,
    values: *const f64,
) -> f64 {
    let count = if n < 0 { 0 } else { n as usize };
    unsafe {
        // Allocate a plain object with `count` inline slots. class_id 0
        // is the generic-object class used by Object.create / {} / URL.
        let obj = js_object_alloc(0, count as u32);
        if obj.is_null() {
            // Fallback to undefined — should never happen but defensive.
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj_handle = scope.root_raw_mut_ptr(obj);

        // Initialize an empty keys array so `js_object_set_field_by_name`
        // can append to it. Pre-populating the keys array AND calling
        // set_field_by_name would double every key — the property
        // setter's "add key to keys_array" step runs unconditionally.
        let keys_arr = crate::array::js_array_alloc(0);
        let mut obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
        js_object_set_keys(obj, keys_arr);

        // Set each (key, value) pair on the object. We route through
        // `js_object_set_field_by_name` so the standard property-write
        // path (inline-slot allocation, shape transitions, accessor
        // dispatch) handles everything. This matches how user-written
        // `obj.k = v` and `js_object_assign_one` populate objects, so
        // downstream reads (PropertyGet PIC, Object.keys, JSON.stringify)
        // all work without special-casing the namespace shape.
        for i in 0..count {
            let key_data = *keys.add(i);
            let key_len = *key_lens.add(i);
            let key_len_u = if key_len < 0 { 0u32 } else { key_len as u32 };
            // Use the heap StringHeader path so the property machinery
            // (which expects a real `StringHeader*`) gets a valid
            // pointer. Pre-SSO-only would crash on >7-byte export names.
            let key_hdr = crate::string::js_string_from_bytes(key_data, key_len_u);
            obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
            let val = *values.add(i);
            js_object_set_field_by_name(obj, key_hdr, val);
        }

        // NaN-box POINTER_TAG and return.
        obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
        let bits = (obj as u64) | 0x7FFD_0000_0000_0000;
        f64::from_bits(bits)
    }
}
