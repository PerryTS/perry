//! Callback-style fs APIs — pre-flight probe + (err, value) dispatch.

use std::fs;

use crate::closure::ClosureHeader;
use crate::string::js_string_from_bytes;

use super::*;

#[no_mangle]
pub extern "C" fn js_fs_read_file_callback(path_value: f64, encoding: f64, callback: f64) -> f64 {
    use crate::closure::js_closure_call2;
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;

    let cb_ptr = last_callback(&[encoding, callback]);
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

pub(crate) fn last_callback(args: &[f64]) -> *const ClosureHeader {
    for value in args.iter().rev() {
        let ptr = extract_closure_ptr(*value);
        if !ptr.is_null() {
            return ptr;
        }
    }
    std::ptr::null()
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
    let cb = last_callback(&[arg2, arg3]);
    unsafe {
        if let Some(err_val) = fs_callback_write_parent_error(path_value, "open") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_write_file_sync_options(path_value, content_value, options);
    call_cb0(cb);
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
    let cb = last_callback(&[arg2, arg3]);
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
    let _ = js_fs_mkdir_sync_options(path_value, options);
    call_cb0(last_callback(&[arg1, arg2]));
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.unlink(path, callback)` — sync unlink + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_unlink_callback(path_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let cb = last_callback(&[callback]);
    unsafe {
        if let Some(err_val) = fs_callback_lstat_error(path_value, "unlink") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_unlink_sync(path_value);
    call_cb0(cb);
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
    let _ = js_fs_rm_recursive_options(path_value, options);
    call_cb0(last_callback(&[arg1, arg2]));
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.access(path[, mode], callback)` — sync access + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_access_callback(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let mode = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = last_callback(&[arg1, arg2]);
    unsafe {
        if let Some(err_val) = fs_callback_read_error(path_value, "access") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_access_sync_mode(path_value, mode);
    call_cb0(cb);
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.exists(path, callback)` — deprecated Node callback shape:
/// invokes the callback with a single boolean, not `(err, value)`.
#[no_mangle]
pub extern "C" fn js_fs_exists_callback(path_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let exists = js_fs_exists_sync(path_value) == 1;
    let cb = last_callback(&[callback]);
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
    let cb = last_callback(&[arg1, arg2]);
    unsafe {
        if let Some(err_val) = fs_callback_read_error(path_value, "scandir") {
            call_cb_err2(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let entries = js_fs_readdir_sync(path_value, arg1);
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
    let cb = last_callback(&[arg1, arg2]);
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
    let cb = last_callback(&[arg1, arg2]);
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
    let cb = last_callback(&[arg1, arg2]);
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
    let cb = last_callback(&[arg1, arg2]);
    unsafe {
        if let Some(err_val) = fs_callback_read_error(path_value, "opendir") {
            call_cb_err2(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let dir = js_fs_opendir_sync(path_value);
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
    let raw = js_fs_glob_sync_options(pattern_value, options);
    let entries = f64::from_bits(crate::value::JSValue::pointer(raw.to_bits() as *const u8).bits());
    let cb = last_callback(&[arg1, arg2]);
    if !cb.is_null() {
        crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), entries);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.fstat(fd, callback)` — sync fstat + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_fstat_callback(fd_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let options = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let stats = js_fs_fstat_sync_options(fd_value, options);
    let cb = last_callback(&[arg1, arg2]);
    if !cb.is_null() {
        crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), stats);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.chmod(path, mode, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_chmod_callback(path_value: f64, mode_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let cb = last_callback(&[callback]);
    unsafe {
        if let Some(err_val) = fs_callback_read_error(path_value, "chmod") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_chmod_sync(path_value, mode_value);
    call_cb0(cb);
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
    let cb = last_callback(&[callback]);
    unsafe {
        if let Some(err_val) = fs_callback_read_error(path_value, "chown") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_chown_sync(path_value, uid_value, gid_value);
    call_cb0(cb);
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
    let cb = last_callback(&[callback]);
    unsafe {
        if let Some(err_val) = fs_callback_lstat_error(path_value, "lchown") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_lchown_sync(path_value, uid_value, gid_value);
    call_cb0(cb);
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
    let cb = last_callback(&[len_value, callback]);
    unsafe {
        if let Some(err_val) = fs_callback_read_error(path_value, "open") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_truncate_sync(path_value, len);
    call_cb0(cb);
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.link(existingPath, newPath, callback)` — sync hard link + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_link_callback(from_value: f64, to_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let cb = last_callback(&[callback]);
    unsafe {
        if let Some(err_val) = fs_callback_lstat_error(from_value, "link") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_link_sync(from_value, to_value);
    call_cb0(cb);
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
    let _ = js_fs_symlink_sync(from_value, to_value);
    call_cb0(last_callback(&[arg2, arg3]));
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
    let cb = last_callback(&[arg1, arg2]);
    unsafe {
        if let Some(err_val) = fs_callback_lstat_error(path_value, "readlink") {
            call_cb_err2(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let value = js_fs_readlink_dispatch(path_value, options);
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
    let cb = last_callback(&[arg1, arg2]);
    unsafe {
        if let Some(err_val) = fs_callback_read_error(path_value, "realpath") {
            call_cb_err2(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let value = js_fs_realpath_dispatch(path_value, options);
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
    let value = js_fs_mkdtemp_dispatch(prefix_value, options);
    let cb = last_callback(&[arg1, arg2]);
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
    let cb = last_callback(&[arg1, arg2, arg3]);
    let flags = if !extract_closure_ptr(arg1).is_null() {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        arg1
    };
    // Probe only the clear read-only cases (`"r"`, `"r+"`, or undefined ⇒
    // Node defaults to `"r"`). Anything else — `"w"`, `"a"`, numeric flag
    // bitsets like `O_CREAT|O_WRONLY` — may create the file, so we defer
    // to the underlying open instead of pre-rejecting on a missing path.
    let read_only = unsafe { open_flag_is_read_only(flags) };
    if read_only {
        unsafe {
            if let Some(err_val) = fs_callback_read_error(path_value, "open") {
                call_cb_err2(cb, err_val);
                return f64::from_bits(TAG_UNDEFINED);
            }
        }
    }
    let fd = js_fs_open_sync(path_value, flags);
    if !cb.is_null() {
        crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), fd);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// Probe used by `fs/promises.open` to decide whether to resolve with a
/// FileHandle or reject. Returns `Some(err_val)` when the underlying open
/// would fail with ENOENT/EACCES/EEXIST/etc., else `None`.
///
/// Exposed at `pub(crate)` so `node_submodules::thunk_fs_promises_open` can
/// turn the io::Error into a rejected Promise instead of resolving with a
/// FileHandle whose `fd === -1`.
pub(crate) unsafe fn fs_promises_open_probe_error(
    path_value: f64,
    flags_value: f64,
) -> Option<f64> {
    // Only probe for read-only flags; anything that may create the file —
    // including numeric `O_CREAT|…` bitsets — is left to the underlying
    // open so it can succeed when the file doesn't exist yet.
    if open_flag_is_read_only(flags_value) {
        fs_callback_read_error(path_value, "open")
    } else {
        None
    }
}

/// Returns true when an `open` flags value is unambiguously read-only.
/// Treats `undefined` (Node's default) as read-only, the string flags
/// `"r"` and `"r+"` as read-only, and everything else — including any
/// numeric/integer flag — as potentially creating, so the caller skips
/// the missing-path probe and defers to the syscall.
pub(crate) unsafe fn open_flag_is_read_only(flags_value: f64) -> bool {
    let jsval = crate::value::JSValue::from_bits(flags_value.to_bits());
    if jsval.is_undefined() {
        return true;
    }
    match decode_flags_string(flags_value).as_deref() {
        Some("r") | Some("r+") => true,
        _ => false,
    }
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
    let _ = js_fs_close_sync(fd_value);
    call_cb0(last_callback(&[callback]));
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
    let cb = last_callback(&[arg2, arg3]);
    unsafe {
        if let Some(err_val) = fs_callback_lstat_error(from_value, "cp") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_cp_sync_options(from_value, to_value, options);
    call_cb0(cb);
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
    let cb = last_callback(&[arg1, arg2]);
    unsafe {
        if let Some(err_val) = fs_callback_read_error(path_value, "rmdir") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_rmdir_sync_options(path_value, options);
    call_cb0(cb);
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.ftruncate(fd, len, callback)` — sync ftruncate + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_ftruncate_callback(fd_value: f64, len_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let _ = js_fs_ftruncate_sync(fd_value, len_value);
    call_cb0(last_callback(&[callback]));
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.fsync(fd, callback)` — sync fsync + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_fsync_callback(fd_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let _ = js_fs_fsync_sync(fd_value);
    call_cb0(last_callback(&[callback]));
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.fdatasync(fd, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_fdatasync_callback(fd_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let _ = js_fs_fdatasync_sync(fd_value);
    call_cb0(last_callback(&[callback]));
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.fchmod(fd, mode, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_fchmod_callback(fd_value: f64, mode_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let _ = js_fs_fchmod_sync(fd_value, mode_value);
    call_cb0(last_callback(&[callback]));
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
    let _ = js_fs_fchown_sync(fd_value, uid_value, gid_value);
    call_cb0(last_callback(&[callback]));
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
    let _ = js_fs_utimes_sync(path_value, atime_value, mtime_value);
    call_cb0(last_callback(&[callback]));
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
    let _ = js_fs_lutimes_sync(path_value, atime_value, mtime_value);
    call_cb0(last_callback(&[callback]));
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
    let _ = js_fs_futimes_sync(fd_value, atime_value, mtime_value);
    call_cb0(last_callback(&[callback]));
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
    let bytes = js_fs_read_sync(
        fd_value,
        buffer_value,
        offset_value,
        length_value,
        position_value,
    );
    let cb = last_callback(&[callback]);
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
    let buffer_len = buffer_len_from_value(buffer_value) as f64;
    let offset = unsafe { options_number_field(options_value, b"offset") }.unwrap_or(0.0);
    let length = unsafe { options_number_field(options_value, b"length") }
        .unwrap_or_else(|| (buffer_len - offset).max(0.0));
    let position = unsafe { options_number_field(options_value, b"position") }
        .unwrap_or(f64::from_bits(crate::value::TAG_NULL));
    let bytes = js_fs_read_sync(fd_value, buffer_value, offset, length, position);
    let cb = last_callback(&[callback]);
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
    let bytes = js_fs_write_sync(fd_value, data_value);
    let cb = last_callback(&[callback]);
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
    let buffer_len = buffer_len_from_value(buffer_value) as f64;
    let offset = unsafe { options_number_field(options_value, b"offset") }.unwrap_or(0.0);
    let length = unsafe { options_number_field(options_value, b"length") }
        .unwrap_or_else(|| (buffer_len - offset).max(0.0));
    let position = unsafe { options_number_field(options_value, b"position") }
        .unwrap_or(f64::from_bits(crate::value::TAG_NULL));
    let bytes = js_fs_write_buffer_sync(fd_value, buffer_value, offset, length, position);
    let cb = last_callback(&[callback]);
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
    let bytes = js_fs_write_buffer_sync(
        fd_value,
        buffer_value,
        offset_value,
        length_value,
        position_value,
    );
    let cb = last_callback(&[callback]);
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
    let bytes = js_fs_readv_sync(fd_value, buffers_value, position_value);
    let cb = last_callback(&[callback]);
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
    let bytes = js_fs_writev_sync(fd_value, buffers_value, position_value);
    let cb = last_callback(&[callback]);
    if !cb.is_null() {
        crate::closure::js_closure_call3(cb, f64::from_bits(TAG_NULL), bytes, buffers_value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.rename(oldPath, newPath, callback)` — sync rename + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_rename_callback(from_value: f64, to_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let cb = last_callback(&[callback]);
    unsafe {
        if let Some(err_val) = fs_callback_lstat_error(from_value, "rename") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_rename_sync(from_value, to_value);
    call_cb0(cb);
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
    let cb = last_callback(&[arg2, arg3]);
    unsafe {
        if let Some(err_val) = fs_callback_read_error(from_value, "copyfile") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_copy_file_sync_flags(from_value, to_value, flags);
    call_cb0(cb);
    f64::from_bits(TAG_UNDEFINED)
}

#[cfg(test)]
mod sso_tests_1781 {
    use super::*;

    /// #1781: fs flag strings ("r", "r+", "w", "a", "wx", …) are all <= 5
    /// bytes, so they are inline SSO values (tag 0x7FF9). `is_string()` is
    /// STRING_TAG-only and rejected every one — `decode_flags_string`
    /// returned None for all flags, breaking the read-only fast-path probe.
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

    #[test]
    fn open_flag_is_read_only_recognizes_sso_flags() {
        let r = crate::value::JSValue::try_short_string(b"r").unwrap();
        assert!(unsafe { open_flag_is_read_only(f64::from_bits(r.bits())) });
        let w = crate::value::JSValue::try_short_string(b"w").unwrap();
        assert!(!unsafe { open_flag_is_read_only(f64::from_bits(w.bits())) });
    }
}
