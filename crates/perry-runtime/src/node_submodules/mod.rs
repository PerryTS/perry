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
use std::sync::atomic::{AtomicI64, AtomicPtr, Ordering};

use crate::closure::{
    js_closure_alloc, js_closure_call0, js_closure_call1, js_closure_call2, js_closure_call_array,
    js_closure_get_capture_f64, js_closure_get_capture_ptr, js_closure_set_capture_f64,
    js_closure_set_capture_ptr, js_register_closure_arity, ClosureHeader,
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
    let promise = crate::promise::js_promise_new();
    crate::promise::js_promise_resolve(promise, value);
    f64::from_bits(JSValue::pointer(promise as *const u8).bits())
}

fn promise_rejected(reason: f64) -> f64 {
    let promise = crate::promise::js_promise_rejected(reason);
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

#[inline]
fn undefined_value() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[inline]
fn value_from_ptr(ptr: *const u8) -> f64 {
    f64::from_bits(JSValue::pointer(ptr).bits())
}

#[inline]
fn raw_ptr_from_value(value: f64) -> usize {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);
    if jsval.is_pointer() || jsval.is_string() || jsval.is_bigint() {
        return (bits & crate::value::POINTER_MASK) as usize;
    }
    if bits != 0 && bits < 0x0001_0000_0000_0000 {
        return bits as usize;
    }
    0
}

#[inline]
unsafe fn gc_type_for_ptr(raw: usize) -> Option<u8> {
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let header = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    let gc_type = (*header).obj_type;
    if gc_type <= crate::gc::GC_TYPE_MAX {
        Some(gc_type)
    } else {
        None
    }
}

fn object_ptr_from_value(value: f64) -> Option<*mut ObjectHeader> {
    let raw = raw_ptr_from_value(value);
    if raw < 0x10000 || crate::buffer::is_registered_buffer(raw) {
        return None;
    }
    unsafe {
        if gc_type_for_ptr(raw) != Some(crate::gc::GC_TYPE_OBJECT) {
            return None;
        }
    }
    Some(raw as *mut ObjectHeader)
}

fn get_object_property(value: f64, name: &[u8]) -> Option<f64> {
    let obj = object_ptr_from_value(value)?;
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, key);
    if JSValue::from_bits(value.to_bits()).is_undefined() {
        None
    } else {
        Some(value)
    }
}

fn options_signal(options: f64) -> Option<f64> {
    let jsval = JSValue::from_bits(options.to_bits());
    if jsval.is_undefined() || jsval.is_null() {
        return None;
    }
    get_object_property(options, b"signal")
}

fn signal_aborted(signal: f64) -> bool {
    get_object_property(signal, b"aborted").is_some_and(|v| crate::value::js_is_truthy(v) != 0)
}

fn abort_error_value() -> f64 {
    let msg = b"AbortError";
    let header = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_error_new_with_message(header);
    value_from_ptr(err as *const u8)
}

fn signal_reason(signal: f64) -> f64 {
    match get_object_property(signal, b"reason") {
        Some(reason) if !JSValue::from_bits(reason.to_bits()).is_undefined() => reason,
        _ => abort_error_value(),
    }
}

extern "C" fn stream_promises_abort_listener(closure: *const ClosureHeader) -> f64 {
    let promise_value = js_closure_get_capture_f64(closure, 0);
    let signal = js_closure_get_capture_f64(closure, 1);
    let promise =
        crate::value::js_nanbox_get_pointer(promise_value) as *mut crate::promise::Promise;
    crate::promise::js_promise_reject(promise, signal_reason(signal));
    undefined_value()
}

fn promise_value_from_ptr(promise: *mut crate::promise::Promise) -> f64 {
    value_from_ptr(promise as *const u8)
}

