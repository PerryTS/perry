use super::*;
use std::collections::HashSet;
use std::ffi::{c_char, c_void};

pub const NAPI_WRITABLE: u32 = 1 << 0;
pub const NAPI_ENUMERABLE: u32 = 1 << 1;
pub const NAPI_CONFIGURABLE: u32 = 1 << 2;
pub const NAPI_STATIC: u32 = 1 << 10;
const NAPI_KNOWN_PROPERTY_ATTRIBUTES: u32 =
    NAPI_WRITABLE | NAPI_ENUMERABLE | NAPI_CONFIGURABLE | NAPI_STATIC;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NapiPropertyDescriptor {
    pub utf8name: *const c_char,
    pub name: NapiValue,
    pub method: NapiCallback,
    pub getter: NapiCallback,
    pub setter: NapiCallback,
    pub value: NapiValue,
    pub attributes: u32,
    pub data: *mut c_void,
}

#[repr(i32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NapiKeyCollectionMode {
    IncludePrototypes = 0,
    OwnOnly = 1,
}

#[repr(i32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NapiKeyConversion {
    KeepNumbers = 0,
    NumbersToStrings = 1,
}

pub const NAPI_KEY_ALL_PROPERTIES: u32 = 0;
pub const NAPI_KEY_WRITABLE: u32 = 1 << 0;
pub const NAPI_KEY_ENUMERABLE: u32 = 1 << 1;
pub const NAPI_KEY_CONFIGURABLE: u32 = 1 << 2;
pub const NAPI_KEY_SKIP_STRINGS: u32 = 1 << 3;
pub const NAPI_KEY_SKIP_SYMBOLS: u32 = 1 << 4;
const NAPI_KNOWN_KEY_FILTER: u32 = NAPI_KEY_WRITABLE
    | NAPI_KEY_ENUMERABLE
    | NAPI_KEY_CONFIGURABLE
    | NAPI_KEY_SKIP_STRINGS
    | NAPI_KEY_SKIP_SYMBOLS;

#[derive(Hash, Eq, PartialEq)]
enum PropertyIdentity {
    String(Vec<u8>),
    Symbol(u64),
}

#[derive(Clone, Copy)]
struct PreparedDescriptor {
    raw: NapiPropertyDescriptor,
}

fn prepare_descriptors(
    env: NapiEnv,
    count: usize,
    properties: *const NapiPropertyDescriptor,
) -> Result<Vec<PreparedDescriptor>, NapiStatus> {
    if count != 0 && properties.is_null() {
        return Err(set_status(
            env,
            NapiStatus::InvalidArg,
            "property descriptors must not be null",
        ));
    }
    let descriptors = unsafe { std::slice::from_raw_parts(properties, count) };
    let mut prepared = Vec::with_capacity(count);
    for descriptor in descriptors {
        if descriptor.name.is_null() && descriptor.utf8name.is_null() {
            return Err(set_status(
                env,
                NapiStatus::NameExpected,
                "a property descriptor needs a name",
            ));
        }
        if !descriptor.name.is_null() && value_bits(env, descriptor.name).is_err() {
            return Err(set_status(
                env,
                NapiStatus::InvalidArg,
                "property name is not a live handle",
            ));
        }
        if descriptor.attributes & !NAPI_KNOWN_PROPERTY_ATTRIBUTES != 0 {
            return Err(set_status(
                env,
                NapiStatus::InvalidArg,
                "property descriptor has unknown attributes",
            ));
        }
        let data_members =
            usize::from(!descriptor.value.is_null()) + usize::from(descriptor.method.is_some());
        let accessor_members =
            usize::from(descriptor.getter.is_some()) + usize::from(descriptor.setter.is_some());
        if data_members > 1 || (data_members != 0 && accessor_members != 0) {
            return Err(set_status(
                env,
                NapiStatus::InvalidArg,
                "property descriptor mixes value, method, and accessor fields",
            ));
        }
        if !descriptor.value.is_null() && value_bits(env, descriptor.value).is_err() {
            return Err(set_status(
                env,
                NapiStatus::InvalidArg,
                "property value is not a live handle",
            ));
        }
        prepared.push(PreparedDescriptor { raw: *descriptor });
    }
    Ok(prepared)
}

