//! `JSON.parse(text, reviver)` — applies a user-supplied reviver function
//! to every property of the parsed value (post-order, root last).

use super::*;
use crate::value::TAG_UNDEFINED;
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

unsafe fn create_data_property(holder: f64, key: f64, value: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let holder_handle = scope.root_nanbox_f64(holder);
    let key_handle = scope.root_nanbox_f64(key);
    let value_handle = scope.root_nanbox_f64(value);
    let descriptor =
        crate::object::build_data_descriptor(value_handle.get_nanbox_f64(), true, true, true);
    let descriptor_handle = scope.root_nanbox_f64(descriptor);
    let _ = crate::proxy::js_reflect_define_property(
        holder_handle.get_nanbox_f64(),
        key_handle.get_nanbox_f64(),
        descriptor_handle.get_nanbox_f64(),
    );
}

unsafe fn delete_property(holder: f64, key: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let holder_handle = scope.root_nanbox_f64(holder);
    let key_handle = scope.root_nanbox_f64(key);
    let _ = crate::proxy::js_reflect_delete(
        holder_handle.get_nanbox_f64(),
        key_handle.get_nanbox_f64(),
    );
}

unsafe fn call_reviver(
    holder: f64,
    key: f64,
    value: f64,
    reviver: *const crate::closure::ClosureHeader,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let holder_handle = scope.root_nanbox_f64(holder);
    let key_handle = scope.root_nanbox_f64(key);
    let value_handle = scope.root_nanbox_f64(value);
    let reviver_handle = scope.root_raw_const_ptr(reviver);
    let reviver_bits = POINTER_TAG
        | (reviver_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>() as u64
            & POINTER_MASK);
    let reviver_value_handle = scope.root_nanbox_u64(reviver_bits);
    let rebound_bits = crate::closure::clone_closure_rebind_this(
        reviver_value_handle.get_nanbox_u64(),
        holder_handle.get_nanbox_f64(),
    );
    let rebound_handle = scope.root_nanbox_u64(rebound_bits);
    let rebound =
        (rebound_handle.get_nanbox_u64() & POINTER_MASK) as *const crate::closure::ClosureHeader;
    let prev_this = crate::object::js_implicit_this_set(holder_handle.get_nanbox_f64());
    let result = crate::js_closure_call2(
        rebound,
        key_handle.get_nanbox_f64(),
        value_handle.get_nanbox_f64(),
    );
    crate::object::js_implicit_this_set(prev_this);

    let result_bits = result.to_bits();
    if result_bits == key_handle.get_nanbox_f64().to_bits() {
        key_handle.get_nanbox_f64()
    } else if result_bits == value_handle.get_nanbox_f64().to_bits() {
        value_handle.get_nanbox_f64()
    } else if result_bits == holder_handle.get_nanbox_f64().to_bits() {
        holder_handle.get_nanbox_f64()
    } else {
        result
    }
}

unsafe fn internalize_array_elements(
    array_value: f64,
    reviver: *const crate::closure::ClosureHeader,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let array_handle = scope.root_nanbox_f64(array_value);
    let reviver_handle = scope.root_raw_const_ptr(reviver);
    let Some(ptr) = extract_pointer(array_handle.get_nanbox_u64()) else {
        return;
    };
    let arr = ptr as *const crate::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    for i in 0..len {
        let idx = i.to_string();
        let key = js_string_from_bytes(idx.as_ptr(), idx.len() as u32);
        let key_handle = scope.root_nanbox_f64(nanbox_string_f64(key));
        let _ = internalize_json_property(
            array_handle.get_nanbox_f64(),
            key_handle.get_nanbox_f64(),
            reviver_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
        );
    }
}

unsafe fn internalize_object_properties(
    object_value: f64,
    reviver: *const crate::closure::ClosureHeader,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let object_handle = scope.root_nanbox_f64(object_value);
    let reviver_handle = scope.root_raw_const_ptr(reviver);
    let Some(ptr) = extract_pointer(object_handle.get_nanbox_u64()) else {
        return;
    };
    let keys = crate::object::js_object_keys(ptr as *const crate::ObjectHeader);
    if keys.is_null() {
        return;
    }
    let keys_handle = scope.root_raw_mut_ptr(keys);
    let len = crate::array::js_array_length(keys_handle.get_raw_const_ptr::<crate::ArrayHeader>());
    for i in 0..len {
        let key = crate::array::js_array_get_f64(
            keys_handle.get_raw_const_ptr::<crate::ArrayHeader>(),
            i,
        );
        let key_handle = scope.root_nanbox_f64(key);
        let _ = internalize_json_property(
            object_handle.get_nanbox_f64(),
            key_handle.get_nanbox_f64(),
            reviver_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
        );
    }
}

