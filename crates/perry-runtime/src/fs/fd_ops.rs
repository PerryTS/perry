//! POSIX fd ops: open/close/read/write/readv/writev, mkdtemp, realpath,
//! readlink, link/symlink, rename, truncate, fchmod/fchown/fstat/fsync/
//! fdatasync/ftruncate/futimes/utimes/lutimes/rmdir + statfs builder.

use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};

use crate::closure::ClosureHeader;

use super::*;

pub(crate) fn array_ptr_from_value(value: f64) -> *const crate::array::ArrayHeader {
    let raw = (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
    if raw < 0x1000 {
        std::ptr::null()
    } else {
        raw as *const crate::array::ArrayHeader
    }
}

fn open_options_from_flags(flags_value: f64) -> (fs::OpenOptions, bool) {
    let mut opts = fs::OpenOptions::new();
    let append_mode;
    if flags_value.is_finite() {
        let flags = flags_value as i32;
        append_mode = apply_numeric_open_flags(&mut opts, flags);
    } else {
        let flags = flag_string(flags_value);
        append_mode = matches!(flags.as_str(), "a" | "a+" | "ax" | "ax+");
        match flags.as_str() {
            "r" | "rs" => {
                opts.read(true);
            }
            "r+" | "rs+" => {
                opts.read(true).write(true);
            }
            "w" => {
                opts.write(true).create(true).truncate(true);
            }
            "w+" => {
                opts.read(true).write(true).create(true).truncate(true);
            }
            "a" => {
                opts.write(true).create(true).append(true);
            }
            "a+" => {
                opts.read(true).write(true).create(true).append(true);
            }
            "wx" => {
                opts.write(true).create_new(true);
            }
            "wx+" => {
                opts.read(true).write(true).create_new(true);
            }
            "ax" => {
                opts.write(true).create_new(true).append(true);
            }
            "ax+" => {
                opts.read(true).write(true).create_new(true).append(true);
            }
            _ => {
                opts.read(true);
            }
        }
    }
    (opts, append_mode)
}

#[cfg(unix)]
fn apply_numeric_open_flags(opts: &mut fs::OpenOptions, flags: i32) -> bool {
    use std::os::unix::fs::OpenOptionsExt;
    match flags & libc::O_ACCMODE {
        libc::O_WRONLY => {
            opts.write(true);
        }
        libc::O_RDWR => {
            opts.read(true).write(true);
        }
        _ => {
            opts.read(true);
        }
    }
    if flags & libc::O_CREAT != 0 && flags & libc::O_EXCL != 0 {
        opts.create_new(true);
    } else if flags & libc::O_CREAT != 0 {
        opts.create(true);
    }
    if flags & libc::O_TRUNC != 0 {
        opts.truncate(true);
    }
    let append_mode = flags & libc::O_APPEND != 0;
    if append_mode {
        opts.append(true).write(true);
    }
    // Everything `OpenOptions` does not model — O_NONBLOCK, O_NOCTTY, O_NOFOLLOW,
    // O_SYNC, O_CLOEXEC… — still has to reach open(2); Node passes the flag word
    // through verbatim. Dropping O_NONBLOCK left the descriptor blocking, so a
    // `readSync` that Node answers with EAGAIN hung forever instead.
    // `custom_flags` cannot clobber the access mode: std masks O_ACCMODE out.
    const MODELED: i32 =
        libc::O_ACCMODE | libc::O_CREAT | libc::O_EXCL | libc::O_TRUNC | libc::O_APPEND;
    opts.custom_flags(flags & !MODELED);
    append_mode
}

#[cfg(not(unix))]
fn apply_numeric_open_flags(opts: &mut fs::OpenOptions, flags: i32) -> bool {
    let append_mode = flags & 0x8 != 0;
    let access = flags & 0x3;
    match access {
        1 => {
            opts.write(true);
        }
        2 => {
            opts.read(true).write(true);
        }
        _ => {
            opts.read(true);
        }
    }
    if flags & 0x200 != 0 && flags & 0x800 != 0 {
        opts.create_new(true);
    } else if flags & 0x200 != 0 {
        opts.create(true);
    }
    if flags & 0x400 != 0 {
        opts.truncate(true);
    }
    if append_mode {
        opts.append(true).write(true);
    }
    append_mode
}

pub(crate) unsafe fn fs_open_sync_result(
    path_value: f64,
    flags_value: f64,
) -> Result<i32, (std::io::Error, String)> {
    crate::fs::validate::validate_path("path", path_value);
    let path_str = match decode_path_value(path_value) {
        Some(s) => s,
        None => {
            return Err((
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid path"),
                String::new(),
            ));
        }
    };
    let (opts, append_mode) = open_options_from_flags(flags_value);
    match opts.open(&path_str) {
        Ok(file) => {
            let fd = allocate_synthetic_fd();
            FD_REGISTRY.with(|r| {
                r.borrow_mut().insert(fd, file);
            });
            FD_PATHS.with(|r| {
                r.borrow_mut().insert(fd, path_str.to_string());
            });
            FD_APPEND_MODE.with(|r| {
                r.borrow_mut().insert(fd, append_mode);
            });
            Ok(fd)
        }
        Err(err) => Err((err, path_str)),
    }
}

/// `fs.openSync(path, flags)` — small fd registry for deterministic tests.
#[no_mangle]
pub extern "C" fn js_fs_open_sync(path_value: f64, flags_value: f64) -> f64 {
    unsafe {
        match fs_open_sync_result(path_value, flags_value) {
            Ok(fd) => fd as f64,
            Err((err, path)) => {
                crate::exception::js_throw(build_fs_error_value(&err, "open", &path))
            }
        }
    }
}

/// `fs.closeSync(fd)` — close a registry fd.
#[no_mangle]
pub extern "C" fn js_fs_close_sync(fd_value: f64) -> i32 {
    crate::fs::validate::validate_fd_open(fd_value, "close");
    let fd = fd_value as i32;
    FD_REGISTRY.with(|r| {
        if r.borrow_mut().remove(&fd).is_some() {
            FD_PATHS.with(|paths| {
                paths.borrow_mut().remove(&fd);
            });
            FD_APPEND_MODE.with(|flags| {
                flags.borrow_mut().remove(&fd);
            });
            1
        } else {
            0
        }
    })
}

/// `fs.readSync(fd, buffer, offset, length, position)` — Buffer subset.
#[no_mangle]
pub extern "C" fn js_fs_read_sync(
    fd_value: f64,
    buffer_value: f64,
    offset_value: f64,
    length_value: f64,
    position_value: f64,
) -> f64 {
    crate::fs::validate::validate_fd_open(fd_value, "read");
    match read_sync_result(
        fd_value,
        buffer_value,
        offset_value,
        length_value,
        position_value,
    ) {
        Ok(bytes) => bytes,
        // Node surfaces the syscall error (EAGAIN on a non-blocking fd with no
        // data ready, EISDIR, EIO…). Collapsing every failure to 0 made it
        // indistinguishable from a clean EOF.
        Err(err) => unsafe {
            crate::exception::js_throw(build_fs_error_value_no_path(&err, "read"))
        },
    }
}

/// The `readSync` core, with the syscall error preserved so the throwing
/// (`fs.readSync`) and reporting (`fs.read(…, cb)`) surfaces can each render it
/// the way Node does.
pub(crate) fn read_sync_result(
    fd_value: f64,
    buffer_value: f64,
    offset_value: f64,
    length_value: f64,
    position_value: f64,
) -> Result<f64, std::io::Error> {
    let fd = fd_value as i32;
    let offset = offset_value.max(0.0) as usize;
    let length = length_value.max(0.0) as usize;
    let position = if position_value.is_finite() && position_value >= 0.0 {
        Some(position_value as u64)
    } else {
        None
    };
    let buf = buffer_ptr_from_value(buffer_value);
    if buf.is_null() {
        return Ok(0.0);
    }
    FD_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let Some(file) = reg.get_mut(&fd) else {
            return Ok(0.0);
        };
        let restore_pos = position.and_then(|_| file.stream_position().ok());
        if let Some(pos) = position {
            let _ = file.seek(SeekFrom::Start(pos));
        }
        unsafe {
            let cap = (*buf).length as usize;
            if offset >= cap {
                if let Some(pos) = restore_pos {
                    let _ = file.seek(SeekFrom::Start(pos));
                }
                return Ok(0.0);
            }
            let n = length.min(cap - offset);
            let data = crate::buffer::buffer_data_mut(buf).add(offset);
            let result = match file.read(std::slice::from_raw_parts_mut(data, n)) {
                Ok(read) => Ok(read as f64),
                Err(err) => Err(err),
            };
            if let Some(pos) = restore_pos {
                let _ = file.seek(SeekFrom::Start(pos));
            }
            result
        }
    })
}

