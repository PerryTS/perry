//! Process module - provides access to environment and process information

use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64, ClosureHeader,
};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};

mod credentials;
pub use credentials::{
    js_process_getegid, js_process_geteuid, js_process_getgid, js_process_getgroups,
    js_process_getuid, js_process_initgroups, js_process_setegid, js_process_seteuid,
    js_process_setgid, js_process_setgroups, js_process_setuid,
};

static PROCESS_UNCAUGHT_CAPTURE_CALLBACK_SET: AtomicBool = AtomicBool::new(false);

fn bool_value(value: bool) -> f64 {
    f64::from_bits(if value {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    })
}

fn undefined_value() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

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

fn is_function_value(value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<u8>() as usize;
        if crate::closure::is_closure_ptr(ptr) {
            return true;
        }
    }
    crate::value::js_handle_is_function(value)
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
    let exit_code = match validate_exit_code(code) {
        Some(c) => c,
        None => 0,
    };
    js_process_run_finalization_exit();
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

fn supported_builtin_module_name(name: &str) -> Option<&str> {
    match name {
        "assert" | "assert/strict" | "async_hooks" | "buffer" | "child_process" | "cluster"
        | "console" | "constants" | "crypto" | "dns" | "dns/promises" | "events" | "fs"
        | "http" | "http2" | "https" | "net" | "os" | "path" | "perf_hooks" | "process"
        | "punycode" | "querystring" | "readline" | "readline/promises" | "sea" | "stream"
        | "stream/promises" | "string_decoder" | "sys" | "test" | "test/reporters" | "timers"
        | "timers/promises" | "tty" | "url" | "util" | "util/types" | "vm" | "worker_threads"
        | "zlib" => Some(name),
        _ => None,
    }
}

pub(crate) const MODULE_BUILTIN_MODULES: &[&str] = &[
    "_http_agent",
    "_http_client",
    "_http_common",
    "_http_incoming",
    "_http_outgoing",
    "_http_server",
    "_stream_duplex",
    "_stream_passthrough",
    "_stream_readable",
    "_stream_transform",
    "_stream_wrap",
    "_stream_writable",
    "_tls_common",
    "_tls_wrap",
    "assert",
    "assert/strict",
    "async_hooks",
    "buffer",
    "child_process",
    "cluster",
    "console",
    "constants",
    "crypto",
    "dgram",
    "diagnostics_channel",
    "dns",
    "dns/promises",
    "domain",
    "events",
    "fs",
    "fs/promises",
    "http",
    "http2",
    "https",
    "inspector",
    "inspector/promises",
    "module",
    "net",
    "node:sea",
    "node:sqlite",
    "node:test",
    "node:test/reporters",
    "os",
    "path",
    "path/posix",
    "path/win32",
    "perf_hooks",
    "process",
    "punycode",
    "querystring",
    "readline",
    "readline/promises",
    "repl",
    "stream",
    "stream/consumers",
    "stream/promises",
    "stream/web",
    "string_decoder",
    "sys",
    "timers",
    "timers/promises",
    "tls",
    "trace_events",
    "tty",
    "url",
    "util",
    "util/types",
    "v8",
    "vm",
    "wasi",
    "worker_threads",
    "zlib",
];

fn module_string_value(value: &str) -> f64 {
    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn module_object_value(obj: *mut crate::object::ObjectHeader) -> f64 {
    f64::from_bits(JSValue::object_ptr(obj as *mut u8).bits())
}

fn module_set_field(obj: *mut crate::object::ObjectHeader, name: &str, value: f64) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_set_field_by_name(obj, key, value);
}

type ModuleFunction1 = extern "C" fn(*const crate::closure::ClosureHeader, f64) -> f64;
type ModuleFunction2 = extern "C" fn(*const crate::closure::ClosureHeader, f64, f64) -> f64;

#[derive(Clone, Copy, Eq, PartialEq)]
enum ProcessFinalizationKind {
    Exit,
    BeforeExit,
}

#[derive(Clone, Copy)]
struct ProcessFinalizationEntry {
    obj: f64,
    callback: f64,
    kind: ProcessFinalizationKind,
}

thread_local! {
    static PROCESS_FINALIZATION_REGISTRY: RefCell<Vec<ProcessFinalizationEntry>> =
        RefCell::new(Vec::new());
    static PROCESS_FINALIZATION_BEFORE_EXIT_RAN: Cell<bool> = const { Cell::new(false) };
    static PROCESS_FINALIZATION_EXIT_RAN: Cell<bool> = const { Cell::new(false) };
    static PROCESS_FINALIZATION_OBJECT: Cell<f64> = const { Cell::new(0.0) };
    static PROCESS_FINALIZATION_BEFORE_EXIT_LISTENER: Cell<*const crate::closure::ClosureHeader> =
        const { Cell::new(std::ptr::null()) };
    static PROCESS_FINALIZATION_BEFORE_EXIT_LISTENER_INSTALLED: Cell<bool> =
        const { Cell::new(false) };
}

extern "C" fn module_source_map_noop(_closure: *const crate::closure::ClosureHeader) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn module_noop_function(name: &str) -> f64 {
    let func_ptr = module_source_map_noop as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 0);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    crate::value::js_nanbox_pointer(closure as i64)
}

fn module_function1(name: &str, thunk: ModuleFunction1, length: u32) -> f64 {
    let func_ptr = thunk as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 1);
    crate::closure::js_register_closure_length(func_ptr, length);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    crate::object::set_builtin_closure_length(closure as usize, length);
    crate::value::js_nanbox_pointer(closure as i64)
}

fn module_function2(name: &str, thunk: ModuleFunction2, length: u32) -> f64 {
    let func_ptr = thunk as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 2);
    crate::closure::js_register_closure_length(func_ptr, length);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    crate::object::set_builtin_closure_length(closure as usize, length);
    crate::value::js_nanbox_pointer(closure as i64)
}

extern "C" fn process_finalization_before_exit_listener(
    _closure: *const crate::closure::ClosureHeader,
    _code: f64,
) -> f64 {
    js_process_run_finalization_before_exit();
    undefined_value()
}

fn process_finalization_before_exit_listener_ptr() -> *const crate::closure::ClosureHeader {
    PROCESS_FINALIZATION_BEFORE_EXIT_LISTENER.with(|cell| {
        let existing = cell.get();
        if !existing.is_null() {
            return existing;
        }
        let func_ptr = process_finalization_before_exit_listener as *const u8;
        crate::closure::js_register_closure_arity(func_ptr, 1);
        crate::closure::js_register_closure_length(func_ptr, 1);
        let closure = crate::closure::js_closure_alloc(func_ptr, 0);
        crate::object::set_bound_native_closure_name(closure, "processFinalizationBeforeExit");
        crate::object::set_builtin_closure_length(closure as usize, 1);
        cell.set(closure);
        closure
    })
}

fn process_finalization_has_before_exit_entries() -> bool {
    PROCESS_FINALIZATION_REGISTRY.with(|registry| {
        registry
            .borrow()
            .iter()
            .any(|entry| entry.kind == ProcessFinalizationKind::BeforeExit)
    })
}

fn ensure_process_finalization_before_exit_listener() {
    let callback = process_finalization_before_exit_listener_ptr();
    PROCESS_FINALIZATION_BEFORE_EXIT_LISTENER_INSTALLED.with(|installed| {
        if installed.replace(true) {
            return;
        }
        crate::os::add_internal_process_listener("beforeExit", callback);
    });
}

fn sync_process_finalization_before_exit_listener() {
    if process_finalization_has_before_exit_entries() {
        ensure_process_finalization_before_exit_listener();
        return;
    }
    let callback = PROCESS_FINALIZATION_BEFORE_EXIT_LISTENER.with(|cell| cell.get());
    PROCESS_FINALIZATION_BEFORE_EXIT_LISTENER_INSTALLED.with(|installed| {
        if !installed.replace(false) {
            return;
        }
        crate::os::remove_internal_process_listener("beforeExit", callback);
    });
}

fn process_finalization_ref_is_valid(value: f64) -> bool {
    if is_function_value(value) {
        return true;
    }
    if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        return false;
    }
    module_object_ptr(value).is_some()
}

