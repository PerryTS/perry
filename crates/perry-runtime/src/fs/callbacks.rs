//! Callback-style fs APIs — pre-flight probe + (err, value) dispatch.

use crate::closure::ClosureHeader;
use std::os::raw::c_int;

use super::*;

#[no_mangle]
pub extern "C" fn js_fs_read_file_callback(path_value: f64, encoding: f64, callback: f64) -> f64 {
    use crate::closure::js_closure_call2;
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;

    let cb_ptr = callback_from_options_arg(encoding, callback);
    unsafe {
        if let Some(err_val) = fs_callback_read_error(path_value, "open") {
            if !cb_ptr.is_null() {
                js_closure_call2(cb_ptr, err_val, f64::from_bits(TAG_UNDEFINED));
            }
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let encoding_is_callback = !extract_closure_ptr(encoding).is_null();
    let want_buffer = encoding_is_callback || read_file_encoding(encoding).is_none();
    let data_val = if want_buffer {
        let buf = js_fs_read_file_binary_options(path_value, encoding);
        if buf.is_null() {
            f64::from_bits(TAG_UNDEFINED)
        } else {
            f64::from_bits(crate::value::JSValue::pointer(buf as *const u8).bits())
        }
    } else {
        let str_ptr = js_fs_read_file_sync_options(path_value, encoding);
        if str_ptr.is_null() {
            f64::from_bits(TAG_UNDEFINED)
        } else {
            f64::from_bits(crate::value::js_nanbox_string(str_ptr as i64).to_bits())
        }
    };

    if !cb_ptr.is_null() {
        js_closure_call2(cb_ptr, f64::from_bits(TAG_NULL), data_val);
    }
    f64::from_bits(TAG_UNDEFINED)
}

fn required_callback(value: f64) -> *const ClosureHeader {
    crate::fs::validate::validate_required_callback("cb", value)
}

fn required_callback_named(arg_name: &str, value: f64) -> *const ClosureHeader {
    crate::fs::validate::validate_required_callback(arg_name, value)
}

fn is_undefined_value(value: f64) -> bool {
    crate::value::JSValue::from_bits(value.to_bits()).is_undefined()
}

fn callback_or_arg2(arg1: f64, arg2: f64) -> *const ClosureHeader {
    let first = extract_closure_ptr(arg1);
    if !first.is_null() {
        first
    } else {
        required_callback(arg2)
    }
}

fn callback_or_arg2_named(arg_name: &str, arg1: f64, arg2: f64) -> *const ClosureHeader {
    let first = extract_closure_ptr(arg1);
    if !first.is_null() {
        first
    } else {
        required_callback_named(arg_name, arg2)
    }
}

fn callback_or_arg3(arg2: f64, arg3: f64) -> *const ClosureHeader {
    let second = extract_closure_ptr(arg2);
    if !second.is_null() {
        second
    } else {
        required_callback(arg3)
    }
}

fn callback_or_symlink_type_arg(arg2: f64, arg3: f64) -> *const ClosureHeader {
    let fourth = extract_closure_ptr(arg3);
    if !fourth.is_null() {
        return fourth;
    }
    if !is_undefined_value(arg3) {
        return required_callback(arg3);
    }

    let third = extract_closure_ptr(arg2);
    if !third.is_null() {
        third
    } else {
        required_callback(arg2)
    }
}

fn callback_from_options_arg(options: f64, callback: f64) -> *const ClosureHeader {
    let cb = extract_closure_ptr(callback);
    if !cb.is_null() {
        cb
    } else if crate::value::js_is_truthy(callback) != 0 {
        required_callback(callback)
    } else {
        let options_cb = extract_closure_ptr(options);
        if !options_cb.is_null() {
            options_cb
        } else {
            required_callback(options)
        }
    }
}

fn catch_callback_throw(call: impl FnOnce() -> f64) -> Result<f64, f64> {
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

pub(crate) fn call_cb0(callback: *const ClosureHeader) {
    if !callback.is_null() {
        crate::closure::js_closure_call1(callback, f64::from_bits(0x7FFC_0000_0000_0002));
    }
}

/// Invoke a 2-arg callback with (err, undefined). Used by read-style ops
/// when the pre-flight probe detected an io::Error.
pub(crate) unsafe fn call_cb_err2(callback: *const ClosureHeader, err_val: f64) {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    if !callback.is_null() {
        crate::closure::js_closure_call2(callback, err_val, f64::from_bits(TAG_UNDEFINED));
    }
}

/// Invoke a 1-arg callback with (err). Used by void ops (mkdir/unlink/rm/…)
/// when the pre-flight probe detected an io::Error.
pub(crate) unsafe fn call_cb_err1(callback: *const ClosureHeader, err_val: f64) {
    if !callback.is_null() {
        crate::closure::js_closure_call1(callback, err_val);
    }
}

/// `fs.writeFile(path, data, callback)` — sync write + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_write_file_callback(
    path_value: f64,
    content_value: f64,
    arg2: f64,
    arg3: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let options = if extract_closure_ptr(arg2).is_null() {
        arg2
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = callback_from_options_arg(arg2, arg3);
    unsafe {
        if let Some(err_val) = fs_callback_write_parent_error(path_value, "open") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    unsafe {
        match write_file_path_or_fd_result(path_value, content_value, options) {
            Ok(()) => call_cb0(cb),
            Err(err_val) => call_cb_err1(cb, err_val),
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.appendFile(path, data, callback)` — sync append + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_append_file_callback(
    path_value: f64,
    content_value: f64,
    arg2: f64,
    arg3: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let options = if extract_closure_ptr(arg2).is_null() {
        arg2
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = callback_from_options_arg(arg2, arg3);
    unsafe {
        if let Some(err_val) = fs_callback_write_parent_error(path_value, "open") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_append_file_sync_options(path_value, content_value, options);
    call_cb0(cb);
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.mkdir(path[, options], callback)` — sync mkdir + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_mkdir_callback(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let options = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = callback_or_arg2(arg1, arg2);
    unsafe {
        match js_fs_mkdir_result(path_value, options) {
            Ok(()) => call_cb0(cb),
            Err(err_val) => {
                call_cb_err1(cb, err_val);
                return f64::from_bits(TAG_UNDEFINED);
            }
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.unlink(path, callback)` — sync unlink + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_unlink_callback(path_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let cb = required_callback(callback);
    unsafe {
        match js_fs_unlink_result(path_value) {
            Ok(()) => call_cb0(cb),
            Err(err_val) => {
                call_cb_err1(cb, err_val);
                return f64::from_bits(TAG_UNDEFINED);
            }
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.rm(path[, options], callback)` — recursive sync removal + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_rm_callback(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let options = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = callback_or_arg2(arg1, arg2);
    unsafe {
        match crate::fs::js_fs_rm_result(path_value, options) {
            Ok(()) => call_cb0(cb),
            Err(err_val) => call_cb_err1(cb, err_val),
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.access(path[, mode], callback)` — sync access + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_access_callback(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    crate::fs::validate::validate_path("path", path_value);
    let mode = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = callback_or_arg2(arg1, arg2);
    crate::fs::validate::validate_fs_mode(mode);
    unsafe {
        match crate::fs::js_fs_access_result(path_value, mode) {
            Ok(()) => call_cb0(cb),
            Err(err_val) => call_cb_err1(cb, err_val),
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.exists(path, callback)` — deprecated Node callback shape:
/// invokes the callback with a single boolean, not `(err, value)`.
#[no_mangle]
pub extern "C" fn js_fs_exists_callback(path_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    // Node throws `ERR_INVALID_ARG_TYPE` if the callback isn't a function
    // (this is the *only* arg `fs.exists` validates — a bad path just
    // makes the callback fire with `false`, see test-fs-exists.js).
    crate::fs::validate::validate_function("cb", callback);
    let exists = js_fs_exists_sync(path_value) == 1;
    let cb = required_callback(callback);
    if !cb.is_null() {
        let arg = if exists { TAG_TRUE } else { TAG_FALSE };
        crate::closure::js_closure_call1(cb, f64::from_bits(arg));
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.readdir(path[, options], callback)` — sync readdir + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_readdir_callback(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    // `fs.readdir(path, callback)` puts the callback in `arg1`; only
    // `fs.readdir(path, options, callback)` carries real options there. Passing
    // the callback closure to `js_fs_readdir_sync` as `options` (the old bug)
    // made it read garbage out of the closure object and halt the program
    // before the callback ever fired. Disambiguate the same way the stat/lstat
    // callbacks do: a closure in `arg1` means there are no options.
    let options = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = callback_or_arg2(arg1, arg2);
    unsafe {
        if let Some(err_val) = fs_callback_read_error(path_value, "scandir") {
            call_cb_err2(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let entries = js_fs_readdir_sync(path_value, options);
    let entries =
        f64::from_bits(crate::value::JSValue::pointer(entries.to_bits() as *const u8).bits());
    if !cb.is_null() {
        crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), entries);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.stat(path[, options], callback)` — sync stat + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_stat_callback(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let options = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = callback_or_arg2(arg1, arg2);
    unsafe {
        if let Some(err_val) = fs_callback_read_error(path_value, "stat") {
            call_cb_err2(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let stats = js_fs_stat_sync_options(path_value, options);
    if !cb.is_null() {
        crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), stats);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.lstat(path[, options], callback)` — sync lstat + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_lstat_callback(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let options = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = callback_or_arg2(arg1, arg2);
    unsafe {
        if let Some(err_val) = fs_callback_lstat_error(path_value, "lstat") {
            call_cb_err2(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let stats = js_fs_lstat_sync_options(path_value, options);
    if !cb.is_null() {
        crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), stats);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.statfs(path[, options], callback)` — sync statfs + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_statfs_callback(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let options = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = callback_or_arg2(arg1, arg2);
    unsafe {
        if let Some(err_val) = fs_callback_read_error(path_value, "statfs") {
            call_cb_err2(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let stats = js_fs_statfs_sync_options(path_value, options);
    if !cb.is_null() {
        crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), stats);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.opendir(path[, options], callback)` — sync open + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_opendir_callback(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let cb = callback_or_arg2_named("callback", arg1, arg2);
    let dir = match js_fs_opendir_value_with_path(path_value) {
        Ok(dir) => dir,
        Err(err) => {
            unsafe { call_cb_err1(cb, err) };
            return f64::from_bits(TAG_UNDEFINED);
        }
    };
    if !cb.is_null() {
        crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), dir);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.glob(pattern[, options], callback)`.
#[no_mangle]
pub extern "C" fn js_fs_glob_callback(pattern_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let options = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = callback_or_arg2(arg1, arg2);
    match catch_callback_throw(|| {
        let raw = js_fs_glob_sync_options(pattern_value, options);
        f64::from_bits(crate::value::JSValue::pointer(raw.to_bits() as *const u8).bits())
    }) {
        Ok(entries) => {
            if !cb.is_null() {
                crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), entries);
            }
        }
        Err(err) => unsafe { call_cb_err2(cb, err) },
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.fstat(fd, callback)` — sync fstat + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_fstat_callback(fd_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    crate::fs::validate::validate_fd(fd_value);
    let options = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let bigint = unsafe { options_bool_field(options, b"bigint") };
    crate::fs::validate::validate_fd(fd_value);
    let cb = callback_or_arg2(arg1, arg2);
    let result = fstat_stats_value(fd_value as i32, bigint);
    if !cb.is_null() {
        match result {
            Ok(stats) => {
                crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), stats);
            }
            Err(err) => unsafe { call_cb_err2(cb, err) },
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.chmod(path, mode, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_chmod_callback(path_value: f64, mode_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let cb = required_callback(callback);
    unsafe {
        match crate::fs::js_fs_chmod_result(path_value, mode_value) {
            Ok(()) => call_cb0(cb),
            Err(err_val) => call_cb_err1(cb, err_val),
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.chown(path, uid, gid, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_chown_callback(
    path_value: f64,
    uid_value: f64,
    gid_value: f64,
    callback: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let cb = required_callback(callback);
    unsafe {
        match crate::fs::js_fs_chown_result(path_value, uid_value, gid_value, true) {
            Ok(()) => call_cb0(cb),
            Err(err_val) => call_cb_err1(cb, err_val),
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.lchown(path, uid, gid, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_lchown_callback(
    path_value: f64,
    uid_value: f64,
    gid_value: f64,
    callback: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    crate::fs::validate::validate_path("path", path_value);
    crate::fs::validate::validate_int32(uid_value, "uid", -1, u32::MAX as i64);
    crate::fs::validate::validate_int32(gid_value, "gid", -1, u32::MAX as i64);
    let cb = required_callback(callback);
    unsafe {
        match crate::fs::js_fs_chown_result(path_value, uid_value, gid_value, false) {
            Ok(()) => call_cb0(cb),
            Err(err_val) => call_cb_err1(cb, err_val),
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.lchmod(path, mode, callback)`. macOS/BSD-only; on Linux Node exposes
/// the property with value `undefined`, so any attempted call throws a plain
/// TypeError before argument validation.
#[no_mangle]
pub extern "C" fn js_fs_lchmod_callback(path_value: f64, mode_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    if !crate::fs::lchmod_is_callable_on_this_platform() {
        let _ = (path_value, mode_value, callback);
        let message = "fs.lchmod is not a function";
        let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
        let err = crate::error::js_typeerror_new(msg);
        crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
    }
    crate::fs::validate::validate_path("path", path_value);
    let cb = required_callback(callback);
    unsafe {
        match crate::fs::js_fs_lchmod_result(path_value, mode_value) {
            Ok(()) => call_cb0(cb),
            Err(err_val) => call_cb_err1(cb, err_val),
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.truncate(path, len, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_truncate_callback(path_value: f64, len_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let len = if extract_closure_ptr(len_value).is_null() {
        len_value
    } else {
        0.0
    };
    let cb = if !extract_closure_ptr(len_value).is_null() {
        extract_closure_ptr(len_value)
    } else {
        required_callback(callback)
    };
    unsafe {
        match crate::fs::js_fs_truncate_result(path_value, len) {
            Ok(()) => call_cb0(cb),
            Err(err_val) => call_cb_err1(cb, err_val),
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.link(existingPath, newPath, callback)` — sync hard link + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_link_callback(from_value: f64, to_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let cb = required_callback(callback);
    unsafe {
        match crate::fs::js_fs_link_result(from_value, to_value) {
            Ok(()) => call_cb0(cb),
            Err(err_val) => call_cb_err1(cb, err_val),
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.symlink(target, path[, type], callback)` — sync symlink + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_symlink_callback(
    from_value: f64,
    to_value: f64,
    arg2: f64,
    arg3: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let cb = callback_or_symlink_type_arg(arg2, arg3);
    unsafe {
        match crate::fs::js_fs_symlink_result(from_value, to_value) {
            Ok(()) => call_cb0(cb),
            Err(err_val) => call_cb_err1(cb, err_val),
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.readlink(path[, options], callback)`.
#[no_mangle]
pub extern "C" fn js_fs_readlink_callback(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let options = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = callback_or_arg2(arg1, arg2);
    let value = match crate::fs::js_fs_readlink_value_result(path_value, options) {
        Ok(v) => v,
        Err(err_val) => {
            unsafe { call_cb_err2(cb, err_val) };
            return f64::from_bits(TAG_UNDEFINED);
        }
    };
    if !cb.is_null() {
        crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.realpath(path[, options], callback)`.
#[no_mangle]
pub extern "C" fn js_fs_realpath_callback(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let options = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = callback_or_arg2(arg1, arg2);
    let value = match crate::fs::js_fs_realpath_value_result(path_value, options, "lstat") {
        Ok(value) => value,
        Err(err_val) => {
            unsafe { call_cb_err2(cb, err_val) };
            return f64::from_bits(TAG_UNDEFINED);
        }
    };
    if !cb.is_null() {
        crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.mkdtemp(prefix[, options], callback)`.
#[no_mangle]
pub extern "C" fn js_fs_mkdtemp_callback(prefix_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let options = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = callback_or_arg2(arg1, arg2);
    validate::validate_path("prefix", prefix_value);
    validate::validate_string_or_object_options("options", options);
    let value = match super::mkdtemp_bytes_result(prefix_value) {
        Ok(bytes) => {
            if fs_encoding_option(options).as_deref() == Some("buffer") {
                buffer_value_from_bytes(&bytes)
            } else {
                let enc = fs_encoding_option(options).unwrap_or_else(|| "utf8".to_string());
                let s = encoded_string_ptr(&bytes, &enc);
                f64::from_bits(crate::value::JSValue::string_ptr(s).bits())
            }
        }
        Err(err_val) => unsafe {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        },
    };
    if !cb.is_null() {
        crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.open(path, flags, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_open_callback(path_value: f64, arg1: f64, arg2: f64, arg3: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let cb = if !extract_closure_ptr(arg3).is_null() {
        extract_closure_ptr(arg3)
    } else if !extract_closure_ptr(arg2).is_null() {
        extract_closure_ptr(arg2)
    } else if !is_undefined_value(arg2) {
        required_callback(arg3)
    } else if !is_undefined_value(arg1) {
        required_callback(arg1)
    } else {
        required_callback(arg1)
    };
    let flags = if !extract_closure_ptr(arg1).is_null() {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        arg1
    };
    let fd = match unsafe { fs_open_sync_result(path_value, flags) } {
        Ok(fd) => fd as f64,
        Err((err, path)) => unsafe {
            call_cb_err2(cb, build_fs_error_value(&err, "open", &path));
            return f64::from_bits(TAG_UNDEFINED);
        },
    };
    if !cb.is_null() {
        crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), fd);
    }
    f64::from_bits(TAG_UNDEFINED)
}

pub(crate) unsafe fn decode_flags_string(value: f64) -> Option<String> {
    let jsval = crate::value::JSValue::from_bits(value.to_bits());
    // #1781: fs flag strings ("r", "r+", "w", "a", "wx", …) are ALL
    // <= 5 bytes, so they are inline SSO values and `is_string()`
    // (STRING_TAG-only) rejects every one of them. Decode the inline
    // bytes directly before falling through to the heap-string path.
    if jsval.is_short_string() {
        let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = jsval.short_string_to_buf(&mut buf);
        return std::str::from_utf8(&buf[..n]).ok().map(|s| s.to_string());
    }
    if !jsval.is_string() {
        return None;
    }
    let ptr = jsval.as_string_ptr();
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    std::str::from_utf8(std::slice::from_raw_parts(data, len))
        .ok()
        .map(|s| s.to_string())
}

/// `fs.close(fd, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_close_callback(fd_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    crate::fs::validate::validate_fd(fd_value);
    let cb = required_callback(callback);
    // #3332: deliver EBADF to the callback for a bad descriptor rather than
    // throwing it; the close only runs when the fd is open.
    if let Some(err_val) = crate::fs::validate::fd_open_callback_error(fd_value, "close") {
        unsafe { call_cb_err1(cb, err_val) };
        return f64::from_bits(TAG_UNDEFINED);
    }
    let _ = js_fs_close_sync(fd_value);
    call_cb0(cb);
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.cp(src, dest, options, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_cp_callback(from_value: f64, to_value: f64, arg2: f64, arg3: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let options = if extract_closure_ptr(arg2).is_null() {
        arg2
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = callback_or_arg3(arg2, arg3);
    match js_fs_cp_async_result(from_value, to_value, options) {
        Ok(()) => call_cb0(cb),
        Err(err_val) => unsafe {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        },
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.rmdir(path[, options], callback)`.
#[no_mangle]
pub extern "C" fn js_fs_rmdir_callback(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let options = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = callback_or_arg2(arg1, arg2);
    unsafe {
        match crate::fs::js_fs_rmdir_result(path_value, options) {
            Ok(()) => call_cb0(cb),
            Err(err_val) => call_cb_err1(cb, err_val),
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.ftruncate(fd, len, callback)` — sync ftruncate + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_ftruncate_callback(fd_value: f64, len_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    // Node's `validateInt32(fd)` throws synchronously on a non-numeric fd; the
    // "valid type but bad/closed descriptor" and syscall failures are delivered
    // to the callback as the first arg (#2749).
    crate::fs::validate::validate_fd(fd_value);
    let cb = required_callback(callback);
    unsafe {
        match crate::fs::js_fs_ftruncate_result(fd_value, len_value) {
            Ok(()) => call_cb0(cb),
            Err(err_val) => call_cb_err1(cb, err_val),
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.fsync(fd, callback)` — sync fsync + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_fsync_callback(fd_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    crate::fs::validate::validate_fd(fd_value);
    let cb = required_callback(callback);
    if let Some(err_val) = crate::fs::validate::fd_open_callback_error(fd_value, "fsync") {
        unsafe { call_cb_err1(cb, err_val) };
        return f64::from_bits(TAG_UNDEFINED);
    }
    let _ = js_fs_fsync_sync(fd_value);
    call_cb0(cb);
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.fdatasync(fd, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_fdatasync_callback(fd_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    crate::fs::validate::validate_fd(fd_value);
    let cb = required_callback(callback);
    if let Some(err_val) = crate::fs::validate::fd_open_callback_error(fd_value, "fdatasync") {
        unsafe { call_cb_err1(cb, err_val) };
        return f64::from_bits(TAG_UNDEFINED);
    }
    let _ = js_fs_fdatasync_sync(fd_value);
    call_cb0(cb);
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.fchmod(fd, mode, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_fchmod_callback(fd_value: f64, mode_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    crate::fs::validate::validate_fd(fd_value);
    let cb = required_callback(callback);
    if let Some(err_val) = crate::fs::validate::fd_open_callback_error(fd_value, "fchmod") {
        unsafe { call_cb_err1(cb, err_val) };
        return f64::from_bits(TAG_UNDEFINED);
    }
    let _ = js_fs_fchmod_sync(fd_value, mode_value);
    call_cb0(cb);
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.fchown(fd, uid, gid, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_fchown_callback(
    fd_value: f64,
    uid_value: f64,
    gid_value: f64,
    callback: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    // Node validates fd/uid/gid types synchronously (throwing on bad types),
    // then delivers EBADF (closed fd) and syscall failures (e.g. EPERM) to the
    // callback as the first argument (#2749).
    crate::fs::validate::validate_fd(fd_value);
    crate::fs::validate::validate_int32(uid_value, "uid", -1, u32::MAX as i64);
    crate::fs::validate::validate_int32(gid_value, "gid", -1, u32::MAX as i64);
    let cb = required_callback(callback);
    unsafe {
        match crate::fs::js_fs_fchown_result(fd_value, uid_value, gid_value) {
            Ok(()) => call_cb0(cb),
            Err(err_val) => call_cb_err1(cb, err_val),
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.utimes(path, atime, mtime, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_utimes_callback(
    path_value: f64,
    atime_value: f64,
    mtime_value: f64,
    callback: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let cb = required_callback(callback);
    unsafe {
        match crate::fs::js_fs_utimes_result(path_value, atime_value, mtime_value, false) {
            Ok(()) => call_cb0(cb),
            Err(err_val) => call_cb_err1(cb, err_val),
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.lutimes(path, atime, mtime, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_lutimes_callback(
    path_value: f64,
    atime_value: f64,
    mtime_value: f64,
    callback: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let cb = required_callback(callback);
    unsafe {
        match crate::fs::js_fs_utimes_result(path_value, atime_value, mtime_value, true) {
            Ok(()) => call_cb0(cb),
            Err(err_val) => call_cb_err1(cb, err_val),
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.futimes(fd, atime, mtime, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_futimes_callback(
    fd_value: f64,
    atime_value: f64,
    mtime_value: f64,
    callback: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    // Node's `validateInt32(fd)` throws synchronously on a non-numeric fd; the
    // bad/closed descriptor and syscall failures are delivered to the callback
    // as the first argument (#2749).
    crate::fs::validate::validate_fd(fd_value);
    let cb = required_callback(callback);
    unsafe {
        match crate::fs::js_fs_futimes_result(fd_value, atime_value, mtime_value) {
            Ok(()) => call_cb0(cb),
            Err(err_val) => call_cb_err1(cb, err_val),
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.read(fd, buffer, offset, length, position, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_read_callback(
    fd_value: f64,
    buffer_value: f64,
    offset_value: f64,
    length_value: f64,
    position_value: f64,
    callback: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    crate::fs::validate::validate_fd(fd_value);
    let cb = required_callback(callback);
    if let Some(err_val) = crate::fs::validate::fd_open_callback_error(fd_value, "read") {
        crate::closure::js_closure_call3(cb, err_val, 0.0, buffer_value);
        return f64::from_bits(TAG_UNDEFINED);
    }
    let bytes = match crate::fs::read_sync_result(
        fd_value,
        buffer_value,
        offset_value,
        length_value,
        position_value,
    ) {
        Ok(bytes) => bytes,
        // The callback form reports the syscall error, it does not throw.
        Err(err) => {
            let err_val = unsafe { build_fs_error_value_no_path(&err, "read") };
            crate::closure::js_closure_call3(cb, err_val, 0.0, buffer_value);
            return f64::from_bits(TAG_UNDEFINED);
        }
    };
    if !cb.is_null() {
        crate::closure::js_closure_call3(cb, f64::from_bits(TAG_NULL), bytes, buffer_value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.read(fd, buffer, options, callback)` object-options form.
#[no_mangle]
pub extern "C" fn js_fs_read_callback_options(
    fd_value: f64,
    buffer_value: f64,
    options_value: f64,
    callback: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    crate::fs::validate::validate_fd(fd_value);
    let cb = required_callback(callback);
    if let Some(err_val) = crate::fs::validate::fd_open_callback_error(fd_value, "read") {
        crate::closure::js_closure_call3(cb, err_val, 0.0, buffer_value);
        return f64::from_bits(TAG_UNDEFINED);
    }
    let buffer_len = buffer_len_from_value(buffer_value) as f64;
    let offset = unsafe { options_number_field(options_value, b"offset") }.unwrap_or(0.0);
    let length = unsafe { options_number_field(options_value, b"length") }
        .unwrap_or_else(|| (buffer_len - offset).max(0.0));
    let position = unsafe { options_number_field(options_value, b"position") }
        .unwrap_or(f64::from_bits(crate::value::TAG_NULL));
    let bytes = match crate::fs::read_sync_result(fd_value, buffer_value, offset, length, position)
    {
        Ok(bytes) => bytes,
        // The callback form reports the syscall error, it does not throw.
        Err(err) => {
            let err_val = unsafe { build_fs_error_value_no_path(&err, "read") };
            crate::closure::js_closure_call3(cb, err_val, 0.0, buffer_value);
            return f64::from_bits(TAG_UNDEFINED);
        }
    };
    if !cb.is_null() {
        crate::closure::js_closure_call3(cb, f64::from_bits(TAG_NULL), bytes, buffer_value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.write(fd, string, callback)` / deterministic string subset.
#[no_mangle]
pub extern "C" fn js_fs_write_callback(fd_value: f64, data_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    crate::fs::validate::validate_fd(fd_value);
    let cb = required_callback(callback);
    if let Some(err_val) = crate::fs::validate::fd_open_callback_error(fd_value, "write") {
        crate::closure::js_closure_call3(cb, err_val, 0.0, data_value);
        return f64::from_bits(TAG_UNDEFINED);
    }
    let bytes = match crate::fs::write_string_sync_result(
        fd_value as i32,
        data_value,
        f64::from_bits(crate::value::TAG_UNDEFINED),
    ) {
        Ok(bytes) => bytes,
        Err(err) => {
            let err_val = unsafe { build_fs_error_value_no_path(&err, "write") };
            crate::closure::js_closure_call3(cb, err_val, 0.0, data_value);
            return f64::from_bits(TAG_UNDEFINED);
        }
    };
    if !cb.is_null() {
        crate::closure::js_closure_call3(cb, f64::from_bits(TAG_NULL), bytes, data_value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.write(fd, buffer, options, callback)` object-options form.
#[no_mangle]
pub extern "C" fn js_fs_write_buffer_callback_options(
    fd_value: f64,
    buffer_value: f64,
    options_value: f64,
    callback: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    crate::fs::validate::validate_fd(fd_value);
    let cb = required_callback(callback);
    if let Some(err_val) = crate::fs::validate::fd_open_callback_error(fd_value, "write") {
        crate::closure::js_closure_call3(cb, err_val, 0.0, buffer_value);
        return f64::from_bits(TAG_UNDEFINED);
    }
    let buffer_len = buffer_len_from_value(buffer_value) as f64;
    let offset = unsafe { options_number_field(options_value, b"offset") }.unwrap_or(0.0);
    let length = unsafe { options_number_field(options_value, b"length") }
        .unwrap_or_else(|| (buffer_len - offset).max(0.0));
    let position = unsafe { options_number_field(options_value, b"position") }
        .unwrap_or(f64::from_bits(crate::value::TAG_NULL));
    let bytes = match crate::fs::write_buffer_sync_result(
        fd_value as i32,
        buffer_value,
        offset,
        length,
        position,
    ) {
        Ok(bytes) => bytes,
        Err(err) => {
            let err_val = unsafe { build_fs_error_value_no_path(&err, "write") };
            crate::closure::js_closure_call3(cb, err_val, 0.0, buffer_value);
            return f64::from_bits(TAG_UNDEFINED);
        }
    };
    if !cb.is_null() {
        crate::closure::js_closure_call3(cb, f64::from_bits(TAG_NULL), bytes, buffer_value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.write(fd, buffer, offset, length, position, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_write_buffer_callback(
    fd_value: f64,
    buffer_value: f64,
    offset_value: f64,
    length_value: f64,
    position_value: f64,
    callback: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    crate::fs::validate::validate_fd(fd_value);
    let cb = required_callback(callback);
    if let Some(err_val) = crate::fs::validate::fd_open_callback_error(fd_value, "write") {
        crate::closure::js_closure_call3(cb, err_val, 0.0, buffer_value);
        return f64::from_bits(TAG_UNDEFINED);
    }
    let bytes = match crate::fs::write_buffer_sync_result(
        fd_value as i32,
        buffer_value,
        offset_value,
        length_value,
        position_value,
    ) {
        Ok(bytes) => bytes,
        Err(err) => {
            let err_val = unsafe { build_fs_error_value_no_path(&err, "write") };
            crate::closure::js_closure_call3(cb, err_val, 0.0, buffer_value);
            return f64::from_bits(TAG_UNDEFINED);
        }
    };
    if !cb.is_null() {
        crate::closure::js_closure_call3(cb, f64::from_bits(TAG_NULL), bytes, buffer_value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.readv(fd, buffers[, position], callback)`.
#[no_mangle]
pub extern "C" fn js_fs_readv_callback(
    fd_value: f64,
    buffers_value: f64,
    position_value: f64,
    callback: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    crate::fs::validate::validate_fd(fd_value);
    let cb = required_callback(callback);
    if let Some(err_val) = crate::fs::validate::fd_open_callback_error(fd_value, "read") {
        crate::closure::js_closure_call3(cb, err_val, 0.0, buffers_value);
        return f64::from_bits(TAG_UNDEFINED);
    }
    let bytes = js_fs_readv_sync(fd_value, buffers_value, position_value);
    if !cb.is_null() {
        crate::closure::js_closure_call3(cb, f64::from_bits(TAG_NULL), bytes, buffers_value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.writev(fd, buffers[, position], callback)`.
#[no_mangle]
pub extern "C" fn js_fs_writev_callback(
    fd_value: f64,
    buffers_value: f64,
    position_value: f64,
    callback: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    crate::fs::validate::validate_fd(fd_value);
    let cb = required_callback(callback);
    if let Some(err_val) = crate::fs::validate::fd_open_callback_error(fd_value, "write") {
        crate::closure::js_closure_call3(cb, err_val, 0.0, buffers_value);
        return f64::from_bits(TAG_UNDEFINED);
    }
    let bytes = js_fs_writev_sync(fd_value, buffers_value, position_value);
    if !cb.is_null() {
        crate::closure::js_closure_call3(cb, f64::from_bits(TAG_NULL), bytes, buffers_value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.rename(oldPath, newPath, callback)` — sync rename + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_rename_callback(from_value: f64, to_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let cb = required_callback(callback);
    unsafe {
        match crate::fs::js_fs_rename_result(from_value, to_value) {
            Ok(()) => call_cb0(cb),
            Err(err_val) => call_cb_err1(cb, err_val),
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.copyFile(src, dest, callback)` — sync copy + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_copy_file_callback(
    from_value: f64,
    to_value: f64,
    arg2: f64,
    arg3: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let flags = if extract_closure_ptr(arg2).is_null() {
        arg2
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    crate::fs::validate::validate_path("src", from_value);
    crate::fs::validate::validate_path("dest", to_value);
    let cb = callback_or_arg3(arg2, arg3);
    crate::fs::validate::validate_fs_mode(flags);
    unsafe {
        match crate::fs::js_fs_copy_file_result(from_value, to_value, flags) {
            Ok(()) => call_cb0(cb),
            Err(err_val) => call_cb_err1(cb, err_val),
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[cfg(test)]
mod sso_tests_1781 {
    use super::*;

    /// #1781: fs flag strings ("r", "r+", "w", "a", "wx", …) are all <= 5
    /// bytes, so they are inline SSO values (tag 0x7FF9). `is_string()` is
    /// STRING_TAG-only and rejected every one — `decode_flags_string`
    /// returned None for all flags, breaking string flag parsing.
    #[test]
    fn decode_flags_string_handles_sso_flags() {
        for flag in ["r", "r+", "w", "w+", "a", "a+", "wx", "ax", "as"] {
            let v = crate::value::JSValue::try_short_string(flag.as_bytes())
                .expect("flag <= 5 bytes encodes as inline SSO");
            assert!(
                v.is_short_string(),
                "{flag:?} should be an inline SSO value"
            );
            let got = unsafe { decode_flags_string(f64::from_bits(v.bits())) };
            assert_eq!(got.as_deref(), Some(flag), "decode mismatch for {flag:?}");
        }
    }
}
