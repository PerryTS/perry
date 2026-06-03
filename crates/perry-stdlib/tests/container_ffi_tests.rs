#![cfg(feature = "container")]
// Feature: perry-container | Layer: ffi-contract | Req: 11.7 | Property: -

use perry_runtime::{Promise, StringHeader};
use std::ptr::null;

/// Helper to create a StringHeader for testing
fn make_string_header(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let len = bytes.len() as u32;
    let header_size = std::mem::size_of::<StringHeader>();
    let mut buf = vec![0u8; header_size + bytes.len()];

    let header = StringHeader {
        utf16_len: s.chars().count() as u32,
        byte_len: len,
        capacity: len,
        refcount: 0,
        flags: 0,
    };

    unsafe {
        std::ptr::copy_nonoverlapping(
            &header as *const StringHeader as *const u8,
            buf.as_mut_ptr(),
            header_size,
        );
    }
    buf[header_size..].copy_from_slice(bytes);
    buf
}

/// Safe helper to call an FFI function and drive the promise to completion
unsafe fn await_promise_sync(promise: *mut Promise) -> Result<u64, String> {
    assert!(
        !promise.is_null(),
        "FFI function must return a non-null promise"
    );

    let mut count = 0;
    loop {
        perry_runtime::js_promise_run_microtasks();
        perry_stdlib::common::js_stdlib_process_pending();

        let state = perry_runtime::js_promise_state(promise);
        if state == 1 {
            // Resolved
            return Ok(perry_runtime::js_promise_value(promise) as u64);
        } else if state == 2 {
            // Rejected
            return Err("Promise rejected".to_string());
        }

        count += 1;
        if count > 200 {
            return Err("Promise timed out".to_string());
        }
        std::thread::yield_now();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

// ========== js_container_run ==========

// Feature: perry-container | Layer: ffi-contract | Req: 11.7 | Property: -
#[tokio::test]
async fn test_js_container_run_null() {
    unsafe {
        let p = perry_stdlib::container::js_container_run(null());
        let res = await_promise_sync(p);
        assert!(res.is_err());
    }
}

// ========== js_container_list ==========

// Feature: perry-container | Layer: ffi-contract | Req: 11.7 | Property: -
#[tokio::test]
async fn test_js_container_list_contract() {
    unsafe {
        let p = perry_stdlib::container::js_container_list(1);
        let _ = await_promise_sync(p);
    }
}

// ========== js_container_listImages ==========

// Feature: perry-container | Layer: ffi-contract | Req: 11.7 | Property: -
#[tokio::test]
async fn test_js_container_list_images_contract() {
    unsafe {
        let p = perry_stdlib::container::js_container_listImages();
        let _ = await_promise_sync(p);
    }
}

// ========== js_container_getBackend ==========

// Feature: perry-container | Layer: ffi-contract | Req: 1.4 | Property: -
#[test]
fn test_js_container_get_backend_contract() {
    unsafe {
        let header = perry_stdlib::container::js_container_getBackend();
        assert!(!header.is_null());
    }
}

// ========== js_container_detectBackend ==========

// Feature: perry-container | Layer: ffi-contract | Req: 1.8 | Property: -
#[tokio::test]
async fn test_js_container_detect_backend_contract() {
    unsafe {
        let p = perry_stdlib::container::js_container_detectBackend();
        let _ = await_promise_sync(p);
    }
}

// ========== js_container_compose_ps ==========

// Feature: perry-container | Layer: ffi-contract | Req: 11.7 | Property: -
#[tokio::test]
async fn test_js_container_compose_ps_contract() {
    unsafe {
        let p = perry_stdlib::container::js_container_compose_ps(0.0);
        let res = await_promise_sync(p);
        assert!(res.is_err());
    }
}

// ========== js_container_compose_logs ==========

// Feature: perry-container | Layer: ffi-contract | Req: 11.7 | Property: -
#[tokio::test]
async fn test_js_container_compose_logs_null() {
    unsafe {
        let p = perry_stdlib::container::js_container_compose_logs(0.0, null(), 10.0);
        let res = await_promise_sync(p);
        assert!(res.is_err());
    }
}

// ========== js_container_compose_exec ==========

// Feature: perry-container | Layer: ffi-contract | Req: 11.7 | Property: -
#[tokio::test]
async fn test_js_container_compose_exec_null() {
    unsafe {
        let p = perry_stdlib::container::js_container_compose_exec(0.0, null(), null());
        let res = await_promise_sync(p);
        assert!(res.is_err());
    }
}

// Feature: perry-container | Layer: ffi-contract | Req: 11.7 | Property: -
#[tokio::test]
async fn test_js_container_run_malformed() {
    unsafe {
        let header = make_string_header("{ bad json");
        let p = perry_stdlib::container::js_container_run(header.as_ptr() as *const StringHeader);
        let res = await_promise_sync(p);
        assert!(res.is_err());
    }
}

// ========== js_container_create ==========

// Feature: perry-container | Layer: ffi-contract | Req: 11.7 | Property: -
#[tokio::test]
async fn test_js_container_create_null() {
    unsafe {
        let p = perry_stdlib::container::js_container_create(null());
        let res = await_promise_sync(p);
        assert!(res.is_err());
    }
}

// ========== js_container_start ==========

// Feature: perry-container | Layer: ffi-contract | Req: 11.7 | Property: -
#[tokio::test]
async fn test_js_container_start_null() {
    unsafe {
        let p = perry_stdlib::container::js_container_start(null());
        let res = await_promise_sync(p);
        assert!(res.is_err());
    }
}

// ========== js_container_stop ==========

// Feature: perry-container | Layer: ffi-contract | Req: 11.7 | Property: -
#[tokio::test]
async fn test_js_container_stop_null() {
    unsafe {
        let p = perry_stdlib::container::js_container_stop(null(), 10);
        let res = await_promise_sync(p);
        assert!(res.is_err());
    }
}

// ========== js_container_remove ==========

// Feature: perry-container | Layer: ffi-contract | Req: 11.7 | Property: -
#[tokio::test]
async fn test_js_container_remove_null() {
    unsafe {
        let p = perry_stdlib::container::js_container_remove(null(), 1);
        let res = await_promise_sync(p);
        assert!(res.is_err());
    }
}

// ========== js_container_inspect ==========

// Feature: perry-container | Layer: ffi-contract | Req: 11.7 | Property: -
#[tokio::test]
async fn test_js_container_inspect_null() {
    unsafe {
        let p = perry_stdlib::container::js_container_inspect(null());
        let res = await_promise_sync(p);
        assert!(res.is_err());
    }
}

// ========== js_container_logs ==========

// Feature: perry-container | Layer: ffi-contract | Req: 11.7 | Property: -
#[tokio::test]
async fn test_js_container_logs_null() {
    unsafe {
        let p = perry_stdlib::container::js_container_logs(null(), 10);
        let res = await_promise_sync(p);
        assert!(res.is_err());
    }
}

// ========== js_container_exec ==========

// Feature: perry-container | Layer: ffi-contract | Req: 11.7 | Property: -
#[tokio::test]
async fn test_js_container_exec_null() {
    unsafe {
        let p = perry_stdlib::container::js_container_exec(null(), null(), null(), null());
        let res = await_promise_sync(p);
        assert!(res.is_err());
    }
}

// ========== js_container_pullImage ==========

// Feature: perry-container | Layer: ffi-contract | Req: 11.7 | Property: -
#[tokio::test]
async fn test_js_container_pull_image_null() {
    unsafe {
        let p = perry_stdlib::container::js_container_pullImage(null());
        let res = await_promise_sync(p);
        assert!(res.is_err());
    }
}

// ========== js_container_removeImage ==========

// Feature: perry-container | Layer: ffi-contract | Req: 11.7 | Property: -
#[tokio::test]
async fn test_js_container_remove_image_null() {
    unsafe {
        let p = perry_stdlib::container::js_container_removeImage(null(), 0);
        let res = await_promise_sync(p);
        assert!(res.is_err());
    }
}

// ========== js_container_composeUp ==========

// Feature: perry-container | Layer: ffi-contract | Req: 11.7 | Property: -
#[tokio::test]
async fn test_js_container_compose_up_null() {
    unsafe {
        let p = perry_stdlib::container::js_container_composeUp(null());
        let res = await_promise_sync(p);
        assert!(res.is_err());
    }
}

// ========== js_container_compose_down ==========

// Feature: perry-container | Layer: ffi-contract | Req: 11.7 | Property: -
#[tokio::test]
async fn test_js_container_compose_down_contract() {
    unsafe {
        let p = perry_stdlib::container::js_container_compose_down(0.0, null());
        let res = await_promise_sync(p);
        assert!(res.is_err());
    }
}
