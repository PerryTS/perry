//! Object freeze / seal / extensibility helpers, extracted from
//! `object_ops.rs` to keep that file under the 2k-line limit. These map the
//! `Object.freeze` / `Object.seal` / `Object.preventExtensions` family (and
//! their `is*` predicates) onto the per-object GC-header reserved-flag bits.

use super::*;

/// Drop `writable`/`configurable` on every own **symbol-keyed** property of
/// `obj`. The string-keyed table is handled by `mark_all_keys`; symbol props
/// live in a separate side table, so `Object.freeze`/`Object.seal` must walk
/// it too (else a frozen object's symbol props stay writable and strict writes
/// to them wrongly succeed).
unsafe fn mark_all_symbol_keys(
    obj: *mut ObjectHeader,
    drop_writable: bool,
    drop_configurable: bool,
) {
    let owner = obj as usize;
    for (sym_ptr, _) in crate::symbol::clone_symbol_entries_for_obj_ptr(owner) {
        let mut attrs = crate::symbol::get_symbol_property_attrs(owner, sym_ptr)
            .unwrap_or_else(|| PropertyAttrs::new(true, true, true));
        if drop_writable {
            attrs.bits &= !PropertyAttrs::WRITABLE;
        }
        if drop_configurable {
            attrs.bits &= !PropertyAttrs::CONFIGURABLE;
        }
        crate::symbol::set_symbol_property_attrs(owner, sym_ptr, attrs);
    }
}

/// Build a partial property descriptor object for `SetIntegrityLevel`:
/// `{ configurable: false }` (sealed, or a frozen accessor) or
/// `{ configurable: false, writable: false }` (a frozen data property).
unsafe fn build_integrity_descriptor(set_writable_false: bool) -> f64 {
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let obj = js_object_alloc(0, 2);
    let false_v = f64::from_bits(TAG_FALSE);
    let ck = crate::string::js_string_from_bytes(b"configurable".as_ptr(), 12);
    js_object_set_field_by_name(obj, ck, false_v);
    if set_writable_false {
        let wk = crate::string::js_string_from_bytes(b"writable".as_ptr(), 8);
        js_object_set_field_by_name(obj, wk, false_v);
    }
    crate::value::js_nanbox_pointer(obj as i64)
}

/// `SetIntegrityLevel(O, level)` (ECMA-262 7.3.16) for a Proxy receiver. A
/// Proxy is a small registered id, not a heap object — `extract_obj_ptr` yields
/// the fake pointer and `gc_header_for` would deref unmapped memory (the
/// `Object.seal`/`Object.freeze` proxy crash cluster). Route through the proxy
/// `[[PreventExtensions]]`, `[[OwnPropertyKeys]]`, `[[GetOwnProperty]]` and
/// `[[DefineOwnProperty]]` traps. Returns the proxy; throws a `TypeError` when a
/// trap reports failure (`PreventExtensions` false ⇒ `SetIntegrityLevel` false
/// ⇒ `Object.seal`/`Object.freeze` throw).
unsafe fn set_integrity_level_proxy(obj_value: f64, frozen: bool) -> f64 {
    let ok = crate::proxy::js_reflect_prevent_extensions(obj_value);
    if crate::value::js_is_truthy(ok) == 0 {
        throw_object_type_error(
            b"Cannot set integrity level: 'preventExtensions' trap returned falsish",
        );
    }
    let keys = crate::proxy::js_proxy_own_keys(obj_value);
    let keys_ptr = extract_obj_ptr(keys) as *const crate::array::ArrayHeader;
    if keys_ptr.is_null() {
        return obj_value;
    }
    let len = crate::array::js_array_length(keys_ptr);
    for i in 0..len {
        let k = crate::array::js_array_get_f64(keys_ptr, i);
        let desc = if frozen {
            let cur = crate::proxy::js_reflect_get_own_property_descriptor(obj_value, k);
            if crate::value::JSValue::from_bits(cur.to_bits()).is_undefined() {
                continue;
            }
            let is_accessor = desc_has_field(cur, b"get") || desc_has_field(cur, b"set");
            build_integrity_descriptor(!is_accessor)
        } else {
            build_integrity_descriptor(false)
        };
        // DefinePropertyOrThrow: js_object_define_property routes the proxy
        // through the `[[DefineOwnProperty]]` trap and throws if it reports
        // failure, propagating the abrupt completion just like the spec.
        js_object_define_property(obj_value, k, desc);
    }
    obj_value
}

