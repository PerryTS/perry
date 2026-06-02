//! `fs.Utf8Stream` — the SonicBoom-style fast UTF-8 append stream (the bulk
//! of the #3814 fs additions), split out of `stream.rs` to keep that file
//! under the 2k-line limit. `use super::*` pulls in the private `stream.rs`
//! helpers (`Utf8StreamState`, `UTF8_STREAM_REGISTRY`, `alloc_utf8_stream`,
//! `string_value_str`, the closure plumbing, …). Items used by the rest of
//! `stream.rs` (state init, prop refresh, the `extern "C"` method impls) are
//! `pub(crate)` and re-exported from `stream.rs`.

use super::*;

fn throw_utf8_invalid_arg_type(name: &str, expected: &str, value: f64) -> ! {
    let message = format!(
        "The \"{}\" argument must be of type {}. Received {}",
        name,
        expected,
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn throw_utf8_invalid_arg_value(name: &str, value: impl std::fmt::Display, reason: &str) -> ! {
    let message = format!(
        "The argument '{}' is invalid. Received {}. {}",
        name, value, reason
    );
    crate::fs::validate::throw_range_error_named(&message, "ERR_INVALID_ARG_VALUE")
}

fn js_number_value(value: f64) -> Option<f64> {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_int32() {
        Some(js.as_int32() as f64)
    } else if js.is_number() || value.is_finite() {
        Some(value)
    } else {
        None
    }
}

fn js_i32_number_value(value: f64) -> Option<i32> {
    let n = js_number_value(value)?;
    if n.is_finite() && n.fract() == 0.0 && n >= 0.0 && n <= i32::MAX as f64 {
        Some(n as i32)
    } else {
        None
    }
}

fn is_undefined_or_null(value: f64) -> bool {
    let js = JSValue::from_bits(value.to_bits());
    js.is_undefined() || js.is_null()
}

fn is_undefined_value(value: f64) -> bool {
    JSValue::from_bits(value.to_bits()).is_undefined()
}

fn option_raw_field(options_value: f64, field: &[u8]) -> Option<f64> {
    unsafe { options_field_value(options_value, field).map(|v| f64::from_bits(v.bits())) }
}

fn object_field_value_by_name(object_value: f64, name: &[u8]) -> Option<f64> {
    let obj = object_ptr_from_value(object_value)?;
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = crate::object::js_object_get_field_by_name(obj, key);
    Some(f64::from_bits(value.bits()))
}

fn object_has_callable_field(object_value: f64, name: &[u8]) -> bool {
    object_field_value_by_name(object_value, name)
        .map(is_callable_value)
        .unwrap_or(false)
}

fn validate_utf8_options_object(options_value: f64) {
    let js = JSValue::from_bits(options_value.to_bits());
    if js.is_undefined() || object_ptr_from_value(options_value).is_some() {
        return;
    }
    let message = format!(
        "The \"options\" argument must be of type object. Received {}",
        crate::fs::validate::describe_received(options_value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

fn validate_utf8_custom_fs(options_value: f64) -> f64 {
    let Some(fs_value) = option_raw_field(options_value, b"fs") else {
        return undefined_value();
    };
    if is_undefined_value(fs_value) {
        return undefined_value();
    }
    if object_ptr_from_value(fs_value).is_none() {
        let message = format!(
            "The \"options.fs\" argument must be of type object. Received {}",
            crate::fs::validate::describe_received(fs_value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    for method in [
        b"write".as_slice(),
        b"writeSync".as_slice(),
        b"fsync".as_slice(),
        b"fsyncSync".as_slice(),
        b"close".as_slice(),
        b"open".as_slice(),
        b"mkdir".as_slice(),
        b"mkdirSync".as_slice(),
    ] {
        if let Some(value) = object_field_value_by_name(fs_value, method) {
            if !is_undefined_value(value) && !is_callable_value(value) {
                let method_name = String::from_utf8_lossy(method);
                let message = format!(
                    "The \"options.fs.{}\" argument must be of type function. Received {}",
                    method_name,
                    crate::fs::validate::describe_received(value)
                );
                crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
            }
        }
    }
    fs_value
}

fn utf8_option_bool(options_value: f64, field: &[u8], default_value: bool) -> bool {
    let Some(value) = option_raw_field(options_value, field) else {
        return default_value;
    };
    if is_undefined_value(value) {
        return default_value;
    }
    if crate::value::js_is_truthy(value) == 0 {
        return false;
    }
    let js = JSValue::from_bits(value.to_bits());
    if js.is_bool() {
        return true;
    }
    let field_name = format!("options.{}", String::from_utf8_lossy(field));
    let message = format!(
        "The \"{}\" argument must be of type boolean. Received {}",
        field_name,
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn utf8_option_uint32(options_value: f64, field: &[u8], default_value: usize) -> usize {
    let Some(value) = option_raw_field(options_value, field) else {
        return default_value;
    };
    if is_undefined_value(value) || crate::value::js_is_truthy(value) == 0 {
        return default_value;
    }
    let Some(n) = js_number_value(value) else {
        let field_name = format!("options.{}", String::from_utf8_lossy(field));
        let message = format!(
            "The \"{}\" argument must be of type number. Received {}",
            field_name,
            crate::fs::validate::describe_received(value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    };
    if !n.is_finite() || n.fract() != 0.0 || n < 0.0 || n > u32::MAX as f64 {
        let field_name = format!("options.{}", String::from_utf8_lossy(field));
        let message = format!(
            "The value of \"{}\" is out of range. It must be >= 0 && <= {}. Received {}",
            field_name,
            u32::MAX,
            crate::fs::validate::format_received_number(n)
        );
        crate::fs::validate::throw_range_error_with_code(&message);
    }
    n as usize
}

fn utf8_content_mode(options_value: f64) -> Utf8ContentMode {
    let Some(value) = option_raw_field(options_value, b"contentMode") else {
        return Utf8ContentMode::Utf8;
    };
    if is_undefined_value(value) {
        return Utf8ContentMode::Utf8;
    }
    let Some(mode) = js_string_value(value) else {
        let message = format!(
            "The \"options.contentMode\" argument must be one of: 'buffer', 'utf8'. Received {}",
            crate::fs::validate::describe_received(value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    };
    match mode.as_str() {
        "utf8" => Utf8ContentMode::Utf8,
        "buffer" => Utf8ContentMode::Buffer,
        _ => {
            let message = format!(
                "The \"options.contentMode\" argument must be one of: 'buffer', 'utf8'. Received '{}'",
                mode
            );
            crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE")
        }
    }
}

fn utf8_retry_eagain(options_value: f64) -> f64 {
    let Some(value) = option_raw_field(options_value, b"retryEAGAIN") else {
        return undefined_value();
    };
    if is_undefined_value(value) || crate::value::js_is_truthy(value) == 0 {
        return undefined_value();
    }
    if is_callable_value(value) {
        return value;
    }
    let message = format!(
        "The \"options.retryEAGAIN\" argument must be of type function. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn utf8_fd_or_dest(options_value: f64) -> f64 {
    let fd = option_raw_field(options_value, b"fd").unwrap_or_else(undefined_value);
    if !is_undefined_or_null(fd) {
        return fd;
    }
    option_raw_field(options_value, b"dest").unwrap_or_else(undefined_value)
}

fn utf8_stream_parent_dir(file: &str) -> Option<&Path> {
    let parent = Path::new(file).parent()?;
    if parent.as_os_str().is_empty() {
        None
    } else {
        Some(parent)
    }
}

fn utf8_call_custom_method(custom_fs: f64, method: &[u8], args: &[f64]) -> Option<f64> {
    if is_undefined_value(custom_fs) || !object_has_callable_field(custom_fs, method) {
        return None;
    }
    Some(unsafe {
        crate::object::js_native_call_method(
            custom_fs,
            method.as_ptr() as *const i8,
            method.len(),
            args.as_ptr(),
            args.len(),
        )
    })
}

fn utf8_register_native_fd(file: std::fs::File, path: &str, append_mode: bool) -> i32 {
    let fd = allocate_synthetic_fd();
    FD_REGISTRY.with(|r| {
        r.borrow_mut().insert(fd, file);
    });
    FD_PATHS.with(|r| {
        r.borrow_mut().insert(fd, path.to_string());
    });
    FD_APPEND_MODE.with(|r| {
        r.borrow_mut().insert(fd, append_mode);
    });
    fd
}

fn utf8_open_path_result(
    path_value: f64,
    file: &str,
    append: bool,
    mkdir: bool,
    custom_fs: f64,
) -> Result<i32, f64> {
    if mkdir {
        if let Some(parent) = utf8_stream_parent_dir(file) {
            if let Some(parent_str) = parent.to_str() {
                let parent_value = string_value_str(parent_str);
                let options_obj = crate::object::js_object_alloc(0, 1);
                let key = js_string_from_bytes(b"recursive".as_ptr(), 9);
                crate::object::js_object_set_field_by_name(options_obj, key, bool_value(true));
                let options_value = object_value(options_obj);
                if utf8_call_custom_method(custom_fs, b"mkdirSync", &[parent_value, options_value])
                    .is_none()
                {
                    let _ = std::fs::create_dir_all(parent);
                }
            }
        }
    }

    let flags = if append { "a" } else { "w" };
    let flags_value = string_value_str(flags);
    if let Some(value) = utf8_call_custom_method(
        custom_fs,
        b"openSync",
        &[path_value, flags_value, undefined_value()],
    ) {
        if let Some(fd) = js_i32_number_value(value) {
            return Ok(fd);
        }
    }
    match unsafe { fs_open_sync_result(path_value, flags_value) } {
        Ok(fd) => Ok(fd),
        Err((err, path)) => Err(unsafe { build_fs_error_value(&err, "open", &path) }),
    }
}

fn utf8_open_path(path_value: f64, file: &str, append: bool, mkdir: bool, custom_fs: f64) -> i32 {
    match utf8_open_path_result(path_value, file, append, mkdir, custom_fs) {
        Ok(fd) => fd,
        Err(err) => crate::exception::js_throw(err),
    }
}

fn utf8_native_mkdir_result(file: &str) -> Result<(), f64> {
    if let Some(parent) = utf8_stream_parent_dir(file) {
        if let Err(err) = std::fs::create_dir_all(parent) {
            let path = parent.to_string_lossy();
            return Err(unsafe { build_fs_error_value(&err, "mkdir", &path) });
        }
    }
    Ok(())
}

fn utf8_native_async_open_path_result(
    path_value: f64,
    file: &str,
    append: bool,
    mkdir: bool,
) -> Result<i32, f64> {
    validate::validate_path("path", path_value);
    if mkdir {
        utf8_native_mkdir_result(file)?;
    }

    let mut options = std::fs::OpenOptions::new();
    if append {
        options.write(true).create(true).append(true);
    } else {
        options.write(true).create(true).truncate(true);
    }
    match options.open(file) {
        Ok(file_handle) => Ok(utf8_register_native_fd(file_handle, file, append)),
        Err(err) => Err(unsafe { build_fs_error_value(&err, "open", file) }),
    }
}

pub(crate) fn utf8_initial_state(options_value: f64) -> Utf8StreamState {
    validate_utf8_options_object(options_value);
    let custom_fs = validate_utf8_custom_fs(options_value);
    let content_mode = utf8_content_mode(options_value);
    let min_length = utf8_option_uint32(options_value, b"minLength", 0);
    let max_length = utf8_option_uint32(options_value, b"maxLength", 0);
    let max_write = utf8_option_uint32(options_value, b"maxWrite", 16 * 1024);
    let periodic_flush = utf8_option_uint32(options_value, b"periodicFlush", 0);
    let sync = utf8_option_bool(options_value, b"sync", false);
    let fsync = utf8_option_bool(options_value, b"fsync", false);
    let append = utf8_option_bool(options_value, b"append", true);
    let mkdir = utf8_option_bool(options_value, b"mkdir", false);
    let retry_eagain = utf8_retry_eagain(options_value);
    if min_length >= max_write {
        throw_utf8_invalid_arg_value(
            "minLength",
            min_length,
            &format!("should be smaller than maxWrite ({})", max_write),
        );
    }
    let mode_value = option_raw_field(options_value, b"mode").unwrap_or_else(undefined_value);
    let fd_or_dest = utf8_fd_or_dest(options_value);
    let mut file = None;
    let mut pending_file = None;
    let mut opening = false;
    let mut writing = false;
    let fd = if let Some(numeric_fd) = js_i32_number_value(fd_or_dest) {
        numeric_fd
    } else if JSValue::from_bits(fd_or_dest.to_bits()).is_any_string() {
        let path = path_from_value(fd_or_dest);
        if sync {
            let opened_fd = utf8_open_path(fd_or_dest, &path, append, mkdir, custom_fs);
            file = Some(path);
            opened_fd
        } else {
            pending_file = Some(path);
            opening = true;
            writing = true;
            -1
        }
    } else {
        throw_utf8_invalid_arg_type("fd", "number or string", fd_or_dest);
    };
    Utf8StreamState {
        fd,
        file,
        pending_file,
        reopen_old_fd: None,
        append,
        content_mode,
        sync,
        fsync,
        min_length,
        max_length,
        max_write,
        periodic_flush,
        periodic_flush_timer: None,
        mkdir,
        mode_value,
        retry_eagain,
        custom_fs,
        buffers: Vec::new(),
        len: 0,
        writing,
        opening,
        ending: false,
        destroyed: false,
        closed: false,
        listeners: StdHashMap::new(),
        object_value: undefined_value(),
    }
}

fn utf8_content_mode_str(mode: Utf8ContentMode) -> &'static str {
    match mode {
        Utf8ContentMode::Utf8 => "utf8",
        Utf8ContentMode::Buffer => "buffer",
    }
}

pub(crate) fn update_utf8_props(state: &Utf8StreamState) {
    let obj = state.object_value;
    set_object_field(obj, b"append", bool_value(state.append));
    set_object_field_str(
        obj,
        b"contentMode",
        utf8_content_mode_str(state.content_mode),
    );
    set_object_field(obj, b"fd", state.fd as f64);
    match state.file.as_deref() {
        Some(file) => set_object_field_str(obj, b"file", file),
        None => set_object_field(obj, b"file", null_value()),
    }
    set_object_field(obj, b"fsync", bool_value(state.fsync));
    set_object_field(obj, b"maxLength", state.max_length as f64);
    set_object_field(obj, b"minLength", state.min_length as f64);
    set_object_field(obj, b"mkdir", bool_value(state.mkdir));
    set_object_field(obj, b"mode", state.mode_value);
    set_object_field(obj, b"periodicFlush", state.periodic_flush as f64);
    set_object_field(obj, b"sync", bool_value(state.sync));
    set_object_field(obj, b"writing", bool_value(state.writing));
    set_object_field(obj, b"destroyed", bool_value(state.destroyed));
}

fn utf8_callbacks_for_event(id: usize, event: &str) -> Vec<f64> {
    UTF8_STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(state) = registry.get_mut(&id) else {
            return Vec::new();
        };
        let Some(listeners) = state.listeners.get_mut(event) else {
            return Vec::new();
        };
        let callbacks = listeners.iter().map(|listener| listener.callback).collect();
        listeners.retain(|listener| !listener.once);
        callbacks
    })
}

pub(crate) fn utf8_emit_event0(id: usize, event: &str) {
    let callbacks = utf8_callbacks_for_event(id, event);
    for cb in callbacks {
        let cb_ptr = extract_closure_ptr(cb);
        if !cb_ptr.is_null() {
            crate::closure::js_closure_call0(cb_ptr);
        }
    }
}

fn utf8_emit_event1(id: usize, event: &str, arg: f64) {
    let callbacks = utf8_callbacks_for_event(id, event);
    for cb in callbacks {
        let cb_ptr = extract_closure_ptr(cb);
        if !cb_ptr.is_null() {
            crate::closure::js_closure_call1(cb_ptr, arg);
        }
    }
}

fn utf8_add_listener(id: usize, event: &str, cb: f64, once: bool) {
    if !is_callable_value(cb) {
        return;
    }
    let immediate = UTF8_STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let Some(state) = registry.get(&id) else {
            return None;
        };
        match event {
            "ready" if state.fd >= 0 && !state.destroyed => Some(("ready", undefined_value())),
            "finish" if state.ending && state.destroyed => Some(("finish", undefined_value())),
            "close" if state.closed => Some(("close", undefined_value())),
            _ => None,
        }
    });
    if let Some((_name, _arg)) = immediate {
        let cb_ptr = extract_closure_ptr(cb);
        if !cb_ptr.is_null() {
            crate::closure::js_closure_call0(cb_ptr);
        }
        return;
    }
    UTF8_STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state
                .listeners
                .entry(event.to_string())
                .or_default()
                .push(StreamListener { callback: cb, once });
        }
    });
}

fn utf8_remove_listener(id: usize, event: &str, cb: f64) {
    UTF8_STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            if let Some(listeners) = state.listeners.get_mut(event) {
                listeners.retain(|listener| listener.callback.to_bits() != cb.to_bits());
            }
        }
    });
}

fn utf8_buffered_chunk_value(mode: Utf8ContentMode, bytes: &[u8]) -> f64 {
    match mode {
        Utf8ContentMode::Utf8 => string_value_str(&String::from_utf8_lossy(bytes)),
        Utf8ContentMode::Buffer => buffer_value_from_bytes(bytes),
    }
}

fn utf8_native_write_fd(fd: i32, bytes: &[u8]) -> Result<usize, f64> {
    if bytes.is_empty() {
        return Ok(0);
    }
    write_fd_chunk_result(fd, bytes, false).map(|_| bytes.len())
}

fn utf8_write_chunk(id: usize, bytes: &[u8]) -> Result<usize, f64> {
    let (fd, mode, custom_fs) = UTF8_STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let Some(state) = registry.get(&id) else {
            return (-1, Utf8ContentMode::Utf8, undefined_value());
        };
        (state.fd, state.content_mode, state.custom_fs)
    });
    if fd < 0 {
        return Err(crate::fs::validate::build_ebadf_error_value("write"));
    }
    if !is_undefined_value(custom_fs) && object_has_callable_field(custom_fs, b"writeSync") {
        let data = utf8_buffered_chunk_value(mode, bytes);
        let result = if mode == Utf8ContentMode::Utf8 {
            utf8_call_custom_method(
                custom_fs,
                b"writeSync",
                &[fd as f64, data, string_value(b"utf8")],
            )
        } else {
            utf8_call_custom_method(custom_fs, b"writeSync", &[fd as f64, data])
        };
        return Ok(result
            .and_then(js_number_value)
            .unwrap_or(bytes.len() as f64) as usize);
    }
    utf8_native_write_fd(fd, bytes)
}

fn utf8_fsync(id: usize) {
    let (fd, custom_fs) = UTF8_STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let Some(state) = registry.get(&id) else {
            return (-1, undefined_value());
        };
        (state.fd, state.custom_fs)
    });
    if fd < 0 {
        return;
    }
    if !is_undefined_value(custom_fs) && object_has_callable_field(custom_fs, b"fsyncSync") {
        let _ = utf8_call_custom_method(custom_fs, b"fsyncSync", &[fd as f64]);
    } else if fd_is_registered(fd) {
        let _ = fsync_sync_inner(fd);
    }
}

