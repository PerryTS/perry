use super::*;
use crate::value::JSValue;
use std::ffi::{c_char, c_void};

#[repr(i32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NapiValueType {
    Undefined = 0,
    Null = 1,
    Boolean = 2,
    Number = 3,
    String = 4,
    Symbol = 5,
    Object = 6,
    Function = 7,
    External = 8,
    Bigint = 9,
}

fn write_handle(env: NapiEnv, bits: u64, result: *mut NapiValue) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let Ok(handle) = add_handle(env, bits) else {
        return NapiStatus::InvalidArg;
    };
    unsafe { *result = handle };
    ok(env)
}

fn value_as_number(bits: u64) -> Option<f64> {
    let value = JSValue::from_bits(bits);
    if value.is_int32() {
        Some(value.as_int32() as f64)
    } else if value.is_number() {
        Some(value.as_number())
    } else {
        None
    }
}

fn value_as_bool(bits: u64) -> Option<bool> {
    let value = JSValue::from_bits(bits);
    value.is_bool().then(|| value.as_bool())
}

fn to_int32(number: f64) -> i32 {
    if !number.is_finite() || number == 0.0 {
        return 0;
    }
    let modulo = number.trunc().rem_euclid(4_294_967_296.0);
    if modulo >= 2_147_483_648.0 {
        (modulo - 4_294_967_296.0) as i32
    } else {
        modulo as i32
    }
}

fn pointer_bits(ptr: *const u8) -> u64 {
    JSValue::pointer(ptr).bits()
}

fn string_bits(ptr: *mut crate::string::StringHeader) -> u64 {
    JSValue::string_ptr(ptr).bits()
}

fn bigint_bits(ptr: *mut crate::bigint::BigIntHeader) -> u64 {
    JSValue::bigint_ptr(ptr).bits()
}

fn create_string(env: NapiEnv, bytes: &[u8], wtf8: bool, result: *mut NapiValue) -> NapiStatus {
    let ptr = if wtf8 {
        crate::string::js_string_from_wtf8_bytes(bytes.as_ptr(), bytes.len() as u32)
    } else {
        crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
    };
    write_handle(env, string_bits(ptr), result)
}

fn input_len(ptr: *const c_char, length: usize) -> Result<usize, NapiStatus> {
    if ptr.is_null() {
        return Err(NapiStatus::InvalidArg);
    }
    let length = if length == NAPI_AUTO_LENGTH {
        unsafe { std::ffi::CStr::from_ptr(ptr).to_bytes().len() }
    } else {
        length
    };
    if length > i32::MAX as usize {
        return Err(NapiStatus::InvalidArg);
    }
    Ok(length)
}

fn push_wtf8(code: u32, out: &mut Vec<u8>) {
    if code <= 0x7f {
        out.push(code as u8);
    } else if code <= 0x7ff {
        out.push((0xc0 | (code >> 6)) as u8);
        out.push((0x80 | (code & 0x3f)) as u8);
    } else if code <= 0xffff {
        out.push((0xe0 | (code >> 12)) as u8);
        out.push((0x80 | ((code >> 6) & 0x3f)) as u8);
        out.push((0x80 | (code & 0x3f)) as u8);
    } else {
        out.push((0xf0 | (code >> 18)) as u8);
        out.push((0x80 | ((code >> 12) & 0x3f)) as u8);
        out.push((0x80 | ((code >> 6) & 0x3f)) as u8);
        out.push((0x80 | (code & 0x3f)) as u8);
    }
}

fn utf16_to_wtf8(units: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(units.len());
    let mut i = 0;
    while i < units.len() {
        let first = units[i] as u32;
        if (0xd800..=0xdbff).contains(&first) && i + 1 < units.len() {
            let second = units[i + 1] as u32;
            if (0xdc00..=0xdfff).contains(&second) {
                push_wtf8(
                    0x1_0000 + ((first - 0xd800) << 10) + (second - 0xdc00),
                    &mut out,
                );
                i += 2;
                continue;
            }
        }
        push_wtf8(first, &mut out);
        i += 1;
    }
    out
}

fn string_bytes(bits: u64) -> Result<Vec<u8>, NapiStatus> {
    let value = JSValue::from_bits(bits);
    if !value.is_any_string() {
        return Err(NapiStatus::StringExpected);
    }
    let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
    let Some((ptr, len)) =
        crate::string::str_bytes_from_jsvalue(f64::from_bits(bits), &mut scratch)
    else {
        return Err(NapiStatus::StringExpected);
    };
    if len == 0 {
        return Ok(Vec::new());
    }
    Ok(unsafe { std::slice::from_raw_parts(ptr, len as usize) }.to_vec())
}