fn register_abort_listener(signal: f64, promise: *mut crate::promise::Promise) {
    let Some(signal_obj) = object_ptr_from_value(signal) else {
        return;
    };
    let closure = js_closure_alloc(stream_promises_abort_listener as *const u8, 2);
    js_closure_set_capture_f64(closure, 0, promise_value_from_ptr(promise));
    js_closure_set_capture_f64(closure, 1, signal);
    let event = b"abort";
    let event_str = js_string_from_bytes(event.as_ptr(), event.len() as u32);
    let event_value = f64::from_bits(JSValue::string_ptr(event_str).bits());
    let listener_value = value_from_ptr(closure as *const u8);
    crate::url::js_abort_signal_add_listener(signal_obj, event_value, listener_value);
}

fn pending_abortable_promise(signal: f64) -> f64 {
    let promise = crate::promise::js_promise_new();
    register_abort_listener(signal, promise);
    promise_value_from_ptr(promise)
}

fn invoke_destination_method(destination: f64, method: &[u8], args: &[f64]) -> f64 {
    let Some(func) = get_object_property(destination, method) else {
        return undefined_value();
    };
    let prev_this = crate::object::js_implicit_this_set(destination);
    let result = unsafe { crate::closure::js_native_call_value(func, args.as_ptr(), args.len()) };
    crate::object::js_implicit_this_set(prev_this);
    result
}

fn write_chunks_to_destination(destination: f64, chunks: &[f64]) {
    let undef = undefined_value();
    for chunk in chunks {
        let args = [*chunk, undef];
        let _ = invoke_destination_method(destination, b"write", &args);
    }
    let end_args = [undef];
    let _ = invoke_destination_method(destination, b"end", &end_args);
}

extern "C" fn thunk_streamP_pipeline(
    _closure: *const ClosureHeader,
    source: f64,
    destination: f64,
    options: f64,
) -> f64 {
    let signal = options_signal(options);
    if let Some(signal) = signal {
        if signal_aborted(signal) {
            return promise_rejected(signal_reason(signal));
        }
    }

    match crate::node_stream::js_node_stream_readable_chunks_result(source) {
        Err(err) => promise_rejected(err),
        Ok(Some(chunks)) => {
            write_chunks_to_destination(destination, &chunks);
            if let Some(signal) = signal {
                if signal_aborted(signal) {
                    return promise_rejected(signal_reason(signal));
                }
            }
            promise_undefined()
        }
        Ok(None) => {
            if let Some(signal) = signal {
                pending_abortable_promise(signal)
            } else if let Some(err) =
                crate::node_stream::js_node_stream_hidden_error_after_read(source)
            {
                promise_rejected(err)
            } else {
                promise_undefined()
            }
        }
    }
}

extern "C" fn thunk_streamP_finished(
    _closure: *const ClosureHeader,
    stream: f64,
    options: f64,
) -> f64 {
    if let Some(signal) = options_signal(options) {
        if signal_aborted(signal) {
            return promise_rejected(signal_reason(signal));
        }
        if let Some(err) = crate::node_stream::js_node_stream_hidden_error_after_read(stream) {
            return promise_rejected(err);
        }
        if crate::node_stream::js_node_stream_is_stub_ended_after_read(stream) {
            return promise_undefined();
        }
        return pending_abortable_promise(signal);
    }

    if let Some(err) = crate::node_stream::js_node_stream_hidden_error_after_read(stream) {
        promise_rejected(err)
    } else {
        promise_undefined()
    }
}

fn buffer_from_bytes(
    bytes: &[u8],
    mark_array_buffer: bool,
    mark_uint8_array: bool,
) -> *mut crate::buffer::BufferHeader {
    let buf = crate::buffer::buffer_alloc(bytes.len() as u32);
    unsafe {
        (*buf).length = bytes.len() as u32;
        if !bytes.is_empty() {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                crate::buffer::buffer_data_mut(buf),
                bytes.len(),
            );
        }
    }
    if mark_array_buffer {
        crate::buffer::mark_as_array_buffer(buf as usize);
    }
    if mark_uint8_array {
        crate::buffer::mark_as_uint8array(buf as usize);
    }
    buf
}

fn bytes_to_buffer_value(bytes: &[u8]) -> f64 {
    let buf = buffer_from_bytes(bytes, false, false);
    f64::from_bits(JSValue::pointer(buf as *const u8).bits())
}

