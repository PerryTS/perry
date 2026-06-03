//! `node:sea` public API surface for normal Perry executables.
//!
//! Perry is not currently running from a Node SEA asset blob, so the observable
//! Node-compatible behavior is module availability, `isSea() === false`, and
//! asset APIs throwing `ERR_NOT_IN_SINGLE_EXECUTABLE_APPLICATION` after key
//! validation.

use crate::value::{JSValue, TAG_FALSE};

#[used]
static KEEP_SEA_IS_SEA: extern "C" fn() -> f64 = js_sea_is_sea;
#[used]
static KEEP_SEA_GET_ASSET: extern "C" fn(f64, f64) -> f64 = js_sea_get_asset;
#[used]
static KEEP_SEA_GET_ASSET_AS_BLOB: extern "C" fn(f64, f64) -> f64 = js_sea_get_asset_as_blob;
#[used]
static KEEP_SEA_GET_RAW_ASSET: extern "C" fn(f64) -> f64 = js_sea_get_raw_asset;
#[used]
static KEEP_SEA_GET_ASSET_KEYS: extern "C" fn() -> f64 = js_sea_get_asset_keys;

fn false_value() -> f64 {
    f64::from_bits(TAG_FALSE)
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

#[no_mangle]
pub extern "C" fn js_sea_is_sea() -> f64 {
    false_value()
}

#[no_mangle]
pub extern "C" fn js_sea_get_asset(key: f64, _encoding: f64) -> f64 {
    validate_key(key);
    throw_not_in_sea()
}

#[no_mangle]
pub extern "C" fn js_sea_get_asset_as_blob(key: f64, _options: f64) -> f64 {
    validate_key(key);
    throw_not_in_sea()
}

#[no_mangle]
pub extern "C" fn js_sea_get_raw_asset(key: f64) -> f64 {
    validate_key(key);
    throw_not_in_sea()
}

#[no_mangle]
pub extern "C" fn js_sea_get_asset_keys() -> f64 {
    throw_not_in_sea()
}
