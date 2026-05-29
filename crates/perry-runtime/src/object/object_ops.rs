//! `Object.*` static methods and descriptor machinery:
//! `Object.fromEntries`/`groupBy`/`is`/`hasOwn`/`create`/`freeze`/`seal`/
//! `defineProperty`/`getOwnPropertyDescriptor`/`getPrototypeOf`/... plus
//! the `js_object_*` helpers backing them.
//!
//! Split out of `object.rs` (issue #1103). Pure relocation. The
//! `globalThis` singleton subsystem stays in the parent module because
//! it is also consumed by class/builtin-resolution code there.

use super::*;

fn throw_from_entries_type_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn throw_from_entries_not_iterable() -> ! {
    throw_from_entries_type_error(b"undefined is not iterable")
}

fn throw_from_entries_non_object_entry() -> ! {
    throw_from_entries_type_error(b"Iterator value is not an entry object")
}

unsafe fn object_from_entries_gc_type(raw_ptr: i64) -> Option<u8> {
    if raw_ptr < crate::gc::GC_HEADER_SIZE as i64 + 0x1000 {
        return None;
    }
    let addr = raw_ptr as usize;
    if crate::symbol::is_registered_symbol(addr) {
        return None;
    }
    if crate::set::is_registered_set(addr) {
        return Some(crate::gc::GC_TYPE_SET);
    }
    if crate::map::is_registered_map(addr) {
        return Some(crate::gc::GC_TYPE_MAP);
    }
    let ptr = raw_ptr as *const u8;
    if !crate::object::is_valid_obj_ptr(ptr) {
        return None;
    }
    let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    Some((*gc_header).obj_type)
}

unsafe fn object_from_entries_array_ptr(value: f64) -> *mut ArrayHeader {
    let raw = crate::value::js_nanbox_get_pointer(value);
    let gc_type = object_from_entries_gc_type(raw);
    if gc_type != Some(crate::gc::GC_TYPE_ARRAY) && gc_type != Some(crate::gc::GC_TYPE_LAZY_ARRAY) {
        throw_from_entries_not_iterable();
    }
    raw as *mut ArrayHeader
}

unsafe fn object_from_entries_has_iterator(value: f64, raw: i64, gc_type: Option<u8>) -> bool {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_any_string() {
        return true;
    }
    match gc_type {
        Some(crate::gc::GC_TYPE_ARRAY)
        | Some(crate::gc::GC_TYPE_LAZY_ARRAY)
        | Some(crate::gc::GC_TYPE_MAP)
        | Some(crate::gc::GC_TYPE_SET) => return true,
        Some(crate::gc::GC_TYPE_OBJECT) => {
            let obj = raw as *mut ObjectHeader;
            if crate::url::try_read_as_search_params(obj).is_some() {
                return true;
            }
            if !obj.is_null() && (*obj).class_id == crate::array::ARRAY_ITERATOR_CLASS_ID {
                return true;
            }
        }
        _ => {}
    }

    let iter_sym = crate::symbol::well_known_symbol("iterator");
    if !iter_sym.is_null() {
        let sym_value =
            f64::from_bits(crate::value::JSValue::pointer(iter_sym as *const u8).bits());
        let iter_fn = crate::symbol::js_object_get_symbol_property(value, sym_value);
        let iter_fn_ptr = crate::value::js_nanbox_get_pointer(iter_fn);
        if iter_fn_ptr != 0 && crate::closure::is_closure_ptr(iter_fn_ptr as usize) {
            return true;
        }
    }

    crate::array::has_iterator_next(value)
}

unsafe fn object_from_entries_materialize_entries(entries_value: f64) -> *mut ArrayHeader {
    let jv = crate::value::JSValue::from_bits(entries_value.to_bits());
    if jv.is_null() || jv.is_undefined() || jv.is_bool() || jv.is_number() || jv.is_int32() {
        throw_from_entries_not_iterable();
    }
    if jv.is_bigint() {
        throw_from_entries_not_iterable();
    }

    let raw = crate::value::js_nanbox_get_pointer(entries_value);
    let gc_type = object_from_entries_gc_type(raw);

    if !jv.is_any_string() && raw == 0 {
        throw_from_entries_not_iterable();
    }

    if !object_from_entries_has_iterator(entries_value, raw, gc_type) {
        throw_from_entries_not_iterable();
    }

    if gc_type == Some(crate::gc::GC_TYPE_MAP) {
        return crate::map::js_map_entries(raw as *const crate::map::MapHeader);
    }

    if gc_type == Some(crate::gc::GC_TYPE_OBJECT) {
        let obj = raw as *mut ObjectHeader;
        if crate::url::try_read_as_search_params(obj).is_some() {
            let boxed = crate::url::js_url_search_params_entries_arr(obj);
            return object_from_entries_array_ptr(boxed);
        }
    }

    let boxed = crate::array::js_for_of_to_array(entries_value);
    object_from_entries_array_ptr(boxed)
}

