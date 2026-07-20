//! Container module for Perry
//!
//! Provides OCI container management with platform-adaptive backend selection.

pub mod backend;
pub mod capability;
pub mod compose;
pub mod types;
pub mod verification;

// Topical FFI sub-modules split out of this trunk (pure code move).
mod backend_ctl;
mod compose_ffi;
mod images;
mod lifecycle;
mod logs_exec;
mod workload;

// Re-export the `#[no_mangle]` FFI surface (js_container_* / js_compose_* /
// js_workload_*) at the `container::` path. These fns were defined directly in
// this module before the split, so by-path consumers (e.g. the
// `container_ffi_tests` integration test referencing
// `perry_stdlib::container::js_container_run`) keep resolving.
pub use backend_ctl::*;
pub use compose_ffi::*;
pub use images::*;
pub use lifecycle::*;
pub use logs_exec::*;
pub use workload::*;

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
pub(crate) static BACKEND: OnceLock<Arc<dyn ContainerBackend>> = OnceLock::new();
static BACKEND_INIT_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Get or initialise the global backend instance.
///
/// Per SPEC §5.1 step 4: on `detect_backend()` failure, if stderr is an
/// interactive TTY *and* `PERRY_NO_INSTALL_PROMPT` is unset, hand off to
/// `BackendInstaller` so the user can pick + install a runtime. Both gates
/// must hold; otherwise the original `NoBackendFound` error propagates.
pub(crate) async fn get_global_backend(
) -> Result<&'static Arc<dyn ContainerBackend>, ContainerError> {
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
pub(crate) unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
}

/// Helper to create a JS string from a Rust string
pub(crate) unsafe fn string_to_js(s: &str) -> *const StringHeader {
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
pub(crate) fn handle_to_promise_bits(id: u64) -> u64 {
    POINTER_TAG_BITS | (id & 0x0000_FFFF_FFFF_FFFF)
}

/// `TAG_UNDEFINED` as raw f64 bits. Used by `Promise<void>` FFIs to resolve
/// with `undefined` rather than `0` (matches JS semantics).
pub(crate) const PROMISE_VOID_BITS: u64 = 0x7FFC_0000_0000_0001;

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
pub(crate) fn handle_id_from_f64(boxed: f64) -> i64 {
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

pub(crate) async fn maybe_verify_image(image: &str) -> Result<(), String> {
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
            let _ = wrapper.down(false, false).await;
        }
    }
}

#[cfg(test)]
mod smoke_tests {
    use super::*;
    use backend_ctl::js_container_getBackend;
    use images::{js_container_listImages, js_container_pullImage};
    use lifecycle::{
        js_container_create, js_container_inspect, js_container_list, js_container_remove,
        js_container_run, js_container_start, js_container_stop,
    };
    use logs_exec::js_container_logs;

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