/// Truthiness of a boolean descriptor field (`configurable`/`writable`).
unsafe fn desc_field_true(desc: f64, name: &[u8]) -> bool {
    let v = desc_read_field(desc, name);
    crate::value::js_is_truthy(f64::from_bits(v.bits())) != 0
}

/// `TestIntegrityLevel(O, level)` (ECMA-262 7.3.15) for a Proxy receiver. Drives
/// the `[[IsExtensible]]`, `[[OwnPropertyKeys]]` and `[[GetOwnProperty]]` traps
/// in spec order (test262 `isSealed`/`isFrozen` `proxy-no-ownkeys-returned-keys-order`
/// asserts each key returned by `ownKeys` is queried via `getOwnPropertyDescriptor`).
unsafe fn test_integrity_level_proxy(obj_value: f64, frozen: bool) -> bool {
    let ext = crate::proxy::js_reflect_is_extensible(obj_value);
    if crate::value::js_is_truthy(ext) != 0 {
        return false;
    }
    let keys = crate::proxy::js_proxy_own_keys(obj_value);
    let keys_ptr = extract_obj_ptr(keys) as *const crate::array::ArrayHeader;
    if keys_ptr.is_null() {
        return true;
    }
    let len = crate::array::js_array_length(keys_ptr);
    for i in 0..len {
        let k = crate::array::js_array_get_f64(keys_ptr, i);
        let cur = crate::proxy::js_reflect_get_own_property_descriptor(obj_value, k);
        if crate::value::JSValue::from_bits(cur.to_bits()).is_undefined() {
            continue;
        }
        if desc_field_true(cur, b"configurable") {
            return false;
        }
        if frozen {
            let is_data = desc_has_field(cur, b"value") || desc_has_field(cur, b"writable");
            if is_data && desc_field_true(cur, b"writable") {
                return false;
            }
        }
    }
    true
}

