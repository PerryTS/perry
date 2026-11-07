//! `node:sea` APIs backed by Perry's explicitly enabled embedded-asset profile.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::value::{JSValue, TAG_FALSE, TAG_TRUE};

static SEA_PROFILE: AtomicBool = AtomicBool::new(false);

/// Called only by the generated startup object for an explicit `--sea` build.
#[no_mangle]
pub extern "C" fn js_register_sea_profile() {
    SEA_PROFILE.store(true, Ordering::Release);
}

// The generated registration object references this function outside Rust's
// reachability graph; retain it through the release whole-program LTO pass.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_SEA_PROFILE_REGISTRATION: unsafe extern "C" fn() = js_register_sea_profile;

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
    crate::value::boxed_text::get_string_content(value)
}

fn blob_content_type(value: f64) -> String {
    let value = JSValue::from_bits(value.to_bits());
    if value.is_undefined() {
        return String::new();
    }
    // Node's Blob constructor applies Web IDL DOMString conversion to `type`,
    // then keeps printable ASCII and lowercases ASCII letters.
    let value = f64::from_bits(value.bits());
    let string = crate::value::js_nanbox_string(crate::value::js_jsvalue_to_string(value) as i64);
    let raw = crate::value::boxed_text::get_string_content(string);
    if !raw.bytes().all(|byte| (0x20..=0x7e).contains(&byte)) {
        return String::new();
    }
    raw.to_ascii_lowercase()
}

fn asset_bytes(key: &str) -> &'static [u8] {
    crate::embedded::lookup_sea_asset(key).unwrap_or_else(|| {
        crate::fs::validate::throw_error_with_code(
            &format!("No such asset: {key}"),
            "ERR_SINGLE_EXECUTABLE_APPLICATION_ASSET_NOT_FOUND",
        )
    })
}

#[no_mangle]
pub extern "C" fn js_sea_is_sea() -> f64 {
    if sea_enabled() {
        true_value()
    } else {
        false_value()
    }
}

#[no_mangle]
pub extern "C" fn js_sea_get_asset(key: f64, encoding: f64) -> f64 {
    let encoding_js = JSValue::from_bits(encoding.to_bits());
    if !encoding_js.is_undefined() && !encoding_js.is_any_string() {
        crate::fs::validate::throw_type_error_with_code(
            "The \"encoding\" argument must be of type string",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    validate_key(key);
    if !sea_enabled() {
        throw_not_in_sea();
    }
    let bytes = asset_bytes(&asset_key(key));
    if encoding_js.is_undefined() {
        return crate::node_submodules::consumers::bytes_to_array_buffer_value(bytes);
    }
    let encoding = crate::value::boxed_text::get_string_content(encoding);
    let Some(decoded) = crate::text::decode_bytes_with_label(bytes, &encoding) else {
        crate::fs::validate::throw_range_error_named(
            &format!("The \"encoding\" argument is invalid. Received '{encoding}'"),
            "ERR_ENCODING_NOT_SUPPORTED",
        );
    };
    crate::value::js_nanbox_string(decoded)
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
            blob_content_type(type_value)
        } else {
            // Blob's Web IDL dictionary conversion accepts primitive values
            // (which have no `type` member) and nullish values.
            String::new()
        };
        return crate::node_submodules::blob::blob_value_from_bytes_and_type(bytes, &content_type);
    }
    throw_not_in_sea()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodings_accept_only_labels_known_to_text_decoder() {
        assert!(crate::text::decode_bytes_with_label(b"hi", "utf-8").is_some());
        assert!(crate::text::decode_bytes_with_label(b"hi", "this-is-not-an-encoding").is_none());
    }
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
            .as_pointer::<crate::array::ArrayHeader>()
            .cast_mut();
        let grown = crate::array::js_array_push_f64(array_ptr, string.get_nanbox_f64());
        array.set_nanbox_f64(crate::value::js_nanbox_pointer(grown as i64));
    }
    array.get_nanbox_f64()
}