fn utf8_close_fd(id: usize) {
    let (fd, custom_fs, timer) = UTF8_STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(state) = registry.get_mut(&id) else {
            return (-1, undefined_value(), None);
        };
        let timer = state.periodic_flush_timer.take();
        (state.fd, state.custom_fs, timer)
    });
    if let Some(timer) = timer {
        crate::timer::clearInterval(timer);
    }
    if fd >= 0 {
        if !is_undefined_value(custom_fs) && object_has_callable_field(custom_fs, b"closeSync") {
            let _ = utf8_call_custom_method(custom_fs, b"closeSync", &[fd as f64]);
        } else if fd_is_registered(fd) {
            let _ = js_fs_close_sync(fd as f64);
        }
    }
    UTF8_STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.closed = true;
            update_utf8_props(state);
        }
    });
}

fn utf8_drain_buffers(id: usize) -> Result<(), f64> {
    loop {
        let next = UTF8_STREAM_REGISTRY.with(|registry| {
            let mut registry = registry.borrow_mut();
            let Some(state) = registry.get_mut(&id) else {
                return None;
            };
            if state.destroyed || state.buffers.is_empty() {
                return None;
            }
            state.writing = true;
            update_utf8_props(state);
            Some(state.buffers.remove(0))
        });
        let Some(bytes) = next else {
            break;
        };
        let written = utf8_write_chunk(id, &bytes)?;
        UTF8_STREAM_REGISTRY.with(|registry| {
            if let Some(state) = registry.borrow_mut().get_mut(&id) {
                state.len = state.len.saturating_sub(written);
                update_utf8_props(state);
            }
        });
        utf8_emit_event1(id, "write", written as f64);
    }
    let should_fsync = UTF8_STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(state) = registry.get_mut(&id) else {
            return false;
        };
        state.writing = false;
        update_utf8_props(state);
        state.fsync
    });
    if should_fsync {
        utf8_fsync(id);
    }
    Ok(())
}