#[no_mangle]
pub extern "C" fn js_fs_read_sync_options(
    fd_value: f64,
    buffer_value: f64,
    options_value: f64,
) -> f64 {
    unsafe {
        let offset = options_number_field(options_value, b"offset").unwrap_or(0.0);
        let length = options_number_field(options_value, b"length")
            .unwrap_or_else(|| buffer_len_from_value(buffer_value) as f64 - offset.max(0.0));
        let position = options_field_value(options_value, b"position")
            .map(|v| f64::from_bits(v.bits()))
            .unwrap_or_else(|| f64::from_bits(crate::value::TAG_NULL));
        js_fs_read_sync(fd_value, buffer_value, offset, length, position)
    }
}

/// `fs.writeSync(fd, string)` — string subset.
#[no_mangle]
pub extern "C" fn js_fs_write_sync(fd_value: f64, data_value: f64) -> f64 {
    js_fs_write_string_sync_options(
        fd_value,
        data_value,
        f64::from_bits(crate::value::TAG_UNDEFINED),
    )
}

/// `fs.writeSync(fd, string[, position[, encoding]])`.
#[no_mangle]
pub extern "C" fn js_fs_write_string_sync_options(
    fd_value: f64,
    data_value: f64,
    position_value: f64,
) -> f64 {
    crate::fs::validate::validate_fd_open(fd_value, "write");
    write_string_sync_inner(fd_value as i32, data_value, position_value)
}