fn wtf8_code_points(bytes: &[u8]) -> Vec<u32> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let (advance, _, code) = crate::string::wtf8_step(bytes, i);
        out.push(code);
        i = i.saturating_add(advance.max(1));
    }
    out
}

fn wtf8_to_utf8(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for code in wtf8_code_points(bytes) {
        let ch = char::from_u32(code).unwrap_or(char::REPLACEMENT_CHARACTER);
        let mut encoded = [0; 4];
        out.extend_from_slice(ch.encode_utf8(&mut encoded).as_bytes());
    }
    out
}

fn wtf8_to_utf16(bytes: &[u8]) -> Vec<u16> {
    let mut out = Vec::new();
    for code in wtf8_code_points(bytes) {
        if code <= 0xffff {
            out.push(code as u16);
        } else {
            let code = code - 0x1_0000;
            out.push((0xd800 + (code >> 10)) as u16);
            out.push((0xdc00 + (code & 0x3ff)) as u16);
        }
    }
    out
}

fn get_string_source(env: NapiEnv, value: NapiValue) -> Result<Vec<u8>, NapiStatus> {
    string_bytes(value_bits(env, value)?)
}

fn set_string_error(env: NapiEnv, status: NapiStatus) -> NapiStatus {
    match status {
        NapiStatus::StringExpected => set_status(env, status, "value must be a JavaScript string"),
        _ => set_status(env, status, "invalid Node-API string argument"),
    }
}

fn named_key(env: NapiEnv, name: *const c_char) -> Result<NapiValue, NapiStatus> {
    let len = input_len(name, NAPI_AUTO_LENGTH)?;
    let bytes = unsafe { std::slice::from_raw_parts(name.cast::<u8>(), len) };
    let ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    add_handle(env, string_bits(ptr))
}

