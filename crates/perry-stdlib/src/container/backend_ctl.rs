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
