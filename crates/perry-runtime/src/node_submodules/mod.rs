//! Issue #841 — wire up named exports + namespace imports for five
//! Node.js submodules that Perry's manifest had registered but whose
//! FFI export tables defaulted to a `TAG_TRUE` sentinel cell:
//!
//!   - `node:timers/promises` (setTimeout / setImmediate / setInterval / scheduler.*)
//!   - `node:readline/promises` (createInterface, Interface, Readline)
//!   - `node:stream/promises` (pipeline, finished)
//!   - `node:stream/consumers` (text, json, buffer, arrayBuffer, bytes, blob)
//!   - `node:sys` (deprecated alias for node:util — re-exports format, inspect, etc.)
//!
//! Pre-fix `import { setTimeout } from "node:timers/promises"; typeof setTimeout`
//! reported `"boolean"` (the value was literally `true`) and `import * as ns
//! from "node:..."` errored at compile time with the "switch to named imports"
//! diagnostic. This module ships per-export function singletons whose `typeof`
//! is `"function"`, plus per-submodule namespace stubs whose properties point
//! at the same singletons.
//!
//! The thunks are deliberately minimal — they throw `Error("<api> is not yet
//! implemented in Perry")` when invoked. Full functional implementations of
//! these APIs are tracked separately under the #793 Node compatibility
//! roadmap. The fix here is strictly about restoring the import surface so
//! consuming code can at least introspect the bindings (typeof checks,
//! `=== util.format` comparisons, dynamic-shape introspection) without
//! tripping over `true`-as-a-function downstream errors.

use std::cell::RefCell;
use std::collections::HashMap;
use std::os::raw::c_int;
use std::sync::atomic::{AtomicI64, Ordering};

use crate::closure::{
    js_closure_alloc, js_closure_call0, js_closure_call1, js_closure_call2, js_closure_call_array,
    js_closure_get_capture_ptr, js_closure_set_capture_ptr, js_register_closure_arity,
    ClosureHeader,
};
use crate::object::{
    js_object_alloc, js_object_get_field_by_name_f64, js_object_set_field_by_name, ObjectHeader,
};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;

mod diagnostics;
pub use diagnostics::*;

/// One entry per named export of one submodule.
struct ExportSpec {
    name: &'static str,
    thunk: ExportThunk,
}

enum ExportThunk {
    Fn1(extern "C" fn(*const ClosureHeader, f64) -> f64),
    Fn2(extern "C" fn(*const ClosureHeader, f64, f64) -> f64),
    Fn3(extern "C" fn(*const ClosureHeader, f64, f64, f64) -> f64),
}

impl ExportThunk {
    fn as_ptr(&self) -> *const u8 {
        match self {
            ExportThunk::Fn1(f) => *f as *const u8,
            ExportThunk::Fn2(f) => *f as *const u8,
            ExportThunk::Fn3(f) => *f as *const u8,
        }
    }
    fn arity(&self) -> u32 {
        match self {
            ExportThunk::Fn1(_) => 1,
            ExportThunk::Fn2(_) => 2,
            ExportThunk::Fn3(_) => 3,
        }
    }
}

/// One entry per submodule. `exports` lists every named export the
/// codegen / parity tests reach for; the codegen's lookup is keyed by
/// `(submodule_key, export_name)` and falls back to `TAG_TRUE` if no
/// matching entry is found (preserving the pre-#841 behavior for any
/// future export Perry doesn't yet know about).
struct SubmoduleSpec {
    /// Stable key — matches the prefix used in the generated FFI symbol
    /// names (`js_node_submod_<key>_export_<name>`).
    key: &'static str,
    exports: &'static [ExportSpec],
}

// ----- thunks -----
//
// One thunk per (submodule, export). All thunks share the same shape:
// they raise an explicit `Error` describing what's missing. Closure
// dispatch invokes them via `js_closure_call0` / `js_closure_call1`
// regardless of declared arity, so a single `(_closure, _arg) -> f64`
// signature is sufficient — Perry's closure ABI tolerates an arg shape
// mismatch on the receiving side (the value is just ignored).

macro_rules! thunk {
    ($name:ident, $msg:expr) => {
        extern "C" fn $name(_closure: *const ClosureHeader, _arg: f64) -> f64 {
            let msg: &'static str = $msg;
            let bytes = msg.as_bytes();
            let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            let err = crate::error::js_error_new_with_message(header);
            let bits = JSValue::pointer(err as *const u8).bits();
            crate::exception::js_throw(f64::from_bits(bits))
        }
    };
}

/// node:timers/promises.setTimeout(delay, value?) — a Promise that resolves
/// with `value` (or undefined) after `delay` ms. Composes the existing
/// promise-returning timer primitive; the closure dispatch pads a missing
/// `value` arg with undefined (arity registered in `ensure_export_singleton`).
/// Refs #1213.
extern "C" fn timers_promises_set_timeout(
    _closure: *const ClosureHeader,
    delay_ms: f64,
    value: f64,
) -> f64 {
    let promise = crate::timer::js_set_timeout_value(delay_ms, value);
    crate::value::js_nanbox_pointer(promise as i64)
}