fn validate_process_finalization_ref(value: f64) {
    if process_finalization_ref_is_valid(value) {
        return;
    }
    let message = format!(
        "The \"obj\" argument must be of type object. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

fn process_finalization_register(kind: ProcessFinalizationKind, obj: f64, callback: f64) -> f64 {
    validate_process_finalization_ref(obj);
    PROCESS_FINALIZATION_REGISTRY.with(|registry| {
        registry.borrow_mut().push(ProcessFinalizationEntry {
            obj,
            callback,
            kind,
        });
    });
    if kind == ProcessFinalizationKind::BeforeExit {
        ensure_process_finalization_before_exit_listener();
    }
    undefined_value()
}

fn process_finalization_unregister(obj: f64) -> f64 {
    let obj_bits = obj.to_bits();
    PROCESS_FINALIZATION_REGISTRY.with(|registry| {
        registry
            .borrow_mut()
            .retain(|entry| entry.obj.to_bits() != obj_bits);
    });
    sync_process_finalization_before_exit_listener();
    undefined_value()
}

fn process_finalization_mark_ran(kind: ProcessFinalizationKind) -> bool {
    match kind {
        ProcessFinalizationKind::BeforeExit => {
            PROCESS_FINALIZATION_BEFORE_EXIT_RAN.with(|ran| ran.replace(true))
        }
        ProcessFinalizationKind::Exit => {
            PROCESS_FINALIZATION_EXIT_RAN.with(|ran| ran.replace(true))
        }
    }
}

fn process_finalization_event_name(kind: ProcessFinalizationKind) -> &'static str {
    match kind {
        ProcessFinalizationKind::BeforeExit => "beforeExit",
        ProcessFinalizationKind::Exit => "exit",
    }
}

fn run_process_finalization_callbacks(kind: ProcessFinalizationKind) {
    if process_finalization_mark_ran(kind) {
        return;
    }
    let entries = PROCESS_FINALIZATION_REGISTRY.with(|registry| {
        registry
            .borrow()
            .iter()
            .filter(|entry| entry.kind == kind)
            .copied()
            .collect::<Vec<_>>()
    });
    if entries.is_empty() {
        return;
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let event_handle =
        scope.root_nanbox_f64(module_string_value(process_finalization_event_name(kind)));
    let handles = entries
        .iter()
        .map(|entry| {
            (
                scope.root_nanbox_f64(entry.obj),
                scope.root_nanbox_f64(entry.callback),
            )
        })
        .collect::<Vec<_>>();

    for (obj_handle, callback_handle) in handles {
        let callback = callback_handle.get_nanbox_f64();
        if !is_function_value(callback) {
            crate::closure::throw_not_callable();
        }
        let args = [obj_handle.get_nanbox_f64(), event_handle.get_nanbox_f64()];
        unsafe {
            crate::closure::js_native_call_value(callback, args.as_ptr(), args.len());
        }
    }
}

pub fn scan_process_finalization_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    PROCESS_FINALIZATION_OBJECT.with(|cell| {
        let mut value = cell.get();
        if value != 0.0 && visitor.visit_nanbox_f64_slot(&mut value) {
            cell.set(value);
        }
    });
    PROCESS_FINALIZATION_REGISTRY.with(|registry| {
        for entry in registry.borrow_mut().iter_mut() {
            visitor.visit_nanbox_f64_slot(&mut entry.obj);
            visitor.visit_nanbox_f64_slot(&mut entry.callback);
        }
    });
    PROCESS_FINALIZATION_BEFORE_EXIT_LISTENER.with(|cell| {
        let mut callback = cell.get();
        if !callback.is_null() && visitor.visit_raw_const_ptr_slot(&mut callback) {
            cell.set(callback);
        }
    });
}

extern "C" fn process_finalization_register_function(
    _closure: *const crate::closure::ClosureHeader,
    obj: f64,
    callback: f64,
) -> f64 {
    process_finalization_register(ProcessFinalizationKind::Exit, obj, callback)
}

extern "C" fn process_finalization_register_before_exit_function(
    _closure: *const crate::closure::ClosureHeader,
    obj: f64,
    callback: f64,
) -> f64 {
    process_finalization_register(ProcessFinalizationKind::BeforeExit, obj, callback)
}

extern "C" fn process_finalization_unregister_function(
    _closure: *const crate::closure::ClosureHeader,
    obj: f64,
) -> f64 {
    process_finalization_unregister(obj)
}

#[no_mangle]
pub extern "C" fn js_process_run_finalization_before_exit() {
    run_process_finalization_callbacks(ProcessFinalizationKind::BeforeExit);
}

#[no_mangle]
pub extern "C" fn js_process_run_finalization_exit() {
    run_process_finalization_callbacks(ProcessFinalizationKind::Exit);
}

fn module_array_value(items: &[&str]) -> f64 {
    let arr = crate::array::js_array_alloc_with_length(items.len() as u32);
    for (i, item) in items.iter().enumerate() {
        crate::array::js_array_set_f64(arr, i as u32, module_string_value(item));
    }
    f64::from_bits(JSValue::array_ptr(arr).bits())
}

fn module_set_value(items: &[&str]) -> f64 {
    let mut set = crate::set::js_set_alloc(items.len() as u32);
    for item in items {
        set = crate::set::js_set_add(set, module_string_value(item));
    }
    crate::value::js_nanbox_pointer(set as i64)
}

fn process_argv0_string() -> String {
    std::env::args().next().unwrap_or_default()
}

fn node_arch_name() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "arm" => "arm",
        "x86" | "i386" | "i686" => "ia32",
        "powerpc64" => "ppc64",
        "riscv64" => "riscv64",
        "s390x" => "s390x",
        _ => std::env::consts::ARCH,
    }
}

fn process_release_value() -> f64 {
    let obj = crate::object::js_object_alloc(0, 3);
    module_set_field(obj, "name", module_string_value("node"));
    module_set_field(obj, "sourceUrl", module_string_value(""));
    module_set_field(obj, "headersUrl", module_string_value(""));
    module_object_value(obj)
}

fn process_features_value() -> f64 {
    let obj = crate::object::js_object_alloc(0, 13);
    module_set_field(obj, "inspector", bool_value(false));
    module_set_field(obj, "debug", bool_value(false));
    module_set_field(obj, "uv", bool_value(true));
    module_set_field(obj, "ipv6", bool_value(true));
    module_set_field(obj, "tls_alpn", bool_value(true));
    module_set_field(obj, "tls_sni", bool_value(true));
    module_set_field(obj, "tls_ocsp", bool_value(true));
    module_set_field(obj, "tls", bool_value(true));
    module_set_field(obj, "openssl_is_boringssl", bool_value(false));
    module_set_field(obj, "cached_builtins", bool_value(false));
    module_set_field(obj, "require_module", bool_value(false));
    module_set_field(obj, "quic", bool_value(false));
    module_set_field(obj, "typescript", module_string_value("transform"));
    module_object_value(obj)
}

fn process_finalization_value() -> f64 {
    let cached = PROCESS_FINALIZATION_OBJECT.with(|c| c.get());
    if cached != 0.0 {
        return cached;
    }

    let obj = crate::object::js_object_alloc(0, 3);
    module_set_field(
        obj,
        "register",
        module_function2("register", process_finalization_register_function, 2),
    );
    module_set_field(
        obj,
        "registerBeforeExit",
        module_function2(
            "registerBeforeExit",
            process_finalization_register_before_exit_function,
            2,
        ),
    );
    module_set_field(
        obj,
        "unregister",
        module_function1("unregister", process_finalization_unregister_function, 1),
    );
    let value = module_object_value(obj);
    PROCESS_FINALIZATION_OBJECT.with(|c| c.set(value));
    value
}

extern "C" fn process_report_function_get_report(
    _closure: *const crate::closure::ClosureHeader,
    err: f64,
) -> f64 {
    validate_report_error_arg(err);
    process_report_object("GetReport", None)
}

extern "C" fn process_report_function_write_report(
    _closure: *const crate::closure::ClosureHeader,
    file: f64,
    err: f64,
) -> f64 {
    let mut file_arg = file;
    let mut err_arg = err;
    let file_value = JSValue::from_bits(file_arg.to_bits());

    if !file_value.is_undefined() && !file_value.is_any_string() {
        if module_object_ptr(file_arg).is_some() {
            err_arg = file_arg;
            file_arg = undefined_value();
        } else {
            throw_report_invalid_arg_type("file", "string", file_arg);
        }
    }

    validate_report_error_arg(err_arg);

    let filename = module_value_to_string(file_arg)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(process_report_default_filename);
    let report_json = process_report_json_string("API", Some(&filename));
    if let Err(err) = std::fs::write(&filename, report_json) {
        crate::fs::validate::throw_type_error_with_code(
            &format!("Failed to write diagnostic report to {filename}: {err}"),
            "ERR_REPORT_WRITE_FAILED",
        );
    }

    eprintln!("\nWriting Node.js report to file: {filename}");
    eprintln!("Node.js report completed");
    module_string_value(&filename)
}

fn validate_report_error_arg(value: f64) {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_undefined() {
        return;
    }
    if module_object_ptr(value).is_none() {
        throw_report_invalid_arg_type("err", "object", value);
    }
}

fn throw_report_invalid_arg_type(name: &str, expected: &str, value: f64) -> ! {
    let message = format!(
        "The \"{}\" argument must be of type {}. Received {}",
        name,
        expected,
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn process_report_default_filename() -> String {
    format!("report.{}.json", std::process::id())
}

fn process_report_value() -> f64 {
    use std::cell::Cell;
    thread_local! {
        static CACHED_REPORT: Cell<f64> = const { Cell::new(0.0) };
    }

    let cached = CACHED_REPORT.with(|c| c.get());
    if cached != 0.0 {
        return cached;
    }

    let obj = process_report_controller_object();
    CACHED_REPORT.with(|c| c.set(obj));
    obj
}

pub(crate) fn process_permission_enabled() -> bool {
    let mut enabled = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--permission" => enabled = true,
            "--no-permission" => enabled = false,
            _ => {}
        }
    }
    enabled
}

fn process_permission_flag_values(flag: &str) -> Vec<String> {
    let mut values = Vec::new();
    let prefix = format!("{flag}=");
    let mut args = std::env::args().skip(1).peekable();
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix(&prefix) {
            values.extend(
                value
                    .split(',')
                    .filter(|part| !part.is_empty())
                    .map(|part| part.to_string()),
            );
        } else if arg == flag {
            if let Some(next) = args.peek() {
                if !next.starts_with("--") {
                    if let Some(value) = args.next() {
                        values.extend(
                            value
                                .split(',')
                                .filter(|part| !part.is_empty())
                                .map(|part| part.to_string()),
                        );
                    }
                } else {
                    values.push("*".to_string());
                }
            } else {
                values.push("*".to_string());
            }
        }
    }
    values
}

fn process_permission_has_flag(flag: &str) -> bool {
    std::env::args().skip(1).any(|arg| arg == flag)
}

fn permission_canonical_path(path: &str) -> Option<std::path::PathBuf> {
    std::fs::canonicalize(path).ok()
}

fn permission_path_allowed(reference: &str, allowed: &[String]) -> bool {
    if allowed.iter().any(|entry| entry == "*") {
        return true;
    }
    let reference_path = permission_canonical_path(reference);
    for entry in allowed {
        if entry == reference {
            return true;
        }
        if let (Some(reference_path), Some(allowed_path)) =
            (reference_path.as_ref(), permission_canonical_path(entry))
        {
            if reference_path == &allowed_path || reference_path.starts_with(&allowed_path) {
                return true;
            }
        }
    }
    false
}

fn process_permission_scope_allowed(scope: &str, reference: Option<&str>) -> bool {
    match scope {
        "fs.read" => {
            let allowed = process_permission_flag_values("--allow-fs-read");
            match reference {
                Some(reference) => permission_path_allowed(reference, &allowed),
                None => allowed.iter().any(|entry| entry == "*"),
            }
        }
        "fs.write" => {
            let allowed = process_permission_flag_values("--allow-fs-write");
            match reference {
                Some(reference) => permission_path_allowed(reference, &allowed),
                None => allowed.iter().any(|entry| entry == "*"),
            }
        }
        "child" => process_permission_has_flag("--allow-child-process"),
        "worker" => process_permission_has_flag("--allow-worker"),
        "addon" => process_permission_has_flag("--allow-addons"),
        _ => false,
    }
}