fn utf8_maybe_drain_after_write(id: usize) {
    let should_write = UTF8_STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let Some(state) = registry.get(&id) else {
            return false;
        };
        !state.writing && state.len >= state.min_length
    });
    if should_write {
        match utf8_drain_buffers(id) {
            Ok(()) => utf8_emit_event0(id, "drain"),
            Err(err) => utf8_emit_event1(id, "error", err),
        }
    }
}

fn utf8_stream_id_from_value(stream_value: f64) -> Option<usize> {
    let bits = stream_value.to_bits();
    UTF8_STREAM_REGISTRY.with(|registry| {
        registry.borrow().iter().find_map(|(id, state)| {
            if state.object_value.to_bits() == bits {
                Some(*id)
            } else {
                None
            }
        })
    })
}

fn utf8_stream_write_by_id(id: usize, data: f64) -> f64 {
    let (destroyed, mode, max_length, max_write, high_water_mark, current_len) =
        UTF8_STREAM_REGISTRY.with(|registry| {
            let registry = registry.borrow();
            let Some(state) = registry.get(&id) else {
                return (true, Utf8ContentMode::Utf8, 0, 16 * 1024, 16 * 1024 + 3, 0);
            };
            (
                state.destroyed,
                state.content_mode,
                state.max_length,
                state.max_write,
                state.min_length.max(16 * 1024 + 3),
                state.len,
            )
        });
    if destroyed {
        crate::fs::validate::throw_error_with_code(
            "Invalid state: Utf8Stream is destroyed",
            "ERR_INVALID_STATE",
        );
    }
    let bytes = match mode {
        Utf8ContentMode::Utf8 => {
            if !JSValue::from_bits(data.to_bits()).is_any_string() {
                throw_utf8_invalid_arg_type("data", "string", data);
            }
            bytes_from_value(data)
        }
        Utf8ContentMode::Buffer => {
            if crate::buffer::js_buffer_is_buffer(data.to_bits() as i64) != 1 {
                throw_utf8_invalid_arg_type("data", "Buffer", data);
            }
            bytes_from_buffer_value(data)
        }
    };
    let new_len = current_len.saturating_add(bytes.len());
    if max_length > 0 && new_len > max_length {
        utf8_emit_event1(id, "drop", data);
        return bool_value(current_len < high_water_mark);
    }
    UTF8_STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            if state
                .buffers
                .last()
                .is_none_or(|last| last.len().saturating_add(bytes.len()) > max_write)
            {
                state.buffers.push(bytes);
            } else if let Some(last) = state.buffers.last_mut() {
                last.extend_from_slice(&bytes);
            }
            state.len = new_len;
            update_utf8_props(state);
        }
    });
    utf8_maybe_drain_after_write(id);
    let len_after = UTF8_STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .map(|state| state.len)
            .unwrap_or(0)
    });
    bool_value(len_after < high_water_mark)
}

