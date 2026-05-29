//! `node:fs/promises` thunks + shared Promise-construction helpers, plus the
//! `node:readline/promises` not-yet-implemented stubs.
//!
//! Extracted from `mod.rs` so the parent module stays under the file-size
//! gate. Pure code movement — no logic changes. The `promise_value` /
//! `promise_rejected` / `promise_undefined` helpers are `pub(crate)` because
//! the stream/promises, stream/consumers, and blob modules build their
//! resolved/rejected Promises through them.

use crate::closure::ClosureHeader;
use crate::value::JSValue;

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

pub(crate) extern "C" fn thunk_fs_promises_readFile(
    _closure: *const ClosureHeader,
    path: f64,
    encoding: f64,
) -> f64 {
    promise_value(crate::fs::js_fs_read_file_dispatch(path, encoding))
}

pub(crate) extern "C" fn thunk_fs_promises_open(
    _closure: *const ClosureHeader,
    path: f64,
    flags: f64,
    _mode: f64,
) -> f64 {
    match unsafe { crate::fs::js_fs_filehandle_open_result(path, flags) } {
        Ok(handle) => promise_value(handle),
        Err(err_val) => promise_rejected(err_val),
    }
}

pub(crate) extern "C" fn thunk_fs_promises_writeFile(
    _closure: *const ClosureHeader,
    path: f64,
    data: f64,
    options: f64,
) -> f64 {
    let _ = crate::fs::js_fs_write_file_sync_options(path, data, options);
    promise_undefined()
}

pub(crate) extern "C" fn thunk_fs_promises_appendFile(
    _closure: *const ClosureHeader,
    path: f64,
    data: f64,
    options: f64,
) -> f64 {
    let _ = crate::fs::js_fs_append_file_sync_options(path, data, options);
    promise_undefined()
}

pub(crate) extern "C" fn thunk_fs_promises_chmod(
    _closure: *const ClosureHeader,
    path: f64,
    mode: f64,
) -> f64 {
    let _ = crate::fs::js_fs_chmod_sync(path, mode);
    promise_undefined()
}

pub(crate) extern "C" fn thunk_fs_promises_chown(
    _closure: *const ClosureHeader,
    path: f64,
    uid: f64,
    gid: f64,
) -> f64 {
    let _ = crate::fs::js_fs_chown_sync(path, uid, gid);
    promise_undefined()
}

pub(crate) extern "C" fn thunk_fs_promises_lchown(
    _closure: *const ClosureHeader,
    path: f64,
    uid: f64,
    gid: f64,
) -> f64 {
    let _ = crate::fs::js_fs_lchown_sync(path, uid, gid);
    promise_undefined()
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
    let _ = crate::fs::js_fs_lchmod_sync(path, mode);
    promise_undefined()
}

pub(crate) extern "C" fn thunk_fs_promises_mkdir(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    let _ = crate::fs::js_fs_mkdir_sync_options(path, options);
    promise_undefined()
}

pub(crate) extern "C" fn thunk_fs_promises_readdir(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    let raw = crate::fs::js_fs_readdir_sync(path, options);
    promise_value(f64::from_bits(
        JSValue::pointer(raw.to_bits() as *const u8).bits(),
    ))
}

pub(crate) extern "C" fn thunk_fs_promises_stat(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    promise_value(crate::fs::js_fs_stat_sync_options(path, options))
}

pub(crate) extern "C" fn thunk_fs_promises_statfs(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    promise_value(crate::fs::js_fs_statfs_sync_options(path, options))
}

pub(crate) extern "C" fn thunk_fs_promises_lstat(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    promise_value(crate::fs::js_fs_lstat_sync_options(path, options))
}

pub(crate) extern "C" fn thunk_fs_promises_rm(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    let _ = crate::fs::js_fs_rm_recursive_options(path, options);
    promise_undefined()
}

pub(crate) extern "C" fn thunk_fs_promises_rmdir(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    let _ = crate::fs::js_fs_rmdir_sync_options(path, options);
    promise_undefined()
}

