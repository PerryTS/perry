//! Container module for Perry
//!
//! Provides OCI container management with platform-adaptive backend selection.

pub mod backend;
pub mod capability;
pub mod compose;
pub mod types;
pub mod verification;

mod mod_private {
    use super::get_global_backend;
    use crate::container::backend::ContainerBackend;
    use std::sync::Arc;

    pub async fn get_global_backend_instance() -> Result<Arc<dyn ContainerBackend>, String> {
        get_global_backend()
            .await
            .map(|b| Arc::clone(b))
            .map_err(|e| e.to_string())
    }
}

// Re-export commonly used types
pub use types::{
    ComposeHandle, ComposeSpec, ContainerError, ContainerHandle, ContainerInfo, ContainerLogs,
    ContainerSpec, ImageInfo, ListOrDict,
};

pub use backend::{detect_backend, ContainerBackend};
use perry_runtime::{js_promise_new, Promise, StringHeader};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;

// Global backend instance - initialised once at first use
static BACKEND: OnceLock<Arc<dyn ContainerBackend>> = OnceLock::new();
static BACKEND_INIT_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Get or initialise the global backend instance.
///
/// Per SPEC §5.1 step 4: on `detect_backend()` failure, if stderr is an
/// interactive TTY *and* `PERRY_NO_INSTALL_PROMPT` is unset, hand off to
/// `BackendInstaller` so the user can pick + install a runtime. Both gates
/// must hold; otherwise the original `NoBackendFound` error propagates.
async fn get_global_backend() -> Result<&'static Arc<dyn ContainerBackend>, ContainerError> {
    if let Some(b) = BACKEND.get() {
        return Ok(b);
    }

    let _guard = BACKEND_INIT_MUTEX.lock().await;

    if let Some(b) = BACKEND.get() {
        return Ok(b);
    }

    let b = match detect_backend().await {
        Ok(backend) => Arc::from(backend) as Arc<dyn ContainerBackend>,
        Err(e) => {
            use std::io::IsTerminal;
            let interactive = std::io::stderr().is_terminal();
            let prompt_disabled = std::env::var("PERRY_NO_INSTALL_PROMPT").is_ok();
            if interactive && !prompt_disabled {
                let installer = perry_container_compose::BackendInstaller::new();
                match installer.run().await {
                    Ok(backend) => Arc::from(backend) as Arc<dyn ContainerBackend>,
                    Err(_) => return Err(ContainerError::from(e)),
                }
            } else {
                return Err(ContainerError::from(e));
            }
        }
    };

    let _ = BACKEND.set(b);
    Ok(BACKEND.get().unwrap())
}

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
}

