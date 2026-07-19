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
pub(crate) mod diagnostics_gc;
pub use diagnostics::*;
pub(crate) mod diagnostics_tail;
pub(crate) use diagnostics_tail::*;

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

pub(crate) mod blob;
mod consumers;
mod fs_promises;
mod hono_jsx;
mod stream_promises;
mod test;
mod timers;
mod trace_events;
mod zlib;
pub use zlib::{js_zlib_resolve_level, js_zlib_validate_buffer_arg, js_zlib_validate_options};

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
pub(crate) use fs_promises::js_readline_promises_readline_new;
use fs_promises::{
    thunk_fs_promises_access, thunk_fs_promises_appendFile, thunk_fs_promises_chmod,
    thunk_fs_promises_chown, thunk_fs_promises_constants, thunk_fs_promises_copyFile,
    thunk_fs_promises_cp, thunk_fs_promises_glob, thunk_fs_promises_lchmod,
    thunk_fs_promises_lchown, thunk_fs_promises_link, thunk_fs_promises_lstat,
    thunk_fs_promises_lutimes, thunk_fs_promises_mkdir, thunk_fs_promises_mkdtemp,
    thunk_fs_promises_mkdtempDisposable, thunk_fs_promises_open, thunk_fs_promises_opendir,
    thunk_fs_promises_readFile, thunk_fs_promises_readdir, thunk_fs_promises_readlink,
    thunk_fs_promises_realpath, thunk_fs_promises_rename, thunk_fs_promises_rm,
    thunk_fs_promises_rmdir, thunk_fs_promises_stat, thunk_fs_promises_statfs,
    thunk_fs_promises_symlink, thunk_fs_promises_truncate, thunk_fs_promises_unlink,
    thunk_fs_promises_utimes, thunk_fs_promises_watch, thunk_fs_promises_writeFile,
    thunk_readline_Interface, thunk_readline_Readline, thunk_readline_createInterface,
};
use stream_promises::{thunk_streamP_finished, thunk_streamP_pipeline, value_from_ptr};
use test::{
    thunk_reporter_dot, thunk_reporter_junit, thunk_reporter_lcov, thunk_reporter_spec,
    thunk_reporter_tap, thunk_test, thunk_test_hook, thunk_test_only, thunk_test_run,
    thunk_test_skip, thunk_test_todo,
};
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

extern "C" fn thunk_vm_create_context(_closure: *const ClosureHeader, sandbox: f64) -> f64 {
    crate::object::js_vm_create_context(sandbox)
}

// ----- submodule table -----

// ----- per-submodule specs (devirt) -----
static SUBMOD_VM: SubmoduleSpec = SubmoduleSpec {
    key: "vm",
    exports: &[ExportSpec {
        name: "createContext",
        thunk: ExportThunk::Fn1(thunk_vm_create_context),
    }],
};

static SUBMOD_TIMERS: SubmoduleSpec = SubmoduleSpec {
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
};

static SUBMOD_TIMERS_PROMISES: SubmoduleSpec = SubmoduleSpec {
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
};

static SUBMOD_FS_PROMISES: SubmoduleSpec = SubmoduleSpec {
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
            name: "mkdtempDisposable",
            thunk: ExportThunk::Fn2(thunk_fs_promises_mkdtempDisposable),
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
        ExportSpec {
            name: "constants",
            thunk: ExportThunk::Fn1(thunk_fs_promises_constants),
        },
    ],
};

static SUBMOD_READLINE_PROMISES: SubmoduleSpec = SubmoduleSpec {
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
            thunk: ExportThunk::Fn2(thunk_readline_Readline),
        },
    ],
};

static SUBMOD_STREAM_PROMISES: SubmoduleSpec = SubmoduleSpec {
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
};

static SUBMOD_STREAM_CONSUMERS: SubmoduleSpec = SubmoduleSpec {
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
};

static SUBMOD_STREAM_WEB: SubmoduleSpec = SubmoduleSpec {
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
};