/// node:timers/promises.setImmediate(value?) — a Promise that resolves with
/// `value` (or undefined) on a later turn. Refs #1213.
extern "C" fn timers_promises_set_immediate(_closure: *const ClosureHeader, value: f64) -> f64 {
    let promise = crate::timer::js_set_timeout_value(0.0, value);
    crate::value::js_nanbox_pointer(promise as i64)
}

// ── node:timers namespace (`import * as timers from "node:timers"`) ──────────
// Route to the SAME global timer runtime fns the bare globals use, so
// `timers.setTimeout(...)` matches `setTimeout(...)`. NOTE: named imports
// (`import { setTimeout } from "node:timers"`) deliberately bypass this and
// keep the codegen global fast-path (which handles `setTimeout(fn, delay,
// ...args)` varargs) — compile.rs skips registering node:timers named imports
// as submodule exports. Refs #1213.
fn callback_arg_to_i64(v: f64) -> i64 {
    (v.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64
}
extern "C" fn timers_ns_set_timeout(_c: *const ClosureHeader, cb: f64, ms: f64) -> f64 {
    crate::value::js_nanbox_pointer(crate::timer::js_set_timeout_callback(
        callback_arg_to_i64(cb),
        ms,
    ))
}
extern "C" fn timers_ns_set_interval(_c: *const ClosureHeader, cb: f64, ms: f64) -> f64 {
    crate::value::js_nanbox_pointer(crate::timer::setInterval(callback_arg_to_i64(cb), ms))
}
extern "C" fn timers_ns_set_immediate(_c: *const ClosureHeader, cb: f64) -> f64 {
    crate::value::js_nanbox_pointer(crate::timer::js_set_immediate_callback(
        callback_arg_to_i64(cb),
    ))
}
extern "C" fn timers_ns_clear_timeout(_c: *const ClosureHeader, arg: f64) -> f64 {
    crate::timer::js_clear_timeout_value(arg);
    f64::from_bits(TAG_UNDEFINED)
}
extern "C" fn timers_ns_clear_interval(_c: *const ClosureHeader, arg: f64) -> f64 {
    crate::timer::js_clear_interval_value(arg);
    f64::from_bits(TAG_UNDEFINED)
}
// Immediates live in the shared timer pool; clearTimeout retains-out both pools.
extern "C" fn timers_ns_clear_immediate(_c: *const ClosureHeader, arg: f64) -> f64 {
    crate::timer::js_clear_timeout_value(arg);
    f64::from_bits(TAG_UNDEFINED)
}

thunk!(
    thunk_timers_setInterval,
    "node:timers/promises.setInterval is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_timers_scheduler,
    "node:timers/promises.scheduler is not yet implemented in Perry (tracked by issue #793)."
);

fn promise_value(value: f64) -> f64 {
    let promise = crate::promise::js_promise_resolved(value);
    f64::from_bits(JSValue::pointer(promise as *const u8).bits())
}

fn promise_undefined() -> f64 {
    promise_value(f64::from_bits(crate::value::TAG_UNDEFINED))
}

extern "C" fn thunk_fs_promises_readFile(
    _closure: *const ClosureHeader,
    path: f64,
    encoding: f64,
) -> f64 {
    promise_value(crate::fs::js_fs_read_file_dispatch(path, encoding))
}

extern "C" fn thunk_fs_promises_open(
    _closure: *const ClosureHeader,
    path: f64,
    flags: f64,
    _mode: f64,
) -> f64 {
    // Probe before opening so a missing path rejects the Promise instead of
    // resolving with a FileHandle whose `fd === -1`. Matches Node's behavior
    // for `fs/promises.open(path)` on ENOENT/EACCES.
    if let Some(err_val) = unsafe { crate::fs::fs_promises_open_probe_error(path, flags) } {
        let promise = crate::promise::js_promise_rejected(err_val);
        return f64::from_bits(JSValue::pointer(promise as *const u8).bits());
    }
    promise_value(crate::fs::js_fs_filehandle_open(path, flags))
}

extern "C" fn thunk_fs_promises_writeFile(
    _closure: *const ClosureHeader,
    path: f64,
    data: f64,
    options: f64,
) -> f64 {
    let _ = crate::fs::js_fs_write_file_sync_options(path, data, options);
    promise_undefined()
}

extern "C" fn thunk_fs_promises_appendFile(
    _closure: *const ClosureHeader,
    path: f64,
    data: f64,
    options: f64,
) -> f64 {
    let _ = crate::fs::js_fs_append_file_sync_options(path, data, options);
    promise_undefined()
}

extern "C" fn thunk_fs_promises_chmod(_closure: *const ClosureHeader, path: f64, mode: f64) -> f64 {
    let _ = crate::fs::js_fs_chmod_sync(path, mode);
    promise_undefined()
}

extern "C" fn thunk_fs_promises_chown(
    _closure: *const ClosureHeader,
    path: f64,
    uid: f64,
    gid: f64,
) -> f64 {
    let _ = crate::fs::js_fs_chown_sync(path, uid, gid);
    promise_undefined()
}

extern "C" fn thunk_fs_promises_lchown(
    _closure: *const ClosureHeader,
    path: f64,
    uid: f64,
    gid: f64,
) -> f64 {
    let _ = crate::fs::js_fs_lchown_sync(path, uid, gid);
    promise_undefined()
}

extern "C" fn thunk_fs_promises_mkdir(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    let _ = crate::fs::js_fs_mkdir_sync_options(path, options);
    promise_undefined()
}

extern "C" fn thunk_fs_promises_readdir(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    let raw = crate::fs::js_fs_readdir_sync(path, options);
    promise_value(f64::from_bits(
        JSValue::pointer(raw.to_bits() as *const u8).bits(),
    ))
}

extern "C" fn thunk_fs_promises_stat(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    promise_value(crate::fs::js_fs_stat_sync_options(path, options))
}

extern "C" fn thunk_fs_promises_statfs(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    promise_value(crate::fs::js_fs_statfs_sync_options(path, options))
}

extern "C" fn thunk_fs_promises_lstat(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    promise_value(crate::fs::js_fs_lstat_sync_options(path, options))
}

extern "C" fn thunk_fs_promises_rm(_closure: *const ClosureHeader, path: f64, options: f64) -> f64 {
    let _ = crate::fs::js_fs_rm_recursive_options(path, options);
    promise_undefined()
}

extern "C" fn thunk_fs_promises_rmdir(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    let _ = crate::fs::js_fs_rmdir_sync_options(path, options);
    promise_undefined()
}

extern "C" fn thunk_fs_promises_unlink(_closure: *const ClosureHeader, path: f64) -> f64 {
    let _ = crate::fs::js_fs_unlink_sync(path);
    promise_undefined()
}

extern "C" fn thunk_fs_promises_rename(_closure: *const ClosureHeader, from: f64, to: f64) -> f64 {
    let _ = crate::fs::js_fs_rename_sync(from, to);
    promise_undefined()
}

extern "C" fn thunk_fs_promises_copyFile(
    _closure: *const ClosureHeader,
    from: f64,
    to: f64,
    flags: f64,
) -> f64 {
    let _ = crate::fs::js_fs_copy_file_sync_flags(from, to, flags);
    promise_undefined()
}

extern "C" fn thunk_fs_promises_cp(
    _closure: *const ClosureHeader,
    from: f64,
    to: f64,
    options: f64,
) -> f64 {
    let _ = crate::fs::js_fs_cp_sync_options(from, to, options);
    promise_undefined()
}

extern "C" fn thunk_fs_promises_truncate(
    _closure: *const ClosureHeader,
    path: f64,
    len: f64,
) -> f64 {
    let _ = crate::fs::js_fs_truncate_sync(path, len);
    promise_undefined()
}

extern "C" fn thunk_fs_promises_utimes(
    _closure: *const ClosureHeader,
    path: f64,
    atime: f64,
    mtime: f64,
) -> f64 {
    let _ = crate::fs::js_fs_utimes_sync(path, atime, mtime);
    promise_undefined()
}

extern "C" fn thunk_fs_promises_lutimes(
    _closure: *const ClosureHeader,
    path: f64,
    atime: f64,
    mtime: f64,
) -> f64 {
    let _ = crate::fs::js_fs_lutimes_sync(path, atime, mtime);
    promise_undefined()
}

extern "C" fn thunk_fs_promises_link(_closure: *const ClosureHeader, from: f64, to: f64) -> f64 {
    let _ = crate::fs::js_fs_link_sync(from, to);
    promise_undefined()
}

extern "C" fn thunk_fs_promises_symlink(
    _closure: *const ClosureHeader,
    target: f64,
    path: f64,
    _type: f64,
) -> f64 {
    let _ = crate::fs::js_fs_symlink_sync(target, path);
    promise_undefined()
}

extern "C" fn thunk_fs_promises_readlink(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    promise_value(crate::fs::js_fs_readlink_dispatch(path, options))
}

extern "C" fn thunk_fs_promises_realpath(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    promise_value(crate::fs::js_fs_realpath_dispatch(path, options))
}

extern "C" fn thunk_fs_promises_mkdtemp(
    _closure: *const ClosureHeader,
    prefix: f64,
    options: f64,
) -> f64 {
    promise_value(crate::fs::js_fs_mkdtemp_dispatch(prefix, options))
}

extern "C" fn thunk_fs_promises_opendir(_closure: *const ClosureHeader, path: f64) -> f64 {
    promise_value(crate::fs::js_fs_opendir_sync(path))
}

extern "C" fn thunk_fs_promises_glob(
    _closure: *const ClosureHeader,
    pattern: f64,
    options: f64,
) -> f64 {
    let raw = crate::fs::js_fs_glob_sync_options(pattern, options);
    promise_value(f64::from_bits(
        JSValue::pointer(raw.to_bits() as *const u8).bits(),
    ))
}

extern "C" fn thunk_fs_promises_watch(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    crate::fs::js_fs_watch(path, options, f64::from_bits(crate::value::TAG_UNDEFINED))
}

extern "C" fn thunk_fs_promises_access(
    _closure: *const ClosureHeader,
    path: f64,
    mode: f64,
) -> f64 {
    let _ = crate::fs::js_fs_access_sync_mode(path, mode);
    promise_undefined()
}

thunk!(thunk_readline_createInterface, "node:readline/promises.createInterface is not yet implemented in Perry (tracked by issue #793).");
thunk!(
    thunk_readline_Interface,
    "node:readline/promises.Interface is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_readline_Readline,
    "node:readline/promises.Readline is not yet implemented in Perry (tracked by issue #793)."
);

thunk!(
    thunk_streamP_pipeline,
    "node:stream/promises.pipeline is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_streamP_finished,
    "node:stream/promises.finished is not yet implemented in Perry (tracked by issue #793)."
);

thunk!(
    thunk_consumers_text,
    "node:stream/consumers.text is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_consumers_json,
    "node:stream/consumers.json is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_consumers_buffer,
    "node:stream/consumers.buffer is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_consumers_arrayBuffer,
    "node:stream/consumers.arrayBuffer is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_consumers_bytes,
    "node:stream/consumers.bytes is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_consumers_blob,
    "node:stream/consumers.blob is not yet implemented in Perry (tracked by issue #793)."
);

// node:sys is a deprecated alias for node:util — point each export at
// the same thunks until util's named-export surface is wired up. The
// parity test compares `sys.format === util.format` for identity; for
// now both report `typeof === "function"` (passing the typeof gate) but
// the strict-equality check still diverges. That divergence is
// pre-existing (node:util's named exports lower to NativeModuleRef =>
// `typeof === "object"` today) — it's the parent-module half of #793.
thunk!(thunk_sys_format, "node:sys.format is not yet implemented in Perry (use node:util.format; node:sys is deprecated).");
thunk!(thunk_sys_inspect, "node:sys.inspect is not yet implemented in Perry (use node:util.inspect; node:sys is deprecated).");
thunk!(thunk_sys_debuglog, "node:sys.debuglog is not yet implemented in Perry (use node:util.debuglog; node:sys is deprecated).");
thunk!(thunk_sys_deprecate, "node:sys.deprecate is not yet implemented in Perry (use node:util.deprecate; node:sys is deprecated).");
thunk!(thunk_sys_promisify, "node:sys.promisify is not yet implemented in Perry (use node:util.promisify; node:sys is deprecated).");
thunk!(thunk_sys_callbackify, "node:sys.callbackify is not yet implemented in Perry (use node:util.callbackify; node:sys is deprecated).");
thunk!(thunk_sys_isArray, "node:sys.isArray is not yet implemented in Perry (use node:util.isArray; node:sys is deprecated).");

// ----- submodule table -----

const SUBMODULES: &[SubmoduleSpec] = &[
    SubmoduleSpec {
        // node:timers namespace object (`import * as timers`). Named imports
        // bypass this (compile.rs) to keep the global fast-path. (#1213)
        key: "timers",
        exports: &[
            ExportSpec {
                name: "setTimeout",
                thunk: ExportThunk::Fn2(timers_ns_set_timeout),
            },
            ExportSpec {
                name: "setInterval",
                thunk: ExportThunk::Fn2(timers_ns_set_interval),
            },
            ExportSpec {
                name: "setImmediate",
                thunk: ExportThunk::Fn1(timers_ns_set_immediate),
            },
            ExportSpec {
                name: "clearTimeout",
                thunk: ExportThunk::Fn1(timers_ns_clear_timeout),
            },
            ExportSpec {
                name: "clearInterval",
                thunk: ExportThunk::Fn1(timers_ns_clear_interval),
            },
            ExportSpec {
                name: "clearImmediate",
                thunk: ExportThunk::Fn1(timers_ns_clear_immediate),
            },
        ],
    },
    SubmoduleSpec {
        key: "timers_promises",
        exports: &[
            ExportSpec {
                name: "setTimeout",
                thunk: ExportThunk::Fn2(timers_promises_set_timeout),
            },
            ExportSpec {
                name: "setImmediate",
                thunk: ExportThunk::Fn1(timers_promises_set_immediate),
            },
            ExportSpec {
                name: "setInterval",
                thunk: ExportThunk::Fn1(thunk_timers_setInterval),
            },
            ExportSpec {
                name: "scheduler",
                thunk: ExportThunk::Fn1(thunk_timers_scheduler),
            },
        ],
    },
    SubmoduleSpec {
        key: "fs_promises",
        exports: &[
            ExportSpec {
                name: "readFile",
                thunk: ExportThunk::Fn2(thunk_fs_promises_readFile),
            },
            ExportSpec {
                name: "open",
                thunk: ExportThunk::Fn3(thunk_fs_promises_open),
            },
            ExportSpec {
                name: "writeFile",
                thunk: ExportThunk::Fn3(thunk_fs_promises_writeFile),
            },
            ExportSpec {
                name: "appendFile",
                thunk: ExportThunk::Fn3(thunk_fs_promises_appendFile),
            },
            ExportSpec {
                name: "chmod",
                thunk: ExportThunk::Fn2(thunk_fs_promises_chmod),
            },
            ExportSpec {
                name: "chown",
                thunk: ExportThunk::Fn3(thunk_fs_promises_chown),
            },
            ExportSpec {
                name: "lchown",
                thunk: ExportThunk::Fn3(thunk_fs_promises_lchown),
            },
            ExportSpec {
                name: "mkdir",
                thunk: ExportThunk::Fn2(thunk_fs_promises_mkdir),
            },
            ExportSpec {
                name: "readdir",
                thunk: ExportThunk::Fn2(thunk_fs_promises_readdir),
            },
            ExportSpec {
                name: "stat",
                thunk: ExportThunk::Fn2(thunk_fs_promises_stat),
            },
            ExportSpec {
                name: "statfs",
                thunk: ExportThunk::Fn2(thunk_fs_promises_statfs),
            },
            ExportSpec {
                name: "lstat",
                thunk: ExportThunk::Fn2(thunk_fs_promises_lstat),
            },
            ExportSpec {
                name: "rm",
                thunk: ExportThunk::Fn2(thunk_fs_promises_rm),
            },
            ExportSpec {
                name: "rmdir",
                thunk: ExportThunk::Fn2(thunk_fs_promises_rmdir),
            },
            ExportSpec {
                name: "unlink",
                thunk: ExportThunk::Fn1(thunk_fs_promises_unlink),
            },
            ExportSpec {
                name: "rename",
                thunk: ExportThunk::Fn2(thunk_fs_promises_rename),
            },
            ExportSpec {
                name: "copyFile",
                thunk: ExportThunk::Fn3(thunk_fs_promises_copyFile),
            },
            ExportSpec {
                name: "cp",
                thunk: ExportThunk::Fn3(thunk_fs_promises_cp),
            },
            ExportSpec {
                name: "truncate",
                thunk: ExportThunk::Fn2(thunk_fs_promises_truncate),
            },
            ExportSpec {
                name: "utimes",
                thunk: ExportThunk::Fn3(thunk_fs_promises_utimes),
            },
            ExportSpec {
                name: "lutimes",
                thunk: ExportThunk::Fn3(thunk_fs_promises_lutimes),
            },
            ExportSpec {
                name: "link",
                thunk: ExportThunk::Fn2(thunk_fs_promises_link),
            },
            ExportSpec {
                name: "symlink",
                thunk: ExportThunk::Fn3(thunk_fs_promises_symlink),
            },
            ExportSpec {
                name: "readlink",
                thunk: ExportThunk::Fn2(thunk_fs_promises_readlink),
            },
            ExportSpec {
                name: "realpath",
                thunk: ExportThunk::Fn2(thunk_fs_promises_realpath),
            },
            ExportSpec {
                name: "mkdtemp",
                thunk: ExportThunk::Fn2(thunk_fs_promises_mkdtemp),
            },
            ExportSpec {
                name: "opendir",
                thunk: ExportThunk::Fn1(thunk_fs_promises_opendir),
            },
            ExportSpec {
                name: "glob",
                thunk: ExportThunk::Fn2(thunk_fs_promises_glob),
            },
            ExportSpec {
                name: "watch",
                thunk: ExportThunk::Fn2(thunk_fs_promises_watch),
            },
            ExportSpec {
                name: "access",
                thunk: ExportThunk::Fn2(thunk_fs_promises_access),
            },
        ],
    },
    SubmoduleSpec {
        key: "readline_promises",
        exports: &[
            ExportSpec {
                name: "createInterface",
                thunk: ExportThunk::Fn1(thunk_readline_createInterface),
            },
            ExportSpec {
                name: "Interface",
                thunk: ExportThunk::Fn1(thunk_readline_Interface),
            },
            ExportSpec {
                name: "Readline",
                thunk: ExportThunk::Fn1(thunk_readline_Readline),
            },
        ],
    },
    SubmoduleSpec {
        key: "stream_promises",
        exports: &[
            ExportSpec {
                name: "pipeline",
                thunk: ExportThunk::Fn1(thunk_streamP_pipeline),
            },
            ExportSpec {
                name: "finished",
                thunk: ExportThunk::Fn1(thunk_streamP_finished),
            },
        ],
    },
    SubmoduleSpec {
        key: "stream_consumers",
        exports: &[
            ExportSpec {
                name: "text",
                thunk: ExportThunk::Fn1(thunk_consumers_text),
            },
            ExportSpec {
                name: "json",
                thunk: ExportThunk::Fn1(thunk_consumers_json),
            },
            ExportSpec {
                name: "buffer",
                thunk: ExportThunk::Fn1(thunk_consumers_buffer),
            },
            ExportSpec {
                name: "arrayBuffer",
                thunk: ExportThunk::Fn1(thunk_consumers_arrayBuffer),
            },
            ExportSpec {
                name: "bytes",
                thunk: ExportThunk::Fn1(thunk_consumers_bytes),
            },
            ExportSpec {
                name: "blob",
                thunk: ExportThunk::Fn1(thunk_consumers_blob),
            },
        ],
    },
    SubmoduleSpec {
        key: "sys",
        exports: &[
            ExportSpec {
                name: "format",
                thunk: ExportThunk::Fn1(thunk_sys_format),
            },
            ExportSpec {
                name: "inspect",
                thunk: ExportThunk::Fn1(thunk_sys_inspect),
            },
            ExportSpec {
                name: "debuglog",
                thunk: ExportThunk::Fn1(thunk_sys_debuglog),
            },
            ExportSpec {
                name: "deprecate",
                thunk: ExportThunk::Fn1(thunk_sys_deprecate),
            },
            ExportSpec {
                name: "promisify",
                thunk: ExportThunk::Fn1(thunk_sys_promisify),
            },
            ExportSpec {
                name: "callbackify",
                thunk: ExportThunk::Fn1(thunk_sys_callbackify),
            },
            ExportSpec {
                name: "isArray",
                thunk: ExportThunk::Fn1(thunk_sys_isArray),
            },
        ],
    },
    // #906 follow-up: pino reads `tracingChannel('pino_asJson')` at
    // module init time. The thunks here return useful stub values
    // (an object with `hasSubscribers: false`) instead of throwing,
    // so pino's "no subscribers → fast path" branch is taken and the
    // tracing machinery never enters.
    SubmoduleSpec {
        key: "diagnostics_channel",
        exports: &[
            ExportSpec {
                name: "tracingChannel",
                thunk: ExportThunk::Fn1(thunk_diag_tracing_channel),
            },
            ExportSpec {
                name: "channel",
                thunk: ExportThunk::Fn1(thunk_diag_channel),
            },
            ExportSpec {
                name: "subscribe",
                thunk: ExportThunk::Fn2(thunk_diag_subscribe),
            },
            ExportSpec {
                name: "unsubscribe",
                thunk: ExportThunk::Fn2(thunk_diag_unsubscribe),
            },
            ExportSpec {
                name: "publish",
                thunk: ExportThunk::Fn1(thunk_diag_noop),
            },
            ExportSpec {
                name: "hasSubscribers",
                thunk: ExportThunk::Fn1(thunk_diag_has_subscribers),
            },
            ExportSpec {
                name: "Channel",
                thunk: ExportThunk::Fn1(thunk_diag_noop),
            },
        ],
    },
];

fn find_submodule(key: &str) -> Option<&'static SubmoduleSpec> {
    SUBMODULES.iter().find(|s| s.key == key)
}

