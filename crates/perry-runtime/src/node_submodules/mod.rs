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
//! Most thunks are deliberately minimal — they throw `Error("<api> is not yet
//! implemented in Perry")` when invoked. `node:stream/consumers` is the first
//! submodule here with concrete behavior, so consuming code can import and use
//! its helpers while the broader #793 Node compatibility roadmap continues.

use std::cell::RefCell;
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
use crate::string::js_string_from_bytes;
use crate::value::JSValue;

pub(crate) mod diagnostics;
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
        pub(crate) extern "C" fn $name(
            _closure: *const crate::closure::ClosureHeader,
            _arg: f64,
        ) -> f64 {
            let msg: &'static str = $msg;
            let bytes = msg.as_bytes();
            let header = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            let err = crate::error::js_error_new_with_message(header);
            let bits = crate::value::JSValue::pointer(err as *const u8).bits();
            crate::exception::js_throw(f64::from_bits(bits))
        }
    };
}

mod blob;
mod consumers;
mod fs_promises;
mod hono_jsx;
mod stream_promises;
mod timers;
mod trace_events;

// #1671: hono/jsx/server + hono/jsx/streaming. Re-export the stream-creation
// registration so perry-stdlib's `bundled-streams` init can wire it up.
pub use hono_jsx::js_register_jsx_render_stream;
use hono_jsx::{
    thunk_hono_fragment, thunk_hono_jsx, thunk_hono_jsxnode, thunk_hono_jsxs,
    thunk_hono_render_to_readable_stream, thunk_hono_suspense,
};

use consumers::{
    thunk_consumers_arrayBuffer, thunk_consumers_blob, thunk_consumers_buffer,
    thunk_consumers_bytes, thunk_consumers_json, thunk_consumers_text,
};
// Re-export at the `node_submodules` root so perry-stdlib's
// `perry_runtime::node_submodules::js_register_stream_consumer_callbacks`
// call site keeps resolving after the consumers split.
pub use consumers::js_register_stream_consumer_callbacks;
use fs_promises::{
    thunk_fs_promises_access, thunk_fs_promises_appendFile, thunk_fs_promises_chmod,
    thunk_fs_promises_chown, thunk_fs_promises_copyFile, thunk_fs_promises_cp,
    thunk_fs_promises_glob, thunk_fs_promises_lchmod, thunk_fs_promises_lchown,
    thunk_fs_promises_link, thunk_fs_promises_lstat, thunk_fs_promises_lutimes,
    thunk_fs_promises_mkdir, thunk_fs_promises_mkdtemp, thunk_fs_promises_open,
    thunk_fs_promises_opendir, thunk_fs_promises_readFile, thunk_fs_promises_readdir,
    thunk_fs_promises_readlink, thunk_fs_promises_realpath, thunk_fs_promises_rename,
    thunk_fs_promises_rm, thunk_fs_promises_rmdir, thunk_fs_promises_stat,
    thunk_fs_promises_statfs, thunk_fs_promises_symlink, thunk_fs_promises_truncate,
    thunk_fs_promises_unlink, thunk_fs_promises_utimes, thunk_fs_promises_watch,
    thunk_fs_promises_writeFile, thunk_readline_Interface, thunk_readline_Readline,
    thunk_readline_createInterface,
};
use stream_promises::{thunk_streamP_finished, thunk_streamP_pipeline, value_from_ptr};
use timers::{
    timers_ns_clear_immediate, timers_ns_clear_interval, timers_ns_clear_timeout,
    timers_ns_set_immediate, timers_ns_set_interval, timers_ns_set_timeout,
    timers_promises_scheduler, timers_promises_scheduler_wait, timers_promises_scheduler_yield,
    timers_promises_set_immediate, timers_promises_set_interval, timers_promises_set_timeout,
};
use trace_events::{thunk_trace_events_createTracing, thunk_trace_events_getEnabledCategories};