pub(crate) fn write_string_sync_inner(fd: i32, data_value: f64, position_value: f64) -> f64 {
    let bytes = bytes_from_value(data_value);
    let position = if position_value.is_finite() && position_value >= 0.0 {
        Some(position_value as u64)
    } else {
        None
    };
    FD_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let Some(file) = reg.get_mut(&fd) else {
            return 0.0;
        };
        let restore_pos = position.and_then(|_| file.stream_position().ok());
        if let Some(pos) = position {
            let _ = file.seek(SeekFrom::Start(pos));
        }
        let result = match file.write(&bytes) {
            Ok(n) => n as f64,
            Err(_) => 0.0,
        };
        if let Some(pos) = restore_pos {
            let _ = file.seek(SeekFrom::Start(pos));
        }
        result
    })
}

/// `fs.writeSync(fd, buffer, offset, length, position)` — Buffer subset.
#[no_mangle]
pub extern "C" fn js_fs_write_buffer_sync(
    fd_value: f64,
    buffer_value: f64,
    offset_value: f64,
    length_value: f64,
    position_value: f64,
) -> f64 {
    crate::fs::validate::validate_fd_open(fd_value, "write");
    write_buffer_sync_inner(
        fd_value as i32,
        buffer_value,
        offset_value,
        length_value,
        position_value,
    )
}

pub(crate) fn write_buffer_sync_inner(
    fd: i32,
    buffer_value: f64,
    offset_value: f64,
    length_value: f64,
    position_value: f64,
) -> f64 {
    let offset = offset_value.max(0.0) as usize;
    let length = length_value.max(0.0) as usize;
    let position = if position_value.is_finite() && position_value >= 0.0 {
        Some(position_value as u64)
    } else {
        None
    };
    let buf = buffer_ptr_from_value(buffer_value);
    if buf.is_null() {
        return 0.0;
    }
    FD_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let Some(file) = reg.get_mut(&fd) else {
            return 0.0;
        };
        let restore_pos = position.and_then(|_| file.stream_position().ok());
        if let Some(pos) = position {
            let _ = file.seek(SeekFrom::Start(pos));
        }
        unsafe {
            let cap = (*buf).length as usize;
            if offset >= cap {
                if let Some(pos) = restore_pos {
                    let _ = file.seek(SeekFrom::Start(pos));
                }
                return 0.0;
            }
            let n = length.min(cap - offset);
            let data = crate::buffer::buffer_data(buf).add(offset);
            let result = match file.write(std::slice::from_raw_parts(data, n)) {
                Ok(written) => written as f64,
                Err(_) => 0.0,
            };
            if let Some(pos) = restore_pos {
                let _ = file.seek(SeekFrom::Start(pos));
            }
            result
        }
    })
}

#[no_mangle]
pub extern "C" fn js_fs_write_sync_options_dispatch(
    fd_value: f64,
    data_value: f64,
    options_value: f64,
) -> f64 {
    unsafe {
        if options_field_value(options_value, b"offset").is_some()
            || options_field_value(options_value, b"length").is_some()
            || options_field_value(options_value, b"position").is_some()
        {
            let offset = options_number_field(options_value, b"offset").unwrap_or(0.0);
            let length = options_number_field(options_value, b"length")
                .unwrap_or_else(|| buffer_len_from_value(data_value) as f64 - offset.max(0.0));
            let position = options_field_value(options_value, b"position")
                .map(|v| f64::from_bits(v.bits()))
                .unwrap_or_else(|| f64::from_bits(crate::value::TAG_NULL));
            js_fs_write_buffer_sync(fd_value, data_value, offset, length, position)
        } else {
            js_fs_write_string_sync_options(fd_value, data_value, options_value)
        }
    }
}

/// `fs.readvSync(fd, buffers[, position])` — deterministic Buffer[] subset.
#[no_mangle]
pub extern "C" fn js_fs_readv_sync(fd_value: f64, buffers_value: f64, position_value: f64) -> f64 {
    // #2013: always reject a non-number fd; only run the EBADF registry
    // probe when the iovec array is non-empty. Node's empty-array path
    // surfaces `EINVAL` from the syscall instead of the JS-level EBADF —
    // matching that exact code would need an `EINVAL` throw here that
    // doesn't fit the validate-then-pass shape, so we settle for the
    // common-case `readv(123, [buf])` → `EBADF` parity and leave the
    // empty-array divergence as a follow-up.
    let buffers_for_check = array_ptr_from_value(buffers_value);
    let buffers_nonempty = !buffers_for_check.is_null()
        && unsafe { crate::array::js_array_length(buffers_for_check) } > 0;
    if buffers_nonempty {
        crate::fs::validate::validate_fd_open(fd_value, "read");
    } else {
        crate::fs::validate::validate_fd(fd_value);
    }
    let fd = fd_value as i32;
    let position = if position_value.is_finite() && position_value >= 0.0 {
        Some(position_value as u64)
    } else {
        None
    };
    let buffers = array_ptr_from_value(buffers_value);
    if buffers.is_null() {
        return 0.0;
    }
    FD_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let Some(file) = reg.get_mut(&fd) else {
            return 0.0;
        };
        let restore_pos = position.and_then(|_| file.stream_position().ok());
        if let Some(pos) = position {
            let _ = file.seek(SeekFrom::Start(pos));
        }
        let mut total = 0usize;
        unsafe {
            let len = crate::array::js_array_length(buffers);
            for i in 0..len {
                let value = crate::array::js_array_get_f64(buffers, i);
                let buf = buffer_ptr_from_value(value);
                if buf.is_null() {
                    continue;
                }
                let cap = (*buf).length as usize;
                if cap == 0 {
                    continue;
                }
                let data = crate::buffer::buffer_data_mut(buf);
                // Node's readv fills each iovec completely (short read only
                // at EOF). Use `read` in a loop so we don't return partially
                // filled buffers when the kernel splits the read.
                let mut filled = 0usize;
                let mut eof = false;
                while filled < cap {
                    let slice = std::slice::from_raw_parts_mut(data.add(filled), cap - filled);
                    match file.read(slice) {
                        Ok(0) => {
                            eof = true;
                            break;
                        }
                        Ok(n) => filled += n,
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => {
                            eof = true;
                            break;
                        }
                    }
                }
                total += filled;
                if eof {
                    break;
                }
            }
        }
        if let Some(pos) = restore_pos {
            let _ = file.seek(SeekFrom::Start(pos));
        }
        total as f64
    })
}