#[no_mangle]
pub extern "C" fn js_object_freeze(obj_value: f64) -> f64 {
    if crate::proxy::js_proxy_is_proxy(obj_value) != 0 {
        return unsafe {
            set_integrity_level_proxy(obj_value, /*frozen=*/ true)
        };
    }
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        // Reject the WHOLE handle band, not a bare `> 0x10000` floor: a
        // common-band registry id (crypto `Hash`, `Blob`, …) can sit above
        // 0x10000, and the `gc_header_for(obj)` write just below would store into
        // unmapped memory (SIGSEGV) — `Object.freeze(handle)` is a no-op that
        // returns the handle (test_gap_handle_band_object_ops `Object.freeze(blob)`).
        if !obj.is_null() && crate::value::addr_class::is_above_handle_band(obj as usize) {
            let gc = gc_header_for(obj);
            (*gc)._reserved |= crate::gc::OBJ_FLAG_FROZEN
                | crate::gc::OBJ_FLAG_SEALED
                | crate::gc::OBJ_FLAG_NO_EXTEND;
            // TypedArray receivers are NOT `ObjectHeader`s — the key walk
            // below would read a garbage `keys_array` off the TA header and
            // can fault depending on heap layout. The GC flags above are the
            // entire integrity state for them.
            if crate::typedarray::lookup_typed_array_kind(obj as usize).is_some()
                || crate::typedarray_props::typed_array_addr_from_value(obj_value).is_some()
            {
                return obj_value;
            }
            // Map / Set instances are NOT plain `ObjectHeader`s: their entries
            // live in a dedicated backing registered in the map/set tables, and
            // the `keys_array` slot the walk below reads is not a real key array.
            // `mark_all_keys` would dereference that garbage and corrupt the
            // backing — after `Object.freeze(map)`, a later `map.get(k)` /
            // `map.values()` then read a bad pointer and SIGSEGV (mime's
            // `_freeze()` froze its `#typeToExtensions` Map-of-Sets). Per spec,
            // freezing a Map/Set only makes the object non-extensible; its
            // entries stay mutable through the collection methods. So set the
            // integrity GC flags (already done above) and stop — same shape as
            // the TypedArray arm.
            if crate::map::is_registered_map(obj as usize)
                || crate::set::is_registered_set(obj as usize)
            {
                return obj_value;
            }
            // Closures: own props are `name`/`length` + dynamic props — the
            // keys_array walk below would read garbage off the ClosureHeader.
            // Record explicit non-writable/non-configurable attrs.
            if crate::closure::is_closure_ptr(obj as usize) {
                let owner = obj as usize;
                for builtin in ["name", "length"] {
                    super::set_property_attrs(
                        owner,
                        builtin.to_string(),
                        PropertyAttrs::new(false, false, false),
                    );
                }
                for (name, _) in crate::closure::closure_dynamic_props_snapshot(owner) {
                    let cur = super::get_property_attrs(owner, &name)
                        .unwrap_or(PropertyAttrs::new(true, true, true));
                    super::set_property_attrs(
                        owner,
                        name,
                        PropertyAttrs::new(false, cur.enumerable(), false),
                    );
                }
                mark_all_symbol_keys(
                    obj, /*drop_writable=*/ true, /*drop_configurable=*/ true,
                );
                return obj_value;
            }
            // Date / RegExp / Error exotic instances: their own props live in
            // the side tables, so freeze them there instead of the key walk.
            if let Some(kind) = super::exotic_expando::exotic_expando_kind(obj as usize) {
                for key in super::exotic_expando::exotic_own_keys(kind, obj as usize, false) {
                    let cur = super::get_property_attrs(obj as usize, &key)
                        .unwrap_or(PropertyAttrs::new(true, true, true));
                    super::set_property_attrs(
                        obj as usize,
                        key,
                        PropertyAttrs::new(false, cur.enumerable(), false),
                    );
                }
                return obj_value;
            }
            // Arrays store indices densely and named props in a side table —
            // neither is in `keys_array`, so handle them explicitly.
            if super::mark_all_array_props(
                obj, /*drop_writable=*/ true, /*drop_configurable=*/ true,
            ) {
                mark_all_symbol_keys(
                    obj, /*drop_writable=*/ true, /*drop_configurable=*/ true,
                );
                return obj_value;
            }
            // Drop writable + configurable for every existing key.
            mark_all_keys(
                obj, /*drop_writable=*/ true, false, /*drop_configurable=*/ true,
            );
            mark_all_symbol_keys(
                obj, /*drop_writable=*/ true, /*drop_configurable=*/ true,
            );
        }
    }
    obj_value
}

