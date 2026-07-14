//! Core `process.*` runtime surface that isn't part of `node:module`,
//! `process.report`, `process.permission`, or the finalization registry:
//! exit/abort, uncaught-exception capture callbacks, `process.env`
//! get/set/remove, CPU/memory/resource usage, `emitWarning`, `exitCode`,
//! `title`, `umask`, `chdir`, `execve`, `loadEnvFile`, the ref/unref timer
//! shims, and the small stub entry points. Split out of the `process` trunk.
//! Pure code move — no behavior change.

use super::*;
use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64, ClosureHeader,
};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};

static PROCESS_UNCAUGHT_CAPTURE_CALLBACK_SET: AtomicBool = AtomicBool::new(false);

fn timer_handle_id(value: f64) -> Option<i64> {
    let js_value = JSValue::from_bits(value.to_bits());
    if !js_value.is_pointer() {
        return None;
    }
    let id = (value.to_bits() & crate::value::POINTER_MASK) as i64;
    crate::timer::is_known_timer_id(id).then_some(id)
}

#[no_mangle]
pub extern "C" fn js_process_ref(value: f64) -> f64 {
    if let Some(id) = timer_handle_id(value) {
        crate::timer::js_timer_ref(id);
    }
    undefined_value()
}

#[no_mangle]
pub extern "C" fn js_process_unref(value: f64) -> f64 {
    if let Some(id) = timer_handle_id(value) {
        crate::timer::js_timer_unref(id);
    }
    undefined_value()
}