fn throw_permission_arg_type(name: &str, value: f64) -> ! {
    let message = format!(
        "The \"{}\" argument must be of type string. Received {}",
        name,
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

extern "C" fn process_permission_has_thunk(
    _closure: *const crate::closure::ClosureHeader,
    scope_value: f64,
    reference_value: f64,
) -> f64 {
    let Some(scope) = module_value_to_string(scope_value) else {
        throw_permission_arg_type("scope", scope_value);
    };
    let reference_js = JSValue::from_bits(reference_value.to_bits());
    let reference = if reference_js.is_undefined() || reference_js.is_null() {
        None
    } else if let Some(reference) = module_value_to_string_or_buffer(reference_value) {
        Some(reference)
    } else {
        throw_permission_arg_type("reference", reference_value);
    };
    bool_value(process_permission_scope_allowed(
        &scope,
        reference.as_deref(),
    ))
}

fn process_permission_value() -> Option<f64> {
    if !process_permission_enabled() {
        return None;
    }
    use std::cell::Cell;
    thread_local! {
        static CACHED_PERMISSION: Cell<f64> = const { Cell::new(0.0) };
    }

    let cached = CACHED_PERMISSION.with(|c| c.get());
    if cached != 0.0 {
        return Some(cached);
    }

    let obj = crate::object::js_object_alloc(0, 1);
    module_set_field(
        obj,
        "has",
        module_function2("has", process_permission_has_thunk, 2),
    );
    let value = module_object_value(obj);
    CACHED_PERMISSION.with(|c| c.set(value));
    crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    Some(value)
}

fn process_report_controller_object() -> f64 {
    let obj = crate::object::js_object_alloc(0, 11);
    module_set_field(obj, "compact", bool_value(false));
    module_set_field(obj, "directory", module_string_value(""));
    module_set_field(obj, "excludeEnv", bool_value(false));
    module_set_field(obj, "excludeNetwork", bool_value(false));
    module_set_field(obj, "filename", module_string_value(""));
    module_set_field(
        obj,
        "getReport",
        module_function1("getReport", process_report_function_get_report, 1),
    );
    module_set_field(obj, "reportOnFatalError", bool_value(false));
    module_set_field(obj, "reportOnSignal", bool_value(false));
    module_set_field(obj, "reportOnUncaughtException", bool_value(false));
    module_set_field(obj, "signal", module_string_value("SIGUSR2"));
    module_set_field(
        obj,
        "writeReport",
        module_function2("writeReport", process_report_function_write_report, 2),
    );
    module_object_value(obj)
}

fn process_report_object(trigger: &str, filename: Option<&str>) -> f64 {
    let obj = crate::object::js_object_alloc(0, 11);
    module_set_field(
        obj,
        "header",
        process_report_header_object(trigger, filename),
    );
    module_set_field(
        obj,
        "javascriptStack",
        process_report_javascript_stack_object(),
    );
    module_set_field(
        obj,
        "javascriptHeap",
        process_report_javascript_heap_object(),
    );
    module_set_field(obj, "nativeStack", module_array_value(&[]));
    module_set_field(obj, "resourceUsage", process_report_resource_usage_object());
    module_set_field(
        obj,
        "uvthreadResourceUsage",
        process_report_thread_resource_usage_object(),
    );
    module_set_field(obj, "libuv", module_array_value(&[]));
    module_set_field(obj, "workers", module_array_value(&[]));
    module_set_field(
        obj,
        "environmentVariables",
        module_object_value(crate::object::js_object_alloc(0, 0)),
    );
    module_set_field(obj, "userLimits", process_report_user_limits_object());
    module_set_field(obj, "sharedObjects", module_array_value(&[]));
    module_object_value(obj)
}

fn process_report_header_object(trigger: &str, filename: Option<&str>) -> f64 {
    let obj = crate::object::js_object_alloc(0, 22);
    let now_ms = process_report_unix_time_ms();
    module_set_field(obj, "reportVersion", 5.0);
    module_set_field(obj, "event", module_string_value("JavaScript API"));
    module_set_field(obj, "trigger", module_string_value(trigger));
    module_set_field(obj, "filename", module_string_value(filename.unwrap_or("")));
    module_set_field(
        obj,
        "dumpEventTime",
        module_string_value(&format!("{:.0}", now_ms / 1000.0)),
    );
    module_set_field(obj, "dumpEventTimeStamp", now_ms);
    module_set_field(obj, "processId", std::process::id() as f64);
    module_set_field(obj, "threadId", 0.0);
    module_set_field(
        obj,
        "cwd",
        module_string_value(&std::env::current_dir().map_or_else(
            |_| String::new(),
            |path| path.to_string_lossy().into_owned(),
        )),
    );
    module_set_field(obj, "commandLine", process_report_command_line_array());
    module_set_field(obj, "nodejsVersion", module_string_value("v22.0.0"));
    module_set_field(obj, "wordSize", (std::mem::size_of::<usize>() * 8) as f64);
    module_set_field(obj, "arch", module_string_value(node_arch_name()));
    module_set_field(obj, "platform", module_string_value(node_platform_name()));
    module_set_field(
        obj,
        "componentVersions",
        process_report_component_versions(),
    );
    module_set_field(obj, "release", process_release_value());
    module_set_field(obj, "osName", module_string_value(std::env::consts::OS));
    module_set_field(obj, "osRelease", module_string_value(""));
    module_set_field(obj, "osVersion", module_string_value(""));
    module_set_field(
        obj,
        "osMachine",
        module_string_value(std::env::consts::ARCH),
    );
    module_set_field(obj, "host", module_string_value(""));
    module_object_value(obj)
}

fn process_report_javascript_stack_object() -> f64 {
    let obj = crate::object::js_object_alloc(0, 3);
    module_set_field(obj, "message", module_string_value(""));
    module_set_field(obj, "stack", module_array_value(&[]));
    module_set_field(
        obj,
        "errorProperties",
        module_object_value(crate::object::js_object_alloc(0, 0)),
    );
    module_object_value(obj)
}

fn process_report_javascript_heap_object() -> f64 {
    let mut heap_used: u64 = 0;
    let mut heap_total: u64 = 0;
    crate::arena::js_arena_stats(&mut heap_used, &mut heap_total);

    let obj = crate::object::js_object_alloc(0, 8);
    module_set_field(obj, "totalMemory", heap_total as f64);
    module_set_field(obj, "executableMemory", 0.0);
    module_set_field(obj, "totalCommittedMemory", heap_total as f64);
    module_set_field(obj, "availableMemory", js_process_available_memory());
    module_set_field(obj, "totalGlobalHandlesMemory", 0.0);
    module_set_field(obj, "usedGlobalHandlesMemory", 0.0);
    module_set_field(obj, "usedMemory", heap_used as f64);
    module_set_field(
        obj,
        "heapSpaces",
        module_object_value(crate::object::js_object_alloc(0, 0)),
    );
    module_object_value(obj)
}

fn process_report_resource_usage_object() -> f64 {
    let (user, system) = read_process_cpu_micros();
    let obj = crate::object::js_object_alloc(0, 6);
    module_set_field(obj, "userCpuSeconds", user / 1_000_000.0);
    module_set_field(obj, "kernelCpuSeconds", system / 1_000_000.0);
    module_set_field(obj, "cpuConsumptionPercent", 0.0);
    module_set_field(obj, "rss", get_rss_bytes() as f64);
    module_set_field(obj, "maxRss", get_rss_bytes() as f64);
    module_set_field(
        obj,
        "fsActivity",
        module_object_value(crate::object::js_object_alloc(0, 0)),
    );
    module_object_value(obj)
}

fn process_report_thread_resource_usage_object() -> f64 {
    let (user, system) = read_thread_cpu_micros();
    let obj = crate::object::js_object_alloc(0, 3);
    module_set_field(obj, "userCpuSeconds", user / 1_000_000.0);
    module_set_field(obj, "kernelCpuSeconds", system / 1_000_000.0);
    module_set_field(obj, "cpuConsumptionPercent", 0.0);
    module_object_value(obj)
}

fn process_report_user_limits_object() -> f64 {
    let obj = crate::object::js_object_alloc(0, 3);
    module_set_field(
        obj,
        "core_file_size_blocks",
        module_string_value("unlimited"),
    );
    module_set_field(obj, "data_size_kbytes", module_string_value("unlimited"));
    module_set_field(obj, "file_size_blocks", module_string_value("unlimited"));
    module_object_value(obj)
}

fn process_report_command_line_array() -> f64 {
    let args: Vec<String> = std::env::args().collect();
    let items = if args.is_empty() {
        vec![process_argv0_string()]
    } else {
        args
    };
    let arr = crate::array::js_array_alloc_with_length(items.len() as u32);
    for (i, item) in items.iter().enumerate() {
        crate::array::js_array_set_f64(arr, i as u32, module_string_value(item));
    }
    f64::from_bits(JSValue::array_ptr(arr).bits())
}

fn process_report_component_versions() -> f64 {
    let obj = crate::object::js_object_alloc(0, 4);
    module_set_field(obj, "node", module_string_value("22.0.0"));
    module_set_field(obj, "v8", module_string_value("12.4.254.21"));
    module_set_field(obj, "uv", module_string_value("1.51.0"));
    module_set_field(obj, "perry", module_string_value("0.4.71"));
    module_object_value(obj)
}

fn process_report_unix_time_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as f64)
        .unwrap_or(0.0)
}

fn node_platform_name() -> &'static str {
    match std::env::consts::OS {
        "macos" | "ios" => "darwin",
        "windows" => "win32",
        "linux" => "linux",
        "freebsd" => "freebsd",
        other => other,
    }
}

fn process_report_json_string(trigger: &str, filename: Option<&str>) -> String {
    let args: Vec<String> = std::env::args().collect();
    let command_line = if args.is_empty() {
        vec![process_argv0_string()]
    } else {
        args
    };
    let now_ms = process_report_unix_time_ms();
    let mut heap_used: u64 = 0;
    let mut heap_total: u64 = 0;
    crate::arena::js_arena_stats(&mut heap_used, &mut heap_total);
    let (proc_user, proc_system) = read_process_cpu_micros();
    let (thread_user, thread_system) = read_thread_cpu_micros();

    let value = serde_json::json!({
        "header": {
            "reportVersion": 5,
            "event": "JavaScript API",
            "trigger": trigger,
            "filename": filename.unwrap_or(""),
            "dumpEventTime": format!("{:.0}", now_ms / 1000.0),
            "dumpEventTimeStamp": now_ms,
            "processId": std::process::id(),
            "threadId": 0,
            "cwd": std::env::current_dir().map_or_else(
                |_| String::new(),
                |path| path.to_string_lossy().into_owned(),
            ),
            "commandLine": command_line,
            "nodejsVersion": "v22.0.0",
            "wordSize": std::mem::size_of::<usize>() * 8,
            "arch": node_arch_name(),
            "platform": node_platform_name(),
            "componentVersions": {
                "node": "22.0.0",
                "v8": "12.4.254.21",
                "uv": "1.51.0",
                "perry": "0.4.71"
            },
            "release": {
                "name": "node",
                "sourceUrl": "",
                "headersUrl": ""
            },
            "osName": std::env::consts::OS,
            "osRelease": "",
            "osVersion": "",
            "osMachine": std::env::consts::ARCH,
            "host": ""
        },
        "javascriptStack": {
            "message": "",
            "stack": [],
            "errorProperties": {}
        },
        "javascriptHeap": {
            "totalMemory": heap_total,
            "executableMemory": 0,
            "totalCommittedMemory": heap_total,
            "availableMemory": js_process_available_memory(),
            "totalGlobalHandlesMemory": 0,
            "usedGlobalHandlesMemory": 0,
            "usedMemory": heap_used,
            "heapSpaces": {}
        },
        "nativeStack": [],
        "resourceUsage": {
            "userCpuSeconds": proc_user / 1_000_000.0,
            "kernelCpuSeconds": proc_system / 1_000_000.0,
            "cpuConsumptionPercent": 0,
            "rss": get_rss_bytes(),
            "maxRss": get_rss_bytes(),
            "fsActivity": {}
        },
        "uvthreadResourceUsage": {
            "userCpuSeconds": thread_user / 1_000_000.0,
            "kernelCpuSeconds": thread_system / 1_000_000.0,
            "cpuConsumptionPercent": 0
        },
        "libuv": [],
        "workers": [],
        "environmentVariables": {},
        "userLimits": {
            "core_file_size_blocks": "unlimited",
            "data_size_kbytes": "unlimited",
            "file_size_blocks": "unlimited"
        },
        "sharedObjects": []
    });

    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
}