/// Object.seal(obj) — sets the sealed flag and drops `configurable` on every
/// existing key. Writable is preserved (sealed ≠ frozen). Returns the object.
#[no_mangle]
pub extern "C" fn js_object_seal(obj_value: f64) -> f64 {
    if crate::proxy::js_proxy_is_proxy(obj_value) != 0 {
        return unsafe {
            set_integrity_level_proxy(obj_value, /*frozen=*/ false)
        };
    }
    // A TypedArray is NOT an ObjectHeader: reading `keys_array` off its
    // header yields garbage that `mark_all_keys` then dereferences
    // (`Object.seal(new Uint32Array())` segfaulted on Linux). Seal on a TA
    // only needs the no-extend/sealed flags — integer-indexed elements are
    // exotic and never configurable through this table.
    if crate::typedarray_props::typed_array_addr_from_value(obj_value).is_some() {
        unsafe {
            let obj = extract_obj_ptr(obj_value);
            if !obj.is_null() && (obj as usize) > 0x10000 {
                let gc = gc_header_for(obj);
                (*gc)._reserved |= crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND;
            }
        }
        return obj_value;
    }
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if !obj.is_null() && (obj as usize) > 0x10000 {
            let gc = gc_header_for(obj);
            (*gc)._reserved |= crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND;
            // TypedArray receivers: GC flags only — see `js_object_freeze`.
            if crate::typedarray::lookup_typed_array_kind(obj as usize).is_some()
                || crate::typedarray_props::typed_array_addr_from_value(obj_value).is_some()
            {
                return obj_value;
            }
            // Map / Set: GC integrity flags only — the `keys_array` walk would
            // corrupt their backing. See the matching arm in `js_object_freeze`.
            if crate::map::is_registered_map(obj as usize)
                || crate::set::is_registered_set(obj as usize)
            {
                return obj_value;
            }
            // Closures: seal via the side tables (drop configurable only) —
            // see the matching arm in `js_object_freeze`.
            if crate::closure::is_closure_ptr(obj as usize) {
                let owner = obj as usize;
                for builtin in ["name", "length"] {
                    let cur = super::get_property_attrs(owner, builtin)
                        .unwrap_or(PropertyAttrs::new(false, false, true));
                    super::set_property_attrs(
                        owner,
                        builtin.to_string(),
                        PropertyAttrs::new(cur.writable(), cur.enumerable(), false),
                    );
                }
                for (name, _) in crate::closure::closure_dynamic_props_snapshot(owner) {
                    let cur = super::get_property_attrs(owner, &name)
                        .unwrap_or(PropertyAttrs::new(true, true, true));
                    super::set_property_attrs(
                        owner,
                        name,
                        PropertyAttrs::new(cur.writable(), cur.enumerable(), false),
                    );
                }
                mark_all_symbol_keys(
                    obj, /*drop_writable=*/ false, /*drop_configurable=*/ true,
                );
                return obj_value;
            }
            // Date / RegExp / Error exotic instances: seal via the side
            // tables (drop configurable only) — see `js_object_freeze`.
            if let Some(kind) = super::exotic_expando::exotic_expando_kind(obj as usize) {
                for key in super::exotic_expando::exotic_own_keys(kind, obj as usize, false) {
                    let cur = super::get_property_attrs(obj as usize, &key)
                        .unwrap_or(PropertyAttrs::new(true, true, true));
                    super::set_property_attrs(
                        obj as usize,
                        key,
                        PropertyAttrs::new(cur.writable(), cur.enumerable(), false),
                    );
                }
                return obj_value;
            }
            // Arrays: indices + named props live outside `keys_array`.
            if super::mark_all_array_props(
                obj, /*drop_writable=*/ false, /*drop_configurable=*/ true,
            ) {
                mark_all_symbol_keys(
                    obj, /*drop_writable=*/ false, /*drop_configurable=*/ true,
                );
                return obj_value;
            }
            // Drop configurable for every existing key (but leave writable intact).
            mark_all_keys(
                obj, /*drop_writable=*/ false, false, /*drop_configurable=*/ true,
            );
            mark_all_symbol_keys(
                obj, /*drop_writable=*/ false, /*drop_configurable=*/ true,
            );
        }
    }
    obj_value
}

/// Object.preventExtensions(obj) — sets the no-extend flag. Returns the object.
#[no_mangle]
pub extern "C" fn js_object_prevent_extensions(obj_value: f64) -> f64 {
    // A Proxy is a small registered id, not a heap object — `extract_obj_ptr`
    // yields the fake pointer and `gc_header_for` would deref unmapped memory.
    // Route through the `[[PreventExtensions]]` trap; per spec throw a TypeError
    // if it reports failure, then return the proxy. (Proxy crash cluster.)
    if crate::proxy::js_proxy_is_proxy(obj_value) != 0 {
        let ok = crate::proxy::js_reflect_prevent_extensions(obj_value);
        if crate::value::js_is_truthy(ok) == 0 {
            throw_object_type_error(b"'preventExtensions' on proxy: trap returned falsish");
        }
        return obj_value;
    }
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if !obj.is_null() && (obj as usize) > 0x10000 {
            // Typed arrays: side table, NOT the GC header — small typed
            // arrays are plain-`alloc`ed with no `GcHeader`, so the flag
            // write below would corrupt allocator metadata.
            if crate::typedarray::lookup_typed_array_kind(obj as usize).is_some() {
                crate::typedarray_props::typed_array_mark_no_extend(obj as usize);
                return obj_value;
            }
            let gc = gc_header_for(obj);
            (*gc)._reserved |= crate::gc::OBJ_FLAG_NO_EXTEND;
        }
    }
    obj_value
}