fn bytes_to_array_buffer_value(bytes: &[u8]) -> f64 {
    let buf = buffer_from_bytes(bytes, true, false);
    f64::from_bits(JSValue::pointer(buf as *const u8).bits())
}

fn bytes_to_uint8_array_value(bytes: &[u8]) -> f64 {
    let buf = buffer_from_bytes(bytes, false, true);
    f64::from_bits(JSValue::pointer(buf as *const u8).bits())
}

fn bytes_to_text_value(bytes: &[u8]) -> f64 {
    let cow = String::from_utf8_lossy(bytes);
    let ptr = js_string_from_bytes(cow.as_bytes().as_ptr(), cow.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

#[derive(Clone, Copy)]
enum ConsumerKind {
    Text = 0,
    Json = 1,
    Buffer = 2,
    ArrayBuffer = 3,
    Bytes = 4,
    Blob = 5,
}

impl ConsumerKind {
    fn from_i64(value: i64) -> Self {
        match value {
            1 => Self::Json,
            2 => Self::Buffer,
            3 => Self::ArrayBuffer,
            4 => Self::Bytes,
            5 => Self::Blob,
            _ => Self::Text,
        }
    }

    fn chunk_mode(self) -> ChunkMode {
        match self {
            Self::Text | Self::Json => ChunkMode::Text,
            Self::Buffer | Self::ArrayBuffer | Self::Bytes | Self::Blob => ChunkMode::Binary,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChunkMode {
    Binary,
    Text,
}

#[derive(Clone, Copy)]
enum CollectMethod {
    Next = 0,
    Read = 1,
}

type StreamGetReaderFn = unsafe extern "C" fn(f64) -> f64;
type StreamReaderReadFn = unsafe extern "C" fn(f64) -> *mut crate::Promise;

static STREAM_CONSUMER_GET_READER: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static STREAM_CONSUMER_READER_READ: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

#[no_mangle]
pub extern "C" fn js_register_stream_consumer_callbacks(
    get_reader: StreamGetReaderFn,
    reader_read: StreamReaderReadFn,
) {
    STREAM_CONSUMER_GET_READER.store(get_reader as *mut (), Ordering::Release);
    STREAM_CONSUMER_READER_READ.store(reader_read as *mut (), Ordering::Release);
}

impl CollectMethod {
    fn from_i64(value: i64) -> Self {
        if value == 1 {
            Self::Read
        } else {
            Self::Next
        }
    }

    fn name(self) -> &'static [u8] {
        match self {
            Self::Next => b"next",
            Self::Read => b"read",
        }
    }
}

fn boxed_pointer(ptr: *const u8) -> f64 {
    f64::from_bits(JSValue::pointer(ptr).bits())
}

fn is_integral_handle_value(value: f64) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    (jsval.is_int32() && jsval.as_int32() > 0)
        || (jsval.is_number() && value.is_finite() && value > 0.0 && value.fract() == 0.0)
}

fn is_undefined_value(value: f64) -> bool {
    value.to_bits() == crate::value::TAG_UNDEFINED
        || JSValue::from_bits(value.to_bits()).is_undefined()
}

fn raw_ptr_from_value(value: f64) -> usize {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);
    if jsval.is_pointer() || jsval.is_string() || jsval.is_bigint() {
        return (bits & crate::value::POINTER_MASK) as usize;
    }
    if bits != 0 && bits < 0x0001_0000_0000_0000 {
        return bits as usize;
    }
    0
}

unsafe fn gc_type_for_ptr(raw: usize) -> Option<u8> {
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let header = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    let gc_type = (*header).obj_type;
    if gc_type <= crate::gc::GC_TYPE_MAX {
        Some(gc_type)
    } else {
        None
    }
}

fn object_ptr_from_value(value: f64) -> Option<*const ObjectHeader> {
    let raw = raw_ptr_from_value(value);
    if raw < 0x10000 || crate::buffer::is_registered_buffer(raw) {
        return None;
    }
    unsafe {
        if gc_type_for_ptr(raw) != Some(crate::gc::GC_TYPE_OBJECT) {
            return None;
        }
    }
    Some(raw as *const ObjectHeader)
}

fn named_key(bytes: &[u8]) -> *const StringHeader {
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn get_named_value(value: f64, name: &[u8]) -> f64 {
    let Some(obj) = object_ptr_from_value(value) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    let key = named_key(name);
    js_object_get_field_by_name_f64(obj, key)
}

fn is_callable_value(value: f64) -> bool {
    let raw = raw_ptr_from_value(value);
    if raw < 0x10000 || crate::buffer::is_registered_buffer(raw) {
        return false;
    }
    unsafe {
        gc_type_for_ptr(raw) == Some(crate::gc::GC_TYPE_CLOSURE)
            && crate::closure::is_closure_ptr(raw)
    }
}

fn has_named_callable(value: f64, name: &[u8]) -> bool {
    is_callable_value(get_named_value(value, name))
}

fn invalid_chunk_error(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());
    let (kind, detail) = if jsval.is_int32() {
        ("number", format!(" ({})", jsval.as_int32()))
    } else if jsval.is_number() {
        ("number", format!(" ({})", value))
    } else if jsval.is_bool() {
        ("boolean", String::new())
    } else if jsval.is_undefined() {
        ("undefined", String::new())
    } else if jsval.is_null() {
        ("null", String::new())
    } else if is_callable_value(value) {
        ("function", String::new())
    } else {
        ("object", String::new())
    };
    let msg = format!(
        "The \"chunk\" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received type {}{}",
        kind, detail
    );
    let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    crate::node_submodules::register_error_code_pub(msg_ptr, "ERR_INVALID_ARG_TYPE");
    let err = crate::error::js_typeerror_new(msg_ptr);
    boxed_pointer(err as *const u8)
}

fn append_string_value_bytes(value: f64, out: &mut Vec<u8>) {
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    append_string_ptr_bytes(ptr, out);
}

fn append_string_ptr_bytes(ptr: *const StringHeader, out: &mut Vec<u8>) {
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return;
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        out.extend_from_slice(std::slice::from_raw_parts(data, len));
    }
}

fn append_buffer_value_bytes(raw: usize, out: &mut Vec<u8>) {
    if raw < 0x10000 || !crate::buffer::is_registered_buffer(raw) {
        return;
    }
    unsafe {
        let buf = raw as *const crate::buffer::BufferHeader;
        let len = (*buf).length as usize;
        let data = crate::buffer::buffer_data(buf);
        out.extend_from_slice(std::slice::from_raw_parts(data, len));
    }
}

fn append_number_chunk(value: f64, jsval: JSValue, out: &mut Vec<u8>) {
    let text = if jsval.is_int32() {
        jsval.as_int32().to_string()
    } else if value.is_finite() && value.fract() == 0.0 {
        (value as i64).to_string()
    } else {
        value.to_string()
    };
    out.extend_from_slice(text.as_bytes());
}

fn append_array_chunk_bytes(
    raw: usize,
    out: &mut Vec<u8>,
    mode: ChunkMode,
    depth: u8,
) -> Result<(), f64> {
    if raw < 0x10000 {
        return Ok(());
    }
    let arr = raw as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    for i in 0..len {
        let chunk = crate::array::js_array_get_f64(arr, i);
        append_chunk_bytes_for_consumer(chunk, out, mode, depth + 1)?;
    }
    Ok(())
}

fn append_chunk_bytes_for_consumer(
    value: f64,
    out: &mut Vec<u8>,
    mode: ChunkMode,
    depth: u8,
) -> Result<(), f64> {
    if depth > 16 {
        return Err(invalid_chunk_error(value));
    }
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_any_string() {
        append_string_value_bytes(value, out);
        return Ok(());
    }
    if jsval.is_int32() || (jsval.is_number() && value.is_finite()) {
        if mode == ChunkMode::Text {
            return Err(invalid_chunk_error(value));
        }
        append_number_chunk(value, jsval, out);
        return Ok(());
    }

    let raw = raw_ptr_from_value(value);
    if raw < 0x10000 {
        return Err(invalid_chunk_error(value));
    }
    if crate::buffer::is_registered_buffer(raw) {
        append_buffer_value_bytes(raw, out);
        return Ok(());
    }

    unsafe {
        match gc_type_for_ptr(raw) {
            Some(crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_LAZY_ARRAY) => {
                append_array_chunk_bytes(raw, out, mode, depth)
            }
            Some(crate::gc::GC_TYPE_OBJECT) => {
                if let Some(Ok(chunks)) =
                    crate::node_stream::js_node_stream_collect_chunks_result(value)
                {
                    append_chunk_bytes_for_consumer(chunks, out, mode, depth + 1)
                } else {
                    Err(invalid_chunk_error(value))
                }
            }
            Some(crate::gc::GC_TYPE_STRING) => {
                append_string_ptr_bytes(raw as *const StringHeader, out);
                Ok(())
            }
            _ => Err(invalid_chunk_error(value)),
        }
    }
}

fn chunks_to_bytes(chunks: f64, mode: ChunkMode) -> Result<Vec<u8>, f64> {
    let mut out = Vec::new();
    append_chunk_bytes_for_consumer(chunks, &mut out, mode, 0)?;
    Ok(out)
}

fn finish_consumer_from_chunks(kind: ConsumerKind, chunks: f64) -> Result<f64, f64> {
    let bytes = chunks_to_bytes(chunks, kind.chunk_mode())?;
    match kind {
        ConsumerKind::Text => Ok(bytes_to_text_value(&bytes)),
        ConsumerKind::Json => {
            let text = bytes_to_text_value(&bytes);
            let text_ptr = crate::value::js_get_string_pointer_unified(text) as *const StringHeader;
            unsafe { crate::json::js_json_parse_result(text_ptr).map(|v| f64::from_bits(v.bits())) }
        }
        ConsumerKind::Buffer => Ok(bytes_to_buffer_value(&bytes)),
        ConsumerKind::ArrayBuffer => Ok(bytes_to_array_buffer_value(&bytes)),
        ConsumerKind::Bytes => Ok(bytes_to_uint8_array_value(&bytes)),
        ConsumerKind::Blob => Ok(blob_value_from_bytes(&bytes)),
    }
}

fn promise_from_consumer_chunks(kind: ConsumerKind, chunks: Result<f64, f64>) -> f64 {
    match chunks.and_then(|chunks| finish_consumer_from_chunks(kind, chunks)) {
        Ok(value) => promise_value(value),
        Err(err) => promise_rejected(err),
    }
}

fn settle_consumer_from_chunks(promise: *mut crate::Promise, kind: ConsumerKind, chunks: f64) {
    if promise.is_null() {
        return;
    }
    match finish_consumer_from_chunks(kind, chunks) {
        Ok(value) => crate::promise::js_promise_resolve(promise, value),
        Err(err) => crate::promise::js_promise_reject(promise, err),
    }
}

fn promise_ptr_from_value(value: f64) -> Option<*mut crate::Promise> {
    if crate::promise::js_value_is_promise(value) == 0 {
        return None;
    }
    let raw = raw_ptr_from_value(value);
    if raw < 0x10000 {
        None
    } else {
        Some(raw as *mut crate::Promise)
    }
}

fn registered_readable_stream_reader(stream: f64) -> Option<f64> {
    if !is_integral_handle_value(stream) {
        return None;
    }
    let f = STREAM_CONSUMER_GET_READER.load(Ordering::Acquire);
    if f.is_null() {
        return None;
    }
    let reader = unsafe {
        let func: StreamGetReaderFn = std::mem::transmute(f);
        func(stream)
    };
    if is_undefined_value(reader) {
        None
    } else {
        Some(reader)
    }
}

fn registered_reader_read_promise(reader: f64) -> Option<*mut crate::Promise> {
    if !is_integral_handle_value(reader) {
        return None;
    }
    let f = STREAM_CONSUMER_READER_READ.load(Ordering::Acquire);
    if f.is_null() {
        return None;
    }
    Some(unsafe {
        let func: StreamReaderReadFn = std::mem::transmute(f);
        func(reader)
    })
}

fn call_collector_method(
    receiver: f64,
    method: CollectMethod,
    step: *const ClosureHeader,
    reject: *const ClosureHeader,
) {
    if let CollectMethod::Read = method {
        if let Some(promise) = registered_reader_read_promise(receiver) {
            crate::promise::js_promise_then(promise, step, reject);
            return;
        }
    }

    let name = method.name();
    let result = unsafe {
        crate::object::js_native_call_method(
            receiver,
            name.as_ptr() as *const i8,
            name.len(),
            std::ptr::null(),
            0,
        )
    };
    if let Some(promise) = promise_ptr_from_value(result) {
        crate::promise::js_promise_then(promise, step, reject);
    } else {
        consumer_collect_step(step, result);
    }
}

fn collect_by_method_promise(kind: ConsumerKind, receiver: f64, method: CollectMethod) -> f64 {
    let result_promise = crate::promise::js_promise_new();
    let result_arr = crate::array::js_array_alloc(0);
    let step = js_closure_alloc(consumer_collect_step as *const u8, 6);
    let reject = js_closure_alloc(consumer_collect_rejected as *const u8, 1);
    js_closure_set_capture_ptr(step, 0, result_promise as i64);
    js_closure_set_capture_ptr(step, 1, result_arr as i64);
    js_closure_set_capture_f64(step, 2, receiver);
    js_closure_set_capture_ptr(step, 3, reject as i64);
    js_closure_set_capture_ptr(step, 4, method as i64);
    js_closure_set_capture_ptr(step, 5, kind as i64);
    js_closure_set_capture_ptr(reject, 0, result_promise as i64);
    call_collector_method(receiver, method, step, reject);
    boxed_pointer(result_promise as *const u8)
}

fn call_symbol_async_iterator(stream: f64) -> Option<f64> {
    let sym = crate::symbol::well_known_symbol("asyncIterator");
    if sym.is_null() {
        return None;
    }
    let sym_f64 = boxed_pointer(sym as *const u8);
    let method = unsafe { crate::symbol::js_object_get_symbol_property(stream, sym_f64) };
    if !is_callable_value(method) {
        return None;
    }
    let prev_this = crate::object::js_implicit_this_set(stream);
    let iterator = unsafe { crate::closure::js_native_call_value(method, std::ptr::null(), 0) };
    crate::object::js_implicit_this_set(prev_this);
    if iterator.to_bits() == crate::value::TAG_UNDEFINED {
        None
    } else {
        Some(iterator)
    }
}

fn async_consumer_promise(kind: ConsumerKind, stream: f64) -> Option<f64> {
    if let Some(iterator) = call_symbol_async_iterator(stream) {
        if has_named_callable(iterator, b"next") {
            return Some(collect_by_method_promise(
                kind,
                iterator,
                CollectMethod::Next,
            ));
        }
    }
    if has_named_callable(stream, b"next") {
        return Some(collect_by_method_promise(kind, stream, CollectMethod::Next));
    }
    if has_named_callable(stream, b"getReader") {
        let reader = unsafe {
            crate::object::js_native_call_method(
                stream,
                b"getReader".as_ptr() as *const i8,
                b"getReader".len(),
                std::ptr::null(),
                0,
            )
        };
        if reader.to_bits() == crate::value::TAG_UNDEFINED {
            return Some(promise_rejected(invalid_chunk_error(stream)));
        }
        return Some(collect_by_method_promise(kind, reader, CollectMethod::Read));
    }
    if let Some(reader) = registered_readable_stream_reader(stream) {
        return Some(collect_by_method_promise(kind, reader, CollectMethod::Read));
    }
    None
}

fn consume_stream(kind: ConsumerKind, stream: f64) -> f64 {
    if let Some(chunks) = crate::node_stream::js_node_stream_collect_chunks_result(stream) {
        return promise_from_consumer_chunks(kind, chunks);
    }
    if let Some(promise) = async_consumer_promise(kind, stream) {
        return promise;
    }
    let empty = crate::array::js_array_alloc(0);
    promise_from_consumer_chunks(kind, Ok(boxed_pointer(empty as *const u8)))
}

extern "C" fn consumer_collect_rejected(closure: *const ClosureHeader, reason: f64) -> f64 {
    let promise = js_closure_get_capture_ptr(closure, 0) as *mut crate::Promise;
    crate::promise::js_promise_reject(promise, reason);
    0.0
}

extern "C" fn consumer_collect_step(closure: *const ClosureHeader, iter_result: f64) -> f64 {
    let promise = js_closure_get_capture_ptr(closure, 0) as *mut crate::Promise;
    let mut result_arr = js_closure_get_capture_ptr(closure, 1) as *mut crate::array::ArrayHeader;
    let receiver = js_closure_get_capture_f64(closure, 2);
    let reject = js_closure_get_capture_ptr(closure, 3) as *const ClosureHeader;
    let method = CollectMethod::from_i64(js_closure_get_capture_ptr(closure, 4));
    let kind = ConsumerKind::from_i64(js_closure_get_capture_ptr(closure, 5));
    if promise.is_null() || result_arr.is_null() {
        return 0.0;
    }

    let Some(result_obj) = object_ptr_from_value(iter_result) else {
        let arr_value = boxed_pointer(result_arr as *const u8);
        settle_consumer_from_chunks(promise, kind, arr_value);
        return 0.0;
    };

    let done = js_object_get_field_by_name_f64(result_obj, named_key(b"done"));
    if crate::value::js_is_truthy(done) != 0 {
        let arr_value = boxed_pointer(result_arr as *const u8);
        settle_consumer_from_chunks(promise, kind, arr_value);
        return 0.0;
    }

    let value = js_object_get_field_by_name_f64(result_obj, named_key(b"value"));
    result_arr = crate::array::js_array_push_f64(result_arr, value);
    js_closure_set_capture_ptr(closure as *mut ClosureHeader, 1, result_arr as i64);
    call_collector_method(receiver, method, closure, reject);
    0.0
}

const CLASS_ID_BLOB: u32 = 0xFFFF0026;

extern "C" fn blob_text_method(closure: *const ClosureHeader) -> f64 {
    let bytes = captured_blob_bytes(closure);
    promise_value(bytes_to_text_value(&bytes))
}

extern "C" fn blob_array_buffer_method(closure: *const ClosureHeader) -> f64 {
    let bytes = captured_blob_bytes(closure);
    promise_value(bytes_to_array_buffer_value(&bytes))
}

extern "C" fn blob_bytes_method(closure: *const ClosureHeader) -> f64 {
    let bytes = captured_blob_bytes(closure);
    promise_value(bytes_to_uint8_array_value(&bytes))
}

extern "C" fn blob_slice_method(
    closure: *const ClosureHeader,
    start: f64,
    end: f64,
    content_type: f64,
) -> f64 {
    let bytes = captured_blob_bytes(closure);
    let len = bytes.len() as i64;
    let normalize = |value: f64, default: i64| -> i64 {
        if value.is_nan() || value.to_bits() == crate::value::TAG_UNDEFINED {
            return default;
        }
        let n = value as i64;
        if n < 0 {
            (len + n).max(0)
        } else {
            n.min(len)
        }
    };
    let lo = normalize(start, 0);
    let hi = normalize(end, len);
    let (lo, hi) = if hi < lo { (lo, lo) } else { (lo, hi) };
    let content_type = string_from_value(content_type).unwrap_or_default();
    blob_value_from_bytes_and_type(&bytes[lo as usize..hi as usize], &content_type)
}

extern "C" fn blob_stream_method(closure: *const ClosureHeader) -> f64 {
    let bytes = captured_blob_bytes(closure);
    crate::node_stream::js_node_stream_readable_from(bytes_to_uint8_array_value(&bytes))
}

fn captured_blob_bytes(closure: *const ClosureHeader) -> Vec<u8> {
    let raw = js_closure_get_capture_ptr(closure, 0) as usize;
    if raw < 0x10000 || !crate::buffer::is_registered_buffer(raw) {
        return Vec::new();
    }
    unsafe {
        let buf = raw as *const crate::buffer::BufferHeader;
        let len = (*buf).length as usize;
        let data = crate::buffer::buffer_data(buf);
        std::slice::from_raw_parts(data, len).to_vec()
    }
}

fn set_named_value(obj: *mut ObjectHeader, name: &[u8], value: f64) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(obj, key, value);
}