fn find_export(submod: &SubmoduleSpec, name: &str) -> Option<&'static ExportSpec> {
    submod.exports.iter().find(|e| e.name == name)
}

// ----- singleton storage -----
//
// One AtomicI64 slot per thunk so concurrent first-use callers don't
// leak a closure. Stored in a thread_local Vec for simplicity — these
// singletons are allocated on first reach and live until process exit
// (they're root-marked by `scan_node_submodule_singleton_roots` below).

thread_local! {
    /// Map from (submod_key_ptr, export_name_ptr) — both `&'static str`,
    /// so pointer-equality is sufficient — to the cached singleton
    /// ClosureHeader pointer for that export's thunk.
    static EXPORT_SINGLETONS: RefCell<std::collections::HashMap<(usize, usize), *mut ClosureHeader>> =
        RefCell::new(std::collections::HashMap::new());

    /// Map from submod_key_ptr to the cached namespace ObjectHeader
    /// pointer — populated once per submodule on first namespace use.
    static NAMESPACE_SINGLETONS: RefCell<std::collections::HashMap<usize, *mut ObjectHeader>> =
        RefCell::new(std::collections::HashMap::new());
}

// We also need a process-wide "any singleton allocated?" flag so the
// GC scanner can early-out without taking the thread_local borrow on
// every cycle. Using `AtomicI64` instead of `AtomicBool` so the scanner
// can also use it as a release fence against the thread_local writes.
static ANY_SINGLETON_ALLOCATED: AtomicI64 = AtomicI64::new(0);

