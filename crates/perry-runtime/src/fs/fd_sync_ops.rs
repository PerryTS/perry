use super::*;

use std::fs;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};
#[cfg(unix)]
use std::os::unix::io::AsRawFd;

/// Core path-based `truncate` op. Node surfaces the `open` syscall error
/// when the path can't be opened for truncation (ENOENT / EISDIR / EACCES),
/// so failures are reported with `code`/`syscall: "open"`/`path` (#2743)
/// instead of collapsing to a silent no-op.
pub(crate) unsafe fn js_fs_truncate_result(path_value: f64, len_value: f64) -> Result<(), f64> {
    validate::validate_path("path", path_value);
    let path_str = match decode_path_value(path_value) {
        Some(s) => s,
        None => return Ok(()),
    };
    let len = if len_value.is_finite() && len_value >= 0.0 {
        len_value as u64
    } else {
        0
    };
    match fs::OpenOptions::new().write(true).open(&path_str) {
        Ok(file) => match file.set_len(len) {
            Ok(()) => Ok(()),
            Err(err) => Err(build_fs_error_value(&err, "ftruncate", &path_str)),
        },
        Err(err) => Err(build_fs_error_value(&err, "open", &path_str)),
    }
}

/// `fs.truncateSync(path, len)` — truncate/extend a file by path.
#[no_mangle]
pub extern "C" fn js_fs_truncate_sync(path_value: f64, len_value: f64) -> i32 {
    validate::validate_path("path", path_value);
    unsafe {
        match js_fs_truncate_result(path_value, len_value) {
            Ok(()) => 1,
            Err(err_val) => crate::exception::js_throw(err_val),
        }
    }
}

/// Core fd-based `ftruncate` op. Surfaces `EBADF` for a closed/unknown fd and
/// the underlying syscall error (e.g. `EINVAL`) when `set_len` fails, instead
/// of collapsing to a silent status-0 (#2749). Returns a NaN-boxed Node-shaped
/// fs error carrying `code`/`syscall: "ftruncate"`.
pub(crate) unsafe fn js_fs_ftruncate_result(fd_value: f64, len_value: f64) -> Result<(), f64> {
    let fd = fd_value as i32;
    let len = if len_value.is_finite() && len_value >= 0.0 {
        len_value as u64
    } else {
        0
    };
    FD_REGISTRY.with(|r| {
        let reg = r.borrow();
        let Some(file) = reg.get(&fd) else {
            return Err(crate::fs::validate::build_ebadf_error_value("ftruncate"));
        };
        match file.set_len(len) {
            Ok(()) => Ok(()),
            Err(err) => Err(build_fs_error_value_no_path(&err, "ftruncate")),
        }
    })
}

/// `fs.ftruncateSync(fd, len)` — truncate/extend an open registry fd.
#[no_mangle]
pub extern "C" fn js_fs_ftruncate_sync(fd_value: f64, len_value: f64) -> i32 {
    crate::fs::validate::validate_fd(fd_value);
    unsafe {
        match js_fs_ftruncate_result(fd_value, len_value) {
            Ok(()) => 1,
            Err(err_val) => crate::exception::js_throw(err_val),
        }
    }
}

/// `fs.fsyncSync(fd)` — flush an open registry fd.
#[no_mangle]
pub extern "C" fn js_fs_fsync_sync(fd_value: f64) -> i32 {
    crate::fs::validate::validate_fd_open(fd_value, "fsync");
    fsync_sync_inner(fd_value as i32)
}

/// Internal fsync without validation — for the FileHandle wrappers, which
/// may legitimately hold a `-1` sentinel from a failed open and rely on
/// the silent no-op behavior.
pub(crate) fn fsync_sync_inner(fd: i32) -> i32 {
    FD_REGISTRY.with(|r| {
        let reg = r.borrow();
        let Some(file) = reg.get(&fd) else {
            return 0;
        };
        if file.sync_all().is_ok() {
            1
        } else {
            0
        }
    })
}

/// `fs.fdatasyncSync(fd)` — flush file data for an open registry fd.
/// Perry maps this to `sync_data`, falling back to fsync-like semantics.
#[no_mangle]
pub extern "C" fn js_fs_fdatasync_sync(fd_value: f64) -> i32 {
    crate::fs::validate::validate_fd_open(fd_value, "fdatasync");
    fdatasync_sync_inner(fd_value as i32)
}

pub(crate) fn fdatasync_sync_inner(fd: i32) -> i32 {
    FD_REGISTRY.with(|r| {
        let reg = r.borrow();
        let Some(file) = reg.get(&fd) else {
            return 0;
        };
        if file.sync_data().is_ok() {
            1
        } else {
            0
        }
    })
}

/// `fs.fchmodSync(fd, mode)`.
#[no_mangle]
pub extern "C" fn js_fs_fchmod_sync(fd_value: f64, mode: f64) -> i32 {
    // #2013: fd validation (type + range) + EBADF on missing fd. Mode
    // validation deliberately omitted — Node uses `parseFileMode`,
    // which throws `ERR_INVALID_ARG_VALUE`, before the fd check; adding
    // the same shape here is a separate follow-up tracked alongside the
    // mode-on-existing-path gap in `lchmodSync`.
    crate::fs::validate::validate_fd_open(fd_value, "fchmod");
    let fd = fd_value as i32;
    FD_REGISTRY.with(|r| {
        let reg = r.borrow();
        let Some(file) = reg.get(&fd) else {
            return 0;
        };
        #[cfg(unix)]
        {
            let perms = fs::Permissions::from_mode(mode as u32);
            if file.set_permissions(perms).is_ok() {
                1
            } else {
                0
            }
        }
        #[cfg(not(unix))]
        {
            let _ = (file, mode);
            1
        }
    })
}

