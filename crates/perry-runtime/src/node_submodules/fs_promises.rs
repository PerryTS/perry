//! `node:fs/promises` thunks + shared Promise-construction helpers, plus the
//! `node:readline/promises` not-yet-implemented stubs.
//!
//! Extracted from `mod.rs` so the parent module stays under the file-size
//! gate. Pure code movement — no logic changes. The `promise_value` /
//! `promise_rejected` / `promise_undefined` helpers are `pub(crate)` because
//! the stream/promises, stream/consumers, and blob modules build their
//! resolved/rejected Promises through them.

use crate::closure::{
    get_valid_func_ptr, js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64,
    js_native_call_value, js_register_closure_arity, ClosureHeader,
};
use crate::object::{
    js_object_alloc, js_object_get_field_by_name_f64, js_object_set_field_by_name, ObjectHeader,
};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::url::abort::abort_signal_ptr_from_value;
use crate::value::{js_jsvalue_to_string, JSValue};
use std::os::raw::c_int;

pub(crate) fn promise_value(value: f64) -> f64 {
    let promise = crate::promise::js_promise_new();
    crate::promise::js_promise_resolve(promise, value);
    f64::from_bits(JSValue::pointer(promise as *const u8).bits())
}

pub(crate) fn promise_rejected(reason: f64) -> f64 {
    let promise = crate::promise::js_promise_rejected(reason);
    f64::from_bits(JSValue::pointer(promise as *const u8).bits())
}

pub(crate) fn promise_undefined() -> f64 {
    promise_value(f64::from_bits(crate::value::TAG_UNDEFINED))
}

fn catch_fs_promises_throw(call: impl FnOnce() -> f64) -> Result<f64, f64> {
    let trap_buf = crate::exception::js_try_push();
    let jumped = unsafe { crate::ffi::setjmp::setjmp(trap_buf as *mut c_int) };
    if jumped == 0 {
        let value = call();
        crate::exception::js_try_end();
        Ok(value)
    } else {
        let err = crate::exception::js_get_exception();
        crate::exception::js_clear_exception();
        crate::exception::js_try_end();
        Err(err)
    }
}

fn promise_from_sync_value(call: impl FnOnce() -> f64) -> f64 {
    match catch_fs_promises_throw(call) {
        Ok(value) => promise_value(value),
        Err(err) => promise_rejected(err),
    }
}

fn promise_from_sync_undefined(call: impl FnOnce()) -> f64 {
    match catch_fs_promises_throw(|| {
        call();
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }) {
        Ok(_) => promise_undefined(),
        Err(err) => promise_rejected(err),
    }
}

fn promise_from_result_undefined(call: impl FnOnce() -> Result<(), f64>) -> f64 {
    match catch_fs_promises_throw(|| match call() {
        Ok(()) => promise_undefined(),
        Err(err_val) => promise_rejected(err_val),
    }) {
        Ok(promise) => promise,
        Err(err) => promise_rejected(err),
    }
}

fn promise_from_result_value(call: impl FnOnce() -> Result<f64, f64>) -> f64 {
    match catch_fs_promises_throw(|| match call() {
        Ok(value) => promise_value(value),
        Err(err_val) => promise_rejected(err_val),
    }) {
        Ok(promise) => promise,
        Err(err) => promise_rejected(err),
    }
}

#[no_mangle]
pub extern "C" fn js_fs_promises_read_file(path: f64, options: f64) -> f64 {
    thunk_fs_promises_readFile(std::ptr::null(), path, options)
}

#[no_mangle]
pub extern "C" fn js_fs_promises_write_file(path: f64, data: f64, options: f64) -> f64 {
    thunk_fs_promises_writeFile(std::ptr::null(), path, data, options)
}

#[no_mangle]
pub extern "C" fn js_fs_promises_append_file(path: f64, data: f64, options: f64) -> f64 {
    thunk_fs_promises_appendFile(std::ptr::null(), path, data, options)
}

#[no_mangle]
pub extern "C" fn js_fs_promises_mkdir(path: f64, options: f64) -> f64 {
    thunk_fs_promises_mkdir(std::ptr::null(), path, options)
}

