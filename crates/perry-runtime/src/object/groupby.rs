//! `Object.groupBy` / `Map.groupBy` (#2777/#2779) — split out of
//! `object_ops.rs` to keep it under the 2000-line cap. Pure relocation;
//! `use super::*` gives the same visibility the parent module has.

use super::*;

/// `Object.groupBy(items, callback)` — Node 22+ static method.
///
/// Consumes any iterable `items`, calls `callback(value, index)` to compute
/// a key per element, converts the key with ToPropertyKey (Symbols stay
/// Symbol keys, everything else is coerced to a String), and returns a new
/// **null-prototype** object whose keys are the distinct callback results
/// and whose values are arrays of the items that produced each key.
/// Insertion order of first-seen keys is preserved.
///
/// Throws `TypeError` for nullish `items` or a non-callable `callback`.
/// Returns the result object as a NaN-boxed POINTER_TAG f64.
#[no_mangle]
pub extern "C" fn js_object_group_by(items_value: f64, callback: f64) -> f64 {
    unsafe {
        let pairs = group_by_collect(items_value, callback, b"Object.groupBy");

        // Coalesce by ToPropertyKey. String keys group by string contents;
        // Symbol keys group by Symbol identity (pointer).
        use std::collections::HashMap;
        enum Key {
            Str(String),
            Sym(f64),
        }
        let mut str_index: HashMap<String, usize> = HashMap::new();
        let mut sym_index: HashMap<u64, usize> = HashMap::new();
        let mut order: Vec<Key> = Vec::new();
        let mut groups: Vec<Vec<f64>> = Vec::new();

        for (key_val, item) in pairs {
            if group_by_value_is_symbol(key_val) {
                let raw = crate::value::js_nanbox_get_pointer(key_val) as u64;
                let idx = *sym_index.entry(raw).or_insert_with(|| {
                    order.push(Key::Sym(key_val));
                    groups.push(Vec::new());
                    groups.len() - 1
                });
                groups[idx].push(item);
            } else {
                let key_ptr = crate::builtins::js_string_coerce(key_val);
                let key_string = if key_ptr.is_null() {
                    "undefined".to_string()
                } else {
                    let len = (*key_ptr).byte_len as usize;
                    let data = (key_ptr as *const u8)
                        .add(std::mem::size_of::<crate::string::StringHeader>());
                    let bytes = std::slice::from_raw_parts(data, len);
                    std::str::from_utf8(bytes).unwrap_or("").to_string()
                };
                let key_clone = key_string.clone();
                let idx = *str_index.entry(key_string).or_insert_with(|| {
                    order.push(Key::Str(key_clone));
                    groups.push(Vec::new());
                    groups.len() - 1
                });
                groups[idx].push(item);
            }
        }

        // Null-prototype result object (Node: getPrototypeOf === null).
        let obj = js_object_alloc_null_proto(0, order.len() as u32);
        if obj.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        let obj_boxed = f64::from_bits((obj as u64) | 0x7FFD_0000_0000_0000);
        for (idx, key) in order.iter().enumerate() {
            let arr = group_by_make_array(&groups[idx]);
            let arr_boxed = f64::from_bits((arr as u64) | 0x7FFD_0000_0000_0000);
            match key {
                Key::Str(s) => {
                    let key_str_ptr =
                        crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                    js_object_set_field_by_name(obj, key_str_ptr, arr_boxed);
                }
                Key::Sym(sym_val) => {
                    crate::symbol::js_object_set_symbol_property(obj_boxed, *sym_val, arr_boxed);
                }
            }
        }
        obj_boxed
    }
}

/// `Map.groupBy(items, callback)` — Node 22+ static method.
///
/// Consumes any iterable `items`, calls `callback(value, index)` per element,
/// and groups elements into a new `Map` keyed by the callback results
/// **without coercion** (numbers, objects, and Symbols all retain identity
/// via SameValueZero). Values are arrays of the grouped items, in first-seen
/// key order.
///
/// Throws `TypeError` for nullish `items` or a non-callable `callback`.
/// Returns the result Map as a NaN-boxed POINTER_TAG f64.
#[no_mangle]
pub extern "C" fn js_map_group_by(items_value: f64, callback: f64) -> f64 {
    unsafe {
        let pairs = group_by_collect(items_value, callback, b"Map.groupBy");

        let map = crate::map::js_map_alloc(0);
        if map.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }

        // Coalesce by SameValueZero. Collect groups in first-seen order, then
        // materialize into the Map at the end so per-push array reallocation
        // never invalidates a value stored inside the Map.
        let mut order: Vec<f64> = Vec::new();
        let mut groups: Vec<Vec<f64>> = Vec::new();

        'outer: for (key_val, item) in pairs {
            for (idx, existing) in order.iter().enumerate() {
                if crate::value::js_jsvalue_same_value_zero(*existing, key_val) != 0 {
                    groups[idx].push(item);
                    continue 'outer;
                }
            }
            order.push(key_val);
            groups.push(vec![item]);
        }

        for (idx, key_val) in order.iter().enumerate() {
            let arr = group_by_make_array(&groups[idx]);
            let arr_boxed = f64::from_bits((arr as u64) | 0x7FFD_0000_0000_0000);
            crate::map::js_map_set(map, *key_val, arr_boxed);
        }

        f64::from_bits((map as u64) | 0x7FFD_0000_0000_0000)
    }
}