fn property_call(
    env: NapiEnv,
    object: NapiValue,
    key: NapiValue,
    f: impl FnOnce(f64, f64) -> f64,
) -> Result<f64, NapiStatus> {
    if pending_exception(env).is_some() {
        return Err(NapiStatus::PendingException);
    }
    let object_bits = value_bits(env, object)?;
    let key_bits = value_bits(env, key)?;
    catch_value_call(env, || {
        f(f64::from_bits(object_bits), f64::from_bits(key_bits))
    })
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_undefined(env: NapiEnv, result: *mut NapiValue) -> NapiStatus {
    write_handle(env, crate::value::TAG_UNDEFINED, result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_null(env: NapiEnv, result: *mut NapiValue) -> NapiStatus {
    write_handle(env, crate::value::TAG_NULL, result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_global(env: NapiEnv, result: *mut NapiValue) -> NapiStatus {
    if pending_exception(env).is_some() {
        return set_status(env, NapiStatus::PendingException, "an exception is pending");
    }
    let global = crate::object::js_get_global_this();
    write_handle(env, global.to_bits(), result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_boolean(
    env: NapiEnv,
    value: bool,
    result: *mut NapiValue,
) -> NapiStatus {
    write_handle(env, JSValue::bool(value).bits(), result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_object(env: NapiEnv, result: *mut NapiValue) -> NapiStatus {
    let object = crate::object::js_object_alloc(0, 0);
    write_handle(env, pointer_bits(object.cast()), result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_array(env: NapiEnv, result: *mut NapiValue) -> NapiStatus {
    let array = crate::array::js_array_alloc(0);
    write_handle(env, pointer_bits(array.cast()), result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_array_with_length(
    env: NapiEnv,
    length: usize,
    result: *mut NapiValue,
) -> NapiStatus {
    let Ok(length) = u32::try_from(length) else {
        return set_status(env, NapiStatus::InvalidArg, "array length exceeds u32");
    };
    let array = crate::array::js_array_alloc_with_length(length);
    write_handle(env, pointer_bits(array.cast()), result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_double(
    env: NapiEnv,
    value: f64,
    result: *mut NapiValue,
) -> NapiStatus {
    write_handle(env, JSValue::number(value).bits(), result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_int32(
    env: NapiEnv,
    value: i32,
    result: *mut NapiValue,
) -> NapiStatus {
    write_handle(env, JSValue::int32(value).bits(), result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_uint32(
    env: NapiEnv,
    value: u32,
    result: *mut NapiValue,
) -> NapiStatus {
    let bits = if value <= i32::MAX as u32 {
        JSValue::int32(value as i32).bits()
    } else {
        JSValue::number(value as f64).bits()
    };
    write_handle(env, bits, result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_int64(
    env: NapiEnv,
    value: i64,
    result: *mut NapiValue,
) -> NapiStatus {
    write_handle(env, JSValue::number(value as f64).bits(), result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_value_double(
    env: NapiEnv,
    value: NapiValue,
    result: *mut f64,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let Ok(bits) = value_bits(env, value) else {
        return set_status(env, NapiStatus::InvalidArg, "value is not a live handle");
    };
    let Some(number) = value_as_number(bits) else {
        return set_status(env, NapiStatus::NumberExpected, "value must be a number");
    };
    *result = number;
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_value_int32(
    env: NapiEnv,
    value: NapiValue,
    result: *mut i32,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let Ok(bits) = value_bits(env, value) else {
        return set_status(env, NapiStatus::InvalidArg, "value is not a live handle");
    };
    let Some(number) = value_as_number(bits) else {
        return set_status(env, NapiStatus::NumberExpected, "value must be a number");
    };
    *result = to_int32(number);
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_value_uint32(
    env: NapiEnv,
    value: NapiValue,
    result: *mut u32,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let Ok(bits) = value_bits(env, value) else {
        return set_status(env, NapiStatus::InvalidArg, "value is not a live handle");
    };
    let Some(number) = value_as_number(bits) else {
        return set_status(env, NapiStatus::NumberExpected, "value must be a number");
    };
    *result = to_int32(number) as u32;
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_value_int64(
    env: NapiEnv,
    value: NapiValue,
    result: *mut i64,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let Ok(bits) = value_bits(env, value) else {
        return set_status(env, NapiStatus::InvalidArg, "value is not a live handle");
    };
    let Some(number) = value_as_number(bits) else {
        return set_status(env, NapiStatus::NumberExpected, "value must be a number");
    };
    *result = number as i64;
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_value_bool(
    env: NapiEnv,
    value: NapiValue,
    result: *mut bool,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let Ok(bits) = value_bits(env, value) else {
        return set_status(env, NapiStatus::InvalidArg, "value is not a live handle");
    };
    let Some(boolean) = value_as_bool(bits) else {
        return set_status(env, NapiStatus::BooleanExpected, "value must be a boolean");
    };
    *result = boolean;
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_typeof(
    env: NapiEnv,
    value: NapiValue,
    result: *mut NapiValueType,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let Ok(bits) = value_bits(env, value) else {
        return set_status(env, NapiStatus::InvalidArg, "value is not a live handle");
    };
    let js = JSValue::from_bits(bits);
    let value_type = if js.is_undefined() {
        NapiValueType::Undefined
    } else if js.is_null() {
        NapiValueType::Null
    } else if js.is_bool() {
        NapiValueType::Boolean
    } else if js.is_number() || js.is_int32() {
        NapiValueType::Number
    } else if js.is_any_string() {
        NapiValueType::String
    } else if js.is_bigint() {
        NapiValueType::Bigint
    } else if crate::symbol::js_is_symbol(f64::from_bits(bits)) != 0 {
        NapiValueType::Symbol
    } else if js.is_pointer() && crate::closure::is_closure_ptr(js.as_pointer::<u8>() as usize) {
        NapiValueType::Function
    } else {
        NapiValueType::Object
    };
    *result = value_type;
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_string_utf8(
    env: NapiEnv,
    value: *const c_char,
    length: usize,
    result: *mut NapiValue,
) -> NapiStatus {
    let Ok(length) = input_len(value, length) else {
        return set_status(env, NapiStatus::InvalidArg, "string data must not be null");
    };
    let bytes = std::slice::from_raw_parts(value.cast::<u8>(), length);
    create_string(env, bytes, false, result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_string_latin1(
    env: NapiEnv,
    value: *const c_char,
    length: usize,
    result: *mut NapiValue,
) -> NapiStatus {
    let Ok(length) = input_len(value, length) else {
        return set_status(env, NapiStatus::InvalidArg, "string data must not be null");
    };
    let input = std::slice::from_raw_parts(value.cast::<u8>(), length);
    let mut utf8 = Vec::with_capacity(length);
    for &byte in input {
        push_wtf8(byte as u32, &mut utf8);
    }
    create_string(env, &utf8, false, result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_string_utf16(
    env: NapiEnv,
    value: *const u16,
    length: usize,
    result: *mut NapiValue,
) -> NapiStatus {
    if value.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "string data must not be null");
    }
    let length = if length == NAPI_AUTO_LENGTH {
        let mut len = 0;
        while *value.add(len) != 0 {
            len += 1;
        }
        len
    } else {
        length
    };
    if length > i32::MAX as usize {
        return set_status(env, NapiStatus::InvalidArg, "string length exceeds i32");
    }
    let wtf8 = utf16_to_wtf8(std::slice::from_raw_parts(value, length));
    create_string(env, &wtf8, true, result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_value_string_utf8(
    env: NapiEnv,
    value: NapiValue,
    buffer: *mut c_char,
    buffer_size: usize,
    result: *mut usize,
) -> NapiStatus {
    let bytes = match get_string_source(env, value) {
        Ok(bytes) => wtf8_to_utf8(&bytes),
        Err(status) => return set_string_error(env, status),
    };
    let copied = if buffer.is_null() || buffer_size == 0 {
        0
    } else {
        let copied = bytes.len().min(buffer_size - 1);
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer.cast::<u8>(), copied);
        *buffer.add(copied) = 0;
        copied
    };
    if !result.is_null() {
        *result = if buffer.is_null() {
            bytes.len()
        } else {
            copied
        };
    }
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_value_string_latin1(
    env: NapiEnv,
    value: NapiValue,
    buffer: *mut c_char,
    buffer_size: usize,
    result: *mut usize,
) -> NapiStatus {
    let bytes = match get_string_source(env, value) {
        Ok(bytes) => wtf8_code_points(&bytes)
            .into_iter()
            .map(|code| if code <= 0xff { code as u8 } else { b'?' })
            .collect::<Vec<_>>(),
        Err(status) => return set_string_error(env, status),
    };
    let copied = if buffer.is_null() || buffer_size == 0 {
        0
    } else {
        let copied = bytes.len().min(buffer_size - 1);
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer.cast::<u8>(), copied);
        *buffer.add(copied) = 0;
        copied
    };
    if !result.is_null() {
        *result = if buffer.is_null() {
            bytes.len()
        } else {
            copied
        };
    }
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_value_string_utf16(
    env: NapiEnv,
    value: NapiValue,
    buffer: *mut u16,
    buffer_size: usize,
    result: *mut usize,
) -> NapiStatus {
    let units = match get_string_source(env, value) {
        Ok(bytes) => wtf8_to_utf16(&bytes),
        Err(status) => return set_string_error(env, status),
    };
    let copied = if buffer.is_null() || buffer_size == 0 {
        0
    } else {
        let copied = units.len().min(buffer_size - 1);
        std::ptr::copy_nonoverlapping(units.as_ptr(), buffer, copied);
        *buffer.add(copied) = 0;
        copied
    };
    if !result.is_null() {
        *result = if buffer.is_null() {
            units.len()
        } else {
            copied
        };
    }
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_coerce_to_bool(
    env: NapiEnv,
    value: NapiValue,
    result: *mut NapiValue,
) -> NapiStatus {
    let Ok(bits) = value_bits(env, value) else {
        return set_status(env, NapiStatus::InvalidArg, "value is not a live handle");
    };
    write_handle(
        env,
        JSValue::bool(crate::value::js_is_truthy(f64::from_bits(bits)) != 0).bits(),
        result,
    )
}

#[no_mangle]
pub unsafe extern "C" fn napi_coerce_to_number(
    env: NapiEnv,
    value: NapiValue,
    result: *mut NapiValue,
) -> NapiStatus {
    let Ok(bits) = value_bits(env, value) else {
        return set_status(env, NapiStatus::InvalidArg, "value is not a live handle");
    };
    match catch_value_call(env, || {
        crate::builtins::js_number_coerce(f64::from_bits(bits))
    }) {
        Ok(value) => write_handle(env, value.to_bits(), result),
        Err(status) => status,
    }
}

#[no_mangle]
pub unsafe extern "C" fn napi_coerce_to_string(
    env: NapiEnv,
    value: NapiValue,
    result: *mut NapiValue,
) -> NapiStatus {
    let Ok(bits) = value_bits(env, value) else {
        return set_status(env, NapiStatus::InvalidArg, "value is not a live handle");
    };
    match catch_value_call(env, || {
        let ptr = crate::value::js_jsvalue_to_string(f64::from_bits(bits));
        f64::from_bits(string_bits(ptr))
    }) {
        Ok(value) => write_handle(env, value.to_bits(), result),
        Err(status) => status,
    }
}

#[no_mangle]
pub unsafe extern "C" fn napi_coerce_to_object(
    env: NapiEnv,
    value: NapiValue,
    result: *mut NapiValue,
) -> NapiStatus {
    let Ok(bits) = value_bits(env, value) else {
        return set_status(env, NapiStatus::InvalidArg, "value is not a live handle");
    };
    match catch_value_call(env, || {
        crate::object::js_object_coerce(f64::from_bits(bits))
    }) {
        Ok(value) => write_handle(env, value.to_bits(), result),
        Err(status) => status,
    }
}

#[no_mangle]
pub unsafe extern "C" fn napi_set_property(
    env: NapiEnv,
    object: NapiValue,
    key: NapiValue,
    value: NapiValue,
) -> NapiStatus {
    let Ok(value_bits) = value_bits(env, value) else {
        return set_status(env, NapiStatus::InvalidArg, "value is not a live handle");
    };
    match property_call(env, object, key, |object, key| {
        crate::object::js_object_set_property_key(object, key, f64::from_bits(value_bits))
    }) {
        Ok(_) => ok(env),
        Err(status) => set_status(env, status, "property assignment failed"),
    }
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_property(
    env: NapiEnv,
    object: NapiValue,
    key: NapiValue,
    result: *mut NapiValue,
) -> NapiStatus {
    match property_call(env, object, key, |object, key| unsafe {
        crate::object::js_object_get_property_key(object, key)
    }) {
        Ok(value) => write_handle(env, value.to_bits(), result),
        Err(status) => set_status(env, status, "property lookup failed"),
    }
}

#[no_mangle]
pub unsafe extern "C" fn napi_has_property(
    env: NapiEnv,
    object: NapiValue,
    key: NapiValue,
    result: *mut bool,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    match property_call(env, object, key, |object, key| {
        crate::object::js_object_has_property(object, key)
    }) {
        Ok(value) => {
            *result = JSValue::from_bits(value.to_bits()).to_bool();
            ok(env)
        }
        Err(status) => set_status(env, status, "property lookup failed"),
    }
}

#[no_mangle]
pub unsafe extern "C" fn napi_has_own_property(
    env: NapiEnv,
    object: NapiValue,
    key: NapiValue,
    result: *mut bool,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    match property_call(env, object, key, |object, key| {
        crate::object::js_object_has_own(object, key)
    }) {
        Ok(value) => {
            *result = JSValue::from_bits(value.to_bits()).to_bool();
            ok(env)
        }
        Err(status) => set_status(env, status, "own-property lookup failed"),
    }
}

#[no_mangle]
pub unsafe extern "C" fn napi_delete_property(
    env: NapiEnv,
    object: NapiValue,
    key: NapiValue,
    result: *mut bool,
) -> NapiStatus {
    match property_call(env, object, key, |object, key| {
        f64::from(crate::object::js_object_delete_dynamic_value(object, key))
    }) {
        Ok(value) => {
            if !result.is_null() {
                *result = value != 0.0;
            }
            ok(env)
        }
        Err(status) => set_status(env, status, "property deletion failed"),
    }
}

#[no_mangle]
pub unsafe extern "C" fn napi_set_named_property(
    env: NapiEnv,
    object: NapiValue,
    name: *const c_char,
    value: NapiValue,
) -> NapiStatus {
    let key = match named_key(env, name) {
        Ok(key) => key,
        Err(status) => return set_status(env, status, "property name must not be null"),
    };
    napi_set_property(env, object, key, value)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_named_property(
    env: NapiEnv,
    object: NapiValue,
    name: *const c_char,
    result: *mut NapiValue,
) -> NapiStatus {
    let key = match named_key(env, name) {
        Ok(key) => key,
        Err(status) => return set_status(env, status, "property name must not be null"),
    };
    napi_get_property(env, object, key, result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_has_named_property(
    env: NapiEnv,
    object: NapiValue,
    name: *const c_char,
    result: *mut bool,
) -> NapiStatus {
    let key = match named_key(env, name) {
        Ok(key) => key,
        Err(status) => return set_status(env, status, "property name must not be null"),
    };
    napi_has_property(env, object, key, result)
}

fn element_key(env: NapiEnv, index: u32) -> Result<NapiValue, NapiStatus> {
    add_handle(env, JSValue::number(index as f64).bits())
}

#[no_mangle]
pub unsafe extern "C" fn napi_set_element(
    env: NapiEnv,
    object: NapiValue,
    index: u32,
    value: NapiValue,
) -> NapiStatus {
    let Ok(key) = element_key(env, index) else {
        return NapiStatus::InvalidArg;
    };
    napi_set_property(env, object, key, value)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_element(
    env: NapiEnv,
    object: NapiValue,
    index: u32,
    result: *mut NapiValue,
) -> NapiStatus {
    let Ok(key) = element_key(env, index) else {
        return NapiStatus::InvalidArg;
    };
    napi_get_property(env, object, key, result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_has_element(
    env: NapiEnv,
    object: NapiValue,
    index: u32,
    result: *mut bool,
) -> NapiStatus {
    let Ok(key) = element_key(env, index) else {
        return NapiStatus::InvalidArg;
    };
    napi_has_property(env, object, key, result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_delete_element(
    env: NapiEnv,
    object: NapiValue,
    index: u32,
    result: *mut bool,
) -> NapiStatus {
    let Ok(key) = element_key(env, index) else {
        return NapiStatus::InvalidArg;
    };
    napi_delete_property(env, object, key, result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_is_array(
    env: NapiEnv,
    value: NapiValue,
    result: *mut bool,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let Ok(bits) = value_bits(env, value) else {
        return set_status(env, NapiStatus::InvalidArg, "value is not a live handle");
    };
    *result = JSValue::from_bits(crate::array::js_array_is_array(f64::from_bits(bits)).to_bits())
        .to_bool();
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_array_length(
    env: NapiEnv,
    value: NapiValue,
    result: *mut u32,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let Ok(bits) = value_bits(env, value) else {
        return set_status(env, NapiStatus::InvalidArg, "value is not a live handle");
    };
    if !JSValue::from_bits(crate::array::js_array_is_array(f64::from_bits(bits)).to_bits())
        .to_bool()
    {
        return set_status(env, NapiStatus::ArrayExpected, "value must be an array");
    }
    let mut length_value = std::ptr::null_mut();
    let status = napi_get_named_property(env, value, c"length".as_ptr(), &mut length_value);
    if status != NapiStatus::Ok {
        return status;
    }
    napi_get_value_uint32(env, length_value, result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_strict_equals(
    env: NapiEnv,
    lhs: NapiValue,
    rhs: NapiValue,
    result: *mut bool,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let (Ok(lhs), Ok(rhs)) = (value_bits(env, lhs), value_bits(env, rhs)) else {
        return set_status(env, NapiStatus::InvalidArg, "values must be live handles");
    };
    *result = crate::value::js_jsvalue_equals(f64::from_bits(lhs), f64::from_bits(rhs)) != 0;
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_prototype(
    env: NapiEnv,
    object: NapiValue,
    result: *mut NapiValue,
) -> NapiStatus {
    let Ok(bits) = value_bits(env, object) else {
        return set_status(env, NapiStatus::InvalidArg, "object is not a live handle");
    };
    match catch_value_call(env, || {
        crate::object::js_object_get_prototype_of(f64::from_bits(bits))
    }) {
        Ok(value) => write_handle(env, value.to_bits(), result),
        Err(status) => status,
    }
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_property_names(
    env: NapiEnv,
    object: NapiValue,
    result: *mut NapiValue,
) -> NapiStatus {
    let Ok(bits) = value_bits(env, object) else {
        return set_status(env, NapiStatus::InvalidArg, "object is not a live handle");
    };
    match catch_value_call(env, || {
        let array = crate::object::js_object_keys_value(f64::from_bits(bits));
        f64::from_bits(pointer_bits(array.cast()))
    }) {
        Ok(value) => write_handle(env, value.to_bits(), result),
        Err(status) => status,
    }
}

fn create_error_kind(
    env: NapiEnv,
    code: NapiValue,
    message: NapiValue,
    result: *mut NapiValue,
    kind: extern "C" fn(*mut crate::string::StringHeader) -> *mut crate::error::ErrorHeader,
) -> NapiStatus {
    let Ok(message_bits) = value_bits(env, message) else {
        return set_status(env, NapiStatus::InvalidArg, "message is not a live handle");
    };
    if !JSValue::from_bits(message_bits).is_any_string() {
        return set_status(env, NapiStatus::StringExpected, "message must be a string");
    }
    let message_ptr = crate::value::js_get_string_pointer_unified(f64::from_bits(message_bits))
        as *mut crate::string::StringHeader;
    let scope = crate::gc::RuntimeHandleScope::new();
    let message_root = scope.root_string_ptr(message_ptr);
    let error = kind(
        message_root
            .get_raw_const_ptr::<crate::string::StringHeader>()
            .cast_mut(),
    );
    let status = write_handle(env, pointer_bits(error.cast()), result);
    if status != NapiStatus::Ok || code.is_null() {
        return status;
    }
    let error_handle = unsafe { *result };
    unsafe { napi_set_named_property(env, error_handle, c"code".as_ptr(), code) }
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_error(
    env: NapiEnv,
    code: NapiValue,
    message: NapiValue,
    result: *mut NapiValue,
) -> NapiStatus {
    create_error_kind(
        env,
        code,
        message,
        result,
        crate::error::js_error_new_with_message,
    )
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_type_error(
    env: NapiEnv,
    code: NapiValue,
    message: NapiValue,
    result: *mut NapiValue,
) -> NapiStatus {
    create_error_kind(env, code, message, result, crate::error::js_typeerror_new)
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_range_error(
    env: NapiEnv,
    code: NapiValue,
    message: NapiValue,
    result: *mut NapiValue,
) -> NapiStatus {
    create_error_kind(env, code, message, result, crate::error::js_rangeerror_new)
}

#[no_mangle]
pub unsafe extern "C" fn napi_is_error(
    env: NapiEnv,
    value: NapiValue,
    result: *mut bool,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let Ok(bits) = value_bits(env, value) else {
        return set_status(env, NapiStatus::InvalidArg, "value is not a live handle");
    };
    *result = JSValue::from_bits(crate::error::js_error_is_error(f64::from_bits(bits)).to_bits())
        .to_bool();
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_bigint_int64(
    env: NapiEnv,
    value: i64,
    result: *mut NapiValue,
) -> NapiStatus {
    write_handle(
        env,
        bigint_bits(crate::bigint::js_bigint_from_i64(value)),
        result,
    )
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_bigint_uint64(
    env: NapiEnv,
    value: u64,
    result: *mut NapiValue,
) -> NapiStatus {
    write_handle(
        env,
        bigint_bits(crate::bigint::js_bigint_from_u64(value)),
        result,
    )
}

fn bigint_value(
    env: NapiEnv,
    value: NapiValue,
) -> Result<*const crate::bigint::BigIntHeader, NapiStatus> {
    let bits = value_bits(env, value)?;
    let value = JSValue::from_bits(bits);
    if !value.is_bigint() {
        return Err(NapiStatus::BigintExpected);
    }
    Ok(value.as_bigint_ptr())
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_value_bigint_int64(
    env: NapiEnv,
    value: NapiValue,
    result: *mut i64,
    lossless: *mut bool,
) -> NapiStatus {
    if result.is_null() || lossless.is_null() {
        return set_status(
            env,
            NapiStatus::InvalidArg,
            "result and lossless must not be null",
        );
    }
    let pointer = match bigint_value(env, value) {
        Ok(pointer) => pointer,
        Err(NapiStatus::BigintExpected) => {
            return set_status(env, NapiStatus::BigintExpected, "value must be a BigInt");
        }
        Err(status) => return set_status(env, status, "value is not a live handle"),
    };
    let limbs = (*pointer).limbs;
    let low = limbs[0] as i64;
    let fill = if low < 0 { u64::MAX } else { 0 };
    *result = low;
    *lossless = limbs[1..].iter().all(|limb| *limb == fill);
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_value_bigint_uint64(
    env: NapiEnv,
    value: NapiValue,
    result: *mut u64,
    lossless: *mut bool,
) -> NapiStatus {
    if result.is_null() || lossless.is_null() {
        return set_status(
            env,
            NapiStatus::InvalidArg,
            "result and lossless must not be null",
        );
    }
    let pointer = match bigint_value(env, value) {
        Ok(pointer) => pointer,
        Err(NapiStatus::BigintExpected) => {
            return set_status(env, NapiStatus::BigintExpected, "value must be a BigInt");
        }
        Err(status) => return set_status(env, status, "value is not a live handle"),
    };
    let limbs = (*pointer).limbs;
    *result = limbs[0];
    *lossless = limbs[1..].iter().all(|limb| *limb == 0);
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_symbol(
    env: NapiEnv,
    description: NapiValue,
    result: *mut NapiValue,
) -> NapiStatus {
    let description = if description.is_null() {
        std::ptr::null_mut()
    } else {
        let Ok(bits) = value_bits(env, description) else {
            return set_status(
                env,
                NapiStatus::InvalidArg,
                "description is not a live handle",
            );
        };
        if !JSValue::from_bits(bits).is_any_string() {
            return set_status(
                env,
                NapiStatus::StringExpected,
                "description must be a string",
            );
        }
        crate::value::js_get_string_pointer_unified(f64::from_bits(bits))
            as *mut crate::string::StringHeader
    };
    let symbol = if description.is_null() {
        crate::symbol::alloc_symbol(std::ptr::null_mut(), false)
    } else {
        let scope = crate::gc::RuntimeHandleScope::new();
        let description_root = scope.root_string_ptr(description);
        crate::symbol::alloc_symbol(
            description_root
                .get_raw_const_ptr::<crate::string::StringHeader>()
                .cast_mut(),
            false,
        )
    };
    write_handle(env, pointer_bits(symbol.cast()), result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_date(
    env: NapiEnv,
    time: f64,
    result: *mut NapiValue,
) -> NapiStatus {
    write_handle(
        env,
        crate::date::js_date_new_from_timestamp(time).to_bits(),
        result,
    )
}

#[no_mangle]
pub unsafe extern "C" fn napi_is_date(
    env: NapiEnv,
    value: NapiValue,
    result: *mut bool,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let Ok(bits) = value_bits(env, value) else {
        return set_status(env, NapiStatus::InvalidArg, "value is not a live handle");
    };
    *result = crate::date::is_date_value(f64::from_bits(bits));
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_date_value(
    env: NapiEnv,
    value: NapiValue,
    result: *mut f64,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let Ok(bits) = value_bits(env, value) else {
        return set_status(env, NapiStatus::InvalidArg, "value is not a live handle");
    };
    if !crate::date::is_date_value(f64::from_bits(bits)) {
        return set_status(env, NapiStatus::DateExpected, "value must be a Date");
    }
    *result = crate::date::date_cell_timestamp(f64::from_bits(bits));
    ok(env)
}

fn throw_c_error(
    env: NapiEnv,
    code: *const c_char,
    message: *const c_char,
    create: unsafe extern "C" fn(NapiEnv, NapiValue, NapiValue, *mut NapiValue) -> NapiStatus,
) -> NapiStatus {
    if message.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "message must not be null");
    }
    let mut message_value = std::ptr::null_mut();
    let status =
        unsafe { napi_create_string_utf8(env, message, NAPI_AUTO_LENGTH, &mut message_value) };
    if status != NapiStatus::Ok {
        return status;
    }
    let mut code_value = std::ptr::null_mut();
    if !code.is_null() {
        let status =
            unsafe { napi_create_string_utf8(env, code, NAPI_AUTO_LENGTH, &mut code_value) };
        if status != NapiStatus::Ok {
            return status;
        }
    }
    let mut error = std::ptr::null_mut();
    let status = unsafe { create(env, code_value, message_value, &mut error) };
    if status != NapiStatus::Ok {
        return status;
    }
    unsafe { napi_throw(env, error) }
}

#[no_mangle]
pub unsafe extern "C" fn napi_throw_error(
    env: NapiEnv,
    code: *const c_char,
    message: *const c_char,
) -> NapiStatus {
    throw_c_error(env, code, message, napi_create_error)
}

#[no_mangle]
pub unsafe extern "C" fn napi_throw_type_error(
    env: NapiEnv,
    code: *const c_char,
    message: *const c_char,
) -> NapiStatus {
    throw_c_error(env, code, message, napi_create_type_error)
}

#[no_mangle]
pub unsafe extern "C" fn napi_throw_range_error(
    env: NapiEnv,
    code: *const c_char,
    message: *const c_char,
) -> NapiStatus {
    throw_c_error(env, code, message, napi_create_range_error)
}

#[no_mangle]
pub unsafe extern "C" fn napi_instanceof(
    env: NapiEnv,
    object: NapiValue,
    constructor: NapiValue,
    result: *mut bool,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let (Ok(object), Ok(constructor)) = (value_bits(env, object), value_bits(env, constructor))
    else {
        return set_status(env, NapiStatus::InvalidArg, "values must be live handles");
    };
    match catch_value_call(env, || {
        crate::object::js_instanceof_dynamic(f64::from_bits(object), f64::from_bits(constructor))
    }) {
        Ok(value) => {
            *result = JSValue::from_bits(value.to_bits()).to_bool();
            ok(env)
        }
        Err(status) => status,
    }
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_external(
    env: NapiEnv,
    _data: *mut c_void,
    _finalize_cb: Option<unsafe extern "C" fn(NapiEnv, *mut c_void, *mut c_void)>,
    _finalize_hint: *mut c_void,
    _result: *mut NapiValue,
) -> NapiStatus {
    set_status(
        env,
        NapiStatus::GenericFailure,
        "external values and finalizers are not enabled in this host core",
    )
}