static SUBMOD_HONO_JSX_SERVER: SubmoduleSpec = SubmoduleSpec {
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
};

static SUBMOD_HONO_JSX_STREAMING: SubmoduleSpec = SubmoduleSpec {
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
};

static SUBMOD_SYS: SubmoduleSpec = SubmoduleSpec {
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
};

static SUBMOD_DIAGNOSTICS_CHANNEL: SubmoduleSpec = SubmoduleSpec {
    key: "diagnostics_channel",
    exports: &[
        ExportSpec {
            name: "tracingChannel",
            thunk: ExportThunk::Fn1(thunk_diag_tracing_channel),
        },
        ExportSpec {
            name: "boundedChannel",
            thunk: ExportThunk::Fn1(thunk_diag_bounded_channel),
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
            name: "hasSubscribers",
            thunk: ExportThunk::Fn1(thunk_diag_has_subscribers),
        },
        ExportSpec {
            name: "Channel",
            thunk: ExportThunk::Fn1(thunk_diag_noop),
        },
        ExportSpec {
            name: "BoundedChannel",
            thunk: ExportThunk::Fn1(thunk_diag_bounded_channel),
        },
    ],
};

static SUBMOD_TRACE_EVENTS: SubmoduleSpec = SubmoduleSpec {
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
};

static SUBMOD_TEST: SubmoduleSpec = SubmoduleSpec {
    key: "test",
    exports: &[
        ExportSpec {
            name: "default",
            thunk: ExportThunk::Fn3(thunk_test),
        },
        ExportSpec {
            name: "test",
            thunk: ExportThunk::Fn3(thunk_test),
        },
        ExportSpec {
            name: "skip",
            thunk: ExportThunk::Fn3(thunk_test_skip),
        },
        ExportSpec {
            name: "todo",
            thunk: ExportThunk::Fn3(thunk_test_todo),
        },
        ExportSpec {
            name: "only",
            thunk: ExportThunk::Fn3(thunk_test_only),
        },
        ExportSpec {
            name: "suite",
            thunk: ExportThunk::Fn3(thunk_test),
        },
        ExportSpec {
            name: "describe",
            thunk: ExportThunk::Fn3(thunk_test),
        },
        ExportSpec {
            name: "it",
            thunk: ExportThunk::Fn3(thunk_test),
        },
        ExportSpec {
            name: "before",
            thunk: ExportThunk::Fn1(thunk_test_hook),
        },
        ExportSpec {
            name: "after",
            thunk: ExportThunk::Fn1(thunk_test_hook),
        },
        ExportSpec {
            name: "beforeEach",
            thunk: ExportThunk::Fn1(thunk_test_hook),
        },
        ExportSpec {
            name: "afterEach",
            thunk: ExportThunk::Fn1(thunk_test_hook),
        },
        ExportSpec {
            name: "run",
            thunk: ExportThunk::Fn1(thunk_test_run),
        },
        // Object-valued exports are handled by `special_export_value`.
        ExportSpec {
            name: "mock",
            thunk: ExportThunk::Fn1(thunk_test_run),
        },
        ExportSpec {
            name: "snapshot",
            thunk: ExportThunk::Fn1(thunk_test_run),
        },
    ],
};

static SUBMOD_TEST_REPORTERS: SubmoduleSpec = SubmoduleSpec {
    key: "test_reporters",
    exports: &[
        ExportSpec {
            name: "spec",
            thunk: ExportThunk::Fn1(thunk_reporter_spec),
        },
        ExportSpec {
            name: "tap",
            thunk: ExportThunk::Fn1(thunk_reporter_tap),
        },
        ExportSpec {
            name: "dot",
            thunk: ExportThunk::Fn1(thunk_reporter_dot),
        },
        ExportSpec {
            name: "junit",
            thunk: ExportThunk::Fn1(thunk_reporter_junit),
        },
        ExportSpec {
            name: "lcov",
            thunk: ExportThunk::Fn1(thunk_reporter_lcov),
        },
    ],
};