fn ensure_export_singleton(
    submod: &'static SubmoduleSpec,
    export: &'static ExportSpec,
) -> *mut ClosureHeader {
    let key = (submod.key.as_ptr() as usize, export.name.as_ptr() as usize);
    if let Some(cached) = EXPORT_SINGLETONS.with(|m| m.borrow().get(&key).copied()) {
        return cached;
    }
    let thunk_ptr = export.thunk.as_ptr();
    let allocated = js_closure_alloc(thunk_ptr, 0);
    // Arity is encoded in the ExportThunk variant, so the closure dispatch
    // pads missing args with undefined for variadic-friendly thunks. This
    // replaces the per-submodule arity tables in earlier revisions.
    crate::closure::js_register_closure_arity(thunk_ptr, export.thunk.arity());
    EXPORT_SINGLETONS.with(|m| {
        m.borrow_mut().insert(key, allocated);
    });
    ANY_SINGLETON_ALLOCATED.store(1, Ordering::Release);
    allocated
}

fn ensure_namespace_singleton(submod: &'static SubmoduleSpec) -> *mut ObjectHeader {
    let key = submod.key.as_ptr() as usize;
    if let Some(cached) = NAMESPACE_SINGLETONS.with(|m| m.borrow().get(&key).copied()) {
        return cached;
    }
    // Allocate a fresh object with one inline slot per known export;
    // the dynamic-property path in `js_object_set_field_by_name` will
    // grow it if needed.
    let field_count = submod.exports.len() as u32;
    let obj = js_object_alloc(0, field_count);
    // Populate fields. Each export's value is the singleton closure
    // pointer NaN-boxed as POINTER. We route through
    // `js_object_set_field_by_name` so the keys array gets built up
    // identically to what user code's literal object init would
    // produce — that's what `js_object_keys` / spread / Reflect.ownKeys
    // walks at runtime.
    for spec in submod.exports {
        let closure_ptr = ensure_export_singleton(submod, spec);
        let value_bits = JSValue::pointer(closure_ptr as *const u8).bits();
        let value_f64 = f64::from_bits(value_bits);
        unsafe {
            let name_bytes = spec.name.as_bytes();
            let name_header = js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
            crate::object::js_object_set_field_by_name(obj, name_header, value_f64);
        }
    }
    NAMESPACE_SINGLETONS.with(|m| {
        m.borrow_mut().insert(key, obj);
    });
    ANY_SINGLETON_ALLOCATED.store(1, Ordering::Release);
    obj
}