pub(crate) extern "C" fn thunk_fs_promises_readFile(
    _closure: *const ClosureHeader,
    path: f64,
    encoding: f64,
) -> f64 {
    promise_from_sync_value(|| crate::fs::js_fs_read_file_dispatch(path, encoding))
}

pub(crate) extern "C" fn thunk_fs_promises_open(
    _closure: *const ClosureHeader,
    path: f64,
    flags: f64,
    _mode: f64,
) -> f64 {
    match catch_fs_promises_throw(|| {
        match unsafe { crate::fs::js_fs_filehandle_open_result(path, flags) } {
            Ok(handle) => promise_value(handle),
            Err(err_val) => promise_rejected(err_val),
        }
    }) {
        Ok(promise) => promise,
        Err(err) => promise_rejected(err),
    }
}

pub(crate) extern "C" fn thunk_fs_promises_writeFile(
    _closure: *const ClosureHeader,
    path: f64,
    data: f64,
    options: f64,
) -> f64 {
    match catch_fs_promises_throw(|| {
        match unsafe { crate::fs::write_file_path_or_fd_result(path, data, options) } {
            Ok(()) => promise_undefined(),
            Err(err) => promise_rejected(err),
        }
    }) {
        Ok(promise) => promise,
        Err(err) => promise_rejected(err),
    }
}

pub(crate) extern "C" fn thunk_fs_promises_appendFile(
    _closure: *const ClosureHeader,
    path: f64,
    data: f64,
    options: f64,
) -> f64 {
    promise_from_sync_undefined(|| {
        let _ = crate::fs::js_fs_append_file_sync_options(path, data, options);
    })
}

pub(crate) extern "C" fn thunk_fs_promises_chmod(
    _closure: *const ClosureHeader,
    path: f64,
    mode: f64,
) -> f64 {
    promise_from_result_undefined(|| unsafe { crate::fs::js_fs_chmod_result(path, mode) })
}

pub(crate) extern "C" fn thunk_fs_promises_chown(
    _closure: *const ClosureHeader,
    path: f64,
    uid: f64,
    gid: f64,
) -> f64 {
    promise_from_result_undefined(|| unsafe { crate::fs::js_fs_chown_result(path, uid, gid, true) })
}

pub(crate) extern "C" fn thunk_fs_promises_lchown(
    _closure: *const ClosureHeader,
    path: f64,
    uid: f64,
    gid: f64,
) -> f64 {
    promise_from_result_undefined(|| unsafe {
        crate::fs::js_fs_chown_result(path, uid, gid, false)
    })
}

pub(crate) extern "C" fn thunk_fs_promises_lchmod(
    _closure: *const ClosureHeader,
    path: f64,
    mode: f64,
) -> f64 {
    if !crate::fs::lchmod_is_callable_on_this_platform() {
        let _ = (path, mode);
        let message = "The lchmod() method is not implemented";
        let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
        crate::node_submodules::register_error_code_pub(msg, "ERR_METHOD_NOT_IMPLEMENTED");
        let err = crate::error::js_error_new_with_message(msg);
        return promise_rejected(crate::value::js_nanbox_pointer(err as i64));
    }
    promise_from_result_undefined(|| unsafe { crate::fs::js_fs_lchmod_result(path, mode) })
}

pub(crate) extern "C" fn thunk_fs_promises_mkdir(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    promise_from_sync_undefined(|| {
        let _ = crate::fs::js_fs_mkdir_sync_options(path, options);
    })
}

pub(crate) extern "C" fn thunk_fs_promises_readdir(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    promise_from_sync_value(|| {
        let raw = crate::fs::js_fs_readdir_sync(path, options);
        f64::from_bits(JSValue::pointer(raw.to_bits() as *const u8).bits())
    })
}

pub(crate) extern "C" fn thunk_fs_promises_stat(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    promise_from_sync_value(|| crate::fs::js_fs_stat_sync_options(path, options))
}

pub(crate) extern "C" fn thunk_fs_promises_statfs(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    promise_from_sync_value(|| crate::fs::js_fs_statfs_sync_options(path, options))
}

