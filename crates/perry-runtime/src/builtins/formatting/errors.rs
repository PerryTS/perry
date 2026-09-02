//! `util.inspect` formatting for native Errors and ordinary-layout Error
//! subclasses.

use super::*;

unsafe fn string_header_to_string(ptr: *mut StringHeader, fallback: &str) -> String {
    if ptr.is_null() {
        return fallback.to_string();
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    std::str::from_utf8(bytes).unwrap_or(fallback).to_string()
}

unsafe fn format_error_headline(error_ptr: *const crate::error::ErrorHeader) -> String {
    let scope = crate::gc::RuntimeHandleScope::new();
    let error_h = scope.root_raw_const_ptr(error_ptr);
    let own_name_h = error_h
        .with_const_ptr::<crate::error::ErrorHeader, _>(|error_ptr| {
            crate::node_submodules::error_user_prop(error_ptr as usize, "name")
        })
        .map(|value| scope.root_nanbox_f64(value));
    let (own_message_h, error_ptr) = error_h.across_const::<crate::error::ErrorHeader, _>(|| {
        error_h
            .with_const_ptr::<crate::error::ErrorHeader, _>(|error_ptr| {
                crate::node_submodules::error_user_prop(error_ptr as usize, "message")
            })
            .map(|value| scope.root_nanbox_f64(value))
    });
    let display_part = |value: Option<&crate::gc::RuntimeHandle<'_>>,
                        header: *mut StringHeader,
                        fallback: &str| {
        value
            .and_then(|handle| jsvalue_string_content(handle.get_nanbox_f64()))
            .unwrap_or_else(|| string_header_to_string(header, fallback))
    };
    // `ErrorHeader.name` is internal backing storage for the inherited
    // Error-family prototype value. An explicit `error.name = ...` is an own
    // expando and must drive inspection without being redundantly printed as
    // a body property (#9440).
    let name_str = display_part(own_name_h.as_ref(), (*error_ptr).name, "Error");
    let message_str = display_part(own_message_h.as_ref(), (*error_ptr).message, "");
    if message_str.is_empty() {
        name_str
    } else {
        format!("{}: {}", name_str, message_str)
    }
}

unsafe fn format_error_stack_frame(error_ptr: *const crate::error::ErrorHeader) -> Option<String> {
    let stack = string_header_to_string((*error_ptr).stack, "");
    stack
        .lines()
        .skip(1)
        .find(|line| !line.trim().is_empty())
        .map(str::to_string)
}

unsafe fn format_error_array(arr_ptr: *const crate::array::ArrayHeader, depth: usize) -> String {
    if arr_ptr.is_null() {
        return "[]".to_string();
    }
    let length = (*arr_ptr).length as usize;
    if length == 0 {
        return "[]".to_string();
    }
    let data_ptr =
        (arr_ptr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64;
    let mut out = String::from("[");
    for i in 0..length {
        out.push('\n');
        out.push_str("    ");
        out.push_str(&format_jsvalue_for_json(*data_ptr.add(i), depth + 1));
    }
    out.push('\n');
    out.push_str("  ]");
    out
}

pub(super) unsafe fn format_error_value(
    error_ptr: *const crate::error::ErrorHeader,
    depth: usize,
) -> String {
    // Headline lookup consults the ordinary expando bag and may allocate.
    // Keep the native Error live and re-read its address for every later slot.
    let scope = crate::gc::RuntimeHandleScope::new();
    let error_h = scope.root_raw_const_ptr(error_ptr);
    let headline = error_h.with_const_ptr::<crate::error::ErrorHeader, _>(|error_ptr| {
        format_error_headline(error_ptr)
    });
    let mut entries: Vec<(String, String)> = error_h
        .with_const_ptr::<crate::error::ErrorHeader, _>(|error_ptr| {
            crate::node_submodules::error_user_props(error_ptr as usize)
        })
        .into_iter()
        .filter(|(key, _)| key != "cause" && key != "errors" && key != "name")
        .map(|(key, value)| (key, format_jsvalue_for_json(value, depth + 1)))
        .collect();

    let cause =
        error_h.with_const_ptr::<crate::error::ErrorHeader, _>(|error_ptr| (*error_ptr).cause);
    if !crate::value::JSValue::from_bits(cause.to_bits()).is_undefined() {
        entries.push((
            "[cause]".to_string(),
            format_jsvalue_for_json(cause, depth + 1),
        ));
    }

    let errors =
        error_h.with_const_ptr::<crate::error::ErrorHeader, _>(|error_ptr| (*error_ptr).errors);
    if !errors.is_null() {
        entries.push((
            "[errors]".to_string(),
            format_error_array(errors, depth + 1),
        ));
    }

    if entries.is_empty() {
        return headline;
    }

    let mut out = headline;
    if let Some(frame) = error_h.with_const_ptr::<crate::error::ErrorHeader, _>(|error_ptr| {
        format_error_stack_frame(error_ptr)
    }) {
        out.push('\n');
        out.push_str(&frame);
        out.push_str(" {");
    } else {
        out.push_str("\n{");
    }

    let last = entries.len().saturating_sub(1);
    for (idx, (label, value)) in entries.into_iter().enumerate() {
        out.push('\n');
        out.push_str("  ");
        out.push_str(&label);
        out.push_str(": ");
        out.push_str(&value);
        if idx != last {
            out.push(',');
        }
    }
    out.push('\n');
    out.push('}');
    out
}

/// Build Node's Error headline for a user class whose instances use the
/// ordinary object layout. The class registry supplies the Error-family
/// prototype name; own `message` and explicitly assigned `name` values come
/// from the instance slots (#9440).
pub(super) unsafe fn format_error_subclass_headline(
    obj_ptr: *const crate::object::ObjectHeader,
    class_id: u32,
    class_name: &str,
) -> (String, *const crate::object::ObjectHeader) {
    // String coercion below can collect. Keep both the receiver and the two
    // values which can participate in the headline live across either
    // conversion, and return the receiver's refreshed address to the caller.
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_raw_const_ptr(obj_ptr);
    let keys = obj_h.with_const_ptr::<crate::object::ObjectHeader, _>(|obj_ptr| {
        crate::object::object_keys_array(obj_ptr)
    });
    let mut own_name: Option<f64> = None;
    let mut own_message: Option<f64> = None;
    if !keys.is_null() {
        let len = crate::array::js_array_length(keys);
        for index in 0..len {
            let key = crate::array::js_array_get(keys, index);
            if !key.is_string() {
                continue;
            }
            let key_ptr = key.as_string_ptr();
            if key_ptr.is_null() {
                continue;
            }
            let key_len = (*key_ptr).byte_len as usize;
            let key_data = (key_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            let key_bytes = std::slice::from_raw_parts(key_data, key_len);
            if key_bytes == b"name" {
                own_name = Some(obj_h.with_const_ptr::<crate::object::ObjectHeader, _>(
                    |obj_ptr| crate::object::js_object_get_field_f64(obj_ptr, index),
                ));
            } else if key_bytes == b"message" {
                own_message = Some(obj_h.with_const_ptr::<crate::object::ObjectHeader, _>(
                    |obj_ptr| crate::object::js_object_get_field_f64(obj_ptr, index),
                ));
            }
        }
    }

    let own_name_h = own_name.map(|value| scope.root_nanbox_f64(value));
    let own_message_h = own_message.map(|value| scope.root_nanbox_f64(value));
    // The string coercions below can collect; compute the headline inside
    // `across_const` so the receiver address handed back is re-read afterwards.
    let (headline, obj_ptr) = obj_h.across_const::<crate::object::ObjectHeader, _>(|| {
        let value_string = |value: &crate::gc::RuntimeHandle<'_>, fallback: &str| {
            let value = value.get_nanbox_f64();
            let js = JSValue::from_bits(value.to_bits());
            if js.is_undefined() {
                return fallback.to_string();
            }
            jsvalue_string_content(value).unwrap_or_else(|| {
                let string = crate::value::js_jsvalue_to_string(value);
                string_header_to_string(string, fallback)
            })
        };
        let prototype_name = crate::object::builtin_error_prototype_name(class_id);
        let name = own_name_h
            .as_ref()
            .map(|value| value_string(value, prototype_name))
            .unwrap_or_else(|| prototype_name.to_string());
        let message = own_message_h
            .as_ref()
            .map(|value| value_string(value, ""))
            .unwrap_or_default();
        let display_name = if own_name_h.is_none() && class_name != name {
            format!("{class_name} [{name}]")
        } else {
            name
        };
        if display_name.is_empty() {
            message
        } else if message.is_empty() {
            display_name
        } else {
            format!("{display_name}: {message}")
        }
    });
    (headline, obj_ptr)
}
