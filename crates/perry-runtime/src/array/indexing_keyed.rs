//! Indexing — the string-or-index keyed entry points (`arr[key]` with a
//! runtime key) and their canonical-index classification.
//!
//! Child module of `indexing.rs`, split out to stay under the 2,000-line file
//! gate; `use super::*` keeps the parent's private helpers reachable.

use super::*;

/// `arr[stringKey] = value` — handles the JS spec rule that numeric-string
/// keys on arrays are coerced to integer indices. Pre-fix the codegen's
/// IndexSet array fast-path applied `fptosi(double, i32)` directly to the
/// NaN-boxed string value, producing garbage indices that all collapsed
/// onto slot 0 (every iteration overwrote the previous).
///
/// Spec: an "array index" is a string whose canonical numeric form is a
/// non-negative integer < 2^32-1. Such writes update the array's element
/// storage; non-numeric string keys fall through to the object-property
/// path on the array's expando map (rare).
///
/// Issue #637 followup: this helper is also called from the polymorphic
/// IndexSet dispatch when the receiver type isn't statically known —
/// the runtime detects the receiver's gc_type byte and routes to the
/// per-kind setter. For Object/Closure receivers, fall through to
/// `js_object_set_field_by_name`. For Array receivers, parse the key
/// as integer and route to `js_array_set_f64_extend`.
#[no_mangle]
pub extern "C" fn js_array_set_string_key(
    arr: *mut ArrayHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> *mut ArrayHeader {
    if arr.is_null() || key.is_null() {
        return arr;
    }
    // A class-ref value (INT32 tag 0x7FFE) reaching this polymorphic setter
    // (`C[name] = v` where `C` is a runtime class-ref value) is not an array —
    // its high bits are set, so the `is_array` GC-header probe below would
    // dereference unmapped memory. Route to the by-name object setter, which
    // detects the class-ref tag and stores into the static-field tables.
    if (arr as u64) >> 48 == 0x7FFE {
        crate::object::js_object_set_field_by_name(
            arr as *mut crate::object::ObjectHeader,
            key,
            value,
        );
        return arr;
    }
    // Issue #637: also called from polymorphic IndexSet — detect the
    // receiver's gc_type and route accordingly. For Object/Closure
    // (non-array) receivers, just call the object setter directly so
    // the standard expando-property path runs.
    let is_array = unsafe {
        if (arr as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let gc_header =
                (arr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY
        } else {
            false
        }
    };
    if !is_array {
        crate::object::js_object_set_field_by_name(
            arr as *mut crate::object::ObjectHeader,
            key,
            value,
        );
        return arr;
    }
    // Read the key as a Rust &str via the standard StringHeader layout.
    let key_str = unsafe {
        let len = (*key).byte_len as usize;
        if len == 0 {
            return arr;
        }
        let data = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        match std::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => return arr,
        }
    };
    // `length` is a real own property of every array — a polymorphic /
    // computed string-key write (`arr["length"] = n`, or an `Object.assign`
    // copying a source's own `length` onto an array target) must resize the
    // array (truncate / extend + holes), NOT land as an inert expando. The
    // dedicated `arr.length = n` codegen path already routes to
    // `js_array_set_length`; this covers the by-string-key entry points.
    // (test262 Object/assign/target-Array: `Object.assign([7,8,9], {1:2,
    // length:2})` truncates the target to `[1,2]`.)
    if key_str == "length" {
        js_array_set_length(arr, value);
        return arr;
    }
    // Try parse as a non-negative integer in array-index range.
    if let Ok(idx) = key_str.parse::<u32>() {
        // Reject leading zeros / signs that would round-trip differently
        // (e.g. "01" -> 1, but the canonical form is "1"; per spec only
        // "1" is a valid array index, "01" is a generic property).
        let canonical = idx.to_string();
        if canonical == key_str && idx < u32::MAX {
            return js_array_set_f64_extend(arr, idx, value);
        }
    }
    if array_is_frozen(arr) {
        return arr;
    }
    let existing = unsafe { array_named_property_get(arr, key).is_some() };
    if !existing && array_is_sealed_or_no_extend(arr) {
        return arr;
    }
    // Named accessor installed via `Object.defineProperty(arr, "prop",
    // {get,set})`: dispatch the setter instead of the expando store.
    if array_object_flags(arr) & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0 {
        if let Some(acc) = crate::object::get_accessor_descriptor(arr as usize, key_str) {
            if acc.set != 0 {
                unsafe {
                    crate::object::invoke_accessor_setter(
                        acc.set,
                        crate::value::js_nanbox_pointer(arr as i64),
                        value,
                    );
                }
            }
            return arr;
        }
    }
    if let Some(attrs) = crate::object::get_property_attrs(arr as usize, key_str) {
        if !attrs.writable() {
            return arr;
        }
    }
    // Non-numeric string key — fall through to object-property set on the
    // array's expando map. Arrays with named properties are rare but spec-
    // legal.
    unsafe {
        array_named_property_set(arr, key, value);
    }
    arr
}

/// `arr[idx]` where `idx` may be a number or property-key value. This mirrors
/// `js_array_set_index_or_string` for read paths that cannot safely narrow the
/// key through i32 codegen.
#[no_mangle]
pub extern "C" fn js_array_get_index_or_string(arr: *const ArrayHeader, idx: f64) -> f64 {
    if arr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let bits = idx.to_bits();
    let top16 = bits >> 48;
    if top16 == 0x7FFF {
        let key = (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader;
        return array_get_property_by_key(arr, key);
    }
    if top16 == 0x7FF9 {
        let key = crate::value::js_get_string_pointer_unified(idx) as *const crate::StringHeader;
        return array_get_property_by_key(arr, key);
    }

    let numeric = if (bits & crate::value::TAG_MASK) == crate::value::INT32_TAG {
        Some(crate::value::JSValue::from_bits(bits).as_int32() as f64)
    } else if !(0x7FF8..=0x7FFF).contains(&top16) {
        Some(idx)
    } else {
        None
    };
    if let Some(n) = numeric {
        if n.is_finite() && n.trunc() == n && n >= 0.0 && n < u32::MAX as f64 {
            return js_array_get_f64(arr, n as u32);
        }
        if n.is_finite() && n.trunc() == n {
            let key = if n == 0.0 {
                "0".to_string()
            } else {
                format!("{:.0}", n)
            };
            // #6935: `js_string_from_bytes` ALLOCATES, so it can trigger a GC
            // that evacuates the receiver; `arr` is a bare Rust local.
            let scope = crate::gc::RuntimeHandleScope::new();
            let arr_handle = scope.root_raw_const_ptr(arr);
            // Allocating key build + receiver re-read as one combinator (#7341).
            let (key_ptr, arr_now) = arr_handle.across_const::<ArrayHeader, _>(|| {
                crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32)
            });
            return array_get_property_by_key(arr_now, key_ptr);
        }
    }

    if unsafe { crate::symbol::js_is_symbol(idx) } != 0 {
        // Symbol-keyed read on an array: `arr[sym] = v` stores into the
        // symbol side table keyed by the header address (write arm in
        // `js_array_set_index_or_string`), so read it back through the
        // standard symbol getter — which also serves an accessor installed
        // via `defineProperty(arr, sym, {get})`. This used to hard-return
        // `undefined`, making every stored symbol property unreadable
        // (test262 getOwnPropertySymbols/order-after-define-property,
        // Array-receiver half).
        return unsafe {
            crate::symbol::js_object_get_symbol_property(
                crate::value::js_nanbox_pointer(arr as i64),
                idx,
            )
        };
    }
    // #6935: read-side sibling of `js_array_set_index_or_string` below —
    // `js_jsvalue_to_string` on an object key (`a[new Number(1)]`,
    // `a[{toString(){...}}]`) runs user JS, allocates and can evacuate `arr`.
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_const_ptr(arr);
    let key = crate::value::js_jsvalue_to_string(idx);
    if key.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    array_get_property_by_key(
        arr_handle.get_raw_const_ptr::<ArrayHeader>(),
        key as *const crate::StringHeader,
    )
}

/// `arr[idx] = value` where idx may be a NaN-boxed string (numeric-string
/// key) OR a number. Dispatches at runtime: string tags → parse and route
/// to `js_array_set_string_key`; otherwise treat as numeric and route to
/// `js_array_set_f64_extend`. Issue #637 followup: the array fast-path's
/// `fptosi(idx_double, i32)` collapsed every NaN-boxed string to slot 0
/// (NaN→i32 = 0 on most platforms), so `forEach((k) => arr[k] = ...)`
/// over `["0","1","2"]` overwrote slot 0 three times. Codegen routes
/// the array fast-path here when the index expression isn't statically
/// numeric.
#[no_mangle]
pub extern "C" fn js_array_set_index_or_string(
    arr: *mut ArrayHeader,
    idx: f64,
    value: f64,
) -> *mut ArrayHeader {
    if arr.is_null() {
        return arr;
    }
    let bits = idx.to_bits();
    let top16 = bits >> 48;
    // STRING_TAG (0x7FFF) heap pointer — dispatch through the string-key
    // helper which parses the numeric value and routes appropriately.
    // SHORT_STRING_TAG (0x7FF9) is the SSO variant; same path via
    // `js_get_string_pointer_unified` — handled inside `js_string_*` helpers.
    if top16 == 0x7FFF {
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader;
        return js_array_set_string_key(arr, ptr, value);
    }
    if top16 == 0x7FF9 {
        // SHORT_STRING_TAG (SSO). Materialize as a real StringHeader
        // via `js_get_string_pointer_unified` so `js_array_set_string_key`
        // can read the bytes through the standard layout.
        let str_ptr =
            crate::value::js_get_string_pointer_unified(idx) as *const crate::StringHeader;
        return js_array_set_string_key(arr, str_ptr, value);
    }
    // Treat numeric keys according to the array-index boundary. Only
    // integers in 0..2^32-2 extend element storage; 2^32-1 and larger are
    // ordinary string properties.
    let numeric = if (bits & crate::value::TAG_MASK) == crate::value::INT32_TAG {
        Some(crate::value::JSValue::from_bits(bits).as_int32() as f64)
    } else if !(0x7FF8..=0x7FFF).contains(&top16) {
        Some(idx)
    } else {
        None
    };
    if let Some(n) = numeric {
        if n.is_finite() && n.trunc() == n && n >= 0.0 && n < u32::MAX as f64 {
            return js_array_set_f64_extend(arr, n as u32, value);
        }
        // Any other finite/non-finite number that is NOT a canonical array
        // index (2^32-1 and above, negatives, and non-integer floats such as
        // `a[1.5]`) becomes an ordinary string property. Route through
        // `js_jsvalue_to_string` so the key is the spec ToString of the
        // number ("4294967295", "-1", "1.5", "NaN") rather than a truncated
        // integer — `js_array_set_string_key` then stores it on the expando
        // map without touching `length` or any element slot. (Issue #4543.)
        // #6935: `js_jsvalue_to_string` allocates the stringified key, so it can
        // GC and evacuate both the receiver and the value being stored.
        let scope = crate::gc::RuntimeHandleScope::new();
        let arr_handle = scope.root_raw_mut_ptr(arr);
        let value_handle = scope.root_nanbox_f64(value);
        let key = crate::value::js_jsvalue_to_string(idx);
        if !key.is_null() {
            return js_array_set_string_key(
                arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
                key as *const crate::StringHeader,
                value_handle.get_nanbox_f64(),
            );
        }
        return arr_handle.get_raw_mut_ptr::<ArrayHeader>();
    }
    // Symbol-keyed write: store through the symbol side table (keyed by the
    // header address), exactly like a plain-object receiver. This arm used to
    // be missing — a symbol key fell past the string fallback below (guarded
    // `js_is_symbol == 0`) to the final bare return, so the write was
    // silently DROPPED and `arr[sym]` / `getOwnPropertySymbols(arr)` saw
    // nothing (test262 getOwnPropertySymbols/order-after-define-property,
    // Array-receiver half).
    if unsafe { crate::symbol::js_is_symbol(idx) } != 0 {
        // The store can run a user setter (symbol accessor installed on the
        // array), which can GC and evacuate the receiver.
        let scope = crate::gc::RuntimeHandleScope::new();
        let arr_handle = scope.root_raw_mut_ptr(arr);
        unsafe {
            crate::symbol::js_object_set_symbol_property(
                crate::value::js_nanbox_pointer(arr as i64),
                idx,
                value,
            );
        }
        return arr_handle.get_raw_mut_ptr::<ArrayHeader>();
    }
    // Fallback for a NON-numeric key: a primitive (`a[null]`, `a[undefined]`,
    // `a[true]`, `a[10n]`) or a boxed object (`a[new Number(1)]`). Per
    // ToPropertyKey these become string property keys (or, for `10n`, the
    // canonical index "10"); `js_array_set_string_key` routes accordingly.
    // Arrays previously DROPPED these writes (plain objects handled them).
    // Restricted to `numeric.is_none()`: numeric keys (including non-integer
    // finite floats) are handled above. Symbols are handled by the arm above.
    //
    // #6935: this is the boxed-object arm the doc comment above names, so
    // `js_jsvalue_to_string` here runs a USER `toString` / `valueOf` — allocate
    // → GC → evacuation. Pre-fix `arr` and `value` were both raw Rust locals
    // across it, so a stale receiver dropped the write and a stale `value`
    // stored a dangling pointer inside a live array.
    if numeric.is_none() && unsafe { crate::symbol::js_is_symbol(idx) } == 0 {
        let scope = crate::gc::RuntimeHandleScope::new();
        let arr_handle = scope.root_raw_mut_ptr(arr);
        let value_handle = scope.root_nanbox_f64(value);
        let key = crate::value::js_jsvalue_to_string(idx);
        if !key.is_null() {
            return js_array_set_string_key(
                arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
                key as *const crate::StringHeader,
                value_handle.get_nanbox_f64(),
            );
        }
        return arr_handle.get_raw_mut_ptr::<ArrayHeader>();
    }
    arr
}

/// Strict-mode `arr[key] = v` (dynamic index-or-string key) — `Set` with
/// `Throw = true`. For a canonical numeric index this enforces the frozen /
/// non-extensible guard (throwing a TypeError) before delegating; non-index
/// keys fall through to the ordinary path unchanged. This is the assignment
/// entry point behind `js_typed_feedback_array_set_index_or_string`; the plain
/// `js_array_set_index_or_string` keeps its silent contract for any internal
/// caller. test262 built-ins/Array element-write-on-frozen (string/dynamic key).
#[no_mangle]
pub extern "C" fn js_array_set_index_or_string_strict(
    arr: *mut ArrayHeader,
    idx: f64,
    value: f64,
) -> *mut ArrayHeader {
    if !arr.is_null() {
        // Resolve the canonical array-index interpretation of the key (mirrors
        // the numeric branch of `js_array_set_index_or_string`), and guard it.
        // A string key that spells an index (`"0"`) also targets the element
        // store, so ToString it and re-parse.
        let index = canonical_index_of_set_key(idx);
        if let Some(i) = index {
            // The non-strict dispatcher would parse the same key again and
            // then enter the extend helper after a separate strict guard. The
            // canonical index is already proved here, so use the fused strict
            // element path and share one receiver resolution across policy
            // and store.
            return js_array_set_f64_extend_strict(arr, i, value);
        }
    }
    js_array_set_index_or_string(arr, idx, value)
}

/// The canonical array index (`0..2^32-1`) a dynamic `arr[key] = v` key targets,
/// or `None` for a non-index key. Numbers use the array-index boundary; string
/// keys are parsed via their ToString so `arr["3"]` on a frozen array throws
/// like `arr[3]`.
fn canonical_index_of_set_key(idx: f64) -> Option<u32> {
    let bits = idx.to_bits();
    let top16 = bits >> 48;
    // Heap string / SSO key: parse the string as a canonical index.
    if top16 == 0x7FFF || top16 == 0x7FF9 {
        let s = crate::value::js_get_string_pointer_unified(idx) as *const crate::StringHeader;
        if s.is_null() {
            return None;
        }
        let len = unsafe { (*s).byte_len as usize };
        let data = unsafe { (s as *const u8).add(std::mem::size_of::<crate::StringHeader>()) };
        let bytes = unsafe { std::slice::from_raw_parts(data, len) };
        let name = std::str::from_utf8(bytes).ok()?;
        return crate::object::canonical_array_index(name);
    }
    // Numeric key.
    let n = if (bits & crate::value::TAG_MASK) == crate::value::INT32_TAG {
        crate::value::JSValue::from_bits(bits).as_int32() as f64
    } else if !(0x7FF8..=0x7FFF).contains(&top16) {
        idx
    } else {
        return None;
    };
    if n.is_finite() && n.trunc() == n && n >= 0.0 && n < u32::MAX as f64 {
        Some(n as u32)
    } else {
        None
    }
}
