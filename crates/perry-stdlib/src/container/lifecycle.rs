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

// ============ Container Lifecycle ============

/// Run a container from the given spec
/// FFI: js_container_run(spec_json: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_run(spec_ptr: *const StringHeader) -> *mut Promise {
    let promise = js_promise_new();

    let spec = match types::parse_container_spec(spec_ptr) {
        Ok(s) => s,
        Err(e) => {
            crate::common::spawn_for_promise(
                promise as *mut u8,
                async move { Err::<u64, String>(e) },
            );
            return promise;
        }
    };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        if let Err(e) = maybe_verify_image(&spec.image).await {
            return Err::<u64, String>(e);
        }
        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        // Route through the security-aware path when the spec carries
        // fields (`seccomp`, `no_new_privileges`) that only exist on
        // the protocol's `security_args`. Pre-fix `run()` always
        // called `backend.run()`, so those knobs were unreachable via
        // the public API — serde dropped them from the JSON and the
        // documented hardening silently never reached the runtime.
        let result = if spec.has_security_opts() {
            let profile = spec.security_profile();
            backend.run_with_security(&spec, &profile).await
        } else {
            backend.run(&spec).await
        };
        match result {
            Ok(handle) => {
                let handle_id = types::register_container_handle(handle);
                Ok(handle_to_promise_bits(handle_id as u64))
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Create a container from the given spec without starting it
/// FFI: js_container_create(spec_json: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_create(spec_ptr: *const StringHeader) -> *mut Promise {
    let promise = js_promise_new();

    let spec = match types::parse_container_spec(spec_ptr) {
        Ok(s) => s,
        Err(e) => {
            crate::common::spawn_for_promise(
                promise as *mut u8,
                async move { Err::<u64, String>(e) },
            );
            return promise;
        }
    };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        if let Err(e) = maybe_verify_image(&spec.image).await {
            return Err::<u64, String>(e);
        }
        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        // Same security routing as `run()` above — `create()` must not
        // silently drop `seccomp` / `no_new_privileges` either.
        let result = if spec.has_security_opts() {
            let profile = spec.security_profile();
            backend.create_with_security(&spec, &profile).await
        } else {
            backend.create(&spec).await
        };
        match result {
            Ok(handle) => {
                let handle_id = types::register_container_handle(handle);
                Ok(handle_to_promise_bits(handle_id as u64))
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Start a previously created container
/// FFI: js_container_start(id: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_start(id_ptr: *const StringHeader) -> *mut Promise {
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

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        match backend.start(&id).await {
            Ok(()) => Ok(PROMISE_VOID_BITS),
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Stop a running container
/// FFI: js_container_stop(id: *const StringHeader, timeout: i32) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_stop(
    id_ptr: *const StringHeader,
    timeout: i32,
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

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let timeout_opt = if timeout < 0 {
            None
        } else {
            Some(timeout as u32)
        };
        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        match backend.stop(&id, timeout_opt).await {
            Ok(()) => Ok(PROMISE_VOID_BITS),
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Remove a container
/// FFI: js_container_remove(id: *const StringHeader, force: i32) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_remove(
    id_ptr: *const StringHeader,
    force: i32,
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

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        match backend.remove(&id, force != 0).await {
            Ok(()) => Ok(PROMISE_VOID_BITS),
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

// ============ Cleanup helpers (no ComposeHandle required) ============
//
// `down_by_project` / `down_all` / `remove_if_exists` cover the
// "I crashed without calling down()" / "I want to clean up between
// dev iterations" / "I don't have the ComposeHandle anymore" use
// cases. They drive the same `ContainerBackend` trait every other
// FFI uses, scoped by Perry's `perry.compose.project` label so they
// only ever touch resources the user's program created.

/// Tear down every container labelled with `perry.compose.project = <project>`.
/// Resolves with a JSON-encoded `CleanupReport` string:
///
/// ```text
/// {"containers_removed":2,"networks_removed":0,"volumes_removed":0,"errors":[]}
/// ```
///
/// FFI: `js_container_downByProject(project: *const StringHeader, opts_json: *const StringHeader) -> *mut Promise`
#[no_mangle]
pub unsafe extern "C" fn js_container_downByProject(
    project_ptr: *const StringHeader,
    opts_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();
    let project = match string_from_header(project_ptr) {
        Some(s) if !s.is_empty() => s,
        _ => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("project name required".to_string())
            });
            return promise;
        }
    };
    let opts_json = string_from_header(opts_ptr);

    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            use perry_container_compose::compose::{down_by_project, CleanupOptions};
            let opts = parse_cleanup_options(&opts_json);
            let backend = get_global_backend().await.map_err(|e| e.to_string())?;
            let report = down_by_project(backend.as_ref(), &project, &opts).await;
            serde_json::to_string(&report).map_err(|e| e.to_string())
        },
        |json| {
            let str_ptr = perry_runtime::js_string_from_bytes(json.as_ptr(), json.len() as u32);
            perry_runtime::JSValue::string_ptr(str_ptr).bits()
        },
    );

    promise
}

/// Tear down every Perry-managed container on this host. Equivalent to
/// `downByProject` for every project at once. Returns the same JSON-
/// encoded `CleanupReport` summary.
///
/// **Use sparingly** — this stops every stack the user has ever brought
/// up via `perry/compose`, regardless of which terminal session it's
/// running in.
///
/// FFI: `js_container_downAll(opts_json: *const StringHeader) -> *mut Promise`
#[no_mangle]
pub unsafe extern "C" fn js_container_downAll(opts_ptr: *const StringHeader) -> *mut Promise {
    let promise = js_promise_new();
    let opts_json = string_from_header(opts_ptr);

    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            use perry_container_compose::compose::{down_all, CleanupOptions};
            let opts = parse_cleanup_options(&opts_json);
            let backend = get_global_backend().await.map_err(|e| e.to_string())?;
            let report = down_all(backend.as_ref(), &opts).await;
            serde_json::to_string(&report).map_err(|e| e.to_string())
        },
        |json| {
            let str_ptr = perry_runtime::js_string_from_bytes(json.as_ptr(), json.len() as u32);
            perry_runtime::JSValue::string_ptr(str_ptr).bits()
        },
    );

    promise
}