/// Helper to create a JS string from a Rust string
unsafe fn string_to_js(s: &str) -> *const StringHeader {
    let bytes = s.as_bytes();
    perry_runtime::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// `POINTER_TAG` for NaN-boxing a handle id as an opaque pointer. This is
/// what every codegen `unbox_to_i64` call expects to find at the receiver
/// slot of a `has_receiver: true` dispatch row — the lower 48 bits are
/// masked off (`POINTER_MASK = 0x0000_FFFF_FFFF_FFFF`) and used as the
/// handle id directly. Matches `perry_runtime::value::POINTER_TAG`.
const POINTER_TAG_BITS: u64 = 0x7FFD_0000_0000_0000;

/// Encode a u64 handle id as the f64 bits a Promise resolution slot expects.
///
/// The async-bridge stores `result_bits: u64` and resolves the Promise via
/// `f64::from_bits(result_bits)`. Two things have to be true of those bits:
///
/// 1. **`${handle}` interpolation must produce something sane.** Pre-fix
///    `Ok(1u64)` resolved with f64 = `5e-324` (subnormal), which prints as
///    `"0"` — the user can't tell their handle from a void-resolution.
///
/// 2. **`down(stack, …)` / `stack.down(…)` dispatch must be able to recover
///    the original handle id.** The codegen lowers `stack` via
///    `unbox_to_i64` which expects a NaN-boxed value: it does
///    `bits & POINTER_MASK` (lower 48 bits) and treats that as the i64
///    handle. A bare `(id as f64).to_bits()` produces `0x3FF0_0000_…` for
///    id=1 — masked to lower 48, that's 0, and the FFI sees "Invalid
///    compose handle".
///
/// Both invariants are satisfied by NaN-boxing the handle with
/// `POINTER_TAG = 0x7FFD` in the upper 16 bits and the id in the lower
/// 48: `unbox_to_i64` recovers the id verbatim, and `JSValue::format`
/// (called by template-string coercion) sees the POINTER_TAG and prints
/// the id as a numeric handle.
#[inline]
fn handle_to_promise_bits(id: u64) -> u64 {
    POINTER_TAG_BITS | (id & 0x0000_FFFF_FFFF_FFFF)
}

/// `TAG_UNDEFINED` as raw f64 bits. Used by `Promise<void>` FFIs to resolve
/// with `undefined` rather than `0` (matches JS semantics).
const PROMISE_VOID_BITS: u64 = 0x7FFC_0000_0000_0001;

/// Decode a NaN-boxed f64 receiver/handle back to its registry id (i64).
///
/// The codegen `NA_F64` arg-coercion rule passes the user's `stack` variable
/// through to the FFI as `double`. So when `js_compose_down` etc. take the
/// handle as their first parameter, the LLVM declare emits `double`, the
/// f64 lands in XMM0, and Rust must read it as `f64` to match the calling
/// convention (declaring the arg as `i64` makes Rust read RDI instead and
/// the FFI sees garbage).
///
/// `handle_to_promise_bits` NaN-boxes the id with POINTER_TAG, so the f64
/// the user receives carries the id in its lower 48 bits. This helper
/// reverses that boxing — masking off the tag and reading the id verbatim.
#[inline]
fn handle_id_from_f64(boxed: f64) -> i64 {
    (boxed.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64
}

/// Optionally verify a container image's signature before pulling/running.
///
/// Gated on `PERRY_CONTAINER_VERIFY_IMAGES=1` so the default path stays
/// cosign-free for development + CI parity. When the env var is set, the
/// image is run through `verification::verify_image()` (cosign keyless
/// verification against Chainguard identity) and a failure short-circuits
/// the FFI call with a `verification failed` error string.
///
/// SPEC §11.2 calls this out as "present but not yet enforced in HEAD"; this
/// helper is the integration point. Per-call guard rather than a global
/// `up()`-only one so users can pin individual `run`/`create`/`pullImage`
/// invocations to verified images while leaving compose stacks unchecked.
/// Image-verification mode controlled by `PERRY_CONTAINER_VERIFY_IMAGES`.
///
/// | Value | Behavior |
/// |---|---|
/// | unset / `"0"` / `"off"` (default) | Skip verification entirely. |
/// | `"warn"` | Run cosign verification; on fail, print a warning to stderr and proceed. Useful as a "soft-enable" during rollout — surfaces signing gaps without blocking deployment. |
/// | `"1"` / `"on"` / `"enforce"` (production) | Run cosign verification; on fail, reject the FFI call with `verification failed`. **This is the recommended setting for production deploys.** |
///
/// Values other than the above are treated as `"warn"` (forgiving default
/// for typos like `PERRY_CONTAINER_VERIFY_IMAGES=true`).
#[derive(Clone, Copy)]
enum VerifyMode {
    Off,
    Warn,
    Enforce,
}

fn current_verify_mode() -> VerifyMode {
    match std::env::var("PERRY_CONTAINER_VERIFY_IMAGES")
        .ok()
        .as_deref()
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        None | Some("") | Some("0") | Some("off") | Some("false") | Some("no") => VerifyMode::Off,
        Some("1") | Some("on") | Some("enforce") | Some("strict") => VerifyMode::Enforce,
        // anything else (including "warn", "true", "yes", typos) → warn
        Some(_) => VerifyMode::Warn,
    }
}

async fn maybe_verify_image(image: &str) -> Result<(), String> {
    match current_verify_mode() {
        VerifyMode::Off => Ok(()),
        VerifyMode::Enforce => crate::container::verification::verify_image(image)
            .await
            .map(|_digest| ()),
        VerifyMode::Warn => match crate::container::verification::verify_image(image).await {
            Ok(_digest) => Ok(()),
            Err(e) => {
                eprintln!(
                    "[perry/container] WARNING: image verification failed for {image}: {e} \
                     (PERRY_CONTAINER_VERIFY_IMAGES=warn — proceeding anyway; \
                     set =enforce / =1 to reject unsigned images, =off / =0 to skip the check)"
                );
                Ok(())
            }
        },
    }
}

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
        match backend.run(&spec).await {
            Ok(handle) => {
                let handle_id = types::register_container_handle(handle);
                Ok(handle_to_promise_bits(handle_id as u64))
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

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
        match backend.create(&spec).await {
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
fn parse_cleanup_options(
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

/// Get the current backend name.
///
/// FFI: `js_container_getBackend() -> *const StringHeader`
///
/// Returns the canonical backend name (e.g. `"docker"` / `"podman"` /
/// `"apple/container"` / `"colima"` / `"orbstack"` / `"lima"`) when the
/// backend singleton is initialised. If not yet initialised, performs a
/// synchronous in-place detection so user code that calls `getBackend()`
/// at module scope (before any `await` has triggered `get_global_backend`)
/// gets the live name instead of the misleading `"unknown"` sentinel.
///
/// The synchronous probe uses `tokio::runtime::Handle::try_current()` +
/// `block_in_place` when called from inside a tokio worker, falling back
/// to a one-shot `Runtime::new().block_on(...)` otherwise. Returns
/// `"unknown"` only when detection genuinely fails (no backend installed
/// + non-interactive). Detection latency is bounded by the same 2-second
/// per-candidate timeout as `detect_backend()`.
#[no_mangle]
pub unsafe extern "C" fn js_container_getBackend() -> *const StringHeader {
    if let Some(b) = BACKEND.get() {
        return string_to_js(b.backend_name());
    }

    // No backend yet — try to populate the singleton synchronously.
    // Strategy:
    //   1. If we're inside a tokio worker, `block_in_place` lets us call
    //      the async detect_backend() without deadlocking the runtime.
    //   2. If we're on the main thread with no runtime active, spin up
    //      a fresh single-threaded runtime for the probe.
    //   3. On any failure (no runtime + main-thread-bound, detection
    //      error, etc.), fall back to the legacy "unknown" sentinel.
    let resolved = if let Ok(handle) = tokio::runtime::Handle::try_current() {
        match handle.runtime_flavor() {
            tokio::runtime::RuntimeFlavor::CurrentThread => {
                // current_thread runtimes can't `block_in_place`; the only
                // safe move is to skip the sync probe and let the next
                // async FFI call populate BACKEND. Return "unknown".
                None
            }
            _ => Some(tokio::task::block_in_place(|| {
                handle.block_on(get_global_backend())
            })),
        }
    } else {
        // No active runtime — spin up a temp one purely for detection.
        // The result is stored in the OnceLock so subsequent FFI calls
        // see it; the temp runtime is dropped immediately after.
        match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => Some(rt.block_on(get_global_backend())),
            Err(_) => None,
        }
    };

    match resolved {
        Some(Ok(b)) => string_to_js(b.backend_name()),
        _ => string_to_js("unknown"),
    }
}

/// Detect backend and return probed info
/// FFI: js_container_detectBackend() -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_detectBackend() -> *mut Promise {
    let promise = js_promise_new();
    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            match detect_backend().await {
                Ok(b) => {
                    let name = b.backend_name().to_string();
                    let json = serde_json::json!([{
                        "name": name,
                        "available": true,
                        "reason": ""
                    }])
                    .to_string();
                    Ok(json)
                }
                Err(e) => {
                    use perry_container_compose::error::ComposeError;
                    let json = match e {
                        ComposeError::NoBackendFound { probed } => {
                            serde_json::to_string(&probed).unwrap_or_else(|_| "[]".to_string())
                        }
                        _ => serde_json::json!([{
                            "name": "unknown",
                            "available": false,
                            "reason": e.to_string()
                        }])
                        .to_string(),
                    };
                    Ok(json)
                }
            }
        },
        |json| {
            let str_ptr = perry_runtime::js_string_from_bytes(json.as_ptr(), json.len() as u32);
            perry_runtime::JSValue::string_ptr(str_ptr).bits()
        },
    );
    promise
}

/// FFI: `js_container_selectBackendFor(spec_json, mode) -> *const StringHeader`
///
/// Pick the highest-priority backend whose `BackendCapabilities` can
/// honor every feature the spec uses. Pure introspection — no probes,
/// no network calls, no filesystem access. Returns the canonical
/// backend name (e.g. `"apple/container"`, `"docker"`, `"podman"`) or
/// the JSON sentinel `"null"` if no backend can honor the spec under
/// the given strictness mode.
///
/// **Mode semantics** (string arg, falls back to `AcceptEmulated`):
/// - `"strict-native"` — only `Native` features count
/// - `"accept-emulated"` (default) — `Native` + `Emulated` count
/// - `"accept-partial"` — `Native` + `Emulated` + `Partial` count
///
/// **Workflow:**
/// ```typescript
/// const best = selectBackendFor(JSON.stringify(spec), 'accept-emulated');
/// if (best === 'null') throw new Error('no backend can honor this spec');
/// const parsed = JSON.parse(best); // -> "docker" | "apple/container" | ...
/// await setBackend(parsed);
/// await up(spec);
/// ```
#[no_mangle]
pub unsafe extern "C" fn js_container_selectBackendFor(
    spec_ptr: *const StringHeader,
    mode_ptr: *const StringHeader,
) -> *const StringHeader {
    let spec_json = match string_from_header(spec_ptr) {
        Some(s) => s,
        None => return string_to_js("null"),
    };
    let mode_str = string_from_header(mode_ptr).unwrap_or_default();
    let mode = match mode_str.as_str() {
        "strict-native" => perry_container_compose::SelectMode::StrictNative,
        "accept-partial" => perry_container_compose::SelectMode::AcceptPartial,
        _ => perry_container_compose::SelectMode::AcceptEmulated,
    };

    let spec: perry_container_compose::ComposeSpec = match serde_json::from_str(&spec_json) {
        Ok(s) => s,
        Err(_) => return string_to_js("null"),
    };

    match perry_container_compose::select_backend_for(&spec, mode) {
        Some(name) => {
            let json = serde_json::to_string(name).unwrap_or_else(|_| "null".to_string());
            string_to_js(&json)
        }
        None => string_to_js("null"),
    }
}

/// FFI: `js_container_getAvailableBackends() -> *mut Promise`
///
/// Probe **every** backend in the platform priority list and return
/// one `BackendInfo` per candidate, in priority order. Unlike
/// `detectBackend()`, never short-circuits — always returns the full
/// list, with `available: true` on the ones that probed cleanly and
/// `available: false` plus a `reason` on the rest.
///
/// Useful for:
/// - Diagnostics ("what's installed on this host?")
/// - CI matrix lane resolution ("can I run the apple/container lane here?")
/// - User-facing UIs that want to render a backend picker
/// - Programmatic fallback chains: take the available subset and feed
///   it to `setBackends()`.
///
/// Each candidate gets a 2-second probe timeout. Worst-case latency
/// is `2s × len(platform_candidates())` — on macOS that's up to 16s
/// in the all-uninstalled case, but in practice only one or two
/// candidates take the full 2s before bailing.
///
/// @returns JSON-encoded `BackendInfo[]`, length always equal to
///   `getBackendPriority().length`.
///
/// @example
///   const all = JSON.parse(await getAvailableBackends()) as BackendInfo[];
///   const ready = all.filter(b => b.available);
///   if (ready.length === 0) throw new Error('no container runtime installed');
///   await setBackends(ready.map(b => b.name));
#[no_mangle]
pub unsafe extern "C" fn js_container_getAvailableBackends() -> *mut Promise {
    let promise = js_promise_new();
    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            let probed = perry_container_compose::probe_all_candidates().await;
            let json = serde_json::to_string(&probed).unwrap_or_else(|_| "[]".to_string());
            Ok::<String, String>(json)
        },
        |json| {
            let str_ptr = perry_runtime::js_string_from_bytes(json.as_ptr(), json.len() as u32);
            perry_runtime::JSValue::string_ptr(str_ptr).bits()
        },
    );
    promise
}