fn process_config_value() -> f64 {
    let config = crate::object::js_object_alloc(0, 2);
    let variables = crate::object::js_object_alloc(0, 10);
    let target_defaults = crate::object::js_object_alloc(0, 7);
    let configurations = crate::object::js_object_alloc(0, 1);

    module_set_field(
        variables,
        "target_arch",
        module_string_value(node_arch_name()),
    );
    module_set_field(
        variables,
        "host_arch",
        module_string_value(node_arch_name()),
    );
    module_set_field(variables, "node_module_version", 141.0);
    module_set_field(variables, "node_shared_openssl", bool_value(false));
    module_set_field(variables, "node_use_openssl", bool_value(true));
    module_set_field(variables, "node_use_node_code_cache", bool_value(false));
    module_set_field(variables, "node_use_node_snapshot", bool_value(false));
    module_set_field(variables, "v8_enable_i18n_support", 1.0);
    module_set_field(variables, "v8_enable_pointer_compression", 0.0);
    module_set_field(variables, "uv_parent_path", module_string_value(""));

    module_set_field(target_defaults, "cflags", module_array_value(&[]));
    module_set_field(target_defaults, "conditions", module_array_value(&[]));
    module_set_field(target_defaults, "defines", module_array_value(&[]));
    module_set_field(target_defaults, "include_dirs", module_array_value(&[]));
    module_set_field(target_defaults, "libraries", module_array_value(&[]));
    module_set_field(
        target_defaults,
        "default_configuration",
        module_string_value("Release"),
    );
    module_set_field(
        configurations,
        "Release",
        module_object_value(crate::object::js_object_alloc(0, 0)),
    );
    module_set_field(
        target_defaults,
        "configurations",
        module_object_value(configurations),
    );

    module_set_field(config, "variables", module_object_value(variables));
    module_set_field(
        config,
        "target_defaults",
        module_object_value(target_defaults),
    );
    module_object_value(config)
}

fn process_allowed_flags_value() -> f64 {
    const FLAGS: &[&str] = &[
        "--abort-on-uncaught-exception",
        "--addons",
        "--allow-addons",
        "--allow-child-process",
        "--allow-fs-read",
        "--allow-fs-write",
        "--allow-inspector",
        "--allow-net",
        "--allow-wasi",
        "--allow-worker",
        "--async-context-frame",
        "--conditions",
        "--cpu-prof",
        "--cpu-prof-dir",
        "--cpu-prof-interval",
        "--cpu-prof-name",
        "--debug-arraybuffer-allocations",
        "--debug-port",
        "--deprecation",
        "--diagnostic-dir",
        "--disable-proto",
        "--disable-sigusr1",
        "--disable-warning",
        "--disable-wasm-trap-handler",
        "--disallow-code-generation-from-strings",
        "--dns-result-order",
        "--enable-etw-stack-walking",
        "--enable-fips",
        "--enable-network-family-autoselection",
        "--enable-source-maps",
        "--entry-url",
        "--es-module-specifier-resolution",
        "--experimental-abortcontroller",
        "--experimental-addon-modules",
        "--experimental-detect-module",
        "--experimental-eventsource",
        "--experimental-fetch",
        "--experimental-global-customevent",
        "--experimental-global-navigator",
        "--experimental-global-webcrypto",
        "--experimental-import-meta-resolve",
        "--experimental-json-modules",
        "--experimental-loader",
        "--experimental-modules",
        "--experimental-print-required-tla",
        "--experimental-quic",
        "--experimental-repl-await",
        "--experimental-report",
        "--experimental-require-module",
        "--experimental-shadow-realm",
        "--experimental-specifier-resolution",
        "--experimental-sqlite",
        "--experimental-strip-types",
        "--experimental-test-isolation",
        "--experimental-top-level-await",
        "--experimental-transform-types",
        "--experimental-vm-modules",
        "--experimental-wasi-unstable-preview1",
        "--experimental-wasm-modules",
        "--experimental-websocket",
        "--experimental-webstorage",
        "--experimental-worker",
        "--expose-gc",
        "--extra-info-on-fatal-exception",
        "--force-async-hooks-checks",
        "--force-context-aware",
        "--force-fips",
        "--force-node-api-uncaught-exceptions-policy",
        "--frozen-intrinsics",
        "--global-search-paths",
        "--heap-prof",
        "--heap-prof-dir",
        "--heap-prof-interval",
        "--heap-prof-name",
        "--heapsnapshot-near-heap-limit",
        "--heapsnapshot-signal",
        "--http-parser",
        "--icu-data-dir",
        "--import",
        "--input-type",
        "--insecure-http-parser",
        "--inspect",
        "--inspect-brk",
        "--inspect-port",
        "--inspect-publish-uid",
        "--inspect-wait",
        "--interpreted-frames-native-stack",
        "--jitless",
        "--loader",
        "--localstorage-file",
        "--max-http-header-size",
        "--max-old-space-size",
        "--max-old-space-size-percentage",
        "--max-semi-space-size",
        "--napi-modules",
        "--network-family-autoselection",
        "--network-family-autoselection-attempt-timeout",
        "--no-addons",
        "--no-allow-addons",
        "--no-allow-child-process",
        "--no-allow-inspector",
        "--no-allow-net",
        "--no-allow-wasi",
        "--no-allow-worker",
        "--no-async-context-frame",
        "--no-cpu-prof",
        "--no-debug-arraybuffer-allocations",
        "--no-deprecation",
        "--no-disable-sigusr1",
        "--no-disable-wasm-trap-handler",
        "--no-enable-fips",
        "--no-enable-source-maps",
        "--no-entry-url",
        "--no-experimental-addon-modules",
        "--no-experimental-detect-module",
        "--no-experimental-eventsource",
        "--no-experimental-global-navigator",
        "--no-experimental-import-meta-resolve",
        "--no-experimental-print-required-tla",
        "--no-experimental-repl-await",
        "--no-experimental-require-module",
        "--no-experimental-shadow-realm",
        "--no-experimental-sqlite",
        "--no-experimental-transform-types",
        "--no-experimental-vm-modules",
        "--no-experimental-websocket",
        "--no-experimental-webstorage",
        "--no-extra-info-on-fatal-exception",
        "--no-force-async-hooks-checks",
        "--no-force-context-aware",
        "--no-force-fips",
        "--no-force-node-api-uncaught-exceptions-policy",
        "--no-frozen-intrinsics",
        "--no-global-search-paths",
        "--no-heap-prof",
        "--no-insecure-http-parser",
        "--no-inspect",
        "--no-inspect-brk",
        "--no-inspect-wait",
        "--no-network-family-autoselection",
        "--no-node-snapshot",
        "--no-openssl-legacy-provider",
        "--no-openssl-shared-config",
        "--no-pending-deprecation",
        "--no-permission",
        "--no-permission-audit",
        "--no-preserve-symlinks",
        "--no-preserve-symlinks-main",
        "--no-report-compact",
        "--no-report-exclude-env",
        "--no-report-exclude-network",
        "--no-report-on-fatalerror",
        "--no-report-on-signal",
        "--no-report-uncaught-exception",
        "--no-require-module",
        "--no-strip-types",
        "--no-test-only",
        "--no-throw-deprecation",
        "--no-tls-max-v1.2",
        "--no-tls-max-v1.3",
        "--no-tls-min-v1.0",
        "--no-tls-min-v1.1",
        "--no-tls-min-v1.2",
        "--no-tls-min-v1.3",
        "--no-trace-deprecation",
        "--no-trace-env",
        "--no-trace-env-js-stack",
        "--no-trace-env-native-stack",
        "--no-trace-exit",
        "--no-trace-promises",
        "--no-trace-sigint",
        "--no-trace-sync-io",
        "--no-trace-tls",
        "--no-trace-uncaught",
        "--no-trace-warnings",
        "--no-track-heap-objects",
        "--no-use-bundled-ca",
        "--no-use-env-proxy",
        "--no-use-openssl-ca",
        "--no-use-system-ca",
        "--no-verify-base-objects",
        "--no-warnings",
        "--no-watch",
        "--no-watch-preserve-output",
        "--no-zero-fill-buffers",
        "--node-memory-debug",
        "--node-snapshot",
        "--openssl-config",
        "--openssl-legacy-provider",
        "--openssl-shared-config",
        "--pending-deprecation",
        "--perf-basic-prof",
        "--perf-basic-prof-only-functions",
        "--perf-prof",
        "--perf-prof-unwinding-info",
        "--permission",
        "--permission-audit",
        "--preserve-symlinks",
        "--preserve-symlinks-main",
        "--prof-process",
        "--redirect-warnings",
        "--report-compact",
        "--report-dir",
        "--report-directory",
        "--report-exclude-env",
        "--report-exclude-network",
        "--report-filename",
        "--report-on-fatalerror",
        "--report-on-signal",
        "--report-signal",
        "--report-uncaught-exception",
        "--require",
        "--require-module",
        "--secure-heap",
        "--secure-heap-min",
        "--snapshot-blob",
        "--stack-trace-limit",
        "--strip-types",
        "--test-coverage-branches",
        "--test-coverage-exclude",
        "--test-coverage-functions",
        "--test-coverage-include",
        "--test-coverage-lines",
        "--test-global-setup",
        "--test-isolation",
        "--test-name-pattern",
        "--test-only",
        "--test-reporter",
        "--test-reporter-destination",
        "--test-rerun-failures",
        "--test-shard",
        "--test-skip-pattern",
        "--throw-deprecation",
        "--title",
        "--tls-cipher-list",
        "--tls-keylog",
        "--tls-max-v1.2",
        "--tls-max-v1.3",
        "--tls-min-v1.0",
        "--tls-min-v1.1",
        "--tls-min-v1.2",
        "--tls-min-v1.3",
        "--trace-deprecation",
        "--trace-env",
        "--trace-env-js-stack",
        "--trace-env-native-stack",
        "--trace-event-categories",
        "--trace-event-file-pattern",
        "--trace-events-enabled",
        "--trace-exit",
        "--trace-promises",
        "--trace-require-module",
        "--trace-sigint",
        "--trace-sync-io",
        "--trace-tls",
        "--trace-uncaught",
        "--trace-warnings",
        "--track-heap-objects",
        "--unhandled-rejections",
        "--use-bundled-ca",
        "--use-env-proxy",
        "--use-largepages",
        "--use-openssl-ca",
        "--use-system-ca",
        "--v8-pool-size",
        "--verify-base-objects",
        "--warnings",
        "--watch",
        "--watch-kill-signal",
        "--watch-path",
        "--watch-preserve-output",
        "--webstorage",
        "--zero-fill-buffers",
        "-C",
        "-r",
    ];
    module_set_value(FLAGS)
}

