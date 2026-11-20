//! Shared key allocation and error-value construction.
use crate::string::{js_string_from_bytes, StringHeader};

pub(crate) fn key_ptr(key: &[u8]) -> *mut StringHeader {
    js_string_from_bytes(key.as_ptr(), key.len() as u32)
}

pub(crate) fn syntax_error(message: &str) -> crate::error::CoreError {
    let message = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let error = crate::error::CoreError::syntax_error(message);
    error
}

/// Data decoding errors retain the missing-hook marker separately from native errors.
pub(crate) enum DataError {
    Core(crate::error::CoreError),
    Open(f64),
}
impl DataError {
    pub(crate) fn value(&self) -> f64 {
        match self {
            Self::Core(error) => error.value(),
            Self::Open(value) => *value,
        }
    }
}
