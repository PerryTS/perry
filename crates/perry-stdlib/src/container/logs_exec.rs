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

// ============ Container Logs and Exec ============

/// Get logs from a container
/// FFI: js_container_logs(id: *const StringHeader, tail: i32) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_logs(id_ptr: *const StringHeader, tail: i32) -> *mut Promise {
    let promise = js_promise_new();

    let id = match string_from_header(id_ptr) {
        Some(s) => s,
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid container ID".to_string())
            });
            return promise;
        }
    };

    let tail_opt = if tail >= 0 { Some(tail as u32) } else { None };

    // Resolves with a JSON-encoded `ContainerLogs` string.
    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            let backend = get_global_backend().await.map_err(|e| e.to_string())?;
            let logs = backend
                .logs(&id, tail_opt)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_string(&logs).map_err(|e| e.to_string())
        },
        |json| {
            let str_ptr = perry_runtime::js_string_from_bytes(json.as_ptr(), json.len() as u32);
            perry_runtime::JSValue::string_ptr(str_ptr).bits()
        },
    );

    promise
}

/// Execute a command in a container
/// FFI: js_container_exec(id: *const StringHeader, cmd_json: *const StringHeader, env_json: *const StringHeader, workdir: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_exec(
    id_ptr: *const StringHeader,
    cmd_json_ptr: *const StringHeader,
    env_json_ptr: *const StringHeader,
    workdir_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();

    let id = match string_from_header(id_ptr) {
        Some(s) => s,
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid container ID".to_string())
            });
            return promise;
        }
    };

    let cmd_json = string_from_header(cmd_json_ptr);
    let env_json = string_from_header(env_json_ptr);
    let workdir = string_from_header(workdir_ptr);

    // Resolves with a JSON-encoded `ContainerLogs` string.
    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            let cmd: Vec<String> = cmd_json
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            let env: Option<HashMap<String, String>> =
                env_json.and_then(|s| serde_json::from_str(&s).ok());
            let backend = get_global_backend().await.map_err(|e| e.to_string())?;
            let logs = backend
                .exec(&id, &cmd, env.as_ref(), workdir.as_deref())
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_string(&logs).map_err(|e| e.to_string())
        },
        |json| {
            let str_ptr = perry_runtime::js_string_from_bytes(json.as_ptr(), json.len() as u32);
            perry_runtime::JSValue::string_ptr(str_ptr).bits()
        },
    );

    promise
}