unsafe fn property_name_handle(
    env: NapiEnv,
    descriptor: &NapiPropertyDescriptor,
) -> Result<NapiValue, NapiStatus> {
    if !descriptor.name.is_null() {
        return Ok(descriptor.name);
    }
    let mut name = std::ptr::null_mut();
    let status = napi_create_string_utf8(env, descriptor.utf8name, NAPI_AUTO_LENGTH, &mut name);
    if status == NapiStatus::Ok {
        Ok(name)
    } else {
        Err(status)
    }
}

unsafe fn set_descriptor_field(
    env: NapiEnv,
    descriptor_object: NapiValue,
    field: &'static std::ffi::CStr,
    value: NapiValue,
) -> Result<(), NapiStatus> {
    let status = napi_set_named_property(env, descriptor_object, field.as_ptr(), value);
    (status == NapiStatus::Ok).then_some(()).ok_or(status)
}

unsafe fn set_descriptor_bool(
    env: NapiEnv,
    descriptor_object: NapiValue,
    field: &'static std::ffi::CStr,
    value: bool,
) -> Result<(), NapiStatus> {
    let mut handle = std::ptr::null_mut();
    let status = napi_get_boolean(env, value, &mut handle);
    if status != NapiStatus::Ok {
        return Err(status);
    }
    set_descriptor_field(env, descriptor_object, field, handle)
}

unsafe fn callback_value(
    env: NapiEnv,
    name: *const c_char,
    callback: NapiCallback,
    data: *mut c_void,
) -> Result<NapiValue, NapiStatus> {
    let mut value = std::ptr::null_mut();
    let status = napi_create_function(env, name, NAPI_AUTO_LENGTH, callback, data, &mut value);
    if status == NapiStatus::Ok {
        Ok(value)
    } else {
        Err(status)
    }
}

unsafe fn define_one(
    env: NapiEnv,
    target: NapiValue,
    descriptor: &NapiPropertyDescriptor,
) -> Result<(), NapiStatus> {
    let key = property_name_handle(env, descriptor)?;
    let mut descriptor_object = std::ptr::null_mut();
    let status = napi_create_object(env, &mut descriptor_object);
    if status != NapiStatus::Ok {
        return Err(status);
    }

    if let Some(method) = descriptor.method {
        let value = callback_value(env, descriptor.utf8name, Some(method), descriptor.data)?;
        set_descriptor_field(env, descriptor_object, c"value", value)?;
        set_descriptor_bool(
            env,
            descriptor_object,
            c"writable",
            descriptor.attributes & NAPI_WRITABLE != 0,
        )?;
    } else if !descriptor.value.is_null() {
        set_descriptor_field(env, descriptor_object, c"value", descriptor.value)?;
        set_descriptor_bool(
            env,
            descriptor_object,
            c"writable",
            descriptor.attributes & NAPI_WRITABLE != 0,
        )?;
    } else if descriptor.getter.is_some() || descriptor.setter.is_some() {
        if let Some(getter) = descriptor.getter {
            let value = callback_value(env, descriptor.utf8name, Some(getter), descriptor.data)?;
            set_descriptor_field(env, descriptor_object, c"get", value)?;
        }
        if let Some(setter) = descriptor.setter {
            let value = callback_value(env, descriptor.utf8name, Some(setter), descriptor.data)?;
            set_descriptor_field(env, descriptor_object, c"set", value)?;
        }
    } else {
        let undefined = add_handle(env, crate::value::TAG_UNDEFINED)?;
        set_descriptor_field(env, descriptor_object, c"value", undefined)?;
        set_descriptor_bool(
            env,
            descriptor_object,
            c"writable",
            descriptor.attributes & NAPI_WRITABLE != 0,
        )?;
    }
    set_descriptor_bool(
        env,
        descriptor_object,
        c"enumerable",
        descriptor.attributes & NAPI_ENUMERABLE != 0,
    )?;
    set_descriptor_bool(
        env,
        descriptor_object,
        c"configurable",
        descriptor.attributes & NAPI_CONFIGURABLE != 0,
    )?;

    let target_bits = value_bits(env, target)?;
    let key_bits = value_bits(env, key)?;
    let descriptor_bits = value_bits(env, descriptor_object)?;
    catch_value_call(env, || {
        crate::object::js_object_define_property(
            f64::from_bits(target_bits),
            f64::from_bits(key_bits),
            f64::from_bits(descriptor_bits),
        )
    })?;
    Ok(())
}

