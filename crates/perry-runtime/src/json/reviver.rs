//! `JSON.parse(text, reviver)` — applies a user-supplied reviver function
//! to every property of the parsed value (post-order, root last).

use super::*;
use crate::{js_string_from_bytes, JSValue, StringHeader};

// ─── JSON.parse with reviver ────────────────────────────────────────────────

/// Force-materialize a lazy-tape array (`PERRY_JSON_TAPE`) into a real
/// `ArrayHeader` tree and return a JSValue pointing at it. The reviver walk
/// below reads `length`/`capacity`/element f64s directly off the pointer — a
/// `LazyArrayHeader` has a different layout, so without this the walk reads
/// garbage and SIGSEGVs. Unlike `redirect_lazy_to_materialized` (stringify),
/// this forces materialization even when nothing has indexed the array yet.
/// No-op for non-lazy values. Refs #1424.
unsafe fn force_materialize_if_lazy(value: JSValue) -> JSValue {
    let bits = value.bits();
    if (bits >> 48) != 0x7FFD {
        return value;
    }
    let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const u8;
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return value;
    }
    let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*gc_header).obj_type != crate::gc::GC_TYPE_LAZY_ARRAY {
        return value;
    }
    let lazy = ptr as *mut crate::json_tape::LazyArrayHeader;
    if (*lazy).magic != crate::json_tape::LAZY_ARRAY_MAGIC {
        return value;
    }
    let materialized = crate::json_tape::force_materialize_lazy(lazy);
    if materialized.is_null() {
        return value;
    }
    JSValue::object_ptr(materialized as *mut u8)
}

/// Apply reviver to a parsed JSON value. The reviver is called as reviver(key, value).
/// For objects, it's called for each property; for the root, key is "".
pub(crate) unsafe fn apply_reviver(
    value: JSValue,
    key_f64: f64,
    reviver: *const crate::closure::ClosureHeader,
) -> JSValue {
    // A lazy-tape array must be materialized before the in-place element walk
    // (its header layout differs from ArrayHeader). #1424.
    let value = force_materialize_if_lazy(value);
    let bits = value.bits();

    // If value is an object, recurse into its properties first
    if let Some(ptr) = extract_pointer(bits) {
        if is_object_pointer(ptr) {
            let obj = ptr as *const crate::ObjectHeader;
            let num_fields = (*obj).field_count;
            let keys_arr = (*obj).keys_array;
            let keys_len = (*keys_arr).length;
            let keys_elements = (keys_arr as *const u8)
                .add(std::mem::size_of::<crate::ArrayHeader>())
                as *const f64;
            let fields_ptr =
                (ptr as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *mut f64;
            let actual_fields = std::cmp::min(num_fields, keys_len);

            for f in 0..actual_fields {
                let field_key_f64 = *keys_elements.add(f as usize);
                let field_val_f64 = *fields_ptr.add(f as usize);
                let child_val = JSValue::from_bits(field_val_f64.to_bits());
                let revived_child = apply_reviver(child_val, field_key_f64, reviver);
                // Write back the revived value
                *fields_ptr.add(f as usize) = f64::from_bits(revived_child.bits());
                crate::gc::layout_note_slot(obj as usize, f as usize, revived_child.bits());
            }
        } else {
            // Check if it's an array
            let arr = ptr as *const crate::ArrayHeader;
            if !arr.is_null() {
                let len = (*arr).length;
                let cap = (*arr).capacity;
                if len <= cap && cap > 0 && cap < 10000 {
                    let elements = (arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>())
                        as *mut f64;
                    for i in 0..len {
                        let idx_str = i.to_string();
                        let idx_ptr = js_string_from_bytes(idx_str.as_ptr(), idx_str.len() as u32);
                        let idx_key_f64 = nanbox_string_f64(idx_ptr);
                        let elem_f64 = *elements.add(i as usize);
                        let child_val = JSValue::from_bits(elem_f64.to_bits());
                        let revived_child = apply_reviver(child_val, idx_key_f64, reviver);
                        *elements.add(i as usize) = f64::from_bits(revived_child.bits());
                        crate::array::note_array_slot(
                            arr as *mut crate::ArrayHeader,
                            i as usize,
                            revived_child.bits(),
                        );
                    }
                }
            }
        }
    }

    // Now call reviver on this value
    let value_f64 = f64::from_bits(value.bits());
    let result = crate::js_closure_call2(reviver, key_f64, value_f64);
    JSValue::from_bits(result.to_bits())
}

#[cfg(test)]
pub(crate) unsafe fn test_apply_reviver_for_value(
    value: JSValue,
    key_f64: f64,
    reviver: *const crate::closure::ClosureHeader,
) -> JSValue {
    apply_reviver(value, key_f64, reviver)
}

/// JSON.parse(text, reviver) — parse JSON with a reviver function.
#[no_mangle]
pub unsafe extern "C" fn js_json_parse_with_reviver(
    text_ptr: *const StringHeader,
    reviver_ptr: i64,
) -> JSValue {
    // First, parse normally
    let parsed = js_json_parse(text_ptr);

    let reviver = reviver_ptr as *const crate::closure::ClosureHeader;
    if reviver.is_null() || (reviver_ptr as u64) < 0x1000 {
        return parsed;
    }

    // Apply reviver starting from root
    let empty_str = js_string_from_bytes(b"".as_ptr(), 0);
    let empty_key_f64 = nanbox_string_f64(empty_str);
    apply_reviver(parsed, empty_key_f64, reviver)
}