pub fn process_metadata_property(property: &str) -> Option<f64> {
    Some(match property {
        "allowedNodeEnvironmentFlags" => process_allowed_flags_value(),
        "argv0" | "execPath" => module_string_value(&process_argv0_string()),
        "config" => process_config_value(),
        "debugPort" => 9229.0,
        "execArgv" | "moduleLoadList" => module_array_value(&[]),
        "features" => process_features_value(),
        "finalization" => process_finalization_value(),
        "permission" => process_permission_value()?,
        "release" => process_release_value(),
        "report" => process_report_value(),
        "sourceMapsEnabled" => js_process_source_maps_enabled(),
        "title" => js_process_title(),
        "_eval" => undefined_value(),
        "_events" => empty_object_value(),
        "_eventsCount" => 0.0,
        "_exiting" => bool_value(false),
        "_maxListeners" => undefined_value(),
        "_preload_modules" => module_array_value(&[]),
        "domain" => active_domain_value(),
        _ => return None,
    })
}

fn active_domain_value() -> f64 {
    let ptr = crate::value::JS_NATIVE_DOMAIN_DISPATCH.load(Ordering::SeqCst);
    if ptr.is_null() {
        return f64::from_bits(crate::value::TAG_NULL);
    }
    let dispatch: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64 =
        unsafe { std::mem::transmute(ptr) };
    unsafe { dispatch(b"active".as_ptr(), b"active".len(), std::ptr::null(), 0) }
}

/// `module.builtinModules` — Node exposes this as an Array of builtin module
/// specifiers. Perry's supported subset is smaller, but the public inventory
/// shape should still match Node's module API.
#[no_mangle]
pub extern "C" fn js_module_builtin_modules() -> f64 {
    let arr = crate::array::js_array_alloc_with_length(MODULE_BUILTIN_MODULES.len() as u32);
    for (i, name) in MODULE_BUILTIN_MODULES.iter().enumerate() {
        crate::array::js_array_set_f64(arr, i as u32, module_string_value(name));
    }
    f64::from_bits(JSValue::array_ptr(arr).bits())
}

/// Minimal `module.constants` shape. The compile-cache status values are not
/// backed by an actual bytecode cache in Perry, but Node exposes the enum as
/// stable process state for feature detection.
#[no_mangle]
pub extern "C" fn js_module_constants() -> f64 {
    let constants = crate::object::js_object_alloc(0, 1);
    let compile_cache_status = crate::object::js_object_alloc(0, 4);
    module_set_field(compile_cache_status, "FAILED", 0.0);
    module_set_field(compile_cache_status, "ENABLED", 1.0);
    module_set_field(compile_cache_status, "ALREADY_ENABLED", 2.0);
    module_set_field(compile_cache_status, "DISABLED", 3.0);
    module_set_field(
        constants,
        "compileCacheStatus",
        module_object_value(compile_cache_status),
    );
    module_object_value(constants)
}

/// Constructor for `new module.SourceMap(payload)`. Preserves the payload
/// object and exposes working `findEntry`/`findOrigin` lookups. The bound
/// method closures capture the payload (slot 0) so the lookup thunks can
/// decode its `mappings`/`sources`/`names` without a separate `this` channel
/// (mirrors the dgram socket-method pattern). #3675.
#[no_mangle]
pub extern "C" fn js_module_source_map_new(payload: f64) -> f64 {
    let obj = crate::object::js_object_alloc(0, 3);
    module_set_field(obj, "payload", payload);
    module_set_field(
        obj,
        "findEntry",
        source_map_method(payload, "findEntry", source_map_find_entry_thunk),
    );
    module_set_field(
        obj,
        "findOrigin",
        source_map_method(payload, "findOrigin", source_map_find_origin_thunk),
    );
    module_object_value(obj)
}

type SourceMapThunk = extern "C" fn(*const ClosureHeader, f64) -> f64;

