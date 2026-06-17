use perry_runtime::StringHeader;

use crate::common::Handle;

use super::{POINTER_TAG, PTR_MASK};

pub(super) unsafe fn dispatch_method(handle: Handle, method: &str, args: &[f64]) -> Option<f64> {
    extern "C" {
        fn js_ext_http_client_request_is_handle(handle: i64) -> i32;
        fn js_ext_http_client_request_dispatch_method(
            handle: i64,
            method_ptr: *const u8,
            method_len: usize,
            args_ptr: *const f64,
            args_len: usize,
        ) -> f64;
    }
    if unsafe { js_ext_http_client_request_is_handle(handle) } == 0 {
        return None;
    }
    Some(unsafe {
        js_ext_http_client_request_dispatch_method(
            handle,
            method.as_ptr(),
            method.len(),
            args.as_ptr(),
            args.len(),
        )
    })
}

unsafe fn dispatch_property(handle: Handle, property: &str) -> Option<f64> {
    extern "C" {
        fn js_ext_http_client_request_is_handle(handle: i64) -> i32;
        fn js_ext_http_client_request_dispatch_property(
            handle: i64,
            property_ptr: *const u8,
            property_len: usize,
        ) -> f64;
    }
    if unsafe { js_ext_http_client_request_is_handle(handle) } == 0 {
        return None;
    }
    Some(unsafe {
        js_ext_http_client_request_dispatch_property(handle, property.as_ptr(), property.len())
    })
}

pub(super) unsafe fn string_property(handle: Handle, property: &str) -> Option<*mut StringHeader> {
    let value = unsafe { dispatch_property(handle, property) }?;
    let bits = value.to_bits();
    let tag = bits & !PTR_MASK;
    if tag != 0x7FFF_0000_0000_0000 && tag != POINTER_TAG {
        return None;
    }
    let ptr = (bits & PTR_MASK) as *mut StringHeader;
    if ptr.is_null() {
        None
    } else {
        Some(ptr)
    }
}