/// GC root scanner: pin every (export-singleton, namespace-singleton)
/// allocated by this module against the next sweep. Wired up from
/// `gc::gc_init`.
pub fn scan_node_submodule_singleton_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_node_submodule_singleton_roots_mut(&mut visitor);
}

pub fn scan_node_submodule_singleton_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    if ANY_SINGLETON_ALLOCATED.load(Ordering::Acquire) == 0 {
        return;
    }
    EXPORT_SINGLETONS.with(|m| {
        for closure_ptr in m.borrow_mut().values_mut() {
            visitor.visit_raw_mut_ptr_slot(closure_ptr);
        }
    });
    NAMESPACE_SINGLETONS.with(|m| {
        for obj_ptr in m.borrow_mut().values_mut() {
            visitor.visit_raw_mut_ptr_slot(obj_ptr);
        }
    });
    // #906 follow-up: the no-op closure shared by every TracingChannel /
    // Channel stub field also needs pinning against the next sweep. The
    // returned stub objects themselves are caller-owned (we don't cache
    // them) so they're traced through normal allocator roots.
    DIAG_NOOP_CLOSURE.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(ptr) = slot.as_mut() {
            visitor.visit_raw_mut_ptr_slot(ptr);
        }
    });
    DIAG_CHANNELS.with(|m| {
        for state in m.borrow_mut().values_mut() {
            visitor.visit_nanbox_f64_slot(&mut state.name);
            visitor.visit_raw_mut_ptr_slot(&mut state.obj);
            for subscriber in &mut state.subscribers {
                visitor.visit_nanbox_f64_slot(subscriber);
            }
            for (store, transform) in &mut state.stores {
                visitor.visit_nanbox_f64_slot(store);
                if let Some(t) = transform.as_mut() {
                    visitor.visit_nanbox_f64_slot(t);
                }
            }
        }
    });
    DIAG_TRACES.with(|m| {
        for trace in m.borrow_mut().values_mut() {
            visitor.visit_raw_mut_ptr_slot(&mut trace.obj);
        }
    });
}