pub(crate) extern "C" fn thunk_fs_promises_unlink(
    _closure: *const ClosureHeader,
    path: f64,
) -> f64 {
    match unsafe { crate::fs::js_fs_unlink_result(path) } {
        Ok(()) => promise_undefined(),
        Err(err_val) => promise_rejected(err_val),
    }
}

pub(crate) extern "C" fn thunk_fs_promises_rename(
    _closure: *const ClosureHeader,
    from: f64,
    to: f64,
) -> f64 {
    let _ = crate::fs::js_fs_rename_sync(from, to);
    promise_undefined()
}

pub(crate) extern "C" fn thunk_fs_promises_copyFile(
    _closure: *const ClosureHeader,
    from: f64,
    to: f64,
    flags: f64,
) -> f64 {
    let _ = crate::fs::js_fs_copy_file_sync_flags(from, to, flags);
    promise_undefined()
}

pub(crate) extern "C" fn thunk_fs_promises_cp(
    _closure: *const ClosureHeader,
    from: f64,
    to: f64,
    options: f64,
) -> f64 {
    let _ = crate::fs::js_fs_cp_async_options(from, to, options);
    promise_undefined()
}

pub(crate) extern "C" fn thunk_fs_promises_truncate(
    _closure: *const ClosureHeader,
    path: f64,
    len: f64,
) -> f64 {
    let _ = crate::fs::js_fs_truncate_sync(path, len);
    promise_undefined()
}

pub(crate) extern "C" fn thunk_fs_promises_utimes(
    _closure: *const ClosureHeader,
    path: f64,
    atime: f64,
    mtime: f64,
) -> f64 {
    let _ = crate::fs::js_fs_utimes_sync(path, atime, mtime);
    promise_undefined()
}

pub(crate) extern "C" fn thunk_fs_promises_lutimes(
    _closure: *const ClosureHeader,
    path: f64,
    atime: f64,
    mtime: f64,
) -> f64 {
    let _ = crate::fs::js_fs_lutimes_sync(path, atime, mtime);
    promise_undefined()
}

pub(crate) extern "C" fn thunk_fs_promises_link(
    _closure: *const ClosureHeader,
    from: f64,
    to: f64,
) -> f64 {
    let _ = crate::fs::js_fs_link_sync(from, to);
    promise_undefined()
}

pub(crate) extern "C" fn thunk_fs_promises_symlink(
    _closure: *const ClosureHeader,
    target: f64,
    path: f64,
    _type: f64,
) -> f64 {
    let _ = crate::fs::js_fs_symlink_sync(target, path);
    promise_undefined()
}

pub(crate) extern "C" fn thunk_fs_promises_readlink(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    promise_value(crate::fs::js_fs_readlink_dispatch(path, options))
}

pub(crate) extern "C" fn thunk_fs_promises_realpath(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    promise_value(crate::fs::js_fs_realpath_dispatch(path, options))
}

pub(crate) extern "C" fn thunk_fs_promises_mkdtemp(
    _closure: *const ClosureHeader,
    prefix: f64,
    options: f64,
) -> f64 {
    promise_value(crate::fs::js_fs_mkdtemp_dispatch(prefix, options))
}

pub(crate) extern "C" fn thunk_fs_promises_opendir(
    _closure: *const ClosureHeader,
    path: f64,
) -> f64 {
    promise_value(crate::fs::js_fs_opendir_sync(path))
}

pub(crate) extern "C" fn thunk_fs_promises_glob(
    _closure: *const ClosureHeader,
    pattern: f64,
    options: f64,
) -> f64 {
    let raw = crate::fs::js_fs_glob_sync_options(pattern, options);
    promise_value(f64::from_bits(
        JSValue::pointer(raw.to_bits() as *const u8).bits(),
    ))
}

pub(crate) extern "C" fn thunk_fs_promises_watch(
    _closure: *const ClosureHeader,
    path: f64,
    options: f64,
) -> f64 {
    crate::fs::js_fs_watch(path, options, f64::from_bits(crate::value::TAG_UNDEFINED))
}

pub(crate) extern "C" fn thunk_fs_promises_access(
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