pub(crate) extern "C" fn utf8_stream_write_impl(closure: *const ClosureHeader, data: f64) -> f64 {
    utf8_stream_write_by_id(stream_id_of(closure), data)
}

pub(crate) fn utf8_stream_write_value(stream_value: f64, data: f64) -> Option<f64> {
    utf8_stream_id_from_value(stream_value).map(|id| utf8_stream_write_by_id(id, data))
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_write(stream_value: f64, data: f64) -> f64 {
    utf8_stream_write_value(stream_value, data).unwrap_or_else(undefined_value)
}

fn utf8_stream_flush_by_id(id: usize, callback: f64) -> f64 {
    if !is_undefined_value(callback)
        && !is_undefined_or_null(callback)
        && !is_callable_value(callback)
    {
        let message = format!(
            "The \"cb\" argument must be of type function. Received {}",
            crate::fs::validate::describe_received(callback)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    let (destroyed, min_length, opening_or_writing) = UTF8_STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .map_or((true, 0, false), |state| {
                (
                    state.destroyed,
                    state.min_length,
                    state.opening || state.writing,
                )
            })
    });
    if destroyed {
        let err = crate::fs::validate::build_type_error_with_code_value(
            "Invalid state: Utf8Stream is destroyed",
            "ERR_INVALID_STATE",
        );
        if is_callable_value(callback) {
            let cb = extract_closure_ptr(callback);
            if !cb.is_null() {
                crate::closure::js_closure_call1(cb, err);
            }
            return undefined_value();
        }
        crate::exception::js_throw(err);
    }
    if min_length == 0 {
        if is_callable_value(callback) {
            let cb = extract_closure_ptr(callback);
            if !cb.is_null() {
                crate::closure::js_closure_call0(cb);
            }
        }
        return undefined_value();
    }
    if opening_or_writing {
        if is_callable_value(callback) {
            utf8_add_listener(id, "drain", callback, true);
        }
        return undefined_value();
    }
    let result = utf8_drain_buffers(id);
    if let Err(err) = result {
        utf8_emit_event1(id, "error", err);
        if is_callable_value(callback) {
            let cb = extract_closure_ptr(callback);
            if !cb.is_null() {
                crate::closure::js_closure_call1(cb, err);
            }
        }
    } else if is_callable_value(callback) {
        let cb = extract_closure_ptr(callback);
        if !cb.is_null() {
            crate::closure::js_closure_call0(cb);
        }
    }
    undefined_value()
}

pub(crate) extern "C" fn utf8_stream_flush_impl(
    closure: *const ClosureHeader,
    callback: f64,
) -> f64 {
    utf8_stream_flush_by_id(stream_id_of(closure), callback)
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_flush(stream_value: f64, callback: f64) -> f64 {
    match utf8_stream_id_from_value(stream_value) {
        Some(id) => utf8_stream_flush_by_id(id, callback),
        None => undefined_value(),
    }
}

fn utf8_stream_flush_sync_by_id(id: usize) -> f64 {
    let destroyed_or_bad_fd = UTF8_STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let Some(state) = registry.get(&id) else {
            return Some("Utf8Stream is destroyed");
        };
        if state.destroyed {
            Some("Utf8Stream is destroyed")
        } else if state.fd < 0 {
            Some("Invalid file descriptor")
        } else {
            None
        }
    });
    if let Some(message) = destroyed_or_bad_fd {
        crate::fs::validate::throw_error_with_code(
            &format!("Invalid state: {}", message),
            "ERR_INVALID_STATE",
        );
    }
    if let Err(err) = utf8_drain_buffers(id) {
        crate::exception::js_throw(err);
    }
    utf8_fsync(id);
    undefined_value()
}