// node:sys is a deprecated alias for node:util. Known util-backed
// exports are rebound to util's callable singletons below so identity
// checks like `sys.format === util.format` match Node; these thunks
// remain as fallbacks for sys names Perry does not yet expose through
// the util native module table.
thunk!(thunk_sys_format, "node:sys.format is not yet implemented in Perry (use node:util.format; node:sys is deprecated).");
thunk!(thunk_sys_inspect, "node:sys.inspect is not yet implemented in Perry (use node:util.inspect; node:sys is deprecated).");
thunk!(thunk_sys_debuglog, "node:sys.debuglog is not yet implemented in Perry (use node:util.debuglog; node:sys is deprecated).");
thunk!(thunk_sys_deprecate, "node:sys.deprecate is not yet implemented in Perry (use node:util.deprecate; node:sys is deprecated).");
thunk!(thunk_sys_promisify, "node:sys.promisify is not yet implemented in Perry (use node:util.promisify; node:sys is deprecated).");
thunk!(thunk_sys_callbackify, "node:sys.callbackify is not yet implemented in Perry (use node:util.callbackify; node:sys is deprecated).");
thunk!(thunk_sys_isArray, "node:sys.isArray is not yet implemented in Perry (use node:util.isArray; node:sys is deprecated).");

// #1545: node:stream/web (WHATWG Web Streams). The named-export bindings
// exist so `typeof ReadableStream === "function"` and `import * as web from
// "node:stream/web"` work. Construction (`new ReadableStream(...)`,
// `new CountQueuingStrategy(...)`, …) is handled in codegen's
// builtin-constructor dispatch (lower_call/builtin.rs), which routes by the
// textual class name regardless of how it was imported — so this thunk only
// runs when a Web Streams class is *called* without `new`, which throws a
// TypeError in Node too. One shared thunk covers all 17 exported classes.
thunk!(
    thunk_stream_web_ctor,
    "Web Streams constructors (node:stream/web) require the 'new' operator."
);

// ----- submodule table -----