/// `TestIntegrityLevel(O, level)` (ECMA-262 7.3.16) for an ordinary heap
/// object: the object must be non-extensible, and every own property must be
/// non-configurable — plus, for the `frozen` level, every own *data* property
/// must be non-writable. Returns `true` if the object satisfies the level.
///
/// A key with no side-table descriptor entry uses the default
/// `{writable: true, enumerable: true, configurable: true}`, which is
/// configurable (and writable), so any such key fails both levels — matching
/// the behaviour of `Object.freeze`/`Object.seal`, which populate the table.
unsafe fn object_integrity_level(obj: *mut ObjectHeader, frozen: bool) -> bool {
    // Must be non-extensible first.
    let gc = gc_header_for(obj);
    if (*gc)._reserved & crate::gc::OBJ_FLAG_NO_EXTEND == 0 {
        return false;
    }
    // Closures: own props are `name`/`length` + dynamic props (side-table
    // attrs), NOT a `keys_array` — the walk below would read garbage off the
    // ClosureHeader. `name`/`length` are non-writable but configurable by
    // default, so an un-frozen function fails both levels; `js_object_freeze`
    // / `seal` record explicit attrs that satisfy them.
    if (*gc).obj_type == crate::gc::GC_TYPE_CLOSURE || crate::closure::is_closure_ptr(obj as usize)
    {
        let owner = obj as usize;
        for builtin in ["name", "length"] {
            if crate::closure::closure_is_key_deleted(owner, builtin) {
                continue;
            }
            let attrs = get_property_attrs(owner, builtin)
                .unwrap_or(PropertyAttrs::new(false, false, true));
            if attrs.configurable() {
                return false;
            }
        }
        for (name, _) in crate::closure::closure_dynamic_props_snapshot(owner) {
            if crate::closure::closure_is_key_deleted(owner, &name) {
                continue;
            }
            let Some(attrs) = get_property_attrs(owner, &name) else {
                return false;
            };
            if attrs.configurable() {
                return false;
            }
            if frozen
                && get_accessor_descriptor(owner, &name).is_none()
                && attrs.writable()
                && name != "prototype"
            {
                return false;
            }
        }
        return true;
    }
    // Date / RegExp / Error exotic instances: own props live in the side
    // tables; the keys_array walk below would bit-cast the cell.
    if let Some(kind) = super::exotic_expando::exotic_expando_kind(obj as usize) {
        let owner = obj as usize;
        for name in super::exotic_expando::exotic_own_keys(kind, owner, false) {
            let Some(attrs) = get_property_attrs(owner, &name) else {
                return false;
            };
            if attrs.configurable() {
                return false;
            }
            if frozen && get_accessor_descriptor(owner, &name).is_none() && attrs.writable() {
                return false;
            }
        }
        return true;
    }
    // Arrays: a non-empty array's dense elements are configurable/writable
    // unless frozen/sealed dropped those bits (tracked by the array flags).
    if (*gc).obj_type == crate::gc::GC_TYPE_ARRAY {
        let arr = obj as *const crate::array::ArrayHeader;
        let len = crate::array::js_array_length(arr);
        if len > 0 {
            // Has index properties: frozen iff the FROZEN flag is set; sealed
            // iff SEALED (frozen implies sealed).
            let flag = if frozen {
                crate::gc::OBJ_FLAG_FROZEN
            } else {
                crate::gc::OBJ_FLAG_SEALED
            };
            return (*gc)._reserved & flag != 0;
        }
        // Empty array + non-extensible ⇒ integrity holds.
        return true;
    }
    let keys = (*obj).keys_array;
    if keys.is_null() {
        return true; // no own keys + non-extensible ⇒ frozen/sealed
    }
    let keys_ptr = keys as usize;
    if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
        return true;
    }
    let key_count = crate::array::js_array_length(keys) as usize;
    if key_count > 65536 {
        return false;
    }
    for i in 0..key_count {
        let key_val = crate::array::js_array_get(keys, i as u32);
        if !key_val.is_string() {
            continue;
        }
        let stored_key = key_val.as_string_ptr();
        if stored_key.is_null() {
            continue;
        }
        let name_ptr = (stored_key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let name_len = (*stored_key).byte_len as usize;
        let name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
            Ok(s) => s,
            Err(_) => continue,
        };
        // No side-table entry ⇒ default {w,e,c}=true ⇒ configurable ⇒ fails.
        let Some(attrs) = get_property_attrs(obj as usize, name) else {
            return false;
        };
        if attrs.configurable() {
            return false;
        }
        if frozen {
            // Data properties must be non-writable; accessor properties have no
            // writability constraint.
            let is_accessor = get_accessor_descriptor(obj as usize, name).is_some();
            if !is_accessor && attrs.writable() {
                return false;
            }
        }
    }
    // Symbol-keyed own properties must satisfy the same constraints.
    let owner = obj as usize;
    for (sym_ptr, _) in crate::symbol::clone_symbol_entries_for_obj_ptr(owner) {
        let Some(attrs) = crate::symbol::get_symbol_property_attrs(owner, sym_ptr) else {
            return false; // default {w,e,c}=true ⇒ configurable ⇒ fails
        };
        if attrs.configurable() {
            return false;
        }
        if frozen {
            let is_accessor =
                crate::symbol::symbol_accessor_descriptor_bits(owner, sym_ptr).is_some();
            if !is_accessor && attrs.writable() {
                return false;
            }
        }
    }
    true
}