/// `fs.writevSync(fd, buffers[, position])` — deterministic Buffer[] subset.
#[no_mangle]
pub extern "C" fn js_fs_writev_sync(fd_value: f64, buffers_value: f64, position_value: f64) -> f64 {
    // Match Node: skip fd validation when the buffers array is empty (Node's
    // own writev returns 0 without touching the fd), validate only when
    // there's something to write.
    let buffers_for_check = array_ptr_from_value(buffers_value);
    let buffers_nonempty = !buffers_for_check.is_null()
        && unsafe { crate::array::js_array_length(buffers_for_check) } > 0;
    if buffers_nonempty {
        // #2013: upgrade from type-only validation to type + EBADF so
        // `fs.writevSync(123, [buf])` matches Node's
        // `EBADF: bad file descriptor, writev` instead of silently
        // returning 0.
        crate::fs::validate::validate_fd_open(fd_value, "writev");
    }
    writev_sync_inner(fd_value as i32, buffers_value, position_value)
}

pub(crate) fn writev_sync_inner(fd: i32, buffers_value: f64, position_value: f64) -> f64 {
    let position = if position_value.is_finite() && position_value >= 0.0 {
        Some(position_value as u64)
    } else {
        None
    };
    let buffers = array_ptr_from_value(buffers_value);
    if buffers.is_null() {
        return 0.0;
    }
    FD_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let Some(file) = reg.get_mut(&fd) else {
            return 0.0;
        };
        let restore_pos = position.and_then(|_| file.stream_position().ok());
        if let Some(pos) = position {
            let _ = file.seek(SeekFrom::Start(pos));
        }
        let mut total = 0usize;
        unsafe {
            let len = crate::array::js_array_length(buffers);
            for i in 0..len {
                let value = crate::array::js_array_get_f64(buffers, i);
                let buf = buffer_ptr_from_value(value);
                if buf.is_null() {
                    continue;
                }
                let cap = (*buf).length as usize;
                if cap == 0 {
                    continue;
                }
                let data = crate::buffer::buffer_data(buf);
                // Node guarantees each iovec is fully written before the
                // next; use `write_all` semantics to match.
                let slice = std::slice::from_raw_parts(data, cap);
                if file.write_all(slice).is_err() {
                    break;
                }
                total += cap;
            }
        }
        if let Some(pos) = restore_pos {
            let _ = file.seek(SeekFrom::Start(pos));
        }
        total as f64
    })
}

#[derive(Default, Clone, Copy)]
struct StatFsFields {
    fs_type: u64,
    bsize: u64,
    frsize: u64,
    blocks: u64,
    bfree: u64,
    bavail: u64,
    files: u64,
    ffree: u64,
}

unsafe fn build_statfs_object(fields: StatFsFields, bigint: bool) -> f64 {
    let obj = crate::object::js_object_alloc(0, 8);
    let set = |name: &str, v: f64| {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, key, v);
    };
    for (name, value) in [
        ("type", fields.fs_type),
        ("bsize", fields.bsize),
        ("frsize", fields.frsize),
        ("blocks", fields.blocks),
        ("bfree", fields.bfree),
        ("bavail", fields.bavail),
        ("files", fields.files),
        ("ffree", fields.ffree),
    ] {
        if bigint {
            set(name, bigint_u64_value(value));
        } else {
            set(name, value as f64);
        }
    }
    f64::from_bits(crate::value::JSValue::pointer(obj as *const u8).bits())
}

#[cfg(any(
    target_os = "android",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "ios",
    target_os = "linux",
    target_os = "macos",
    target_os = "tvos",
    target_os = "watchos"
))]
unsafe fn statfs_type(c_path: *const libc::c_char) -> u64 {
    let mut stat: libc::statfs = std::mem::zeroed();
    if libc::statfs(c_path, &mut stat) == 0 {
        stat.f_type as u64
    } else {
        0
    }
}

#[cfg(not(any(
    target_os = "android",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "ios",
    target_os = "linux",
    target_os = "macos",
    target_os = "tvos",
    target_os = "watchos"
)))]
unsafe fn statfs_type(_c_path: *const libc::c_char) -> u64 {
    0
}