/// Build a bound SourceMap method closure that captures `payload` in slot 0
/// and packs all call arguments into a single rest array.
fn source_map_method(payload: f64, name: &str, thunk: SourceMapThunk) -> f64 {
    let func_ptr = thunk as *const u8;
    let closure = js_closure_alloc(func_ptr, 1);
    js_closure_set_capture_f64(closure, 0, payload);
    crate::closure::js_register_closure_rest(func_ptr, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    crate::value::js_nanbox_pointer(closure as i64)
}

/// Decode a base64 VLQ alphabet byte to its 0–63 value.
fn source_map_b64(c: u8) -> Option<i64> {
    match c {
        b'A'..=b'Z' => Some((c - b'A') as i64),
        b'a'..=b'z' => Some((c - b'a' + 26) as i64),
        b'0'..=b'9' => Some((c - b'0' + 52) as i64),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Decode one comma-delimited segment's VLQ fields.
fn source_map_decode_segment(seg: &[u8]) -> Vec<i64> {
    let mut out = Vec::new();
    let mut value: i64 = 0;
    let mut shift: u32 = 0;
    for &b in seg {
        let Some(digit) = source_map_b64(b) else {
            continue;
        };
        let cont = (digit & 0x20) != 0;
        value += (digit & 0x1f) << shift;
        if cont {
            shift += 5;
        } else {
            let negative = (value & 1) != 0;
            let decoded = value >> 1;
            out.push(if negative { -decoded } else { decoded });
            value = 0;
            shift = 0;
        }
    }
    out
}

#[derive(Clone, Copy)]
struct SourceMapEntry {
    generated_line: i64,
    generated_column: i64,
    // `None` for genCol-only (1-field) segments that mark an unmapped position.
    // The inner name index is `Some` only for segments that carried an explicit
    // 5th VLQ field (a named mapping).
    original: Option<(i64, i64, i64, Option<i64>)>, // (source_index, line, column, name_index)
}

/// Decode the full `mappings` string into ordered entries with cumulative
/// source/line/column/name indices per the Source Map v3 grammar. `name_index`
/// is attached only to genuinely-named (5-field) segments, matching how a
/// position with no explicit name resolves (Node returns no `name` for the
/// names-less mapping in the issue repro).
fn source_map_decode(mappings: &str) -> Vec<SourceMapEntry> {
    let mut entries = Vec::new();
    let (mut src_idx, mut src_line, mut src_col, mut name_idx) = (0i64, 0i64, 0i64, 0i64);
    for (gen_line, line) in mappings.split(';').enumerate() {
        let mut gen_col = 0i64;
        for seg in line.split(',') {
            if seg.is_empty() {
                continue;
            }
            let fields = source_map_decode_segment(seg.as_bytes());
            if fields.is_empty() {
                continue;
            }
            gen_col += fields[0];
            let original = if fields.len() >= 4 {
                src_idx += fields[1];
                src_line += fields[2];
                src_col += fields[3];
                let name = if fields.len() >= 5 {
                    name_idx += fields[4];
                    Some(name_idx)
                } else {
                    None
                };
                Some((src_idx, src_line, src_col, name))
            } else {
                None
            };
            entries.push(SourceMapEntry {
                generated_line: gen_line as i64,
                generated_column: gen_col,
                original,
            });
        }
    }
    entries
}

/// Read `payload.<field>` as a raw JSValue f64 (undefined when absent or when
/// the payload is not a heap object).
fn source_map_field(payload: f64, field: &str) -> f64 {
    let p = JSValue::from_bits(payload.to_bits());
    if !p.is_pointer() {
        return undefined_value();
    }
    let obj = crate::value::js_nanbox_get_pointer(payload) as *const crate::object::ObjectHeader;
    if obj.is_null() {
        return undefined_value();
    }
    let key = js_string_from_bytes(field.as_ptr(), field.len() as u32);
    let v = crate::object::js_object_get_field_by_name(obj, key);
    f64::from_bits(v.bits())
}

/// Read `payload.<field>` as a Rust string, if it is a string value.
fn source_map_field_string(payload: f64, field: &str) -> Option<String> {
    let value = JSValue::from_bits(source_map_field(payload, field).to_bits());
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let bytes = unsafe { crate::string::js_string_key_bytes(value, &mut sso) }?;
    Some(String::from_utf8_lossy(bytes).into_owned())
}

/// Read `payload.<arrayField>[index]` as a raw JSValue f64 (undefined when out
/// of range or not an array).
fn source_map_array_element(payload: f64, field: &str, index: i64) -> f64 {
    if index < 0 {
        return undefined_value();
    }
    let arr_value = source_map_field(payload, field);
    let av = JSValue::from_bits(arr_value.to_bits());
    if !av.is_pointer() {
        return undefined_value();
    }
    let arr = crate::value::js_nanbox_get_pointer(arr_value) as *const crate::array::ArrayHeader;
    if arr.is_null() {
        return undefined_value();
    }
    let len = crate::array::js_array_length(arr);
    if index as u32 >= len {
        return undefined_value();
    }
    crate::array::js_array_get_f64(arr, index as u32)
}

fn source_map_collect_args(rest: f64) -> Vec<f64> {
    let rv = JSValue::from_bits(rest.to_bits());
    if !rv.is_pointer() {
        return Vec::new();
    }
    let arr = crate::value::js_nanbox_get_pointer(rest) as *const crate::array::ArrayHeader;
    if arr.is_null() {
        return Vec::new();
    }
    let len = crate::array::js_array_length(arr);
    (0..len)
        .map(|i| crate::array::js_array_get_f64(arr, i))
        .collect()
}

/// Coerce call argument `idx` to a finite number, if it is one.
fn source_map_arg_number(args: &[f64], idx: usize) -> Option<f64> {
    args.get(idx)
        .map(|v| JSValue::from_bits(v.to_bits()).to_number())
        .filter(|n| n.is_finite())
}

fn source_map_arg_i64(args: &[f64], idx: usize) -> i64 {
    source_map_arg_number(args, idx)
        .map(|n| n as i64)
        .unwrap_or(0)
}

/// Decode the payload's `mappings` and return the greatest entry whose
/// generated position is `<=` (line, column). Entries are emitted in
/// non-decreasing order, so the last non-exceeding one wins.
fn source_map_lookup(payload: f64, line: i64, col: i64) -> Option<SourceMapEntry> {
    let mappings = source_map_field_string(payload, "mappings")?;
    let mut best = None;
    for entry in source_map_decode(&mappings) {
        if (entry.generated_line, entry.generated_column) <= (line, col) {
            best = Some(entry);
        } else {
            break;
        }
    }
    best
}

/// Build the `{ name?, fileName, lineNumber, columnNumber }` shape Node's
/// `findOrigin` echoes (name/fileName from the matched entry; line/column from
/// the call arguments). Insertion order matches Node for byte-identical JSON.
fn source_map_origin_object(
    payload: f64,
    entry: Option<SourceMapEntry>,
    line: Option<f64>,
    col: Option<f64>,
) -> f64 {
    let obj = crate::object::js_object_alloc(0, 4);
    if let Some(SourceMapEntry {
        original: Some((source_index, _, _, name_index)),
        ..
    }) = entry
    {
        if let Some(name_index) = name_index {
            let name = source_map_array_element(payload, "names", name_index);
            if JSValue::from_bits(name.to_bits()).is_string() {
                module_set_field(obj, "name", name);
            }
        }
        module_set_field(
            obj,
            "fileName",
            source_map_array_element(payload, "sources", source_index),
        );
    }
    let null = f64::from_bits(crate::value::TAG_NULL);
    module_set_field(obj, "lineNumber", line.map_or(null, |n| n));
    module_set_field(obj, "columnNumber", col.map_or(null, |n| n));
    module_object_value(obj)
}

/// `SourceMap#findEntry(lineNumber, columnNumber)` — return the greatest
/// decoded entry whose generated position is `<=` the query, shaped like
/// Node's `{ generatedLine, generatedColumn, originalSource, originalLine,
/// originalColumn, name? }`. Returns `{}` when no entry precedes the query.
extern "C" fn source_map_find_entry_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    let payload = js_closure_get_capture_f64(closure, 0);
    let args = source_map_collect_args(rest);
    let query_line = source_map_arg_i64(&args, 0);
    let query_col = source_map_arg_i64(&args, 1);

    let Some(entry) = source_map_lookup(payload, query_line, query_col) else {
        return module_object_value(crate::object::js_object_alloc(0, 0));
    };

    let obj = crate::object::js_object_alloc(0, 6);
    module_set_field(obj, "generatedLine", entry.generated_line as f64);
    module_set_field(obj, "generatedColumn", entry.generated_column as f64);
    if let Some((source_index, original_line, original_column, name_index)) = entry.original {
        module_set_field(
            obj,
            "originalSource",
            source_map_array_element(payload, "sources", source_index),
        );
        module_set_field(obj, "originalLine", original_line as f64);
        module_set_field(obj, "originalColumn", original_column as f64);
        if let Some(name_index) = name_index {
            let name = source_map_array_element(payload, "names", name_index);
            if JSValue::from_bits(name.to_bits()).is_string() {
                module_set_field(obj, "name", name);
            }
        }
    }
    module_object_value(obj)
}

/// `SourceMap#findOrigin(lineNumber, columnNumber)`. Node echoes the queried
/// coordinates (as `lineNumber`/`columnNumber`, or `null` when an argument is
/// not a finite number) and tags on the `name`/`fileName` of the entry at that
/// generated position. The lone special case is a numeric `(0, 0)` query, for
/// which Node returns an empty object.
extern "C" fn source_map_find_origin_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    let payload = js_closure_get_capture_f64(closure, 0);
    let args = source_map_collect_args(rest);
    let line = source_map_arg_number(&args, 0);
    let col = source_map_arg_number(&args, 1);

    if line == Some(0.0) && col == Some(0.0) {
        return module_object_value(crate::object::js_object_alloc(0, 0));
    }

    let entry = source_map_lookup(
        payload,
        line.map(|n| n as i64).unwrap_or(0),
        col.map(|n| n as i64).unwrap_or(0),
    );
    source_map_origin_object(payload, entry, line, col)
}

/// Module.isBuiltin(id) -> boolean
#[no_mangle]
pub extern "C" fn js_module_is_builtin(id: f64) -> f64 {
    let value = JSValue::from_bits(id.to_bits());
    let mut sso_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(bytes) = (unsafe { crate::string::js_string_key_bytes(value, &mut sso_buf) }) else {
        return f64::from_bits(crate::value::TAG_FALSE);
    };
    let Ok(specifier) = std::str::from_utf8(bytes) else {
        return f64::from_bits(crate::value::TAG_FALSE);
    };
    let is_builtin = if let Some(name) = specifier.strip_prefix("node:") {
        MODULE_BUILTIN_MODULES.contains(&specifier) || MODULE_BUILTIN_MODULES.contains(&name)
    } else {
        MODULE_BUILTIN_MODULES.contains(&specifier)
    };
    f64::from_bits(if is_builtin {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    })
}

/// `module.findPackageJSON(specifier[, base])` — resolve the nearest
/// `package.json` for a resolved specifier (#3120). Perry implements the
/// local-specifier path: the `specifier` is resolved against `base`'s
/// directory (when relative/absolute) and Perry walks parent directories
/// looking for `package.json`, returning its absolute path. The result is
/// canonicalized to match Node's realpath-based output.
///
/// Argument validation matches Node's observable surface:
///   * missing `specifier` → `TypeError [ERR_MISSING_ARGS]`
///   * `base` that is not a string/URL (number, null, …) →
///     `TypeError [ERR_INVALID_ARG_TYPE]`
///   * no enclosing `package.json` → `undefined`
#[no_mangle]
pub extern "C" fn js_module_find_package_json(specifier: f64, base: f64) -> f64 {
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);

    // `specifier` is required and must be a string (Perry covers the
    // local-path/file-URL specifier shape).
    if specifier.to_bits() == crate::value::TAG_UNDEFINED {
        crate::fs::validate::throw_error_with_code(
            "The \"specifier\" argument must be specified",
            "ERR_MISSING_ARGS",
        );
    }
    let mut sso_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let spec_value = JSValue::from_bits(specifier.to_bits());
    let Some(spec_bytes) =
        (unsafe { crate::string::js_string_key_bytes(spec_value, &mut sso_buf) })
    else {
        let message = format!(
            "The \"specifier\" argument must be of type string. Received {}",
            crate::fs::validate::describe_received(specifier)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    };
    let specifier_str = String::from_utf8_lossy(spec_bytes).into_owned();

    // Resolve `base` to a directory. A missing/undefined base anchors at the
    // current working directory (Node requires a base for relative specifiers,
    // but the observable test surface always passes one).
    let base_path = if base.to_bits() == crate::value::TAG_UNDEFINED {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        match crate::url::node_compat::module_base_to_path(base) {
            Some(p) => p,
            None => {
                let message = format!(
                    "The \"base\" argument must be of type string or an instance of URL. Received {}",
                    crate::fs::validate::describe_received(base)
                );
                crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
            }
        }
    };

    let Some(pkg_path) = find_nearest_package_json(&specifier_str, &base_path) else {
        return undefined;
    };
    module_string_value(&pkg_path)
}

/// Resolve `specifier` against `base`'s directory, then walk parent
/// directories looking for a `package.json`. Returns the canonicalized
/// absolute path of the first match. `base` may name a file or a directory
/// (trailing separator); both anchor at the containing directory.
fn find_nearest_package_json(specifier: &str, base: &str) -> Option<String> {
    use std::path::{Path, PathBuf};

    let base_path = Path::new(base);
    // A directory base (trailing separator) or an existing directory anchors
    // resolution at itself; otherwise resolve against the parent directory of
    // the base file.
    let base_dir: PathBuf = if base.ends_with(std::path::MAIN_SEPARATOR) || base_path.is_dir() {
        base_path.to_path_buf()
    } else {
        base_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    };

    let resolved = if Path::new(specifier).is_absolute() {
        PathBuf::from(specifier)
    } else {
        base_dir.join(specifier)
    };

    // Start the upward walk at the directory containing the resolved target.
    let mut dir = if resolved.is_dir() {
        resolved
    } else {
        resolved
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or(base_dir)
    };

    loop {
        let candidate = dir.join("package.json");
        if candidate.is_file() {
            let canonical = std::fs::canonicalize(&candidate).unwrap_or(candidate);
            return Some(canonical.to_string_lossy().into_owned());
        }
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => return None,
        }
    }
}