#[allow(clippy::missing_transmute_annotations)]
fn blob_method_value(
    func: *const u8,
    arity: u32,
    backing: *mut crate::buffer::BufferHeader,
) -> f64 {
    js_register_closure_arity(func, arity);
    let closure = js_closure_alloc(func, 1);
    js_closure_set_capture_ptr(closure, 0, backing as i64);
    f64::from_bits(JSValue::pointer(closure as *const u8).bits())
}

fn blob_value_from_bytes(bytes: &[u8]) -> f64 {
    blob_value_from_bytes_and_type(bytes, "")
}

fn blob_value_from_bytes_and_type(bytes: &[u8], content_type: &str) -> f64 {
    let backing = buffer_from_bytes(bytes, false, false);
    let obj = js_object_alloc(CLASS_ID_BLOB, 7);
    set_named_value(obj, b"size", bytes.len() as f64);
    set_named_value(obj, b"type", bytes_to_text_value(content_type.as_bytes()));
    set_named_value(
        obj,
        b"text",
        blob_method_value(blob_text_method as *const u8, 0, backing),
    );
    set_named_value(
        obj,
        b"arrayBuffer",
        blob_method_value(blob_array_buffer_method as *const u8, 0, backing),
    );
    set_named_value(
        obj,
        b"bytes",
        blob_method_value(blob_bytes_method as *const u8, 0, backing),
    );
    set_named_value(
        obj,
        b"slice",
        blob_method_value(blob_slice_method as *const u8, 3, backing),
    );
    set_named_value(
        obj,
        b"stream",
        blob_method_value(blob_stream_method as *const u8, 0, backing),
    );
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

fn string_from_value(value: f64) -> Option<String> {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_any_string() {
        return None;
    }
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return None;
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        Some(String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned())
    }
}