pub(crate) extern "C" fn thunk_fs_promises_lstat(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    promise_from_sync_value(|| crate::fs::js_fs_lstat_sync_options(path, options))
}

pub(crate) extern "C" fn thunk_fs_promises_rm(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    promise_from_result_undefined(|| unsafe { crate::fs::js_fs_rm_result(path, options) })
}

pub(crate) extern "C" fn thunk_fs_promises_rmdir(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    promise_from_result_undefined(|| unsafe { crate::fs::js_fs_rmdir_result(path, options) })
}

pub(crate) extern "C" fn thunk_fs_promises_unlink(
    _closure: *const ClosureHeader,
    path: f64,
) -> f64 {
    promise_from_result_undefined(|| unsafe { crate::fs::js_fs_unlink_result(path) })
}

pub(crate) extern "C" fn thunk_fs_promises_rename(
    _closure: *const ClosureHeader,
    from: f64,
    to: f64,
) -> f64 {
    promise_from_result_undefined(|| unsafe { crate::fs::js_fs_rename_result(from, to) })
}

pub(crate) extern "C" fn thunk_fs_promises_copyFile(
    _closure: *const ClosureHeader,
    from: f64,
    to: f64,
    flags: f64,
) -> f64 {
    promise_from_result_undefined(|| unsafe { crate::fs::js_fs_copy_file_result(from, to, flags) })
}

pub(crate) extern "C" fn thunk_fs_promises_cp(
    _closure: *const ClosureHeader,
    from: f64,
    to: f64,
    options: f64,
) -> f64 {
    promise_from_result_undefined(|| crate::fs::js_fs_cp_async_result(from, to, options))
}

pub(crate) extern "C" fn thunk_fs_promises_truncate(
    _closure: *const ClosureHeader,
    path: f64,
    len: f64,
) -> f64 {
    promise_from_result_undefined(|| unsafe { crate::fs::js_fs_truncate_result(path, len) })
}

pub(crate) extern "C" fn thunk_fs_promises_utimes(
    _closure: *const ClosureHeader,
    path: f64,
    atime: f64,
    mtime: f64,
) -> f64 {
    promise_from_result_undefined(|| unsafe {
        crate::fs::js_fs_utimes_result(path, atime, mtime, false)
    })
}

pub(crate) extern "C" fn thunk_fs_promises_lutimes(
    _closure: *const ClosureHeader,
    path: f64,
    atime: f64,
    mtime: f64,
) -> f64 {
    promise_from_result_undefined(|| unsafe {
        crate::fs::js_fs_utimes_result(path, atime, mtime, true)
    })
}

pub(crate) extern "C" fn thunk_fs_promises_link(
    _closure: *const ClosureHeader,
    from: f64,
    to: f64,
) -> f64 {
    promise_from_result_undefined(|| unsafe { crate::fs::js_fs_link_result(from, to) })
}

pub(crate) extern "C" fn thunk_fs_promises_symlink(
    _closure: *const ClosureHeader,
    target: f64,
    path: f64,
    _type: f64,
) -> f64 {
    promise_from_result_undefined(|| unsafe { crate::fs::js_fs_symlink_result(target, path) })
}

pub(crate) extern "C" fn thunk_fs_promises_readlink(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    promise_from_result_value(|| crate::fs::js_fs_readlink_value_result(path, options))
}

pub(crate) extern "C" fn thunk_fs_promises_realpath(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    promise_from_sync_value(|| crate::fs::js_fs_realpath_promises_dispatch(path, options))
}

pub(crate) extern "C" fn thunk_fs_promises_mkdtemp(
    _closure: *const ClosureHeader,
    prefix: f64,
    options: f64,
) -> f64 {
    promise_from_sync_value(|| crate::fs::js_fs_mkdtemp_dispatch(prefix, options))
}

pub(crate) extern "C" fn thunk_fs_promises_mkdtempDisposable(
    _closure: *const ClosureHeader,
    prefix: f64,
    options: f64,
) -> f64 {
    promise_from_sync_value(|| crate::fs::js_fs_mkdtemp_disposable_object(prefix, options, true))
}

