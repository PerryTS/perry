//! `node:util` MIME classes and legacy helper exports.

use crate::array::{js_array_alloc, js_array_get_f64, js_array_length, js_array_push_f64};
use crate::object::{
    js_object_alloc, js_object_get_field_by_name_f64, js_object_get_field_f64, js_object_keys,
    js_object_set_field_by_name, js_object_set_field_f64, js_object_set_keys, js_register_class_id,
    js_register_class_method, js_register_class_name, js_register_class_setter,
    set_builtin_property_attrs, ObjectHeader, PropertyAttrs,
};
use crate::string::js_string_from_bytes;
use crate::value::{js_jsvalue_to_string, js_nanbox_pointer, JSValue};
use crate::{ArrayHeader, StringHeader};
use std::sync::Once;

pub const CLASS_ID_MIME_TYPE: u32 = 0xFFFF_00C0;
pub const CLASS_ID_MIME_PARAMS: u32 = 0xFFFF_00C1;

const MIME_TYPE_TYPE: u32 = 0;
const MIME_TYPE_SUBTYPE: u32 = 1;
const MIME_TYPE_ESSENCE: u32 = 2;
const MIME_TYPE_PARAMS: u32 = 3;
const MIME_TYPE_FIELD_COUNT: u32 = 4;

const MIME_PARAMS_ENTRIES: u32 = 0;
const MIME_PARAMS_FIELD_COUNT: u32 = 1;

static INIT_MIME_CLASSES: Once = Once::new();

fn undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn null() -> f64 {
    f64::from_bits(crate::value::TAG_NULL)
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(if value {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    })
}

fn string_value(value: &str) -> f64 {
    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

unsafe fn string_from_header(ptr: *mut StringHeader) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
}

fn value_to_string(value: f64) -> String {
    unsafe { string_from_header(js_jsvalue_to_string(value)) }
}

fn optional_string(value: f64) -> Option<String> {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_undefined() || js.is_null() {
        None
    } else {
        Some(value_to_string(value))
    }
}

fn object_ptr_from_value(value: f64) -> Option<*mut ObjectHeader> {
    let js = JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        return None;
    }
    let ptr = js.as_pointer::<u8>();
    if ptr.is_null() || !crate::object::is_valid_obj_ptr(ptr as *const u8) {
        return None;
    }
    Some(ptr as *mut ObjectHeader)
}

fn ptr_from_stored_array(value: f64) -> *mut ArrayHeader {
    let bits = value.to_bits();
    let top = bits >> 48;
    if top == (crate::value::POINTER_TAG >> 48) {
        (bits & crate::value::POINTER_MASK) as *mut ArrayHeader
    } else {
        bits as usize as *mut ArrayHeader
    }
}

fn ptr_value<T>(ptr: *mut T) -> f64 {
    js_nanbox_pointer(ptr as i64)
}

fn throw_type_error(message: &str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(js_nanbox_pointer(err as i64))
}

fn throw_invalid_mime_syntax(kind: &str, input: &str, index: Option<usize>) -> ! {
    let message = match index {
        Some(i) => format!("The MIME syntax for a {kind} in \"{input}\" is invalid at {i}"),
        None => format!("The MIME syntax for a {kind} in \"{input}\" is invalid"),
    };
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_MIME_SYNTAX")
}

fn is_token_char(byte: u8) -> bool {
    matches!(
        byte,
        b'!' | b'#'
            | b'$'
            | b'%'
            | b'&'
            | b'\''
            | b'*'
            | b'+'
            | b'-'
            | b'.'
            | b'^'
            | b'_'
            | b'`'
            | b'|'
            | b'~'
            | b'0'..=b'9'
            | b'A'..=b'Z'
            | b'a'..=b'z'
    )
}

fn validate_token(kind: &str, value: &str, original: &str) {
    if value.is_empty() {
        throw_invalid_mime_syntax(kind, original, None);
    }
    for (idx, byte) in value.bytes().enumerate() {
        if !is_token_char(byte) {
            throw_invalid_mime_syntax(kind, original, Some(idx));
        }
    }
}

fn validate_param_value(value: &str, original: &str) {
    for (idx, ch) in value.char_indices() {
        if ch == '\r' || ch == '\n' {
            throw_invalid_mime_syntax("parameter value", original, Some(idx));
        }
    }
}

