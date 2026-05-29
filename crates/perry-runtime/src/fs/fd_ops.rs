//! POSIX fd ops: open/close/read/write/readv/writev, mkdtemp, realpath,
//! readlink, link/symlink, rename, truncate, fchmod/fchown/fstat/fsync/
//! fdatasync/ftruncate/futimes/utimes/lutimes/rmdir + statfs builder.

use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

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

/// `fs.openSync(path, flags)` — small fd registry for deterministic tests.
#[no_mangle]
pub extern "C" fn js_fs_open_sync(path_value: f64, flags_value: f64) -> f64 {
    crate::fs::validate::validate_path("path", path_value);
    unsafe {
        let path_str = match decode_path_value(path_value) {
            Some(s) => s,
            None => return -1.0,
        };
        let mut opts = fs::OpenOptions::new();
        let append_mode;
        if flags_value.is_finite() {
            let flags = flags_value as i32;
            append_mode = flags & 0x8 != 0;
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
        match opts.open(&path_str) {
            Ok(file) => {
                let fd = NEXT_FD.with(|n| {
                    let mut n = n.borrow_mut();
                    let fd = *n;
                    *n += 1;
                    fd
                });
                FD_REGISTRY.with(|r| {
                    r.borrow_mut().insert(fd, file);
                });
                FD_PATHS.with(|r| {
                    r.borrow_mut().insert(fd, path_str.to_string());
                });
                FD_APPEND_MODE.with(|r| {
                    r.borrow_mut().insert(fd, append_mode);
                });
                fd as f64
            }
            Err(_) => -1.0,
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
            let data = crate::buffer::buffer_data_mut(buf).add(offset);
            let result = match file.read(std::slice::from_raw_parts_mut(data, n)) {
                Ok(read) => read as f64,
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
    let bigint = unsafe { options_bool_field(options_value, b"bigint") };
    unsafe {
        let path = match decode_path_value(path_value) {
            Some(s) => s,
            None => return build_statfs_object(StatFsFields::default(), bigint),
        };
        #[cfg(unix)]
        {
            let c_path = match std::ffi::CString::new(path) {
                Ok(s) => s,
                Err(_) => return build_statfs_object(StatFsFields::default(), bigint),
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
        }
        #[cfg(not(unix))]
        {
            let _ = path;
        }
        build_statfs_object(StatFsFields::default(), bigint)
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
            },
        );
    });
    id
}

pub(crate) fn dir_id_of(closure: *const ClosureHeader) -> usize {
    crate::closure::js_closure_get_capture_ptr(closure, 0) as usize
}

pub(crate) fn dir_read_next(id: usize) -> f64 {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    DIR_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let Some(state) = reg.get_mut(&id) else {
            return f64::from_bits(TAG_NULL);
        };
        if state.closed || state.index >= state.entries.len() {
            return f64::from_bits(TAG_NULL);
        }
        let value = state.entries[state.index];
        state.index += 1;
        value
    })
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
    DIR_REGISTRY.with(|r| {
        if let Some(state) = r.borrow_mut().get_mut(&id) {
            state.closed = true;
        }
    });
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(crate) extern "C" fn dir_read_impl(closure: *const ClosureHeader, callback: f64) -> f64 {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let entry = dir_read_next(dir_id_of(closure));
    let cb = extract_closure_ptr(callback);
    if !cb.is_null() {
        crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), entry);
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    promise_value_fs(entry)
}

pub(crate) extern "C" fn dir_close_impl(closure: *const ClosureHeader, callback: f64) -> f64 {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let _ = dir_close_sync_impl(closure);
    let cb = extract_closure_ptr(callback);
    if !cb.is_null() {
        crate::closure::js_closure_call1(cb, f64::from_bits(TAG_NULL));
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    promise_undefined_fs()
}

pub(crate) unsafe fn build_dir_object(id: usize, path: &str) -> f64 {
    crate::closure::js_register_closure_arity(dir_read_impl as *const u8, 1);
    crate::closure::js_register_closure_arity(dir_close_impl as *const u8, 1);
    let obj = crate::object::js_object_alloc(0, 6);
    let set = |name: &str, v: f64| {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, key, v);
    };
    let path_ptr = js_string_from_bytes(path.as_ptr(), path.len() as u32);
    set("path", crate::value::js_nanbox_string(path_ptr as i64));
    set(
        "readSync",
        make_dir_method(id, dir_read_sync_impl as *const u8),
    );
    set(
        "closeSync",
        make_dir_method(id, dir_close_sync_impl as *const u8),
    );
    set("read", make_dir_method(id, dir_read_impl as *const u8));
    set("close", make_dir_method(id, dir_close_impl as *const u8));
    set(
        "Symbol.asyncIterator",
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
    f64::from_bits(crate::value::JSValue::pointer(obj as *const u8).bits())
}
