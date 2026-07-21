//! opendir / glob / watch / watchFile / unwatchFile.

// `PathBuf` is only named by the regex-engine-gated glob helpers
// (`pathbuf_to_slashes`); the cp path at the bottom uses the fully-qualified
// `std::path::PathBuf`, so the bare import is gated to avoid an unused-import
// warning when the engine is off.

use super::*;

mod glob;
mod opendir;
mod watch;

// Re-export the opendir entry points consumed cross-module (callbacks.rs,
// node_submodules/fs_promises.rs) plus the unmangled FFI sync symbol.
pub use opendir::js_fs_opendir_sync;
pub(crate) use opendir::js_fs_opendir_value_with_path;

// Re-export the glob machinery: the `#[no_mangle]` FFI sync symbols are `pub`,
// while the run/value helpers and match types are consumed by the `watch`
// sibling (glob iterator) and stay `pub(crate)`.
pub(crate) use glob::{glob_entry_value, run_fs_glob_result, FsGlobMatch};
pub use glob::{js_fs_glob_sync, js_fs_glob_sync_options};

// Re-export the watch/watchFile entry points + GC scanner + the shared promise
// helpers used across the `fs` module (fd_ops.rs, filehandle.rs, etc.).
pub(crate) use watch::{
    js_fs_promises_glob_iterator, promise_rejected_fs, promise_undefined_fs, promise_value_fs,
    scan_fs_watcher_roots_mut,
};
pub use watch::{js_fs_promises_watch, js_fs_unwatch_file, js_fs_watch, js_fs_watch_file};

// ---------------------------------------------------------------------------
// Shared helpers used by more than one sibling module. Kept in the trunk and
// marked `pub(crate)` so each sibling reaches them via `use super::*;`.
// ---------------------------------------------------------------------------

pub(crate) fn normalize_slashes(path: &str) -> String {
    path.replace('\\', "/")
}

pub(crate) fn undefined_value() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(crate) fn bool_value(value: bool) -> f64 {
    f64::from_bits(crate::value::JSValue::bool(value).bits())
}

pub(crate) fn boxed_ptr(ptr: *const u8) -> f64 {
    f64::from_bits(crate::value::JSValue::pointer(ptr).bits())
}

pub(crate) fn string_value(bytes: &[u8]) -> f64 {
    let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    f64::from_bits(crate::value::JSValue::string_ptr(ptr).bits())
}

pub(crate) fn is_nullish(value: f64) -> bool {
    let js = crate::value::JSValue::from_bits(value.to_bits());
    js.is_undefined() || js.is_null()
}

pub(crate) fn is_callable(value: f64) -> bool {
    !extract_closure_ptr(value).is_null()
}