fn throw_uncaught_capture_callback_type_error(value: f64) -> ! {
    let message = format!(
        "The \"fn\" argument must be of type function or null. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

#[no_mangle]
pub extern "C" fn js_process_has_uncaught_exception_capture_callback() -> f64 {
    bool_value(PROCESS_UNCAUGHT_CAPTURE_CALLBACK_SET.load(Ordering::SeqCst))
}

#[no_mangle]
pub extern "C" fn js_process_set_uncaught_exception_capture_callback(callback: f64) -> f64 {
    let jv = JSValue::from_bits(callback.to_bits());
    if jv.is_null() {
        PROCESS_UNCAUGHT_CAPTURE_CALLBACK_SET.store(false, Ordering::SeqCst);
        return undefined_value();
    }
    if !is_function_value(callback) {
        throw_uncaught_capture_callback_type_error(callback);
    }
    PROCESS_UNCAUGHT_CAPTURE_CALLBACK_SET.store(true, Ordering::SeqCst);
    undefined_value()
}

#[no_mangle]
pub extern "C" fn js_process_add_uncaught_exception_capture_callback(callback: f64) -> f64 {
    if !is_function_value(callback) {
        throw_uncaught_capture_callback_type_error(callback);
    }
    undefined_value()
}

/// Exit the process with the given exit code.
/// process.exit(code?: number | string | null) -> never
/// Uses libc::_exit() to bypass cleanup handlers that can cause SIGILL
/// during async event loop drain and V8 isolate destruction.
#[no_mangle]
pub extern "C" fn js_process_exit(code: f64) {
    // #3041 — match Node's `parseAndValidateExitCode`:
    //   * `undefined` / `null`  → exit with the prior `process.exitCode`
    //     (0 by default here, since the validated path never stored one).
    //   * number                → must be a finite integer, else
    //     RangeError [ERR_OUT_OF_RANGE] ("It must be an integer").
    //   * string                → coerced with `Number()`; empty string or
    //     a non-numeric string (`Number()` → NaN) throws
    //     TypeError [ERR_INVALID_ARG_TYPE], otherwise it is validated as a
    //     number (so `"2.5"` → RangeError, `"2"` → exit 2).
    //   * anything else (boolean/object/array) → TypeError.
    let exit_code = validate_exit_code(code).unwrap_or_default();
    js_process_run_finalization_exit();
    crate::gc::js_gc_release_current_thread_collection_side_allocations();
    terminate_without_atexit(exit_code)
}

/// Terminate without running process-wide cleanup after thread-local GC state
/// has been torn down.
fn terminate_without_atexit(exit_code: i32) -> ! {
    // Use _exit() instead of std::process::exit() to avoid SIGILL during cleanup.
    // std::process::exit() runs atexit handlers and C++ destructors which can trigger
    // illegal instructions when exception handler state (jmp_buf), GC roots, or
    // V8 isolate state is invalid.
    #[cfg(unix)]
    unsafe {
        libc::_exit(exit_code);
    }
    #[cfg(windows)]
    {
        extern "system" {
            fn ExitProcess(uExitCode: u32);
        }
        unsafe {
            ExitProcess(exit_code as u32);
        }
    }
    #[cfg(not(any(unix, windows)))]
    std::process::exit(exit_code);
}

/// Terminate after releasing current-thread collection storage.
///
/// This is used by fatal paths that have already completed their reporting
/// callbacks and would otherwise bypass the generated executable epilogue.
pub(crate) fn exit_after_current_thread_collection_teardown(code: i32) -> ! {
    crate::gc::js_gc_release_current_thread_collection_side_allocations();
    terminate_without_atexit(code)
}

/// Validate + coerce a `process.exit(code)` argument the way Node's
/// `parseAndValidateExitCode` does, returning the truncated 32-bit exit
/// status (Node wraps the integer into the platform's 0-255 byte; an
/// `i32` cast reproduces that for the `_exit()` call). Returns `None` for
/// nullish input (caller falls back to the prior `process.exitCode`, 0).
/// Diverges via `js_throw` for invalid values.
fn validate_exit_code(code: f64) -> Option<i32> {
    let jv = JSValue::from_bits(code.to_bits());
    if jv.is_undefined() || jv.is_null() {
        return None;
    }
    // Resolve `code` to a JS number. Strings are coerced with `Number()`
    // (trim + hex/binary/octal/exponent), with empty-string and
    // NaN-producing strings rejected as TypeError; everything that is not
    // already a number is a TypeError too.
    let n = if crate::fs::validate::is_numeric(jv) {
        if jv.is_int32() {
            jv.as_int32() as f64
        } else {
            jv.as_number()
        }
    } else if jv.is_any_string() {
        match coerce_exit_code_string(code) {
            Some(num) => num,
            None => throw_exit_code_type_error(code),
        }
    } else {
        throw_exit_code_type_error(code);
    };
    // Now validate as a number: must be a finite integer.
    if !n.is_finite() || n.fract() != 0.0 {
        throw_exit_code_range_error(n);
    }
    Some(n as i32)
}

/// `Number(string)` for `process.exit("…")`. Returns `None` for the empty
/// string or any string `Number()` maps to `NaN` (Node throws TypeError
/// for those rather than RangeError).
fn coerce_exit_code_string(code: f64) -> Option<f64> {
    let ptr = crate::value::js_get_string_pointer_unified(code) as *const StringHeader;
    if ptr.is_null() {
        return None;
    }
    let s = unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    };
    // Node's `Number("")` is 0, but `process.exit("")` throws TypeError;
    // reject the empty string explicitly.
    if s.is_empty() {
        return None;
    }
    let n = js_number_coerce_string(&s);
    if n.is_nan() {
        None
    } else {
        Some(n)
    }
}

/// JS `Number(s)` semantics for an exit-code string: trim ASCII
/// whitespace, then parse decimal/hex/binary/octal/exponent. A
/// whitespace-only string is 0 (mirrors `Number("  ")`). Returns `NaN`
/// for anything that doesn't fully parse.
fn js_number_coerce_string(s: &str) -> f64 {
    let t = s.trim_matches(|c: char| c.is_ascii_whitespace());
    if t.is_empty() {
        return 0.0;
    }
    let lower = t.to_ascii_lowercase();
    let radix = |body: &str, base: u32| -> f64 {
        i64::from_str_radix(body, base)
            .map(|v| v as f64)
            .unwrap_or(f64::NAN)
    };
    if let Some(body) = lower.strip_prefix("0x") {
        return radix(body, 16);
    }
    if let Some(body) = lower.strip_prefix("0o") {
        return radix(body, 8);
    }
    if let Some(body) = lower.strip_prefix("0b") {
        return radix(body, 2);
    }
    match t {
        "Infinity" | "+Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        // Reject Rust-accepted forms JS `Number()` does not (underscores,
        // `inf`, `nan`, leading/trailing dots are fine in JS though).
        _ if t.bytes().any(|b| b == b'_') => f64::NAN,
        _ => t.parse::<f64>().unwrap_or(f64::NAN),
    }
}

fn throw_exit_code_type_error(code: f64) -> ! {
    let message = format!(
        "The \"code\" argument must be of type number. Received {}",
        crate::fs::validate::describe_received(code)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn throw_exit_code_range_error(n: f64) -> ! {
    let message = format!(
        "The value of \"code\" is out of range. It must be an integer. Received {}",
        crate::fs::validate::format_received_number(n)
    );
    crate::fs::validate::throw_range_error_with_code(&message)
}

/// process.abort() -> never. Raises SIGABRT (no clean shutdown). Matches
/// Node's behavior — atexit handlers and other shutdown logic are skipped.
#[no_mangle]
pub extern "C" fn js_process_abort() {
    #[cfg(unix)]
    unsafe {
        libc::abort();
    }
    #[cfg(not(unix))]
    std::process::abort();
}

/// process.getActiveResourcesInfo() -> string[]. Node returns names of
/// libuv handles currently keeping the loop alive (TLSWrap, Timeout,
/// TCPSERVERWRAP, ...). Perry reports its active timeout/interval handles as
/// "Timeout", matching the resource name Node uses for both timer families.
#[no_mangle]
pub extern "C" fn js_process_active_resources_info() -> f64 {
    let timeout_count = crate::timer::active_timeout_resource_count();
    let mut arr = crate::array::js_array_alloc(timeout_count as u32);
    for _ in 0..timeout_count {
        let s = js_string_from_bytes(b"Timeout".as_ptr(), "Timeout".len() as u32);
        arr = crate::array::js_array_push(arr, JSValue::string_ptr(s));
    }
    f64::from_bits(JSValue::pointer(arr as *const u8).bits())
}

fn empty_array_value() -> f64 {
    let arr = crate::array::js_array_alloc_with_length(0);
    f64::from_bits(JSValue::array_ptr(arr).bits())
}

#[no_mangle]
pub extern "C" fn js_process_binding(_name: f64) -> f64 {
    empty_object_value()
}

#[no_mangle]
pub extern "C" fn js_process_linked_binding(_name: f64) -> f64 {
    empty_object_value()
}

#[no_mangle]
pub extern "C" fn js_process_dlopen() -> f64 {
    undefined_value()
}

#[no_mangle]
pub extern "C" fn js_process_raw_debug() -> f64 {
    undefined_value()
}

#[no_mangle]
pub extern "C" fn js_process_debug_process() -> f64 {
    undefined_value()
}

#[no_mangle]
pub extern "C" fn js_process_debug_end() -> f64 {
    undefined_value()
}

#[no_mangle]
pub extern "C" fn js_process_start_profiler_idle_notifier() -> f64 {
    undefined_value()
}

#[no_mangle]
pub extern "C" fn js_process_stop_profiler_idle_notifier() -> f64 {
    undefined_value()
}

#[no_mangle]
pub extern "C" fn js_process_really_exit() -> f64 {
    undefined_value()
}

#[no_mangle]
pub extern "C" fn js_process_fatal_exception(_err: f64, _from_promise: f64) -> f64 {
    bool_value(false)
}

#[no_mangle]
pub extern "C" fn js_process_tick_callback() -> f64 {
    undefined_value()
}

#[no_mangle]
pub extern "C" fn js_process_get_active_handles() -> f64 {
    empty_array_value()
}

#[no_mangle]
pub extern "C" fn js_process_get_active_requests() -> f64 {
    empty_array_value()
}

#[no_mangle]
pub extern "C" fn js_process_open_stdin() -> f64 {
    undefined_value()
}

#[no_mangle]
pub extern "C" fn js_process_internal_kill() -> f64 {
    undefined_value()
}

/// process.cpuUsage(prior?) -> { user, system } µs.
/// Reads CPU time consumed by the process via getrusage(RUSAGE_SELF) on
/// unix. With a `prior` object, returns the diff from that sample.
/// Non-unix targets return `{ user: 0, system: 0 }`.
#[no_mangle]
pub extern "C" fn js_process_cpu_usage(prior: f64) -> f64 {
    // #3040 — validate the previous-value object and its user/system
    // fields like Node. `undefined`/`null` fall through to a baseline read;
    // anything else must be a non-array object whose `user`/`system` fields
    // are finite non-negative numbers, else TypeError [ERR_INVALID_ARG_TYPE]
    // (wrong shape / non-number field) or RangeError [ERR_INVALID_ARG_VALUE]
    // (negative / NaN / Infinity field value).
    let (mut user_us, mut system_us) = read_process_cpu_micros();
    if let Some((prev_user, prev_system)) = validate_cpu_usage_prior(prior) {
        user_us = (user_us - prev_user).max(0.0);
        system_us = (system_us - prev_system).max(0.0);
    }
    let obj = crate::object::js_object_alloc(0, 2);
    let set_field = |name: &str, value: f64| {
        let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, key, value);
    };
    set_field("user", user_us);
    set_field("system", system_us);
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

const MAX_SAFE_INTEGER_F64: f64 = 9_007_199_254_740_991.0;

fn validate_cpu_usage_prior(value: f64) -> Option<(f64, f64)> {
    if crate::value::js_is_truthy(value) == 0 {
        return None;
    }

    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() || is_array_value(jv) {
        throw_cpu_prior_invalid_type(value);
    }

    let obj_ptr = jv.as_pointer::<u8>() as *mut crate::object::ObjectHeader;
    if obj_ptr.is_null() {
        throw_cpu_prior_invalid_type(value);
    }

    Some((
        validate_cpu_usage_field(obj_ptr, "user"),
        validate_cpu_usage_field(obj_ptr, "system"),
    ))
}

fn validate_cpu_usage_field(obj: *mut crate::object::ObjectHeader, name: &'static str) -> f64 {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = crate::object::js_object_get_field_by_name_f64(obj, key);
    let jv = JSValue::from_bits(value.to_bits());
    if !crate::fs::validate::is_numeric(jv) {
        let message = format!(
            "The \"prevValue.{name}\" property must be of type number. Received {}",
            crate::fs::validate::describe_received(value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }

    let n = numeric_value(jv);
    if !previous_cpu_value_is_valid(n) {
        let message = format!(
            "The property 'prevValue.{name}' is invalid. Received {}",
            format_node_number(n)
        );
        crate::fs::validate::throw_range_error_named(&message, "ERR_INVALID_ARG_VALUE");
    }
    n
}

fn previous_cpu_value_is_valid(value: f64) -> bool {
    value.is_finite() && (0.0..=MAX_SAFE_INTEGER_F64).contains(&value)
}

fn throw_cpu_prior_invalid_type(value: f64) -> ! {
    let message = format!(
        "The \"prevValue\" argument must be of type object. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn numeric_value(jv: JSValue) -> f64 {
    if jv.is_int32() {
        jv.as_int32() as f64
    } else {
        jv.as_number()
    }
}

fn format_node_number(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        }
        .to_string();
    }
    if value.fract() == 0.0 && value.abs() < 1e21 {
        format!("{}", value as i64)
    } else {
        format!("{}", value)
    }
}

fn string_value(s: &str) -> f64 {
    let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn warning_value_to_string(v: f64) -> String {
    if JSValue::from_bits(v.to_bits()).is_undefined() {
        return String::new();
    }
    let ptr = crate::value::js_jsvalue_to_string(v);
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let header = &*ptr;
        let len = header.byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

/// Validate the optional `type` positional of `process.emitWarning` (#3662).
///
/// Node only type-checks `type` when it is supplied as a non-object value:
/// `undefined`/`null`, a string, an object (the `{ type, code, detail }`
/// overload), or a function (custom error ctor) are all accepted. A non-string
/// *primitive* (number/boolean/bigint/symbol) throws
/// `TypeError [ERR_INVALID_ARG_TYPE]` with the `"type"` argument message.
fn validate_emit_warning_type(type_name: f64) {
    let jv = JSValue::from_bits(type_name.to_bits());
    if jv.is_undefined() || jv.is_null() || jv.is_any_string() || jv.is_pointer() {
        return;
    }
    let received = crate::fs::validate::describe_received(type_name);
    let message = format!("The \"type\" argument must be of type string. Received {received}");
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

fn object_from_value(value: f64) -> Option<*mut crate::object::ObjectHeader> {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer::<u8>() as *mut u8;
    if ptr.is_null() || !crate::object::is_valid_obj_ptr(ptr as *const u8) {
        return None;
    }
    Some(ptr as *mut crate::object::ObjectHeader)
}

fn object_string_field(obj_handle: &crate::gc::RuntimeHandle<'_>, name: &str) -> Option<String> {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = crate::object::js_object_get_field_by_name_f64(
        obj_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>(),
        key,
    );
    if JSValue::from_bits(value.to_bits()).is_undefined() {
        None
    } else {
        Some(warning_value_to_string(value))
    }
}

fn set_error_string_prop(error: *mut crate::error::ErrorHeader, name: &str, value: &str) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let error_handle = scope.root_raw_mut_ptr(error);
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let key_handle = scope.root_string_ptr(key);
    let value_handle = scope.root_nanbox_f64(string_value(value));
    crate::object::js_object_set_field_by_name(
        error_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>(),
        key_handle.get_raw_const_ptr::<StringHeader>() as *mut StringHeader,
        value_handle.get_nanbox_f64(),
    );
}

static WARNED_PROCESS_WARNING_TRACE_HINT: AtomicBool = AtomicBool::new(false);

extern "C" fn process_warning_callback(closure: *const ClosureHeader) -> f64 {
    use std::io::Write;

    if closure.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let warning_handle = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 0));
    let line = warning_value_to_string(js_closure_get_capture_f64(closure, 1));
    let detail = warning_value_to_string(js_closure_get_capture_f64(closure, 2));
    let hint = warning_value_to_string(js_closure_get_capture_f64(closure, 3));

    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(stderr, "{line}");
    if !detail.is_empty() {
        let _ = writeln!(stderr, "{detail}");
    }
    if !hint.is_empty() {
        let _ = writeln!(stderr, "{hint}");
    }

    crate::os::emit_process_event("warning", &[warning_handle.get_nanbox_f64()]);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn schedule_warning(warning: f64, label: &str, code: &str, msg: &str, detail: &str) {
    let pid = std::process::id();
    let line = if code.is_empty() {
        format!("(node:{pid}) {label}: {msg}")
    } else {
        format!("(node:{pid}) [{code}] {label}: {msg}")
    };
    let hint_flag = if label == "DeprecationWarning" {
        "--trace-deprecation"
    } else {
        "--trace-warnings"
    };
    let hint = if !WARNED_PROCESS_WARNING_TRACE_HINT.swap(true, Ordering::AcqRel) {
        format!("(Use `node {hint_flag} ...` to show where the warning was created)")
    } else {
        String::new()
    };

    let scope = crate::gc::RuntimeHandleScope::new();
    let warning_handle = scope.root_nanbox_f64(warning);
    let line_handle = scope.root_nanbox_f64(string_value(&line));
    let detail_handle = scope.root_nanbox_f64(string_value(detail));
    let hint_handle = scope.root_nanbox_f64(string_value(&hint));

    let callback = js_closure_alloc(process_warning_callback as *const u8, 4);
    if callback.is_null() {
        return;
    }
    let callback_handle = scope.root_raw_mut_ptr(callback);
    js_closure_set_capture_f64(
        callback_handle.get_raw_mut_ptr(),
        0,
        warning_handle.get_nanbox_f64(),
    );
    js_closure_set_capture_f64(
        callback_handle.get_raw_mut_ptr(),
        1,
        line_handle.get_nanbox_f64(),
    );
    js_closure_set_capture_f64(
        callback_handle.get_raw_mut_ptr(),
        2,
        detail_handle.get_nanbox_f64(),
    );
    js_closure_set_capture_f64(
        callback_handle.get_raw_mut_ptr(),
        3,
        hint_handle.get_nanbox_f64(),
    );
    crate::builtins::js_queue_next_tick(callback_handle.get_raw_const_ptr::<ClosureHeader>() as i64);
}

/// process.emitWarning(warning[, type, code, ctor]) -> undefined.
///
/// The direct-call lowering still passes the first three JS values here. The
/// runtime parses the modern options-object overload, creates an Error-like
/// warning object, and queues the warning job so stderr/event delivery happens
/// after the current synchronous frame.
#[no_mangle]
pub extern "C" fn js_process_emit_warning(warning: f64, type_name: f64, code: f64) {
    // #3662 — Node validates the optional `type` (when supplied as a non-object
    // positional) and then the `warning` argument before building the warning,
    // throwing `TypeError [ERR_INVALID_ARG_TYPE]`. The object overload (where
    // `type_name` carries `{ type, code, detail }`) is exempt, as is the
    // function (custom ctor) form — both are valid Node usages.
    validate_emit_warning_type(type_name);
    let warning_jv = JSValue::from_bits(warning.to_bits());
    let warning_is_valid = warning_jv.is_any_string()
        || crate::error::js_error_is_error(warning).to_bits() == crate::value::TAG_TRUE;
    if !warning_is_valid {
        let received = crate::fs::validate::describe_received(warning);
        let message = format!(
            "The \"warning\" argument must be of type string or an instance of Error. Received {received}"
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }

    let msg = warning_value_to_string(warning);

    let (raw_type, raw_code, detail) = if let Some(options) = object_from_value(type_name) {
        let scope = crate::gc::RuntimeHandleScope::new();
        let options_handle = scope.root_raw_mut_ptr(options);
        (
            object_string_field(&options_handle, "type").unwrap_or_default(),
            object_string_field(&options_handle, "code").unwrap_or_default(),
            object_string_field(&options_handle, "detail").unwrap_or_default(),
        )
    } else {
        (
            warning_value_to_string(type_name),
            warning_value_to_string(code),
            String::new(),
        )
    };
    let label = if raw_type.is_empty() {
        "Warning".to_string()
    } else {
        raw_type
    };

    let message_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let warning_error = crate::error::js_error_new_with_message(message_ptr);
    let scope = crate::gc::RuntimeHandleScope::new();
    let warning_handle = scope.root_raw_mut_ptr(warning_error);
    set_error_string_prop(
        warning_handle.get_raw_mut_ptr::<crate::error::ErrorHeader>(),
        "name",
        &label,
    );
    if !raw_code.is_empty() {
        set_error_string_prop(
            warning_handle.get_raw_mut_ptr::<crate::error::ErrorHeader>(),
            "code",
            &raw_code,
        );
    }
    if !detail.is_empty() {
        set_error_string_prop(
            warning_handle.get_raw_mut_ptr::<crate::error::ErrorHeader>(),
            "detail",
            &detail,
        );
    }
    let warning_value = crate::value::js_nanbox_pointer(
        warning_handle.get_raw_const_ptr::<crate::error::ErrorHeader>() as i64,
    );
    schedule_warning(warning_value, &label, &raw_code, &msg, &detail);
}

/// process.availableMemory() -> number. Free system memory available to
/// the process in bytes. Delegates to `js_os_freemem`'s host-statistics
/// path on macOS/iOS, sysinfo on Linux, GlobalMemoryStatusEx on Windows.
#[no_mangle]
pub extern "C" fn js_process_available_memory() -> f64 {
    crate::os::js_os_freemem()
}

/// process.constrainedMemory() -> number. The memory limit imposed by the
/// OS (cgroups v2 on Linux containers), in bytes. Returns 0 when no
/// effective limit applies — Node also returns 0 in that case. macOS and
/// Windows have no per-process equivalent we read here, so they always
/// return 0.
#[no_mangle]
pub extern "C" fn js_process_constrained_memory() -> f64 {
    #[cfg(target_os = "linux")]
    {
        // cgroups v2 reports the memory limit as a decimal number in
        // bytes, or the literal string "max" for "no limit". Older
        // cgroups v1 expose memory.limit_in_bytes — we try both.
        for path in [
            "/sys/fs/cgroup/memory.max",
            "/sys/fs/cgroup/memory/memory.limit_in_bytes",
        ] {
            if let Ok(s) = std::fs::read_to_string(path) {
                let s = s.trim();
                if s == "max" {
                    return 0.0;
                }
                if let Ok(v) = s.parse::<u64>() {
                    // Kernel returns u64::MAX (or close to it) to mean
                    // "unlimited" in cgroups v1; treat anything near that
                    // ceiling as unconstrained.
                    if v < (u64::MAX / 2) {
                        return v as f64;
                    }
                    return 0.0;
                }
            }
        }
        0.0
    }
    #[cfg(not(target_os = "linux"))]
    {
        0.0
    }
}

/// Get an environment variable by name (takes JS string pointer)
/// Returns a string pointer, or null (0) if not found
#[no_mangle]
pub extern "C" fn js_getenv(name_ptr: *const StringHeader) -> *mut StringHeader {
    unsafe {
        if name_ptr.is_null() || (name_ptr as usize) < 0x1000 {
            return std::ptr::null_mut();
        }

        let len = (*name_ptr).byte_len as usize;
        let data_ptr = (name_ptr as *const u8).add(std::mem::size_of::<StringHeader>());

        // Convert to Rust string
        let name_bytes = std::slice::from_raw_parts(data_ptr, len);
        let name = match std::str::from_utf8(name_bytes) {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };

        match std::env::var(name) {
            Ok(value) => {
                // Create a JS string from the value
                let bytes = value.as_bytes();
                js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
            }
            Err(_) => std::ptr::null_mut(), // Not found, return null
        }
    }
}

/// Get an environment variable, returning a fully NaN-boxed JS value.
///
/// Unlike `js_getenv` (which returns a raw `*mut StringHeader`, 0 when
/// unset), this returns an f64 NaN-boxed value the call site can use
/// directly. An unset var yields `undefined` — matching Node, where
/// `process.env.UNSET` is `undefined` — so `process.env.X ?? default`
/// applies the default. Tagging the null pointer as a STRING_TAG value
/// instead (the old fast-path behavior) produced a value that read as
/// `typeof "string"` yet stringified to `null` and was non-nullish, so
/// `??` silently swallowed the fallback (#1312).
///
/// A var that IS set to the empty string still returns `""` (a valid,
/// non-null string), which is falsy but not nullish — also matching
/// Node, so `??` won't clobber a legitimately empty value.
#[no_mangle]
pub extern "C" fn js_getenv_value(name_ptr: *const StringHeader) -> f64 {
    let ptr = js_getenv(name_ptr);
    let val = if ptr.is_null() {
        JSValue::undefined()
    } else {
        JSValue::string_ptr(ptr)
    };
    f64::from_bits(val.bits())
}

// ─── #1350: process.exitCode (default undefined + set/get) ────────────────────
//
// Node lets user code stash an exit code that `process.exit()` (no arg)
// will use as the final code. Reads start `undefined`; writes coerce
// the value to a number-like and stash it. We back this with a single
// thread-local cell holding the NaN-boxed bits, default-initialised to
// `JSValue::undefined()`'s bit pattern.

thread_local! {
    static PROCESS_EXIT_CODE: std::cell::Cell<u64> =
        std::cell::Cell::new(crate::value::JSValue::undefined().bits());
}

/// `process.exitCode` value-read. Returns the last value assigned, or
/// `undefined` if nothing has been set.
#[no_mangle]
pub extern "C" fn js_process_exit_code_get() -> f64 {
    let bits = PROCESS_EXIT_CODE.with(|c| c.get());
    f64::from_bits(bits)
}

/// `process.exitCode = v`. Stores the raw NaN-boxed bits verbatim so
/// the read round-trips byte-for-byte — Node forwards e.g. the string
/// `"0"` as a string and only coerces when `process.exit()` runs.
///
/// Returns `value` so the call site can use it as the result of the
/// assignment expression (JS assignment evaluates to the RHS value).
/// That keeps the codegen path uniform with other `js_*` runtime
/// helpers that return f64 — see `lower_call/extern_func.rs:330` for
/// the direct-call path.
#[no_mangle]
pub extern "C" fn js_process_exit_code_set(value: f64) -> f64 {
    PROCESS_EXIT_CODE.with(|c| c.set(value.to_bits()));
    value
}

/// Set an environment variable. Backs `process.env.X = v` (#1344).
///
/// Reads via `js_getenv_value` already hit `std::env::var`, so writing
/// through `std::env::set_var` round-trips with no caching layer to
/// keep in sync. Non-string values are coerced via the same
/// `js_jsvalue_to_string` Perry uses for `String(x)` / template
/// concat — matching Node, which coerces `process.env.PORT = 8080` to
/// `"8080"` before storing.
///
/// On unset (calling code routes `delete process.env.X` here too if
/// it lowers the delete to `process.env.X = undefined` — the empty
/// SAFE-EMPTY-STRING vs unset distinction is handled by
/// `js_removeenv` below, which the delete path can call directly).
#[no_mangle]
pub extern "C" fn js_setenv(name_ptr: *const StringHeader, value: f64) {
    use crate::value::js_jsvalue_to_string;
    unsafe {
        if name_ptr.is_null() || (name_ptr as usize) < 0x1000 {
            return;
        }
        let len = (*name_ptr).byte_len as usize;
        let data_ptr = (name_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let name_bytes = std::slice::from_raw_parts(data_ptr, len);
        let name = match std::str::from_utf8(name_bytes) {
            Ok(s) => s,
            Err(_) => return,
        };
        if !env_name_is_settable(name) {
            return;
        }

        // Coerce value to string. js_jsvalue_to_string handles
        // numbers/booleans/null/undefined and returns a *mut StringHeader.
        let value_str_hdr = js_jsvalue_to_string(value);
        if value_str_hdr.is_null() {
            // Defensive: null shouldn't happen for non-undefined inputs,
            // but if it does we silently no-op rather than crash. The
            // `= undefined` case is intentionally rare in practice.
            return;
        }

        // Read the string bytes back into a Rust &str directly off the
        // StringHeader payload — same layout as `js_getenv` uses for the
        // name above.
        let v_len = (*value_str_hdr).byte_len as usize;
        let v_data = (value_str_hdr as *const u8).add(std::mem::size_of::<StringHeader>());
        let v_bytes = std::slice::from_raw_parts(v_data, v_len);
        let v_str = match std::str::from_utf8(v_bytes) {
            Ok(s) => s,
            Err(_) => return,
        };
        std::env::set_var(name, v_str);
        // Keep the cached `process.env` object in step so enumeration
        // (`Object.keys(process.env)`, `for…in`, spread) sees the new key —
        // reads go through `js_getenv`, but enumeration walks this object.
        let cached = CACHED_ENV.with(|c| c.get());
        if cached != 0.0 {
            let obj = crate::value::js_nanbox_get_pointer(cached) as *mut crate::ObjectHeader;
            if !obj.is_null() {
                let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
                let val = js_string_from_bytes(v_str.as_ptr(), v_str.len() as u32);
                let val_f64 = f64::from_bits(JSValue::string_ptr(val).bits());
                crate::object::js_object_set_field_by_name(obj, key, val_f64);
            }
        }
    }
}

// #1344: `js_setenv` / `js_removeenv` are emitted by codegen for
// `process.env.X = v` and `delete process.env.X`, but nothing in the Rust
// crate graph references them. The default `.a` staticlib keeps `#[no_mangle]`
// exports via staticlib-export semantics, but the auto-optimize build round-
// trips the runtime through whole-program LLVM bitcode and is free to
// internalize + dead-strip an unreferenced symbol — leaving the codegen call
// dangling (`Undefined symbols: _js_setenv` at final link, which is exactly
// how #1344's acceptance test still failed on main). The `#[used]` statics
// below pin a retained reference edge so both survive every link mode. See
// the same pattern in `value/dyn_index.rs`.
#[used]
static KEEP_JS_SETENV: extern "C" fn(*const StringHeader, f64) = js_setenv;
#[used]
static KEEP_JS_REMOVEENV: extern "C" fn(*const StringHeader) = js_removeenv;
// #3120: codegen emits `js_module_find_package_json` only from generated `.o`,
// so pin a retained reference edge for the auto-optimize whole-program build.
#[used]
static KEEP_JS_MODULE_FIND_PACKAGE_JSON: extern "C" fn(f64, f64) -> f64 =
    js_module_find_package_json;
// node:module helper-state APIs are codegen-emitted from generated `.o`, so pin
// retained reference edges for the auto-optimize whole-program build.
#[used]
static KEEP_JS_MODULE_ENABLE_COMPILE_CACHE: extern "C" fn(f64) -> f64 =
    js_module_enable_compile_cache;
#[used]
static KEEP_JS_MODULE_FLUSH_COMPILE_CACHE: extern "C" fn() -> f64 = js_module_flush_compile_cache;
#[used]
static KEEP_JS_MODULE_GET_COMPILE_CACHE_DIR: extern "C" fn() -> f64 =
    js_module_get_compile_cache_dir;
#[used]
static KEEP_JS_MODULE_GET_SOURCE_MAPS_SUPPORT: extern "C" fn() -> f64 =
    js_module_get_source_maps_support;
#[used]
static KEEP_JS_MODULE_SET_SOURCE_MAPS_SUPPORT: extern "C" fn(f64, f64) -> f64 =
    js_module_set_source_maps_support;
#[used]
static KEEP_JS_MODULE_STRIP_TYPESCRIPT_TYPES: extern "C" fn(f64, f64) -> f64 =
    js_module_strip_typescript_types;
#[used]
static KEEP_JS_MODULE_REGISTER: extern "C" fn(f64, f64, f64) -> f64 = js_module_register;
#[used]
static KEEP_JS_MODULE_REGISTER_HOOKS: extern "C" fn(f64) -> f64 = js_module_register_hooks;
#[used]
static KEEP_JS_MODULE_DYNAMIC_IMPORT_APPLY_HOOKS: extern "C" fn(f64) -> f64 =
    js_module_dynamic_import_apply_hooks;
#[used]
static KEEP_JS_MODULE_MODULE_NEW: extern "C" fn(f64) -> f64 = js_module_module_new;
#[used]
static KEEP_JS_MODULE_FIND_PATH: extern "C" fn(f64, f64, f64) -> f64 = js_module_find_path;
#[used]
static KEEP_JS_MODULE_INIT_PATHS: extern "C" fn() -> f64 = js_module_init_paths;
#[used]
static KEEP_JS_MODULE_LOAD: extern "C" fn(f64, f64, f64) -> f64 = js_module_load;
#[used]
static KEEP_JS_MODULE_NODE_MODULE_PATHS: extern "C" fn(f64) -> f64 = js_module_node_module_paths;
#[used]
static KEEP_JS_MODULE_PRELOAD_MODULES: extern "C" fn(f64) -> f64 = js_module_preload_modules;
#[used]
static KEEP_JS_MODULE_RESOLVE_FILENAME: extern "C" fn(f64, f64, f64, f64) -> f64 =
    js_module_resolve_filename;
#[used]
static KEEP_JS_MODULE_RESOLVE_LOOKUP_PATHS: extern "C" fn(f64, f64) -> f64 =
    js_module_resolve_lookup_paths;

/// Unset an environment variable. Backs `delete process.env.X` (#1344).
#[no_mangle]
pub extern "C" fn js_removeenv(name_ptr: *const StringHeader) {
    unsafe {
        if name_ptr.is_null() || (name_ptr as usize) < 0x1000 {
            return;
        }
        let len = (*name_ptr).byte_len as usize;
        let data_ptr = (name_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let name_bytes = std::slice::from_raw_parts(data_ptr, len);
        let name = match std::str::from_utf8(name_bytes) {
            Ok(s) => s,
            Err(_) => return,
        };
        std::env::remove_var(name);
    }
}

/// `process.env` as a materialized JS object.
///
/// Built lazily on first access from `std::env::vars()` so the object
/// reflects the inherited OS environment (matching Node/Bun semantics).
/// Subsequent calls return the same cached pointer — user mutations to
/// keys stay visible, which is Node's spec too (`process.env` is a live
/// object, not a snapshot rebuilt on every read).
///
/// Returns an f64 NaN-boxed POINTER_TAG value so the codegen can hand
/// it straight to subsequent PropertyGet dispatch.
#[no_mangle]
pub extern "C" fn js_process_env() -> f64 {
    js_process_env_impl()
}

thread_local! {
    static CACHED_ENV: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
}

/// Is `value` the live `process.env` object? Writes to it must reach the real
/// environment (`js_setenv`), not just the cached field bag: `process.env.X`
/// READS lower to `js_getenv`, so a field-only store is invisible.
/// `Object.assign(process.env, parsed)` is how `@next/env` loads `.env` files —
/// under Perry the keys landed in the object and every read still returned
/// `undefined`, so a Next.js standalone server saw NONE of its `.env` config
/// (myairank: `process.env.DATABASE_URL` undefined ⇒ mysql2 connected with an
/// empty user/database and the MySQL handshake timed out).
pub fn is_process_env_object(value: f64) -> bool {
    let cached = CACHED_ENV.with(|c| c.get());
    cached != 0.0 && cached.to_bits() == value.to_bits()
}

/// True when `addr` is the heap address of the cached `process.env` object.
///
/// The pointer form of [`is_process_env_object`], for call sites that have
/// already unboxed the target (`Object.assign`'s write funnel).
pub fn is_process_env_ptr(addr: usize) -> bool {
    let cached = CACHED_ENV.with(|c| c.get());
    if cached == 0.0 {
        return false;
    }
    crate::value::js_nanbox_get_pointer(cached) as usize == addr
}

/// `std::env::set_var` PANICS — and, being called from an `extern "C"` frame,
/// aborts the process — when the name is empty, contains `=`, or contains a NUL
/// byte. `Object.assign(process.env, parsed)` feeds it arbitrary object keys, so
/// a single malformed key in a `.env` file would take the whole server down.
/// Node accepts such an assignment silently, so skip these names rather than
/// crash.
fn env_name_is_settable(name: &str) -> bool {
    !name.is_empty() && !name.contains('=') && !name.contains('\0')
}

fn js_process_env_impl() -> f64 {
    ipc::process_ipc_ensure_initialized();
    let cached = CACHED_ENV.with(|c| c.get());
    if cached != 0.0 {
        return cached;
    }

    let vars: Vec<(String, String)> = std::env::vars().collect();
    // Pad alloc_limit so small env sets still have headroom; large
    // environments (CI runners) spill to the overflow Vec path.
    let alloc_limit = std::cmp::max(vars.len() as u32, 8);
    let obj = crate::object::js_object_alloc(0, alloc_limit);
    for (k, v) in &vars {
        let key = js_string_from_bytes(k.as_ptr(), k.len() as u32);
        let val = js_string_from_bytes(v.as_ptr(), v.len() as u32);
        let val_f64 = f64::from_bits(JSValue::string_ptr(val).bits());
        crate::object::js_object_set_field_by_name(obj, key, val_f64);
    }
    let boxed = f64::from_bits(JSValue::pointer(obj as *const u8).bits());
    CACHED_ENV.with(|c| c.set(boxed));
    boxed
}

/// process.threadCpuUsage(prior?) -> object { user, system } in microseconds.
/// CPU time consumed by the current thread. Uses CLOCK_THREAD_CPUTIME_ID
/// (available on macOS 10.12+ and Linux). Platforms without the clock get
/// 0.0 for both fields.
#[no_mangle]
pub extern "C" fn js_process_thread_cpu_usage(prior: f64) -> f64 {
    let (mut user_us, mut system_us) = read_thread_cpu_micros();
    if let Some((prev_user, prev_system)) = validate_cpu_usage_prior(prior) {
        user_us -= prev_user;
        system_us -= prev_system;
    }

    let obj = crate::object::js_object_alloc(0, 2);
    let set_field = |name: &str, value: f64| {
        let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, key, value);
    };
    set_field("user", user_us);
    set_field("system", system_us);
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// process.memoryUsage() -> object { rss, heapTotal, heapUsed, external, arrayBuffers }
/// Returns memory usage information matching Node.js API
#[no_mangle]
pub extern "C" fn js_process_memory_usage() -> f64 {
    let mut heap_used: u64 = 0;
    let mut heap_total: u64 = 0;
    crate::arena::js_arena_stats(&mut heap_used, &mut heap_total);

    let rss = get_rss_bytes();

    // Allocate object with 5 fields
    let obj = crate::object::js_object_alloc(0, 5);

    // Set fields by name to match Node.js API
    let set_field = |name: &str, value: f64| {
        let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, key, value);
    };

    set_field("rss", rss as f64);
    set_field("heapTotal", heap_total as f64);
    set_field("heapUsed", heap_used as f64);
    set_field("external", 0.0);
    set_field("arrayBuffers", 0.0);

    // Return as NaN-boxed pointer (convert bits to f64)
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// process.loadEnvFile(path?) — read a `.env`-formatted file from disk and
/// merge its `KEY=value` entries into `process.env`. Node 20.12+. With no
/// path, the default is `.env` in the current working directory. Throws a
/// Node-shaped `Error` (`code: "ENOENT"`, `syscall: "open"`) when the file
/// can't be opened. #2135 (#1399 follow-through): previously a no-op that
/// returned undefined so probe-and-call sites didn't crash; with
/// `process.env.X = v` now persisting via std::env (#1344), eager loading
/// is meaningful.
#[no_mangle]
pub extern "C" fn js_process_load_env_file(path_value: f64) {
    let target = load_env_file_path(path_value);
    let contents = match std::fs::read_to_string(&target) {
        Ok(s) => s,
        Err(err) => unsafe {
            throw_load_env_file_open_error(&err, &target);
        },
    };
    for (key, value) in crate::util_parse_env::parse_env(&contents) {
        if std::env::var_os(&key).is_none() {
            std::env::set_var(key, value);
        }
    }
}

fn load_env_file_path(value: f64) -> String {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() || jv.is_null() {
        return ".env".to_string();
    }
    unsafe {
        validate_load_env_file_url(value);
        crate::fs::decode_path_value(value)
            .unwrap_or_else(|| crate::fs::validate::throw_invalid_path_arg("path", value))
    }
}

unsafe fn validate_load_env_file_url(value: f64) {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return;
    }
    let obj = jv.as_pointer::<crate::object::ObjectHeader>() as *mut crate::object::ObjectHeader;
    if obj.is_null() || !crate::url::is_url_object_shape(obj) {
        return;
    }
    let protocol = crate::url::get_string_content(crate::object::js_object_get_field_f64(
        obj,
        crate::url::parse::URL_PROTOCOL,
    ));
    if protocol != "file:" {
        throw_invalid_load_env_file_url_scheme();
    }
    let pathname = crate::url::get_string_content(crate::object::js_object_get_field_f64(
        obj,
        crate::url::parse::URL_PATHNAME,
    ));
    if has_encoded_forward_slash(&pathname) {
        crate::fs::validate::throw_type_error_with_code(
            "File URL path must not include encoded / characters",
            "ERR_INVALID_FILE_URL_PATH",
        );
    }
}

fn has_encoded_forward_slash(pathname: &str) -> bool {
    let bytes = pathname.as_bytes();
    let mut i = 0usize;
    while i + 2 < bytes.len() {
        if bytes[i] == b'%' && bytes[i + 1] == b'2' && (bytes[i + 2] | 0x20) == b'f' {
            return true;
        }
        i += 1;
    }
    false
}

fn throw_invalid_load_env_file_url_scheme() -> ! {
    crate::fs::validate::throw_type_error_with_code(
        "The URL must be of scheme file",
        "ERR_INVALID_URL_SCHEME",
    )
}

unsafe fn throw_load_env_file_open_error(err: &std::io::Error, target: &str) -> ! {
    use std::io::ErrorKind;
    let code: &'static str = match err.kind() {
        ErrorKind::NotFound => "ENOENT",
        ErrorKind::PermissionDenied => "EACCES",
        _ => "EIO",
    };
    let desc = match code {
        "ENOENT" => "no such file or directory",
        "EACCES" => "permission denied",
        _ => "i/o error",
    };
    let message = format!("{code}: {desc}, open '{target}'");
    let msg_ptr = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg_ptr, code);
    crate::node_submodules::register_error_syscall(msg_ptr, "open");
    crate::node_submodules::register_error_path(msg_ptr, target.to_string());
    let err_ptr = crate::error::js_error_new_with_message(msg_ptr);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err_ptr as i64));
}

// Issue #2013 — process-arg-validation helpers shared by `js_process_chdir`
// and `js_process_hrtime`. Sited here (not os.rs) so the process surface's
// validation logic stays under the 2000-line file gate as the os.rs splits
// progress.

/// `process.chdir(value)` entry point that takes the full NaN-boxed
/// value. Throws `TypeError [ERR_INVALID_ARG_TYPE]` for any non-string
/// (matching Node), then re-dispatches to `js_process_chdir` with the
/// extracted `StringHeader`. The codegen now emits this entry instead
/// of the bare string-only one so a `process.chdir(123)` call throws
/// the right error code instead of garbage-deref'ing to an `ENOENT`
/// based on whatever bytes the numeric value masqueraded as.
#[no_mangle]
pub unsafe extern "C" fn js_process_chdir_jsv(value: f64) {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_any_string() {
        let message = format!(
            "The \"directory\" argument must be of type string. Received {}",
            crate::fs::validate::describe_received(value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    crate::os::js_process_chdir(ptr);
}

fn execve_throw_invalid_arg_type(name: &str, expected: &str, value: f64) -> ! {
    let message = format!(
        "The \"{}\" argument must be {}. Received {}",
        name,
        expected,
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn execve_received_value(value: f64) -> String {
    let jv = JSValue::from_bits(value.to_bits());
    if crate::fs::validate::is_numeric(jv) {
        let n = if jv.is_int32() {
            jv.as_int32() as f64
        } else {
            jv.as_number()
        };
        return crate::fs::validate::format_received_number(n);
    }
    if let Some(value) = module_value_to_string(value) {
        return format!("'{}'", value);
    }
    crate::fs::validate::describe_received(value)
}

fn execve_env_received(value: f64) -> String {
    let Some(obj) = module_object_ptr(value) else {
        return crate::fs::validate::describe_received(value);
    };
    let keys = crate::object::js_object_keys(obj);
    let len = crate::array::js_array_length(keys);
    let mut parts = Vec::new();
    for i in 0..len.min(3) {
        let key_value = crate::array::js_array_get_f64(keys, i);
        let key = module_value_to_string(key_value).unwrap_or_default();
        let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
        let field = crate::object::js_object_get_field_by_name_f64(obj, key_ptr);
        parts.push(format!("{}: {}", key, execve_received_value(field)));
    }
    if len > 3 {
        parts.push("...".to_string());
    }
    format!("{{ {} }}", parts.join(", "))
}

fn execve_throw_invalid_arg_value(name: &str, received: String) -> ! {
    let message = format!(
        "The argument '{}' must be a string without null bytes. Received {}",
        name, received
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE")
}

fn execve_throw_invalid_env(value: f64) -> ! {
    let message = format!(
        "The argument 'env' must be an object with string keys and values without null bytes. Received {}",
        execve_env_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE")
}

fn execve_parse_args(args: f64) -> Vec<String> {
    let args_value = JSValue::from_bits(args.to_bits());
    if args_value.is_undefined() {
        return Vec::new();
    }
    if !is_array_value(args_value) {
        execve_throw_invalid_arg_type("args", "an instance of Array", args);
    }
    let arr = args_value.as_pointer::<crate::array::ArrayHeader>();
    let len = crate::array::js_array_length(arr);
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        let value = crate::array::js_array_get_f64(arr, i);
        let Some(item) = module_value_to_string(value) else {
            execve_throw_invalid_arg_value(&format!("args[{i}]"), execve_received_value(value));
        };
        if item.as_bytes().contains(&0) {
            execve_throw_invalid_arg_value(&format!("args[{i}]"), execve_received_value(value));
        }
        out.push(item);
    }
    out
}

fn execve_parse_env(env: f64) -> Vec<(String, String)> {
    let env_value = JSValue::from_bits(env.to_bits());
    if env_value.is_undefined() {
        return std::env::vars().collect();
    }
    let Some(obj) = module_object_ptr(env) else {
        execve_throw_invalid_arg_type("env", "of type object", env);
    };
    let keys = crate::object::js_object_keys(obj);
    let len = crate::array::js_array_length(keys);
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        let key_value = crate::array::js_array_get_f64(keys, i);
        let Some(key) = module_value_to_string(key_value) else {
            execve_throw_invalid_env(env);
        };
        if key.as_bytes().contains(&0) {
            execve_throw_invalid_env(env);
        }
        let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
        let value = crate::object::js_object_get_field_by_name_f64(obj, key_ptr);
        let Some(value_string) = module_value_to_string(value) else {
            execve_throw_invalid_env(env);
        };
        if value_string.as_bytes().contains(&0) {
            execve_throw_invalid_env(env);
        }
        out.push((key, value_string));
    }
    out
}

#[no_mangle]
pub extern "C" fn js_process_execve(exec_path: f64, args: f64, env: f64) -> f64 {
    let Some(path) = module_value_to_string(exec_path) else {
        execve_throw_invalid_arg_type("execPath", "of type string", exec_path);
    };
    if path.as_bytes().contains(&0) {
        execve_throw_invalid_arg_value("execPath", execve_received_value(exec_path));
    }
    let argv = execve_parse_args(args);
    let env_pairs = execve_parse_env(env);

    #[cfg(unix)]
    {
        let path_c = match std::ffi::CString::new(path.as_str()) {
            Ok(path_c) => path_c,
            Err(_) => execve_throw_invalid_arg_value("execPath", execve_received_value(exec_path)),
        };
        let argv_c: Vec<std::ffi::CString> = argv
            .iter()
            .map(|arg| std::ffi::CString::new(arg.as_str()).unwrap())
            .collect();
        let env_c: Vec<std::ffi::CString> = env_pairs
            .iter()
            .map(|(key, value)| std::ffi::CString::new(format!("{key}={value}")).unwrap())
            .collect();
        let mut argv_ptrs: Vec<*const libc::c_char> =
            argv_c.iter().map(|arg| arg.as_ptr()).collect();
        let mut env_ptrs: Vec<*const libc::c_char> =
            env_c.iter().map(|entry| entry.as_ptr()).collect();
        argv_ptrs.push(std::ptr::null());
        env_ptrs.push(std::ptr::null());
        unsafe {
            libc::execve(path_c.as_ptr(), argv_ptrs.as_ptr(), env_ptrs.as_ptr());
            libc::abort();
        }
    }

    #[cfg(not(unix))]
    {
        let _ = (path, argv, env_pairs);
        crate::fs::validate::throw_type_error_with_code(
            "process.execve() is unavailable on this platform",
            "ERR_FEATURE_UNAVAILABLE_ON_PLATFORM",
        )
    }
}

/// process.resourceUsage() -> object with getrusage(RUSAGE_SELF)
/// counters matching Node's shape (#1376). Linux's `ru_maxrss` is in
/// kilobytes; macOS/BSD's is in bytes — Node normalizes Linux to bytes,
/// so we do too. Non-unix targets return zeroed fields.
#[no_mangle]
pub extern "C" fn js_process_resource_usage() -> f64 {
    #[allow(unused_mut)]
    let mut user_cpu: f64 = 0.0;
    #[allow(unused_mut)]
    let mut system_cpu: f64 = 0.0;
    #[allow(unused_mut)]
    let mut max_rss: f64 = 0.0;
    #[allow(unused_mut)]
    let mut shared_mem: f64 = 0.0;
    #[allow(unused_mut)]
    let mut unshared_data: f64 = 0.0;
    #[allow(unused_mut)]
    let mut unshared_stack: f64 = 0.0;
    #[allow(unused_mut)]
    let mut minor_faults: f64 = 0.0;
    #[allow(unused_mut)]
    let mut major_faults: f64 = 0.0;
    #[allow(unused_mut)]
    let mut swapped_out: f64 = 0.0;
    #[allow(unused_mut)]
    let mut fs_read: f64 = 0.0;
    #[allow(unused_mut)]
    let mut fs_write: f64 = 0.0;
    #[allow(unused_mut)]
    let mut ipc_sent: f64 = 0.0;
    #[allow(unused_mut)]
    let mut ipc_recv: f64 = 0.0;
    #[allow(unused_mut)]
    let mut signals: f64 = 0.0;
    #[allow(unused_mut)]
    let mut vcsw: f64 = 0.0;
    #[allow(unused_mut)]
    let mut ivcsw: f64 = 0.0;

    #[cfg(unix)]
    {
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } == 0 {
            user_cpu = (usage.ru_utime.tv_sec as f64) * 1_000_000.0 + usage.ru_utime.tv_usec as f64;
            system_cpu =
                (usage.ru_stime.tv_sec as f64) * 1_000_000.0 + usage.ru_stime.tv_usec as f64;
            #[cfg(target_os = "linux")]
            {
                max_rss = (usage.ru_maxrss as f64) * 1024.0;
            }
            #[cfg(not(target_os = "linux"))]
            {
                max_rss = usage.ru_maxrss as f64;
            }
            shared_mem = usage.ru_ixrss as f64;
            unshared_data = usage.ru_idrss as f64;
            unshared_stack = usage.ru_isrss as f64;
            minor_faults = usage.ru_minflt as f64;
            major_faults = usage.ru_majflt as f64;
            swapped_out = usage.ru_nswap as f64;
            fs_read = usage.ru_inblock as f64;
            fs_write = usage.ru_oublock as f64;
            ipc_sent = usage.ru_msgsnd as f64;
            ipc_recv = usage.ru_msgrcv as f64;
            signals = usage.ru_nsignals as f64;
            vcsw = usage.ru_nvcsw as f64;
            ivcsw = usage.ru_nivcsw as f64;
        }
    }

    let obj = crate::object::js_object_alloc(0, 16);
    let set_field = |name: &str, value: f64| {
        let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, key, value);
    };
    set_field("userCPUTime", user_cpu);
    set_field("systemCPUTime", system_cpu);
    set_field("maxRSS", max_rss);
    set_field("sharedMemorySize", shared_mem);
    set_field("unsharedDataSize", unshared_data);
    set_field("unsharedStackSize", unshared_stack);
    set_field("minorPageFault", minor_faults);
    set_field("majorPageFault", major_faults);
    set_field("swappedOut", swapped_out);
    set_field("fsRead", fs_read);
    set_field("fsWrite", fs_write);
    set_field("ipcSent", ipc_sent);
    set_field("ipcReceived", ipc_recv);
    set_field("signalsCount", signals);
    set_field("voluntaryContextSwitches", vcsw);
    set_field("involuntaryContextSwitches", ivcsw);
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// process.title -> string. Returns the value set via the setter, or
/// falls back to argv[0].
#[no_mangle]
pub extern "C" fn js_process_title() -> f64 {
    use crate::value::JSValue;
    let stored: Option<String> = PROCESS_TITLE.with(|c| c.borrow().clone());
    let s = stored.unwrap_or_else(|| std::env::args().next().unwrap_or_default());
    let bytes = s.as_bytes();
    let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

/// process.title = value — coerces to string and stores in the cell.
#[no_mangle]
pub extern "C" fn js_process_set_title(value: f64) {
    let ptr = crate::value::js_jsvalue_to_string(value);
    let s = if ptr.is_null() {
        String::new()
    } else {
        unsafe {
            let header = &*ptr;
            let len = header.byte_len as usize;
            let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
        }
    };
    #[cfg(target_os = "linux")]
    {
        let mut buf = [0i8; 16];
        let src = s.as_bytes();
        let copy_len = std::cmp::min(src.len(), 15);
        for i in 0..copy_len {
            buf[i] = src[i] as i8;
        }
        unsafe {
            libc::prctl(libc::PR_SET_NAME, buf.as_ptr() as libc::c_ulong, 0, 0, 0);
        }
    }
    PROCESS_TITLE.with(|c| *c.borrow_mut() = Some(s));
}

/// process.umask() -> number. Returns the current file-mode creation mask
/// without modifying it. POSIX's `umask` syscall has no read-only form, so
/// we set the mask to 0, capture the previous value, then restore it.
#[no_mangle]
pub extern "C" fn js_process_umask() -> f64 {
    #[cfg(unix)]
    unsafe {
        let prev = libc::umask(0);
        libc::umask(prev);
        prev as f64
    }
    #[cfg(not(unix))]
    {
        0.0
    }
}

/// process.umask(mask) -> number. Validates and parses `mask` the way Node's
/// `process.umask` (`parseMode`) does, sets the file-mode creation mask, and
/// returns the previous value (#2920).
///
/// Node accepts either a 32-bit unsigned integer or an octal string:
/// - a non-number / non-string (`null`, object, boolean, …) throws
///   `TypeError [ERR_INVALID_ARG_TYPE]` ("must be of type number"); `null`
///   reports as `Received undefined` to match Node's `parseMode`;
/// - an octal string (`"077"`) is parsed via radix-8 `parseInt`; a string that
///   is not all-octal-digits (empty, `"abc"`, `"8"`, `"0xff"`, leading/trailing
///   whitespace) throws `TypeError [ERR_INVALID_ARG_VALUE]`;
/// - a non-integer / `NaN` / `Infinity` number throws
///   `RangeError [ERR_OUT_OF_RANGE]` ("must be an integer");
/// - a value `< 0` or `> 4294967295` (either form) throws
///   `RangeError [ERR_OUT_OF_RANGE]` ("must be >= 0 && <= 4294967295").
///
/// An explicit `undefined` is handled at the call site as the read-only
/// no-argument form (so `js_process_umask` is called instead), matching Node's
/// `umask(undefined)` no-op-returns-current behavior.
#[no_mangle]
pub extern "C" fn js_process_umask_set(mask: f64) -> f64 {
    // An explicit `undefined` argument is the read-only form (Node:
    // `umask(undefined)` returns the current mask without changing it).
    if JSValue::from_bits(mask.to_bits()).is_undefined() {
        return js_process_umask();
    }
    let parsed = parse_umask_mask(mask);
    #[cfg(unix)]
    unsafe {
        libc::umask(parsed as libc::mode_t) as f64
    }
    #[cfg(not(unix))]
    {
        let _ = parsed;
        0.0
    }
}

/// Node's `parseMode("mask", value)` for `process.umask`. Diverges via
/// `js_throw` on an invalid value; otherwise returns the validated 32-bit
/// unsigned mask.
fn parse_umask_mask(mask: f64) -> u32 {
    use crate::fs::validate::{
        describe_received, is_numeric, throw_range_error_named, throw_type_error_with_code,
    };
    let jv = JSValue::from_bits(mask.to_bits());

    if jv.is_any_string() {
        let s = read_js_string_lossy(mask);
        // Node parses the string with radix 8 (`parseInt(str, 8)`) but only
        // after asserting the whole string is octal digits — leading/trailing
        // whitespace, prefixes, empty, or non-octal chars are rejected.
        let valid = !s.is_empty() && s.bytes().all(|b| (b'0'..=b'7').contains(&b));
        let parsed = if valid {
            u64::from_str_radix(&s, 8).ok()
        } else {
            None
        };
        match parsed {
            Some(n) if n <= u32::MAX as u64 => return n as u32,
            Some(n) => {
                let message = format!(
                    "The value of \"mask\" is out of range. It must be >= 0 && <= 4294967295. Received {}",
                    n
                );
                throw_range_error_named(&message, "ERR_OUT_OF_RANGE");
            }
            None => {
                let message = format!(
                    "The argument 'mask' must be a 32-bit unsigned integer or an octal string. Received '{}'",
                    s
                );
                throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE");
            }
        }
    }

    if !is_numeric(jv) {
        // Node's `parseMode` treats `null` like a missing value here, so its
        // ERR_INVALID_ARG_TYPE renders `Received undefined`.
        let received = if jv.is_null() {
            "undefined".to_string()
        } else {
            describe_received(mask)
        };
        let message = format!(
            "The \"mask\" argument must be of type number. Received {}",
            received
        );
        throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }

    let n = if jv.is_int32() {
        jv.as_int32() as f64
    } else {
        jv.as_number()
    };
    if !(n.is_finite() && n.fract() == 0.0) {
        let message = format!(
            "The value of \"mask\" is out of range. It must be an integer. Received {}",
            format_out_of_range_number(n)
        );
        throw_range_error_named(&message, "ERR_OUT_OF_RANGE");
    }
    if n < 0.0 || n > u32::MAX as f64 {
        let message = format!(
            "The value of \"mask\" is out of range. It must be >= 0 && <= 4294967295. Received {}",
            format_out_of_range_number(n)
        );
        throw_range_error_named(&message, "ERR_OUT_OF_RANGE");
    }
    n as u32
}

/// Render a number the way Node prints the `Received …` clause of an
/// `ERR_OUT_OF_RANGE` message (no `type number (...)` wrapper).
pub(crate) fn format_out_of_range_number(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        }
        .to_string();
    }
    if n.fract() == 0.0 && n.abs() < 1e21 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

/// Read a JS string (heap `StringHeader` or inline SSO) into a Rust `String`.
fn read_js_string_lossy(value: f64) -> String {
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

// Codegen emits these two entry points only from generated `.o` (see the
// process native table). Pin retained-reference edges so the auto-optimize
// whole-program build doesn't internalize + dead-strip them. Same rationale
// as KEEP_JS_SETENV above.
#[used]
static KEEP_JS_PROCESS_SOURCE_MAPS_ENABLED: extern "C" fn() -> f64 = js_process_source_maps_enabled;
#[used]
static KEEP_JS_PROCESS_SET_SOURCE_MAPS_ENABLED: extern "C" fn(f64) -> f64 =
    js_process_set_source_maps_enabled;
#[used]
static KEEP_JS_PROCESS_REF: extern "C" fn(f64) -> f64 = js_process_ref;
#[used]
static KEEP_JS_PROCESS_UNREF: extern "C" fn(f64) -> f64 = js_process_unref;