pub(crate) extern "C" fn thunk_fs_promises_opendir(
    _closure: *const ClosureHeader,
    path: f64,
) -> f64 {
    promise_from_result_value(|| crate::fs::js_fs_opendir_value_with_path(path))
}

pub(crate) extern "C" fn thunk_fs_promises_glob(
    _closure: *const ClosureHeader,
    pattern: f64,
    options: f64,
) -> f64 {
    crate::fs::js_fs_promises_glob_iterator(pattern, options)
}

pub(crate) extern "C" fn thunk_fs_promises_watch(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    crate::fs::js_fs_promises_watch(path, options)
}

pub(crate) extern "C" fn thunk_fs_promises_access(
    _closure: *const ClosureHeader,
    path: f64,
    mode: f64,
) -> f64 {
    promise_from_result_undefined(|| unsafe { crate::fs::js_fs_access_result(path, mode) })
}

const RL_IF_INPUT: &[u8] = b"__perryReadlinePromisesInput";
const RL_IF_OUTPUT: &[u8] = b"__perryReadlinePromisesOutput";
const RL_IF_PENDING_TEXT: &[u8] = b"__perryReadlinePromisesPendingText";
const RL_IF_PENDING_PROMISE: &[u8] = b"__perryReadlinePromisesPendingPromise";
const RL_IF_ABORT_SIGNAL: &[u8] = b"__perryReadlinePromisesAbortSignal";
const RL_IF_ABORT_LISTENER: &[u8] = b"__perryReadlinePromisesAbortListener";
const RL_IF_CLOSED: &[u8] = b"__perryReadlinePromisesClosed";

const RL_ACTION_OUTPUT: &[u8] = b"__perryReadlinePromisesActionOutput";
const RL_ACTION_QUEUE: &[u8] = b"__perryReadlinePromisesActionQueue";
const RL_ACTION_AUTO: &[u8] = b"__perryReadlinePromisesActionAutoCommit";
const RL_ACTION_AUTO_PENDING: &[u8] = b"__perryReadlinePromisesActionAutoPending";

fn undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn null_value() -> f64 {
    f64::from_bits(JSValue::null().bits())
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(JSValue::bool(value).bits())
}

fn boxed_str(bytes: &[u8]) -> f64 {
    let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn string_header_to_string(ptr: *const StringHeader) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

fn value_to_string(value: f64) -> String {
    string_header_to_string(js_jsvalue_to_string(value) as *const StringHeader)
}

fn object_ptr_from_value(value: f64) -> Option<*mut ObjectHeader> {
    let js = JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        return None;
    }
    let ptr = js.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
    if (ptr as usize) < 0x10000 {
        None
    } else {
        Some(ptr)
    }
}

fn raw_ptr_from_value(value: f64) -> Option<i64> {
    let js = JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        return None;
    }
    let raw = js.as_pointer::<u8>() as i64;
    if raw >= 0x10000 {
        Some(raw)
    } else {
        None
    }
}

fn key_ptr(key: &[u8]) -> *mut StringHeader {
    js_string_from_bytes(key.as_ptr(), key.len() as u32)
}

fn object_field(value: f64, key: &[u8]) -> Option<f64> {
    let obj = object_ptr_from_value(value)?;
    let field = js_object_get_field_by_name_f64(obj as *const ObjectHeader, key_ptr(key));
    if JSValue::from_bits(field.to_bits()).is_undefined() {
        None
    } else {
        Some(field)
    }
}

fn set_object_field_value(obj: *mut ObjectHeader, key: &[u8], value: f64) {
    js_object_set_field_by_name(obj, key_ptr(key), value);
}

fn set_value_field(object: f64, key: &[u8], value: f64) {
    if let Some(obj) = object_ptr_from_value(object) {
        set_object_field_value(obj, key, value);
    }
}

fn is_undefined_value(value: f64) -> bool {
    JSValue::from_bits(value.to_bits()).is_undefined()
}

fn is_null_or_undefined(value: f64) -> bool {
    let js = JSValue::from_bits(value.to_bits());
    js.is_null() || js.is_undefined()
}

fn is_true_value(value: f64) -> bool {
    let js = JSValue::from_bits(value.to_bits());
    js.is_bool() && js.as_bool()
}

