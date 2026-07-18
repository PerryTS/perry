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

/// Start compose services.
///
/// FFI: `js_container_compose_start(handle: f64, services_json: *const StringHeader) -> *mut Promise`
#[no_mangle]
pub unsafe extern "C" fn js_container_compose_start(
    handle: f64,
    services_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();
    let handle_id = handle_id_from_f64(handle);

    let engine = match types::get_compose_handle(handle_id as u64) {
        Some(h) => h.clone(),
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid compose handle".to_string())
            });
            return promise;
        }
    };

    let services_json = unsafe { string_from_header(services_json_ptr) };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let services: Vec<String> = services_json
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        engine
            .start(&services)
            .await
            .map(|_| PROMISE_VOID_BITS)
            .map_err(|e| e.to_string())
    });

    promise
}

/// Stop compose services.
///
/// FFI: `js_container_compose_stop(handle: f64, services_json: *const StringHeader) -> *mut Promise`
#[no_mangle]
pub unsafe extern "C" fn js_container_compose_stop(
    handle: f64,
    services_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();
    let handle_id = handle_id_from_f64(handle);

    let engine = match types::get_compose_handle(handle_id as u64) {
        Some(h) => h.clone(),
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid compose handle".to_string())
            });
            return promise;
        }
    };

    let services_json = unsafe { string_from_header(services_json_ptr) };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let services: Vec<String> = services_json
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        engine
            .stop(&services)
            .await
            .map(|_| PROMISE_VOID_BITS)
            .map_err(|e| e.to_string())
    });

    promise
}

/// Restart compose services.
///
/// FFI: `js_container_compose_restart(handle: f64, services_json: *const StringHeader) -> *mut Promise`
#[no_mangle]
pub unsafe extern "C" fn js_container_compose_restart(
    handle: f64,
    services_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();
    let handle_id = handle_id_from_f64(handle);

    let engine = match types::get_compose_handle(handle_id as u64) {
        Some(h) => h.clone(),
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid compose handle".to_string())
            });
            return promise;
        }
    };

    let services_json = unsafe { string_from_header(services_json_ptr) };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let services: Vec<String> = services_json
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        engine
            .restart(&services)
            .await
            .map(|_| PROMISE_VOID_BITS)
            .map_err(|e| e.to_string())
    });

    promise
}

/// Get compose configuration
/// Get the resolved compose YAML configuration.
///
/// FFI: `js_container_compose_config(handle: f64) -> *mut Promise`
#[no_mangle]
pub unsafe extern "C" fn js_container_compose_config(handle: f64) -> *mut Promise {
    let promise = js_promise_new();
    let handle_id = handle_id_from_f64(handle);

    let engine = match types::get_compose_handle(handle_id as u64) {
        Some(h) => h.clone(),
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid compose handle".to_string())
            });
            return promise;
        }
    };

    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move { engine.config().map_err(|e| e.to_string()) },
        |yaml| {
            let str_ptr = perry_runtime::js_string_from_bytes(yaml.as_ptr(), yaml.len() as u32);
            perry_runtime::JSValue::string_ptr(str_ptr).bits()
        },
    );

    promise
}

// ============ Compose Functions ============

