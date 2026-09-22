//! Per-addon identity inside Perry's shared Node-API environment (#10456).
//!
//! Node gives every loaded module its own `napi_env`, which carries the
//! module's declared Node-API version and file URL. Perry keeps one
//! environment per agent, so the two per-module facts Node-API makes
//! observable are attributed instead: each host-initiated entry into addon
//! code that receives an environment (the initializer, native function
//! callbacks, async-work completions, TSFN callbacks and finalizers) runs with
//! the module that created it marked active.

use super::*;
use std::ffi::{c_char, CString};
use std::path::Path;

/// `NODE_API_DEFAULT_MODULE_API_VERSION`: what Node assumes for an addon that
/// declares no version, or any version below it.
pub(crate) const NAPI_DEFAULT_MODULE_VERSION: i32 = 8;
/// `NAPI_VERSION_EXPERIMENTAL`, declared by addons built with
/// `NAPI_EXPERIMENTAL`. The experimental entry points are not exported.
pub(crate) const NAPI_VERSION_EXPERIMENTAL: i32 = i32::MAX;

pub(crate) struct ModuleRecord {
    /// `file://` URL of the canonical sidecar path the addon was loaded from.
    /// The `CString` buffer never moves, and records are never removed.
    file_url: CString,
    /// The declared version after Node's clamp to at least version 8.
    api_version: i32,
}

/// Apply Node's module-version rules to a declared
/// `node_api_module_get_api_version_v1()` result (`None` when the addon does
/// not export one).
pub(crate) fn effective_module_version(declared: Option<i32>) -> Result<i32, String> {
    let Some(declared) = declared else {
        return Ok(NAPI_DEFAULT_MODULE_VERSION);
    };
    if declared == NAPI_VERSION_EXPERIMENTAL {
        return Err(format!(
            "addon requests the experimental Node-API surface (NAPI_EXPERIMENTAL), which Perry does not provide; Perry supports versions 1 through {NAPI_VERSION}"
        ));
    }
    if declared < 1 || declared as u32 > NAPI_VERSION {
        return Err(format!(
            "addon requests Node-API version {declared}, but Perry supports versions 1 through {NAPI_VERSION}"
        ));
    }
    Ok(declared.max(NAPI_DEFAULT_MODULE_VERSION))
}

/// Record a module whose initializer is about to run and return the index
/// its callbacks are attributed to.
pub(crate) fn register_module(env: NapiEnv, path: &Path, api_version: i32) -> Option<u32> {
    let text = path.to_string_lossy();
    // A canonical Windows path carries the verbatim prefix, which is not part
    // of the file URL Node reports.
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    let url = crate::url::node_compat::path_to_file_url_string(text, cfg!(windows));
    let file_url = CString::new(url).ok()?;
    with_env_mut(env, |env| {
        let index = u32::try_from(env.modules.len()).ok()?;
        env.modules.push(ModuleRecord {
            file_url,
            api_version,
        });
        Some(index)
    })
    .flatten()
}

/// The module whose code is running on `env`, captured by records that call
/// back into that module later.
pub(crate) fn active_module(env: NapiEnv) -> Option<u32> {
    with_env(env, |env| env.active_module).flatten()
}

/// [`active_module`] for the current agent's environment, for record
/// constructors that do not receive an environment.
pub(crate) fn current_active_module() -> Option<u32> {
    NODE_API_ENV.with(|cell| cell.try_borrow().ok()?.as_deref()?.active_module)
}

/// Run addon code attributed to `module`, restoring the previous attribution
/// afterwards. `None` (a record created outside any addon) keeps the current
/// attribution.
pub(crate) fn with_active_module<R>(env: NapiEnv, module: Option<u32>, f: impl FnOnce() -> R) -> R {
    let Some(module) = module else {
        return f();
    };
    let previous = with_env_mut(env, |env| env.active_module.replace(module)).flatten();
    let result = f();
    with_env_mut(env, |env| env.active_module = previous);
    result
}

/// Deliver an exception an addon left pending from a callback with no
/// JavaScript caller to return it to (`node_napi_env__::CallbackIntoModule`).
///
/// Node reports it as an uncaught exception. For the TSFN callbacks, which
/// Node calls with `enforceUncaughtExceptionPolicy = false`, a module that
/// declares a version below 10 instead gets the DEP0168 warning and the
/// exception is dropped. Once the environment is shutting down it is dropped,
/// as Node does for a terminating environment.
pub(crate) fn settle_callback_exception(env: NapiEnv, module: Option<u32>, enforce_policy: bool) {
    let Some((exception, report)) = with_env_mut(env, |env| {
        let exception = env.pending_exception_bits.take()?;
        let api_version = module
            .and_then(|index| env.modules.get(index as usize))
            .map_or(NAPI_VERSION as i32, |record| record.api_version);
        let report = if env.shutting_down {
            None
        } else {
            Some(enforce_policy || api_version >= 10)
        };
        Some((exception, report))
    })
    .flatten() else {
        return;
    };
    match report {
        Some(true) => report_uncaught_exception(f64::from_bits(exception)),
        Some(false) => emit_uncaught_callback_deprecation(),
        None => {}
    }
}

fn report_uncaught_exception(mut error: f64) {
    // No generated frame is on the stack here, so trap the delivery: an
    // exception thrown by an 'uncaughtException' listener is itself uncaught
    // and is delivered again, as the timer and watcher pumps do.
    loop {
        match crate::exception::js_call_catching(|| {
            crate::os::emit_process_uncaught_exception(error);
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }) {
            Ok(_) => return,
            Err(thrown) => error = thrown,
        }
    }
}

fn emit_uncaught_callback_deprecation() {
    fn string(text: &str) -> f64 {
        let ptr = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
        f64::from_bits(crate::value::JSValue::string_ptr(ptr).bits())
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let message = scope.root_nanbox_f64(string(
        "Uncaught Node-API callback exception detected, please run node with option --force-node-api-uncaught-exceptions-policy=true to handle those exceptions properly.",
    ));
    let kind = scope.root_nanbox_f64(string("DeprecationWarning"));
    let code = string("DEP0168");
    crate::process::js_process_emit_warning(message.get_nanbox_f64(), kind.get_nanbox_f64(), code);
}

#[no_mangle]
pub unsafe extern "C" fn node_api_get_module_file_name(
    env: NapiEnv,
    result: *mut *const c_char,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    // Code outside every attributed entry point (an environment cleanup hook,
    // which is not handed an environment) sees the most recently loaded
    // module.
    let Some(url) = with_env(env, |env| {
        let index = env
            .active_module
            .map(|index| index as usize)
            .or_else(|| env.modules.len().checked_sub(1));
        index
            .and_then(|index| env.modules.get(index))
            .map_or(c"".as_ptr(), |record| record.file_url.as_ptr())
    }) else {
        return NapiStatus::InvalidArg;
    };
    *result = url;
    ok(env)
}
