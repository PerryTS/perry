//! `fs/promises.FileHandle` — per-method closures + object construction.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};

use crate::closure::ClosureHeader;

use super::*;

thread_local! {
    static READ_LINES_REGISTRY: RefCell<HashMap<usize, ReadLinesState>> =
        RefCell::new(HashMap::new());
    static NEXT_READ_LINES_ID: RefCell<usize> = const { RefCell::new(1) };
}

struct ReadLinesState {
    lines: Vec<String>,
    index: usize,
    fd: i32,
    handle: f64,
}

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

pub(crate) fn make_filehandle_method_with_handle(fd: i32, handle: f64, func: *const u8) -> f64 {
    let closure = crate::closure::js_closure_alloc(func, 2);
    crate::closure::js_closure_set_capture_ptr(closure, 0, fd as i64);
    crate::closure::js_closure_set_capture_f64(closure, 1, handle);
    f64::from_bits(crate::value::JSValue::pointer(closure as *const u8).bits())
}

pub(crate) fn filehandle_fd(closure: *const ClosureHeader) -> i32 {
    crate::closure::js_closure_get_capture_ptr(closure, 0) as i32
}

fn filehandle_object(closure: *const ClosureHeader) -> Option<f64> {
    if closure.is_null() {
        return None;
    }
    let captures = crate::closure::real_capture_count(unsafe { (*closure).capture_count });
    if captures < 2 {
        return None;
    }
    Some(crate::closure::js_closure_get_capture_f64(closure, 1))
}

fn filehandle_field_fd(handle: f64) -> Option<i32> {
    let ptr = crate::value::js_nanbox_get_pointer(handle);
    if ptr < 0x1000 {
        return None;
    }
    let key = crate::string::js_string_from_bytes(b"fd".as_ptr(), 2);
    let value =
        crate::object::js_object_get_field_by_name(ptr as *const crate::object::ObjectHeader, key);
    let js = crate::value::JSValue::from_bits(value.bits());
    if js.is_int32() {
        Some(js.as_int32())
    } else if js.is_number() {
        Some(f64::from_bits(value.bits()) as i32)
    } else {
        None
    }
}

fn set_filehandle_field_fd(handle: f64, fd: i32) {
    let ptr = crate::value::js_nanbox_get_pointer(handle);
    if ptr < 0x1000 {
        return;
    }
    let key = crate::string::js_string_from_bytes(b"fd".as_ptr(), 2);
    crate::object::js_object_set_field_by_name(
        ptr as *mut crate::object::ObjectHeader,
        key,
        fd as f64,
    );
}

fn close_filehandle_fd(fd: i32, handle: f64) {
    if fd >= 0 && crate::fs::fd_is_registered(fd) {
        let _ = js_fs_close_sync(fd as f64);
    }
    set_filehandle_field_fd(handle, -1);
}

fn make_read_lines_method(id: usize, func: *const u8) -> f64 {
    let closure = crate::closure::js_closure_alloc(func, 1);
    crate::closure::js_closure_set_capture_ptr(closure, 0, id as i64);
    f64::from_bits(crate::value::JSValue::pointer(closure as *const u8).bits())
}

fn read_lines_id(closure: *const ClosureHeader) -> usize {
    crate::closure::js_closure_get_capture_ptr(closure, 0) as usize
}

fn build_read_lines_step(value: f64, done: bool) -> f64 {
    let obj = crate::object::js_object_alloc(0, 2);
    let set = |name: &[u8], v: f64| {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, key, v);
    };
    set(b"value", value);
    set(
        b"done",
        f64::from_bits(if done {
            crate::value::TAG_TRUE
        } else {
            crate::value::TAG_FALSE
        }),
    );
    f64::from_bits(crate::value::JSValue::pointer(obj as *const u8).bits())
}

extern "C" fn read_lines_next_impl(closure: *const ClosureHeader, _arg: f64) -> f64 {
    let id = read_lines_id(closure);
    let next_line = READ_LINES_REGISTRY.with(|states| {
        let mut states = states.borrow_mut();
        let Some(state) = states.get_mut(&id) else {
            return None;
        };
        if state.index >= state.lines.len() {
            close_filehandle_fd(state.fd, state.handle);
            states.remove(&id);
            return None;
        }
        let line = state.lines[state.index].clone();
        state.index += 1;
        Some(line)
    });
    let Some(line) = next_line else {
        return promise_value_fs(build_read_lines_step(
            f64::from_bits(crate::value::TAG_UNDEFINED),
            true,
        ));
    };
    let s = js_string_from_bytes(line.as_ptr(), line.len() as u32);
    let value = f64::from_bits(crate::value::JSValue::string_ptr(s).bits());
    promise_value_fs(build_read_lines_step(value, false))
}

extern "C" fn read_lines_return_impl(closure: *const ClosureHeader, _arg: f64) -> f64 {
    READ_LINES_REGISTRY.with(|states| {
        states.borrow_mut().remove(&read_lines_id(closure));
    });
    promise_value_fs(build_read_lines_step(
        f64::from_bits(crate::value::TAG_UNDEFINED),
        true,
    ))
}