/// `fs.statfsSync(path)` — stable StatFs subset used by Node/Bun tests.
#[no_mangle]
pub extern "C" fn js_fs_statfs_sync(path_value: f64) -> f64 {
    js_fs_statfs_sync_options(path_value, f64::from_bits(crate::value::TAG_UNDEFINED))
}

#[no_mangle]
pub extern "C" fn js_fs_statfs_sync_options(path_value: f64, options_value: f64) -> f64 {
    // #2921 — Node validates `path` and surfaces filesystem errors instead of
    // returning a zero-filled StatFs object. A non path-like argument throws
    // `TypeError [ERR_INVALID_ARG_TYPE]`; a missing/unreadable path throws the
    // OS error (`ENOENT`, …) with syscall `statfs`.
    crate::fs::validate::validate_path("path", path_value);
    let bigint = unsafe { options_bool_field(options_value, b"bigint") };
    unsafe {
        let path = match decode_path_value(path_value) {
            Some(s) => s,
            None => {
                // The type was accepted by `validate_path` but could not be
                // decoded — treat as a missing target rather than fake stats.
                let err = std::io::Error::from(std::io::ErrorKind::NotFound);
                crate::exception::js_throw(build_fs_error_value(&err, "statfs", ""))
            }
        };
        #[cfg(unix)]
        {
            let c_path = match std::ffi::CString::new(path.as_bytes()) {
                Ok(s) => s,
                Err(_) => {
                    let err = std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "path contained an interior null byte",
                    );
                    crate::exception::js_throw(build_fs_error_value(&err, "statfs", &path))
                }
            };
            let mut stat: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(c_path.as_ptr(), &mut stat) == 0 {
                return build_statfs_object(
                    StatFsFields {
                        fs_type: statfs_type(c_path.as_ptr()),
                        bsize: stat.f_bsize as u64,
                        frsize: stat.f_frsize as u64,
                        blocks: stat.f_blocks as u64,
                        bfree: stat.f_bfree as u64,
                        bavail: stat.f_bavail as u64,
                        files: stat.f_files as u64,
                        ffree: stat.f_ffree as u64,
                    },
                    bigint,
                );
            }
            // statvfs failed — surface the OS error (ENOENT, EACCES, …) the way
            // Node does instead of swallowing it into default stats.
            let err = std::io::Error::last_os_error();
            crate::exception::js_throw(build_fs_error_value(&err, "statfs", &path))
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            build_statfs_object(StatFsFields::default(), bigint)
        }
    }
}

pub(crate) fn alloc_dir_state(entries: Vec<f64>) -> usize {
    let id = NEXT_DIR_ID.with(|n| {
        let mut n = n.borrow_mut();
        let id = *n;
        *n += 1;
        id
    });
    DIR_REGISTRY.with(|r| {
        r.borrow_mut().insert(
            id,
            DirState {
                entries,
                index: 0,
                closed: false,
                operation_pending: false,
            },
        );
    });
    id
}

pub(crate) fn scan_fs_dir_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    DIR_REGISTRY.with(|r| {
        for state in r.borrow_mut().values_mut() {
            for entry in &mut state.entries {
                visitor.visit_nanbox_f64_slot(entry);
            }
        }
    });
}

pub(crate) fn dir_id_of(closure: *const ClosureHeader) -> usize {
    crate::closure::js_closure_get_capture_ptr(closure, 0) as usize
}

fn dir_closed_error_value() -> f64 {
    let message = "Directory handle was closed";
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, "ERR_DIR_CLOSED");
    let err = crate::error::js_error_new_with_message(msg);
    crate::value::js_nanbox_pointer(err as i64)
}

fn dir_concurrent_operation_error_value() -> f64 {
    let message =
        "Cannot do synchronous work on directory handle with concurrent asynchronous operations";
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, "ERR_DIR_CONCURRENT_OPERATION");
    let err = crate::error::js_error_new_with_message(msg);
    crate::value::js_nanbox_pointer(err as i64)
}

pub(crate) fn dir_mark_closed(id: usize) {
    DIR_REGISTRY.with(|r| {
        if let Some(state) = r.borrow_mut().get_mut(&id) {
            state.closed = true;
            state.operation_pending = false;
            state.entries.clear();
        }
    });
}

pub(crate) fn dir_close_result(id: usize) -> Result<(), f64> {
    DIR_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let Some(state) = reg.get_mut(&id) else {
            return Err(dir_closed_error_value());
        };
        if state.closed {
            return Err(dir_closed_error_value());
        }
        state.closed = true;
        state.entries.clear();
        Ok(())
    })
}

fn dir_close_sync_result(id: usize) -> Result<(), f64> {
    DIR_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let Some(state) = reg.get_mut(&id) else {
            return Err(dir_closed_error_value());
        };
        if state.closed {
            return Err(dir_closed_error_value());
        }
        if state.operation_pending {
            return Err(dir_concurrent_operation_error_value());
        }
        state.closed = true;
        state.entries.clear();
        Ok(())
    })
}

