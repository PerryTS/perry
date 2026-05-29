//! `fs/promises.FileHandle` — per-method closures + object construction.

use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};

use crate::closure::ClosureHeader;

use super::*;

pub(crate) unsafe fn build_file_io_result(
    count_name: &str,
    count: f64,
    value_name: &str,
    value: f64,
) -> f64 {
    let obj = crate::object::js_object_alloc(0, 2);
    let set = |name: &str, v: f64| {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, key, v);
    };
    set(count_name, count);
    set(value_name, value);
    f64::from_bits(crate::value::JSValue::pointer(obj as *const u8).bits())
}

pub(crate) fn make_filehandle_method(fd: i32, func: *const u8) -> f64 {
    let closure = crate::closure::js_closure_alloc(func, 1);
    crate::closure::js_closure_set_capture_ptr(closure, 0, fd as i64);
    f64::from_bits(crate::value::JSValue::pointer(closure as *const u8).bits())
}

pub(crate) fn filehandle_fd(closure: *const ClosureHeader) -> i32 {
    crate::closure::js_closure_get_capture_ptr(closure, 0) as i32
}

pub(crate) extern "C" fn filehandle_close_impl(closure: *const ClosureHeader) -> f64 {
    let _ = js_fs_close_sync(filehandle_fd(closure) as f64);
    promise_undefined_fs()
}

pub(crate) extern "C" fn filehandle_sync_impl(closure: *const ClosureHeader) -> f64 {
    // Bypass `js_fs_fsync_sync`'s arg-validation: FileHandle may legitimately
    // hold a `-1` fd sentinel from a failed open, and Node's API surfaces that
    // earlier (at `open`), not here.
    let _ = crate::fs::fsync_sync_inner(filehandle_fd(closure));
    promise_undefined_fs()
}

pub(crate) extern "C" fn filehandle_datasync_impl(closure: *const ClosureHeader) -> f64 {
    let _ = crate::fs::fdatasync_sync_inner(filehandle_fd(closure));
    promise_undefined_fs()
}

pub(crate) extern "C" fn filehandle_stat_impl(closure: *const ClosureHeader, options: f64) -> f64 {
    promise_value_fs(js_fs_fstat_sync_options(
        filehandle_fd(closure) as f64,
        options,
    ))
}

pub(crate) extern "C" fn filehandle_truncate_impl(closure: *const ClosureHeader, len: f64) -> f64 {
    let _ = js_fs_ftruncate_sync(filehandle_fd(closure) as f64, len);
    promise_undefined_fs()
}

pub(crate) extern "C" fn filehandle_utimes_impl(
    closure: *const ClosureHeader,
    atime: f64,
    mtime: f64,
) -> f64 {
    let _ = js_fs_futimes_sync(filehandle_fd(closure) as f64, atime, mtime);
    promise_undefined_fs()
}

pub(crate) extern "C" fn filehandle_chmod_impl(closure: *const ClosureHeader, mode: f64) -> f64 {
    let _ = js_fs_fchmod_sync(filehandle_fd(closure) as f64, mode);
    promise_undefined_fs()
}

pub(crate) extern "C" fn filehandle_chown_impl(
    closure: *const ClosureHeader,
    uid: f64,
    gid: f64,
) -> f64 {
    let _ = crate::fs::fchown_sync_inner(filehandle_fd(closure), uid, gid);
    promise_undefined_fs()
}

pub(crate) extern "C" fn filehandle_read_file_impl(
    closure: *const ClosureHeader,
    encoding: f64,
) -> f64 {
    let fd = filehandle_fd(closure);
    FD_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let Some(file) = reg.get_mut(&fd) else {
            return promise_value_fs(f64::from_bits(crate::value::TAG_UNDEFINED));
        };
        let mut bytes = Vec::new();
        let _ = file.read_to_end(&mut bytes);
        if read_file_encoding(encoding).is_none() {
            let buf = crate::buffer::js_buffer_alloc(bytes.len() as i32, 0);
            if !buf.is_null() {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        bytes.as_ptr(),
                        crate::buffer::buffer_data_mut(buf),
                        bytes.len(),
                    );
                    (*buf).length = bytes.len() as u32;
                }
            }
            promise_value_fs(f64::from_bits(
                crate::value::JSValue::pointer(buf as *const u8).bits(),
            ))
        } else {
            let s = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            promise_value_fs(f64::from_bits(crate::value::JSValue::string_ptr(s).bits()))
        }
    })
}