fn is_false_value(value: f64) -> bool {
    let js = JSValue::from_bits(value.to_bits());
    js.is_bool() && !js.as_bool()
}

fn is_callable(value: f64) -> bool {
    raw_ptr_from_value(value)
        .map(|raw| !get_valid_func_ptr(raw as *const ClosureHeader).is_null())
        .unwrap_or(false)
}

fn stream_is_readable(value: f64) -> bool {
    is_true_value(crate::node_stream::js_node_stream_is_readable(value))
}

fn stream_is_writable(value: f64) -> bool {
    is_true_value(crate::node_stream::js_node_stream_is_writable(value))
}

fn call_write_value(output: f64, text: &str) {
    if text.is_empty() {
        return;
    }
    let chunk = boxed_str(text.as_bytes());
    if stream_is_writable(output) {
        if let Some(raw) = raw_ptr_from_value(output) {
            let _ = crate::node_stream::js_node_stream_method_write(
                raw,
                chunk,
                undefined(),
                undefined(),
            );
            return;
        }
    }
    if let Some(write) = object_field(output, b"write").filter(|v| is_callable(*v)) {
        let args = [chunk];
        unsafe {
            let _ = js_native_call_value(write, args.as_ptr(), args.len());
        }
    }
}

fn promise_ptr_from_value(value: f64) -> Option<*mut crate::promise::Promise> {
    raw_ptr_from_value(value).map(|raw| raw as *mut crate::promise::Promise)
}

fn readline_bound_method0(
    func: extern "C" fn(*const ClosureHeader) -> f64,
    this_value: f64,
) -> f64 {
    js_register_closure_arity(func as *const u8, 0);
    let closure = js_closure_alloc(func as *const u8, 1);
    js_closure_set_capture_f64(closure, 0, this_value);
    f64::from_bits(JSValue::pointer(closure as *const u8).bits())
}

fn readline_bound_method1(
    func: extern "C" fn(*const ClosureHeader, f64) -> f64,
    this_value: f64,
) -> f64 {
    js_register_closure_arity(func as *const u8, 1);
    let closure = js_closure_alloc(func as *const u8, 1);
    js_closure_set_capture_f64(closure, 0, this_value);
    f64::from_bits(JSValue::pointer(closure as *const u8).bits())
}

fn readline_bound_method2(
    func: extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
    this_value: f64,
) -> f64 {
    js_register_closure_arity(func as *const u8, 2);
    let closure = js_closure_alloc(func as *const u8, 1);
    js_closure_set_capture_f64(closure, 0, this_value);
    f64::from_bits(JSValue::pointer(closure as *const u8).bits())
}

fn this_value(closure: *const ClosureHeader) -> f64 {
    js_closure_get_capture_f64(closure, 0)
}

fn cleanup_question_abort_listener(interface: f64) {
    let signal = object_field(interface, RL_IF_ABORT_SIGNAL).unwrap_or_else(undefined);
    let listener = object_field(interface, RL_IF_ABORT_LISTENER).unwrap_or_else(undefined);
    if !is_undefined_value(signal) && !is_undefined_value(listener) {
        if let Some(signal_ptr) = abort_signal_ptr_from_value(signal) {
            crate::url::js_abort_signal_remove_listener(signal_ptr, boxed_str(b"abort"), listener);
        }
    }
    set_value_field(interface, RL_IF_ABORT_SIGNAL, undefined());
    set_value_field(interface, RL_IF_ABORT_LISTENER, undefined());
}

fn take_pending_question_promise(interface: f64) -> Option<*mut crate::promise::Promise> {
    let promise_value = object_field(interface, RL_IF_PENDING_PROMISE)?;
    if is_undefined_value(promise_value) {
        return None;
    }
    set_value_field(interface, RL_IF_PENDING_PROMISE, undefined());
    promise_ptr_from_value(promise_value)
}

fn resolve_pending_question(interface: f64, line: String) {
    let Some(promise) = take_pending_question_promise(interface) else {
        return;
    };
    cleanup_question_abort_listener(interface);
    crate::promise::js_promise_resolve(promise, boxed_str(line.as_bytes()));
}

