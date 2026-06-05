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

unsafe fn key_name_from_string_ptr(key: *const StringHeader) -> Option<String> {
    if key.is_null() {
        return None;
    }
    let data = (key as *const u8).add(std::mem::size_of::<StringHeader>());
    let len = (*key).byte_len as usize;
    std::str::from_utf8(std::slice::from_raw_parts(data, len))
        .ok()
        .map(|s| s.to_string())
}

unsafe fn pointer_value(bits: u64) -> f64 {
    f64::from_bits(POINTER_TAG | (bits & POINTER_MASK))
}

unsafe fn data_descriptor_value(value_handle: &crate::gc::RuntimeHandle<'_>) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let desc = crate::object::js_object_alloc(0, 4);
    let desc_handle = scope.root_raw_mut_ptr(desc);

    let value_key = js_string_from_bytes(b"value".as_ptr(), 5);
    let value_key_handle = scope.root_string_ptr(value_key);
    crate::object::js_object_set_field_by_name(
        desc_handle.get_raw_mut_ptr::<crate::ObjectHeader>(),
        value_key_handle.get_raw_const_ptr::<StringHeader>(),
        value_handle.get_nanbox_f64(),
    );

    for (name, field_value) in [
        (b"writable".as_slice(), f64::from_bits(TAG_TRUE)),
        (b"enumerable".as_slice(), f64::from_bits(TAG_TRUE)),
        (b"configurable".as_slice(), f64::from_bits(TAG_TRUE)),
    ] {
        let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let key_handle = scope.root_string_ptr(key);
        crate::object::js_object_set_field_by_name(
            desc_handle.get_raw_mut_ptr::<crate::ObjectHeader>(),
            key_handle.get_raw_const_ptr::<StringHeader>(),
            field_value,
        );
    }
    pointer_value(desc_handle.get_raw_mut_ptr::<crate::ObjectHeader>() as u64)
}

unsafe fn holder_ptr_from_bits(bits: u64) -> *mut crate::ObjectHeader {
    (bits & POINTER_MASK) as *mut crate::ObjectHeader
}

unsafe fn delete_property_or_keep(
    holder_handle: &crate::gc::RuntimeHandle<'_>,
    key_handle: &crate::gc::RuntimeHandle<'_>,
) {
    let holder = holder_ptr_from_bits(holder_handle.get_nanbox_u64());
    if holder.is_null() {
        return;
    }
    let _ = crate::object::js_object_delete_field(
        holder,
        key_handle.get_raw_const_ptr::<StringHeader>(),
    );
}