// ----- submodule registry (devirt) -----
use std::sync::atomic::AtomicPtr;
#[derive(Copy, Clone)]
#[repr(usize)]
enum SubmodBucket {
    Vm,
    Timers,
    TimersPromises,
    FsPromises,
    ReadlinePromises,
    StreamPromises,
    StreamConsumers,
    StreamWeb,
    HonoJsxServer,
    HonoJsxStreaming,
    Sys,
    DiagnosticsChannel,
    TraceEvents,
    Test,
    TestReporters,
}
const SUBMOD_COUNT: usize = 15;
static SUBMOD_REGISTRY: [AtomicPtr<SubmoduleSpec>; SUBMOD_COUNT] =
    [const { AtomicPtr::new(std::ptr::null_mut()) }; SUBMOD_COUNT];
fn submod_index(key: &str) -> Option<SubmodBucket> {
    match key {
        "vm" => Some(SubmodBucket::Vm),
        "timers" => Some(SubmodBucket::Timers),
        "timers_promises" => Some(SubmodBucket::TimersPromises),
        "fs_promises" => Some(SubmodBucket::FsPromises),
        "readline_promises" => Some(SubmodBucket::ReadlinePromises),
        "stream_promises" => Some(SubmodBucket::StreamPromises),
        "stream_consumers" => Some(SubmodBucket::StreamConsumers),
        "stream_web" => Some(SubmodBucket::StreamWeb),
        "hono_jsx_server" => Some(SubmodBucket::HonoJsxServer),
        "hono_jsx_streaming" => Some(SubmodBucket::HonoJsxStreaming),
        "sys" => Some(SubmodBucket::Sys),
        "diagnostics_channel" => Some(SubmodBucket::DiagnosticsChannel),
        "trace_events" => Some(SubmodBucket::TraceEvents),
        "test" => Some(SubmodBucket::Test),
        "test_reporters" => Some(SubmodBucket::TestReporters),
        _ => None,
    }
}
fn find_submodule(key: &str) -> Option<&'static SubmoduleSpec> {
    let b = submod_index(key)?;
    let p = SUBMOD_REGISTRY[b as usize].load(Ordering::Relaxed);
    if !p.is_null() {
        return Some(unsafe { &*(p as *const SubmoduleSpec) });
    }
    // Unit tests call the namespace/export helpers directly, without the
    // codegen-emitted `js_node_submod_install_<key>()` that precedes use in real
    // programs. Lazily populate the registry so tests exercise the real lookup
    // path. (Not compiled into production builds.)
    #[cfg(test)]
    {
        js_node_submod_install_all();
        let p = SUBMOD_REGISTRY[b as usize].load(Ordering::Relaxed);
        if !p.is_null() {
            return Some(unsafe { &*(p as *const SubmoduleSpec) });
        }
    }
    None
}

/// Test-only: whether `key` names a registered submodule spec. #6651 —
/// `process::builtin_submodule_key`'s cross-check (the spec type and its
/// fields are private to this module).
#[cfg(test)]
pub(crate) fn is_registered_submodule_key(key: &str) -> bool {
    ALL_SUBMODULE_SPECS.iter().any(|spec| spec.key == key)
}