/// `fs.fchownSync(fd, uid, gid)`.
#[no_mangle]
pub extern "C" fn js_fs_fchown_sync(fd_value: f64, uid_value: f64, gid_value: f64) -> i32 {
    match js_fs_fchown_result(fd_value, uid_value, gid_value) {
        Ok(()) => 1,
        Err(err) => crate::exception::js_throw(err),
    }
}

pub(crate) fn js_fs_fchown_result(
    fd_value: f64,
    uid_value: f64,
    gid_value: f64,
) -> Result<(), f64> {
    // #2013 order: validate fd type, uid type+range, gid type+range,
    // THEN bounce on EBADF. Node's `validateInteger(uid)` fires before
    // the syscall, so `fchownSync(1, "", 0)` throws ERR_INVALID_ARG_TYPE
    // for `uid`, not EBADF for `fd` — preserve that order even though
    // the missing-fd case still needs EBADF after all args check out.
    crate::fs::validate::validate_fd(fd_value);
    crate::fs::validate::validate_int32(uid_value, "uid", -1, u32::MAX as i64);
    crate::fs::validate::validate_int32(gid_value, "gid", -1, u32::MAX as i64);
    if !crate::fs::fd_is_registered(fd_value as i32) {
        return Err(crate::fs::validate::build_ebadf_error_value("fchown"));
    }
    unsafe { fchown_sync_inner_result(fd_value as i32, uid_value, gid_value) }
}

/// Core fd-based `fchown` op. Surfaces the syscall failure (e.g. `EPERM` for a
/// non-root chown) as a NaN-boxed Node-shaped fs error with `code`/`syscall:
/// "fchown"` instead of collapsing to a silent status-0 (#2749). Assumes the
/// fd has already been validated/registered; a missing fd returns `EBADF`.
pub(crate) unsafe fn fchown_sync_inner_result(
    fd: i32,
    uid_value: f64,
    gid_value: f64,
) -> Result<(), f64> {
    #[cfg(unix)]
    {
        FD_REGISTRY.with(|r| {
            let reg = r.borrow();
            let Some(file) = reg.get(&fd) else {
                return Err(crate::fs::validate::build_ebadf_error_value("fchown"));
            };
            let rc = libc::fchown(
                file.as_raw_fd(),
                uid_value as libc::uid_t,
                gid_value as libc::gid_t,
            );
            if rc == 0 {
                Ok(())
            } else {
                Err(build_fs_error_value_no_path(
                    &std::io::Error::last_os_error(),
                    "fchown",
                ))
            }
        })
    }
    #[cfg(not(unix))]
    {
        let _ = (fd, uid_value, gid_value);
        Ok(())
    }
}

pub(crate) fn fchown_sync_inner(fd: i32, uid_value: f64, gid_value: f64) -> i32 {
    unsafe {
        match fchown_sync_inner_result(fd, uid_value, gid_value) {
            Ok(()) => 1,
            Err(_) => 0,
        }
    }
}

/// `fs.fstatSync(fd)` — return the same Stats shape as `statSync`.
#[no_mangle]
pub extern "C" fn js_fs_fstat_sync(fd_value: f64) -> f64 {
    js_fs_fstat_sync_options(fd_value, f64::from_bits(crate::value::TAG_UNDEFINED))
}

#[no_mangle]
pub extern "C" fn js_fs_fstat_sync_options(fd_value: f64, options_value: f64) -> f64 {
    crate::fs::validate::validate_fd(fd_value);
    let bigint = unsafe { options_bool_field(options_value, b"bigint") };
    let fd = fd_value as i32;
    match fstat_stats_value(fd, bigint) {
        Ok(stats) => stats,
        Err(err) => crate::exception::js_throw(err),
    }
}

pub(crate) fn fstat_stats_value(fd: i32, bigint: bool) -> Result<f64, f64> {
    FD_REGISTRY.with(|r| {
        let reg = r.borrow();
        let Some(file) = reg.get(&fd) else {
            return Err(crate::fs::validate::build_ebadf_error_value("fstat"));
        };
        match file.metadata() {
            Ok(meta) => {
                let ft = meta.file_type();
                #[cfg(unix)]
                let mode = meta.permissions().mode();
                #[cfg(not(unix))]
                let mode = if meta.permissions().readonly() {
                    0o444
                } else {
                    0o666
                };
                let (uid, gid) = metadata_owner_ids(&meta);
                let nlink = metadata_nlink(&meta);
                let (atime, mtime, ctime, birth) = metadata_times_ms(&meta);
                Ok(unsafe {
                    build_stats_object(
                        ft.is_file(),
                        ft.is_dir(),
                        ft.is_symlink(),
                        meta.len(),
                        mode,
                        uid,
                        gid,
                        nlink,
                        atime,
                        mtime,
                        ctime,
                        birth,
                        bigint,
                        Some(&meta),
                    )
                })
            }
            Err(_) => Err(crate::fs::validate::build_ebadf_error_value("fstat")),
        }
    })
}