fn serialize_param_value(value: &str) -> String {
    if !value.is_empty() && value.bytes().all(is_token_char) {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        if ch == '"' || ch == '\\' {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

fn parse_quoted_value(input: &str) -> String {
    let mut out = String::new();
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn parse_mime(input: &str) -> (String, String, Vec<(String, String)>) {
    let mut parts = input.split(';');
    let essence_raw = parts.next().unwrap_or("").trim();
    let slash = essence_raw.find('/');
    let Some(slash_idx) = slash else {
        throw_invalid_mime_syntax("type", input, None);
    };
    let type_raw = &essence_raw[..slash_idx];
    let subtype_raw = &essence_raw[slash_idx + 1..];
    validate_token("type", type_raw, input);
    validate_token("subtype", subtype_raw, input);

    let type_name = type_raw.to_ascii_lowercase();
    let subtype = subtype_raw.to_ascii_lowercase();
    let mut params = Vec::new();
    for part in parts {
        let piece = part.trim();
        if piece.is_empty() {
            continue;
        }
        let Some(eq_idx) = piece.find('=') else {
            continue;
        };
        let name = piece[..eq_idx].trim().to_ascii_lowercase();
        validate_token("parameter name", &name, piece);
        let value_raw = piece[eq_idx + 1..].trim();
        let value =
            if value_raw.len() >= 2 && value_raw.starts_with('"') && value_raw.ends_with('"') {
                parse_quoted_value(&value_raw[1..value_raw.len() - 1])
            } else {
                value_raw.to_string()
            };
        validate_param_value(&value, piece);
        params.push((name, value));
    }
    (type_name, subtype, params)
}

fn make_entries_array(entries: &[(String, String)]) -> *mut ArrayHeader {
    let mut entries_array = js_array_alloc(entries.len() as u32);
    for (key, value) in entries {
        let mut pair = js_array_alloc(2);
        pair = js_array_push_f64(pair, string_value(key));
        pair = js_array_push_f64(pair, string_value(value));
        entries_array = js_array_push_f64(entries_array, ptr_value(pair));
    }
    entries_array
}

fn create_mime_params_object(entries: Vec<(String, String)>) -> *mut ObjectHeader {
    ensure_mime_classes();
    let obj = js_object_alloc(CLASS_ID_MIME_PARAMS, MIME_PARAMS_FIELD_COUNT);
    let mut keys = js_array_alloc(MIME_PARAMS_FIELD_COUNT);
    keys = js_array_push_f64(keys, string_value("_entries"));
    js_object_set_keys(obj, keys);
    set_builtin_property_attrs(
        obj as usize,
        "_entries".to_string(),
        PropertyAttrs::new(true, false, true),
    );
    js_object_set_field_f64(
        obj,
        MIME_PARAMS_ENTRIES,
        ptr_value(make_entries_array(&entries)),
    );
    obj
}

fn get_mime_params_entries(params: *mut ObjectHeader) -> Vec<(String, String)> {
    if params.is_null() {
        return Vec::new();
    }
    let entries_value = js_object_get_field_f64(params, MIME_PARAMS_ENTRIES);
    let entries = ptr_from_stored_array(entries_value);
    if entries.is_null() {
        return Vec::new();
    }
    let len = js_array_length(entries) as usize;
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        let pair = ptr_from_stored_array(js_array_get_f64(entries, i as u32));
        if pair.is_null() || js_array_length(pair) < 2 {
            continue;
        }
        let key = value_to_string(js_array_get_f64(pair, 0));
        let value = value_to_string(js_array_get_f64(pair, 1));
        out.push((key, value));
    }
    out
}

fn set_mime_params_entries(params: *mut ObjectHeader, entries: Vec<(String, String)>) {
    if params.is_null() {
        return;
    }
    js_object_set_field_f64(
        params,
        MIME_PARAMS_ENTRIES,
        ptr_value(make_entries_array(&entries)),
    );
}

fn serialize_params(entries: &[(String, String)]) -> String {
    entries
        .iter()
        .map(|(k, v)| format!("{k}={}", serialize_param_value(v)))
        .collect::<Vec<_>>()
        .join(";")
}

fn mime_type_parts(this: *mut ObjectHeader) -> (String, String, *mut ObjectHeader) {
    let type_name = value_to_string(js_object_get_field_f64(this, MIME_TYPE_TYPE));
    let subtype = value_to_string(js_object_get_field_f64(this, MIME_TYPE_SUBTYPE));
    let params = object_ptr_from_value(js_object_get_field_f64(this, MIME_TYPE_PARAMS))
        .unwrap_or(std::ptr::null_mut());
    (type_name, subtype, params)
}

fn update_mime_type_essence(this: *mut ObjectHeader) {
    let type_name = value_to_string(js_object_get_field_f64(this, MIME_TYPE_TYPE));
    let subtype = value_to_string(js_object_get_field_f64(this, MIME_TYPE_SUBTYPE));
    js_object_set_field_f64(
        this,
        MIME_TYPE_ESSENCE,
        string_value(&format!("{type_name}/{subtype}")),
    );
}

fn create_mime_type_object(input: &str) -> *mut ObjectHeader {
    ensure_mime_classes();
    let (type_name, subtype, params) = parse_mime(input);
    let params_obj = create_mime_params_object(params);
    let obj = js_object_alloc(CLASS_ID_MIME_TYPE, MIME_TYPE_FIELD_COUNT);
    let mut keys = js_array_alloc(MIME_TYPE_FIELD_COUNT);
    for key in ["type", "subtype", "essence", "params"] {
        keys = js_array_push_f64(keys, string_value(key));
    }
    js_object_set_keys(obj, keys);
    for key in ["type", "subtype", "essence", "params"] {
        set_builtin_property_attrs(
            obj as usize,
            key.to_string(),
            PropertyAttrs::new(true, false, true),
        );
    }
    js_object_set_field_f64(obj, MIME_TYPE_TYPE, string_value(&type_name));
    js_object_set_field_f64(obj, MIME_TYPE_SUBTYPE, string_value(&subtype));
    js_object_set_field_f64(
        obj,
        MIME_TYPE_ESSENCE,
        string_value(&format!("{type_name}/{subtype}")),
    );
    js_object_set_field_f64(obj, MIME_TYPE_PARAMS, ptr_value(params_obj));
    obj
}

extern "C" fn mime_type_to_string_vtable(this: f64) -> f64 {
    let Some(obj) = object_ptr_from_value(this) else {
        return string_value("");
    };
    let (type_name, subtype, params) = mime_type_parts(obj);
    let params_str = serialize_params(&get_mime_params_entries(params));
    let out = if params_str.is_empty() {
        format!("{type_name}/{subtype}")
    } else {
        format!("{type_name}/{subtype};{params_str}")
    };
    string_value(&out)
}

extern "C" fn mime_type_set_type_vtable(this: f64, value: f64) -> f64 {
    let Some(obj) = object_ptr_from_value(this) else {
        return undefined();
    };
    let type_name = value_to_string(value).to_ascii_lowercase();
    validate_token("type", &type_name, &type_name);
    js_object_set_field_f64(obj, MIME_TYPE_TYPE, string_value(&type_name));
    update_mime_type_essence(obj);
    undefined()
}

extern "C" fn mime_type_set_subtype_vtable(this: f64, value: f64) -> f64 {
    let Some(obj) = object_ptr_from_value(this) else {
        return undefined();
    };
    let subtype = value_to_string(value).to_ascii_lowercase();
    validate_token("subtype", &subtype, &subtype);
    js_object_set_field_f64(obj, MIME_TYPE_SUBTYPE, string_value(&subtype));
    update_mime_type_essence(obj);
    undefined()
}

extern "C" fn mime_params_get_vtable(this: f64, name: f64) -> f64 {
    let Some(obj) = object_ptr_from_value(this) else {
        return null();
    };
    let name = value_to_string(name);
    for (key, value) in get_mime_params_entries(obj) {
        if key == name {
            return string_value(&value);
        }
    }
    null()
}

extern "C" fn mime_params_has_vtable(this: f64, name: f64) -> f64 {
    let Some(obj) = object_ptr_from_value(this) else {
        return bool_value(false);
    };
    let name = value_to_string(name);
    bool_value(
        get_mime_params_entries(obj)
            .iter()
            .any(|(key, _)| key == &name),
    )
}

extern "C" fn mime_params_set_vtable(this: f64, name: f64, value: f64) -> f64 {
    let Some(obj) = object_ptr_from_value(this) else {
        return undefined();
    };
    let name = value_to_string(name);
    let value = value_to_string(value);
    validate_token("parameter name", &name, &name);
    validate_param_value(&value, &value);
    let mut entries = get_mime_params_entries(obj);
    if let Some((_, existing)) = entries.iter_mut().find(|(key, _)| key == &name) {
        *existing = value;
    } else {
        entries.push((name, value));
    }
    set_mime_params_entries(obj, entries);
    undefined()
}

extern "C" fn mime_params_delete_vtable(this: f64, name: f64) -> f64 {
    let Some(obj) = object_ptr_from_value(this) else {
        return undefined();
    };
    let name = value_to_string(name);
    let mut entries = get_mime_params_entries(obj);
    entries.retain(|(key, _)| key != &name);
    set_mime_params_entries(obj, entries);
    undefined()
}

extern "C" fn mime_params_entries_vtable(this: f64) -> f64 {
    let Some(obj) = object_ptr_from_value(this) else {
        return ptr_value(js_array_alloc(0));
    };
    ptr_value(make_entries_array(&get_mime_params_entries(obj)))
}

extern "C" fn mime_params_keys_vtable(this: f64) -> f64 {
    let Some(obj) = object_ptr_from_value(this) else {
        return ptr_value(js_array_alloc(0));
    };
    let entries = get_mime_params_entries(obj);
    let mut arr = js_array_alloc(entries.len() as u32);
    for (key, _) in entries {
        arr = js_array_push_f64(arr, string_value(&key));
    }
    ptr_value(arr)
}

extern "C" fn mime_params_values_vtable(this: f64) -> f64 {
    let Some(obj) = object_ptr_from_value(this) else {
        return ptr_value(js_array_alloc(0));
    };
    let entries = get_mime_params_entries(obj);
    let mut arr = js_array_alloc(entries.len() as u32);
    for (_, value) in entries {
        arr = js_array_push_f64(arr, string_value(&value));
    }
    ptr_value(arr)
}

extern "C" fn mime_params_to_string_vtable(this: f64) -> f64 {
    let Some(obj) = object_ptr_from_value(this) else {
        return string_value("");
    };
    string_value(&serialize_params(&get_mime_params_entries(obj)))
}

fn register_method(class_id: u32, name: &'static str, func_ptr: usize, param_count: i64) {
    unsafe {
        js_register_class_method(
            class_id as i64,
            name.as_ptr(),
            name.len() as i64,
            func_ptr as i64,
            param_count,
            0,
        );
    }
}

fn register_setter(class_id: u32, name: &'static str, func_ptr: usize) {
    unsafe {
        js_register_class_setter(
            class_id as i64,
            name.as_ptr(),
            name.len() as i64,
            func_ptr as i64,
        );
    }
}

pub fn ensure_mime_classes() {
    INIT_MIME_CLASSES.call_once(|| unsafe {
        js_register_class_id(CLASS_ID_MIME_TYPE);
        js_register_class_name(
            CLASS_ID_MIME_TYPE,
            b"MIMEType".as_ptr(),
            "MIMEType".len() as u32,
        );
        js_register_class_id(CLASS_ID_MIME_PARAMS);
        js_register_class_name(
            CLASS_ID_MIME_PARAMS,
            b"MIMEParams".as_ptr(),
            "MIMEParams".len() as u32,
        );

        register_method(
            CLASS_ID_MIME_TYPE,
            "toString",
            mime_type_to_string_vtable as *const () as usize,
            0,
        );
        register_method(
            CLASS_ID_MIME_TYPE,
            "toJSON",
            mime_type_to_string_vtable as *const () as usize,
            0,
        );
        register_setter(
            CLASS_ID_MIME_TYPE,
            "type",
            mime_type_set_type_vtable as *const () as usize,
        );
        register_setter(
            CLASS_ID_MIME_TYPE,
            "subtype",
            mime_type_set_subtype_vtable as *const () as usize,
        );

        register_method(
            CLASS_ID_MIME_PARAMS,
            "get",
            mime_params_get_vtable as *const () as usize,
            1,
        );
        register_method(
            CLASS_ID_MIME_PARAMS,
            "has",
            mime_params_has_vtable as *const () as usize,
            1,
        );
        register_method(
            CLASS_ID_MIME_PARAMS,
            "set",
            mime_params_set_vtable as *const () as usize,
            2,
        );
        register_method(
            CLASS_ID_MIME_PARAMS,
            "delete",
            mime_params_delete_vtable as *const () as usize,
            1,
        );
        register_method(
            CLASS_ID_MIME_PARAMS,
            "entries",
            mime_params_entries_vtable as *const () as usize,
            0,
        );
        register_method(
            CLASS_ID_MIME_PARAMS,
            "keys",
            mime_params_keys_vtable as *const () as usize,
            0,
        );
        register_method(
            CLASS_ID_MIME_PARAMS,
            "values",
            mime_params_values_vtable as *const () as usize,
            0,
        );
        register_method(
            CLASS_ID_MIME_PARAMS,
            "toString",
            mime_params_to_string_vtable as *const () as usize,
            0,
        );
    });
}

#[no_mangle]
pub extern "C" fn js_util_mime_type_new(input: f64) -> f64 {
    ptr_value(create_mime_type_object(&value_to_string(input)))
}

#[no_mangle]
pub extern "C" fn js_util_mime_params_new() -> f64 {
    ptr_value(create_mime_params_object(Vec::new()))
}

fn set_error_prop(error: *mut crate::error::ErrorHeader, key: &str, value: f64) {
    let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
    js_object_set_field_by_name(error as *mut ObjectHeader, key_ptr, value);
}

fn set_error_string_prop(error: *mut crate::error::ErrorHeader, key: &str, value: &str) {
    set_error_prop(error, key, string_value(value));
}

fn build_system_error(code: i64, syscall: &str, message: String) -> f64 {
    let code_name = crate::util_syserr::system_error_name_for_code(code);
    let msg_ptr = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_error_new_with_message(msg_ptr);
    set_error_prop(err, "errno", code as f64);
    set_error_string_prop(err, "code", &code_name);
    set_error_string_prop(err, "syscall", syscall);
    js_nanbox_pointer(err as i64)
}

fn optional_port(value: f64) -> Option<i64> {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_undefined() || js.is_null() {
        return None;
    }
    if js.is_int32() {
        return Some(js.as_int32() as i64);
    }
    if js.is_number() && value.is_finite() {
        return Some(value as i64);
    }
    None
}

#[no_mangle]
pub extern "C" fn js_util_extend(target: f64, source: f64) -> f64 {
    let Some(source_obj) = object_ptr_from_value(source) else {
        return target;
    };
    let keys = js_object_keys(source_obj as *const ObjectHeader);
    let len = js_array_length(keys) as usize;
    if len == 0 {
        return target;
    }
    let Some(target_obj) = object_ptr_from_value(target) else {
        let key = value_to_string(js_array_get_f64(keys, 0));
        let target_name = if JSValue::from_bits(target.to_bits()).is_null() {
            "null"
        } else {
            "undefined"
        };
        throw_type_error(&format!(
            "Cannot set properties of {target_name} (setting '{key}')"
        ));
    };
    for i in 0..len {
        let key_value = js_array_get_f64(keys, i as u32);
        let key = value_to_string(key_value);
        let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
        let value = js_object_get_field_by_name_f64(source_obj, key_ptr);
        js_object_set_field_by_name(target_obj, key_ptr, value);
    }
    target
}

#[no_mangle]
pub extern "C" fn js_util_errno_exception(err: f64, syscall: f64, original: f64) -> f64 {
    let code = crate::util_syserr::validate_system_error_code(err);
    let code_name = crate::util_syserr::system_error_name_for_code(code);
    let syscall = value_to_string(syscall);
    let message = match optional_string(original) {
        Some(original) if !original.is_empty() => format!("{syscall} {code_name} {original}"),
        _ => format!("{syscall} {code_name}"),
    };
    build_system_error(code, &syscall, message)
}

#[no_mangle]
pub extern "C" fn js_util_exception_with_host_port(
    err: f64,
    syscall: f64,
    address: f64,
    port: f64,
    additional: f64,
) -> f64 {
    let code = crate::util_syserr::validate_system_error_code(err);
    let code_name = crate::util_syserr::system_error_name_for_code(code);
    let syscall = value_to_string(syscall);
    let address = value_to_string(address);
    let port = optional_port(port);
    let mut message = match port {
        Some(port) => format!("{syscall} {code_name} {address}:{port}"),
        None => format!("{syscall} {code_name} {address}"),
    };
    if let Some(additional) = optional_string(additional) {
        if !additional.is_empty() {
            message.push_str(&format!(" - Local ({additional})"));
        }
    }
    let error = build_system_error(code, &syscall, message);
    let err_ptr = crate::value::js_nanbox_get_pointer(error) as *mut crate::error::ErrorHeader;
    set_error_string_prop(err_ptr, "address", &address);
    if let Some(port) = port {
        set_error_prop(err_ptr, "port", port as f64);
    }
    error
}
