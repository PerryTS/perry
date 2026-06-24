use super::*;

pub use types::{
    ComposeHandle, ComposeSpec, ContainerError, ContainerHandle, ContainerInfo, ContainerLogs,
    ContainerSpec, ImageInfo, ListOrDict,
};

pub use backend::{detect_backend, ContainerBackend};
use perry_runtime::{js_promise_new, Promise, StringHeader};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;

// ============ Image Management ============

/// Pull a container image
/// FFI: js_container_pullImage(reference: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_pullImage(
    reference_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();

    let reference = match string_from_header(reference_ptr) {
        Some(s) => s,
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid image reference".to_string())
            });
            return promise;
        }
    };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        if let Err(e) = maybe_verify_image(&reference).await {
            return Err::<u64, String>(e);
        }
        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        match backend.pull_image(&reference).await {
            Ok(()) => Ok(PROMISE_VOID_BITS),
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// List images
/// FFI: js_container_listImages() -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_listImages() -> *mut Promise {
    let promise = js_promise_new();

    // Resolves with a JSON-encoded `ImageInfo[]` string.
    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            let backend = get_global_backend().await.map_err(|e| e.to_string())?;
            let images = backend.list_images().await.map_err(|e| e.to_string())?;
            serde_json::to_string(&images).map_err(|e| e.to_string())
        },
        |json| {
            let str_ptr = perry_runtime::js_string_from_bytes(json.as_ptr(), json.len() as u32);
            perry_runtime::JSValue::string_ptr(str_ptr).bits()
        },
    );

    promise
}

/// Build a container image
/// FFI: js_container_build(spec_json: *const StringHeader, image_name: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_build(
    spec_ptr: *const StringHeader,
    image_name_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();

    let spec_json = string_from_header(spec_ptr).unwrap_or_else(|| "{}".to_string());
    let image_name = string_from_header(image_name_ptr).unwrap_or_default();

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let spec: perry_container_compose::types::ComposeServiceBuild =
            serde_json::from_str(&spec_json).map_err(|e| format!("Invalid build spec: {}", e))?;

        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };

        match backend.build(&spec, &image_name).await {
            Ok(()) => Ok(PROMISE_VOID_BITS),
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Remove an image
/// FFI: js_container_removeImage(reference: *const StringHeader, force: i32) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_removeImage(
    reference_ptr: *const StringHeader,
    force: i32,
) -> *mut Promise {
    let promise = js_promise_new();

    let reference = match string_from_header(reference_ptr) {
        Some(s) => s,
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid image reference".to_string())
            });
            return promise;
        }
    };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        match backend.remove_image(&reference, force != 0).await {
            Ok(()) => Ok(PROMISE_VOID_BITS),
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}