pub(crate) extern "C" fn filehandle_write_file_impl(
    closure: *const ClosureHeader,
    data: f64,
) -> f64 {
    let fd = filehandle_fd(closure);
    let bytes = bytes_from_value(data);
    FD_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        if let Some(file) = reg.get_mut(&fd) {
            let append =
                FD_APPEND_MODE.with(|flags| flags.borrow().get(&fd).copied().unwrap_or(false));
            if append {
                let _ = file.seek(SeekFrom::End(0));
            }
            // Note: Node does NOT rewind/truncate on FileHandle#writeFile —
            // empirically the file pointer advances naturally so successive
            // writeFile calls concatenate (see parity test
            // `fs-promises/basic/write-append-flush-options`). When the
            // caller wants replace-semantics they should reopen the handle.
            let _ = file.write_all(&bytes);
        }
    });
    promise_undefined_fs()
}

pub(crate) extern "C" fn filehandle_append_file_impl(
    closure: *const ClosureHeader,
    data: f64,
) -> f64 {
    let fd = filehandle_fd(closure);
    let bytes = bytes_from_value(data);
    FD_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        if let Some(file) = reg.get_mut(&fd) {
            let _ = file.seek(SeekFrom::End(0));
            let _ = file.write_all(&bytes);
        }
    });
    promise_undefined_fs()
}

pub(crate) extern "C" fn filehandle_read_impl(
    closure: *const ClosureHeader,
    buffer: f64,
    offset: f64,
    length: f64,
    position: f64,
) -> f64 {
    let fd = filehandle_fd(closure);
    let (actual_buffer, actual_offset, actual_length, actual_position) =
        if crate::buffer::js_buffer_is_buffer(buffer.to_bits() as i64) == 1 {
            let buffer_len = buffer_len_from_value(buffer) as f64;
            let actual_offset = if offset.is_finite() { offset } else { 0.0 };
            let actual_length = if length.is_finite() {
                length
            } else {
                (buffer_len - actual_offset).max(0.0)
            };
            (buffer, actual_offset, actual_length, position)
        } else {
            unsafe {
                let actual_buffer = options_field_value(buffer, b"buffer")
                    .map(|v| f64::from_bits(v.bits()))
                    .unwrap_or_else(|| {
                        let buf = crate::buffer::js_buffer_alloc(16 * 1024, 0);
                        f64::from_bits(crate::value::JSValue::pointer(buf as *const u8).bits())
                    });
                let buffer_len = buffer_len_from_value(actual_buffer) as f64;
                let actual_offset = options_number_field(buffer, b"offset").unwrap_or(0.0);
                let actual_length = options_number_field(buffer, b"length")
                    .unwrap_or_else(|| (buffer_len - actual_offset).max(0.0));
                let actual_position = options_number_field(buffer, b"position")
                    .unwrap_or(f64::from_bits(crate::value::TAG_NULL));
                (actual_buffer, actual_offset, actual_length, actual_position)
            }
        };
    let bytes_read = js_fs_read_sync(
        fd as f64,
        actual_buffer,
        actual_offset,
        actual_length,
        actual_position,
    );
    unsafe {
        promise_value_fs(build_file_io_result(
            "bytesRead",
            bytes_read,
            "buffer",
            actual_buffer,
        ))
    }
}

pub(crate) extern "C" fn filehandle_write_impl(
    closure: *const ClosureHeader,
    data: f64,
    offset: f64,
    length: f64,
    position: f64,
) -> f64 {
    let fd = filehandle_fd(closure);
    let bytes_written = if crate::buffer::js_buffer_is_buffer(data.to_bits() as i64) == 1 {
        let buffer_len = buffer_len_from_value(data) as f64;
        let actual_offset = if offset.is_finite() { offset } else { 0.0 };
        let actual_length = if length.is_finite() {
            length
        } else {
            (buffer_len - actual_offset).max(0.0)
        };
        crate::fs::write_buffer_sync_inner(fd, data, actual_offset, actual_length, position)
    } else {
        crate::fs::write_string_sync_inner(fd, data, offset)
    };
    unsafe {
        promise_value_fs(build_file_io_result(
            "bytesWritten",
            bytes_written,
            "buffer",
            data,
        ))
    }
}

pub(crate) extern "C" fn filehandle_readv_impl(
    closure: *const ClosureHeader,
    buffers: f64,
    position: f64,
) -> f64 {
    let fd = filehandle_fd(closure);
    let bytes_read = js_fs_readv_sync(fd as f64, buffers, position);
    unsafe {
        promise_value_fs(build_file_io_result(
            "bytesRead",
            bytes_read,
            "buffers",
            buffers,
        ))
    }
}