/// Test-only: every submodule spec, for exhaustiveness checks (the production
/// `find_submodule` resolves through the registry, not an iterable array).
#[cfg(test)]
pub(super) const ALL_SUBMODULE_SPECS: &[&SubmoduleSpec] = &[
    &SUBMOD_VM,
    &SUBMOD_TIMERS,
    &SUBMOD_TIMERS_PROMISES,
    &SUBMOD_FS_PROMISES,
    &SUBMOD_READLINE_PROMISES,
    &SUBMOD_STREAM_PROMISES,
    &SUBMOD_STREAM_CONSUMERS,
    &SUBMOD_STREAM_WEB,
    &SUBMOD_HONO_JSX_SERVER,
    &SUBMOD_HONO_JSX_STREAMING,
    &SUBMOD_SYS,
    &SUBMOD_DIAGNOSTICS_CHANNEL,
    &SUBMOD_TRACE_EVENTS,
    &SUBMOD_TEST,
    &SUBMOD_TEST_REPORTERS,
];
#[no_mangle]
pub extern "C" fn js_node_submod_install_vm() {
    SUBMOD_REGISTRY[SubmodBucket::Vm as usize].store(
        &SUBMOD_VM as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_timers() {
    SUBMOD_REGISTRY[SubmodBucket::Timers as usize].store(
        &SUBMOD_TIMERS as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_timers_promises() {
    SUBMOD_REGISTRY[SubmodBucket::TimersPromises as usize].store(
        &SUBMOD_TIMERS_PROMISES as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_fs_promises() {
    // `fs/promises` exposes `constants` backed by the native `fs` module.
    crate::object::js_nm_install_fs();
    SUBMOD_REGISTRY[SubmodBucket::FsPromises as usize].store(
        &SUBMOD_FS_PROMISES as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_readline_promises() {
    SUBMOD_REGISTRY[SubmodBucket::ReadlinePromises as usize].store(
        &SUBMOD_READLINE_PROMISES as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_stream_promises() {
    SUBMOD_REGISTRY[SubmodBucket::StreamPromises as usize].store(
        &SUBMOD_STREAM_PROMISES as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_stream_consumers() {
    SUBMOD_REGISTRY[SubmodBucket::StreamConsumers as usize].store(
        &SUBMOD_STREAM_CONSUMERS as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_stream_web() {
    SUBMOD_REGISTRY[SubmodBucket::StreamWeb as usize].store(
        &SUBMOD_STREAM_WEB as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_hono_jsx_server() {
    SUBMOD_REGISTRY[SubmodBucket::HonoJsxServer as usize].store(
        &SUBMOD_HONO_JSX_SERVER as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_hono_jsx_streaming() {
    SUBMOD_REGISTRY[SubmodBucket::HonoJsxStreaming as usize].store(
        &SUBMOD_HONO_JSX_STREAMING as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_sys() {
    // `node:sys` is a deprecated alias that delegates to the native `util` module.
    crate::object::js_nm_install_util();
    SUBMOD_REGISTRY[SubmodBucket::Sys as usize].store(
        &SUBMOD_SYS as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_diagnostics_channel() {
    SUBMOD_REGISTRY[SubmodBucket::DiagnosticsChannel as usize].store(
        &SUBMOD_DIAGNOSTICS_CHANNEL as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_trace_events() {
    SUBMOD_REGISTRY[SubmodBucket::TraceEvents as usize].store(
        &SUBMOD_TRACE_EVENTS as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_test() {
    SUBMOD_REGISTRY[SubmodBucket::Test as usize].store(
        &SUBMOD_TEST as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_test_reporters() {
    SUBMOD_REGISTRY[SubmodBucket::TestReporters as usize].store(
        &SUBMOD_TEST_REPORTERS as *const SubmoduleSpec as *mut SubmoduleSpec,
        Ordering::Relaxed,
    );
}
#[no_mangle]
pub extern "C" fn js_node_submod_install_all() {
    js_node_submod_install_vm();
    js_node_submod_install_timers();
    js_node_submod_install_timers_promises();
    js_node_submod_install_fs_promises();
    js_node_submod_install_readline_promises();
    js_node_submod_install_stream_promises();
    js_node_submod_install_stream_consumers();
    js_node_submod_install_stream_web();
    js_node_submod_install_hono_jsx_server();
    js_node_submod_install_hono_jsx_streaming();
    js_node_submod_install_sys();
    js_node_submod_install_diagnostics_channel();
    js_node_submod_install_trace_events();
    js_node_submod_install_test();
    js_node_submod_install_test_reporters();
}
static SUBMOD_INSTALL_ALL_HOOK: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
#[no_mangle]
pub extern "C" fn js_node_submod_enable_install_all() {
    let f = std::hint::black_box(js_node_submod_install_all as extern "C" fn());
    SUBMOD_INSTALL_ALL_HOOK.store(f as *mut (), Ordering::Relaxed);
}
pub(crate) fn run_submod_install_all_hook() {
    let p = SUBMOD_INSTALL_ALL_HOOK.load(Ordering::Relaxed);
    if !p.is_null() {
        unsafe { std::mem::transmute::<*mut (), extern "C" fn()>(p)() };
    }
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

    /// Map from submod_key_ptr to the cached CommonJS-style default
    /// object for submodules whose `namespace.default` is not the
    /// namespace object itself.
    static DEFAULT_OBJECT_SINGLETONS: RefCell<std::collections::HashMap<usize, *mut ObjectHeader>> =
        RefCell::new(std::collections::HashMap::new());

    /// `node:timers/promises.scheduler` is a non-callable object with
    /// function-valued `wait` and `yield` properties.
    static TIMERS_PROMISES_SCHEDULER_OBJECT: RefCell<Option<*mut ObjectHeader>> =
        const { RefCell::new(None) };
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

fn sys_util_default_value() -> f64 {
    unsafe {
        crate::object::js_native_module_property_by_name(
            b"util".as_ptr(),
            "util".len(),
            b"default".as_ptr(),
            "default".len(),
        )
    }
}

fn sys_util_export_value(name: &str) -> Option<f64> {
    if name == "default" {
        return Some(sys_util_default_value());
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

fn fs_constants_namespace_value() -> f64 {
    unsafe {
        crate::object::js_native_module_property_by_name(
            b"fs".as_ptr(),
            "fs".len(),
            b"constants".as_ptr(),
            "constants".len(),
        )
    }
}

fn timers_promises_scheduler_value() -> f64 {
    TIMERS_PROMISES_SCHEDULER_OBJECT.with(|slot| {
        if let Some(cached) = *slot.borrow() {
            return f64::from_bits(JSValue::pointer(cached as *const u8).bits());
        }

        let obj = js_object_alloc(0, 2);

        let wait = js_closure_alloc(timers_promises_scheduler_wait as *const u8, 0);
        crate::closure::js_register_closure_arity(timers_promises_scheduler_wait as *const u8, 2);
        set_named_value(
            obj,
            "wait",
            f64::from_bits(JSValue::pointer(wait as *const u8).bits()),
        );

        let yield_fn = js_closure_alloc(timers_promises_scheduler_yield as *const u8, 0);
        crate::closure::js_register_closure_arity(timers_promises_scheduler_yield as *const u8, 0);
        set_named_value(
            obj,
            "yield",
            f64::from_bits(JSValue::pointer(yield_fn as *const u8).bits()),
        );

        *slot.borrow_mut() = Some(obj);
        ANY_SINGLETON_ALLOCATED.store(1, Ordering::Release);
        f64::from_bits(JSValue::pointer(obj as *const u8).bits())
    })
}

fn special_export_value(submod_key: &str, name: &str) -> Option<f64> {
    let value = match submod_key {
        "fs_promises" if name == "constants" => Some(fs_constants_namespace_value()),
        "timers_promises" if name == "scheduler" => Some(timers_promises_scheduler_value()),
        "stream_web"
            if matches!(
                name,
                "TextEncoderStream"
                    | "TextDecoderStream"
                    | "CompressionStream"
                    | "DecompressionStream"
            ) =>
        {
            Some(crate::object::js_get_global_this_builtin_value(
                name.as_ptr(),
                name.len(),
            ))
        }
        "test" => test::test_special_export_value(name),
        _ => None,
    };
    if value.is_some() {
        ANY_SINGLETON_ALLOCATED.store(1, Ordering::Release);
    }
    value
}

fn ensure_export_singleton(
    submod: &'static SubmoduleSpec,
    export: &'static ExportSpec,
) -> *mut ClosureHeader {
    let key_name = if submod.key == "test" && matches!(export.name, "default" | "test") {
        find_export(submod, "test")
            .map(|canonical| canonical.name)
            .unwrap_or(export.name)
    } else {
        export.name
    };
    let key = (submod.key.as_ptr() as usize, key_name.as_ptr() as usize);
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
    if submod.key == "test"
        && matches!(
            export.name,
            "default" | "test" | "suite" | "describe" | "it"
        )
    {
        test::decorate_test_export(allocated);
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

fn submodule_has_default_object(submod_key: &str) -> bool {
    matches!(
        submod_key,
        "diagnostics_channel"
            | "fs_promises"
            | "stream_consumers"
            | "stream_web"
            | "test_reporters"
            // `const nodeTimers = require('node:timers')` (Next.js's
            // fast-set-immediate extension) — without a default object the
            // binding read the TAG_TRUE sentinel, so member reads were
            // undefined and the `nodeTimers.setImmediate = patched`
            // monkey-patch threw at module init.
            | "timers"
            | "timers_promises"
    )
}

fn fs_promises_constants_value() -> f64 {
    unsafe {
        crate::object::js_native_module_property_by_name(
            b"fs".as_ptr(),
            "fs".len(),
            b"constants".as_ptr(),
            "constants".len(),
        )
    }
}

fn set_named_value(obj: *mut ObjectHeader, name: &str, value: f64) {
    let name_bytes = name.as_bytes();
    let name_header = js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
    unsafe {
        crate::object::js_object_set_field_by_name(obj, name_header, value);
    }
}

fn submodule_export_value(submod: &'static SubmoduleSpec, spec: &'static ExportSpec) -> f64 {
    if submod.key == "sys" {
        return sys_util_export_value(spec.name).unwrap_or_else(|| {
            let closure_ptr = ensure_export_singleton(submod, spec);
            f64::from_bits(JSValue::pointer(closure_ptr as *const u8).bits())
        });
    }
    if let Some(value) = special_export_value(submod.key, spec.name) {
        return value;
    }
    let closure_ptr = ensure_export_singleton(submod, spec);
    f64::from_bits(JSValue::pointer(closure_ptr as *const u8).bits())
}

fn submodule_default_object_value(submod: &'static SubmoduleSpec) -> Option<f64> {
    if !submodule_has_default_object(submod.key) {
        return None;
    }
    let key = submod.key.as_ptr() as usize;
    if let Some(cached) = DEFAULT_OBJECT_SINGLETONS.with(|m| m.borrow().get(&key).copied()) {
        return Some(f64::from_bits(JSValue::pointer(cached as *const u8).bits()));
    }

    let extra_fields = u32::from(submod.key == "fs_promises");
    let obj = js_object_alloc(0, submod.exports.len() as u32 + extra_fields);
    for spec in submod.exports {
        set_named_value(obj, spec.name, submodule_export_value(submod, spec));
    }
    if submod.key == "fs_promises" {
        set_named_value(obj, "constants", fs_promises_constants_value());
    }

    DEFAULT_OBJECT_SINGLETONS.with(|m| {
        m.borrow_mut().insert(key, obj);
    });
    ANY_SINGLETON_ALLOCATED.store(1, Ordering::Release);
    Some(f64::from_bits(JSValue::pointer(obj as *const u8).bits()))
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

pub(crate) fn is_diagnostics_bounded_channel_constructor_value(value: f64) -> bool {
    let js_value = JSValue::from_bits(value.to_bits());
    if !js_value.is_pointer() {
        return false;
    }
    let ptr = js_value.as_pointer::<ClosureHeader>() as *mut ClosureHeader;
    let Some(submod) = find_submodule("diagnostics_channel") else {
        return false;
    };
    let Some(export) = find_export(submod, "BoundedChannel") else {
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
    let field_count = submod.exports.len() as u32
        + u32::from(submod.key == "fs_promises")
        + u32::from(submodule_has_default_object(submod.key));
    let obj = js_object_alloc(0, field_count);
    // Populate fields. Each export's value is the singleton closure
    // pointer NaN-boxed as POINTER. We route through
    // `js_object_set_field_by_name` so the keys array gets built up
    // identically to what user code's literal object init would
    // produce — that's what `js_object_keys` / spread / Reflect.ownKeys
    // walks at runtime.
    for spec in submod.exports {
        set_named_value(obj, spec.name, submodule_export_value(submod, spec));
    }
    if submod.key == "fs_promises" {
        set_named_value(obj, "constants", fs_promises_constants_value());
    }
    if submod.key == "stream_promises" {
        let value = value_from_ptr(obj as *const u8);
        let name = b"default";
        let name_header = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        unsafe {
            crate::object::js_object_set_field_by_name(obj, name_header, value);
        }
    }
    if submod.key == "timers" {
        let value = crate::object::timers_promises_parent_namespace();
        let name = b"promises";
        let name_header = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, name_header, value);
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
    if let Some(default_value) = submodule_default_object_value(submod) {
        set_named_value(obj, "default", default_value);
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
    DEFAULT_OBJECT_SINGLETONS.with(|m| {
        for obj_ptr in m.borrow_mut().values_mut() {
            visitor.visit_raw_mut_ptr_slot(obj_ptr);
        }
    });
    TIMERS_PROMISES_SCHEDULER_OBJECT.with(|slot| {
        if let Some(ptr) = slot.borrow_mut().as_mut() {
            visitor.visit_raw_mut_ptr_slot(ptr);
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
                if let StoreTransform::Callable(t) = transform {
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
    diagnostics_tail::scan_diagnostics_tail_roots_mut(visitor);
    trace_events::scan_trace_events_roots_mut(visitor);
    test::scan_test_module_roots_mut(visitor);
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
    if submod.key == "fs_promises" && name == "constants" {
        return fs_promises_constants_value();
    }
    if submod.key == "trace_events" && name == "default" {
        let obj = ensure_namespace_singleton(submod);
        let name_header = js_string_from_bytes(b"default".as_ptr(), 7);
        let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, name_header);
        return value;
    }
    if submod.key == "test_reporters" && name == "default" {
        if let Some(value) = submodule_default_object_value(submod) {
            return value;
        }
    }
    if name == "default" {
        if let Some(value) = submodule_default_object_value(submod) {
            return value;
        }
    }
    if submod.key == "timers" && name == "promises" {
        return crate::object::timers_promises_parent_namespace();
    }
    if let Some(value) = special_export_value(submod.key, name) {
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
    // Devirt: dynamically-resolved submodules (require / getBuiltinModule / a
    // `fs.promises`-style property access) had no codegen install — run the
    // install-all hook so the registry is populated. Null/no-op unless armed.
    run_submod_install_all_hook();
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
            return sys_util_default_value();
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
    if submod.key == "fs_promises" && name == "constants" {
        return fs_promises_constants_value();
    }
    if submod.key == "trace_events" && name == "default" {
        let obj = ensure_namespace_singleton(submod);
        let name_header = js_string_from_bytes(b"default".as_ptr(), 7);
        let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, name_header);
        return value;
    }
    if submod.key == "test_reporters" && name == "default" {
        if let Some(value) = submodule_default_object_value(submod) {
            return value;
        }
    }
    if name == "default" {
        if let Some(value) = submodule_default_object_value(submod) {
            return value;
        }
    }
    if submod.key == "timers" && name == "promises" {
        return crate::object::timers_promises_parent_namespace();
    }
    if let Some(value) = special_export_value(submod.key, name) {
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
    run_submod_install_all_hook();
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
mod tests;