fn abort_pending_question(interface: f64) {
    let output = object_field(interface, RL_IF_OUTPUT).unwrap_or_else(undefined);
    let Some(promise) = take_pending_question_promise(interface) else {
        cleanup_question_abort_listener(interface);
        return;
    };
    cleanup_question_abort_listener(interface);
    call_write_value(output, "\r\n");
    crate::promise::js_promise_reject(promise, crate::url::js_abort_error_value());
}

extern "C" fn readline_promises_abort_question(closure: *const ClosureHeader) -> f64 {
    abort_pending_question(this_value(closure));
    undefined()
}

fn append_interface_input(interface: f64, chunk: f64) {
    let mut pending = object_field(interface, RL_IF_PENDING_TEXT)
        .map(value_to_string)
        .unwrap_or_default();
    pending.push_str(&value_to_string(chunk));

    while let Some(pos) = pending.find('\n') {
        let mut line: String = pending.drain(..=pos).collect();
        if line.ends_with('\n') {
            line.pop();
        }
        if line.ends_with('\r') {
            line.pop();
        }
        resolve_pending_question(interface, line);
    }

    set_value_field(interface, RL_IF_PENDING_TEXT, boxed_str(pending.as_bytes()));
}

extern "C" fn readline_promises_input_data(closure: *const ClosureHeader, chunk: f64) -> f64 {
    append_interface_input(this_value(closure), chunk);
    undefined()
}

extern "C" fn readline_promises_input_close(closure: *const ClosureHeader) -> f64 {
    let interface = this_value(closure);
    cleanup_question_abort_listener(interface);
    set_value_field(interface, RL_IF_CLOSED, bool_value(true));
    undefined()
}

fn attach_interface_input(interface: f64, input: f64) {
    if !stream_is_readable(input) {
        return;
    }
    let Some(raw) = raw_ptr_from_value(input) else {
        return;
    };

    let data = js_closure_alloc(readline_promises_input_data as *const u8, 1);
    js_closure_set_capture_f64(data, 0, interface);
    let data_value = f64::from_bits(JSValue::pointer(data as *const u8).bits());

    let close = js_closure_alloc(readline_promises_input_close as *const u8, 1);
    js_closure_set_capture_f64(close, 0, interface);
    let close_value = f64::from_bits(JSValue::pointer(close as *const u8).bits());

    let _ = crate::node_stream::js_node_stream_method_on(raw, boxed_str(b"data"), data_value);
    let _ = crate::node_stream::js_node_stream_method_on(raw, boxed_str(b"end"), close_value);
    let _ = crate::node_stream::js_node_stream_method_on(raw, boxed_str(b"close"), close_value);
}

extern "C" fn readline_promises_close(closure: *const ClosureHeader) -> f64 {
    let interface = this_value(closure);
    cleanup_question_abort_listener(interface);
    set_value_field(interface, RL_IF_PENDING_PROMISE, undefined());
    set_value_field(interface, RL_IF_CLOSED, bool_value(true));
    undefined()
}

extern "C" fn readline_promises_question(
    closure: *const ClosureHeader,
    query: f64,
    options: f64,
) -> f64 {
    let interface = this_value(closure);
    let output = object_field(interface, RL_IF_OUTPUT).unwrap_or_else(undefined);
    call_write_value(output, &value_to_string(query));

    let promise = crate::promise::js_promise_new();
    let promise_value = f64::from_bits(JSValue::pointer(promise as *const u8).bits());
    set_value_field(interface, RL_IF_PENDING_PROMISE, promise_value);

    if let Some(signal) = object_field(options, b"signal") {
        if let Some(signal_ptr) = abort_signal_ptr_from_value(signal) {
            if crate::url::js_abort_signal_is_aborted(signal_ptr) != 0 {
                abort_pending_question(interface);
                return promise_value;
            }

            let listener = js_closure_alloc(readline_promises_abort_question as *const u8, 1);
            js_closure_set_capture_f64(listener, 0, interface);
            let listener_value = f64::from_bits(JSValue::pointer(listener as *const u8).bits());
            set_value_field(interface, RL_IF_ABORT_SIGNAL, signal);
            set_value_field(interface, RL_IF_ABORT_LISTENER, listener_value);
            crate::url::js_abort_signal_add_listener(
                signal_ptr,
                boxed_str(b"abort"),
                listener_value,
            );
        }
    }

    promise_value
}