pub(crate) fn dir_read_next_result(id: usize) -> Result<Option<f64>, f64> {
    DIR_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let Some(state) = reg.get_mut(&id) else {
            return Err(dir_closed_error_value());
        };
        if state.closed {
            return Err(dir_closed_error_value());
        }
        if state.index >= state.entries.len() {
            return Ok(None);
        }
        let value = state.entries[state.index];
        state.index += 1;
        Ok(Some(value))
    })
}

fn dir_read_next_sync_result(id: usize) -> Result<Option<f64>, f64> {
    DIR_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let Some(state) = reg.get_mut(&id) else {
            return Err(dir_closed_error_value());
        };
        if state.closed {
            return Err(dir_closed_error_value());
        }
        if state.operation_pending {
            return Err(dir_concurrent_operation_error_value());
        }
        if state.index >= state.entries.len() {
            return Ok(None);
        }
        let value = state.entries[state.index];
        state.index += 1;
        Ok(Some(value))
    })
}

pub(crate) fn dir_read_next(id: usize) -> f64 {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    match dir_read_next_sync_result(id) {
        Ok(Some(value)) => value,
        Ok(None) => f64::from_bits(TAG_NULL),
        Err(err) => crate::exception::js_throw(err),
    }
}

pub(crate) fn make_dir_method(id: usize, func: *const u8) -> f64 {
    let closure = crate::closure::js_closure_alloc(func, 1);
    crate::closure::js_closure_set_capture_ptr(closure, 0, id as i64);
    f64::from_bits(crate::value::JSValue::pointer(closure as *const u8).bits())
}

pub(crate) extern "C" fn dir_read_sync_impl(closure: *const ClosureHeader) -> f64 {
    dir_read_next(dir_id_of(closure))
}

pub(crate) extern "C" fn dir_close_sync_impl(closure: *const ClosureHeader) -> f64 {
    let id = dir_id_of(closure);
    match dir_close_sync_result(id) {
        Ok(()) => f64::from_bits(crate::value::TAG_UNDEFINED),
        Err(err) => crate::exception::js_throw(err),
    }
}

fn dir_begin_async_operation(id: usize) -> Result<(), f64> {
    DIR_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let Some(state) = reg.get_mut(&id) else {
            return Err(dir_closed_error_value());
        };
        if state.closed {
            return Err(dir_closed_error_value());
        }
        state.operation_pending = true;
        Ok(())
    })
}

fn dir_clear_operation_pending(id: usize) {
    DIR_REGISTRY.with(|r| {
        if let Some(state) = r.borrow_mut().get_mut(&id) {
            state.operation_pending = false;
        }
    });
}

