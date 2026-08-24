use super::*;

pub(super) fn get_field(value: *const ObjectHeader, key: &str) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_raw_const_ptr(value);
    let key = scope.root_string_ptr(js_string_from_bytes(key.as_ptr(), key.len() as u32));
    value.with_const_ptr(|value| {
        key.with_const_ptr(|key| js_object_get_field_by_name_f64(value, key))
    })
}

pub(super) fn get_field_from_raw_handle(value: &crate::gc::RuntimeHandle<'_>, key: &str) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let key = scope.root_string_ptr(js_string_from_bytes(key.as_ptr(), key.len() as u32));
    value.with_const_ptr::<ObjectHeader, _>(|value| {
        key.with_const_ptr(|key| js_object_get_field_by_name_f64(value, key))
    })
}

pub(super) fn get_string_field_from_raw_handle(
    value: &crate::gc::RuntimeHandle<'_>,
    key: &str,
) -> Option<String> {
    string_from_string_value(get_field_from_raw_handle(value, key))
}

pub(super) fn set_internal_field_from_raw_handle(
    value: &crate::gc::RuntimeHandle<'_>,
    key: &str,
    field: f64,
) {
    value.with_mut_ptr(|value| set_internal_field(value, key, field));
}

pub(super) fn get_field_from_value_handle(value: &crate::gc::RuntimeHandle<'_>, key: &str) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let key = scope.root_string_ptr(js_string_from_bytes(key.as_ptr(), key.len() as u32));
    let current = value.get_nanbox_f64();
    let Some(object) = object_ptr_from_value(current) else {
        return undefined();
    };
    key.with_const_ptr(|key| js_object_get_field_by_name_f64(object, key))
}

pub(super) fn set_field(obj: *mut ObjectHeader, key: &str, value: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(obj);
    let value = scope.root_nanbox_f64(value);
    let key = scope.root_string_ptr(js_string_from_bytes(key.as_ptr(), key.len() as u32));
    obj.with_mut_ptr(|obj| {
        key.with_const_ptr(|key| js_object_set_field_by_name(obj, key, value.get_nanbox_f64()))
    });
}

pub(super) fn set_builtin_attrs(obj: *mut ObjectHeader, key: &str, attrs: PropertyAttrs) {
    set_builtin_property_attrs(obj as usize, key.to_string(), attrs);
}

pub(super) fn set_internal_field(obj: *mut ObjectHeader, key: &str, value: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(obj);
    obj.with_mut_ptr(|obj| set_field(obj, key, value));
    obj.with_mut_ptr(|obj| set_builtin_attrs(obj, key, PropertyAttrs::new(true, false, true)));
}

pub(super) fn get_string_field(obj: *const ObjectHeader, key: &str) -> Option<String> {
    string_from_string_value(get_field(obj, key))
}

pub(super) fn get_number_field(obj: *const ObjectHeader, key: &str) -> Option<f64> {
    let value = get_field(obj, key);
    let js = JSValue::from_bits(value.to_bits());
    if js.is_undefined() || js.is_null() {
        None
    } else {
        Some(js.to_number())
    }
}

pub(super) fn get_option_value(options: f64, key: &str) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let options = scope.root_nanbox_f64(options);
    let key = scope.root_string_ptr(js_string_from_bytes(key.as_ptr(), key.len() as u32));
    if crate::proxy::js_proxy_is_proxy(options.get_nanbox_f64()) != 0 {
        return crate::proxy::js_proxy_get(
            options.get_nanbox_f64(),
            key.with_mut_ptr(|key| f64::from_bits(JSValue::string_ptr(key).bits())),
        );
    }
    let Some(obj) = object_ptr_from_value(options.get_nanbox_f64()) else {
        return undefined();
    };
    key.with_const_ptr(|key| js_object_get_field_by_name_f64(obj, key))
}
