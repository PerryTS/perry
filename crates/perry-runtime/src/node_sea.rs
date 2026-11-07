//! `node:sea` APIs backed by Perry's explicitly enabled embedded-asset profile.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::value::{JSValue, TAG_FALSE, TAG_TRUE};

static SEA_PROFILE: AtomicBool = AtomicBool::new(false);

/// Called only by the generated startup object for an explicit `--sea` build.
#[no_mangle]
pub extern "C" fn js_register_sea_profile() {
    SEA_PROFILE.store(true, Ordering::Release);
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_SEA_IS_SEA: extern "C" fn() -> f64 = js_sea_is_sea;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_SEA_GET_ASSET: extern "C" fn(f64, f64) -> f64 = js_sea_get_asset;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_SEA_GET_ASSET_AS_BLOB: extern "C" fn(f64, f64) -> f64 = js_sea_get_asset_as_blob;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_SEA_GET_RAW_ASSET: extern "C" fn(f64) -> f64 = js_sea_get_raw_asset;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_SEA_GET_ASSET_KEYS: extern "C" fn() -> f64 = js_sea_get_asset_keys;

fn false_value() -> f64 {
    f64::from_bits(TAG_FALSE)
}

fn true_value() -> f64 {
    f64::from_bits(TAG_TRUE)
}

fn sea_enabled() -> bool {
    SEA_PROFILE.load(Ordering::Acquire)
}

fn validate_key(value: f64) {
    if JSValue::from_bits(value.to_bits()).is_any_string() {
        return;
    }
    let message = format!(
        "The \"key\" argument must be of type string. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

fn throw_not_in_sea() -> ! {
    crate::fs::validate::throw_error_with_code(
        "Operation cannot be invoked when not in a single-executable application",
        "ERR_NOT_IN_SINGLE_EXECUTABLE_APPLICATION",
    )
}

fn asset_key(value: f64) -> String {
    let value = JSValue::from_bits(value.to_bits());
    crate::string::string_as_str(value.as_string_ptr()).to_string()
}

fn asset_bytes(key: &str) -> &'static [u8] {
    crate::embedded::lookup_sea_asset(key).unwrap_or_else(|| {
        crate::fs::validate::throw_error_with_code(
            &format!("No such asset: {key}"),
            "ERR_INVALID_ARG_VALUE",
        )
    })
}

#[no_mangle]
pub extern "C" fn js_sea_is_sea() -> f64 {
    if sea_enabled() { true_value() } else { false_value() }
}

#[no_mangle]
pub extern "C" fn js_sea_get_asset(key: f64, encoding: f64) -> f64 {
    validate_key(key);
    if !sea_enabled() {
        throw_not_in_sea();
    }
    let bytes = asset_bytes(&asset_key(key));
    let encoding = JSValue::from_bits(encoding.to_bits());
    if encoding.is_undefined() {
        return crate::node_submodules::consumers::bytes_to_array_buffer_value(bytes);
    }
    if !encoding.is_any_string() {
        crate::fs::validate::throw_type_error_with_code(
            "The \"encoding\" argument must be of type string",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    let encoding = crate::string::string_as_str(encoding.as_string_ptr());
    let Some(encoding_impl) = encoding_rs::Encoding::for_label(encoding.as_bytes()) else {
        crate::fs::validate::throw_type_error_with_code(
            &format!("The argument 'encoding' is invalid encoding. Received '{encoding}'"),
            "ERR_INVALID_ARG_VALUE",
        );
    };
    let decoded = encoding_impl.decode_with_bom_removal(bytes);
    crate::value::js_nanbox_string(
        crate::string::js_string_from_bytes(decoded.as_bytes().as_ptr(), decoded.len() as u32)
            as i64,
    )
}

#[no_mangle]
pub extern "C" fn js_sea_get_asset_as_blob(key: f64, options: f64) -> f64 {
    validate_key(key);
    if sea_enabled() {
        let bytes = asset_bytes(&asset_key(key));
        let options = JSValue::from_bits(options.to_bits());
        let content_type = if options.is_undefined() || options.is_null() {
            String::new()
        } else if options.is_pointer() {
            let obj = options.as_pointer::<crate::object::ObjectHeader>();
            let type_key = crate::string::intern_ascii_literal(b"type");
            let type_value = crate::object::js_object_get_field_by_name_f64(obj, type_key);
            let type_value = JSValue::from_bits(type_value.to_bits());
            if type_value.is_undefined() {
                String::new()
            } else if type_value.is_any_string() {
                crate::string::string_as_str(type_value.as_string_ptr()).to_string()
            } else {
                crate::fs::validate::throw_type_error_with_code(
                    "The \"type\" option must be of type string",
                    "ERR_INVALID_ARG_TYPE",
                );
            }
        } else {
            crate::fs::validate::throw_type_error_with_code(
                "The \"options\" argument must be of type object",
                "ERR_INVALID_ARG_TYPE",
            );
        };
        return crate::node_submodules::blob::blob_value_from_bytes_and_type(bytes, &content_type);
    }
    throw_not_in_sea()
}

#[no_mangle]
pub extern "C" fn js_sea_get_raw_asset(key: f64) -> f64 {
    validate_key(key);
    if sea_enabled() {
        let _ = asset_bytes(&asset_key(key));
        crate::fs::validate::throw_error_with_code(
            "getRawAsset() is not supported because Perry cannot expose read-only executable data as an ArrayBuffer",
            "ERR_NOT_SUPPORTED",
        );
    }
    throw_not_in_sea()
}

#[no_mangle]
pub extern "C" fn js_sea_get_asset_keys() -> f64 {
    if !sea_enabled() {
        throw_not_in_sea();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let array = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc(0) as i64,
    ));
    for name in crate::embedded::asset_names() {
        let string = scope.root_nanbox_f64(crate::value::js_nanbox_string(
            crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32) as i64,
        ));
        let array_ptr = JSValue::from_bits(array.get_nanbox_f64().to_bits())
            .as_pointer::<crate::array::ArrayHeader>();
        let grown = crate::array::js_array_push_f64(array_ptr, string.get_nanbox_f64());
        array.set_nanbox_f64(crate::value::js_nanbox_pointer(grown as i64));
    }
    array.get_nanbox_f64()
}