unsafe fn object_from_entries_entry_values(entry_val: f64) -> (f64, f64) {
    let jv = crate::value::JSValue::from_bits(entry_val.to_bits());
    if jv.is_null()
        || jv.is_undefined()
        || jv.is_bool()
        || jv.is_number()
        || jv.is_int32()
        || jv.is_any_string()
        || jv.is_bigint()
    {
        throw_from_entries_non_object_entry();
    }

    let raw = crate::value::js_nanbox_get_pointer(entry_val);
    let gc_type = object_from_entries_gc_type(raw);
    if raw == 0 {
        throw_from_entries_non_object_entry();
    }

    if gc_type == Some(crate::gc::GC_TYPE_ARRAY) || gc_type == Some(crate::gc::GC_TYPE_LAZY_ARRAY) {
        let arr = raw as *const ArrayHeader;
        return (
            crate::array::js_array_get_f64(arr, 0),
            crate::array::js_array_get_f64(arr, 1),
        );
    }

    let obj = raw as *const ObjectHeader;
    if obj.is_null() {
        throw_from_entries_non_object_entry();
    }
    let key0 = crate::string::js_string_from_bytes(b"0".as_ptr(), 1);
    let key1 = crate::string::js_string_from_bytes(b"1".as_ptr(), 1);
    (
        js_object_get_field_by_name_f64(obj, key0),
        js_object_get_field_by_name_f64(obj, key1),
    )
}