/// process.getBuiltinModule(id) -> module namespace | undefined
#[no_mangle]
pub extern "C" fn js_process_get_builtin_module(id: f64) -> f64 {
    let value = JSValue::from_bits(id.to_bits());
    let mut sso_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(bytes) = (unsafe { crate::string::js_string_key_bytes(value, &mut sso_buf) }) else {
        let message = format!(
            "The \"id\" argument must be of type string. Received {}",
            crate::fs::validate::describe_received(id)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    };
    let Ok(specifier) = std::str::from_utf8(bytes) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    if specifier == "sea" {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let name = specifier.strip_prefix("node:").unwrap_or(specifier);
    let Some(module_name) = supported_builtin_module_name(name) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    if module_name == "timers/promises" {
        return unsafe {
            crate::node_submodules::js_node_submodule_namespace(
                b"timers_promises".as_ptr(),
                "timers_promises".len() as u32,
            )
        };
    }
    crate::object::native_module_get_builtin_module_value(module_name)
}

/// Thread-local cell holding the process title set via `process.title = X`
/// (#1401). `None` means "not assigned yet, fall back to argv[0]". The
/// setter records the value here; on Linux it also calls `prctl(PR_SET_NAME)`
/// so `/proc/<pid>/comm` reflects the new value. macOS has no per-process
/// analog — the assignment is still observable via subsequent `process.title`
/// reads, matching Node's best-effort semantics.
thread_local! {
    static PROCESS_TITLE: std::cell::RefCell<Option<String>> = const {
        std::cell::RefCell::new(None)
    };
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
pub(super) fn format_out_of_range_number(n: f64) -> String {
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

fn empty_object_value() -> f64 {
    module_object_value(crate::object::js_object_alloc(0, 0))
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

#[cfg(unix)]
fn read_process_cpu_micros() -> (f64, f64) {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } != 0 {
        return (0.0, 0.0);
    }
    let user = (usage.ru_utime.tv_sec as f64) * 1_000_000.0 + usage.ru_utime.tv_usec as f64;
    let system = (usage.ru_stime.tv_sec as f64) * 1_000_000.0 + usage.ru_stime.tv_usec as f64;
    (user, system)
}

#[cfg(not(unix))]
fn read_process_cpu_micros() -> (f64, f64) {
    (0.0, 0.0)
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
    value.is_finite() && value >= 0.0 && value <= MAX_SAFE_INTEGER_F64
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

/// Get resident set size (RSS) in bytes using platform-specific APIs
pub(crate) fn get_rss_bytes() -> u64 {
    #[cfg(target_os = "macos")]
    {
        use std::mem;
        extern "C" {
            fn mach_task_self() -> u32;
            fn task_info(
                target_task: u32,
                flavor: u32,
                task_info_out: *mut u8,
                task_info_outCnt: *mut u32,
            ) -> i32;
        }
        #[repr(C)]
        struct MachTaskBasicInfo {
            virtual_size: u64,
            resident_size: u64,
            resident_size_max: u64,
            user_time: [u32; 2],
            system_time: [u32; 2],
            policy: i32,
            suspend_count: i32,
        }
        const MACH_TASK_BASIC_INFO: u32 = 20;
        let mut info: MachTaskBasicInfo = unsafe { mem::zeroed() };
        let mut count = (mem::size_of::<MachTaskBasicInfo>() / mem::size_of::<u32>()) as u32;
        let ret = unsafe {
            task_info(
                mach_task_self(),
                MACH_TASK_BASIC_INFO,
                &mut info as *mut _ as *mut u8,
                &mut count,
            )
        };
        if ret == 0 {
            info.resident_size
        } else {
            0
        }
    }
    #[cfg(target_os = "linux")]
    {
        // Read /proc/self/statm - second field is RSS in pages
        if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
            let parts: Vec<&str> = statm.split_whitespace().collect();
            if parts.len() >= 2 {
                if let Ok(pages) = parts[1].parse::<u64>() {
                    return pages * 4096; // page size is typically 4KB
                }
            }
        }
        0
    }
    #[cfg(target_os = "windows")]
    {
        #[repr(C)]
        struct PROCESS_MEMORY_COUNTERS {
            cb: u32,
            page_fault_count: u32,
            peak_working_set_size: usize,
            working_set_size: usize,
            quota_peak_paged_pool_usage: usize,
            quota_paged_pool_usage: usize,
            quota_peak_non_paged_pool_usage: usize,
            quota_non_paged_pool_usage: usize,
            pagefile_usage: usize,
            peak_pagefile_usage: usize,
        }
        extern "system" {
            fn GetCurrentProcess() -> isize;
            fn K32GetProcessMemoryInfo(
                process: isize,
                ppsmemCounters: *mut PROCESS_MEMORY_COUNTERS,
                cb: u32,
            ) -> i32;
        }
        unsafe {
            let mut pmc: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
            pmc.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            if K32GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc, pmc.cb) != 0 {
                pmc.working_set_size as u64
            } else {
                0
            }
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        0
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
    use std::cell::Cell;
    thread_local! {
        static CACHED_ENV: Cell<f64> = const { Cell::new(0.0) };
    }
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

/// Read the current thread's CPU time as (user_us, system_us). The split
/// isn't directly available from CLOCK_THREAD_CPUTIME_ID — that clock
/// reports total. Node returns the user/system split when libuv can
/// produce it (Linux/macOS via getrusage(RUSAGE_THREAD)/thread_info), but
/// for Perry we report all of it as `user` and 0 for `system`. The exact
/// split is uncommon to depend on in tests; the shape is what matters.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn read_thread_cpu_micros() -> (f64, f64) {
    let mut ts: libc::timespec = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    if ok != 0 {
        return (0.0, 0.0);
    }
    let total_us = ((ts.tv_sec as f64) * 1_000_000.0 + (ts.tv_nsec as f64) / 1_000.0).floor();
    (total_us, 0.0)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn read_thread_cpu_micros() -> (f64, f64) {
    (0.0, 0.0)
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

/// True when `jv` is a heap pointer whose GC type tag marks it as an
/// Array. Used by `process.hrtime` to reject any non-array `prior`
/// argument before reading the `[secs, nanos]` tuple.
pub(crate) fn is_array_value(jv: JSValue) -> bool {
    if !jv.is_pointer() {
        return false;
    }
    let ptr = jv.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return false;
    }
    let gc_header = unsafe { &*(ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader) };
    gc_header.obj_type == crate::gc::GC_TYPE_ARRAY
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

// #3108 — `process.sourceMapsEnabled` / `process.setSourceMapsEnabled(bool)`.
//
// Node exposes a live boolean toggle: `setSourceMapsEnabled(true|false)`
// flips the flag and returns `undefined`, the getter reflects it, and a
// non-boolean setter argument throws `TypeError [ERR_INVALID_ARG_TYPE]`.
// Perry compiles AOT and ships no source-map resolver, so the flag drives
// nothing observable beyond its own state — but mirroring Node's round-trip
// + validation lets feature-detecting libraries (and the parity suite)
// behave identically. The flag starts `false`, matching a fresh Node process
// launched without `--enable-source-maps`.
static SOURCE_MAPS_ENABLED: AtomicBool = AtomicBool::new(false);
static SOURCE_MAPS_NODE_MODULES: AtomicBool = AtomicBool::new(false);
static SOURCE_MAPS_GENERATED_CODE: AtomicBool = AtomicBool::new(false);
static MODULE_COMPILE_CACHE_DIR: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

fn module_bool_value(value: bool) -> f64 {
    f64::from_bits(if value {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    })
}

fn module_undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn module_value_to_string(value: f64) -> Option<String> {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_any_string() {
        return None;
    }
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    module_string_header_to_string(ptr)
}

fn module_value_to_string_or_buffer(value: f64) -> Option<String> {
    if let Some(value) = module_value_to_string(value) {
        return Some(value);
    }
    if crate::buffer::js_buffer_is_buffer(value.to_bits() as i64) == 1 {
        let addr =
            (value.to_bits() & crate::value::POINTER_MASK) as *const crate::buffer::BufferHeader;
        let ptr = crate::buffer::js_buffer_to_string(addr, 0);
        return module_string_header_to_string(ptr);
    }
    None
}

fn module_string_header_to_string(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return Some(String::new());
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        Some(String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned())
    }
}

fn module_object_ptr(value: f64) -> Option<*const crate::object::ObjectHeader> {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let gc_header = unsafe { &*(ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader) };
    if gc_header.obj_type == crate::gc::GC_TYPE_OBJECT {
        Some(ptr as *const crate::object::ObjectHeader)
    } else {
        None
    }
}

fn module_required_options_object(
    value: f64,
    name: &str,
) -> Option<*const crate::object::ObjectHeader> {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return None;
    }
    if let Some(obj) = module_object_ptr(value) {
        return Some(obj);
    }
    let message = format!(
        "The \"{}\" argument must be of type object. Received {}",
        name,
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

fn module_get_named_field(obj: *const crate::object::ObjectHeader, name: &str) -> f64 {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_get_field_by_name_f64(obj, key)
}

fn module_throw_plain_type_error(message: &str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn module_throw_syntax_error_with_code(message: &str, code: &'static str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    let err = crate::error::js_syntaxerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn module_validate_bool_property(value: f64, name: &str) -> Option<bool> {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return None;
    }
    if jv.is_bool() {
        return Some(jv.as_bool());
    }
    let message = format!(
        "The \"options.{}\" property must be of type boolean. Received {}",
        name,
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

/// `process.sourceMapsEnabled` getter — returns the current toggle as a
/// NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_process_source_maps_enabled() -> f64 {
    let on = SOURCE_MAPS_ENABLED.load(Ordering::Relaxed);
    f64::from_bits(if on {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    })
}

/// `process.setSourceMapsEnabled(enabled)` — validates that `enabled` is a
/// boolean (else `TypeError [ERR_INVALID_ARG_TYPE]`), stores it, and returns
/// `undefined`. Receives the full NaN-boxed value so missing/null/numeric/
/// string/object arguments are rejected exactly as Node does.
#[no_mangle]
pub extern "C" fn js_process_set_source_maps_enabled(value: f64) -> f64 {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_bool() {
        let message = format!(
            "The \"enabled\" argument must be of type boolean. Received {}",
            crate::fs::validate::describe_received(value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    SOURCE_MAPS_ENABLED.store(jv.as_bool(), Ordering::Relaxed);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// `module.getSourceMapsSupport()` mirrors Node's state object. Perry does not
/// consume source maps during AOT execution, but the helper state is observable
/// through `node:module` and shares the enabled flag with `process`.
#[no_mangle]
pub extern "C" fn js_module_get_source_maps_support() -> f64 {
    let obj = crate::object::js_object_alloc(0, 3);
    module_set_field(
        obj,
        "enabled",
        module_bool_value(SOURCE_MAPS_ENABLED.load(Ordering::Relaxed)),
    );
    module_set_field(
        obj,
        "nodeModules",
        module_bool_value(SOURCE_MAPS_NODE_MODULES.load(Ordering::Relaxed)),
    );
    module_set_field(
        obj,
        "generatedCode",
        module_bool_value(SOURCE_MAPS_GENERATED_CODE.load(Ordering::Relaxed)),
    );
    module_object_value(obj)
}

#[no_mangle]
pub extern "C" fn js_module_set_source_maps_support(enabled: f64, options: f64) -> f64 {
    let enabled_value = JSValue::from_bits(enabled.to_bits());
    if !enabled_value.is_bool() {
        let message = format!(
            "The \"enabled\" argument must be of type boolean. Received {}",
            crate::fs::validate::describe_received(enabled)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }

    let mut node_modules = false;
    let mut generated_code = false;
    if enabled_value.as_bool() {
        if let Some(options_obj) = module_required_options_object(options, "options") {
            if let Some(value) = module_validate_bool_property(
                module_get_named_field(options_obj, "nodeModules"),
                "nodeModules",
            ) {
                node_modules = value;
            }
            if let Some(value) = module_validate_bool_property(
                module_get_named_field(options_obj, "generatedCode"),
                "generatedCode",
            ) {
                generated_code = value;
            }
        }
    } else if !JSValue::from_bits(options.to_bits()).is_undefined() {
        module_required_options_object(options, "options");
    }

    SOURCE_MAPS_ENABLED.store(enabled_value.as_bool(), Ordering::Relaxed);
    SOURCE_MAPS_NODE_MODULES.store(node_modules, Ordering::Relaxed);
    SOURCE_MAPS_GENERATED_CODE.store(generated_code, Ordering::Relaxed);
    module_undefined()
}

#[no_mangle]
pub extern "C" fn js_module_get_compile_cache_dir() -> f64 {
    let guard = MODULE_COMPILE_CACHE_DIR
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match guard.as_deref() {
        Some(dir) => module_string_value(dir),
        None => module_undefined(),
    }
}

#[no_mangle]
pub extern "C" fn js_module_enable_compile_cache(cache_dir: f64) -> f64 {
    let requested_dir = {
        let value = JSValue::from_bits(cache_dir.to_bits());
        if value.is_undefined() {
            std::env::temp_dir()
                .join("node-compile-cache")
                .to_string_lossy()
                .into_owned()
        } else if let Some(dir) = module_value_to_string(cache_dir) {
            dir
        } else {
            crate::fs::validate::throw_type_error_with_code(
                "cacheDir should be a string",
                "ERR_INVALID_ARG_TYPE",
            );
        }
    };

    let mut guard = MODULE_COMPILE_CACHE_DIR
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let status = if guard.is_some() {
        2.0
    } else {
        *guard = Some(requested_dir);
        1.0
    };
    let directory = guard.as_deref().unwrap_or("");

    let obj = crate::object::js_object_alloc(0, 2);
    module_set_field(obj, "status", status);
    module_set_field(obj, "directory", module_string_value(directory));
    module_object_value(obj)
}

#[no_mangle]
pub extern "C" fn js_module_flush_compile_cache() -> f64 {
    module_undefined()
}

fn module_hook_member(value: f64, name: &str) -> f64 {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() || jv.is_null() || is_function_value(value) {
        return value;
    }
    let message = format!(
        "The \"hooks.{}\" property must be of type function. Received {}",
        name,
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

fn module_hooks_deregister_prototype() -> *mut crate::object::ObjectHeader {
    let proto = crate::object::js_object_alloc(0, 1);
    module_set_field(proto, "deregister", module_noop_function("deregister"));
    crate::object::set_property_attrs(
        proto as usize,
        "deregister".to_string(),
        crate::object::PropertyAttrs::new(true, false, true),
    );
    proto
}

/// `module.registerHooks(options)` — synchronous loader customization entry
/// surface. Perry records Node-compatible hook handles and validation, while
/// dynamic import resolution/loading still follows Perry's compile-time graph.
#[no_mangle]
pub extern "C" fn js_module_register_hooks(hooks: f64) -> f64 {
    let hooks_value = JSValue::from_bits(hooks.to_bits());
    if hooks_value.is_undefined() {
        module_throw_plain_type_error(
            "Cannot destructure property 'resolve' of 'hooks' as it is undefined.",
        );
    }
    if hooks_value.is_null() {
        module_throw_plain_type_error(
            "Cannot destructure property 'resolve' of 'hooks' as it is null.",
        );
    }

    let mut resolve = module_undefined();
    let mut load = module_undefined();
    if let Some(hooks_obj) = module_object_ptr(hooks) {
        resolve = module_hook_member(module_get_named_field(hooks_obj, "resolve"), "resolve");
        load = module_hook_member(module_get_named_field(hooks_obj, "load"), "load");
    }

    let handle = crate::object::js_object_alloc(0, 2);
    module_set_field(handle, "resolve", resolve);
    module_set_field(handle, "load", load);

    let proto = module_hooks_deregister_prototype();
    let proto_value = module_object_value(proto);
    crate::object::prototype_chain::object_set_static_prototype(
        handle as usize,
        proto_value.to_bits(),
    );
    module_object_value(handle)
}

fn module_register_invalid_specifier(specifier: &str) -> bool {
    if specifier.starts_with("data:")
        || specifier.starts_with("file:")
        || specifier.starts_with("./")
        || specifier.starts_with("../")
        || specifier.starts_with('/')
    {
        return false;
    }
    specifier.is_empty()
        || specifier.contains('%')
        || specifier.chars().any(|ch| ch.is_ascii_whitespace())
}

/// `module.register(specifier[, parentURL][, options])`. Perry does not load
/// customization modules into the resolver pipeline yet; this entry point
/// matches Node's observable return value for accepted registrations and
/// deterministic invalid specifier errors.
#[no_mangle]
pub extern "C" fn js_module_register(specifier: f64, _parent_url: f64, _options: f64) -> f64 {
    let Some(specifier_str) = module_value_to_string(specifier) else {
        return module_undefined();
    };
    if module_register_invalid_specifier(&specifier_str) {
        let message = format!(
            "Invalid module \"{}\" is not a valid package name",
            specifier_str
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_MODULE_SPECIFIER");
    }
    module_undefined()
}

fn module_word_at(bytes: &[u8], index: usize, word: &[u8]) -> bool {
    if index + word.len() > bytes.len() || &bytes[index..index + word.len()] != word {
        return false;
    }
    let before = index.checked_sub(1).and_then(|i| bytes.get(i)).copied();
    let after = bytes.get(index + word.len()).copied();
    !before.is_some_and(module_is_ident_byte) && !after.is_some_and(module_is_ident_byte)
}

fn module_is_ident_byte(byte: u8) -> bool {
    byte == b'_' || byte == b'$' || byte.is_ascii_alphanumeric()
}

fn module_skip_ws(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    index
}

fn module_space_span(bytes: &mut [u8], start: usize, end: usize) {
    for byte in &mut bytes[start..end] {
        if *byte != b'\n' && *byte != b'\r' {
            *byte = b' ';
        }
    }
}

fn module_strip_interfaces(bytes: &mut [u8]) {
    let mut index = 0;
    while index < bytes.len() {
        if !module_word_at(bytes, index, b"interface") {
            index += 1;
            continue;
        }
        let mut cursor = index + "interface".len();
        cursor = module_skip_ws(bytes, cursor);
        while cursor < bytes.len() && module_is_ident_byte(bytes[cursor]) {
            cursor += 1;
        }
        cursor = module_skip_ws(bytes, cursor);
        if cursor >= bytes.len() || bytes[cursor] != b'{' {
            index += 1;
            continue;
        }
        let mut depth = 0usize;
        let mut end = cursor;
        while end < bytes.len() {
            match bytes[end] {
                b'{' => depth += 1,
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        end += 1;
                        break;
                    }
                }
                _ => {}
            }
            end += 1;
        }
        module_space_span(bytes, index, end.min(bytes.len()));
        index = end;
    }
}

fn module_strip_type_annotations(bytes: &mut [u8]) {
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b':' {
            index += 1;
            continue;
        }

        let mut before = index;
        while before > 0 && bytes[before - 1].is_ascii_whitespace() {
            before -= 1;
        }
        if before == 0 || !module_is_ident_byte(bytes[before - 1]) {
            index += 1;
            continue;
        }

        let after = module_skip_ws(bytes, index + 1);
        if after >= bytes.len()
            || matches!(
                bytes[after],
                b'\'' | b'"' | b'`' | b'0'..=b'9' | b'{' | b'[' | b':' | b',' | b')' | b';'
            )
        {
            index += 1;
            continue;
        }

        let mut end = after;
        while end < bytes.len()
            && !matches!(bytes[end], b'=' | b',' | b')' | b';' | b'{' | b'\n' | b'\r')
        {
            end += 1;
        }
        module_space_span(bytes, index, end);
        index = end;
    }
}

fn module_strip_type_syntax(source: &str) -> String {
    let mut bytes = source.as_bytes().to_vec();
    module_strip_interfaces(&mut bytes);
    module_strip_type_annotations(&mut bytes);
    String::from_utf8(bytes).unwrap_or_else(|_| source.to_string())
}

fn module_contains_enum(source: &str) -> bool {
    let bytes = source.as_bytes();
    (0..bytes.len()).any(|index| module_word_at(bytes, index, b"enum"))
}

fn module_transform_enums(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = String::new();
    let mut index = 0;
    while index < bytes.len() {
        if !module_word_at(bytes, index, b"enum") {
            out.push(bytes[index] as char);
            index += 1;
            continue;
        }

        let enum_start = index;
        let mut cursor = module_skip_ws(bytes, index + "enum".len());
        let name_start = cursor;
        while cursor < bytes.len() && module_is_ident_byte(bytes[cursor]) {
            cursor += 1;
        }
        if name_start == cursor {
            out.push(bytes[index] as char);
            index += 1;
            continue;
        }
        let name = &source[name_start..cursor];
        cursor = module_skip_ws(bytes, cursor);
        if cursor >= bytes.len() || bytes[cursor] != b'{' {
            out.push_str(&source[enum_start..cursor.min(source.len())]);
            index = cursor;
            continue;
        }
        let body_start = cursor + 1;
        let mut depth = 1usize;
        cursor += 1;
        while cursor < bytes.len() {
            match bytes[cursor] {
                b'{' => depth += 1,
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            cursor += 1;
        }
        if cursor >= bytes.len() {
            out.push_str(&source[enum_start..]);
            break;
        }
        let body = &source[body_start..cursor];
        let mut next_value = 0i32;
        out.push_str("var ");
        out.push_str(name);
        out.push_str(";\n(function (");
        out.push_str(name);
        out.push_str(") {\n");
        for raw_member in body.split(',') {
            let member = raw_member.trim();
            if member.is_empty() {
                continue;
            }
            let (member_name, value) = if let Some((left, right)) = member.split_once('=') {
                let parsed = right.trim().parse::<i32>().unwrap_or(next_value);
                (left.trim(), parsed)
            } else {
                (member, next_value)
            };
            if member_name.is_empty() {
                continue;
            }
            out.push_str("  ");
            out.push_str(name);
            out.push('[');
            out.push_str(name);
            out.push_str("[\"");
            out.push_str(member_name);
            out.push_str("\"] = ");
            out.push_str(&value.to_string());
            out.push_str("] = \"");
            out.push_str(member_name);
            out.push_str("\";\n");
            next_value = value.saturating_add(1);
        }
        out.push_str("})(");
        out.push_str(name);
        out.push_str(" || (");
        out.push_str(name);
        out.push_str(" = {}));");
        index = cursor + 1;
    }
    out
}

#[no_mangle]
pub extern "C" fn js_module_strip_typescript_types(code: f64, options: f64) -> f64 {
    let Some(source) = module_value_to_string(code) else {
        let message = format!(
            "The \"code\" argument must be of type string. Received {}",
            crate::fs::validate::describe_received(code)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    };

    let mut mode = "strip".to_string();
    let mut source_map = false;
    if let Some(options_obj) = module_required_options_object(options, "options") {
        let mode_value = module_get_named_field(options_obj, "mode");
        if !JSValue::from_bits(mode_value.to_bits()).is_undefined() {
            let Some(mode_string) = module_value_to_string(mode_value) else {
                let message = format!(
                    "The property 'options.mode' must be one of: 'strip', 'transform'. Received {}",
                    crate::fs::validate::describe_received(mode_value)
                );
                crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE");
            };
            if mode_string != "strip" && mode_string != "transform" {
                let message = format!(
                    "The property 'options.mode' must be one of: 'strip', 'transform'. Received '{}'",
                    mode_string
                );
                crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE");
            }
            mode = mode_string;
        }

        let source_map_value = module_get_named_field(options_obj, "sourceMap");
        if let Some(value) = module_validate_bool_property(source_map_value, "sourceMap") {
            source_map = value;
        }
    }

    if mode == "strip" && module_contains_enum(&source) {
        module_throw_syntax_error_with_code(
            "TypeScript enum is not supported in strip-only mode",
            "ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX",
        );
    }

    let mut output = if mode == "transform" {
        module_strip_type_syntax(&module_transform_enums(&source))
    } else {
        module_strip_type_syntax(&source)
    };
    if mode == "transform" && source_map {
        output.push_str("\n//# sourceMappingURL=data:application/json;base64,e30=");
    }
    module_string_value(&output)
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