/// Bring up a Compose stack
/// FFI: js_container_composeUp(spec_json: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_composeUp(
    spec_ptr: *const perry_runtime::StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();

    let spec = match types::parse_compose_spec(spec_ptr) {
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
        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        let wrapper = compose::ComposeWrapper::new(spec, backend);
        match wrapper.up().await {
            Ok(_handle) => {
                let handle_id = types::register_compose_handle(wrapper.engine().clone());
                Ok(handle_to_promise_bits(handle_id))
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Alias for js_container_composeUp
#[no_mangle]
pub unsafe extern "C" fn js_compose_up(spec_ptr: *const StringHeader) -> *mut Promise {
    js_container_composeUp(spec_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_compose_down(
    handle: f64,
    opts_ptr: *const StringHeader,
) -> *mut Promise {
    js_container_compose_down(handle, opts_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_compose_ps(handle: f64) -> *mut Promise {
    js_container_compose_ps(handle)
}

#[no_mangle]
pub unsafe extern "C" fn js_compose_logs(
    handle: f64,
    service_ptr: *const StringHeader,
    tail: f64,
) -> *mut Promise {
    js_container_compose_logs(handle, service_ptr, tail)
}

#[no_mangle]
pub unsafe extern "C" fn js_compose_exec(
    handle: f64,
    service_ptr: *const StringHeader,
    cmd_json_ptr: *const StringHeader,
) -> *mut Promise {
    js_container_compose_exec(handle, service_ptr, cmd_json_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_compose_config(handle: f64) -> *mut Promise {
    js_container_compose_config(handle)
}

#[no_mangle]
pub unsafe extern "C" fn js_compose_start(
    handle: f64,
    services_json_ptr: *const StringHeader,
) -> *mut Promise {
    js_container_compose_start(handle, services_json_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_compose_stop(
    handle: f64,
    services_json_ptr: *const StringHeader,
) -> *mut Promise {
    js_container_compose_stop(handle, services_json_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_compose_restart(
    handle: f64,
    services_json_ptr: *const StringHeader,
) -> *mut Promise {
    js_container_compose_restart(handle, services_json_ptr)
}

/// Stop and remove compose stack.
///
/// FFI: `js_container_compose_down(handle: f64, opts_json: *const StringHeader)
///       -> *mut Promise`
///
/// `opts_json` is a JSON-encoded `DownOptions` object — the codegen's
/// `js_value_to_str_ptr_for_ffi` helper auto-stringifies the TS object
/// literal `{ volumes: bool, ...}`. Pre-fix the dispatch took the
/// options as `f64` (NA_F64), which only worked when the caller passed a
/// plain numeric flag — every TS user passing `down(handle, { volumes:
/// false })` got `remove_volumes = true` because the NaN-boxed object
/// pointer is non-zero. Same fix shape as `composeUp({...})` from
/// v0.5.370.
///
/// Recognised keys (all optional):
///   - `volumes: boolean`        remove named volumes (default `false`)
///   - `removeOrphans: boolean`  remove orphaned containers (default `false`)
#[no_mangle]
pub unsafe extern "C" fn js_container_compose_down(
    handle: f64,
    opts_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();
    let handle_id = handle_id_from_f64(handle);

    let opts_json = unsafe { string_from_header(opts_ptr) };
    let (remove_volumes, remove_orphans) = match opts_json.as_deref() {
        Some(s) if !s.is_empty() && s != "undefined" && s != "null" => {
            let v: serde_json::Value = serde_json::from_str(s).unwrap_or(serde_json::Value::Null);
            (
                v.get("volumes").and_then(|x| x.as_bool()).unwrap_or(false),
                v.get("removeOrphans")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false),
            )
        }
        _ => (false, false),
    };

    let engine = match types::take_compose_handle(handle_id as u64) {
        Some(h) => h,
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid compose handle".to_string())
            });
            return promise;
        }
    };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let _backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        let wrapper = compose::ComposeWrapper::new_from_engine(engine);
        match wrapper.down(remove_volumes, remove_orphans).await {
            Ok(()) => Ok(PROMISE_VOID_BITS),
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Get container info for compose stack.
///
/// FFI: `js_container_compose_ps(handle: f64) -> *mut Promise`
#[no_mangle]
pub unsafe extern "C" fn js_container_compose_ps(handle: f64) -> *mut Promise {
    let promise = js_promise_new();
    let handle_id = handle_id_from_f64(handle);

    let engine = match types::get_compose_handle(handle_id as u64) {
        Some(h) => h.clone(),
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid compose handle".to_string())
            });
            return promise;
        }
    };

    // Resolve the Promise with a JSON-encoded `ContainerInfo[]` string
    // rather than a registry-id handle. Pre-fix the FFI returned an
    // opaque NaN-boxed integer that user code couldn't iterate; the TS
    // type `Promise<ContainerInfo[]>` lied about the actual shape. Now
    // the Promise resolves to a JSON string the user `JSON.parse`s.
    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            let _backend = get_global_backend().await.map_err(|e| e.to_string())?;
            let wrapper = compose::ComposeWrapper::new_from_engine(engine);
            let containers = wrapper.ps().await.map_err(|e| e.to_string())?;
            serde_json::to_string(&containers).map_err(|e| e.to_string())
        },
        |json| {
            let str_ptr = perry_runtime::js_string_from_bytes(json.as_ptr(), json.len() as u32);
            perry_runtime::JSValue::string_ptr(str_ptr).bits()
        },
    );

    promise
}

/// Get logs from compose stack.
///
/// FFI: `js_container_compose_logs(handle: f64, service: *const StringHeader, tail: f64) -> *mut Promise`
///
/// `tail < 0.0` (or NaN / undefined sentinels) means "no limit".
#[no_mangle]
pub unsafe extern "C" fn js_container_compose_logs(
    handle: f64,
    service_ptr: *const StringHeader,
    tail: f64,
) -> *mut Promise {
    let promise = js_promise_new();
    let handle_id = handle_id_from_f64(handle);

    let engine = match types::get_compose_handle(handle_id as u64) {
        Some(h) => h.clone(),
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid compose handle".to_string())
            });
            return promise;
        }
    };

    let service = unsafe { string_from_header(service_ptr) };
    let tail_opt = if tail.is_finite() && tail >= 0.0 {
        Some(tail as u32)
    } else {
        None
    };

    // Resolve with a JSON-encoded `ContainerLogs` string ({ stdout,
    // stderr }) — see `compose_ps` for the rationale.
    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            let _backend = get_global_backend().await.map_err(|e| e.to_string())?;
            let wrapper = compose::ComposeWrapper::new_from_engine(engine);
            let logs = wrapper
                .logs(service.as_deref(), tail_opt)
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

/// Execute command in compose service.
///
/// FFI: `js_container_compose_exec(handle: f64, service: *const StringHeader, cmd_json: *const StringHeader) -> *mut Promise`
#[no_mangle]
pub unsafe extern "C" fn js_container_compose_exec(
    handle: f64,
    service_ptr: *const StringHeader,
    cmd_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();
    let handle_id = handle_id_from_f64(handle);

    let engine = match types::get_compose_handle(handle_id as u64) {
        Some(h) => h.clone(),
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid compose handle".to_string())
            });
            return promise;
        }
    };

    let service_opt = unsafe { string_from_header(service_ptr) };
    let cmd_json = unsafe { string_from_header(cmd_json_ptr) };

    // Resolve with a JSON-encoded `ContainerLogs` string.
    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            let service = service_opt.ok_or_else(|| "Invalid service name".to_string())?;
            let cmd: Vec<String> = cmd_json
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            let _backend = get_global_backend().await.map_err(|e| e.to_string())?;
            let wrapper = compose::ComposeWrapper::new_from_engine(engine);
            let logs = wrapper
                .exec(&service, &cmd)
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