const SUBMODULES: &[SubmoduleSpec] = &[
    SubmoduleSpec {
        // node:timers namespace object (`import * as timers`). Named imports
        // bypass this (compile.rs) to keep the global fast-path. (#1213)
        key: "timers",
        exports: &[
            ExportSpec {
                name: "setTimeout",
                thunk: ExportThunk::Fn3(timers_ns_set_timeout),
            },
            ExportSpec {
                name: "setInterval",
                thunk: ExportThunk::Fn3(timers_ns_set_interval),
            },
            ExportSpec {
                name: "setImmediate",
                thunk: ExportThunk::Fn2(timers_ns_set_immediate),
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
                thunk: ExportThunk::Fn3(timers_promises_set_timeout),
            },
            ExportSpec {
                name: "setImmediate",
                thunk: ExportThunk::Fn2(timers_promises_set_immediate),
            },
            ExportSpec {
                name: "setInterval",
                thunk: ExportThunk::Fn3(timers_promises_set_interval),
            },
            ExportSpec {
                name: "scheduler",
                thunk: ExportThunk::Fn1(timers_promises_scheduler),
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
                name: "lchmod",
                thunk: ExportThunk::Fn2(thunk_fs_promises_lchmod),
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
                thunk: ExportThunk::Fn3(thunk_streamP_pipeline),
            },
            ExportSpec {
                name: "finished",
                thunk: ExportThunk::Fn2(thunk_streamP_finished),
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
    // #1545: node:stream/web exports the full WHATWG Web Streams class set.
    // Every entry maps to the same throwing thunk — its sole purpose is to
    // give each name `typeof === "function"` and a namespace slot; real
    // construction goes through codegen's builtin `new` dispatch.
    SubmoduleSpec {
        key: "stream_web",
        exports: &[
            ExportSpec {
                name: "ReadableStream",
                thunk: ExportThunk::Fn1(thunk_stream_web_ctor),
            },
            ExportSpec {
                name: "ReadableStreamDefaultReader",
                thunk: ExportThunk::Fn1(thunk_stream_web_ctor),
            },
            ExportSpec {
                name: "ReadableStreamBYOBReader",
                thunk: ExportThunk::Fn1(thunk_stream_web_ctor),
            },
            ExportSpec {
                name: "ReadableStreamDefaultController",
                thunk: ExportThunk::Fn1(thunk_stream_web_ctor),
            },
            ExportSpec {
                name: "ReadableByteStreamController",
                thunk: ExportThunk::Fn1(thunk_stream_web_ctor),
            },
            ExportSpec {
                name: "ReadableStreamBYOBRequest",
                thunk: ExportThunk::Fn1(thunk_stream_web_ctor),
            },
            ExportSpec {
                name: "WritableStream",
                thunk: ExportThunk::Fn1(thunk_stream_web_ctor),
            },
            ExportSpec {
                name: "WritableStreamDefaultWriter",
                thunk: ExportThunk::Fn1(thunk_stream_web_ctor),
            },
            ExportSpec {
                name: "WritableStreamDefaultController",
                thunk: ExportThunk::Fn1(thunk_stream_web_ctor),
            },
            ExportSpec {
                name: "TransformStream",
                thunk: ExportThunk::Fn1(thunk_stream_web_ctor),
            },
            ExportSpec {
                name: "TransformStreamDefaultController",
                thunk: ExportThunk::Fn1(thunk_stream_web_ctor),
            },
            ExportSpec {
                name: "ByteLengthQueuingStrategy",
                thunk: ExportThunk::Fn1(thunk_stream_web_ctor),
            },
            ExportSpec {
                name: "CountQueuingStrategy",
                thunk: ExportThunk::Fn1(thunk_stream_web_ctor),
            },
            ExportSpec {
                name: "TextEncoderStream",
                thunk: ExportThunk::Fn1(thunk_stream_web_ctor),
            },
            ExportSpec {
                name: "TextDecoderStream",
                thunk: ExportThunk::Fn1(thunk_stream_web_ctor),
            },
            ExportSpec {
                name: "CompressionStream",
                thunk: ExportThunk::Fn1(thunk_stream_web_ctor),
            },
            ExportSpec {
                name: "DecompressionStream",
                thunk: ExportThunk::Fn1(thunk_stream_web_ctor),
            },
        ],
    },
    // #1671: hono/jsx/server — the JSX runtime helpers. `jsx`/`jsxs` forward
    // to the built-in `js_jsx` renderer; `Fragment` renders its children;
    // `JSXNode` is an exposed stub (Perry boxes nodes internally).
    SubmoduleSpec {
        key: "hono_jsx_server",
        exports: &[
            ExportSpec {
                name: "jsx",
                thunk: ExportThunk::Fn2(thunk_hono_jsx),
            },
            ExportSpec {
                name: "jsxs",
                thunk: ExportThunk::Fn2(thunk_hono_jsxs),
            },
            ExportSpec {
                name: "Fragment",
                thunk: ExportThunk::Fn1(thunk_hono_fragment),
            },
            ExportSpec {
                name: "JSXNode",
                thunk: ExportThunk::Fn1(thunk_hono_jsxnode),
            },
        ],
    },
    // #1671: hono/jsx/streaming — server-side streaming helpers.
    // `renderToReadableStream` renders eagerly to a single-chunk ReadableStream;
    // `Suspense` renders its children (Perry has no streaming-suspension point).
    SubmoduleSpec {
        key: "hono_jsx_streaming",
        exports: &[
            ExportSpec {
                name: "renderToReadableStream",
                thunk: ExportThunk::Fn2(thunk_hono_render_to_readable_stream),
            },
            ExportSpec {
                name: "Suspense",
                thunk: ExportThunk::Fn1(thunk_hono_suspense),
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
    SubmoduleSpec {
        key: "trace_events",
        exports: &[
            ExportSpec {
                name: "createTracing",
                thunk: ExportThunk::Fn1(thunk_trace_events_createTracing),
            },
            ExportSpec {
                name: "getEnabledCategories",
                thunk: ExportThunk::Fn1(thunk_trace_events_getEnabledCategories),
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
static SYS_DEPRECATION_WARNED: AtomicI64 = AtomicI64::new(0);

pub(crate) fn emit_sys_deprecation_warning_once() {
    if SYS_DEPRECATION_WARNED
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let pid = std::process::id();
    eprintln!(
        "(node:{pid}) [DEP0025] DeprecationWarning: sys is deprecated. Use `node:util` instead."
    );
    eprintln!("(Use `node --trace-deprecation ...` to show where the warning was created)");
}

fn sys_util_namespace_value() -> f64 {
    crate::object::js_create_native_module_namespace(b"util".as_ptr(), "util".len())
}

fn sys_util_export_value(name: &str) -> Option<f64> {
    if name == "default" {
        return Some(sys_util_namespace_value());
    }
    let value = unsafe {
        crate::object::js_native_module_property_by_name(
            b"util".as_ptr(),
            "util".len(),
            name.as_ptr(),
            name.len(),
        )
    };
    if value.to_bits() == crate::value::TAG_UNDEFINED {
        None
    } else {
        Some(value)
    }
}

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
    if let Some(fixed_arity) = export_rest_fixed_arity(submod.key, export.name) {
        crate::closure::js_register_closure_rest(thunk_ptr, fixed_arity);
    } else {
        // Arity is encoded in the ExportThunk variant, so the closure dispatch
        // pads missing args with undefined for variadic-friendly thunks. This
        // replaces the per-submodule arity tables in earlier revisions.
        crate::closure::js_register_closure_arity(thunk_ptr, export.thunk.arity());
    }
    if submod.key == "timers_promises" && export.name == "scheduler" {
        let wait = js_closure_alloc(timers_promises_scheduler_wait as *const u8, 0);
        crate::closure::js_register_closure_arity(timers_promises_scheduler_wait as *const u8, 2);
        crate::closure::closure_set_dynamic_prop(
            allocated as usize,
            "wait",
            f64::from_bits(JSValue::pointer(wait as *const u8).bits()),
        );

        let yield_fn = js_closure_alloc(timers_promises_scheduler_yield as *const u8, 0);
        crate::closure::js_register_closure_arity(timers_promises_scheduler_yield as *const u8, 0);
        crate::closure::closure_set_dynamic_prop(
            allocated as usize,
            "yield",
            f64::from_bits(JSValue::pointer(yield_fn as *const u8).bits()),
        );
    }
    EXPORT_SINGLETONS.with(|m| {
        m.borrow_mut().insert(key, allocated);
    });
    ANY_SINGLETON_ALLOCATED.store(1, Ordering::Release);
    allocated
}

fn export_rest_fixed_arity(submod_key: &str, export_name: &str) -> Option<u32> {
    match (submod_key, export_name) {
        ("stream_promises", "pipeline") => Some(2),
        ("timers", "setTimeout" | "setInterval") => Some(2),
        ("timers", "setImmediate") => Some(1),
        ("trace_events", "getEnabledCategories") => Some(0),
        _ => None,
    }
}

pub(crate) fn is_diagnostics_channel_constructor_value(value: f64) -> bool {
    let js_value = JSValue::from_bits(value.to_bits());
    if !js_value.is_pointer() {
        return false;
    }
    let ptr = js_value.as_pointer::<ClosureHeader>() as *mut ClosureHeader;
    let Some(submod) = find_submodule("diagnostics_channel") else {
        return false;
    };
    let Some(export) = find_export(submod, "Channel") else {
        return false;
    };
    let key = (submod.key.as_ptr() as usize, export.name.as_ptr() as usize);
    EXPORT_SINGLETONS.with(|m| m.borrow().get(&key).copied() == Some(ptr))
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
        let value_f64 = if submod.key == "sys" {
            sys_util_export_value(spec.name).unwrap_or_else(|| {
                let closure_ptr = ensure_export_singleton(submod, spec);
                f64::from_bits(JSValue::pointer(closure_ptr as *const u8).bits())
            })
        } else {
            let closure_ptr = ensure_export_singleton(submod, spec);
            f64::from_bits(JSValue::pointer(closure_ptr as *const u8).bits())
        };
        unsafe {
            let name_bytes = spec.name.as_bytes();
            let name_header = js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
            crate::object::js_object_set_field_by_name(obj, name_header, value_f64);
        }
    }
    if submod.key == "stream_promises" {
        let value = value_from_ptr(obj as *const u8);
        let name = b"default";
        let name_header = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        unsafe {
            crate::object::js_object_set_field_by_name(obj, name_header, value);
        }
    }
    if submod.key == "trace_events" {
        let default_obj = js_object_alloc(0, submod.exports.len() as u32);
        for spec in submod.exports {
            let closure_ptr = ensure_export_singleton(submod, spec);
            let value = f64::from_bits(JSValue::pointer(closure_ptr as *const u8).bits());
            let name_bytes = spec.name.as_bytes();
            let name_header = js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
            crate::object::js_object_set_field_by_name(default_obj, name_header, value);
        }
        let name = b"default";
        let name_header = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(
            obj,
            name_header,
            f64::from_bits(JSValue::pointer(default_obj as *const u8).bits()),
        );
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
    trace_events::scan_trace_events_roots_mut(visitor);
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

/// Returns a NaN-boxed export singleton for the given
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
    if submod.key == "sys" {
        emit_sys_deprecation_warning_once();
        if let Some(value) = sys_util_export_value(name) {
            return value;
        }
    }
    if submod.key == "stream_promises" && name == "default" {
        let obj = ensure_namespace_singleton(submod);
        return f64::from_bits(JSValue::pointer(obj as *const u8).bits());
    }
    if submod.key == "trace_events" && name == "default" {
        let obj = ensure_namespace_singleton(submod);
        let name_header = js_string_from_bytes(b"default".as_ptr(), 7);
        let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, name_header);
        return value;
    }
    let export = match find_export(submod, name) {
        Some(e) => e,
        None => return f64::from_bits(JSValue::bool(true).bits()),
    };
    let closure_ptr = ensure_export_singleton(submod, export);
    f64::from_bits(JSValue::pointer(closure_ptr as *const u8).bits())
}

/// Returns a namespace-member value for a known Node submodule namespace import.
///
/// This differs from `js_node_submodule_export_as_function`: direct named-import
/// fallback behavior still uses the historical `TAG_TRUE` sentinel for
/// unlisted exports, but namespace property reads must behave like ordinary JS
/// objects and return `undefined` for absent properties.
#[no_mangle]
pub unsafe extern "C" fn js_node_submodule_namespace_member(
    submod_key_ptr: *const u8,
    submod_key_len: u32,
    name_ptr: *const u8,
    name_len: u32,
) -> f64 {
    let submod_bytes = std::slice::from_raw_parts(submod_key_ptr, submod_key_len as usize);
    let name_bytes = std::slice::from_raw_parts(name_ptr, name_len as usize);
    let submod_key = match std::str::from_utf8(submod_bytes) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(crate::value::TAG_UNDEFINED),
    };
    let name = match std::str::from_utf8(name_bytes) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(crate::value::TAG_UNDEFINED),
    };
    let submod = match find_submodule(submod_key) {
        Some(s) => s,
        None => return f64::from_bits(crate::value::TAG_UNDEFINED),
    };
    if submod.key == "sys" {
        emit_sys_deprecation_warning_once();
        if name == "default" {
            return sys_util_namespace_value();
        }
        return unsafe {
            crate::object::js_native_module_property_by_name(
                b"util".as_ptr(),
                "util".len(),
                name.as_ptr(),
                name.len(),
            )
        };
    }
    if submod.key == "stream_promises" && name == "default" {
        let obj = ensure_namespace_singleton(submod);
        return f64::from_bits(JSValue::pointer(obj as *const u8).bits());
    }
    if submod.key == "trace_events" && name == "default" {
        let obj = ensure_namespace_singleton(submod);
        let name_header = js_string_from_bytes(b"default".as_ptr(), 7);
        let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, name_header);
        return value;
    }
    let export = match find_export(submod, name) {
        Some(e) => e,
        None => return f64::from_bits(crate::value::TAG_UNDEFINED),
    };
    let closure_ptr = ensure_export_singleton(submod, export);
    f64::from_bits(JSValue::pointer(closure_ptr as *const u8).bits())
}

/// Returns a NaN-boxed namespace stub object for the given submodule.
/// Each known named export of that submodule is exposed as an own
/// property on the object whose value is the export singleton
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
    if submod.key == "sys" {
        emit_sys_deprecation_warning_once();
        return sys_util_namespace_value();
    }
    let obj = ensure_namespace_singleton(submod);
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

#[cfg(test)]
mod tests {
    use super::blob::string_from_value;
    use super::stream_promises::{
        abort_error_value, get_object_property, object_ptr_from_value, undefined_value,
    };
    use super::*;
    use std::cell::RefCell;

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
            "trace_events",
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

    fn boxed_ptr(ptr: *const u8) -> f64 {
        f64::from_bits(JSValue::pointer(ptr).bits())
    }

    fn promise_ptr(value: f64) -> *mut crate::promise::Promise {
        crate::value::js_nanbox_get_pointer(value) as *mut crate::promise::Promise
    }

    fn string_value(s: &str) -> f64 {
        let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
        f64::from_bits(JSValue::string_ptr(ptr).bits())
    }

    #[test]
    fn stream_parent_promises_property_exposes_namespace() {
        let value = unsafe {
            crate::object::js_native_module_property_by_name(
                b"stream".as_ptr(),
                "stream".len(),
                b"promises".as_ptr(),
                "promises".len(),
            )
        };
        let ns = object_ptr_from_value(value).expect("stream.promises should be an object");
        assert!(get_object_property(boxed_ptr(ns as *const u8), b"pipeline").is_some());
        assert!(get_object_property(boxed_ptr(ns as *const u8), b"finished").is_some());
    }

    /// #2133: `fs.promises` (the parent `node:fs` module's `.promises`
    /// property) must resolve to the populated `fs_promises` submodule
    /// singleton — not an empty namespace stub — so destructured exports
    /// like `const { open } = fs.promises` and the indirect form
    /// (`const p = fs.promises; p.open(...)`) both reach real callable
    /// closures and a returned FileHandle dispatches its methods.
    #[test]
    fn fs_parent_promises_property_exposes_namespace() {
        let value = unsafe {
            crate::object::js_native_module_property_by_name(
                b"fs".as_ptr(),
                "fs".len(),
                b"promises".as_ptr(),
                "promises".len(),
            )
        };
        let ns = object_ptr_from_value(value).expect("fs.promises should be an object");
        let ns_value = boxed_ptr(ns as *const u8);
        // Spot-check a few exports from the fs_promises submodule.
        assert!(get_object_property(ns_value, b"open").is_some());
        assert!(get_object_property(ns_value, b"readFile").is_some());
        assert!(get_object_property(ns_value, b"writeFile").is_some());
        assert!(get_object_property(ns_value, b"chmod").is_some());
        assert!(get_object_property(ns_value, b"stat").is_some());
    }

    #[test]
    fn stream_promises_default_export_exposes_namespace() {
        let value = unsafe {
            js_node_submodule_export_as_function(
                b"stream_promises".as_ptr(),
                "stream_promises".len() as u32,
                b"default".as_ptr(),
                "default".len() as u32,
            )
        };
        let ns = object_ptr_from_value(value).expect("default export should be an object");
        let ns_value = boxed_ptr(ns as *const u8);

        assert!(get_object_property(ns_value, b"pipeline").is_some());
        assert!(get_object_property(ns_value, b"finished").is_some());
        assert_eq!(
            get_object_property(ns_value, b"default").unwrap().to_bits(),
            ns_value.to_bits()
        );
    }

    #[test]
    fn namespace_member_missing_export_returns_undefined() {
        let value = unsafe {
            js_node_submodule_namespace_member(
                b"diagnostics_channel".as_ptr(),
                "diagnostics_channel".len() as u32,
                b"boundedChannel".as_ptr(),
                "boundedChannel".len() as u32,
            )
        };

        assert_eq!(value.to_bits(), crate::value::TAG_UNDEFINED);
    }

    #[test]
    fn direct_missing_export_keeps_legacy_true_sentinel() {
        let value = unsafe {
            js_node_submodule_export_as_function(
                b"diagnostics_channel".as_ptr(),
                "diagnostics_channel".len() as u32,
                b"boundedChannel".as_ptr(),
                "boundedChannel".len() as u32,
            )
        };

        assert_eq!(value.to_bits(), crate::value::TAG_TRUE);
    }

    #[test]
    fn sys_format_export_reuses_util_callable() {
        let sys_format = unsafe {
            js_node_submodule_export_as_function(
                b"sys".as_ptr(),
                "sys".len() as u32,
                b"format".as_ptr(),
                "format".len() as u32,
            )
        };
        let util_format = unsafe {
            crate::object::js_native_module_property_by_name(
                b"util".as_ptr(),
                "util".len(),
                b"format".as_ptr(),
                "format".len(),
            )
        };

        assert_eq!(sys_format.to_bits(), util_format.to_bits());
    }

    #[test]
    fn sys_namespace_reuses_util_callable() {
        let value = unsafe { js_node_submodule_namespace(b"sys".as_ptr(), "sys".len() as u32) };
        let ns = object_ptr_from_value(value).expect("sys namespace should be an object");
        let ns_value = boxed_ptr(ns as *const u8);
        let sys_inspect = get_object_property(ns_value, b"inspect").unwrap();
        let util_inspect = unsafe {
            crate::object::js_native_module_property_by_name(
                b"util".as_ptr(),
                "util".len(),
                b"inspect".as_ptr(),
                "inspect".len(),
            )
        };

        assert_eq!(sys_inspect.to_bits(), util_inspect.to_bits());
    }

    #[test]
    fn sys_default_export_is_util_namespace() {
        let sys_default = unsafe {
            js_node_submodule_export_as_function(
                b"sys".as_ptr(),
                "sys".len() as u32,
                b"default".as_ptr(),
                "default".len() as u32,
            )
        };
        let util_default =
            crate::object::js_create_native_module_namespace(b"util".as_ptr(), "util".len());

        assert_eq!(sys_default.to_bits(), util_default.to_bits());
    }

    #[test]
    fn sys_namespace_types_member_reuses_util_types() {
        let sys_types = unsafe {
            js_node_submodule_namespace_member(
                b"sys".as_ptr(),
                "sys".len() as u32,
                b"types".as_ptr(),
                "types".len() as u32,
            )
        };
        let util_types = unsafe {
            crate::object::js_native_module_property_by_name(
                b"util".as_ptr(),
                "util".len(),
                b"types".as_ptr(),
                "types".len(),
            )
        };

        assert_eq!(sys_types.to_bits(), util_types.to_bits());
    }

    #[test]
    fn stream_promises_finished_resolves_for_clean_stub_stream() {
        let stream = crate::node_stream::js_node_stream_passthrough_new(undefined_value());
        let end = get_object_property(stream, b"end").expect("stream.end should exist");
        let prev_this = crate::object::js_implicit_this_set(stream);
        unsafe {
            let _ = crate::closure::js_native_call_value(end, std::ptr::null(), 0);
        }
        crate::object::js_implicit_this_set(prev_this);

        let promise_value = thunk_streamP_finished(std::ptr::null(), stream, undefined_value());
        let promise = promise_ptr(promise_value);

        assert_eq!(crate::promise::js_promise_state(promise), 1);
        assert_eq!(
            crate::promise::js_promise_value(promise).to_bits(),
            crate::value::TAG_UNDEFINED
        );
    }

    #[test]
    fn stream_promises_finished_rejects_hidden_stream_error() {
        let stream = crate::node_stream::js_node_stream_passthrough_new(undefined_value());
        let err = abort_error_value();
        crate::node_stream::test_set_hidden_error(stream, err);

        let promise_value = thunk_streamP_finished(std::ptr::null(), stream, undefined_value());
        let promise = promise_ptr(promise_value);

        assert_eq!(crate::promise::js_promise_state(promise), 2);
        assert_eq!(
            crate::promise::js_promise_reason(promise).to_bits(),
            err.to_bits()
        );
    }

    #[test]
    fn stream_promises_finished_rejects_later_destroy_error() {
        let stream = crate::node_stream::js_node_stream_readable_new(undefined_value());
        crate::node_stream::test_install_manual_read(stream);
        let promise_value = thunk_streamP_finished(std::ptr::null(), stream, undefined_value());
        let promise = promise_ptr(promise_value);

        assert_eq!(crate::promise::js_promise_state(promise), 0);

        let err = string_value("later-error");
        let handle = object_ptr_from_value(stream).expect("stream object") as i64;
        let _ = crate::node_stream::js_node_stream_method_destroy(handle, err);
        let _ = crate::promise::js_promise_run_microtasks();

        assert_eq!(crate::promise::js_promise_state(promise), 2);
        assert_eq!(
            crate::promise::js_promise_reason(promise).to_bits(),
            err.to_bits()
        );
    }

    thread_local! {
        static PIPELINE_CAPTURED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    extern "C" fn pipeline_write_capture(
        _closure: *const ClosureHeader,
        chunk: f64,
        _enc: f64,
    ) -> f64 {
        PIPELINE_CAPTURED.with(|captured| {
            captured
                .borrow_mut()
                .push(string_from_value(chunk).unwrap_or_default());
        });
        f64::from_bits(crate::value::TAG_TRUE)
    }

    extern "C" fn pipeline_end_capture(_closure: *const ClosureHeader, _arg: f64) -> f64 {
        undefined_value()
    }

    #[test]
    fn stream_promises_pipeline_transfers_readable_from_chunks() {
        PIPELINE_CAPTURED.with(|captured| captured.borrow_mut().clear());
        crate::closure::js_register_closure_arity(pipeline_write_capture as *const u8, 2);
        crate::closure::js_register_closure_arity(pipeline_end_capture as *const u8, 1);

        let mut arr = crate::array::js_array_alloc(2);
        arr = crate::array::js_array_push_f64(arr, string_value("await-"));
        arr = crate::array::js_array_push_f64(arr, string_value("works"));
        let source = crate::node_stream::js_node_stream_readable_from(boxed_ptr(arr as *const u8));

        let sink = js_object_alloc(0, 2);
        let write = js_closure_alloc(pipeline_write_capture as *const u8, 0);
        let end = js_closure_alloc(pipeline_end_capture as *const u8, 0);
        js_object_set_field_by_name(
            sink,
            js_string_from_bytes(b"write".as_ptr(), 5),
            boxed_ptr(write as *const u8),
        );
        js_object_set_field_by_name(
            sink,
            js_string_from_bytes(b"end".as_ptr(), 3),
            boxed_ptr(end as *const u8),
        );

        let promise_value = thunk_streamP_pipeline(
            std::ptr::null(),
            source,
            boxed_ptr(sink as *const u8),
            undefined_value(),
        );
        let promise = promise_ptr(promise_value);

        assert_eq!(crate::promise::js_promise_state(promise), 1);
        PIPELINE_CAPTURED.with(|captured| {
            assert_eq!(captured.borrow().join(""), "await-works");
        });
    }

    #[test]
    fn stream_promises_finished_rejects_when_signal_aborts() {
        let controller = crate::url::js_abort_controller_new();
        let signal = crate::url::js_abort_controller_signal(controller);
        let opts = js_object_alloc(0, 1);
        js_object_set_field_by_name(
            opts,
            js_string_from_bytes(b"signal".as_ptr(), 6),
            boxed_ptr(signal as *const u8),
        );
        let stream = crate::node_stream::js_node_stream_passthrough_new(undefined_value());

        let promise_value =
            thunk_streamP_finished(std::ptr::null(), stream, boxed_ptr(opts as *const u8));
        let promise = promise_ptr(promise_value);
        assert_eq!(crate::promise::js_promise_state(promise), 0);

        crate::url::js_abort_controller_abort(controller);

        assert_eq!(crate::promise::js_promise_state(promise), 2);
    }

    #[test]
    fn stream_promises_finished_with_signal_resolves_for_ended_stub_stream() {
        let controller = crate::url::js_abort_controller_new();
        let signal = crate::url::js_abort_controller_signal(controller);
        let opts = js_object_alloc(0, 1);
        js_object_set_field_by_name(
            opts,
            js_string_from_bytes(b"signal".as_ptr(), 6),
            boxed_ptr(signal as *const u8),
        );
        let stream = crate::node_stream::js_node_stream_passthrough_new(undefined_value());
        let end = get_object_property(stream, b"end").expect("stream.end should exist");
        let prev_this = crate::object::js_implicit_this_set(stream);
        unsafe {
            let _ = crate::closure::js_native_call_value(end, std::ptr::null(), 0);
        }
        crate::object::js_implicit_this_set(prev_this);

        let promise_value =
            thunk_streamP_finished(std::ptr::null(), stream, boxed_ptr(opts as *const u8));
        let promise = promise_ptr(promise_value);

        assert_eq!(crate::promise::js_promise_state(promise), 1);
        assert_eq!(
            crate::promise::js_promise_value(promise).to_bits(),
            crate::value::TAG_UNDEFINED
        );
    }
}