unsafe fn create_data_property_or_keep(
    holder_handle: &crate::gc::RuntimeHandle<'_>,
    key_handle: &crate::gc::RuntimeHandle<'_>,
    value_handle: &crate::gc::RuntimeHandle<'_>,
) {
    let holder_addr = (holder_handle.get_nanbox_u64() & POINTER_MASK) as usize;
    if let Some(name) = key_name_from_string_ptr(key_handle.get_raw_const_ptr::<StringHeader>()) {
        if crate::object::get_property_attrs(holder_addr, &name)
            .is_some_and(|attrs| !attrs.configurable())
        {
            return;
        }
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let descriptor = data_descriptor_value(value_handle);
    let descriptor_handle = scope.root_nanbox_f64(descriptor);
    let key_value = nanbox_string_f64(key_handle.get_raw_const_ptr::<StringHeader>());
    crate::object::js_object_define_property(
        holder_handle.get_nanbox_f64(),
        key_value,
        descriptor_handle.get_nanbox_f64(),
    );
}

unsafe fn apply_internalized_child(
    holder_handle: &crate::gc::RuntimeHandle<'_>,
    key_handle: &crate::gc::RuntimeHandle<'_>,
    child: JSValue,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let child_handle = scope.root_nanbox_u64(child.bits());
    if child_handle.get_nanbox_u64() == TAG_UNDEFINED {
        delete_property_or_keep(holder_handle, key_handle);
    } else {
        create_data_property_or_keep(holder_handle, key_handle, &child_handle);
    }
}

unsafe fn internalize_array(
    value_handle: &crate::gc::RuntimeHandle<'_>,
    reviver: *const crate::closure::ClosureHeader,
) {
    let reviver_scope = crate::gc::RuntimeHandleScope::new();
    let reviver_handle = reviver_scope.root_raw_const_ptr(reviver);
    let arr = (value_handle.get_nanbox_u64() & POINTER_MASK) as *const crate::ArrayHeader;
    if arr.is_null() {
        return;
    }
    let len = (*arr).length;
    for i in 0..len {
        let iteration_scope = crate::gc::RuntimeHandleScope::new();
        let idx = i.to_string();
        let key = js_string_from_bytes(idx.as_ptr(), idx.len() as u32);
        let key_handle = iteration_scope.root_string_ptr(key);
        let key_value = nanbox_string_f64(key_handle.get_raw_const_ptr::<StringHeader>());
        let child = internalize_json_property(
            JSValue::from_bits(value_handle.get_nanbox_u64()),
            key_value,
            reviver_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
        );
        apply_internalized_child(value_handle, &key_handle, child);
    }
}

unsafe fn internalize_object(
    value_handle: &crate::gc::RuntimeHandle<'_>,
    reviver: *const crate::closure::ClosureHeader,
) {
    let reviver_scope = crate::gc::RuntimeHandleScope::new();
    let reviver_handle = reviver_scope.root_raw_const_ptr(reviver);
    let keys = crate::object::js_object_keys_value(value_handle.get_nanbox_f64());
    let scope = crate::gc::RuntimeHandleScope::new();
    let keys_handle = scope.root_raw_mut_ptr(keys);
    let len = crate::array::js_array_length(keys_handle.get_raw_mut_ptr::<crate::ArrayHeader>());
    for i in 0..len {
        let iteration_scope = crate::gc::RuntimeHandleScope::new();
        let keys = keys_handle.get_raw_mut_ptr::<crate::ArrayHeader>();
        let key_value = crate::array::js_array_get(keys, i);
        let key_value_handle = iteration_scope.root_nanbox_u64(key_value.bits());
        let key_ptr = crate::value::js_get_string_pointer_unified(key_value_handle.get_nanbox_f64())
            as *const StringHeader;
        let key_handle = iteration_scope.root_string_ptr(key_ptr);
        let key_value = nanbox_string_f64(key_handle.get_raw_const_ptr::<StringHeader>());
        let child = internalize_json_property(
            JSValue::from_bits(value_handle.get_nanbox_u64()),
            key_value,
            reviver_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
        );
        apply_internalized_child(value_handle, &key_handle, child);
    }
}

unsafe fn call_reviver(
    holder_handle: &crate::gc::RuntimeHandle<'_>,
    key_handle: &crate::gc::RuntimeHandle<'_>,
    value_handle: &crate::gc::RuntimeHandle<'_>,
    reviver: *const crate::closure::ClosureHeader,
) -> JSValue {
    let holder_arg = holder_handle.get_nanbox_f64();
    let key_arg = key_handle.get_nanbox_f64();
    let value_arg = value_handle.get_nanbox_f64();
    let prev_this = crate::object::js_implicit_this_set(holder_arg);
    let result = crate::js_closure_call2(reviver, key_arg, value_arg);
    crate::object::js_implicit_this_set(prev_this);
    let result_bits = result.to_bits();
    let revived_bits = if result_bits == value_arg.to_bits() {
        value_handle.get_nanbox_u64()
    } else if result_bits == key_arg.to_bits() {
        key_handle.get_nanbox_u64()
    } else if result_bits == holder_arg.to_bits() {
        holder_handle.get_nanbox_u64()
    } else {
        result_bits
    };
    JSValue::from_bits(revived_bits)
}

unsafe fn internalize_json_property(
    holder: JSValue,
    key_f64: f64,
    reviver: *const crate::closure::ClosureHeader,
) -> JSValue {
    let scope = crate::gc::RuntimeHandleScope::new();
    let holder_handle = scope.root_nanbox_u64(holder.bits());
    let reviver_handle = scope.root_raw_const_ptr(reviver);
    let key_handle = scope.root_nanbox_f64(key_f64);
    let key_ptr = crate::value::js_get_string_pointer_unified(key_handle.get_nanbox_f64())
        as *const StringHeader;
    let key_ptr_handle = scope.root_string_ptr(key_ptr);
    let holder_ptr = holder_ptr_from_bits(holder_handle.get_nanbox_u64());
    let value = crate::object::js_object_get_field_by_name(
        holder_ptr as *const crate::ObjectHeader,
        key_ptr_handle.get_raw_const_ptr::<StringHeader>(),
    );
    let value = force_materialize_if_lazy(value);
    let value_handle = scope.root_nanbox_u64(value.bits());

    if let Some(ptr) = extract_pointer(value_handle.get_nanbox_u64()) {
        match gc_obj_type(ptr) {
            crate::gc::GC_TYPE_ARRAY => {
                internalize_array(
                    &value_handle,
                    reviver_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
                );
            }
            crate::gc::GC_TYPE_OBJECT => {
                internalize_object(
                    &value_handle,
                    reviver_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
                );
            }
            _ => {}
        }
    }

    call_reviver(
        &holder_handle,
        &key_handle,
        &value_handle,
        reviver_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
    )
}

/// Apply reviver to a parsed JSON value through the same root-holder wrapper
/// used by `JSON.parse(text, reviver)`.
pub(crate) unsafe fn apply_reviver(
    value: JSValue,
    key_f64: f64,
    reviver: *const crate::closure::ClosureHeader,
) -> JSValue {
    let scope = crate::gc::RuntimeHandleScope::new();
    let wrapper = crate::object::js_object_alloc(0, 1);
    let wrapper_handle = scope.root_raw_mut_ptr(wrapper);
    let reviver_handle = scope.root_raw_const_ptr(reviver);
    let key_handle = scope.root_nanbox_f64(key_f64);
    let key_ptr = crate::value::js_get_string_pointer_unified(key_handle.get_nanbox_f64())
        as *const StringHeader;
    let key_ptr_handle = scope.root_string_ptr(key_ptr);
    let value = force_materialize_if_lazy(value);
    let value_handle = scope.root_nanbox_u64(value.bits());
    crate::object::js_object_set_field_by_name(
        wrapper_handle.get_raw_mut_ptr::<crate::ObjectHeader>(),
        key_ptr_handle.get_raw_const_ptr::<StringHeader>(),
        value_handle.get_nanbox_f64(),
    );
    internalize_json_property(
        JSValue::object_ptr(wrapper_handle.get_raw_mut_ptr::<crate::ObjectHeader>() as *mut u8),
        key_handle.get_nanbox_f64(),
        reviver_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
    )
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