fn readline_promises_create_interface(opts: f64) -> f64 {
    let obj = js_object_alloc(0, 8);
    let obj_value = f64::from_bits(JSValue::pointer(obj as *const u8).bits());

    let input = object_field(opts, b"input").unwrap_or_else(undefined);
    if is_undefined_value(input)
        || (!stream_is_readable(input) && !object_field(input, b"on").is_some_and(is_callable))
    {
        let message = b"input.on is not a function";
        let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
        let err = crate::error::js_typeerror_new(msg);
        crate::exception::js_throw(f64::from_bits(JSValue::pointer(err as *const u8).bits()));
    }

    let output = object_field(opts, b"output").unwrap_or_else(undefined);
    set_object_field_value(obj, RL_IF_INPUT, input);
    set_object_field_value(obj, RL_IF_OUTPUT, output);
    set_object_field_value(obj, RL_IF_PENDING_TEXT, boxed_str(b""));
    set_object_field_value(obj, RL_IF_PENDING_PROMISE, undefined());
    set_object_field_value(obj, RL_IF_ABORT_SIGNAL, undefined());
    set_object_field_value(obj, RL_IF_ABORT_LISTENER, undefined());
    set_object_field_value(obj, RL_IF_CLOSED, bool_value(false));
    set_object_field_value(
        obj,
        b"close",
        readline_bound_method0(readline_promises_close, obj_value),
    );
    set_object_field_value(
        obj,
        b"question",
        readline_bound_method2(readline_promises_question, obj_value),
    );

    attach_interface_input(obj_value, input);
    obj_value
}

pub(crate) extern "C" fn thunk_readline_createInterface(
    _closure: *const ClosureHeader,
    opts: f64,
) -> f64 {
    readline_promises_create_interface(opts)
}

pub(crate) extern "C" fn thunk_readline_Interface(
    _closure: *const ClosureHeader,
    opts: f64,
) -> f64 {
    readline_promises_create_interface(opts)
}

fn number_to_i32(value: f64) -> i32 {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_int32() {
        return js.as_int32();
    }
    if js.is_number() && value.is_finite() {
        return value as i32;
    }
    0
}

fn readline_action_auto_commit(value: f64) -> bool {
    object_field(value, RL_ACTION_AUTO)
        .map(is_true_value)
        .unwrap_or(true)
}

fn flush_readline_actions(value: f64) {
    let queue = object_field(value, RL_ACTION_QUEUE)
        .map(value_to_string)
        .unwrap_or_default();
    set_value_field(value, RL_ACTION_QUEUE, boxed_str(b""));
    set_value_field(value, RL_ACTION_AUTO_PENDING, bool_value(false));
    if !queue.is_empty() {
        let output = object_field(value, RL_ACTION_OUTPUT).unwrap_or_else(undefined);
        call_write_value(output, &queue);
    }
}

extern "C" fn readline_auto_commit_callback(closure: *const ClosureHeader) -> f64 {
    flush_readline_actions(this_value(closure));
    undefined()
}

fn schedule_readline_auto_commit(value: f64) {
    if !readline_action_auto_commit(value) {
        return;
    }
    if object_field(value, RL_ACTION_AUTO_PENDING)
        .map(is_true_value)
        .unwrap_or(false)
    {
        return;
    }
    set_value_field(value, RL_ACTION_AUTO_PENDING, bool_value(true));
    let callback = js_closure_alloc(readline_auto_commit_callback as *const u8, 1);
    js_closure_set_capture_f64(callback, 0, value);
    crate::timer::js_set_immediate_callback(callback as i64);
}

fn append_readline_action(value: f64, sequence: String) {
    let mut queue = object_field(value, RL_ACTION_QUEUE)
        .map(value_to_string)
        .unwrap_or_default();
    queue.push_str(&sequence);
    set_value_field(value, RL_ACTION_QUEUE, boxed_str(queue.as_bytes()));
    schedule_readline_auto_commit(value);
}

