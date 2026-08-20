//! Common utilities for stdlib modules

use perry_runtime::{string::str_bytes_from_jsvalue, value::JSValue, StringHeader};

pub mod handle;
// Tokio-backed promise/runtime bridge — only needed when an async feature
// (http-server/client, websocket, databases, email, scheduler, rate-limit,
// crypto's bcrypt path, …) pulls in `async-runtime`. Always-on code that
// references it must also be `#[cfg(feature = "async-runtime")]`-gated.
#[cfg(feature = "async-runtime")]
pub mod async_bridge;
pub mod dispatch;
pub(crate) mod dispatch_http;
mod dispatch_ioredis;
pub mod net_method_values;
mod net_socket_bridge;

#[cfg(feature = "async-runtime")]
pub use async_bridge::*;
pub use dispatch::*;
pub use handle::*;

/// Copy a runtime string header into an owned UTF-8 string.
///
/// Invalid UTF-8 and values from Perry's native-handle address band are
/// rejected. The owned return is intentional: callers may allocate or cross
/// an async boundary after this function returns, so no GC-managed payload
/// borrow may escape this accessor.
///
/// # Safety
///
/// `ptr` must be null, a native handle-band value, or point to a live Perry
/// [`StringHeader`].
pub(crate) unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    map_string_header_bytes(ptr, |bytes| {
        std::str::from_utf8(bytes).ok().map(str::to_owned)
    })
    .flatten()
}

/// Copy a runtime string header into an owned string, replacing invalid UTF-8.
///
/// # Safety
///
/// `ptr` has the same requirements as [`string_from_header`].
pub(crate) unsafe fn string_from_header_lossy(ptr: *const StringHeader) -> Option<String> {
    map_string_header_bytes(ptr, |bytes| String::from_utf8_lossy(bytes).into_owned())
}

/// Copy a runtime string header's payload without UTF-8 validation.
///
/// # Safety
///
/// `ptr` has the same requirements as [`string_from_header`].
pub(crate) unsafe fn bytes_from_header(ptr: *const StringHeader) -> Option<Vec<u8>> {
    map_string_header_bytes(ptr, <[u8]>::to_vec)
}

unsafe fn map_string_header_bytes<T>(
    ptr: *const StringHeader,
    map: impl FnOnce(&[u8]) -> T,
) -> Option<T> {
    if ptr.is_null() || perry_runtime::value::addr_class::is_handle_band(ptr as usize) {
        return None;
    }

    let value = f64::from_bits(JSValue::string_ptr(ptr as *mut StringHeader).bits());
    let mut scratch = [0; perry_runtime::value::SHORT_STRING_MAX_LEN];
    let (data, len) = str_bytes_from_jsvalue(value, &mut scratch)?;
    let bytes = std::slice::from_raw_parts(data, len as usize);
    Some(map(bytes))
}

#[cfg(test)]
mod string_header_tests {
    use super::*;

    #[test]
    fn string_readers_reject_null_and_handle_band_values() {
        for ptr in [
            std::ptr::null(),
            1usize as *const StringHeader,
            (perry_runtime::value::addr_class::HANDLE_BAND_MAX - 1) as *const StringHeader,
        ] {
            assert!(unsafe { string_from_header(ptr) }.is_none());
            assert!(unsafe { string_from_header_lossy(ptr) }.is_none());
            assert!(unsafe { bytes_from_header(ptr) }.is_none());
        }
    }

    #[test]
    fn string_readers_preserve_text_and_raw_bytes() {
        let input = b"Perry \xf0\x9f\xa6\x86";
        let ptr = perry_runtime::js_string_from_bytes(input.as_ptr(), input.len() as u32);

        assert_eq!(
            unsafe { string_from_header(ptr) }.as_deref(),
            Some("Perry \u{1f986}")
        );
        assert_eq!(
            unsafe { string_from_header_lossy(ptr) }.as_deref(),
            Some("Perry \u{1f986}")
        );
        assert_eq!(
            unsafe { bytes_from_header(ptr) }.as_deref(),
            Some(input.as_slice())
        );

        let invalid = b"\xff";
        let ptr = perry_runtime::js_string_from_bytes(invalid.as_ptr(), invalid.len() as u32);
        assert!(unsafe { string_from_header(ptr) }.is_none());
        assert_eq!(
            unsafe { string_from_header_lossy(ptr) }.as_deref(),
            Some("\u{fffd}")
        );
        assert_eq!(
            unsafe { bytes_from_header(ptr) }.as_deref(),
            Some(invalid.as_slice())
        );
    }
}