/// Keepalive anchors: these `#[no_mangle]` helpers are only called from
/// codegen-emitted `.o`. The auto-optimize whole-program LLVM rebuild
/// dead-strips unreferenced `#[no_mangle]` symbols (see #3320), so pin them.
#[used]
static KEEP_OBJECT_GROUP_BY: extern "C" fn(f64, f64) -> f64 = js_object_group_by;
#[used]
static KEEP_MAP_GROUP_BY: extern "C" fn(f64, f64) -> f64 = js_map_group_by;

/// Returns true if `value` is a Symbol (registered SymbolHeader pointer).
unsafe fn group_by_value_is_symbol(value: f64) -> bool {
    let raw = crate::value::js_nanbox_get_pointer(value);
    raw != 0 && crate::symbol::is_registered_symbol(raw as usize)
}

/// Shared grouping core for `Object.groupBy` / `Map.groupBy`.
///
/// Validates `items` (nullish → TypeError) and `callback` (non-callable →
/// TypeError), materializes `items` through the iterator protocol (so Sets,
/// strings, and custom iterables all work), then calls
/// `callback(value, index)` for each element. Returns the per-element
/// `(raw_key, item)` pairs in iteration order. The caller decides how to
/// coalesce keys (ToPropertyKey for Object, SameValueZero for Map).
unsafe fn group_by_collect(items_value: f64, callback: f64, callee_name: &[u8]) -> Vec<(f64, f64)> {
    let items_jv = crate::value::JSValue::from_bits(items_value.to_bits());
    if items_jv.is_null() || items_jv.is_undefined() {
        // Match Node: "X.groupBy called on null or undefined"
        let mut msg = callee_name.to_vec();
        msg.extend_from_slice(b" called on null or undefined");
        throw_group_by_type_error(&msg);
    }
    if !group_by_value_is_callable(callback) {
        throw_group_by_type_error(b"callback is not a function");
    }

    // Materialize any iterable into an Array via the iterator protocol.
    let arr_boxed = crate::array::js_for_of_to_array(items_value);
    let raw = crate::value::js_nanbox_get_pointer(arr_boxed) as *const ArrayHeader;
    if raw.is_null() {
        return Vec::new();
    }
    let length = crate::array::js_array_length(raw) as usize;
    let cb_ptr =
        crate::value::js_nanbox_get_pointer(callback) as *const crate::closure::ClosureHeader;

    let mut out: Vec<(f64, f64)> = Vec::with_capacity(length);
    for i in 0..length {
        let item = crate::array::js_array_get_f64(raw, i as u32);
        let key_val = crate::closure::js_closure_call2(cb_ptr, item, i as f64);
        out.push((key_val, item));
    }
    out
}

/// Build an Array<f64> from a slice of grouped element values.
unsafe fn group_by_make_array(items_for_key: &[f64]) -> *mut ArrayHeader {
    let arr = crate::array::js_array_alloc(items_for_key.len() as u32);
    (*arr).length = items_for_key.len() as u32;
    let arr_data = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
    for (i, v) in items_for_key.iter().enumerate() {
        // GC_STORE_AUDIT(INIT): groupBy result array is unpublished; layout is rebuilt before publication.
        std::ptr::write(arr_data.add(i), *v);
    }
    super::rebuild_array_layout_from_slots(arr);
    arr
}

fn throw_group_by_type_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
}

/// Returns true if `value` is a callable closure.
unsafe fn group_by_value_is_callable(value: f64) -> bool {
    let raw = crate::value::js_nanbox_get_pointer(value);
    raw >= 0x10000
        && !crate::closure::get_valid_func_ptr(raw as *const crate::closure::ClosureHeader)
            .is_null()
}