/// FFI: `js_container_getBackendPriority() -> *const StringHeader`
///
/// Returns the platform-specific backend probe order as a JSON-encoded
/// string array (`["apple/container", "orbstack", ...]`). The list is
/// canonical at compile time — see `platform_candidates()` in
/// `perry-container-compose::backend` for the encoding rationale.
///
/// Useful for diagnostics ("which backends will Perry try, in what
/// order?") and for programmatic backend selection (`setBackend()` only
/// accepts names in this list).
#[no_mangle]
pub unsafe extern "C" fn js_container_getBackendPriority() -> *const StringHeader {
    let candidates = perry_container_compose::platform_candidates();
    let json = serde_json::to_string(candidates).unwrap_or_else(|_| "[]".to_string());
    string_to_js(&json)
}

/// FFI: `js_container_setBackend(name: *const StringHeader) -> *mut Promise`
///
/// Programmatically pin a specific backend, equivalent to setting the
/// `PERRY_CONTAINER_BACKEND` env var before process start but callable
/// from TS. Must be called BEFORE any other `perry/container` or
/// `perry/compose` operation that initialises the global backend
/// singleton; once initialised, `BACKEND` is immutable (OnceLock can't
/// be reset) and this function returns an error so the caller knows
/// the override didn't take effect.
///
/// Promise resolves with the canonical backend name on success, or
/// rejects with one of:
/// - `"backend already initialised; setBackend must be called before any other container op"`
/// - `"unknown backend: '<name>'. Valid: [...]"`
/// - `"backend probe failed: <reason>"`
#[no_mangle]
pub unsafe extern "C" fn js_container_setBackend(name_ptr: *const StringHeader) -> *mut Promise {
    let promise = js_promise_new();
    let name = match string_from_header(name_ptr) {
        Some(s) => s,
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid backend name pointer".to_string())
            });
            return promise;
        }
    };

    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            // Reject if BACKEND already initialised — OnceLock can't be
            // reset, so mid-process switching would just be deceptive
            // (env var would update but cached singleton wouldn't).
            if BACKEND.get().is_some() {
                return Err("backend already initialised; setBackend must be called \
                     before any other container op"
                    .to_string());
            }

            // Reject if name isn't in the canonical probe list. We use
            // platform_candidates() rather than a hardcoded list so this
            // stays in sync with `detect_backend()`'s actual probe paths.
            let candidates = perry_container_compose::platform_candidates();
            if !candidates.iter().any(|c| **c == name) {
                return Err(format!(
                    "unknown backend: '{}'. Valid: {:?}",
                    name, candidates
                ));
            }

            // Set the env var so detect_backend() honors it on next call,
            // then trigger detection now to return success/failure to the
            // caller synchronously.
            std::env::set_var("PERRY_CONTAINER_BACKEND", &name);
            match get_global_backend().await {
                Ok(b) => Ok(b.backend_name().to_string()),
                Err(e) => Err(format!("backend probe failed: {}", e)),
            }
        },
        |s| {
            let str_ptr = perry_runtime::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            perry_runtime::JSValue::string_ptr(str_ptr).bits()
        },
    );
    promise
}