pub(crate) extern "C" fn utf8_stream_flush_sync_impl(closure: *const ClosureHeader) -> f64 {
    utf8_stream_flush_sync_by_id(stream_id_of(closure))
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_flush_sync(stream_value: f64) -> f64 {
    match utf8_stream_id_from_value(stream_value) {
        Some(id) => utf8_stream_flush_sync_by_id(id),
        None => undefined_value(),
    }
}

fn utf8_emit_close_events(id: usize, emit_finish: bool) {
    if emit_finish {
        utf8_emit_event0(id, "finish");
    }
    utf8_emit_event0(id, "close");
}

pub(crate) extern "C" fn utf8_close_events_impl(closure: *const ClosureHeader) -> f64 {
    let id = stream_id_of(closure);
    let emit_finish = js_closure_get_capture_ptr(closure, 1) != 0;
    utf8_emit_close_events(id, emit_finish);
    undefined_value()
}

fn utf8_schedule_close_events(id: usize, emit_finish: bool) {
    let closure = js_closure_alloc(utf8_close_events_impl as *const u8, 2);
    js_closure_set_capture_ptr(closure, 0, id as i64);
    js_closure_set_capture_ptr(closure, 1, if emit_finish { 1 } else { 0 });
    crate::builtins::js_queue_microtask(closure as i64);
}

fn utf8_finish_and_close(id: usize, emit_finish: bool) {
    let (should_emit, sync) = UTF8_STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(state) = registry.get_mut(&id) else {
            return (false, true);
        };
        if state.destroyed {
            return (false, state.sync);
        }
        let sync = state.sync;
        state.destroyed = true;
        state.ending = emit_finish || state.ending;
        state.buffers.clear();
        state.len = 0;
        state.writing = false;
        update_utf8_props(state);
        (true, sync)
    });
    if should_emit {
        utf8_close_fd(id);
        if sync {
            utf8_emit_close_events(id, emit_finish);
        } else {
            utf8_schedule_close_events(id, emit_finish);
        }
    }
}