extern "C" fn dir_finish_read_promise_impl(closure: *const ClosureHeader) -> f64 {
    let id = dir_id_of(closure);
    let promise =
        crate::closure::js_closure_get_capture_ptr(closure, 1) as *mut crate::promise::Promise;
    let value = crate::closure::js_closure_get_capture_f64(closure, 2);
    let is_error = crate::closure::js_closure_get_capture_f64(closure, 3) != 0.0;
    dir_clear_operation_pending(id);
    if is_error {
        crate::promise::js_promise_reject(promise, value);
    } else {
        crate::promise::js_promise_resolve(promise, value);
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn dir_pending_read_promise(id: usize, result: Result<Option<f64>, f64>) -> f64 {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let promise = crate::promise::js_promise_new();
    let (value, is_error) = match result {
        Ok(Some(value)) => (value, false),
        Ok(None) => (f64::from_bits(TAG_NULL), false),
        Err(err) => (err, true),
    };
    let closure = crate::closure::js_closure_alloc(dir_finish_read_promise_impl as *const u8, 4);
    crate::closure::js_closure_set_capture_ptr(closure, 0, id as i64);
    crate::closure::js_closure_set_capture_ptr(closure, 1, promise as i64);
    crate::closure::js_closure_set_capture_f64(closure, 2, value);
    crate::closure::js_closure_set_capture_f64(closure, 3, if is_error { 1.0 } else { 0.0 });
    crate::builtins::js_queue_microtask(closure as i64);
    f64::from_bits(crate::value::JSValue::pointer(promise as *const u8).bits())
}

extern "C" fn dir_finish_read_callback_impl(closure: *const ClosureHeader) -> f64 {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let id = dir_id_of(closure);
    let callback = crate::closure::js_closure_get_capture_ptr(closure, 1) as *const ClosureHeader;
    let value = crate::closure::js_closure_get_capture_f64(closure, 2);
    let kind = crate::closure::js_closure_get_capture_f64(closure, 3) as i32;
    match kind {
        0 => {
            crate::closure::js_closure_call2(callback, f64::from_bits(TAG_NULL), value);
        }
        1 => {
            crate::closure::js_closure_call2(
                callback,
                f64::from_bits(TAG_NULL),
                f64::from_bits(TAG_NULL),
            );
        }
        _ => {
            crate::closure::js_closure_call2(callback, value, f64::from_bits(TAG_NULL));
        }
    }
    dir_clear_operation_pending(id);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn dir_schedule_read_callback(
    id: usize,
    callback: *const ClosureHeader,
    result: Result<Option<f64>, f64>,
) {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let (value, kind) = match result {
        Ok(Some(value)) => (value, 0.0),
        Ok(None) => (f64::from_bits(TAG_NULL), 1.0),
        Err(err) => (err, 2.0),
    };
    let closure = crate::closure::js_closure_alloc(dir_finish_read_callback_impl as *const u8, 4);
    crate::closure::js_closure_set_capture_ptr(closure, 0, id as i64);
    crate::closure::js_closure_set_capture_ptr(closure, 1, callback as i64);
    crate::closure::js_closure_set_capture_f64(closure, 2, value);
    crate::closure::js_closure_set_capture_f64(closure, 3, kind);
    crate::builtins::js_queue_microtask(closure as i64);
}

pub(crate) extern "C" fn dir_read_impl(closure: *const ClosureHeader, callback: f64) -> f64 {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let id = dir_id_of(closure);
    let begin = dir_begin_async_operation(id);
    let pending = begin.is_ok();
    let entry = match begin {
        Ok(()) => dir_read_next_result(id),
        Err(err) => Err(err),
    };
    let cb = extract_closure_ptr(callback);
    if !cb.is_null() {
        if pending {
            dir_schedule_read_callback(id, cb, entry);
        } else {
            match entry {
                Ok(Some(value)) => {
                    crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), value);
                }
                Ok(None) => {
                    crate::closure::js_closure_call2(
                        cb,
                        f64::from_bits(TAG_NULL),
                        f64::from_bits(TAG_NULL),
                    );
                }
                Err(err) => {
                    crate::closure::js_closure_call2(cb, err, f64::from_bits(TAG_NULL));
                }
            }
        }
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if pending {
        return dir_pending_read_promise(id, entry);
    }
    match entry {
        Ok(Some(value)) => promise_value_fs(value),
        Ok(None) => promise_value_fs(f64::from_bits(TAG_NULL)),
        Err(err) => promise_rejected_fs(err),
    }
}

pub(crate) extern "C" fn dir_close_impl(closure: *const ClosureHeader, callback: f64) -> f64 {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let closed = dir_close_result(dir_id_of(closure));
    let cb = extract_closure_ptr(callback);
    if !cb.is_null() {
        let err = match closed {
            Ok(()) => f64::from_bits(TAG_NULL),
            Err(err) => err,
        };
        crate::closure::js_closure_call1(cb, err);
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    match closed {
        Ok(()) => promise_undefined_fs(),
        Err(err) => promise_rejected_fs(err),
    }
}

fn dir_iterator_method(id: usize, self_value: f64, func: *const u8) -> f64 {
    let closure = crate::closure::js_closure_alloc(func, 2);
    crate::closure::js_closure_set_capture_ptr(closure, 0, id as i64);
    crate::closure::js_closure_set_capture_f64(closure, 1, self_value);
    f64::from_bits(crate::value::JSValue::pointer(closure as *const u8).bits())
}

fn dir_iterator_id_of(closure: *const ClosureHeader) -> usize {
    crate::closure::js_closure_get_capture_ptr(closure, 0) as usize
}

fn dir_iterator_self_value(closure: *const ClosureHeader) -> f64 {
    crate::closure::js_closure_get_capture_f64(closure, 1)
}

fn dir_iterator_result(value: f64, done: bool) -> f64 {
    use crate::value::JSValue;

    let obj = crate::object::js_object_alloc(0, 2);
    let value_key = js_string_from_bytes(b"value".as_ptr(), b"value".len() as u32);
    let done_key = js_string_from_bytes(b"done".as_ptr(), b"done".len() as u32);
    let keys = crate::array::js_array_alloc(2);
    crate::array::js_array_push(keys, JSValue::string_ptr(value_key));
    crate::array::js_array_push(keys, JSValue::string_ptr(done_key));
    crate::object::js_object_set_keys(obj, keys);

    crate::object::js_object_set_field(obj, 0, JSValue::from_bits(value.to_bits()));
    crate::object::js_object_set_field(
        obj,
        1,
        JSValue::from_bits(crate::value::JSValue::bool(done).bits()),
    );
    f64::from_bits(crate::value::JSValue::pointer(obj as *const u8).bits())
}

extern "C" fn dir_iterator_next_impl(closure: *const ClosureHeader) -> f64 {
    match dir_read_next_result(dir_iterator_id_of(closure)) {
        Ok(Some(value)) => promise_value_fs(dir_iterator_result(value, false)),
        Ok(None) => {
            dir_mark_closed(dir_iterator_id_of(closure));
            promise_value_fs(dir_iterator_result(
                f64::from_bits(crate::value::TAG_UNDEFINED),
                true,
            ))
        }
        Err(err) => promise_rejected_fs(err),
    }
}

extern "C" fn dir_iterator_return_impl(closure: *const ClosureHeader) -> f64 {
    dir_mark_closed(dir_iterator_id_of(closure));
    promise_value_fs(dir_iterator_result(
        f64::from_bits(crate::value::TAG_UNDEFINED),
        true,
    ))
}

extern "C" fn dir_iterator_self_impl(closure: *const ClosureHeader) -> f64 {
    dir_iterator_self_value(closure)
}

extern "C" fn dir_entries_impl(closure: *const ClosureHeader) -> f64 {
    unsafe { build_dir_iterator_object(dir_id_of(closure)) }
}

extern "C" fn dir_dispose_impl(closure: *const ClosureHeader) -> f64 {
    dir_mark_closed(dir_id_of(closure));
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn dir_async_dispose_impl(closure: *const ClosureHeader) -> f64 {
    dir_mark_closed(dir_id_of(closure));
    promise_undefined_fs()
}

extern "C" fn dir_path_getter_impl(closure: *const ClosureHeader) -> f64 {
    crate::closure::js_closure_get_capture_f64(closure, 0)
}

unsafe fn install_dir_symbol(target: f64, short_name: &str, method: f64) {
    let symbol = crate::symbol::well_known_symbol(short_name);
    if symbol.is_null() {
        return;
    }
    let symbol_value = f64::from_bits(crate::value::JSValue::pointer(symbol as *const u8).bits());
    crate::symbol::js_object_set_symbol_property(target, symbol_value, method);
}

unsafe fn build_dir_iterator_object(id: usize) -> f64 {
    let obj = crate::object::js_object_alloc(0, 2);
    let self_value = f64::from_bits(crate::value::JSValue::pointer(obj as *const u8).bits());
    let set = |name: &str, v: f64| {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, key, v);
    };
    set(
        "next",
        dir_iterator_method(id, self_value, dir_iterator_next_impl as *const u8),
    );
    set(
        "return",
        dir_iterator_method(id, self_value, dir_iterator_return_impl as *const u8),
    );
    install_dir_symbol(
        self_value,
        "asyncIterator",
        dir_iterator_method(id, self_value, dir_iterator_self_impl as *const u8),
    );
    self_value
}

unsafe fn install_dir_proto_method(
    proto: *mut crate::object::ObjectHeader,
    name: &str,
    value: f64,
) {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_set_field_by_name(proto, key, value);
    crate::object::set_property_attrs(
        proto as usize,
        name.to_string(),
        crate::object::PropertyAttrs::new(true, false, true),
    );
}

unsafe fn install_dir_proto_path(proto: *mut crate::object::ObjectHeader, path: &str) {
    crate::closure::js_register_closure_arity(dir_path_getter_impl as *const u8, 0);
    let path_ptr = js_string_from_bytes(path.as_ptr(), path.len() as u32);
    let path_value = crate::value::js_nanbox_string(path_ptr as i64);
    let getter = crate::closure::js_closure_alloc(dir_path_getter_impl as *const u8, 1);
    crate::closure::js_closure_set_capture_f64(getter, 0, path_value);
    let getter_value = crate::value::js_nanbox_pointer(getter as i64);

    let key = crate::string::js_string_from_bytes(b"path".as_ptr(), b"path".len() as u32);
    crate::object::js_object_set_field_by_name(proto, key, getter_value);
    crate::object::set_accessor_descriptor(
        proto as usize,
        "path".to_string(),
        crate::object::AccessorDescriptor {
            get: getter_value.to_bits(),
            set: 0,
        },
    );
    crate::object::set_property_attrs(
        proto as usize,
        "path".to_string(),
        crate::object::PropertyAttrs::new(true, false, true),
    );
}

pub(crate) unsafe fn build_dir_object(id: usize, path: &str) -> f64 {
    crate::closure::js_register_closure_arity(dir_read_impl as *const u8, 1);
    crate::closure::js_register_closure_arity(dir_close_impl as *const u8, 1);
    crate::closure::js_register_closure_arity(dir_entries_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(dir_dispose_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(dir_async_dispose_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(dir_finish_read_promise_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(dir_finish_read_callback_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(dir_read_sync_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(dir_close_sync_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(dir_iterator_next_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(dir_iterator_return_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(dir_iterator_self_impl as *const u8, 0);

    let obj = crate::object::js_object_alloc(CLASS_ID_FS_DIR, 0);
    let self_value = f64::from_bits(crate::value::JSValue::pointer(obj as *const u8).bits());
    let proto = crate::object::js_object_alloc(0, 7);
    let proto_value = f64::from_bits(crate::value::JSValue::pointer(proto as *const u8).bits());
    crate::object::prototype_chain::object_set_static_prototype(
        obj as usize,
        proto_value.to_bits(),
    );

    install_dir_proto_method(
        proto,
        "constructor",
        crate::object::bound_native_callable_export_value("fs", "Dir"),
    );
    install_dir_proto_path(proto, path);
    install_dir_proto_method(
        proto,
        "readSync",
        make_dir_method(id, dir_read_sync_impl as *const u8),
    );
    install_dir_proto_method(
        proto,
        "closeSync",
        make_dir_method(id, dir_close_sync_impl as *const u8),
    );
    install_dir_proto_method(
        proto,
        "read",
        make_dir_method(id, dir_read_impl as *const u8),
    );
    install_dir_proto_method(
        proto,
        "close",
        make_dir_method(id, dir_close_impl as *const u8),
    );

    let entries = make_dir_method(id, dir_entries_impl as *const u8);
    install_dir_proto_method(proto, "entries", entries);
    install_dir_symbol(proto_value, "asyncIterator", entries);
    install_dir_symbol(
        proto_value,
        "dispose",
        make_dir_method(id, dir_dispose_impl as *const u8),
    );
    install_dir_symbol(
        proto_value,
        "asyncDispose",
        make_dir_method(id, dir_async_dispose_impl as *const u8),
    );
    self_value
}