/// Idempotent container removal: stop + force-remove if the container
/// exists; treat NotFound as success. Resolves with `"true"` if the
/// container was found and removed, `"false"` if it didn't exist.
///
/// FFI: `js_container_removeIfExists(id: *const StringHeader, force: i32) -> *mut Promise`
#[no_mangle]
pub unsafe extern "C" fn js_container_removeIfExists(
    id_ptr: *const StringHeader,
    force: i32,
) -> *mut Promise {
    let promise = js_promise_new();
    let id = match string_from_header(id_ptr) {
        Some(s) if !s.is_empty() => s,
        _ => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("container ID required".to_string())
            });
            return promise;
        }
    };

    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            use perry_container_compose::compose::remove_if_exists;
            let backend = get_global_backend().await.map_err(|e| e.to_string())?;
            let removed = remove_if_exists(backend.as_ref(), &id, force != 0)
                .await
                .map_err(|e| e.to_string())?;
            Ok(if removed {
                "true".to_string()
            } else {
                "false".to_string()
            })
        },
        |s| {
            let str_ptr = perry_runtime::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            perry_runtime::JSValue::string_ptr(str_ptr).bits()
        },
    );

    promise
}

/// Parse the JSON-encoded `{ volumes?: bool, networks?: bool }`
/// options object into a `CleanupOptions`. Missing/invalid → defaults.
pub(crate) fn parse_cleanup_options(
    json: &Option<String>,
) -> perry_container_compose::compose::CleanupOptions {
    use perry_container_compose::compose::CleanupOptions;
    let s = match json.as_deref() {
        Some(s) if !s.is_empty() && s != "undefined" && s != "null" => s,
        _ => return CleanupOptions::default_for_project(),
    };
    let v: serde_json::Value = match serde_json::from_str(s) {
        Ok(v) => v,
        Err(_) => return CleanupOptions::default_for_project(),
    };
    CleanupOptions {
        volumes: v.get("volumes").and_then(|x| x.as_bool()).unwrap_or(false),
        networks: v.get("networks").and_then(|x| x.as_bool()).unwrap_or(true),
    }
}

/// List containers
/// FFI: `js_container_list(all: i32) -> *mut Promise<JSON string>`
///
/// Resolves with a JSON-encoded `ContainerInfo[]` string. User code does
/// `JSON.parse(await list(true))` to recover the array.
#[no_mangle]
pub unsafe extern "C" fn js_container_list(all: i32) -> *mut Promise {
    let promise = js_promise_new();

    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            let backend = get_global_backend().await.map_err(|e| e.to_string())?;
            let containers = backend.list(all != 0).await.map_err(|e| e.to_string())?;
            serde_json::to_string(&containers).map_err(|e| e.to_string())
        },
        |json| {
            let str_ptr = perry_runtime::js_string_from_bytes(json.as_ptr(), json.len() as u32);
            perry_runtime::JSValue::string_ptr(str_ptr).bits()
        },
    );

    promise
}

/// Inspect a container
/// FFI: js_container_inspect(id: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_inspect(id_ptr: *const StringHeader) -> *mut Promise {
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

    // Resolves with a JSON-encoded `ContainerInfo` string.
    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            let backend = get_global_backend().await.map_err(|e| e.to_string())?;
            let info = backend.inspect(&id).await.map_err(|e| e.to_string())?;
            serde_json::to_string(&info).map_err(|e| e.to_string())
        },
        |json| {
            let str_ptr = perry_runtime::js_string_from_bytes(json.as_ptr(), json.len() as u32);
            perry_runtime::JSValue::string_ptr(str_ptr).bits()
        },
    );

    promise
}