#[cfg(test)]
pub(crate) fn test_seed_node_submodule_roots(
    closure: *mut ClosureHeader,
    namespace: *mut ObjectHeader,
    diag_noop: *mut ClosureHeader,
) {
    EXPORT_SINGLETONS.with(|m| {
        let mut m = m.borrow_mut();
        m.clear();
        m.insert((1, 2), closure);
    });
    NAMESPACE_SINGLETONS.with(|m| {
        let mut m = m.borrow_mut();
        m.clear();
        m.insert(3, namespace);
    });
    DIAG_NOOP_CLOSURE.with(|slot| {
        *slot.borrow_mut() = Some(diag_noop);
    });
    ANY_SINGLETON_ALLOCATED.store(1, Ordering::Release);
}

#[cfg(test)]
pub(crate) fn test_node_submodule_roots() -> (usize, usize, usize) {
    let closure = EXPORT_SINGLETONS.with(|m| {
        m.borrow()
            .get(&(1, 2))
            .map(|ptr| *ptr as usize)
            .unwrap_or(0)
    });
    let namespace =
        NAMESPACE_SINGLETONS.with(|m| m.borrow().get(&3).map(|ptr| *ptr as usize).unwrap_or(0));
    let diag =
        DIAG_NOOP_CLOSURE.with(|slot| slot.borrow().as_ref().map(|ptr| *ptr as usize).unwrap_or(0));
    (closure, namespace, diag)
}

