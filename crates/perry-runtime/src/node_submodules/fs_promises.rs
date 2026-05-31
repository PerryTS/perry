//! `node:fs/promises` thunks + shared Promise-construction helpers, plus the
//! `node:readline/promises` not-yet-implemented stubs.
//!
//! Extracted from `mod.rs` so the parent module stays under the file-size
//! gate. Pure code movement — no logic changes. The `promise_value` /
//! `promise_rejected` / `promise_undefined` helpers are `pub(crate)` because
//! the stream/promises, stream/consumers, and blob modules build their
//! resolved/rejected Promises through them.

use crate::closure::{js_closure_alloc, js_register_closure_arity, ClosureHeader};
use crate::value::JSValue;
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
    match unsafe { crate::fs::js_fs_chmod_result(path, mode) } {
        Ok(()) => promise_undefined(),
        Err(err_val) => promise_rejected(err_val),
    }
}

pub(crate) extern "C" fn thunk_fs_promises_chown(
    _closure: *const ClosureHeader,
    path: f64,
    uid: f64,
    gid: f64,
) -> f64 {
    match unsafe { crate::fs::js_fs_chown_result(path, uid, gid, true) } {
        Ok(()) => promise_undefined(),
        Err(err_val) => promise_rejected(err_val),
    }
}

pub(crate) extern "C" fn thunk_fs_promises_lchown(
    _closure: *const ClosureHeader,
    path: f64,
    uid: f64,
    gid: f64,
) -> f64 {
    match unsafe { crate::fs::js_fs_chown_result(path, uid, gid, false) } {
        Ok(()) => promise_undefined(),
        Err(err_val) => promise_rejected(err_val),
    }
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
    match unsafe { crate::fs::js_fs_lchmod_result(path, mode) } {
        Ok(()) => promise_undefined(),
        Err(err_val) => promise_rejected(err_val),
    }
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
    match unsafe { crate::fs::js_fs_rm_result(path, options) } {
        Ok(()) => promise_undefined(),
        Err(err_val) => promise_rejected(err_val),
    }
}

pub(crate) extern "C" fn thunk_fs_promises_rmdir(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    match unsafe { crate::fs::js_fs_rmdir_result(path, options) } {
        Ok(()) => promise_undefined(),
        Err(err_val) => promise_rejected(err_val),
    }
}

pub(crate) extern "C" fn thunk_fs_promises_unlink(
    _closure: *const ClosureHeader,
    path: f64,
) -> f64 {
    match catch_fs_promises_throw(|| match unsafe { crate::fs::js_fs_unlink_result(path) } {
        Ok(()) => promise_undefined(),
        Err(err_val) => promise_rejected(err_val),
    }) {
        Ok(promise) => promise,
        Err(err) => promise_rejected(err),
    }
}

pub(crate) extern "C" fn thunk_fs_promises_rename(
    _closure: *const ClosureHeader,
    from: f64,
    to: f64,
) -> f64 {
    match unsafe { crate::fs::js_fs_rename_result(from, to) } {
        Ok(()) => promise_undefined(),
        Err(err_val) => promise_rejected(err_val),
    }
}

pub(crate) extern "C" fn thunk_fs_promises_copyFile(
    _closure: *const ClosureHeader,
    from: f64,
    to: f64,
    flags: f64,
) -> f64 {
    match unsafe { crate::fs::js_fs_copy_file_result(from, to, flags) } {
        Ok(()) => promise_undefined(),
        Err(err_val) => promise_rejected(err_val),
    }
}

pub(crate) extern "C" fn thunk_fs_promises_cp(
    _closure: *const ClosureHeader,
    from: f64,
    to: f64,
    options: f64,
) -> f64 {
    match crate::fs::js_fs_cp_async_result(from, to, options) {
        Ok(()) => promise_undefined(),
        Err(err_val) => promise_rejected(err_val),
    }
}

pub(crate) extern "C" fn thunk_fs_promises_truncate(
    _closure: *const ClosureHeader,
    path: f64,
    len: f64,
) -> f64 {
    match unsafe { crate::fs::js_fs_truncate_result(path, len) } {
        Ok(()) => promise_undefined(),
        Err(err_val) => promise_rejected(err_val),
    }
}