extern "C" fn readline_action_clear_line(closure: *const ClosureHeader, dir: f64) -> f64 {
    let value = this_value(closure);
    let mode = match number_to_i32(dir) {
        1 => 0,
        -1 => 1,
        _ => 2,
    };
    append_readline_action(value, format!("\x1b[{mode}K"));
    value
}

extern "C" fn readline_action_clear_screen_down(closure: *const ClosureHeader) -> f64 {
    let value = this_value(closure);
    append_readline_action(value, "\x1b[0J".to_string());
    value
}

extern "C" fn readline_action_cursor_to(closure: *const ClosureHeader, x: f64, y: f64) -> f64 {
    let value = this_value(closure);
    let col = number_to_i32(x).saturating_add(1);
    let sequence = if is_null_or_undefined(y) {
        format!("\x1b[{col}G")
    } else {
        let row = number_to_i32(y).saturating_add(1);
        format!("\x1b[{row};{col}H")
    };
    append_readline_action(value, sequence);
    value
}

extern "C" fn readline_action_move_cursor(closure: *const ClosureHeader, dx: f64, dy: f64) -> f64 {
    let value = this_value(closure);
    let dx = number_to_i32(dx);
    let dy = number_to_i32(dy);
    let mut sequence = String::new();
    if dx > 0 {
        sequence.push_str(&format!("\x1b[{dx}C"));
    } else if dx < 0 {
        sequence.push_str(&format!("\x1b[{}D", dx.saturating_abs()));
    }
    if dy > 0 {
        sequence.push_str(&format!("\x1b[{dy}B"));
    } else if dy < 0 {
        sequence.push_str(&format!("\x1b[{}A", dy.saturating_abs()));
    }
    append_readline_action(value, sequence);
    value
}

extern "C" fn readline_action_commit(closure: *const ClosureHeader) -> f64 {
    flush_readline_actions(this_value(closure));
    promise_value(null_value())
}

extern "C" fn readline_action_rollback(closure: *const ClosureHeader) -> f64 {
    let value = this_value(closure);
    set_value_field(value, RL_ACTION_QUEUE, boxed_str(b""));
    set_value_field(value, RL_ACTION_AUTO_PENDING, bool_value(false));
    value
}

#[no_mangle]
pub extern "C" fn js_readline_promises_readline_new(output: f64, options: f64) -> f64 {
    let obj = js_object_alloc(0, 8);
    let obj_value = f64::from_bits(JSValue::pointer(obj as *const u8).bits());
    let auto_commit = object_field(options, b"autoCommit")
        .map(|v| !is_false_value(v))
        .unwrap_or(true);

    set_object_field_value(obj, RL_ACTION_OUTPUT, output);
    set_object_field_value(obj, RL_ACTION_QUEUE, boxed_str(b""));
    set_object_field_value(obj, RL_ACTION_AUTO, bool_value(auto_commit));
    set_object_field_value(obj, RL_ACTION_AUTO_PENDING, bool_value(false));
    set_object_field_value(
        obj,
        b"clearLine",
        readline_bound_method1(readline_action_clear_line, obj_value),
    );
    set_object_field_value(
        obj,
        b"clearScreenDown",
        readline_bound_method0(readline_action_clear_screen_down, obj_value),
    );
    set_object_field_value(
        obj,
        b"cursorTo",
        readline_bound_method2(readline_action_cursor_to, obj_value),
    );
    set_object_field_value(
        obj,
        b"moveCursor",
        readline_bound_method2(readline_action_move_cursor, obj_value),
    );
    set_object_field_value(
        obj,
        b"commit",
        readline_bound_method0(readline_action_commit, obj_value),
    );
    set_object_field_value(
        obj,
        b"rollback",
        readline_bound_method0(readline_action_rollback, obj_value),
    );
    obj_value
}

pub(crate) extern "C" fn thunk_readline_Readline(
    _closure: *const ClosureHeader,
    output: f64,
    options: f64,
) -> f64 {
    js_readline_promises_readline_new(output, options)
}

thunk!(
    thunk_fs_promises_constants,
    "node:fs/promises.constants is not callable."
);
