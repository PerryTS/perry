//! Shared key allocation and error-value construction.
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;

pub(crate) fn key_ptr(key: &[u8]) -> *mut StringHeader {
    js_string_from_bytes(key.as_ptr(), key.len() as u32)
}

pub(crate) fn syntax_error_value(message: &str) -> f64 {
    let message = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let error = crate::error::js_syntaxerror_new(message);
    f64::from_bits(JSValue::pointer(error as *const u8).bits())
}