extern "C" fn read_lines_close_impl(closure: *const ClosureHeader) -> f64 {
    READ_LINES_REGISTRY.with(|states| {
        states.borrow_mut().remove(&read_lines_id(closure));
    });
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn read_lines_iterator_impl(closure: *const ClosureHeader) -> f64 {
    f64::from_bits(crate::closure::js_closure_get_capture_ptr(closure, 0) as u64)
}

fn install_read_lines_async_iterator(target: f64, iterator: f64) {
    let async_iterator = crate::symbol::well_known_symbol("asyncIterator");
    if async_iterator.is_null() {
        return;
    }
    let closure = crate::closure::js_closure_alloc(read_lines_iterator_impl as *const u8, 1);
    crate::closure::js_closure_set_capture_ptr(closure, 0, iterator.to_bits() as i64);
    let closure_value = f64::from_bits(crate::value::JSValue::pointer(closure as *const u8).bits());
    let symbol_value =
        f64::from_bits(crate::value::JSValue::pointer(async_iterator as *const u8).bits());
    unsafe {
        crate::symbol::js_object_set_symbol_property(target, symbol_value, closure_value);
    }
}

pub(crate) extern "C" fn filehandle_close_impl(closure: *const ClosureHeader) -> f64 {
    let fd = filehandle_fd(closure);
    if let Some(handle) = filehandle_object(closure) {
        close_filehandle_fd(filehandle_field_fd(handle).unwrap_or(fd), handle);
        return promise_undefined_fs();
    }
    let _ = js_fs_close_sync(fd as f64);
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

pub(crate) extern "C" fn filehandle_read_lines_impl(
    closure: *const ClosureHeader,
    options: f64,
) -> f64 {
    let fallback_fd = filehandle_fd(closure);
    let handle = filehandle_object(closure).unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
    let fd = filehandle_field_fd(handle).unwrap_or(fallback_fd);
    if !fd_is_registered(fd) {
        crate::fs::validate::throw_range_error_with_code(
            "The value of \"fd\" is out of range. It must be >= 0 && <= 2147483647. Received -1",
        );
    }

    let bytes = FD_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let mut bytes = Vec::new();
        let Some(file) = reg.get_mut(&fd) else {
            return bytes;
        };
        let start = unsafe { options_number_field(options, b"start") }
            .filter(|n| n.is_finite() && *n >= 0.0)
            .map(|n| n as u64);
        let end = unsafe { options_number_field(options, b"end") }
            .filter(|n| n.is_finite() && *n >= 0.0)
            .map(|n| n as u64);
        if let Some(start) = start {
            let _ = file.seek(SeekFrom::Start(start));
        }
        if let Some(end) = end {
            let start_for_len = start.unwrap_or(0);
            if end >= start_for_len {
                let max_len = end.saturating_sub(start_for_len).saturating_add(1);
                let _ = Read::by_ref(file).take(max_len).read_to_end(&mut bytes);
            }
        } else {
            let _ = file.read_to_end(&mut bytes);
        }
        bytes
    });
    let text = String::from_utf8_lossy(&bytes);
    let lines = text.lines().map(ToOwned::to_owned).collect::<Vec<_>>();
    let id = NEXT_READ_LINES_ID.with(|next| {
        let mut next = next.borrow_mut();
        let id = *next;
        *next = next.saturating_add(1);
        id
    });
    READ_LINES_REGISTRY.with(|states| {
        states.borrow_mut().insert(
            id,
            ReadLinesState {
                lines,
                index: 0,
                fd,
                handle,
            },
        );
    });

    let iterator_obj = crate::object::js_object_alloc(0, 2);
    let set_iterator = |name: &str, v: f64| {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(iterator_obj, key, v);
    };
    set_iterator(
        "next",
        make_read_lines_method(id, read_lines_next_impl as *const u8),
    );
    set_iterator(
        "return",
        make_read_lines_method(id, read_lines_return_impl as *const u8),
    );
    let iterator = f64::from_bits(crate::value::JSValue::pointer(iterator_obj as *const u8).bits());
    install_read_lines_async_iterator(iterator, iterator);

    let interface_obj = crate::object::js_object_alloc(0, 1);
    let close_key = crate::string::js_string_from_bytes(b"close".as_ptr(), 5);
    crate::object::js_object_set_field_by_name(
        interface_obj,
        close_key,
        make_read_lines_method(id, read_lines_close_impl as *const u8),
    );
    let interface =
        f64::from_bits(crate::value::JSValue::pointer(interface_obj as *const u8).bits());
    install_read_lines_async_iterator(interface, iterator);
    interface
}

/// Build a minimal `fs.promises.FileHandle` object for deterministic parity.
#[no_mangle]
pub extern "C" fn js_fs_filehandle_open(path_value: f64, flags_value: f64) -> f64 {
    let fd = js_fs_open_sync(path_value, flags_value) as i32;
    crate::closure::js_register_closure_arity(filehandle_stat_impl as *const u8, 1);
    crate::closure::js_register_closure_arity(filehandle_read_impl as *const u8, 5);
    crate::closure::js_register_closure_arity(filehandle_write_impl as *const u8, 5);
    crate::closure::js_register_closure_arity(filehandle_read_lines_impl as *const u8, 1);
    crate::closure::js_register_closure_arity(read_lines_next_impl as *const u8, 1);
    crate::closure::js_register_closure_arity(read_lines_return_impl as *const u8, 1);
    crate::closure::js_register_closure_arity(read_lines_close_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(read_lines_iterator_impl as *const u8, 0);
    let obj = crate::object::js_object_alloc(0, 19);
    let handle = f64::from_bits(crate::value::JSValue::pointer(obj as *const u8).bits());
    let set = |name: &str, v: f64| {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, key, v);
    };
    set("fd", fd as f64);
    set(
        "close",
        make_filehandle_method_with_handle(fd, handle, filehandle_close_impl as *const u8),
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
    set(
        "readLines",
        make_filehandle_method_with_handle(fd, handle, filehandle_read_lines_impl as *const u8),
    );
    FILEHANDLE_OBJECT_FDS.with(|fds| {
        fds.borrow_mut().insert(obj as usize, fd);
    });
    handle
}