fn utf8_stream_end_by_id(id: usize) -> f64 {
    let (destroyed, opening) = UTF8_STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .map(|state| (state.destroyed, state.opening))
            .unwrap_or((true, false))
    });
    if destroyed {
        crate::fs::validate::throw_error_with_code(
            "Invalid state: Utf8Stream is destroyed",
            "ERR_INVALID_STATE",
        );
    }
    UTF8_STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.ending = true;
            update_utf8_props(state);
        }
    });
    if opening {
        return undefined_value();
    }
    if let Err(err) = utf8_drain_buffers(id) {
        utf8_emit_event1(id, "error", err);
    }
    utf8_finish_and_close(id, true);
    undefined_value()
}

pub(crate) extern "C" fn utf8_stream_end_impl(closure: *const ClosureHeader) -> f64 {
    utf8_stream_end_by_id(stream_id_of(closure))
}

pub(crate) fn utf8_stream_end_value(stream_value: f64, chunk: f64) -> Option<f64> {
    let id = utf8_stream_id_from_value(stream_value)?;
    if !is_undefined_value(chunk) && !is_undefined_or_null(chunk) {
        let _ = utf8_stream_write_by_id(id, chunk);
    }
    Some(utf8_stream_end_by_id(id))
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_end(stream_value: f64, chunk: f64) -> f64 {
    utf8_stream_end_value(stream_value, chunk).unwrap_or_else(undefined_value)
}

fn utf8_stream_destroy_by_id(id: usize) -> f64 {
    utf8_finish_and_close(id, false);
    undefined_value()
}

pub(crate) extern "C" fn utf8_stream_destroy_impl(closure: *const ClosureHeader) -> f64 {
    utf8_stream_destroy_by_id(stream_id_of(closure))
}

pub(crate) fn utf8_stream_destroy_value(stream_value: f64) -> Option<f64> {
    utf8_stream_id_from_value(stream_value).map(|id| {
        let _ = utf8_stream_destroy_by_id(id);
        undefined_value()
    })
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_destroy(stream_value: f64) -> f64 {
    utf8_stream_destroy_value(stream_value).unwrap_or_else(undefined_value)
}

fn utf8_stream_reopen_by_id(id: usize, file_value: f64) -> f64 {
    let (destroyed, existing_file, append, mkdir, sync, custom_fs) =
        UTF8_STREAM_REGISTRY.with(|registry| {
            let registry = registry.borrow();
            let Some(state) = registry.get(&id) else {
                return (true, None, true, false, true, undefined_value());
            };
            (
                state.destroyed,
                state.file.clone(),
                state.append,
                state.mkdir,
                state.sync,
                state.custom_fs,
            )
        });
    if destroyed {
        crate::fs::validate::throw_error_with_code(
            "Invalid state: Utf8Stream is destroyed",
            "ERR_INVALID_STATE",
        );
    }
    let Some(mut file) = existing_file else {
        crate::fs::validate::throw_error_with_code(
            "Unable to reopen a file descriptor, you must pass a file to SonicBoom",
            "ERR_OPERATION_FAILED",
        );
    };
    let new_path_value = if is_undefined_value(file_value) || is_undefined_or_null(file_value) {
        string_value_str(&file)
    } else {
        if !crate::fs::validate::is_path_like(file_value) {
            validate::throw_invalid_path_arg("file", file_value);
        }
        file = unsafe { decode_path_value(file_value).unwrap_or_default() };
        file_value
    };
    let old_fd = UTF8_STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .map(|state| state.fd)
            .unwrap_or(-1)
    });
    if !sync {
        UTF8_STREAM_REGISTRY.with(|registry| {
            if let Some(state) = registry.borrow_mut().get_mut(&id) {
                state.file = Some(file.clone());
                state.pending_file = Some(file);
                state.opening = true;
                state.writing = true;
                state.reopen_old_fd = Some(old_fd);
                state.closed = false;
                update_utf8_props(state);
            }
        });
        utf8_start_async_open(id);
        return undefined_value();
    }
    let new_fd = utf8_open_path(new_path_value, &file, append, mkdir, custom_fs);
    UTF8_STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.fd = new_fd;
            state.file = Some(file);
            state.closed = false;
            update_utf8_props(state);
        }
    });
    if old_fd >= 0 && old_fd != new_fd && fd_is_registered(old_fd) {
        let _ = js_fs_close_sync(old_fd as f64);
    }
    utf8_emit_event0(id, "ready");
    undefined_value()
}

pub(crate) extern "C" fn utf8_stream_reopen_impl(
    closure: *const ClosureHeader,
    file_value: f64,
) -> f64 {
    utf8_stream_reopen_by_id(stream_id_of(closure), file_value)
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_reopen(stream_value: f64, file_value: f64) -> f64 {
    match utf8_stream_id_from_value(stream_value) {
        Some(id) => utf8_stream_reopen_by_id(id, file_value),
        None => undefined_value(),
    }
}

pub(crate) extern "C" fn utf8_stream_on_impl(
    closure: *const ClosureHeader,
    event: f64,
    cb: f64,
) -> f64 {
    let id = stream_id_of(closure);
    utf8_add_listener(id, &event_name(event), cb, false);
    current_receiver_value()
}

pub(crate) extern "C" fn utf8_stream_once_impl(
    closure: *const ClosureHeader,
    event: f64,
    cb: f64,
) -> f64 {
    let id = stream_id_of(closure);
    utf8_add_listener(id, &event_name(event), cb, true);
    current_receiver_value()
}

pub(crate) extern "C" fn utf8_stream_off_impl(
    closure: *const ClosureHeader,
    event: f64,
    cb: f64,
) -> f64 {
    utf8_remove_listener(stream_id_of(closure), &event_name(event), cb);
    current_receiver_value()
}

pub(crate) extern "C" fn utf8_stream_remove_all_impl(
    closure: *const ClosureHeader,
    event: f64,
) -> f64 {
    let id = stream_id_of(closure);
    UTF8_STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            if is_undefined_value(event) {
                state.listeners.clear();
            } else {
                state.listeners.remove(&event_name(event));
            }
        }
    });
    current_receiver_value()
}