/// FFI: `js_container_setBackends(names_json: *const StringHeader) -> *mut Promise`
///
/// User-defined priority list — try each backend in order, first
/// available wins. Generalises `setBackend(name)` for the common
/// production pattern "prefer podman, fall back to docker." Each name
/// must come from `getBackendPriority()`.
///
/// Equivalent to setting `PERRY_CONTAINER_BACKEND=name1,name2,...`
/// before process start. Must be called BEFORE any other container
/// op (the global `OnceLock` can't be reset; setBackends rejects with
/// a clear message after singleton init fires).
///
/// Promise resolves with the canonical name of the backend that
/// actually got picked, or rejects with one of:
/// - `"backend already initialised; setBackends must be called before any other container op"`
/// - `"setBackends requires a non-empty array"`
/// - `"unknown backend: '<typo>'. Valid: [...]"` — any one of the names is unrecognised
/// - `"none of the requested backends could be probed: [...]"` — all named backends are unavailable
///
/// @example
///   import { setBackends, up } from 'perry/container';
///   // Try podman first (rootless, OCI-compatible); fall back to docker.
///   await setBackends(['podman', 'docker']);
///   await up({ services: { ... } });
#[no_mangle]
pub unsafe extern "C" fn js_container_setBackends(
    names_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();
    let names_json = match string_from_header(names_json_ptr) {
        Some(s) => s,
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid names array pointer".to_string())
            });
            return promise;
        }
    };

    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            // Reject if BACKEND already initialised — same OnceLock
            // contract as setBackend.
            if BACKEND.get().is_some() {
                return Err("backend already initialised; setBackends must be called \
                     before any other container op"
                    .to_string());
            }

            // Parse the JSON-encoded array. Caller is expected to do
            // JSON.stringify(['podman', 'docker']) on the TS side.
            let names: Vec<String> = match serde_json::from_str(&names_json) {
                Ok(v) => v,
                Err(e) => {
                    return Err(format!(
                        "invalid backends JSON (expected JSON-encoded string[]): {}",
                        e
                    ))
                }
            };

            if names.is_empty() {
                return Err("setBackends requires a non-empty array".to_string());
            }

            // Validate every name against the canonical probe list
            // BEFORE setting the env var — fail fast on typos so a
            // partially-valid list doesn't masquerade as success.
            let candidates = perry_container_compose::platform_candidates();
            for n in &names {
                if !candidates.iter().any(|c| **c == *n) {
                    return Err(format!("unknown backend: '{}'. Valid: {:?}", n, candidates));
                }
            }

            // Set the env var as a comma-joined list so detect_backend()
            // walks them in user-supplied order. (detect_backend's
            // env-var path was extended to handle comma-separated lists
            // exactly for this — single-name backwards-compat preserved.)
            let joined = names.join(",");
            std::env::set_var("PERRY_CONTAINER_BACKEND", &joined);

            match get_global_backend().await {
                Ok(b) => Ok(b.backend_name().to_string()),
                Err(e) => Err(format!(
                    "none of the requested backends could be probed: {}",
                    e
                )),
            }
        },
        |s| {
            let str_ptr = perry_runtime::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            perry_runtime::JSValue::string_ptr(str_ptr).bits()
        },
    );
    promise
}

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
    let (remove_volumes, _remove_orphans) = match opts_json.as_deref() {
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
        match wrapper.down(remove_volumes).await {
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

// ============ Workload Functions ============

/// Create a workload graph
/// FFI: js_workload_graph(name: *const StringHeader, nodes_json: *const StringHeader) -> *const StringHeader
#[no_mangle]
pub unsafe extern "C" fn js_workload_graph(
    name_ptr: *const StringHeader,
    nodes_json_ptr: *const StringHeader,
) -> *const StringHeader {
    let name = string_from_header(name_ptr).unwrap_or_default();
    let nodes_json = string_from_header(nodes_json_ptr).unwrap_or_else(|| "{}".to_string());

    let graph = perry_container_compose::WorkloadGraph {
        name,
        nodes: serde_json::from_str(&nodes_json).unwrap_or_default(),
        edges: vec![], // Edges inferred from depends_on in nodes
    };

    let json = serde_json::to_string(&graph).unwrap_or_default();
    string_to_js(&json)
}

/// Create a workload node
/// FFI: js_workload_node(name: *const StringHeader, spec_json: *const StringHeader) -> *const StringHeader
#[no_mangle]
pub unsafe extern "C" fn js_workload_node(
    name_ptr: *const StringHeader,
    spec_json_ptr: *const StringHeader,
) -> *const StringHeader {
    let name = string_from_header(name_ptr).unwrap_or_default();
    let spec_json = string_from_header(spec_json_ptr).unwrap_or_else(|| "{}".to_string());

    let mut node: perry_container_compose::WorkloadNode = serde_json::from_str(&spec_json)
        .unwrap_or_else(|_| perry_container_compose::WorkloadNode {
            id: name.clone(),
            name: name.clone(),
            image: None,
            resources: None,
            ports: vec![],
            env: HashMap::new(),
            depends_on: vec![],
            runtime: perry_container_compose::RuntimeSpec::Auto,
            policy: perry_container_compose::PolicySpec::default(),
        });
    node.id = name.clone();
    node.name = name;

    let json = serde_json::to_string(&node).unwrap_or_default();
    string_to_js(&json)
}

/// Run a workload graph
/// FFI: js_workload_runGraph(graph_json: *const StringHeader, opts_json: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_workload_runGraph(
    graph_json_ptr: *const StringHeader,
    opts_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();

    let graph_json = string_from_header(graph_json_ptr).unwrap_or_else(|| "{}".to_string());
    let opts_json = string_from_header(opts_json_ptr).unwrap_or_else(|| "{}".to_string());

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let graph: perry_container_compose::WorkloadGraph = serde_json::from_str(&graph_json)
            .map_err(|e| format!("Failed to parse graph: {}", e))?;
        let opts: perry_container_compose::RunGraphOptions = serde_json::from_str(&opts_json)
            .map_err(|e| format!("Failed to parse options: {}", e))?;

        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };

        let engine = Arc::new(perry_container_compose::WorkloadGraphEngine::new(
            graph, backend,
        ));
        match engine.run(opts).await {
            Ok(_) => {
                let handle_id = types::register_workload_handle(engine);
                Ok(handle_to_promise_bits(handle_id))
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Inspect a workload graph
/// FFI: js_workload_inspectGraph(handle_id: i64) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_workload_inspectGraph(handle_id: i64) -> *mut Promise {
    let promise = js_promise_new();
    let id = handle_id as u64;

    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            let engine = match types::WORKLOAD_HANDLES.get().and_then(|m| m.get(&id)) {
                Some(e) => e.clone(),
                None => return Err("Invalid workload handle".to_string()),
            };

            match engine.status().await {
                Ok(status) => {
                    let json = serde_json::to_string(&status).unwrap_or_default();
                    Ok(json)
                }
                Err(e) => Err(e.to_string()),
            }
        },
        |json| {
            let str_ptr = perry_runtime::js_string_from_bytes(json.as_ptr(), json.len() as u32);
            perry_runtime::JSValue::string_ptr(str_ptr).bits()
        },
    );

    promise
}