// ----- FFI entry points -----
//
// `submod_key_ptr` / `name_ptr` are `*const u8` pointers + lengths
// rather than NUL-terminated strings so codegen can hand off the raw
// bytes from emitted IR (already produced as `private constant
// [N x i8]` arrays via `emit_string_literal`).

/// Returns a NaN-boxed function singleton for the given
/// `(submodule, export)` pair. Falls back to NaN-boxed `TAG_TRUE`
/// (preserving the pre-#841 sentinel) if no matching entry is found —
/// this keeps any not-yet-listed export's behavior unchanged, so
/// later additions to `SUBMODULES` are strictly additive.
///
/// # Safety
///
/// The `submod_key_ptr` / `name_ptr` arguments must point to valid UTF-8
/// byte sequences of the indicated length, and remain alive for the
/// duration of this call.
#[no_mangle]
pub unsafe extern "C" fn js_node_submodule_export_as_function(
    submod_key_ptr: *const u8,
    submod_key_len: u32,
    name_ptr: *const u8,
    name_len: u32,
) -> f64 {
    let submod_bytes = std::slice::from_raw_parts(submod_key_ptr, submod_key_len as usize);
    let name_bytes = std::slice::from_raw_parts(name_ptr, name_len as usize);
    let submod_key = match std::str::from_utf8(submod_bytes) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(JSValue::bool(true).bits()),
    };
    let name = match std::str::from_utf8(name_bytes) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(JSValue::bool(true).bits()),
    };
    let submod = match find_submodule(submod_key) {
        Some(s) => s,
        None => return f64::from_bits(JSValue::bool(true).bits()),
    };
    let export = match find_export(submod, name) {
        Some(e) => e,
        None => return f64::from_bits(JSValue::bool(true).bits()),
    };
    let closure_ptr = ensure_export_singleton(submod, export);
    f64::from_bits(JSValue::pointer(closure_ptr as *const u8).bits())
}