unsafe fn internalize_json_property(
    holder: f64,
    key: f64,
    reviver: *const crate::closure::ClosureHeader,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let holder_handle = scope.root_nanbox_f64(holder);
    let key_handle = scope.root_nanbox_f64(key);
    let reviver_handle = scope.root_raw_const_ptr(reviver);

    let value = crate::proxy::js_reflect_get(
        holder_handle.get_nanbox_f64(),
        key_handle.get_nanbox_f64(),
        holder_handle.get_nanbox_f64(),
    );
    let materialized = force_materialize_if_lazy(JSValue::from_bits(value.to_bits()));
    let value_handle = scope.root_nanbox_u64(materialized.bits());
    if materialized.bits() != value.to_bits() {
        create_data_property(
            holder_handle.get_nanbox_f64(),
            key_handle.get_nanbox_f64(),
            value_handle.get_nanbox_f64(),
        );
    }

    if let Some(ptr) = extract_pointer(value_handle.get_nanbox_u64()) {
        match gc_obj_type(ptr) {
            crate::gc::GC_TYPE_ARRAY => internalize_array_elements(
                value_handle.get_nanbox_f64(),
                reviver_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
            ),
            crate::gc::GC_TYPE_OBJECT => internalize_object_properties(
                value_handle.get_nanbox_f64(),
                reviver_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
            ),
            _ => {}
        }
    }

    let revived = call_reviver(
        holder_handle.get_nanbox_f64(),
        key_handle.get_nanbox_f64(),
        value_handle.get_nanbox_f64(),
        reviver_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
    );
    let revived_handle = scope.root_nanbox_f64(revived);
    if revived_handle.get_nanbox_u64() == TAG_UNDEFINED {
        delete_property(holder_handle.get_nanbox_f64(), key_handle.get_nanbox_f64());
    } else {
        create_data_property(
            holder_handle.get_nanbox_f64(),
            key_handle.get_nanbox_f64(),
            revived_handle.get_nanbox_f64(),
        );
    }
    revived_handle.get_nanbox_f64()
}

/// Apply reviver to a parsed JSON value using the spec's holder/key
/// InternalizeJSONProperty traversal. For JSON.parse's root, key is "".
pub(crate) unsafe fn apply_reviver(
    value: JSValue,
    key_f64: f64,
    reviver: *const crate::closure::ClosureHeader,
) -> JSValue {
    let value = force_materialize_if_lazy(value);
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_u64(value.bits());
    let key_handle = scope.root_nanbox_f64(key_f64);
    let reviver_handle = scope.root_raw_const_ptr(reviver);
    let wrapper = crate::object::js_object_alloc(0, 0);
    let wrapper_handle = scope.root_raw_mut_ptr(wrapper);
    let wrapper_value = nanbox_pointer_f64(wrapper_handle.get_raw_const_ptr::<u8>());
    let wrapper_value_handle = scope.root_nanbox_f64(wrapper_value);
    create_data_property(
        wrapper_value_handle.get_nanbox_f64(),
        key_handle.get_nanbox_f64(),
        value_handle.get_nanbox_f64(),
    );
    let result = internalize_json_property(
        wrapper_value_handle.get_nanbox_f64(),
        key_handle.get_nanbox_f64(),
        reviver_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
    );
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let text_handle = scope.root_string_ptr(text_ptr);
    let reviver = reviver_ptr as *const crate::closure::ClosureHeader;
    let reviver_handle = scope.root_raw_const_ptr(reviver);

    // First, parse normally
    let parsed = js_json_parse(text_handle.get_raw_const_ptr::<StringHeader>());
    let parsed_handle = scope.root_nanbox_u64(parsed.bits());

    if reviver.is_null() || (reviver_ptr as u64) < 0x1000 {
        return JSValue::from_bits(parsed_handle.get_nanbox_u64());
    }

    // Apply reviver starting from root
    let empty_str = js_string_from_bytes(b"".as_ptr(), 0);
    let empty_key_handle = scope.root_nanbox_f64(nanbox_string_f64(empty_str));
    apply_reviver(
        JSValue::from_bits(parsed_handle.get_nanbox_u64()),
        empty_key_handle.get_nanbox_f64(),
        reviver_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
    )
}