/// Stop and remove a workload graph
/// FFI: js_workload_handle_down(handle_id: i64, force: i32) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_workload_handle_down(handle_id: i64, force: i32) -> *mut Promise {
    let promise = js_promise_new();
    let id = handle_id as u64;

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let engine = match types::WORKLOAD_HANDLES.get().and_then(|m| m.get(&id)) {
            Some(e) => e.clone(),
            None => return Err("Invalid workload handle".to_string()),
        };

        match engine.down(force != 0).await {
            Ok(_) => {
                if let Some(handles) = types::WORKLOAD_HANDLES.get() {
                    handles.remove(&id);
                }
                Ok(PROMISE_VOID_BITS)
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Get status of a workload graph
/// FFI: js_workload_handle_status(handle_id: i64) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_workload_handle_status(handle_id: i64) -> *mut Promise {
    let promise = js_promise_new();
    let id = handle_id as u64;

    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            let engine = match types::WORKLOAD_HANDLES.get().and_then(|m| m.get(&id)) {
                Some(e) => e.clone(),
                None => return Err("Invalid workload handle".to_string()),
            };

            match engine.status().await {
                Ok(status) => {
                    let json = serde_json::to_string(&status).unwrap_or_default();
                    Ok(json)
                }
                Err(e) => Err(e.to_string()),
            }
        },
        |json| {
            let str_ptr = perry_runtime::js_string_from_bytes(json.as_ptr(), json.len() as u32);
            perry_runtime::JSValue::string_ptr(str_ptr).bits()
        },
    );

    promise
}