/// Object.fromEntries(entries) — build an object from iterable [key, value] entries.
#[no_mangle]
pub extern "C" fn js_object_from_entries(entries_value: f64) -> f64 {
    unsafe {
        let arr_ptr = object_from_entries_materialize_entries(entries_value);
        let length = crate::array::js_array_length(arr_ptr) as usize;

        // Allocate empty object — class_id 0 = generic object
        let obj = js_object_alloc(0, length as u32);
        if obj.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }

        for i in 0..length {
            let entry_val = crate::array::js_array_get_f64(arr_ptr, i as u32);
            let (key_val, val_val) = object_from_entries_entry_values(entry_val);
            let key_str = crate::builtins::js_string_coerce(key_val);
            if key_str.is_null() {
                continue;
            }
            js_object_set_field_by_name(obj, key_str, val_val);
        }

        crate::value::js_nanbox_pointer(obj as i64)
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
                // GC_STORE_AUDIT(INIT): groupBy result array is unpublished; layout is rebuilt before publication.
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

    // For strings, do content comparison. #1781: accept inline SSO short
    // strings on either side. Two SSO operands with equal content already
    // match via the bit-pattern fallback below, but a mixed SSO/heap pair
    // (same content, different representation — e.g. a JSON-parsed value vs
    // a heap literal) would not. Materialize via the unified decoder so the
    // comparison is representation-independent.
    if a_jsval.is_any_string() && b_jsval.is_any_string() {
        let result = crate::string::js_string_equals(
            crate::value::js_get_string_pointer_unified(f64::from_bits(a_bits))
                as *const crate::StringHeader,
            crate::value::js_get_string_pointer_unified(f64::from_bits(b_bits))
                as *const crate::StringHeader,
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
        if obj.is_null() || (obj as usize) < 0x10000 {
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

/// #2159 helper: install a `target_cid.method` entry from an
/// `Object.defineProperty(C.prototype, name, descriptor)` call.
///
/// The descriptor's `value` came in two main shapes in practice:
///
/// 1. A `BOUND_METHOD_FUNC_PTR` closure returned by `getOwnPropertyDescriptor`
///    on a sibling class (drizzle's `applyMixins(Base, [Mixin])`: the
///    `getOwnPropertyDescriptor(Mixin.prototype, name)` value reads as
///    `js_class_method_bind(Mixin_class_ref, name)`). Dispatching that bound
///    closure would re-enter `js_native_call_method` against the class-ref —
///    a class object reaches the *static* dispatch arm, not the instance
///    method, so calling it would return the wrong thing. Instead we look up
///    the raw vtable entry on the source class and copy it onto the target
///    class's vtable directly, so future `inst.method(args)` dispatches via
///    the regular chain walk with `this = inst`.
///
/// 2. A user-supplied closure (e.g. `Object.defineProperty(C.prototype, "m",
///    { value: function () { … } })`). Route through the same per-class
///    prototype-method side table that `js_register_prototype_method` (#838)
///    uses, so the `inst.m` / `inst.m()` lookup paths in
///    `field_get_set.rs` / `native_call_method.rs` find it after the regular
///    vtable miss.
unsafe fn define_class_prototype_method(target_cid: u32, name: &str, value_bits: u64) {
    use crate::closure::{ClosureHeader, BOUND_METHOD_FUNC_PTR, CLOSURE_MAGIC};
    use crate::object::class_registry::{ClassVTable, VTableMethodEntry, CLASS_VTABLE_REGISTRY};

    // Reject undefined / null / numeric values up front — those aren't
    // methods and shouldn't make it onto the prototype side tables.
    let value = f64::from_bits(value_bits);
    let jsv = crate::JSValue::from_bits(value_bits);
    if !jsv.is_pointer() {
        return;
    }
    let ptr = jsv.as_pointer::<u8>() as usize;
    if ptr < 0x1000 {
        return;
    }

    // Shape (1): BOUND_METHOD closure. Extract source class-ref + method
    // name from the captures (see `js_class_method_bind`), then copy the
    // source class's vtable entry (or any inherited entry up the parent
    // chain) onto `target_cid`.
    if crate::closure::is_closure_ptr(ptr) {
        let closure = ptr as *const ClosureHeader;
        if (*closure).type_tag == CLOSURE_MAGIC && (*closure).func_ptr == BOUND_METHOD_FUNC_PTR {
            let recv = crate::closure::js_closure_get_capture_f64(closure, 0);
            if let Some(source_cid) = super::class_ref_id(recv) {
                if let Some((func_ptr, param_count)) =
                    super::lookup_class_method_in_chain(source_cid, name)
                {
                    let mut guard = CLASS_VTABLE_REGISTRY.write().unwrap();
                    if guard.is_none() {
                        *guard = Some(std::collections::HashMap::new());
                    }
                    let reg = guard.as_mut().unwrap();
                    let vtable = reg.entry(target_cid).or_insert_with(|| ClassVTable {
                        methods: std::collections::HashMap::new(),
                        getters: std::collections::HashMap::new(),
                        setters: std::collections::HashMap::new(),
                    });
                    vtable.methods.insert(
                        name.to_string(),
                        VTableMethodEntry {
                            func_ptr,
                            param_count,
                        },
                    );
                    drop(guard);
                    super::class_registry::js_register_class_id(target_cid);
                    crate::typed_feedback::invalidate_method_change(target_cid);
                    return;
                }
            }
        }
    }

    // Shape (2): any other callable value (user closure, regular function).
    // Mirror the `Class.prototype.method = fn` direct-assignment path so the
    // existing `lookup_prototype_method` walks find it.
    super::class_registry::js_register_prototype_method(
        target_cid,
        name.as_ptr(),
        name.len(),
        value,
    );
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
        // #2159: when the receiver is a class-ref (`Class.prototype` evaluates
        // back to the class itself in Perry — see `class_ref_id` /
        // `js_object_get_own_property_descriptor`'s class-ref arm), route the
        // descriptor through the class-vtable / prototype-method side tables
        // so instance lookups (`new C().method`) see the new entry. Drizzle's
        // `applyMixins(Base, [Mixin])` copies methods between class
        // prototypes via `Object.defineProperty(Base.prototype, name,
        // Object.getOwnPropertyDescriptor(Mixin.prototype, name))` — pre-fix
        // the call hit `extract_obj_ptr → null` (a class-ref isn't a pointer)
        // and silently dropped the descriptor, so `await
        // db.select().from(x)` saw `instance.then === undefined` and `await`
        // unwrapped the builder unchanged.
        if let Some(target_cid) = super::class_ref_id(obj_value) {
            if let Some(name) = super::metadata_key_to_string(key_value) {
                let desc_ptr = extract_obj_ptr(descriptor_value);
                if !desc_ptr.is_null() {
                    let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
                    let value_field =
                        js_object_get_field_by_name(desc_ptr as *const ObjectHeader, value_key);
                    if !value_field.is_undefined() {
                        define_class_prototype_method(target_cid, &name, value_field.bits());
                    }
                }
            }
            return obj_value;
        }

        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() {
            return obj_value;
        }
        // #1250: when the key is a Symbol, route into the symbol side
        // table (`SYMBOL_PROPERTIES`) the same way `obj[sym] = value`
        // does. Without this, `Object.defineProperty(obj, sym, ...)`
        // would drop the symbol and try to coerce it to a string,
        // which is exactly the failure mode reported for
        // `Object.defineProperty(obj, inspect.custom, …)`.
        let key_bits = key_value.to_bits();
        let key_tag = key_bits & 0xFFFF_0000_0000_0000;
        if key_tag == 0x7FFD_0000_0000_0000 {
            let raw_ptr = (key_bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::symbol::SymbolHeader;
            if !raw_ptr.is_null()
                && (raw_ptr as usize) >= 0x1000
                && (*raw_ptr).magic == crate::symbol::SYMBOL_MAGIC
            {
                let desc_ptr = extract_obj_ptr(descriptor_value);
                if !desc_ptr.is_null() {
                    let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
                    let value_field =
                        js_object_get_field_by_name(desc_ptr as *const ObjectHeader, value_key);
                    if !value_field.is_undefined() {
                        crate::symbol::js_object_set_symbol_property(
                            obj_value,
                            key_value,
                            f64::from_bits(value_field.bits()),
                        );
                    }
                }
                return obj_value;
            }
        }
        // Extract key string
        let key_str = crate::builtins::js_string_coerce(key_value);
        if key_str.is_null() {
            return obj_value;
        }
        super::mark_object_dynamic_shape_unknown(obj);
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

/// Object-literal accessor installer for `{ get k(){}, set k(v){} }` (#2442).
///
/// Installs — or *merges into* — the accessor descriptor for `(obj, key)`.
/// Two semantic differences from `Object.defineProperty`:
///
///  1. Object-literal accessors are **enumerable** and **configurable** (JS
///     spec), whereas `defineProperty`'s omitted attrs default to `false`.
///  2. A separate `get k` and `set k` for the *same* key must merge into one
///     accessor rather than clobber each other — so a `js_value_undefined`
///     `getter`/`setter` leaves the existing half of the descriptor untouched.
///
/// `getter` / `setter` are NaN-boxed closure values, or `undefined` to skip.
/// Each present closure is cloned and rebound so it runs with `this === obj`
/// (the same #450 model `js_object_define_property` uses for its accessors).
#[no_mangle]
pub extern "C" fn js_object_define_accessor(
    obj_value: f64,
    key_value: f64,
    getter: f64,
    setter: f64,
) -> f64 {
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() {
            return obj_value;
        }
        let key_str = crate::builtins::js_string_coerce(key_value);
        if key_str.is_null() {
            return obj_value;
        }
        super::mark_object_dynamic_shape_unknown(obj);
        let key_rust: Option<String> = {
            let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key_str).byte_len as usize;
            let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
            std::str::from_utf8(name_bytes).ok().map(|s| s.to_string())
        };
        ensure_key_in_keys_array(obj, key_str);
        let Some(k) = key_rust else {
            return obj_value;
        };
        let recv_box = crate::value::js_nanbox_pointer(obj as i64);
        let existing = get_accessor_descriptor(obj as usize, &k).unwrap_or_default();
        let undef = crate::value::TAG_UNDEFINED;
        let get_bits = if getter.to_bits() == undef {
            existing.get
        } else {
            crate::closure::clone_closure_rebind_this(getter.to_bits(), recv_box)
        };
        let set_bits = if setter.to_bits() == undef {
            existing.set
        } else {
            crate::closure::clone_closure_rebind_this(setter.to_bits(), recv_box)
        };
        set_accessor_descriptor(
            obj as usize,
            k.clone(),
            AccessorDescriptor {
                get: get_bits,
                set: set_bits,
            },
        );
        // Object-literal accessors are enumerable + configurable (spec).
        // `writable` is meaningless for accessors; pass `true` so any data
        // fallthrough write before the accessor override isn't rejected.
        set_property_attrs(obj as usize, k, PropertyAttrs::new(true, true, true));
        obj_value
    }
}

/// Ensure a key appears in the object's keys_array. Used by `Object.defineProperty`
/// so the property is enumerable-filterable and discoverable by `getOwnPropertyNames`
/// even when the value is undefined or the property is an accessor (no underlying slot).
#[allow(unused_assignments)]
unsafe fn ensure_key_in_keys_array(obj: *mut ObjectHeader, key: *const crate::StringHeader) {
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
                    // GC_STORE_AUDIT(INIT): descriptor object is freshly allocated; layout is rebuilt before publication.
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

        // #2059: function objects (closures) are not `ObjectHeader`s — routing
        // them through `extract_obj_ptr`/`own_key_present` below reads an
        // out-of-bounds "keys_array" slot (offset 16, past a 0-capture
        // closure's payload) and segfaults. Resolve their descriptors here:
        // the built-in `name`/`length` slots (non-writable, non-enumerable,
        // configurable per spec) plus any user-attached own data property.
        {
            let jsv = crate::JSValue::from_bits(obj_value.to_bits());
            if jsv.is_pointer() {
                let ptr = jsv.as_pointer::<u8>() as usize;
                if crate::closure::is_closure_ptr(ptr) {
                    let key_str = crate::builtins::js_string_coerce(key_value);
                    if key_str.is_null() {
                        return f64::from_bits(crate::value::TAG_UNDEFINED);
                    }
                    let name_ptr =
                        (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let name_len = (*key_str).byte_len as usize;
                    let name = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
                        .unwrap_or("");

                    // (value, writable, configurable). `name`/`length` are the
                    // built-in own data slots; anything else falls back to the
                    // user-attached dynamic-property side table.
                    let resolved: Option<(f64, bool, bool)> = match name {
                        "length" => {
                            let closure_value = crate::value::js_nanbox_pointer(ptr as i64);
                            if let Some(arity) =
                                super::native_module::bound_native_callable_value_arity(
                                    closure_value,
                                )
                            {
                                Some((arity as f64, false, true))
                            } else {
                                let arity = crate::closure::closure_arity(
                                    ptr as *const crate::closure::ClosureHeader,
                                );
                                // Numbers are NaN-boxed as their raw f64 bits.
                                Some((arity.unwrap_or(0) as f64, false, true))
                            }
                        }
                        "name" => {
                            let dynv = crate::closure::closure_get_dynamic_prop(ptr, "name");
                            if dynv.to_bits() != crate::value::TAG_UNDEFINED {
                                Some((dynv, true, true))
                            } else {
                                let func_ptr = (*(ptr as *const crate::closure::ClosureHeader))
                                    .func_ptr
                                    as usize;
                                let fname = crate::builtins::function_name_for_ptr(func_ptr)
                                    .unwrap_or_default();
                                let s = crate::string::js_string_from_bytes(
                                    fname.as_ptr(),
                                    fname.len() as u32,
                                );
                                Some((crate::js_nanbox_string(s as i64), false, true))
                            }
                        }
                        _ => {
                            let dynv = crate::closure::closure_get_dynamic_prop(ptr, name);
                            if dynv.to_bits() != crate::value::TAG_UNDEFINED {
                                Some((dynv, true, true))
                            } else {
                                None
                            }
                        }
                    };
                    let Some((value, writable, configurable)) = resolved else {
                        return f64::from_bits(crate::value::TAG_UNDEFINED);
                    };
                    // `name`/`length` are non-enumerable; user data props are.
                    let enumerable = !matches!(name, "name" | "length");
                    let packed = b"value\0writable\0enumerable\0configurable";
                    let desc = js_object_alloc_with_shape(
                        0x0D_E5_C0,
                        4,
                        packed.as_ptr(),
                        packed.len() as u32,
                    );
                    let header_size = std::mem::size_of::<ObjectHeader>();
                    let fields = (desc as *mut u8).add(header_size) as *mut f64;
                    // GC_STORE_AUDIT(INIT): descriptor object is freshly allocated; layout is rebuilt before publication.
                    *fields = value;
                    *fields.add(1) = f64::from_bits(if writable { TAG_TRUE } else { TAG_FALSE });
                    *fields.add(2) = f64::from_bits(if enumerable { TAG_TRUE } else { TAG_FALSE });
                    *fields.add(3) =
                        f64::from_bits(if configurable { TAG_TRUE } else { TAG_FALSE });
                    super::rebuild_object_field_layout(desc, 4);
                    return f64::from_bits((desc as u64) | 0x7FFD_0000_0000_0000);
                }
            }
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
            // GC_STORE_AUDIT(INIT): descriptor object is freshly allocated; layout is rebuilt before publication.
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
            // GC_STORE_AUDIT(INIT): descriptor boolean fields are pointer-free and layout is rebuilt below.
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
        // GC_STORE_AUDIT(INIT): descriptor object is freshly allocated; layout is rebuilt before publication.
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
    if obj.is_null() || (obj as usize) < 0x10000 || key.is_null() {
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
        // #1781: SSO-aware match — `hasOwnProperty("id")` previously
        // returned false when "id" lived as an inline SSO key.
        if crate::string::js_string_key_matches(stored, key) {
            return true;
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
    if name_ptr.is_null() {
        return f64::from_bits(TAG_UNDEF);
    }
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) < 0x10000 {
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
            // #1781: SSO-aware match by byte slice — the
            // own-property-or-undef path was the route through which
            // hono's `c.req.X` dispatch decided to invoke the vtable
            // getter, and pre-fix a SSO-stored `X` was invisible here.
            if crate::string::js_string_key_matches_bytes(key_val, key_bytes) {
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

        // String / array values have no `ObjectHeader.keys_array`; their own
        // property names are the index names `"0".."len-1"` plus `"length"`.
        // Reading a bogus `keys_array` off their header segfaulted (#800).
        {
            const TAG_TRUE_BITS: u64 = 0x7FFC_0000_0000_0004;
            let jv = JSValue::from_bits(obj_value.to_bits());
            let n: Option<u32> = if jv.is_any_string() {
                let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                match crate::string::str_bytes_from_jsvalue(obj_value, &mut scratch) {
                    Some((p, blen)) if !p.is_null() => {
                        Some(crate::string::compute_utf16_len(p, blen))
                    }
                    _ => Some(0),
                }
            } else if crate::array::js_array_is_array(obj_value).to_bits() == TAG_TRUE_BITS {
                let ap = extract_obj_ptr(obj_value) as *const crate::array::ArrayHeader;
                Some(crate::array::js_array_length(ap))
            } else {
                None
            };
            if let Some(n) = n {
                let result = crate::array::js_array_alloc(n + 1);
                for i in 0..n {
                    let s = i.to_string();
                    let k = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                    crate::array::js_array_push(result, JSValue::string_ptr(k));
                }
                let lk = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
                crate::array::js_array_push(result, JSValue::string_ptr(lk));
                return f64::from_bits((result as u64) | 0x7FFD_0000_0000_0000);
            }
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

/// Object.getOwnPropertyDescriptors(obj) — returns a new object whose own
/// property keys (the same set `Object.getOwnPropertyNames` reports, including
/// non-enumerable keys and class-ref method names) each map to the property
/// descriptor produced by `js_object_get_own_property_descriptor`. Spec:
/// "for each own property key K of O, set result[K] = descriptor(O, K)".
///
/// effect's `SchemaAST.annotations` builds a fresh AST node via
/// `Object.create(Object.getPrototypeOf(ast), Object.getOwnPropertyDescriptors(ast))`,
/// so without this the plural call lowered to a null callee and Schema.ts
/// module init threw `TypeError: value is not a function` (#1791/#1758).
#[no_mangle]
pub extern "C" fn js_object_get_own_property_descriptors(obj_value: f64) -> f64 {
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    unsafe {
        // Enumerate own keys exactly like Object.getOwnPropertyNames — this
        // handles class refs and plain objects, and includes non-enumerable
        // keys, matching the spec's [[OwnPropertyKeys]] string-key set.
        let names_value = js_object_get_own_property_names(obj_value);
        let names_arr =
            crate::value::js_nanbox_get_pointer(names_value) as *const crate::array::ArrayHeader;

        // Fresh result object that collects { key: descriptor } entries.
        // Like js_object_entries / js_object_get_own_property_names above, the
        // intermediate allocations aren't rooted — Perry's builder helpers
        // follow this convention.
        let result = js_object_alloc(0, 0);

        if !names_arr.is_null() {
            let len = crate::array::js_array_length(names_arr) as usize;
            for i in 0..len {
                let key_val = crate::array::js_array_get(names_arr, i as u32);
                let key_f64 = f64::from_bits(key_val.bits());
                let desc = js_object_get_own_property_descriptor(obj_value, key_f64);
                let key_str = crate::builtins::js_string_coerce(key_f64);
                if !key_str.is_null() {
                    js_object_set_field_by_name(result, key_str, desc);
                }
            }
        }
        f64::from_bits((result as u64) | POINTER_TAG)
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
                // #1805: link the synthetic class_id into the original class's
                // inheritance chain. `Object.getPrototypeOf(instance)` returns
                // the instance pointer itself in Perry's model (see
                // `js_object_get_prototype_of`), so `proto_ptr` here is a real
                // class instance whose `class_id` field IS the user class's
                // id. Registering it as the synthetic cid's parent lets
                // `js_instanceof`'s `get_parent_class_id` walk reach the
                // original class and match — without this, the chain stopped
                // at the unregistered synthetic id and `Object.create(proto)
                // instanceof C` was always false even though property /
                // getter dispatch through the chain worked correctly.
                let parent_class_id = unsafe { (*proto_ptr).class_id };
                if parent_class_id != 0 && parent_class_id != cid {
                    register_class(cid, parent_class_id);
                }
                class_id = cid;
            }
        }
    }
    // #1175: when `proto_value` is null/undefined/non-object, the resulting
    // object has no [[Prototype]]. Stamp OBJ_FLAG_NULL_PROTO so
    // `Object.getPrototypeOf(Object.create(null))` returns null (it
    // previously returned the object itself).
    let null_proto = class_id == 0;
    let obj = if null_proto {
        js_object_alloc_null_proto(class_id, 0)
    } else {
        js_object_alloc(class_id, 0)
    };
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

fn constructor_dynamic_prototype(obj: *const ObjectHeader) -> Option<f64> {
    if obj.is_null() {
        return None;
    }
    let key =
        crate::string::js_string_from_bytes(b"constructor".as_ptr(), b"constructor".len() as u32);
    let constructor = js_object_get_field_by_name_f64(obj, key);
    let bits = constructor.to_bits();
    let top16 = bits >> 48;
    if top16 != 0x7FFD {
        return None;
    }
    let raw_addr = (bits & crate::value::POINTER_MASK) as usize;
    if raw_addr < (crate::gc::GC_HEADER_SIZE as usize) + 0x1000 {
        return None;
    }
    let gc = unsafe { gc_header_for(raw_addr as *const ObjectHeader) };
    if unsafe { (*gc).obj_type } != crate::gc::GC_TYPE_CLOSURE {
        return None;
    }
    let proto = crate::closure::closure_get_dynamic_prop(raw_addr, "prototype");
    if crate::value::JSValue::from_bits(proto.to_bits()).is_undefined() {
        None
    } else {
        Some(proto)
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
            unsafe {
                let obj = raw_addr as *const ObjectHeader;
                let gc = gc_header_for(obj);
                // #1175: objects allocated with a null prototype
                // (Object.create(null), querystring.parse) report null here.
                if (*gc)._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0 {
                    return f64::from_bits(TAG_NULL);
                }
                // #2145: per-kind typed-array `.prototype` objects share a
                // single `%TypedArray%.prototype` parent. Resolved off the
                // cached intrinsic pointer (also a GC root) so the chain holds
                // through copying GC.
                if (*gc)._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0 {
                    let p = crate::object::typed_array_intrinsic_proto_ptr();
                    if !p.is_null() {
                        return f64::from_bits(crate::value::js_nanbox_pointer(p as i64).to_bits());
                    }
                }
                // #489 / #2145: a function/constructor receiver has no
                // walkable [[Prototype]] in Perry's model UNLESS its
                // closure-static-prototype side-table has been set
                // (`Object.setPrototypeOf(closure, parent)` — effect's
                // TagClass and Perry's `%TypedArray%`-chain typed-array
                // constructors use this). Returning the recorded parent
                // satisfies drizzle's `cls = getPrototypeOf(cls)` walk
                // (which terminates when the parent has no further
                // recorded proto) and the test262 `__proto__` chain. When
                // no static prototype is recorded, return null to break
                // the would-be `getPrototypeOf(cls) === cls` self-cycle.
                if (*gc).obj_type == crate::gc::GC_TYPE_CLOSURE {
                    if let Some(proto_bits) =
                        crate::closure::closure_static_prototype(raw_addr as usize)
                    {
                        return f64::from_bits(proto_bits);
                    }
                    return f64::from_bits(TAG_NULL);
                }
                if let Some(proto) = constructor_dynamic_prototype(obj) {
                    return proto;
                }
            }
            return obj_value;
        }
    }
    if top16 == 0 {
        if bits >= (crate::gc::GC_HEADER_SIZE as u64) + 0x1000 {
            unsafe {
                let obj = bits as *const ObjectHeader;
                let gc = gc_header_for(obj);
                if (*gc)._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0 {
                    return f64::from_bits(TAG_NULL);
                }
                if (*gc)._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0 {
                    let p = crate::object::typed_array_intrinsic_proto_ptr();
                    if !p.is_null() {
                        return f64::from_bits(crate::value::js_nanbox_pointer(p as i64).to_bits());
                    }
                }
                // #489 / #2145: function/constructor receiver — see the
                // 0x7FFD branch above. Return the recorded static
                // prototype if any, else null to break the chain-walk
                // self-cycle.
                if (*gc).obj_type == crate::gc::GC_TYPE_CLOSURE {
                    if let Some(proto_bits) =
                        crate::closure::closure_static_prototype(bits as usize)
                    {
                        return f64::from_bits(proto_bits);
                    }
                    return f64::from_bits(TAG_NULL);
                }
                if let Some(proto) = constructor_dynamic_prototype(obj) {
                    return proto;
                }
            }
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
pub extern "C" fn js_object_set_prototype_of(obj_value: f64, proto: f64) -> f64 {
    // #36 / #321: when the target is a closure (a plain function value) and the
    // proto is an object, record the (closure → proto) link in the closure
    // static-prototype side-table. effect's `Context.Tag(id)` returns a
    // function `TagClass` whose `_op`/`[TagTypeId]`/`[EffectTypeId]` live on a
    // `TagProto` object wired in via `Object.setPrototypeOf(TagClass,
    // TagProto)`. Recording the link lets later string/symbol property reads on
    // the closure (and on a subclass that `extends TagClass`) walk to the
    // proto's own properties, so the Tag is recognized as a valid Effect.
    let obj_bits = obj_value.to_bits();
    let proto_bits = proto.to_bits();
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    if (obj_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG
        && (proto_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG
    {
        let obj_ptr = crate::value::js_nanbox_get_pointer(obj_value) as usize;
        let proto_ptr = crate::value::js_nanbox_get_pointer(proto) as usize;
        if obj_ptr != 0 && proto_ptr != 0 && crate::closure::is_closure_ptr(obj_ptr) {
            crate::closure::closure_set_static_prototype(obj_ptr, proto_bits);
        }
    }
    // Spec: `Object.setPrototypeOf(O, proto)` returns O.
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

#[cfg(test)]
mod sso_tests_1781 {
    use super::*;

    #[test]
    fn get_own_property_names_array_and_string_no_crash() {
        // Regression: getOwnPropertyNames on an array/string read a bogus
        // keys_array off the wrong header and segfaulted. Now returns the
        // index names + "length".
        let arr = crate::array::js_array_alloc(4);
        for v in [10.0, 20.0, 30.0] {
            crate::array::js_array_push_f64(arr, v);
        }
        let arr_val = crate::value::js_nanbox_pointer(arr as i64);
        let names = js_object_get_own_property_names(arr_val);
        let names_ptr =
            (names.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::array::ArrayHeader;
        // 3 indices + "length".
        assert_eq!(crate::array::js_array_length(names_ptr), 4);

        let s = crate::string::js_string_from_bytes(b"ab".as_ptr(), 2);
        let s_val = crate::value::js_nanbox_string(s as i64);
        let s_names = js_object_get_own_property_names(s_val);
        let s_ptr = (s_names.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::array::ArrayHeader;
        assert_eq!(crate::array::js_array_length(s_ptr), 3); // "0","1","length"
    }

    /// #1781: `Object.is` content-compares strings only when both sides pass
    /// `is_string()` (STRING_TAG-only). Two SSO operands match via the
    /// bit-pattern fallback, but a mixed SSO/heap pair with equal content
    /// (e.g. a JSON-parsed value vs a heap literal) did not — `Object.is`
    /// wrongly returned false. Now representation-independent.
    #[test]
    fn object_is_compares_sso_and_mixed_strings() {
        let truthy = |v: f64| crate::value::js_is_truthy(v) != 0;
        let a = JSValue::try_short_string(b"abc").unwrap();
        let b = JSValue::try_short_string(b"abc").unwrap();
        assert!(
            truthy(js_object_is(
                f64::from_bits(a.bits()),
                f64::from_bits(b.bits())
            )),
            "two equal SSO strings"
        );

        let heap = JSValue::string_ptr(crate::string::js_string_from_bytes(b"abc".as_ptr(), 3));
        assert!(
            truthy(js_object_is(
                f64::from_bits(a.bits()),
                f64::from_bits(heap.bits())
            )),
            "mixed SSO/heap, equal content"
        );

        let c = JSValue::try_short_string(b"xyz").unwrap();
        assert!(
            !truthy(js_object_is(
                f64::from_bits(a.bits()),
                f64::from_bits(c.bits())
            )),
            "different content"
        );
    }

    /// #1781: an object with an inline-SSO key must answer
    /// `hasOwnProperty("id")` truthfully. Pre-fix the
    /// `is_string()`-gated keys-array iteration in `own_key_present`
    /// skipped the SSO key silently and the call returned false.
    #[test]
    fn own_key_present_finds_sso_stored_key() {
        unsafe {
            let obj = super::super::alloc::js_object_alloc(0, 4);
            // Build a keys array with a single SSO-tagged key directly
            // (skipping `js_object_set_field_by_name`, which would
            // intern the key to heap and bypass the SSO blind spot
            // we're regression-testing).
            let keys = crate::array::js_array_alloc(4);
            let sso = JSValue::try_short_string(b"id").expect("SSO");
            crate::array::js_array_push_f64(keys, f64::from_bits(sso.bits()));
            super::super::set_object_keys_array(obj, keys);

            let incoming = crate::string::js_string_from_bytes(b"id".as_ptr(), 2);
            assert!(
                own_key_present(obj, incoming),
                "SSO key 'id' should be visible to own_key_present"
            );

            let incoming_other = crate::string::js_string_from_bytes(b"tag".as_ptr(), 3);
            assert!(
                !own_key_present(obj, incoming_other),
                "absent key 'tag' must not match"
            );
        }
    }
}
