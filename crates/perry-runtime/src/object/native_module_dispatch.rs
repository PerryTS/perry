//! Dispatch table for `(native_module_namespace).method(...)` calls
//! that escape into the runtime tower from
//! `js_native_call_method`.
//!
//! Split out of `object/mod.rs` (issue #1103). Pure relocation — no
//! logic changes.

use super::*;

fn crypto_dispatch_value_addr(value: f64) -> usize {
    let bits = value.to_bits();
    if (bits >> 48) >= 0x7FF8 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        bits as usize
    }
}

fn crypto_random_fill_number_arg(value: f64, name: &str) -> Option<f64> {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_undefined() {
        return None;
    }
    if !js.is_number() && !js.is_int32() {
        let message = format!(
            "The \"{}\" argument must be of type number. Received {}",
            name,
            crate::fs::validate::describe_received(value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    Some(if js.is_int32() {
        js.as_int32() as f64
    } else {
        value
    })
}

fn crypto_random_fill_range(total: usize, offset_bits: f64, size_bits: f64) -> (usize, usize) {
    let offset = match crypto_random_fill_number_arg(offset_bits, "offset") {
        Some(n) if n.is_finite() && n >= 0.0 && n <= total as f64 => n as usize,
        Some(n) => {
            let message = format!(
                "The value of \"offset\" is out of range. It must be >= 0 && <= {}. Received {}",
                total, n
            );
            crate::fs::validate::throw_range_error_with_code(&message);
        }
        None => 0,
    };
    let size = match crypto_random_fill_number_arg(size_bits, "size") {
        Some(n) if n.is_finite() && n >= 0.0 && n <= i32::MAX as f64 => n as usize,
        Some(n) => {
            let message = format!(
                "The value of \"size\" is out of range. It must be >= 0 && <= 2147483647. Received {}",
                n
            );
            crate::fs::validate::throw_range_error_with_code(&message);
        }
        None => total.saturating_sub(offset),
    };
    let end = offset.saturating_add(size);
    if end > total {
        let message = format!(
            "The value of \"size + offset\" is out of range. It must be <= {}. Received {}",
            total, end
        );
        crate::fs::validate::throw_range_error_with_code(&message);
    }
    (offset, size)
}

fn crypto_random_fill_invalid_buf(value: f64) -> ! {
    let message = format!(
        "The \"buf\" argument must be an instance of Buffer, TypedArray, DataView, or ArrayBuffer. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

unsafe fn crypto_random_fill_sync_dispatch(target: f64, offset_bits: f64, size_bits: f64) -> f64 {
    use rand::RngCore;

    let addr = crypto_dispatch_value_addr(target);
    if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
        let ta = addr as *mut crate::typedarray::TypedArrayHeader;
        if let Some(data) = crate::typedarray::typed_array_bytes_mut(ta) {
            let elem_size = (*ta).elem_size as usize;
            let len = if elem_size == 0 {
                0
            } else {
                data.len() / elem_size
            };
            let (start_elem, count_elem) = crypto_random_fill_range(len, offset_bits, size_bits);
            let start = start_elem.saturating_mul(elem_size);
            let end = start
                .saturating_add(count_elem.saturating_mul(elem_size))
                .min(data.len());
            if end > start {
                rand::thread_rng().fill_bytes(&mut data[start..end]);
            }
            return target;
        }
        crypto_random_fill_invalid_buf(target);
    }
    if crate::buffer::is_registered_buffer(addr) {
        let buf = addr as *mut crate::buffer::BufferHeader;
        let total = (*buf).length as usize;
        let (start, count) = crypto_random_fill_range(total, offset_bits, size_bits);
        if count > 0 {
            let data = crate::buffer::buffer_data_mut(buf);
            rand::thread_rng().fill_bytes(std::slice::from_raw_parts_mut(data.add(start), count));
        }
        return target;
    }
    crypto_random_fill_invalid_buf(target);
}

/// Dispatch a method call on a native module namespace object.
/// Extracts the module name from the object and dispatches to the appropriate
/// runtime function based on (module_name, method_name).
pub(crate) unsafe fn dispatch_native_module_method(
    obj: *const ObjectHeader,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    // Extract the module name from field 0 of the namespace object
    let module_field = js_object_get_field(obj as *mut _, 0);
    let module_name = if module_field.is_string() {
        let str_ptr = module_field.as_string_ptr();
        let len = (*str_ptr).byte_len as usize;
        let data = (str_ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        std::str::from_utf8(std::slice::from_raw_parts(data, len)).unwrap_or("")
    } else {
        ""
    };
    let module_name = match module_name {
        "path/posix" => "path.posix",
        "path/win32" => "path.win32",
        "async_hooks.default" => "async_hooks",
        "os.default" => "os",
        "path.default" => "path",
        "path.posix.default" => "path.posix",
        "path.win32.default" => "path.win32",
        "querystring.default" => "querystring",
        "url.default" => "url",
        "util.default" => "util",
        _ => module_name,
    };
    // Helper: get arg N as f64
    let arg = |n: usize| -> f64 {
        if n < args_len && !args_ptr.is_null() {
            *args_ptr.add(n)
        } else {
            f64::from_bits(JSValue::undefined().bits())
        }
    };
    let i32_arg = |n: usize| -> i32 {
        let v = arg(n);
        let bits = v.to_bits();
        if (bits >> 48) == 0x7FFE {
            return (bits & 0xFFFF_FFFF) as u32 as i32;
        }
        if v.is_nan() || v.is_infinite() {
            0
        } else {
            v as i32
        }
    };

    let require_path_str_ptr = |n: usize| -> *const crate::StringHeader {
        if n < args_len {
            let v = arg(n);
            let ptr = crate::string::js_string_materialize_to_heap(v);
            if !ptr.is_null() {
                return ptr;
            }
        }
        crate::path::throw_invalid_path_arg_type()
    };
    let optional_path_str_ptr = |n: usize| -> *const crate::StringHeader {
        if n >= args_len {
            return std::ptr::null();
        }
        let v = arg(n);
        let jsv = JSValue::from_bits(v.to_bits());
        if jsv.is_undefined() {
            return std::ptr::null();
        }
        let ptr = crate::string::js_string_materialize_to_heap(v);
        if !ptr.is_null() {
            return ptr;
        }
        crate::path::throw_invalid_path_arg_type()
    };

    // Helper: convert i32 boolean to NaN-boxed TAG_TRUE / TAG_FALSE
    let bool_to_f64 = |v: i32| -> f64 {
        if v != 0 {
            f64::from_bits(0x7FFC_0000_0000_0004) // TAG_TRUE
        } else {
            f64::from_bits(0x7FFC_0000_0000_0003) // TAG_FALSE
        }
    };

    // Helper: convert *mut StringHeader to NaN-boxed string f64
    let str_to_f64 =
        |ptr: *mut crate::StringHeader| -> f64 { f64::from_bits(JSValue::string_ptr(ptr).bits()) };
    let path_join_value = |win32: bool| -> f64 {
        if args_len == 0 {
            let result = if win32 {
                crate::path::js_path_win32_join_unchecked(std::ptr::null(), std::ptr::null())
            } else {
                crate::path::js_path_join_unchecked(std::ptr::null(), std::ptr::null())
            };
            return str_to_f64(result);
        }
        let first = require_path_str_ptr(0);
        let mut result = if win32 {
            crate::path::js_path_win32_join_unchecked(first, std::ptr::null())
        } else {
            crate::path::js_path_join_unchecked(first, std::ptr::null())
        };
        for i in 1..args_len {
            let segment = require_path_str_ptr(i);
            result = if win32 {
                crate::path::js_path_win32_join_unchecked(result, segment)
            } else {
                crate::path::js_path_join_unchecked(result, segment)
            };
        }
        str_to_f64(result)
    };
    let path_resolve_value = |win32: bool| -> f64 {
        let mut result = if args_len == 0 {
            if win32 {
                crate::path::js_path_win32_join_unchecked(std::ptr::null(), std::ptr::null())
            } else {
                crate::path::js_path_join_unchecked(std::ptr::null(), std::ptr::null())
            }
        } else {
            require_path_str_ptr(0) as *mut crate::StringHeader
        };
        for i in 1..args_len {
            let segment = require_path_str_ptr(i);
            result = if win32 {
                crate::path::js_path_win32_resolve_join(result, segment)
            } else {
                crate::path::js_path_resolve_join(result, segment)
            };
        }
        if win32 {
            str_to_f64(crate::path::js_path_win32_resolve(result))
        } else {
            str_to_f64(crate::path::js_path_resolve(result))
        }
    };
    let path_basename_value = |win32: bool| -> f64 {
        let path = require_path_str_ptr(0);
        let ext = optional_path_str_ptr(1);
        if win32 {
            if ext.is_null() {
                str_to_f64(crate::path::js_path_win32_basename(path))
            } else {
                str_to_f64(crate::path::js_path_win32_basename_ext(path, ext))
            }
        } else if ext.is_null() {
            str_to_f64(crate::path::js_path_basename(path))
        } else {
            str_to_f64(crate::path::js_path_basename_ext(path, ext))
        }
    };
    let pack_args = || -> *mut crate::array::ArrayHeader {
        let mut arr = crate::array::js_array_alloc(args_len as u32);
        for i in 0..args_len {
            arr = crate::array::js_array_push_f64(arr, arg(i));
        }
        arr
    };
    let pack_args_from = |start: usize| -> *mut crate::array::ArrayHeader {
        let len = args_len.saturating_sub(start);
        let mut arr = crate::array::js_array_alloc(len as u32);
        for i in start..args_len {
            arr = crate::array::js_array_push_f64(arr, arg(i));
        }
        arr
    };
    let bool_tag = |v: bool| -> f64 {
        if v {
            f64::from_bits(0x7FFC_0000_0000_0004)
        } else {
            f64::from_bits(0x7FFC_0000_0000_0003)
        }
    };
    let ptr_addr = |v: f64| -> usize {
        let bits = v.to_bits();
        if (bits >> 48) >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else {
            bits as usize
        }
    };
    let optional_ptr_addr = |v: f64| -> usize {
        let value = JSValue::from_bits(v.to_bits());
        if value.is_undefined() || value.is_null() {
            0
        } else {
            ptr_addr(v)
        }
    };
    let _arg_event_ptr = |n: usize| -> *const crate::StringHeader {
        crate::value::js_get_string_pointer_unified(arg(n)) as *const crate::StringHeader
    };
    // Raw NaN-box bits of arg `n` (undefined when missing). Used by the
    // process EventEmitter arms so the runtime can coerce event names and
    // validate listeners against the full JS value (#3047/#3046).
    let arg_bits = |n: usize| -> i64 { arg(n).to_bits() as i64 };
    let _arg_closure_ptr = |n: usize| -> *const crate::closure::ClosureHeader {
        if n >= args_len {
            return std::ptr::null();
        }
        let v = arg(n);
        let jsv = JSValue::from_bits(v.to_bits());
        if jsv.is_undefined() || jsv.is_null() {
            std::ptr::null()
        } else {
            ptr_addr(v) as *const crate::closure::ClosureHeader
        }
    };
    let ptr_to_f64 = |ptr: *const u8| -> f64 { f64::from_bits(JSValue::pointer(ptr).bits()) };
    let typed_kind = |v: f64| -> Option<u8> {
        let addr = ptr_addr(v);
        if crate::buffer::is_uint8array_buffer(addr) {
            Some(crate::typedarray::KIND_UINT8)
        } else {
            crate::typedarray::lookup_typed_array_kind(addr)
        }
    };
    match (module_name, method_name) {
        // ── Buffer constructor static API ──
        // `class MyBuffer extends Buffer {}; MyBuffer.from(...)` reaches this
        // path through js_class_static_method_call's native-superclass
        // fallback. Return plain Buffer instances, matching Node's internal
        // FastBuffer behavior rather than species/subclass construction.
        ("buffer.Buffer", "from") => {
            let data = arg(0);
            let second = JSValue::from_bits(arg(1).to_bits());
            let second_is_offset = args_len >= 2
                && !second.is_undefined()
                && !second.is_null()
                && !second.is_string()
                && !second.is_short_string();
            let buf = if args_len >= 3 || second_is_offset {
                let len = if args_len >= 3 { i32_arg(2) } else { -1 };
                crate::buffer::js_buffer_from_arraybuffer_slice(
                    data.to_bits() as i64,
                    i32_arg(1),
                    len,
                )
            } else {
                let enc = if args_len >= 2 {
                    crate::buffer::js_encoding_tag_from_value(arg(1))
                } else {
                    0
                };
                crate::buffer::js_buffer_from_value(data.to_bits() as i64, enc)
            };
            ptr_to_f64(buf as *const u8)
        }
        ("buffer.Buffer", "alloc") => {
            let buf = if args_len >= 2 {
                let enc = if args_len >= 3 {
                    crate::buffer::js_encoding_tag_from_value(arg(2))
                } else {
                    0
                };
                crate::buffer::js_buffer_alloc_fill_value(i32_arg(0), arg(1), enc)
            } else {
                crate::buffer::js_buffer_alloc(i32_arg(0), 0)
            };
            ptr_to_f64(buf as *const u8)
        }
        ("buffer.Buffer", "allocUnsafe") | ("buffer.Buffer", "allocUnsafeSlow") => {
            let buf = crate::buffer::js_buffer_alloc_unsafe(i32_arg(0));
            ptr_to_f64(buf as *const u8)
        }
        ("buffer.Buffer", "concat") => {
            let arr = ptr_addr(arg(0)) as *const crate::array::ArrayHeader;
            let buf = if args_len >= 2 {
                crate::buffer::js_buffer_concat_with_length(arr, arg(1))
            } else {
                crate::buffer::js_buffer_concat(arr)
            };
            ptr_to_f64(buf as *const u8)
        }
        ("buffer.Buffer", "copyBytesFrom") => {
            let buf = crate::buffer::js_buffer_copy_bytes_from(arg(0), arg(1), arg(2));
            ptr_to_f64(buf as *const u8)
        }
        ("buffer.Buffer", "of") => {
            let arr = pack_args();
            ptr_to_f64(crate::buffer::js_buffer_from_array(arr) as *const u8)
        }
        ("buffer.Buffer", "isBuffer") => {
            bool_to_f64(crate::buffer::js_buffer_is_buffer(arg(0).to_bits() as i64))
        }
        ("buffer.Buffer", "isEncoding") => {
            bool_to_f64(crate::buffer::js_buffer_is_encoding(arg(0)))
        }
        ("buffer.Buffer", "byteLength") => {
            crate::buffer::js_buffer_byte_length_value(arg(0), arg(1)) as f64
        }
        ("buffer.Buffer", "compare") => {
            let a = ptr_addr(arg(0));
            let b = ptr_addr(arg(1));
            if crate::buffer::is_registered_buffer(a) && crate::buffer::is_registered_buffer(b) {
                crate::buffer::js_buffer_compare(
                    a as *const crate::buffer::BufferHeader,
                    b as *const crate::buffer::BufferHeader,
                ) as f64
            } else {
                0.0
            }
        }
        ("buffer", "isAscii") => crate::buffer::js_buffer_is_ascii(arg(0)),
        ("buffer", "isUtf8") => crate::buffer::js_buffer_is_utf8(arg(0)),

        // ── process EventEmitter API ──
        ("process", "on") => crate::os::js_process_on(arg_bits(0), arg_bits(1)),
        ("process", "addListener") => crate::os::js_process_add_listener(arg_bits(0), arg_bits(1)),
        ("process", "once") => crate::os::js_process_once(arg_bits(0), arg_bits(1)),
        ("process", "prependListener") => {
            crate::os::js_process_prepend_listener(arg_bits(0), arg_bits(1))
        }
        ("process", "prependOnceListener") => {
            crate::os::js_process_prepend_once_listener(arg_bits(0), arg_bits(1))
        }
        ("process", "emit") => crate::os::js_process_emit(arg_bits(0), pack_args_from(1)),
        ("process", "removeListener") => {
            crate::os::js_process_remove_listener(arg_bits(0), arg_bits(1))
        }
        ("process", "off") => crate::os::js_process_off(arg_bits(0), arg_bits(1)),
        ("process", "removeAllListeners") => {
            crate::os::js_process_remove_all_listeners(arg_bits(0))
        }
        ("process", "listenerCount") => {
            crate::os::js_process_listener_count(arg_bits(0), arg_bits(1))
        }
        ("process", "listeners") => {
            ptr_to_f64(crate::os::js_process_listeners(arg_bits(0)) as *const u8)
        }
        ("process", "rawListeners") => {
            ptr_to_f64(crate::os::js_process_raw_listeners(arg_bits(0)) as *const u8)
        }
        ("process", "eventNames") => ptr_to_f64(crate::os::js_process_event_names() as *const u8),
        ("process", "setMaxListeners") => crate::os::js_process_set_max_listeners(arg(0)),
        ("process", "getMaxListeners") => crate::os::js_process_get_max_listeners(),
        ("process", "emitWarning") => {
            crate::process::js_process_emit_warning(arg(0), arg(1), arg(2));
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        ("process", "getBuiltinModule") => crate::process::js_process_get_builtin_module(arg(0)),
        ("module", "enableCompileCache") => crate::process::js_module_enable_compile_cache(arg(0)),
        ("module", "flushCompileCache") => crate::process::js_module_flush_compile_cache(),
        ("module", "getCompileCacheDir") => crate::process::js_module_get_compile_cache_dir(),
        ("module", "getSourceMapsSupport") => crate::process::js_module_get_source_maps_support(),
        ("module", "isBuiltin") => crate::process::js_module_is_builtin(arg(0)),
        ("module", "setSourceMapsSupport") => {
            crate::process::js_module_set_source_maps_support(arg(0), arg(1))
        }
        ("module", "stripTypeScriptTypes") => {
            crate::process::js_module_strip_typescript_types(arg(0), arg(1))
        }
        ("process", "cwd") => str_to_f64(crate::os::js_process_cwd()),
        ("process", "uptime") => crate::os::js_process_uptime(),
        ("process", "memoryUsage") => crate::process::js_process_memory_usage(),
        ("process", "threadCpuUsage") => crate::process::js_process_thread_cpu_usage(arg(0)),
        ("process", "availableMemory") => crate::process::js_process_available_memory(),
        ("process", "constrainedMemory") => crate::process::js_process_constrained_memory(),
        ("process", "resourceUsage") => crate::process::js_process_resource_usage(),
        ("process", "getActiveResourcesInfo") => crate::process::js_process_active_resources_info(),
        ("process", "getuid") => crate::process::js_process_getuid(),
        ("process", "geteuid") => crate::process::js_process_geteuid(),
        ("process", "getgid") => crate::process::js_process_getgid(),
        ("process", "getegid") => crate::process::js_process_getegid(),
        ("process", "sourceMapsEnabled") => crate::process::js_process_source_maps_enabled(),
        ("process", "setSourceMapsEnabled") => {
            crate::process::js_process_set_source_maps_enabled(arg(0))
        }
        ("process", "hasUncaughtExceptionCaptureCallback") => {
            crate::process::js_process_has_uncaught_exception_capture_callback()
        }
        ("process", "setUncaughtExceptionCaptureCallback") => {
            crate::process::js_process_set_uncaught_exception_capture_callback(arg(0))
        }
        ("process", "addUncaughtExceptionCaptureCallback") => {
            crate::process::js_process_add_uncaught_exception_capture_callback(arg(0))
        }
        ("process", "nextTick") => {
            // Validate the callback and forward trailing args (#3046).
            unsafe { crate::os::js_process_next_tick(arg_bits(0), pack_args_from(1)) };
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        ("process", "chdir") => {
            // #3043 — route dynamic/method-value chdir calls through the
            // full-value validator (matching the static codegen path) so a
            // non-string argument throws TypeError [ERR_INVALID_ARG_TYPE]
            // instead of silently no-oping on a null string pointer.
            unsafe {
                crate::process::js_process_chdir_jsv(arg(0));
            }
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        ("process", "loadEnvFile") => {
            crate::process::js_process_load_env_file(arg(0));
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        ("events", "init") => f64::from_bits(crate::value::TAG_UNDEFINED),
        ("events", "EventEmitterAsyncResource") => {
            let message =
                b"Class constructor EventEmitterAsyncResource cannot be invoked without 'new'";
            let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
            let err = crate::error::js_typeerror_new(msg);
            crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
        }
        ("process", "getgroups") => crate::process::js_process_getgroups(),
        ("process", "setuid") => {
            crate::process::js_process_setuid(arg(0));
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        ("process", "seteuid") => {
            crate::process::js_process_seteuid(arg(0));
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        ("process", "setgid") => {
            crate::process::js_process_setgid(arg(0));
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        ("process", "setegid") => {
            crate::process::js_process_setegid(arg(0));
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        ("process", "setgroups") => {
            crate::process::js_process_setgroups(arg(0));
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        ("process", "initgroups") => {
            crate::process::js_process_initgroups(arg(0), arg(1));
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        ("process", "kill") => crate::os::js_process_kill(arg(0), arg(1)),
        ("process", "exit") => {
            crate::process::js_process_exit(arg(0));
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        ("process", "abort") => {
            crate::process::js_process_abort();
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        ("process", "umask") => {
            let mask = arg(0);
            let mask_value = JSValue::from_bits(mask.to_bits());
            if mask_value.is_undefined() {
                crate::process::js_process_umask()
            } else {
                crate::process::js_process_umask_set(mask)
            }
        }
        ("process", "emitWarning") => {
            crate::process::js_process_emit_warning(arg(0), arg(1), arg(2));
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        ("process", "hrtime") => crate::os::js_process_hrtime(arg(0)),
        ("process", "cpuUsage") => crate::process::js_process_cpu_usage(arg(0)),
        // ── crypto module ──
        ("crypto", "randomFillSync") if args_len >= 1 => {
            crypto_random_fill_sync_dispatch(arg(0), arg(1), arg(2))
        }
        ("crypto.webcrypto", "getRandomValues") if args_len >= 1 => {
            let undefined = f64::from_bits(JSValue::undefined().bits());
            crypto_random_fill_sync_dispatch(arg(0), undefined, undefined)
        }

        // ── tty module ──
        ("tty", "isatty") => crate::tty::js_tty_isatty(arg(0)),
        ("tty", "ReadStream") => crate::tty::js_tty_read_stream_new(arg(0)),
        ("tty", "WriteStream") => crate::tty::js_tty_write_stream_new(arg(0)),

        // ── tls module helpers ──
        ("tls", "getCiphers") => crate::tls::js_tls_get_ciphers(),
        ("tls", "getCACertificates") => crate::tls::js_tls_get_ca_certificates(arg(0)),
        ("tls", "setDefaultCACertificates") => {
            crate::tls::js_tls_set_default_ca_certificates(arg(0))
        }
        ("tls", "checkServerIdentity") => crate::tls::js_tls_check_server_identity(arg(0), arg(1)),
        ("tls", "createSecureContext") => crate::tls::js_tls_create_secure_context(arg(0)),
        ("tls", "SecureContext") => crate::tls::js_tls_secure_context_new(arg(0)),

        // ── wasi module ──
        ("wasi", "WASI") => crate::wasi::js_wasi_constructor_call(arg(0)),

        // ── net module legacy/internal helpers ──
        ("net", "_normalizeArgs") => crate::net_validate::js_net_normalize_args(arg(0)),
        ("net", "_createServerHandle") => crate::net_validate::js_net_create_server_handle_stub(
            arg(0),
            arg(1),
            arg(2),
            arg(3),
            arg(4),
        ),

        // ── perf_hooks module (performance.*) ──
        // Statically lowered at call sites (module_static.rs); these arms
        // also serve the generic namespace-object method-dispatch path.
        ("perf_hooks", "now") => crate::date::js_performance_now(),
        ("perf_hooks", "mark") => crate::perf_hooks::js_perf_mark(arg(0), arg(1)),
        ("perf_hooks", "measure") => crate::perf_hooks::js_perf_measure(arg(0), arg(1), arg(2)),
        ("perf_hooks", "getEntries") => crate::perf_hooks::js_perf_get_entries(),
        ("perf_hooks", "getEntriesByType") => {
            crate::perf_hooks::js_perf_get_entries_by_type(arg(0))
        }
        ("perf_hooks", "getEntriesByName") => {
            crate::perf_hooks::js_perf_get_entries_by_name(arg(0), arg(1))
        }
        ("perf_hooks", "clearMarks") => crate::perf_hooks::js_perf_clear_marks(arg(0)),
        ("perf_hooks", "clearMeasures") => crate::perf_hooks::js_perf_clear_measures(arg(0)),
        ("perf_hooks", "eventLoopUtilization") => {
            crate::perf_hooks::js_perf_event_loop_utilization(arg(0), arg(1))
        }
        ("perf_hooks", "toJSON") => crate::perf_hooks::js_perf_to_json(),
        ("perf_hooks", "clearResourceTimings") => {
            crate::perf_hooks::js_perf_clear_resource_timings()
        }
        ("perf_hooks", "setResourceTimingBufferSize") => {
            crate::perf_hooks::js_perf_set_resource_timing_buffer_size(arg(0))
        }
        ("perf_hooks", "markResourceTiming") => crate::perf_hooks::js_perf_mark_resource_timing(
            arg(0),
            arg(1),
            arg(2),
            arg(3),
            arg(4),
            arg(5),
            arg(6),
            arg(7),
        ),
        ("perf_hooks", "timerify") => crate::perf_hooks::js_perf_timerify(arg(0), arg(1)),

        // ── PerformanceObserver instance (perf_observer) ──
        // The registry index lives in field[1] of the namespace object; the
        // runtime fns re-derive it from the object value.
        ("perf_observer", "observe") => {
            let obs_val = crate::value::js_nanbox_pointer(obj as i64);
            crate::perf_hooks::js_perf_observer_observe(obs_val, arg(0))
        }
        ("perf_observer", "disconnect") => {
            let obs_val = crate::value::js_nanbox_pointer(obj as i64);
            crate::perf_hooks::js_perf_observer_disconnect(obs_val)
        }
        ("perf_observer", "takeRecords") => {
            let obs_val = crate::value::js_nanbox_pointer(obj as i64);
            crate::perf_hooks::js_perf_observer_take_records(obs_val)
        }

        // ── PerformanceObserverEntryList (the callback `list` arg) ──
        ("perf_observer_list", "getEntries") => crate::perf_hooks::current_list_get_entries(),
        ("perf_observer_list", "getEntriesByType") => {
            crate::perf_hooks::current_list_get_by_type(arg(0))
        }
        ("perf_observer_list", "getEntriesByName") => {
            crate::perf_hooks::current_list_get_by_name(arg(0))
        }

        // ── Histogram instance methods (#1336) ──
        // Every method is a no-op on the stub — `enable`/`disable`/`reset`
        // don't sample anything, `record`/`recordDelta`/`add` discard input.
        // `percentile(p)` returns 0 (no samples => no rank).
        ("perf_histogram", "enable")
        | ("perf_histogram", "disable")
        | ("perf_histogram", "reset")
        | ("perf_histogram", "record")
        | ("perf_histogram", "recordDelta")
        | ("perf_histogram", "add") => crate::perf_hooks::js_perf_histogram_noop(),
        ("perf_histogram", "percentile") | ("perf_histogram", "percentileBigInt") => {
            crate::perf_hooks::js_perf_histogram_percentile(arg(0))
        }

        // ── timers module ──
        ("timers", "setTimeout") if args_len >= 2 => {
            let cb = arg(0);
            let delay = arg(1);
            let cb_handle = {
                let bits = cb.to_bits();
                if (bits >> 48) >= 0x7FF8 {
                    (bits & 0x0000_FFFF_FFFF_FFFF) as i64
                } else {
                    bits as i64
                }
            };
            if args_len > 2 {
                let extra_ptr = unsafe { args_ptr.add(2) };
                return f64::from_bits(
                    JSValue::pointer(crate::timer::js_set_timeout_callback_args(
                        cb_handle,
                        delay,
                        extra_ptr,
                        (args_len - 2) as i32,
                    ) as *mut u8)
                    .bits(),
                );
            }
            return f64::from_bits(JSValue::pointer(
                crate::timer::js_set_timeout_callback(cb_handle, delay) as *mut u8,
            ).bits());
        }
        ("timers", "setImmediate") if args_len >= 1 => {
            let cb = arg(0);
            let cb_handle = {
                let bits = cb.to_bits();
                if (bits >> 48) >= 0x7FF8 {
                    (bits & 0x0000_FFFF_FFFF_FFFF) as i64
                } else {
                    bits as i64
                }
            };
            if args_len > 1 {
                let extra_ptr = unsafe { args_ptr.add(1) };
                return f64::from_bits(
                    JSValue::pointer(crate::timer::js_set_immediate_callback_args(
                        cb_handle,
                        extra_ptr,
                        (args_len - 1) as i32,
                    ) as *mut u8)
                    .bits(),
                );
            }
            return f64::from_bits(
                JSValue::pointer(crate::timer::js_set_immediate_callback(cb_handle) as *mut u8)
                    .bits(),
            );
        }
        ("timers", "setInterval") if args_len >= 2 => {
            let cb = arg(0);
            let delay = arg(1);
            let bits = cb.to_bits();
            let cb_handle = if (bits >> 48) >= 0x7FF8 {
                (bits & 0x0000_FFFF_FFFF_FFFF) as i64
            } else {
                bits as i64
            };
            if args_len > 2 {
                let extra_ptr = unsafe { args_ptr.add(2) };
                return f64::from_bits(
                    JSValue::pointer(crate::timer::js_set_interval_callback_args(
                        cb_handle,
                        delay,
                        extra_ptr,
                        (args_len - 2) as i32,
                    ) as *mut u8)
                    .bits(),
                );
            }
            return f64::from_bits(
                JSValue::pointer(crate::timer::setInterval(cb_handle, delay) as *mut u8).bits(),
            );
        }
        ("timers", "clearTimeout") if args_len >= 1 => {
            crate::timer::js_clear_timeout_value(arg(0));
            return f64::from_bits(JSValue::undefined().bits());
        }
        ("timers", "clearImmediate") if args_len >= 1 => {
            crate::timer::js_clear_immediate_value(arg(0));
            return f64::from_bits(JSValue::undefined().bits());
        }
        ("timers", "clearInterval") if args_len >= 1 => {
            crate::timer::js_clear_interval_value(arg(0));
            return f64::from_bits(JSValue::undefined().bits());
        }
        // ── assert module ──
        // Root-callable `assert(x, msg)` / `assert.strict(x, msg)` —
        // HIR lowers these to method "default".
        ("assert", "default") | ("assert/strict", "default") => js_assert_ok(arg(0), arg(1)),
        ("assert", "strict") | ("assert/strict", "strict") => js_assert_ok(arg(0), arg(1)),
        ("assert", "ok") | ("assert/strict", "ok") => js_assert_ok(arg(0), arg(1)),
        ("assert", "fail") | ("assert/strict", "fail") => js_assert_fail(arg(0)),
        ("assert", "equal") => js_assert_equal(arg(0), arg(1), arg(2)),
        ("assert", "notEqual") => js_assert_not_equal(arg(0), arg(1), arg(2)),
        ("assert", "strictEqual")
        | ("assert/strict", "strictEqual")
        | ("assert/strict", "equal") => js_assert_strict_equal(arg(0), arg(1), arg(2)),
        ("assert", "notStrictEqual")
        | ("assert/strict", "notStrictEqual")
        | ("assert/strict", "notEqual") => js_assert_not_strict_equal(arg(0), arg(1), arg(2)),
        ("assert", "deepEqual") => js_assert_deep_equal(arg(0), arg(1), arg(2)),
        ("assert", "notDeepEqual") => js_assert_not_deep_equal(arg(0), arg(1), arg(2)),
        ("assert", "deepStrictEqual")
        | ("assert/strict", "deepStrictEqual")
        | ("assert/strict", "deepEqual") => js_assert_deep_strict_equal(arg(0), arg(1), arg(2)),
        ("assert", "partialDeepStrictEqual") | ("assert/strict", "partialDeepStrictEqual") => {
            js_assert_partial_deep_strict_equal(arg(0), arg(1), arg(2))
        }
        ("assert", "notDeepStrictEqual")
        | ("assert/strict", "notDeepStrictEqual")
        | ("assert/strict", "notDeepEqual") => {
            js_assert_not_deep_strict_equal(arg(0), arg(1), arg(2))
        }
        ("assert", "match") | ("assert/strict", "match") => js_assert_match(arg(0), arg(1), arg(2)),
        ("assert", "doesNotMatch") | ("assert/strict", "doesNotMatch") => {
            js_assert_does_not_match(arg(0), arg(1), arg(2))
        }
        ("assert", "throws") | ("assert/strict", "throws") => {
            js_assert_throws(arg(0), arg(1), arg(2))
        }
        ("assert", "doesNotThrow") | ("assert/strict", "doesNotThrow") => {
            js_assert_does_not_throw(arg(0), arg(1), arg(2))
        }
        ("assert", "rejects") | ("assert/strict", "rejects") => {
            js_assert_rejects(arg(0), arg(1), arg(2))
        }
        ("assert", "doesNotReject") | ("assert/strict", "doesNotReject") => {
            js_assert_does_not_reject(arg(0), arg(1), arg(2))
        }
        ("assert", "ifError") | ("assert/strict", "ifError") => js_assert_if_error(arg(0)),

        // ── fs module (args are NaN-boxed f64, booleans return as i32→f64) ──
        ("fs", "_toUnixTimestamp") => crate::fs::js_fs_to_unix_timestamp(arg(0)),
        ("fs", "existsSync") => bool_to_f64(crate::fs::js_fs_exists_sync(arg(0))),
        ("fs", "readFileSync") => crate::fs::js_fs_read_file_dispatch(arg(0), arg(1)),
        ("fs", "writeFileSync") => bool_to_f64(crate::fs::js_fs_write_file_sync_options(
            arg(0),
            arg(1),
            arg(2),
        )),
        ("fs", "appendFileSync") => bool_to_f64(crate::fs::js_fs_append_file_sync_options(
            arg(0),
            arg(1),
            arg(2),
        )),
        ("fs", "mkdirSync") => bool_to_f64(crate::fs::js_fs_mkdir_sync_options(arg(0), arg(1))),
        ("fs", "unlinkSync") => bool_to_f64(crate::fs::js_fs_unlink_sync(arg(0))),
        ("fs", "rmSync") => bool_to_f64(crate::fs::js_fs_rm_recursive_options(arg(0), arg(1))),
        ("fs", "rmdirSync") => bool_to_f64(crate::fs::js_fs_rmdir_sync_options(arg(0), arg(1))),
        ("fs", "readdirSync") => {
            let raw = crate::fs::js_fs_readdir_sync(arg(0), arg(1));
            f64::from_bits(JSValue::pointer(raw.to_bits() as *const u8).bits())
        }
        ("fs", "statSync") => crate::fs::js_fs_stat_sync_options(arg(0), arg(1)),
        ("fs", "lstatSync") => crate::fs::js_fs_lstat_sync_options(arg(0), arg(1)),
        ("fs", "renameSync") => bool_to_f64(crate::fs::js_fs_rename_sync(arg(0), arg(1))),
        ("fs", "copyFileSync") => bool_to_f64(crate::fs::js_fs_copy_file_sync_flags(
            arg(0),
            arg(1),
            arg(2),
        )),
        ("fs", "cpSync") => bool_to_f64(crate::fs::js_fs_cp_sync_options(arg(0), arg(1), arg(2))),
        ("fs", "accessSync") => crate::fs::js_fs_access_sync_throw_mode(arg(0), arg(1)),
        ("fs", "realpathSync") => crate::fs::js_fs_realpath_dispatch(arg(0), arg(1)),
        ("fs", "mkdtempSync") => crate::fs::js_fs_mkdtemp_dispatch(arg(0), arg(1)),
        ("fs", "mkdtempDisposableSync") => crate::fs::js_fs_mkdtemp_disposable_sync(arg(0), arg(1)),
        ("fs", "chmodSync") => bool_to_f64(crate::fs::js_fs_chmod_sync(arg(0), arg(1))),
        ("fs", "chownSync") => bool_to_f64(crate::fs::js_fs_chown_sync(arg(0), arg(1), arg(2))),
        ("fs", "lchownSync") => bool_to_f64(crate::fs::js_fs_lchown_sync(arg(0), arg(1), arg(2))),
        ("fs", "lchmodSync") => bool_to_f64(crate::fs::js_fs_lchmod_sync(arg(0), arg(1))),
        ("fs", "truncateSync") => bool_to_f64(crate::fs::js_fs_truncate_sync(arg(0), arg(1))),
        ("fs", "ftruncateSync") => bool_to_f64(crate::fs::js_fs_ftruncate_sync(arg(0), arg(1))),
        ("fs", "fsyncSync") => bool_to_f64(crate::fs::js_fs_fsync_sync(arg(0))),
        ("fs", "fdatasyncSync") => bool_to_f64(crate::fs::js_fs_fdatasync_sync(arg(0))),
        ("fs", "fchmodSync") => bool_to_f64(crate::fs::js_fs_fchmod_sync(arg(0), arg(1))),
        ("fs", "fchownSync") => bool_to_f64(crate::fs::js_fs_fchown_sync(arg(0), arg(1), arg(2))),
        ("fs", "fstatSync") => crate::fs::js_fs_fstat_sync_options(arg(0), arg(1)),
        ("fs", "utimesSync") => crate::fs::js_fs_utimes_sync(arg(0), arg(1), arg(2)) as f64,
        ("fs", "lutimesSync") => crate::fs::js_fs_lutimes_sync(arg(0), arg(1), arg(2)) as f64,
        ("fs", "futimesSync") => crate::fs::js_fs_futimes_sync(arg(0), arg(1), arg(2)) as f64,
        ("fs", "_toUnixTimestamp") => crate::fs::js_fs_to_unix_timestamp(arg(0)),
        ("fs", "readvSync") => crate::fs::js_fs_readv_sync(arg(0), arg(1), arg(2)),
        ("fs", "writevSync") => crate::fs::js_fs_writev_sync(arg(0), arg(1), arg(2)),
        ("fs", "statfsSync") => crate::fs::js_fs_statfs_sync_options(arg(0), arg(1)),
        ("fs", "opendirSync") => crate::fs::js_fs_opendir_sync(arg(0)),
        ("fs", "globSync") => {
            let raw = crate::fs::js_fs_glob_sync_options(arg(0), arg(1));
            f64::from_bits(JSValue::pointer(raw.to_bits() as *const u8).bits())
        }
        ("fs", "watch") => crate::fs::js_fs_watch(arg(0), arg(1), arg(2)),
        ("fs", "watchFile") => crate::fs::js_fs_watch_file(arg(0), arg(1), arg(2)),
        ("fs", "unwatchFile") => crate::fs::js_fs_unwatch_file(arg(0), arg(1)),
        ("fs", "linkSync") => bool_to_f64(crate::fs::js_fs_link_sync(arg(0), arg(1))),
        ("fs", "symlinkSync") => bool_to_f64(crate::fs::js_fs_symlink_sync(arg(0), arg(1))),
        ("fs", "readlinkSync") => crate::fs::js_fs_readlink_dispatch(arg(0), arg(1)),
        ("fs", "openSync") => crate::fs::js_fs_open_sync(arg(0), arg(1)),
        ("fs", "openAsBlob") => crate::fs::js_fs_open_as_blob(arg(0), arg(1)),
        ("fs", "closeSync") => bool_to_f64(crate::fs::js_fs_close_sync(arg(0))),
        ("fs", "readSync") if args_len == 3 => {
            crate::fs::js_fs_read_sync_options(arg(0), arg(1), arg(2))
        }
        ("fs", "readSync") => crate::fs::js_fs_read_sync(arg(0), arg(1), arg(2), arg(3), arg(4)),
        ("fs", "writeSync") if args_len >= 5 => {
            crate::fs::js_fs_write_buffer_sync(arg(0), arg(1), arg(2), arg(3), arg(4))
        }
        ("fs", "writeSync") if args_len >= 3 => {
            crate::fs::js_fs_write_sync_options_dispatch(arg(0), arg(1), arg(2))
        }
        ("fs", "writeSync") => crate::fs::js_fs_write_sync(arg(0), arg(1)),
        ("fs", "read") if args_len == 4 => {
            crate::fs::js_fs_read_callback_options(arg(0), arg(1), arg(2), arg(3))
        }
        ("fs", "read") => {
            crate::fs::js_fs_read_callback(arg(0), arg(1), arg(2), arg(3), arg(4), arg(5))
        }
        ("fs", "write") if args_len >= 6 => {
            crate::fs::js_fs_write_buffer_callback(arg(0), arg(1), arg(2), arg(3), arg(4), arg(5))
        }
        ("fs", "write") if args_len == 4 => {
            crate::fs::js_fs_write_buffer_callback_options(arg(0), arg(1), arg(2), arg(3))
        }
        ("fs", "write") => crate::fs::js_fs_write_callback(arg(0), arg(1), arg(2)),
        ("fs", "readv") => crate::fs::js_fs_readv_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "writev") => crate::fs::js_fs_writev_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "createWriteStream") => crate::fs::js_fs_create_write_stream(arg(0), arg(1)),
        ("fs", "createReadStream") => crate::fs::js_fs_create_read_stream(arg(0), arg(1)),
        ("fs", "WriteStream") | ("fs", "FileWriteStream") => {
            crate::fs::js_fs_create_write_stream(arg(0), arg(1))
        }
        ("fs", "ReadStream") | ("fs", "FileReadStream") => {
            crate::fs::js_fs_create_read_stream(arg(0), arg(1))
        }
        ("fs", "Utf8Stream") => crate::fs::js_fs_utf8_stream_call_without_new(arg(0)),
        ("fs", "readFile") => crate::fs::js_fs_read_file_callback(arg(0), arg(1), arg(2)),
        ("fs", "writeFile") => crate::fs::js_fs_write_file_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "appendFile") => {
            crate::fs::js_fs_append_file_callback(arg(0), arg(1), arg(2), arg(3))
        }
        ("fs", "chmod") => crate::fs::js_fs_chmod_callback(arg(0), arg(1), arg(2)),
        ("fs", "chown") => crate::fs::js_fs_chown_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "lchown") => crate::fs::js_fs_lchown_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "lchmod") => crate::fs::js_fs_lchmod_callback(arg(0), arg(1), arg(2)),
        ("fs", "truncate") => crate::fs::js_fs_truncate_callback(arg(0), arg(1), arg(2)),
        ("fs", "link") => crate::fs::js_fs_link_callback(arg(0), arg(1), arg(2)),
        ("fs", "symlink") => crate::fs::js_fs_symlink_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "readlink") => crate::fs::js_fs_readlink_callback(arg(0), arg(1), arg(2)),
        ("fs", "realpath") => crate::fs::js_fs_realpath_callback(arg(0), arg(1), arg(2)),
        ("fs", "mkdtemp") => crate::fs::js_fs_mkdtemp_callback(arg(0), arg(1), arg(2)),
        ("fs", "open") => crate::fs::js_fs_open_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "close") => crate::fs::js_fs_close_callback(arg(0), arg(1)),
        ("fs", "cp") => crate::fs::js_fs_cp_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "mkdir") => crate::fs::js_fs_mkdir_callback(arg(0), arg(1), arg(2)),
        ("fs", "unlink") => crate::fs::js_fs_unlink_callback(arg(0), arg(1)),
        ("fs", "rmdir") => crate::fs::js_fs_rmdir_callback(arg(0), arg(1), arg(2)),
        ("fs", "rm") => crate::fs::js_fs_rm_callback(arg(0), arg(1), arg(2)),
        ("fs", "access") => crate::fs::js_fs_access_callback(arg(0), arg(1), arg(2)),
        ("fs", "exists") => crate::fs::js_fs_exists_callback(arg(0), arg(1)),
        ("fs", "readdir") => crate::fs::js_fs_readdir_callback(arg(0), arg(1), arg(2)),
        ("fs", "stat") => crate::fs::js_fs_stat_callback(arg(0), arg(1), arg(2)),
        ("fs", "lstat") => crate::fs::js_fs_lstat_callback(arg(0), arg(1), arg(2)),
        ("fs", "statfs") => crate::fs::js_fs_statfs_callback(arg(0), arg(1), arg(2)),
        ("fs", "opendir") => crate::fs::js_fs_opendir_callback(arg(0), arg(1), arg(2)),
        ("fs", "glob") => crate::fs::js_fs_glob_callback(arg(0), arg(1), arg(2)),
        ("fs", "fstat") => crate::fs::js_fs_fstat_callback(arg(0), arg(1), arg(2)),
        ("fs", "ftruncate") => crate::fs::js_fs_ftruncate_callback(arg(0), arg(1), arg(2)),
        ("fs", "fsync") => crate::fs::js_fs_fsync_callback(arg(0), arg(1)),
        ("fs", "fdatasync") => crate::fs::js_fs_fdatasync_callback(arg(0), arg(1)),
        ("fs", "fchmod") => crate::fs::js_fs_fchmod_callback(arg(0), arg(1), arg(2)),
        ("fs", "fchown") => crate::fs::js_fs_fchown_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "utimes") => crate::fs::js_fs_utimes_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "lutimes") => crate::fs::js_fs_lutimes_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "futimes") => crate::fs::js_fs_futimes_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "rename") => crate::fs::js_fs_rename_callback(arg(0), arg(1), arg(2)),
        ("fs", "copyFile") => crate::fs::js_fs_copy_file_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "isDirectory") => bool_to_f64(crate::fs::js_fs_is_directory(arg(0))),

        // ── os module (no args, return string or f64) ──
        ("os", "tmpdir") => str_to_f64(crate::os::js_os_tmpdir()),
        ("os", "homedir") => str_to_f64(crate::os::js_os_homedir()),
        ("os", "platform") => str_to_f64(crate::os::js_os_platform()),
        ("os", "arch") => str_to_f64(crate::os::js_os_arch()),
        ("os", "hostname") => str_to_f64(crate::os::js_os_hostname()),
        ("os", "type") => str_to_f64(crate::os::js_os_type()),
        ("os", "release") => str_to_f64(crate::os::js_os_release()),
        ("os", "eol") => str_to_f64(crate::os::js_os_eol()),
        ("os", "devNull") => str_to_f64(crate::os::js_os_dev_null()),
        ("os", "totalmem") => crate::os::js_os_totalmem(),
        ("os", "freemem") => crate::os::js_os_freemem(),
        ("os", "uptime") => crate::os::js_os_uptime(),
        ("os", "availableParallelism") => crate::os::js_os_available_parallelism(),
        ("os", "endianness") => str_to_f64(crate::os::js_os_endianness()),
        ("os", "machine") => str_to_f64(crate::os::js_os_machine()),
        ("os", "loadavg") => {
            f64::from_bits(JSValue::pointer(crate::os::js_os_loadavg() as *const u8).bits())
        }
        ("os", "version") => str_to_f64(crate::os::js_os_version()),
        ("os", "cpus") => {
            f64::from_bits(JSValue::pointer(crate::os::js_os_cpus() as *const u8).bits())
        }
        ("os", "networkInterfaces") => f64::from_bits(
            JSValue::pointer(crate::os::js_os_network_interfaces() as *const u8).bits(),
        ),
        ("os", "userInfo") => {
            // #3004 — honor a runtime `options.encoding === "buffer"` value
            // (variable / function-return / computed-key options object).
            let opts_bits = arg(0).to_bits() as i64;
            f64::from_bits(
                JSValue::pointer(crate::os::js_os_user_info_options(opts_bits) as *const u8).bits(),
            )
        }
        ("os", "getPriority") => crate::os::js_os_get_priority(arg(0)),
        ("os", "setPriority") => crate::os::js_os_set_priority(arg(0), arg(1)),

        // ── path module (args are NaN-boxed strings → extract raw StringHeader ptr) ──
        ("path", "dirname") => str_to_f64(crate::path::js_path_dirname(require_path_str_ptr(0))),
        ("path", "basename") => path_basename_value(false),
        ("path", "extname") => str_to_f64(crate::path::js_path_extname(require_path_str_ptr(0))),
        ("path", "normalize") => {
            str_to_f64(crate::path::js_path_normalize(require_path_str_ptr(0)))
        }
        ("path", "resolve") => path_resolve_value(false),
        ("path", "join") => path_join_value(false),
        ("path", "relative") => str_to_f64(crate::path::js_path_relative(
            require_path_str_ptr(0),
            require_path_str_ptr(1),
        )),
        ("path", "isAbsolute") => {
            bool_to_f64(crate::path::js_path_is_absolute(require_path_str_ptr(0)))
        }
        ("path", "toNamespacedPath") => crate::path::js_path_to_namespaced_path_value(arg(0)),
        ("path", "_makeLong") => crate::path::js_path_to_namespaced_path_value(arg(0)),
        ("path", "matchesGlob") => bool_to_f64(crate::path::js_path_matches_glob(
            require_path_str_ptr(0),
            require_path_str_ptr(1),
        )),
        ("path", "parse") => f64::from_bits(
            JSValue::pointer(crate::path::js_path_parse(require_path_str_ptr(0)) as *const u8)
                .bits(),
        ),
        ("path", "format") => str_to_f64(crate::path::js_path_format(arg(0))),

        // #1740: dynamic sub-namespace method dispatch — `path[k].method(...)`
        // where `k` resolves to "win32"/"posix" at runtime. `path[k].sep`
        // (property reads) already worked, but method calls landed here with
        // module_name "path.win32" / "path.posix" and no matching arm, so they
        // returned undefined. win32 routes to the `js_path_win32_*` family;
        // posix routes to the base `js_path_*` family (POSIX `/` semantics),
        // mirroring how the static `path.win32.X()` / `path.posix.X()` forms
        // lower in codegen.
        ("path.win32", "dirname") => {
            str_to_f64(crate::path::js_path_win32_dirname(require_path_str_ptr(0)))
        }
        ("path.win32", "basename") => path_basename_value(true),
        ("path.win32", "extname") => {
            str_to_f64(crate::path::js_path_win32_extname(require_path_str_ptr(0)))
        }
        ("path.win32", "normalize") => str_to_f64(crate::path::js_path_win32_normalize(
            require_path_str_ptr(0),
        )),
        ("path.win32", "resolve") => path_resolve_value(true),
        ("path.win32", "join") => path_join_value(true),
        ("path.win32", "relative") => str_to_f64(crate::path::js_path_win32_relative(
            require_path_str_ptr(0),
            require_path_str_ptr(1),
        )),
        ("path.win32", "toNamespacedPath") => {
            crate::path::js_path_win32_to_namespaced_path_value(arg(0))
        }
        ("path.win32", "_makeLong") => crate::path::js_path_win32_to_namespaced_path_value(arg(0)),
        ("path.win32", "isAbsolute") => bool_to_f64(crate::path::js_path_win32_is_absolute(
            require_path_str_ptr(0),
        )),
        ("path.win32", "matchesGlob") => bool_to_f64(crate::path::js_path_win32_matches_glob(
            require_path_str_ptr(0),
            require_path_str_ptr(1),
        )),
        ("path.win32", "parse") => {
            ptr_to_f64(crate::path::js_path_win32_parse(require_path_str_ptr(0)) as *const u8)
        }
        ("path.win32", "format") => str_to_f64(crate::path::js_path_win32_format(arg(0))),

        ("path.posix", "dirname") => {
            str_to_f64(crate::path::js_path_dirname(require_path_str_ptr(0)))
        }
        ("path.posix", "basename") => path_basename_value(false),
        ("path.posix", "extname") => {
            str_to_f64(crate::path::js_path_extname(require_path_str_ptr(0)))
        }
        ("path.posix", "normalize") => {
            str_to_f64(crate::path::js_path_normalize(require_path_str_ptr(0)))
        }
        ("path.posix", "resolve") => path_resolve_value(false),
        ("path.posix", "join") => path_join_value(false),
        ("path.posix", "relative") => str_to_f64(crate::path::js_path_relative(
            require_path_str_ptr(0),
            require_path_str_ptr(1),
        )),
        ("path.posix", "toNamespacedPath") => crate::path::js_path_to_namespaced_path_value(arg(0)),
        ("path.posix", "_makeLong") => crate::path::js_path_to_namespaced_path_value(arg(0)),
        ("path.posix", "isAbsolute") => {
            bool_to_f64(crate::path::js_path_is_absolute(require_path_str_ptr(0)))
        }
        ("path.posix", "matchesGlob") => bool_to_f64(crate::path::js_path_matches_glob(
            require_path_str_ptr(0),
            require_path_str_ptr(1),
        )),
        ("path.posix", "parse") => {
            ptr_to_f64(crate::path::js_path_parse(require_path_str_ptr(0)) as *const u8)
        }
        ("path.posix", "format") => str_to_f64(crate::path::js_path_format(arg(0))),

        // ── util module ──
        ("util", "format") => crate::builtins::js_util_format(pack_args()),
        ("util", "formatWithOptions") => {
            let effective = args_len.saturating_sub(1);
            let mut arr = crate::array::js_array_alloc(effective as u32);
            for i in 1..args_len {
                arr = crate::array::js_array_push_f64(arr, arg(i));
            }
            crate::builtins::js_util_format_with_options(arg(0), arr)
        }
        ("util", "inspect") => crate::builtins::js_util_inspect(arg(0), arg(1)),
        ("util", "convertProcessSignalToExitCode") => {
            crate::os::js_util_convert_process_signal_to_exit_code(arg(0))
        }
        // #2514: libuv-style errno → name/message/map helpers.
        ("util", "getSystemErrorName") => crate::util_syserr::js_util_get_system_error_name(arg(0)),
        ("util", "getSystemErrorMessage") => {
            crate::util_syserr::js_util_get_system_error_message(arg(0))
        }
        ("util", "getSystemErrorMap") => crate::util_syserr::js_util_get_system_error_map(),
        ("util", "aborted") => crate::util_abort::js_util_aborted(arg(0), arg(1)),
        ("util", "transferableAbortController") => {
            crate::util_abort::js_util_transferable_abort_controller()
        }
        ("util", "transferableAbortSignal") => {
            crate::util_abort::js_util_transferable_abort_signal(arg(0))
        }
        ("util", "getCallSites") => crate::util_call_sites::js_util_get_call_sites(arg(0), arg(1)),
        // #2514: util.parseEnv(content) → object.
        ("util", "parseEnv") => crate::util_parse_env::js_util_parse_env(arg(0)),
        ("util", "debuglog") | ("util", "debug") => {
            crate::util_debuglog::js_util_debuglog(arg(0), arg(1))
        }
        ("util", "inherits") => crate::util_inherits::js_util_inherits(arg(0), arg(1)),
        ("util", "_extend") => crate::util_mime::js_util_extend(arg(0), arg(1)),
        ("util", "_errnoException") => {
            crate::util_mime::js_util_errno_exception(arg(0), arg(1), arg(2))
        }
        ("util", "_exceptionWithHostPort") => crate::util_mime::js_util_exception_with_host_port(
            arg(0),
            arg(1),
            arg(2),
            arg(3),
            arg(4),
        ),
        ("util", "MIMEType") => crate::util_mime::js_util_mime_type_new(arg(0)),
        ("util", "MIMEParams") => crate::util_mime::js_util_mime_params_new(),
        ("util", "diff") => crate::util_diff::js_util_diff(arg(0), arg(1)),
        ("util", "isArray") => crate::array::js_array_is_array(arg(0)),
        ("util", "isDeepStrictEqual") => {
            crate::builtins::js_util_is_deep_strict_equal(arg(0), arg(1))
        }
        ("util", "stripVTControlCharacters") => {
            crate::builtins::js_util_strip_vt_control_characters(arg(0))
        }
        ("util", "styleText") => crate::util_style_text::js_util_style_text(arg(0), arg(1), arg(2)),
        // #2514: util.toUSVString(value) → string with lone surrogates → U+FFFD.
        ("util", "toUSVString") => crate::util_usv::js_util_to_usv_string(arg(0)),
        ("util", "setTraceSigInt") => crate::util_settracesigint::js_util_set_trace_sig_int(arg(0)),
        ("util", "promisify") => crate::util_promisify::js_util_promisify(arg(0)),
        ("util", "callbackify") => crate::util_promisify::js_util_callbackify(arg(0)),
        ("util", "deprecate") => crate::util_promisify::js_util_deprecate(arg(0), arg(1), arg(2)),
        ("util", "parseArgs") => crate::util_parse_args::js_util_parse_args(arg(0)),

        ("util", "isPromise") => {
            let v = JSValue::from_bits(arg(0).to_bits());
            bool_tag(
                v.is_pointer()
                    && crate::promise::js_is_promise(
                        v.as_pointer::<crate::promise::Promise>() as *mut crate::promise::Promise
                    ) != 0,
            )
        }
        ("util", "isArrayBuffer") => bool_tag(crate::buffer::is_array_buffer(ptr_addr(arg(0)))),
        ("util", "isSharedArrayBuffer") => {
            bool_tag(crate::buffer::is_shared_array_buffer(ptr_addr(arg(0))))
        }
        ("util", "isAnyArrayBuffer") => {
            bool_tag(crate::buffer::is_any_array_buffer(ptr_addr(arg(0))))
        }
        ("util", "isArrayBufferView") => {
            let addr = ptr_addr(arg(0));
            bool_tag(
                crate::buffer::is_uint8array_buffer(addr)
                    || crate::buffer::is_data_view(addr)
                    || typed_kind(arg(0)).is_some(),
            )
        }
        ("util", "isTypedArray") => bool_tag(typed_kind(arg(0)).is_some()),
        ("util", "isUint8Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_UINT8))
        }
        ("util", "isInt8Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_INT8))
        }
        ("util", "isInt16Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_INT16))
        }
        ("util", "isUint16Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_UINT16))
        }
        ("util", "isInt32Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_INT32))
        }
        ("util", "isUint32Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_UINT32))
        }
        ("util", "isFloat32Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_FLOAT32))
        }
        ("util", "isFloat64Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_FLOAT64))
        }
        ("util", "isUint8ClampedArray") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_UINT8_CLAMPED))
        }
        ("util", "isMap") => bool_tag(crate::map::is_registered_map(ptr_addr(arg(0)))),
        ("util", "isSet") => bool_tag(crate::set::is_registered_set(ptr_addr(arg(0)))),

        // ── util.types namespace ──
        ("util.types", "isArgumentsObject") => {
            crate::object::js_util_types_is_arguments_object(arg(0))
        }
        ("util.types", "isPromise") => crate::object::js_util_types_is_promise(arg(0)),
        ("util.types", "isBigIntObject") => crate::object::js_util_types_is_big_int_object(arg(0)),
        ("util.types", "isArrayBuffer") => crate::object::js_util_types_is_array_buffer(arg(0)),
        ("util.types", "isSharedArrayBuffer") => {
            crate::object::js_util_types_is_shared_array_buffer(arg(0))
        }
        ("util.types", "isAnyArrayBuffer") => {
            crate::object::js_util_types_is_any_array_buffer(arg(0))
        }
        ("util.types", "isArrayBufferView") => {
            crate::object::js_util_types_is_array_buffer_view(arg(0))
        }
        ("util.types", "isDataView") => crate::object::js_util_types_is_data_view(arg(0)),
        ("util.types", "isTypedArray") => crate::object::js_util_types_is_typed_array(arg(0)),
        ("util.types", "isUint8Array") => crate::object::js_util_types_is_uint8_array(arg(0)),
        ("util.types", "isInt8Array") => crate::object::js_util_types_is_int8_array(arg(0)),
        ("util.types", "isInt16Array") => crate::object::js_util_types_is_int16_array(arg(0)),
        ("util.types", "isUint16Array") => crate::object::js_util_types_is_uint16_array(arg(0)),
        ("util.types", "isInt32Array") => crate::object::js_util_types_is_int32_array(arg(0)),
        ("util.types", "isUint32Array") => crate::object::js_util_types_is_uint32_array(arg(0)),
        ("util.types", "isFloat16Array") => crate::object::js_util_types_is_float16_array(arg(0)),
        ("util.types", "isFloat32Array") => crate::object::js_util_types_is_float32_array(arg(0)),
        ("util.types", "isFloat64Array") => crate::object::js_util_types_is_float64_array(arg(0)),
        ("util.types", "isUint8ClampedArray") => {
            crate::object::js_util_types_is_uint8_clamped_array(arg(0))
        }
        ("util.types", "isBigInt64Array") => {
            crate::object::js_util_types_is_big_int64_array(arg(0))
        }
        ("util.types", "isBigUint64Array") => {
            crate::object::js_util_types_is_big_uint64_array(arg(0))
        }
        ("util.types", "isMap") => crate::object::js_util_types_is_map(arg(0)),
        ("util.types", "isMapIterator") => crate::object::js_util_types_is_map_iterator(arg(0)),
        ("util.types", "isProxy") => crate::object::js_util_types_is_proxy(arg(0)),
        ("util.types", "isExternal") => crate::object::js_util_types_is_external(arg(0)),
        ("util.types", "isModuleNamespaceObject") => {
            crate::object::js_util_types_is_module_namespace_object(arg(0))
        }
        ("util.types", "isSet") => crate::object::js_util_types_is_set(arg(0)),
        ("util.types", "isSetIterator") => crate::object::js_util_types_is_set_iterator(arg(0)),
        ("util.types", "isWeakMap") => crate::object::js_util_types_is_weak_map(arg(0)),
        ("util.types", "isWeakSet") => crate::object::js_util_types_is_weak_set(arg(0)),
        ("util.types", "isDate") => crate::object::js_util_types_is_date(arg(0)),
        ("util.types", "isRegExp") => crate::object::js_util_types_is_reg_exp(arg(0)),
        ("util.types", "isAsyncFunction") => crate::object::js_util_types_is_async_function(arg(0)),
        ("util.types", "isGeneratorFunction") => {
            crate::object::js_util_types_is_generator_function(arg(0))
        }
        ("util.types", "isGeneratorObject") => {
            crate::object::js_util_types_is_generator_object(arg(0))
        }
        ("util.types", "isNativeError") => crate::object::js_util_types_is_native_error(arg(0)),
        ("util.types", "isKeyObject") => crate::object::js_util_types_is_key_object(arg(0)),
        ("util.types", "isCryptoKey") => crate::object::js_util_types_is_crypto_key(arg(0)),
        ("util.types", "isNumberObject") => crate::object::js_util_types_is_number_object(arg(0)),
        ("util.types", "isStringObject") => crate::object::js_util_types_is_string_object(arg(0)),
        ("util.types", "isBooleanObject") => crate::object::js_util_types_is_boolean_object(arg(0)),
        ("util.types", "isSymbolObject") => crate::object::js_util_types_is_symbol_object(arg(0)),
        ("util.types", "isBoxedPrimitive") => {
            crate::object::js_util_types_is_boxed_primitive(arg(0))
        }

        // ── node:util/types direct module ──
        ("util/types", "isArgumentsObject") => {
            crate::object::js_util_types_is_arguments_object(arg(0))
        }
        ("util/types", "isPromise") => crate::object::js_util_types_is_promise(arg(0)),
        ("util/types", "isBigIntObject") => crate::object::js_util_types_is_big_int_object(arg(0)),
        ("util/types", "isArrayBuffer") => crate::object::js_util_types_is_array_buffer(arg(0)),
        ("util/types", "isSharedArrayBuffer") => {
            crate::object::js_util_types_is_shared_array_buffer(arg(0))
        }
        ("util/types", "isAnyArrayBuffer") => {
            crate::object::js_util_types_is_any_array_buffer(arg(0))
        }
        ("util/types", "isArrayBufferView") => {
            crate::object::js_util_types_is_array_buffer_view(arg(0))
        }
        ("util/types", "isDataView") => crate::object::js_util_types_is_data_view(arg(0)),
        ("util/types", "isTypedArray") => crate::object::js_util_types_is_typed_array(arg(0)),
        ("util/types", "isUint8Array") => crate::object::js_util_types_is_uint8_array(arg(0)),
        ("util/types", "isInt8Array") => crate::object::js_util_types_is_int8_array(arg(0)),
        ("util/types", "isInt16Array") => crate::object::js_util_types_is_int16_array(arg(0)),
        ("util/types", "isUint16Array") => crate::object::js_util_types_is_uint16_array(arg(0)),
        ("util/types", "isInt32Array") => crate::object::js_util_types_is_int32_array(arg(0)),
        ("util/types", "isUint32Array") => crate::object::js_util_types_is_uint32_array(arg(0)),
        ("util/types", "isFloat16Array") => crate::object::js_util_types_is_float16_array(arg(0)),
        ("util/types", "isFloat32Array") => crate::object::js_util_types_is_float32_array(arg(0)),
        ("util/types", "isFloat64Array") => crate::object::js_util_types_is_float64_array(arg(0)),
        ("util/types", "isUint8ClampedArray") => {
            crate::object::js_util_types_is_uint8_clamped_array(arg(0))
        }
        ("util/types", "isBigInt64Array") => {
            crate::object::js_util_types_is_big_int64_array(arg(0))
        }
        ("util/types", "isBigUint64Array") => {
            crate::object::js_util_types_is_big_uint64_array(arg(0))
        }
        ("util/types", "isMap") => crate::object::js_util_types_is_map(arg(0)),
        ("util/types", "isMapIterator") => crate::object::js_util_types_is_map_iterator(arg(0)),
        ("util/types", "isProxy") => crate::object::js_util_types_is_proxy(arg(0)),
        ("util/types", "isExternal") => crate::object::js_util_types_is_external(arg(0)),
        ("util/types", "isModuleNamespaceObject") => {
            crate::object::js_util_types_is_module_namespace_object(arg(0))
        }
        ("util/types", "isSet") => crate::object::js_util_types_is_set(arg(0)),
        ("util/types", "isSetIterator") => crate::object::js_util_types_is_set_iterator(arg(0)),
        ("util/types", "isWeakMap") => crate::object::js_util_types_is_weak_map(arg(0)),
        ("util/types", "isWeakSet") => crate::object::js_util_types_is_weak_set(arg(0)),
        ("util/types", "isDate") => crate::object::js_util_types_is_date(arg(0)),
        ("util/types", "isRegExp") => crate::object::js_util_types_is_reg_exp(arg(0)),
        ("util/types", "isAsyncFunction") => crate::object::js_util_types_is_async_function(arg(0)),
        ("util/types", "isGeneratorFunction") => {
            crate::object::js_util_types_is_generator_function(arg(0))
        }
        ("util/types", "isGeneratorObject") => {
            crate::object::js_util_types_is_generator_object(arg(0))
        }
        ("util/types", "isNativeError") => crate::object::js_util_types_is_native_error(arg(0)),
        ("util/types", "isKeyObject") => crate::object::js_util_types_is_key_object(arg(0)),
        ("util/types", "isCryptoKey") => crate::object::js_util_types_is_crypto_key(arg(0)),
        ("util/types", "isNumberObject") => crate::object::js_util_types_is_number_object(arg(0)),
        ("util/types", "isStringObject") => crate::object::js_util_types_is_string_object(arg(0)),
        ("util/types", "isBooleanObject") => crate::object::js_util_types_is_boolean_object(arg(0)),
        ("util/types", "isSymbolObject") => crate::object::js_util_types_is_symbol_object(arg(0)),
        ("util/types", "isBoxedPrimitive") => {
            crate::object::js_util_types_is_boxed_primitive(arg(0))
        }
        // ── url module (module-level functions return NaN-boxed JS values) ──
        ("url", "fileURLToPath") => crate::url::js_url_file_url_to_path(arg(0), arg(1)),
        ("url", "fileURLToPathBuffer") => {
            crate::url::js_url_file_url_to_path_buffer(arg(0), arg(1))
        }
        ("url", "pathToFileURL") => crate::url::js_url_path_to_file_url(arg(0), arg(1)),
        ("url", "domainToASCII") => crate::url::js_url_domain_to_ascii(arg(0)),
        ("url", "domainToUnicode") => crate::url::js_url_domain_to_unicode(arg(0)),
        ("url", "urlToHttpOptions") => crate::url::js_url_to_http_options(arg(0)),
        ("url", "Url") => crate::url::js_url_legacy_url_new(),
        ("url", "format") => crate::url::js_url_format(arg(0), arg(1)),
        ("url", "parse") => crate::url::js_url_legacy_parse(arg(0), arg(1), arg(2)),
        ("url", "resolve") => crate::url::js_url_legacy_resolve(arg(0), arg(1)),
        ("url", "resolveObject") => crate::url::js_url_legacy_resolve_object(arg(0), arg(1)),

        // ── punycode module (deprecated, #2513) ──
        ("punycode", "decode") => crate::punycode::js_punycode_decode(arg(0)),
        ("punycode", "encode") => crate::punycode::js_punycode_encode(arg(0)),
        ("punycode", "toASCII") => crate::punycode::js_punycode_to_ascii(arg(0)),
        ("punycode", "toUnicode") => crate::punycode::js_punycode_to_unicode(arg(0)),
        // ── punycode.ucs2 sub-namespace (#2607) ──
        ("punycode.ucs2", "decode") => crate::punycode::js_punycode_ucs2_decode(arg(0)),
        ("punycode.ucs2", "encode") => crate::punycode::js_punycode_ucs2_encode(arg(0)),

        // ── dgram namespace (`node:dgram` / `dgram`) ──
        ("dgram", "createSocket") | ("dgram", "Socket") => {
            crate::dgram::js_dgram_create_socket(pack_args())
        }

        // ── console module namespace (`node:console` / `console`) ──
        ("console", "Console") => crate::builtins::js_console_new2(arg(0), arg(1)),
        ("console", "log") | ("console", "info") | ("console", "debug") | ("console", "dirxml") => {
            crate::builtins::js_console_log_spread(pack_args());
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "error") => {
            crate::builtins::js_console_error_spread(pack_args());
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "warn") => {
            crate::builtins::js_console_warn_spread(pack_args());
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "assert") => {
            crate::builtins::js_console_assert_spread(arg(0), pack_args_from(1) as i64);
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "dir") => {
            crate::builtins::js_console_log_dynamic(arg(0));
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "trace") => {
            crate::builtins::js_console_trace_spread(pack_args());
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "table") => {
            if args_len > 1 {
                crate::builtins::js_console_table_with_properties(arg(0), arg(1));
            } else {
                crate::builtins::js_console_table(arg(0));
            }
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "clear") => {
            crate::builtins::js_console_clear();
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "count") => {
            crate::builtins::js_console_count_value(arg(0));
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "countReset") => {
            crate::builtins::js_console_count_reset_value(arg(0));
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "time") => {
            crate::builtins::js_console_time_value(arg(0));
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "timeEnd") => {
            crate::builtins::js_console_time_end_value(arg(0));
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "timeLog") => {
            if args_len > 1 {
                crate::builtins::js_console_time_log_spread(arg(0), pack_args_from(1));
            } else {
                crate::builtins::js_console_time_log_value(arg(0));
            }
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "group") | ("console", "groupCollapsed") => {
            if args_len > 0 {
                crate::builtins::js_console_log_dynamic(arg(0));
            }
            crate::builtins::js_console_group_begin();
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "groupEnd") => {
            crate::builtins::js_console_group_end();
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "profile") | ("console", "profileEnd") | ("console", "timeStamp") => {
            f64::from_bits(JSValue::undefined().bits())
        }
        ("stream", "compose") => crate::node_stream::js_node_stream_compose_args(pack_args()),
        ("stream", "duplexPair") => crate::node_stream::js_node_stream_duplex_pair(arg(0)),
        ("stream", "pipeline") => crate::node_stream::js_node_stream_pipeline(pack_args()),
        // Classic stream constructors are legacy-callable in Node:
        // `PassThrough()` behaves like `new PassThrough()`.
        ("stream", "Readable") => crate::node_stream::js_node_stream_readable_new(arg(0)),
        ("stream", "Writable") => crate::node_stream::js_node_stream_writable_new(arg(0)),
        ("stream", "Duplex") => crate::node_stream::js_node_stream_duplex_new(arg(0)),
        ("stream", "Transform") => crate::node_stream::js_node_stream_transform_new(arg(0)),
        ("stream", "PassThrough") => crate::node_stream::js_node_stream_passthrough_new(arg(0)),
        ("readline", "clearLine") => {
            crate::readline_helpers::js_readline_clear_line_args(pack_args())
        }
        ("readline", "clearScreenDown") => {
            crate::readline_helpers::js_readline_clear_screen_down_args(pack_args())
        }
        ("readline", "cursorTo") => {
            crate::readline_helpers::js_readline_cursor_to_args(pack_args())
        }
        ("readline", "moveCursor") => {
            crate::readline_helpers::js_readline_move_cursor_args(pack_args())
        }
        ("readline", "emitKeypressEvents") => {
            crate::readline_helpers::js_readline_emit_keypress_events_args(pack_args())
        }

        // ── node:dns / node:dns/promises configuration ──
        ("dns", "getServers") => crate::dns::dns_get_servers_value(),
        ("dns", "setServers") => crate::dns::dns_set_servers_value(arg(0)),
        ("dns/promises", "getServers") => crate::dns::dns_promises_get_servers_value(),
        ("dns/promises", "setServers") => crate::dns::dns_promises_set_servers_value(arg(0)),
        ("dns" | "dns/promises", "getDefaultResultOrder") => {
            crate::dns::dns_get_default_result_order_value()
        }
        ("dns" | "dns/promises", "setDefaultResultOrder") => {
            crate::dns::dns_set_default_result_order_value(arg(0))
        }

        // #2130: captured-then-called child_process methods (`const spawn =
        // require('child_process').spawn; spawn(...)`, Node's canonical test
        // idiom). The bound-method closure produced by `cp.spawn` (and the
        // other entries allowlisted in `is_native_module_callable_export`)
        // funnels back here when invoked. The method-call form
        // (`cp.spawn(...)`) is lowered to the same FFIs through dedicated
        // codegen arms (`expr/child_proc.rs`); this arm mirrors them for the
        // value-call form. `cmd` / `file` / `module` strings come in NaN-boxed
        // (SSO-safe via `js_string_materialize_to_heap`); `args` is the array
        // pointer (or null); `opts` is the options-object pointer (or 0 →
        // undefined inside the impls).
        ("child_process", "spawn") => {
            let cmd = crate::string::js_string_materialize_to_heap(arg(0)) as i64;
            let args_p = optional_ptr_addr(arg(1)) as i64;
            let opts_p = optional_ptr_addr(arg(2)) as i64;
            crate::child_process::reactor::js_child_process_spawn_streams(cmd, args_p, opts_p)
        }
        ("child_process", "spawnSync") => {
            let cmd = crate::string::js_string_materialize_to_heap(arg(0));
            let args_p = optional_ptr_addr(arg(1)) as *const crate::array::ArrayHeader;
            let opts_p = optional_ptr_addr(arg(2)) as *const ObjectHeader;
            let result = crate::child_process::js_child_process_spawn_sync(cmd, args_p, opts_p);
            ptr_to_f64(result as *const u8)
        }
        ("child_process", "execSync") => {
            let cmd = crate::string::js_string_materialize_to_heap(arg(0));
            let opts_p = optional_ptr_addr(arg(1)) as *const ObjectHeader;
            crate::child_process::js_child_process_exec_sync(cmd, opts_p)
        }
        ("child_process", "exec") => {
            let cmd = crate::string::js_string_materialize_to_heap(arg(0));
            crate::child_process::js_child_process_exec(cmd, arg(1), arg(2))
        }
        ("child_process", "execFile") => {
            let file = crate::string::js_string_materialize_to_heap(arg(0)) as i64;
            crate::child_process::js_child_process_exec_file(file, arg(1), arg(2), arg(3))
        }
        ("child_process", "execFileSync") => {
            let file = crate::string::js_string_materialize_to_heap(arg(0)) as i64;
            crate::child_process::js_child_process_exec_file_sync(file, arg(1), arg(2))
        }
        ("child_process", "fork") => {
            let module = crate::string::js_string_materialize_to_heap(arg(0)) as i64;
            let args_p = optional_ptr_addr(arg(1)) as i64;
            let opts_p = optional_ptr_addr(arg(2)) as i64;
            crate::child_process::fork::js_child_process_fork(module, args_p, opts_p)
        }
        ("cluster", "setupPrimary") | ("cluster", "setupMaster") => {
            crate::cluster::js_cluster_setup_primary(arg(0))
        }
        ("cluster", "fork") => crate::cluster::js_cluster_fork(arg(0)),
        ("cluster", "disconnect") => crate::cluster::js_cluster_disconnect(arg(0)),
        ("cluster", "Worker") => f64::from_bits(JSValue::undefined().bits()),

        // #1577: captured-then-called crypto methods (`const f =
        // crypto.createHash; f(...)`). The impls live in perry-stdlib (which
        // depends on this crate), so route through the dispatcher stdlib
        // registers at startup via `js_set_native_crypto_dispatch`. Null when
        // stdlib isn't linked (e.g. runtime-only tests) → undefined. The
        // `randomFillSync` arm above is handled inline and never reaches here.
        ("crypto" | "crypto.webcrypto", _) => {
            let ptr =
                crate::value::JS_NATIVE_CRYPTO_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
            if ptr.is_null() {
                f64::from_bits(JSValue::undefined().bits())
            } else {
                let dispatch: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64 =
                    std::mem::transmute(ptr);
                dispatch(method_name.as_ptr(), method_name.len(), args_ptr, args_len)
            }
        }
        ("crypto.subtle", _) => {
            let ptr = crate::value::JS_NATIVE_WEBCRYPTO_DISPATCH
                .load(std::sync::atomic::Ordering::SeqCst);
            if ptr.is_null() {
                f64::from_bits(JSValue::undefined().bits())
            } else {
                let dispatch: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64 =
                    std::mem::transmute(ptr);
                dispatch(method_name.as_ptr(), method_name.len(), args_ptr, args_len)
            }
        }
        // Captured-then-called zlib methods (`const f = zlib.gzip; await f(buf)`,
        // `util.promisify(zlib.gzip)`). Mirrors the crypto arm above — the
        // impls live in perry-stdlib which depends on this crate, so route
        // through the dispatcher stdlib registers at startup via
        // `js_set_native_zlib_dispatch`. Null when stdlib isn't linked.
        ("zlib", _) => {
            let ptr =
                crate::value::JS_NATIVE_ZLIB_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
            if ptr.is_null() {
                f64::from_bits(JSValue::undefined().bits())
            } else {
                let dispatch: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64 =
                    std::mem::transmute(ptr);
                dispatch(method_name.as_ptr(), method_name.len(), args_ptr, args_len)
            }
        }
        (
            "querystring",
            "unescapeBuffer" | "unescape" | "escape" | "stringify" | "encode" | "parse" | "decode",
        ) => {
            let ptr = crate::value::JS_NATIVE_QUERYSTRING_DISPATCH
                .load(std::sync::atomic::Ordering::SeqCst);
            if ptr.is_null() {
                f64::from_bits(JSValue::undefined().bits())
            } else {
                let dispatch: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64 =
                    std::mem::transmute(ptr);
                dispatch(method_name.as_ptr(), method_name.len(), args_ptr, args_len)
            }
        }
        ("sqlite", _) => {
            let ptr =
                crate::value::JS_NATIVE_SQLITE_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
            if ptr.is_null() {
                f64::from_bits(JSValue::undefined().bits())
            } else {
                let dispatch: crate::value::JsNativeSqliteDispatchFn = std::mem::transmute(ptr);
                dispatch(
                    method_name.as_ptr(),
                    method_name.len(),
                    args_ptr,
                    args_len,
                    0,
                )
            }
        }
        ("domain", "Domain" | "createDomain" | "create") => {
            let ptr =
                crate::value::JS_NATIVE_DOMAIN_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
            if ptr.is_null() {
                f64::from_bits(JSValue::undefined().bits())
            } else {
                let dispatch: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64 =
                    std::mem::transmute(ptr);
                dispatch(method_name.as_ptr(), method_name.len(), args_ptr, args_len)
            }
        }
        ("crypto.Certificate", _) => {
            let qualified: &[u8] = match method_name {
                "verifySpkac" => b"Certificate.verifySpkac",
                "exportPublicKey" => b"Certificate.exportPublicKey",
                "exportChallenge" => b"Certificate.exportChallenge",
                _ => return f64::from_bits(JSValue::undefined().bits()),
            };
            let ptr =
                crate::value::JS_NATIVE_CRYPTO_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
            if ptr.is_null() {
                f64::from_bits(JSValue::undefined().bits())
            } else {
                let dispatch: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64 =
                    std::mem::transmute(ptr);
                dispatch(qualified.as_ptr(), qualified.len(), args_ptr, args_len)
            }
        }

        // #3142: `new v8.GCProfiler()` is the "v8.GCProfiler" namespace.
        // `start()` returns undefined; `stop()` returns the report object.
        ("v8.GCProfiler", "start") => f64::from_bits(JSValue::undefined().bits()),
        ("v8.GCProfiler", "stop") => crate::node_v8::js_v8_gc_profiler_report(),

        // #3680: `v8.Serializer` / `v8.DefaultSerializer` instance methods.
        // The registry id lives in field[1] of the namespace object; the
        // runtime re-derives it from the receiver value.
        ("v8.Serializer", m) | ("v8.DefaultSerializer", m) => {
            let recv = crate::value::js_nanbox_pointer(obj as i64);
            match m {
                "writeHeader" => crate::node_v8::v8_serializer_write_header(recv),
                "writeValue" => crate::node_v8::v8_serializer_write_value(recv, arg(0)),
                "writeUint32" => crate::node_v8::v8_serializer_write_uint32(recv, arg(0)),
                "writeUint64" => crate::node_v8::v8_serializer_write_uint64(recv, arg(0), arg(1)),
                "writeDouble" => crate::node_v8::v8_serializer_write_double(recv, arg(0)),
                "writeRawBytes" => crate::node_v8::v8_serializer_write_raw_bytes(recv, arg(0)),
                "releaseBuffer" => crate::node_v8::v8_serializer_release_buffer(recv),
                // `_setTreatArrayBufferViewsAsHostObjects` is a no-op for us
                // (our writer always treats them as host objects).
                _ => f64::from_bits(JSValue::undefined().bits()),
            }
        }

        // #3680: `v8.Deserializer` / `v8.DefaultDeserializer` instance methods.
        ("v8.Deserializer", m) | ("v8.DefaultDeserializer", m) => {
            let recv = crate::value::js_nanbox_pointer(obj as i64);
            match m {
                "readHeader" => crate::node_v8::v8_deserializer_read_header(recv),
                "readValue" => crate::node_v8::v8_deserializer_read_value(recv),
                "readUint32" => crate::node_v8::v8_deserializer_read_uint32(recv),
                "readUint64" => crate::node_v8::v8_deserializer_read_uint64(recv),
                "readDouble" => crate::node_v8::v8_deserializer_read_double(recv),
                "readRawBytes" => crate::node_v8::v8_deserializer_read_raw_bytes(recv, arg(0)),
                _ => f64::from_bits(JSValue::undefined().bits()),
            }
        }

        // #3679: `v8.startupSnapshot` namespace methods. Perry never builds a
        // startup snapshot, so `isBuildingSnapshot()` is `0` and the
        // serialize/deserialize-callback registrars throw like Node does when
        // called outside a snapshot-building context.
        ("v8.startupSnapshot", m) => match m {
            "isBuildingSnapshot" => crate::node_v8::js_v8_is_building_snapshot(),
            "addSerializeCallback" | "addDeserializeCallback" | "setDeserializeMainFunction" => {
                crate::fs::validate::throw_type_error_with_code(
                    "Operation not allowed when not building startup snapshot.",
                    "ERR_NOT_BUILDING_SNAPSHOT",
                )
            }
            _ => f64::from_bits(JSValue::undefined().bits()),
        },

        // #3679: `v8.promiseHooks` namespace. Hook registrars return a stop
        // function (Node returns a callable that removes the hook); we hand
        // back a no-op callable so `const stop = onInit(fn); stop()` works.
        ("v8.promiseHooks", m) => match m {
            "onInit" | "onBefore" | "onAfter" | "onSettled" | "createHook" => {
                let c = crate::closure::js_closure_alloc_singleton(
                    crate::node_v8::js_v8_noop_undefined as *const u8,
                );
                crate::value::js_nanbox_pointer(c as i64)
            }
            _ => f64::from_bits(JSValue::undefined().bits()),
        },

        // #2533: captured / aliased server factories
        // (`const createServer = options.createServer || createServerHTTP;
        // createServer(opts, handler)` — `@hono/node-server`'s `serve()`). The
        // method-call form (`http.createServer(...)`) already lowers through a
        // dedicated codegen NATIVE_MODULE_TABLE path; the value-read form yields
        // a bound-method closure (see `is_native_module_callable_export`) that
        // lands here when invoked. The impls live in perry-ext-http-server, so
        // route through the dispatcher perry-stdlib registers at startup under
        // `external-http-server-pump` (enabled whenever http/https/http2 is
        // imported). Null when the http ext crate isn't linked → undefined. The
        // dispatcher takes the module name so one callback serves all three.
        ("http", "createServer")
        | ("http", "Server")
        | ("https", "createServer")
        | ("https", "Server")
        | ("http2", "createServer")
        | ("http2", "createSecureServer")
        | ("http2", "Server") => {
            let ptr =
                crate::value::JS_NATIVE_HTTP_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
            if ptr.is_null() {
                f64::from_bits(JSValue::undefined().bits())
            } else {
                let dispatch: unsafe extern "C" fn(
                    *const u8,
                    usize,
                    *const u8,
                    usize,
                    *const f64,
                    usize,
                ) -> f64 = std::mem::transmute(ptr);
                dispatch(
                    module_name.as_ptr(),
                    module_name.len(),
                    method_name.as_ptr(),
                    method_name.len(),
                    args_ptr,
                    args_len,
                )
            }
        }

        _ => {
            // Method not found on native module — return undefined
            f64::from_bits(JSValue::undefined().bits())
        }
    }
}