/// Object.isFrozen(obj) — returns NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_object_is_frozen(obj_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    if crate::proxy::js_proxy_is_proxy(obj_value) != 0 {
        return if unsafe {
            test_integrity_level_proxy(obj_value, /*frozen=*/ true)
        } {
            f64::from_bits(TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        };
    }
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) <= 0x10000 {
            return f64::from_bits(TAG_TRUE); // non-objects are vacuously frozen
        }
        let gc = gc_header_for(obj);
        // Fast path: the FROZEN flag is authoritative when set.
        if (*gc)._reserved & crate::gc::OBJ_FLAG_FROZEN != 0 {
            return f64::from_bits(TAG_TRUE);
        }
        if object_integrity_level(obj, /*frozen=*/ true) {
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
    if crate::proxy::js_proxy_is_proxy(obj_value) != 0 {
        return if unsafe {
            test_integrity_level_proxy(obj_value, /*frozen=*/ false)
        } {
            f64::from_bits(TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        };
    }
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) <= 0x10000 {
            return f64::from_bits(TAG_TRUE); // non-objects are vacuously sealed
        }
        let gc = gc_header_for(obj);
        if (*gc)._reserved & crate::gc::OBJ_FLAG_SEALED != 0 {
            return f64::from_bits(TAG_TRUE);
        }
        if object_integrity_level(obj, /*frozen=*/ false) {
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
    // Proxy receiver: route through the `[[IsExtensible]]` trap rather than
    // dereferencing the fake pointer. (Proxy crash cluster.)
    if crate::proxy::js_proxy_is_proxy(obj_value) != 0 {
        let r = crate::proxy::js_reflect_is_extensible(obj_value);
        return if crate::value::js_is_truthy(r) != 0 {
            f64::from_bits(TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        };
    }
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) <= 0x10000 {
            return f64::from_bits(TAG_FALSE); // non-objects are not extensible
        }
        // Typed arrays and ArrayBuffers use a non-standard allocation that does
        // not carry the 8-byte object `GcHeader` the freeze/seal/extend flags
        // live in (small typed arrays are raw-`alloc`'d with the
        // `TypedArrayHeader` at offset 0 — only the large-object path interposes
        // a `GcHeader`). Reading `_reserved` for them dereferences whatever
        // precedes the allocation, so `isExtensible` would non-deterministically
        // report `false` depending on heap layout. Integer-indexed exotic
        // objects are extensible by default; report that instead of reading a
        // header that may not exist.
        let raw = crate::value::js_nanbox_get_pointer(obj_value) as usize;
        if raw > 0x10000 && crate::typedarray::lookup_typed_array_kind(raw).is_some() {
            return if crate::typedarray_props::typed_array_owner_no_extend(raw) {
                f64::from_bits(TAG_FALSE)
            } else {
                f64::from_bits(TAG_TRUE)
            };
        }
        if raw > 0x10000 && crate::buffer::is_registered_buffer(raw) {
            return f64::from_bits(TAG_TRUE);
        }
        let gc = gc_header_for(obj);
        if (*gc)._reserved & crate::gc::OBJ_FLAG_NO_EXTEND != 0 {
            f64::from_bits(TAG_FALSE)
        } else {
            f64::from_bits(TAG_TRUE)
        }
    }
}
