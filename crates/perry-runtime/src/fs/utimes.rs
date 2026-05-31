//! `utimes` / `lutimes` / `futimes` (and the shared time-argument coercion +
//! `timespec` conversion), split out of `fs/mod.rs` to keep it under the 2k
//! limit. `use super::*` pulls in the private `validate`, `decode_path_value`,
//! `build_fs_error_value`, and fd-validation helpers.

use super::*;

#[cfg(unix)]
fn seconds_to_timespec(seconds: f64) -> libc::timespec {
    let safe = if seconds.is_finite() {
        seconds
    } else {
        current_unix_timestamp_seconds()
    };
    let seconds_floor = safe.floor();
    let mut secs = seconds_floor as libc::time_t;
    let mut nanos = ((safe - seconds_floor) * 1_000_000_000.0).round() as libc::c_long;
    if nanos >= 1_000_000_000 {
        secs += 1;
        nanos = 0;
    }
    libc::timespec {
        tv_sec: secs,
        tv_nsec: nanos.clamp(0, 999_999_999),
    }
}

fn current_unix_timestamp_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn invalid_time_arg(arg_name: &str, value: f64) -> ! {
    let message = format!(
        "The \"{}\" argument must be an instance of Date or an Time in seconds. Received {}",
        arg_name,
        validate::describe_received(value)
    );
    validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

pub(crate) fn to_unix_timestamp_or_throw(time_value: f64, arg_name: &str) -> f64 {
    let js_value = crate::value::JSValue::from_bits(time_value.to_bits());
    if js_value.is_any_string() {
        let number = crate::builtins::js_number_coerce(time_value);
        if number.is_nan() {
            invalid_time_arg(arg_name, time_value);
        }
        return number;
    }
    if js_value.is_int32() {
        let number = js_value.as_int32() as f64;
        return if number < 0.0 {
            current_unix_timestamp_seconds()
        } else {
            number
        };
    }
    if js_value.is_number() {
        let number = js_value.as_number();
        if !number.is_finite() {
            invalid_time_arg(arg_name, time_value);
        }
        return if number < 0.0 {
            current_unix_timestamp_seconds()
        } else {
            number
        };
    }
    if crate::date::is_date_value(time_value) {
        return crate::date::date_cell_timestamp(time_value) / 1000.0;
    }
    invalid_time_arg(arg_name, time_value);
}

/// Apply timestamps to a path. On `utimensat` failure builds a Node-shaped fs
/// error with the real errno and `syscall: "utime"`/`"lutime"` and `path`
/// (#2745). `Ok(())` on success.
#[cfg(unix)]
pub(crate) fn set_path_times_result(
    path: &str,
    atime: f64,
    mtime: f64,
    nofollow: bool,
) -> Result<(), f64> {
    let c_path = match std::ffi::CString::new(path) {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };
    let times = [seconds_to_timespec(atime), seconds_to_timespec(mtime)];
    let flags = if nofollow {
        libc::AT_SYMLINK_NOFOLLOW
    } else {
        0
    };
    unsafe {
        if libc::utimensat(libc::AT_FDCWD, c_path.as_ptr(), times.as_ptr(), flags) == 0 {
            Ok(())
        } else {
            let syscall = if nofollow { "lutime" } else { "utime" };
            Err(build_fs_error_value(
                &std::io::Error::last_os_error(),
                syscall,
                path,
            ))
        }
    }
}

/// Shared `utimes`/`lutimes` op for sync/callback/promise wrappers (#2745).
pub(crate) unsafe fn js_fs_utimes_result(
    path_value: f64,
    atime_value: f64,
    mtime_value: f64,
    nofollow: bool,
) -> Result<(), f64> {
    validate::validate_path("path", path_value);
    let atime = to_unix_timestamp_or_throw(atime_value, "time");
    let mtime = to_unix_timestamp_or_throw(mtime_value, "time");
    let path = match decode_path_value(path_value) {
        Some(s) => s,
        None => return Ok(()),
    };
    #[cfg(unix)]
    {
        set_path_times_result(&path, atime, mtime, nofollow)
    }
    #[cfg(not(unix))]
    {
        let _ = (path, atime, mtime, nofollow);
        Ok(())
    }
}

/// `fs.utimesSync(path, atime, mtime)`.
#[no_mangle]
pub extern "C" fn js_fs_utimes_sync(path_value: f64, atime_value: f64, mtime_value: f64) -> i32 {
    validate::validate_path("path", path_value);
    let atime = to_unix_timestamp_or_throw(atime_value, "time");
    let mtime = to_unix_timestamp_or_throw(mtime_value, "time");
    unsafe {
        match js_fs_utimes_result(path_value, atime, mtime, false) {
            Ok(()) => 1,
            Err(err_val) => crate::exception::js_throw(err_val),
        }
    }
}

/// `fs.lutimesSync(path, atime, mtime)`.
#[no_mangle]
pub extern "C" fn js_fs_lutimes_sync(path_value: f64, atime_value: f64, mtime_value: f64) -> i32 {
    validate::validate_path("path", path_value);
    let atime = to_unix_timestamp_or_throw(atime_value, "time");
    let mtime = to_unix_timestamp_or_throw(mtime_value, "time");
    unsafe {
        match js_fs_utimes_result(path_value, atime, mtime, true) {
            Ok(()) => 1,
            Err(err_val) => crate::exception::js_throw(err_val),
        }
    }
}

/// Core fd-based `futimes` op. Surfaces `EBADF` for a closed/unknown fd and
/// the underlying `futimens` syscall failure as a NaN-boxed Node-shaped fs
/// error with `code`/`syscall: "futime"` instead of a silent status-0 (#2749).
/// Node names this syscall `"futime"` (singular), so the error mirrors that.
pub(crate) unsafe fn js_fs_futimes_result(
    fd_value: f64,
    atime_value: f64,
    mtime_value: f64,
) -> Result<(), f64> {
    let atime = to_unix_timestamp_or_throw(atime_value, "atime");
    let mtime = to_unix_timestamp_or_throw(mtime_value, "mtime");
    let fd = fd_value as i32;
    FD_REGISTRY.with(|r| {
        let reg = r.borrow();
        let Some(file) = reg.get(&fd) else {
            return Err(crate::fs::validate::build_ebadf_error_value("futime"));
        };
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let times = [seconds_to_timespec(atime), seconds_to_timespec(mtime)];
            if libc::futimens(file.as_raw_fd(), times.as_ptr()) == 0 {
                Ok(())
            } else {
                Err(build_fs_error_value_no_path(
                    &std::io::Error::last_os_error(),
                    "futime",
                ))
            }
        }
        #[cfg(not(unix))]
        {
            let _ = (file, atime, mtime);
            Ok(())
        }
    })
}

/// `fs.futimesSync(fd, atime, mtime)`.
#[no_mangle]
pub extern "C" fn js_fs_futimes_sync(fd_value: f64, atime_value: f64, mtime_value: f64) -> i32 {
    crate::fs::validate::validate_fd(fd_value);
    unsafe {
        match js_fs_futimes_result(fd_value, atime_value, mtime_value) {
            Ok(()) => 1,
            Err(err_val) => crate::exception::js_throw(err_val),
        }
    }
}