/// Returns a NaN-boxed namespace stub object for the given submodule.
/// Each known named export of that submodule is exposed as an own
/// property on the object whose value is the function singleton
/// produced by `js_node_submodule_export_as_function`. Falls back to
/// `js_unresolved_namespace_stub` (the empty-object stub Perry already
/// hands out for unknown namespace imports) if `submod_key` doesn't
/// match a known submodule.
///
/// # Safety
///
/// Same constraints as `js_node_submodule_export_as_function`.
#[no_mangle]
pub unsafe extern "C" fn js_node_submodule_namespace(
    submod_key_ptr: *const u8,
    submod_key_len: u32,
) -> f64 {
    let submod_bytes = std::slice::from_raw_parts(submod_key_ptr, submod_key_len as usize);
    let submod_key = match std::str::from_utf8(submod_bytes) {
        Ok(s) => s,
        Err(_) => return crate::object::js_unresolved_namespace_stub(),
    };
    let submod = match find_submodule(submod_key) {
        Some(s) => s,
        None => return crate::object::js_unresolved_namespace_stub(),
    };
    let obj = ensure_namespace_singleton(submod);
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_submodules_have_at_least_one_export() {
        for s in SUBMODULES {
            assert!(
                !s.exports.is_empty(),
                "submodule {} has zero exports",
                s.key
            );
        }
    }

    #[test]
    fn find_submodule_for_known_keys() {
        for key in [
            "timers_promises",
            "readline_promises",
            "stream_promises",
            "stream_consumers",
            "sys",
            "diagnostics_channel",
        ] {
            assert!(
                find_submodule(key).is_some(),
                "submodule {} missing from SUBMODULES table",
                key
            );
        }
    }

    #[test]
    fn find_submodule_for_unknown_key_returns_none() {
        assert!(find_submodule("not_a_real_submodule").is_none());
    }

    /// #906 follow-up — pino reads `tracingChannel('pino_asJson').hasSubscribers`
    /// before deciding whether to enter the tracing branch. The stub MUST
    /// expose `tracingChannel` as a callable thunk in the SUBMODULES table
    /// so the namespace singleton's field is a function (not TAG_TRUE).
    #[test]
    fn diagnostics_channel_exposes_tracingChannel_export() {
        let submod = find_submodule("diagnostics_channel")
            .expect("diagnostics_channel must be in SUBMODULES");
        let names: Vec<&str> = submod.exports.iter().map(|e| e.name).collect();
        for required in ["tracingChannel", "channel", "subscribe", "unsubscribe"] {
            assert!(
                names.contains(&required),
                "diagnostics_channel must export `{}` for pino's `require('node:diagnostics_channel')` to keep working",
                required
            );
        }
    }
}