pub(crate) extern "C" fn utf8_stream_listener_count_impl(
    closure: *const ClosureHeader,
    event: f64,
) -> f64 {
    let id = stream_id_of(closure);
    let name = event_name(event);
    UTF8_STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .and_then(|state| state.listeners.get(&name))
            .map(|listeners| listeners.len() as f64)
            .unwrap_or(0.0)
    })
}

pub(crate) extern "C" fn utf8_stream_emit_impl(
    closure: *const ClosureHeader,
    event: f64,
    arg: f64,
) -> f64 {
    let id = stream_id_of(closure);
    let name = event_name(event);
    if is_undefined_value(arg) {
        utf8_emit_event0(id, &name);
    } else {
        utf8_emit_event1(id, &name, arg);
    }
    bool_value(true)
}

pub(crate) fn utf8_stream_on_value(
    stream_value: f64,
    event: f64,
    cb: f64,
    once: bool,
) -> Option<f64> {
    let id = utf8_stream_id_from_value(stream_value)?;
    utf8_add_listener(id, &event_name(event), cb, once);
    Some(stream_value)
}

pub(crate) fn utf8_stream_off_value(stream_value: f64, event: f64, cb: f64) -> Option<f64> {
    let id = utf8_stream_id_from_value(stream_value)?;
    utf8_remove_listener(id, &event_name(event), cb);
    Some(stream_value)
}

pub(crate) fn utf8_stream_remove_all_value(stream_value: f64, event: f64) -> Option<f64> {
    let id = utf8_stream_id_from_value(stream_value)?;
    UTF8_STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            if is_undefined_value(event) {
                state.listeners.clear();
            } else {
                state.listeners.remove(&event_name(event));
            }
        }
    });
    Some(stream_value)
}

pub(crate) fn utf8_stream_listener_count_value(stream_value: f64, event: f64) -> Option<f64> {
    let id = utf8_stream_id_from_value(stream_value)?;
    let name = event_name(event);
    Some(UTF8_STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .and_then(|state| state.listeners.get(&name))
            .map(|listeners| listeners.len() as f64)
            .unwrap_or(0.0)
    }))
}

pub(crate) fn utf8_stream_emit_value(stream_value: f64, event: f64, arg: f64) -> Option<f64> {
    let id = utf8_stream_id_from_value(stream_value)?;
    let name = event_name(event);
    if is_undefined_value(arg) {
        utf8_emit_event0(id, &name);
    } else {
        utf8_emit_event1(id, &name, arg);
    }
    Some(bool_value(true))
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_on(stream_value: f64, event: f64, cb: f64) -> f64 {
    utf8_stream_on_value(stream_value, event, cb, false).unwrap_or(stream_value)
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_once(stream_value: f64, event: f64, cb: f64) -> f64 {
    utf8_stream_on_value(stream_value, event, cb, true).unwrap_or(stream_value)
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_off(stream_value: f64, event: f64, cb: f64) -> f64 {
    utf8_stream_off_value(stream_value, event, cb).unwrap_or(stream_value)
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_remove_all(stream_value: f64, event: f64) -> f64 {
    utf8_stream_remove_all_value(stream_value, event).unwrap_or(stream_value)
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_listener_count(stream_value: f64, event: f64) -> f64 {
    utf8_stream_listener_count_value(stream_value, event).unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_fs_utf8_stream_emit(stream_value: f64, event: f64, arg: f64) -> f64 {
    utf8_stream_emit_value(stream_value, event, arg).unwrap_or_else(|| bool_value(false))
}

pub(crate) extern "C" fn utf8_periodic_flush_impl(closure: *const ClosureHeader) -> f64 {
    let id = stream_id_of(closure);
    let destroyed = UTF8_STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .map(|state| state.destroyed)
            .unwrap_or(true)
    });
    if !destroyed {
        let _ = utf8_drain_buffers(id);
    }
    undefined_value()
}

fn utf8_callback_has_error(err: f64) -> bool {
    !is_undefined_or_null(err) && crate::value::js_is_truthy(err) != 0
}

fn utf8_close_raw_fd(fd: i32, custom_fs: f64) {
    if fd < 0 {
        return;
    }
    if !is_undefined_value(custom_fs) && object_has_callable_field(custom_fs, b"closeSync") {
        let _ = utf8_call_custom_method(custom_fs, b"closeSync", &[fd as f64]);
    } else if fd_is_registered(fd) {
        let _ = js_fs_close_sync(fd as f64);
    }
}

fn utf8_async_open_error(id: usize, err: f64) {
    UTF8_STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.opening = false;
            state.writing = false;
            state.reopen_old_fd = None;
            update_utf8_props(state);
        }
    });
    utf8_emit_event1(id, "error", err);
}

fn utf8_async_open_finish(id: usize, err: f64, fd_value: f64) -> f64 {
    if utf8_callback_has_error(err) {
        utf8_async_open_error(id, err);
        return undefined_value();
    }
    let Some(fd) = js_i32_number_value(fd_value) else {
        let message = format!(
            "The \"fd\" argument must be of type number. Received {}",
            crate::fs::validate::describe_received(fd_value)
        );
        let err =
            crate::fs::validate::build_type_error_with_code_value(&message, "ERR_INVALID_ARG_TYPE");
        utf8_async_open_error(id, err);
        return undefined_value();
    };
    let (destroyed, should_drain, should_end, old_fd, custom_fs) =
        UTF8_STREAM_REGISTRY.with(|registry| {
            let mut registry = registry.borrow_mut();
            let Some(state) = registry.get_mut(&id) else {
                return (true, false, false, None, undefined_value());
            };
            if state.destroyed {
                return (
                    true,
                    false,
                    false,
                    state.reopen_old_fd.take(),
                    state.custom_fs,
                );
            }
            state.fd = fd;
            state.file = state.pending_file.take();
            state.opening = false;
            state.writing = false;
            state.closed = false;
            let should_drain = state.len > state.min_length;
            let should_end = state.ending;
            let old_fd = state.reopen_old_fd.take();
            let custom_fs = state.custom_fs;
            update_utf8_props(state);
            (false, should_drain, should_end, old_fd, custom_fs)
        });
    if destroyed {
        utf8_close_raw_fd(fd, custom_fs);
        return undefined_value();
    }
    utf8_emit_event0(id, "ready");
    if let Some(old_fd) = old_fd {
        if old_fd != fd {
            utf8_close_raw_fd(old_fd, custom_fs);
        }
    }
    if should_drain || should_end {
        if let Err(err) = utf8_drain_buffers(id) {
            utf8_emit_event1(id, "error", err);
        }
    }
    if should_end {
        utf8_finish_and_close(id, true);
    } else if should_drain {
        utf8_emit_event0(id, "drain");
    }
    undefined_value()
}

pub(crate) extern "C" fn utf8_async_open_done_impl(
    closure: *const ClosureHeader,
    err: f64,
    fd_value: f64,
) -> f64 {
    utf8_async_open_finish(stream_id_of(closure), err, fd_value)
}

fn utf8_custom_open(id: usize) -> bool {
    let Some((file, append, mode_value, custom_fs)) = UTF8_STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let state = registry.get(&id)?;
        Some((
            state.pending_file.clone()?,
            state.append,
            state.mode_value,
            state.custom_fs,
        ))
    }) else {
        return true;
    };
    if is_undefined_value(custom_fs) || !object_has_callable_field(custom_fs, b"open") {
        return false;
    }
    let cb = js_closure_alloc(utf8_async_open_done_impl as *const u8, 1);
    js_closure_set_capture_ptr(cb, 0, id as i64);
    let cb_value = crate::value::js_nanbox_pointer(cb as i64);
    let flags_value = string_value_str(if append { "a" } else { "w" });
    let path_value = string_value_str(&file);
    let _ = utf8_call_custom_method(
        custom_fs,
        b"open",
        &[path_value, flags_value, mode_value, cb_value],
    );
    true
}