pub(crate) extern "C" fn filehandle_writev_impl(
    closure: *const ClosureHeader,
    buffers: f64,
    position: f64,
) -> f64 {
    let fd = filehandle_fd(closure);
    let bytes_written = crate::fs::writev_sync_inner(fd, buffers, position);
    unsafe {
        promise_value_fs(build_file_io_result(
            "bytesWritten",
            bytes_written,
            "buffers",
            buffers,
        ))
    }
}

pub(crate) fn path_for_fd(fd: i32) -> Option<String> {
    FD_PATHS.with(|paths| paths.borrow().get(&fd).cloned())
}

pub(crate) extern "C" fn filehandle_create_read_stream_impl(
    closure: *const ClosureHeader,
    options: f64,
) -> f64 {
    let fd = filehandle_fd(closure);
    if let Some(path) = path_for_fd(fd) {
        let s = js_string_from_bytes(path.as_ptr(), path.len() as u32);
        js_fs_create_read_stream(crate::value::js_nanbox_string(s as i64), options)
    } else {
        let s = js_string_from_bytes(b"".as_ptr(), 0);
        js_fs_create_read_stream(crate::value::js_nanbox_string(s as i64), options)
    }
}

pub(crate) extern "C" fn filehandle_create_write_stream_impl(
    closure: *const ClosureHeader,
    options: f64,
) -> f64 {
    let fd = filehandle_fd(closure);
    if let Some(path) = path_for_fd(fd) {
        let s = js_string_from_bytes(path.as_ptr(), path.len() as u32);
        js_fs_create_write_stream(crate::value::js_nanbox_string(s as i64), options)
    } else {
        let s = js_string_from_bytes(b"".as_ptr(), 0);
        js_fs_create_write_stream(crate::value::js_nanbox_string(s as i64), options)
    }
}

/// Build a minimal `fs.promises.FileHandle` object for deterministic parity.
#[no_mangle]
pub extern "C" fn js_fs_filehandle_open(path_value: f64, flags_value: f64) -> f64 {
    let fd = js_fs_open_sync(path_value, flags_value) as i32;
    unsafe {
        crate::closure::js_register_closure_arity(filehandle_stat_impl as *const u8, 1);
        crate::closure::js_register_closure_arity(filehandle_read_impl as *const u8, 5);
        crate::closure::js_register_closure_arity(filehandle_write_impl as *const u8, 5);
        let obj = crate::object::js_object_alloc(0, 18);
        let set = |name: &str, v: f64| {
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            crate::object::js_object_set_field_by_name(obj, key, v);
        };
        set("fd", fd as f64);
        set(
            "close",
            make_filehandle_method(fd, filehandle_close_impl as *const u8),
        );
        set(
            "sync",
            make_filehandle_method(fd, filehandle_sync_impl as *const u8),
        );
        set(
            "datasync",
            make_filehandle_method(fd, filehandle_datasync_impl as *const u8),
        );
        set(
            "stat",
            make_filehandle_method(fd, filehandle_stat_impl as *const u8),
        );
        set(
            "truncate",
            make_filehandle_method(fd, filehandle_truncate_impl as *const u8),
        );
        set(
            "utimes",
            make_filehandle_method(fd, filehandle_utimes_impl as *const u8),
        );
        set(
            "chmod",
            make_filehandle_method(fd, filehandle_chmod_impl as *const u8),
        );
        set(
            "chown",
            make_filehandle_method(fd, filehandle_chown_impl as *const u8),
        );
        set(
            "readFile",
            make_filehandle_method(fd, filehandle_read_file_impl as *const u8),
        );
        set(
            "writeFile",
            make_filehandle_method(fd, filehandle_write_file_impl as *const u8),
        );
        set(
            "appendFile",
            make_filehandle_method(fd, filehandle_append_file_impl as *const u8),
        );
        set(
            "read",
            make_filehandle_method(fd, filehandle_read_impl as *const u8),
        );
        set(
            "write",
            make_filehandle_method(fd, filehandle_write_impl as *const u8),
        );
        set(
            "readv",
            make_filehandle_method(fd, filehandle_readv_impl as *const u8),
        );
        set(
            "writev",
            make_filehandle_method(fd, filehandle_writev_impl as *const u8),
        );
        set(
            "createReadStream",
            make_filehandle_method(fd, filehandle_create_read_stream_impl as *const u8),
        );
        set(
            "createWriteStream",
            make_filehandle_method(fd, filehandle_create_write_stream_impl as *const u8),
        );
        FILEHANDLE_OBJECT_FDS.with(|fds| {
            fds.borrow_mut().insert(obj as usize, fd);
        });
        f64::from_bits(crate::value::JSValue::pointer(obj as *const u8).bits())
    }
}