#[no_mangle]
pub unsafe extern "C" fn napi_define_properties(
    env: NapiEnv,
    object: NapiValue,
    property_count: usize,
    properties: *const NapiPropertyDescriptor,
) -> NapiStatus {
    if value_bits(env, object).is_err() {
        return set_status(env, NapiStatus::InvalidArg, "object is not a live handle");
    }
    let prepared = match prepare_descriptors(env, property_count, properties) {
        Ok(prepared) => prepared,
        Err(status) => return status,
    };
    for descriptor in &prepared {
        if let Err(status) = define_one(env, object, &descriptor.raw) {
            return status;
        }
    }
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_define_class(
    env: NapiEnv,
    utf8name: *const c_char,
    length: usize,
    constructor: NapiCallback,
    data: *mut c_void,
    property_count: usize,
    properties: *const NapiPropertyDescriptor,
    result: *mut NapiValue,
) -> NapiStatus {
    if result.is_null() || constructor.is_none() || utf8name.is_null() {
        return set_status(
            env,
            NapiStatus::InvalidArg,
            "class name, constructor, and result must not be null",
        );
    }
    let prepared = match prepare_descriptors(env, property_count, properties) {
        Ok(prepared) => prepared,
        Err(status) => return status,
    };
    let mut constructor_value = std::ptr::null_mut();
    let status = napi_create_function(
        env,
        utf8name,
        length,
        constructor,
        data,
        &mut constructor_value,
    );
    if status != NapiStatus::Ok {
        return status;
    }
    let mut prototype = std::ptr::null_mut();
    let status = napi_create_object(env, &mut prototype);
    if status != NapiStatus::Ok {
        return status;
    }
    let status = napi_set_named_property(env, constructor_value, c"prototype".as_ptr(), prototype);
    if status != NapiStatus::Ok {
        return status;
    }
    let status =
        napi_set_named_property(env, prototype, c"constructor".as_ptr(), constructor_value);
    if status != NapiStatus::Ok {
        return status;
    }
    for descriptor in &prepared {
        let target = if descriptor.raw.attributes & NAPI_STATIC != 0 {
            constructor_value
        } else {
            prototype
        };
        if let Err(status) = define_one(env, target, &descriptor.raw) {
            return status;
        }
    }
    *result = constructor_value;
    ok(env)
}

fn string_key_bytes(bits: u64) -> Option<Vec<u8>> {
    if !crate::value::JSValue::from_bits(bits).is_any_string() {
        return None;
    }
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let (pointer, length) =
        crate::string::str_bytes_from_jsvalue(f64::from_bits(bits), &mut scratch)?;
    if length == 0 {
        return Some(Vec::new());
    }
    Some(unsafe { std::slice::from_raw_parts(pointer, length as usize) }.to_vec())
}

fn canonical_array_index(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() || (bytes.len() > 1 && bytes[0] == b'0') {
        return None;
    }
    let mut value = 0u32;
    for byte in bytes {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value
            .checked_mul(10)?
            .checked_add(u32::from(*byte - b'0'))?;
    }
    (value != u32::MAX).then_some(value)
}

unsafe fn descriptor_flag(
    env: NapiEnv,
    descriptor: NapiValue,
    name: &'static std::ffi::CStr,
) -> Result<bool, NapiStatus> {
    let mut value = std::ptr::null_mut();
    let status = napi_get_named_property(env, descriptor, name.as_ptr(), &mut value);
    if status != NapiStatus::Ok {
        return Err(status);
    }
    let bits = value_bits(env, value)?;
    let value = crate::value::JSValue::from_bits(bits);
    Ok(value.is_bool() && value.as_bool())
}

unsafe fn descriptor_matches_filter(
    env: NapiEnv,
    descriptor: NapiValue,
    filter: u32,
) -> Result<bool, NapiStatus> {
    for (flag, name) in [
        (NAPI_KEY_WRITABLE, c"writable"),
        (NAPI_KEY_ENUMERABLE, c"enumerable"),
        (NAPI_KEY_CONFIGURABLE, c"configurable"),
    ] {
        if filter & flag != 0 && !descriptor_flag(env, descriptor, name)? {
            return Ok(false);
        }
    }
    Ok(true)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_all_property_names(
    env: NapiEnv,
    object: NapiValue,
    key_mode: NapiKeyCollectionMode,
    key_filter: u32,
    key_conversion: NapiKeyConversion,
    result: *mut NapiValue,
) -> NapiStatus {
    if result.is_null() || key_filter & !NAPI_KNOWN_KEY_FILTER != 0 {
        return set_status(
            env,
            NapiStatus::InvalidArg,
            "property-name options are invalid",
        );
    }
    if value_bits(env, object).is_err() {
        return set_status(env, NapiStatus::InvalidArg, "object is not a live handle");
    }
    let mut output = std::ptr::null_mut();
    let status = napi_create_array(env, &mut output);
    if status != NapiStatus::Ok {
        return status;
    }
    let mut seen = HashSet::new();
    let mut current = object;
    let mut output_index = 0u32;
    let mut prototype_depth = 0usize;
    loop {
        let current_bits = match value_bits(env, current) {
            Ok(bits) => bits,
            Err(status) => return set_status(env, status, "prototype handle is invalid"),
        };
        let keys_bits = match catch_value_call(env, || {
            crate::proxy::js_reflect_own_keys(f64::from_bits(current_bits))
        }) {
            Ok(value) => value.to_bits(),
            Err(status) => return status,
        };
        let keys_handle = match add_handle(env, keys_bits) {
            Ok(handle) => handle,
            Err(status) => return status,
        };
        let keys_pointer =
            crate::value::JSValue::from_bits(keys_bits).as_pointer::<crate::array::ArrayHeader>();
        let length = crate::array::js_array_length(keys_pointer);
        for index in 0..length {
            let refreshed_keys = match value_bits(env, keys_handle) {
                Ok(bits) => {
                    crate::value::JSValue::from_bits(bits).as_pointer::<crate::array::ArrayHeader>()
                }
                Err(status) => return status,
            };
            let original_bits = crate::array::js_array_get_f64(refreshed_keys, index).to_bits();
            let is_symbol = crate::symbol::js_is_symbol(f64::from_bits(original_bits)) != 0;
            let string_bytes = string_key_bytes(original_bits);
            if (is_symbol && key_filter & NAPI_KEY_SKIP_SYMBOLS != 0)
                || (string_bytes.is_some() && key_filter & NAPI_KEY_SKIP_STRINGS != 0)
            {
                continue;
            }
            let identity = if is_symbol {
                PropertyIdentity::Symbol(original_bits)
            } else if let Some(bytes) = string_bytes.clone() {
                PropertyIdentity::String(bytes)
            } else {
                continue;
            };
            if !seen.insert(identity) {
                continue;
            }
            let key_handle = match add_handle(env, original_bits) {
                Ok(handle) => handle,
                Err(status) => return status,
            };
            let refreshed_object_bits = match value_bits(env, current) {
                Ok(bits) => bits,
                Err(status) => return status,
            };
            let refreshed_key_bits = match value_bits(env, key_handle) {
                Ok(bits) => bits,
                Err(status) => return status,
            };
            let descriptor_bits = match catch_value_call(env, || {
                crate::object::js_object_get_own_property_descriptor(
                    f64::from_bits(refreshed_object_bits),
                    f64::from_bits(refreshed_key_bits),
                )
            }) {
                Ok(value) => value.to_bits(),
                Err(status) => return status,
            };
            if crate::value::JSValue::from_bits(descriptor_bits).is_undefined() {
                continue;
            }
            let descriptor = match add_handle(env, descriptor_bits) {
                Ok(handle) => handle,
                Err(status) => return status,
            };
            match descriptor_matches_filter(env, descriptor, key_filter) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(status) => return status,
            }
            let output_key = if key_conversion == NapiKeyConversion::KeepNumbers {
                string_bytes
                    .as_deref()
                    .and_then(canonical_array_index)
                    .and_then(|index| {
                        add_handle(env, crate::value::JSValue::number(index as f64).bits()).ok()
                    })
                    .unwrap_or(key_handle)
            } else {
                key_handle
            };
            let status = napi_set_element(env, output, output_index, output_key);
            if status != NapiStatus::Ok {
                return status;
            }
            output_index = match output_index.checked_add(1) {
                Some(index) => index,
                None => {
                    return set_status(env, NapiStatus::GenericFailure, "too many property names")
                }
            };
        }
        if key_mode == NapiKeyCollectionMode::OwnOnly {
            break;
        }
        let mut prototype = std::ptr::null_mut();
        let status = napi_get_prototype(env, current, &mut prototype);
        if status != NapiStatus::Ok {
            return status;
        }
        let prototype_bits = match value_bits(env, prototype) {
            Ok(bits) => bits,
            Err(status) => return status,
        };
        if crate::value::JSValue::from_bits(prototype_bits).is_null() {
            break;
        }
        current = prototype;
        prototype_depth += 1;
        if prototype_depth > 1024 {
            return set_status(env, NapiStatus::GenericFailure, "prototype chain is cyclic");
        }
    }
    *result = output;
    ok(env)
}