pub(crate) extern "C" fn utf8_async_mkdir_done_impl(
    closure: *const ClosureHeader,
    err: f64,
) -> f64 {
    let id = stream_id_of(closure);
    if utf8_callback_has_error(err) {
        utf8_async_open_error(id, err);
        return undefined_value();
    }
    if !utf8_custom_open(id) {
        utf8_schedule_native_open(id);
    }
    undefined_value()
}

fn utf8_custom_mkdir_then_open(id: usize) -> bool {
    let Some((file, custom_fs)) = UTF8_STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let state = registry.get(&id)?;
        if !state.mkdir {
            return None;
        }
        Some((state.pending_file.clone()?, state.custom_fs))
    }) else {
        return false;
    };
    if is_undefined_value(custom_fs) || !object_has_callable_field(custom_fs, b"mkdir") {
        return false;
    }
    let Some(parent) = utf8_stream_parent_dir(&file) else {
        return false;
    };
    let Some(parent_str) = parent.to_str() else {
        return false;
    };
    let options_obj = crate::object::js_object_alloc(0, 1);
    let key = js_string_from_bytes(b"recursive".as_ptr(), 9);
    crate::object::js_object_set_field_by_name(options_obj, key, bool_value(true));
    let options_value = object_value(options_obj);
    let cb = js_closure_alloc(utf8_async_mkdir_done_impl as *const u8, 1);
    js_closure_set_capture_ptr(cb, 0, id as i64);
    let cb_value = crate::value::js_nanbox_pointer(cb as i64);
    let parent_value = string_value_str(parent_str);
    let _ = utf8_call_custom_method(
        custom_fs,
        b"mkdir",
        &[parent_value, options_value, cb_value],
    );
    true
}

fn utf8_needs_native_mkdir_before_custom_open(id: usize) -> bool {
    UTF8_STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let Some(state) = registry.get(&id) else {
            return false;
        };
        state.mkdir
            && !is_undefined_value(state.custom_fs)
            && object_has_callable_field(state.custom_fs, b"open")
            && !object_has_callable_field(state.custom_fs, b"mkdir")
    })
}

fn utf8_schedule_native_mkdir_then_custom_open(id: usize) {
    let closure = js_closure_alloc(
        utf8_async_native_mkdir_then_custom_open_impl as *const u8,
        1,
    );
    js_closure_set_capture_ptr(closure, 0, id as i64);
    crate::builtins::js_queue_microtask(closure as i64);
}

fn utf8_schedule_native_open(id: usize) {
    let closure = js_closure_alloc(utf8_async_open_impl as *const u8, 1);
    js_closure_set_capture_ptr(closure, 0, id as i64);
    crate::builtins::js_queue_microtask(closure as i64);
}

pub(crate) fn utf8_start_async_open(id: usize) {
    if utf8_custom_mkdir_then_open(id) {
        return;
    }
    if utf8_needs_native_mkdir_before_custom_open(id) {
        utf8_schedule_native_mkdir_then_custom_open(id);
        return;
    }
    if utf8_custom_open(id) {
        return;
    }
    utf8_schedule_native_open(id);
}

extern "C" fn utf8_async_native_mkdir_then_custom_open_impl(closure: *const ClosureHeader) -> f64 {
    let id = stream_id_of(closure);
    let Some(file) = UTF8_STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .and_then(|state| state.pending_file.clone())
    }) else {
        return undefined_value();
    };
    if let Err(err) = utf8_native_mkdir_result(&file) {
        utf8_async_open_error(id, err);
        return undefined_value();
    }
    if !utf8_custom_open(id) {
        utf8_schedule_native_open(id);
    }
    undefined_value()
}

pub(crate) extern "C" fn utf8_async_open_impl(closure: *const ClosureHeader) -> f64 {
    let id = stream_id_of(closure);
    let Some(file) = UTF8_STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .and_then(|state| state.pending_file.clone())
    }) else {
        return undefined_value();
    };
    let (append, mkdir) = UTF8_STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let Some(state) = registry.get(&id) else {
            return (true, false);
        };
        (state.append, state.mkdir)
    });
    let path_value = string_value_str(&file);
    match utf8_native_async_open_path_result(path_value, &file, append, mkdir) {
        Ok(fd) => utf8_async_open_finish(id, null_value(), fd as f64),
        Err(err) => utf8_async_open_finish(id, err, undefined_value()),
    }
}
