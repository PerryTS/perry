//! Link-safe stubs for auto-optimized runtimes that do not include #9600's
//! Bun CLI utility backends. The compiler turns `bun-cli-utils` on whenever a
//! Bun import/global is reachable, so these are defensive diagnostics only.

use crate::string::js_string_from_bytes;
use crate::value::JSValue;

fn undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn feature_disabled() -> ! {
    let message = b"Bun CLI utilities are not enabled in this optimized Perry runtime";
    let message = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let error = crate::error::js_error_new_with_message(message);
    crate::exception::js_throw(f64::from_bits(JSValue::pointer(error as *const u8).bits()))
}

pub fn js_bun_yaml() -> f64 {
    undefined()
}

pub fn js_bun_toml() -> f64 {
    undefined()
}

pub fn js_bun_semver() -> f64 {
    undefined()
}

pub fn js_bun_jsonl() -> f64 {
    undefined()
}

pub fn decorate_bun_hash(value: f64) -> f64 {
    value
}

#[no_mangle]
pub extern "C" fn js_bun_deep_equals(_left: f64, _right: f64, _strict: f64) -> f64 {
    feature_disabled()
}

#[no_mangle]
pub extern "C" fn js_bun_strip_ansi(_input: f64) -> f64 {
    feature_disabled()
}

#[no_mangle]
pub extern "C" fn js_bun_wrap_ansi(_input: f64, _columns: f64, _options: f64) -> f64 {
    feature_disabled()
}

#[no_mangle]
pub extern "C" fn js_bun_which(_command: f64, _options: f64) -> f64 {
    feature_disabled()
}

#[no_mangle]
pub extern "C" fn js_bun_zstd_decompress(_input: f64) -> f64 {
    feature_disabled()
}

#[no_mangle]
pub extern "C" fn js_bun_zstd_decompress_sync(_input: f64) -> f64 {
    feature_disabled()
}

#[no_mangle]
pub extern "C" fn js_bun_gc(_full: f64) -> f64 {
    feature_disabled()
}

#[no_mangle]
pub extern "C" fn js_bun_generate_heap_snapshot(_format: f64, _encoding: f64) -> f64 {
    feature_disabled()
}