extern "C" fn thunk_consumers_text(_closure: *const ClosureHeader, stream: f64) -> f64 {
    consume_stream(ConsumerKind::Text, stream)
}

extern "C" fn thunk_consumers_json(_closure: *const ClosureHeader, stream: f64) -> f64 {
    consume_stream(ConsumerKind::Json, stream)
}

extern "C" fn thunk_consumers_buffer(_closure: *const ClosureHeader, stream: f64) -> f64 {
    consume_stream(ConsumerKind::Buffer, stream)
}

extern "C" fn thunk_consumers_arrayBuffer(_closure: *const ClosureHeader, stream: f64) -> f64 {
    consume_stream(ConsumerKind::ArrayBuffer, stream)
}

extern "C" fn thunk_consumers_bytes(_closure: *const ClosureHeader, stream: f64) -> f64 {
    consume_stream(ConsumerKind::Bytes, stream)
}

extern "C" fn thunk_consumers_blob(_closure: *const ClosureHeader, stream: f64) -> f64 {
    consume_stream(ConsumerKind::Blob, stream)
}

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
    if submod.key == "stream_promises" {
        let value = value_from_ptr(obj as *const u8);
        let name = b"default";
        let name_header = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        unsafe {
            crate::object::js_object_set_field_by_name(obj, name_header, value);
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
    if submod.key == "stream_promises" && name == "default" {
        let obj = ensure_namespace_singleton(submod);
        return f64::from_bits(JSValue::pointer(obj as *const u8).bits());
    }
    let export = match find_export(submod, name) {
        Some(e) => e,
        None => return f64::from_bits(JSValue::bool(true).bits()),
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
    let obj = ensure_namespace_singleton(submod);
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

#[cfg(test)]
mod tests {
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
    fn stream_promises_finished_resolves_for_clean_stub_stream() {
        let stream = crate::node_stream::js_node_stream_passthrough_new(undefined_value());
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