pub(crate) extern "C" fn thunk_fs_promises_utimes(
    _closure: *const ClosureHeader,
    path: f64,
    atime: f64,
    mtime: f64,
) -> f64 {
    match unsafe { crate::fs::js_fs_utimes_result(path, atime, mtime, false) } {
        Ok(()) => promise_undefined(),
        Err(err_val) => promise_rejected(err_val),
    }
}

pub(crate) extern "C" fn thunk_fs_promises_lutimes(
    _closure: *const ClosureHeader,
    path: f64,
    atime: f64,
    mtime: f64,
) -> f64 {
    match unsafe { crate::fs::js_fs_utimes_result(path, atime, mtime, true) } {
        Ok(()) => promise_undefined(),
        Err(err_val) => promise_rejected(err_val),
    }
}

pub(crate) extern "C" fn thunk_fs_promises_link(
    _closure: *const ClosureHeader,
    from: f64,
    to: f64,
) -> f64 {
    match unsafe { crate::fs::js_fs_link_result(from, to) } {
        Ok(()) => promise_undefined(),
        Err(err_val) => promise_rejected(err_val),
    }
}

pub(crate) extern "C" fn thunk_fs_promises_symlink(
    _closure: *const ClosureHeader,
    target: f64,
    path: f64,
    _type: f64,
) -> f64 {
    match unsafe { crate::fs::js_fs_symlink_result(target, path) } {
        Ok(()) => promise_undefined(),
        Err(err_val) => promise_rejected(err_val),
    }
}

pub(crate) extern "C" fn thunk_fs_promises_readlink(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    match crate::fs::js_fs_readlink_value_result(path, options) {
        Ok(v) => promise_value(v),
        Err(err_val) => promise_rejected(err_val),
    }
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
    match catch_fs_promises_throw(|| match crate::fs::js_fs_opendir_value_with_path(path) {
        Ok(dir) => promise_value(dir),
        Err(err_val) => promise_rejected(err_val),
    }) {
        Ok(promise) => promise,
        Err(err) => promise_rejected(err),
    }
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
    match unsafe { crate::fs::js_fs_access_result(path, mode) } {
        Ok(()) => promise_undefined(),
        Err(err_val) => promise_rejected(err_val),
    }
}

extern "C" fn readline_promises_close(_closure: *const ClosureHeader) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn readline_promises_question(
    _closure: *const ClosureHeader,
    _query: f64,
    _options: f64,
) -> f64 {
    promise_undefined()
}

fn readline_promises_method0(func: extern "C" fn(*const ClosureHeader) -> f64) -> f64 {
    js_register_closure_arity(func as *const u8, 0);
    let closure = js_closure_alloc(func as *const u8, 0);
    f64::from_bits(JSValue::pointer(closure as *const u8).bits())
}

fn readline_promises_method2(func: extern "C" fn(*const ClosureHeader, f64, f64) -> f64) -> f64 {
    js_register_closure_arity(func as *const u8, 2);
    let closure = js_closure_alloc(func as *const u8, 0);
    f64::from_bits(JSValue::pointer(closure as *const u8).bits())
}

fn set_readline_promises_field(
    obj: *mut crate::object::ObjectHeader,
    name: &'static [u8],
    value: f64,
) {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_set_field_by_name(obj, key, value);
}

pub(crate) extern "C" fn thunk_readline_createInterface(
    _closure: *const ClosureHeader,
    _opts: f64,
) -> f64 {
    let obj = crate::object::js_object_alloc(0, 2);
    set_readline_promises_field(
        obj,
        b"close",
        readline_promises_method0(readline_promises_close),
    );
    set_readline_promises_field(
        obj,
        b"question",
        readline_promises_method2(readline_promises_question),
    );
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

thunk!(
    thunk_fs_promises_constants,
    "node:fs/promises.constants is not callable."
);

thunk!(
    thunk_readline_Interface,
    "node:readline/promises.Interface is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_readline_Readline,
    "node:readline/promises.Readline is not yet implemented in Perry (tracked by issue #793)."
);