/// Get logs from a workload node
/// FFI: js_workload_handle_logs(handle_id: i64, node_id: *const StringHeader, tail: i32) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_workload_handle_logs(
    handle_id: i64,
    node_id_ptr: *const StringHeader,
    tail: i32,
) -> *mut Promise {
    let promise = js_promise_new();
    let id = handle_id as u64;
    let node_id = string_from_header(node_id_ptr).unwrap_or_default();
    let tail_opt = if tail >= 0 { Some(tail as u32) } else { None };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let engine = match types::WORKLOAD_HANDLES.get().and_then(|m| m.get(&id)) {
            Some(e) => e.clone(),
            None => return Err("Invalid workload handle".to_string()),
        };

        match engine.logs(&node_id, tail_opt).await {
            Ok(logs) => {
                let handle_id = types::register_container_logs(logs);
                Ok(handle_to_promise_bits(handle_id))
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Execute command in a workload node
/// FFI: js_workload_handle_exec(handle_id: i64, node_id: *const StringHeader, cmd_json: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_workload_handle_exec(
    handle_id: i64,
    node_id_ptr: *const StringHeader,
    cmd_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();
    let id = handle_id as u64;
    let node_id = string_from_header(node_id_ptr).unwrap_or_default();
    let cmd_json = string_from_header(cmd_json_ptr).unwrap_or_else(|| "[]".to_string());

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let cmd: Vec<String> = serde_json::from_str(&cmd_json).unwrap_or_default();
        let engine = match types::WORKLOAD_HANDLES.get().and_then(|m| m.get(&id)) {
            Some(e) => e.clone(),
            None => return Err("Invalid workload handle".to_string()),
        };

        match engine.exec(&node_id, &cmd).await {
            Ok(logs) => {
                let handle_id = types::register_container_logs(logs);
                Ok(handle_to_promise_bits(handle_id))
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Get process status of a workload graph
/// FFI: js_workload_handle_ps(handle_id: i64) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_workload_handle_ps(handle_id: i64) -> *mut Promise {
    let promise = js_promise_new();
    let id = handle_id as u64;

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let engine = match types::WORKLOAD_HANDLES.get().and_then(|m| m.get(&id)) {
            Some(e) => e.clone(),
            None => return Err("Invalid workload handle".to_string()),
        };

        match engine.ps().await {
            Ok(infos) => {
                // Register NodeInfo list as a container info list (compatible for now)
                // Actually we should probably have a register_node_info_list
                let handle_id = types::register_container_info_list(
                    infos
                        .into_iter()
                        .map(|i| ContainerInfo {
                            id: i.container_id.unwrap_or_default(),
                            name: i.name,
                            image: i.image.unwrap_or_default(),
                            status: format!("{:?}", i.state),
                            ports: vec![],
                            labels: HashMap::new(),
                            created: "".to_string(),
                            ip_address: i.ip_address.unwrap_or_default(),
                        })
                        .collect(),
                );
                Ok(handle_to_promise_bits(handle_id))
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Get graph JSON from workload handle
/// FFI: js_workload_handle_graph(handle_id: i64) -> *const StringHeader
#[no_mangle]
pub unsafe extern "C" fn js_workload_handle_graph(handle_id: i64) -> *const StringHeader {
    let id = handle_id as u64;
    let engine = match types::WORKLOAD_HANDLES.get().and_then(|m| m.get(&id)) {
        Some(e) => e.clone(),
        None => return std::ptr::null(),
    };

    let json = serde_json::to_string(&engine.graph).unwrap_or_default();
    string_to_js(&json)
}

// ============ Module Initialization ============

/// Initialise the container module (called during runtime startup).
///
/// Per SPEC §11.6 / Task 18.1, this is a one-shot link-time anchor that:
/// 1. Forces `libperry_stdlib`'s container symbols to be retained (any
///    user code calling `js_container_module_init()` will pull in the
///    transitively-referenced FFI symbols and prevent dead-strip).
/// 2. Pre-warms the backend singleton when called from a tokio context —
///    avoids paying the probe latency on the first user `run()` call.
///
/// Backend probing is async + may invoke the interactive `BackendInstaller`,
/// so we must not block here. Instead we spawn the probe as a detached
/// tokio task; if a tokio runtime isn't yet running (called from `main`
/// before any async setup), the task simply doesn't run and the first
/// real FFI call will trigger probe-on-demand the same way it always has.
#[no_mangle]
pub extern "C" fn js_container_module_init() {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(async {
            let _ = get_global_backend().await;
        });
    }
    install_default_signal_cleanup();
}

/// Install a process-level SIGINT / SIGTERM handler that tears down any
/// Compose stacks the user brought up but never called `down()` on.
///
/// **Why this exists:** Perry's runtime currently does not deliver
/// POSIX signals to TS-side `process.on('SIGINT', ...)` handlers. So a
/// program that does `await up(spec)` and then waits on something
/// (long-running watch loop, blocked network read, etc.) will, on
/// Ctrl-C, leave every container the stack created running. The user
/// has to `docker rm -f` them by hand.
///
/// This handler runs at the OS-process level: when the process
/// receives SIGINT or SIGTERM, the handler walks the global
/// `COMPOSE_HANDLES` registry, calls `down(volumes=false)` on each
/// engine (so committed data survives), and then exits with status
/// matching the signal (130 for SIGINT, 143 for SIGTERM).
///
/// Idempotent: calling `install_default_signal_cleanup()` multiple
/// times is safe — internally guarded by `OnceLock`.
///
/// Opt out: `PERRY_NO_DEFAULT_SIGINT_CLEANUP=1` skips installation
/// (for callers that intend to handle teardown themselves and don't
/// want the default tear-down).
fn install_default_signal_cleanup() {
    use std::sync::OnceLock;
    static INSTALLED: OnceLock<()> = OnceLock::new();
    if INSTALLED.set(()).is_err() {
        return;
    }
    if std::env::var("PERRY_NO_DEFAULT_SIGINT_CLEANUP").is_ok() {
        return;
    }
    // Need a tokio runtime handle to drive the async `down()` calls
    // from inside the signal handler. If there's no current runtime
    // (the user invoked module_init before any async work), skip the
    // install — the user will set up their own teardown if they need
    // signal handling at all.
    let rt = match tokio::runtime::Handle::try_current() {
        Ok(h) => h,
        Err(_) => return,
    };
    rt.spawn(async {
        // Listen for both SIGINT (Ctrl-C) and SIGTERM (kill) on Unix;
        // Windows only delivers Ctrl-C / Ctrl-Break which tokio maps to
        // ctrl_c() / ctrl_break(). The select! exits as soon as either
        // arrives, then the cleanup runs once.
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigint = match signal(SignalKind::interrupt()) {
                Ok(s) => s,
                Err(_) => return,
            };
            let mut sigterm = match signal(SignalKind::terminate()) {
                Ok(s) => s,
                Err(_) => return,
            };
            let exit_code = tokio::select! {
                _ = sigint.recv()  => 130,  // 128 + SIGINT(2)
                _ = sigterm.recv() => 143,  // 128 + SIGTERM(15)
            };
            drain_compose_handles().await;
            std::process::exit(exit_code);
        }
        #[cfg(not(unix))]
        {
            if tokio::signal::ctrl_c().await.is_ok() {
                drain_compose_handles().await;
                std::process::exit(130);
            }
        }
    });
}

/// Walk the global `COMPOSE_HANDLES` registry and call `down(volumes=
/// false)` on each engine. Run from the SIGINT/SIGTERM cleanup task —
/// volumes are preserved by default so committed data survives an
/// abnormal shutdown; users who want destructive cleanup must call
/// `down(handle, { volumes: true })` explicitly while their process
/// is still alive.
async fn drain_compose_handles() {
    let registry = match types::COMPOSE_HANDLES.get() {
        Some(r) => r,
        None => return,
    };
    // Snapshot the keys so we don't hold the dashmap across awaits.
    let ids: Vec<u64> = registry.iter().map(|e| *e.key()).collect();
    for id in ids {
        if let Some(engine) = types::take_compose_handle(id) {
            let wrapper = compose::ComposeWrapper::new_from_engine(engine);
            let _ = wrapper.down(false).await;
        }
    }
}

#[cfg(test)]
mod smoke_tests {
    use super::*;

    /// Task 27.1: `js_container_module_init` must be callable without panic
    /// outside an active tokio runtime. The link-anchor purpose mustn't
    /// depend on async setup.
    #[test]
    fn module_init_is_safe_to_call_outside_tokio() {
        js_container_module_init();
    }

    /// Task 27.1: when called inside a tokio runtime, module_init schedules
    /// the backend probe without blocking the caller. The detached probe
    /// task may fail (no backend installed in CI); we only assert the call
    /// itself returns synchronously without panic and that the runtime is
    /// still alive afterwards.
    #[test]
    fn module_init_inside_tokio_runtime_does_not_block() {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            js_container_module_init();
            // If we reach here without hanging, the call returned
            // synchronously — invariant proved.
        });
    }

    /// Task 27.1: the canonical FFI symbols listed in SPEC §9.1 must all be
    /// addressable from this crate (link-time check). Unresolved symbols
    /// would fail to build, so this test merely takes the address of each
    /// to force the rustc usage check.
    #[test]
    fn ffi_symbols_resolve() {
        let _ = js_container_run as unsafe extern "C" fn(_) -> _;
        let _ = js_container_create as unsafe extern "C" fn(_) -> _;
        let _ = js_container_start as unsafe extern "C" fn(_) -> _;
        let _ = js_container_stop as unsafe extern "C" fn(_, _) -> _;
        let _ = js_container_remove as unsafe extern "C" fn(_, _) -> _;
        let _ = js_container_list as unsafe extern "C" fn(_) -> _;
        let _ = js_container_inspect as unsafe extern "C" fn(_) -> _;
        let _ = js_container_logs as unsafe extern "C" fn(_, _) -> _;
        let _ = js_container_pullImage as unsafe extern "C" fn(_) -> _;
        let _ = js_container_listImages as unsafe extern "C" fn() -> _;
        let _ = js_container_getBackend as unsafe extern "C" fn() -> _;
        let _ = js_container_module_init as extern "C" fn();
    }
}
