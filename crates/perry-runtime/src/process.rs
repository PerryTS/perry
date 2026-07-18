//! Process module - provides access to environment and process information
//!
//! The bulk of the `process.*` / `node:module` runtime surface lives in topical
//! sub-modules (see the `mod` declarations below); this trunk keeps the shared
//! NaN-box/string/object construction helpers, the process-wide thread-local
//! and static state, the metadata-property dispatcher, and the re-exports that
//! preserve the existing `crate::process::*` call paths.

use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64, ClosureHeader,
};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};

mod credentials;
mod env_misc;
pub(crate) use env_misc::{
    exit_after_current_thread_collection_teardown, format_out_of_range_number,
};
mod finalization;
pub(crate) mod ipc;
mod node_module;
mod permission;
mod report;
pub use credentials::{
    js_process_getegid, js_process_geteuid, js_process_getgid, js_process_getgroups,
    js_process_getuid, js_process_initgroups, js_process_setegid, js_process_seteuid,
    js_process_setgid, js_process_setgroups, js_process_setuid,
};
pub use ipc::*;

// ── env_misc re-exports (preserve `crate::process::*` paths) ────────────────
pub use env_misc::{
    is_process_env_object, is_process_env_ptr, js_getenv, js_getenv_value, js_process_abort,
    js_process_active_resources_info, js_process_add_uncaught_exception_capture_callback,
    js_process_available_memory, js_process_binding, js_process_chdir_jsv,
    js_process_constrained_memory, js_process_cpu_usage, js_process_debug_end,
    js_process_debug_process, js_process_dlopen, js_process_emit_warning, js_process_env,
    js_process_execve, js_process_exit, js_process_exit_code_get, js_process_exit_code_set,
    js_process_fatal_exception, js_process_get_active_handles, js_process_get_active_requests,
    js_process_has_uncaught_exception_capture_callback, js_process_internal_kill,
    js_process_linked_binding, js_process_load_env_file, js_process_memory_usage,
    js_process_open_stdin, js_process_raw_debug, js_process_really_exit, js_process_ref,
    js_process_resource_usage, js_process_set_title,
    js_process_set_uncaught_exception_capture_callback, js_process_start_profiler_idle_notifier,
    js_process_stop_profiler_idle_notifier, js_process_thread_cpu_usage, js_process_tick_callback,
    js_process_title, js_process_umask, js_process_umask_set, js_process_unref, js_removeenv,
    js_setenv,
};

// ── finalization re-exports ─────────────────────────────────────────────────
pub use finalization::{
    js_process_run_finalization_before_exit, js_process_run_finalization_exit,
    scan_process_finalization_roots_mut,
};

// ── permission re-exports ───────────────────────────────────────────────────
pub(crate) use permission::process_permission_enabled;

// ── node_module re-exports ──────────────────────────────────────────────────
pub use node_module::{
    js_module_builtin_modules, js_module_constants, js_module_dynamic_import_apply_hooks,
    js_module_enable_compile_cache, js_module_find_package_json, js_module_find_path,
    js_module_flush_compile_cache, js_module_get_compile_cache_dir,
    js_module_get_source_maps_support, js_module_init_paths, js_module_is_builtin, js_module_load,
    js_module_module_new, js_module_node_module_paths, js_module_preload_modules,
    js_module_register, js_module_register_hooks, js_module_resolve_filename,
    js_module_resolve_lookup_paths, js_module_set_source_maps_support, js_module_source_map_new,
    js_module_strip_typescript_types, js_process_get_builtin_module,
    js_process_get_builtin_module_devirt, js_process_set_source_maps_enabled,
    js_process_source_maps_enabled, scan_process_module_loader_roots_mut,
};

// ─────────────────────────────────────────────────────────────────────────────
// Shared NaN-box / construction helpers (used across the sub-modules above).
// ─────────────────────────────────────────────────────────────────────────────

pub(crate) fn bool_value(value: bool) -> f64 {
    f64::from_bits(if value {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    })
}

pub(crate) fn undefined_value() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(crate) fn is_function_value(value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<u8>() as usize;
        if crate::closure::is_closure_ptr(ptr) {
            return true;
        }
    }
    crate::value::js_handle_is_function(value)
}

pub(crate) fn supported_builtin_module_name(name: &str) -> Option<&str> {
    match name {
        "assert"
        | "assert/strict"
        | "async_hooks"
        | "buffer"
        | "child_process"
        | "cluster"
        | "console"
        | "constants"
        | "crypto"
        | "diagnostics_channel"
        | "dns"
        | "dns/promises"
        | "events"
        | "fs"
        | "http"
        | "http2"
        | "https"
        | "module"
        | "net"
        | "os"
        | "path"
        | "perf_hooks"
        | "process"
        | "punycode"
        | "querystring"
        | "readline"
        | "readline/promises"
        | "sea"
        | "stream"
        | "stream/promises"
        | "string_decoder"
        | "sys"
        | "test"
        | "test/reporters"
        | "timers"
        | "timers/promises"
        | "tty"
        | "url"
        | "util"
        | "util/types"
        | "vm"
        | "worker_threads"
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

pub(crate) fn module_string_value(value: &str) -> f64 {
    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

pub(crate) fn module_object_value(obj: *mut crate::object::ObjectHeader) -> f64 {
    f64::from_bits(JSValue::object_ptr(obj as *mut u8).bits())
}

pub(crate) fn module_set_field(obj: *mut crate::object::ObjectHeader, name: &str, value: f64) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_set_field_by_name(obj, key, value);
}

pub(crate) type ModuleFunction1 = extern "C" fn(*const crate::closure::ClosureHeader, f64) -> f64;
pub(crate) type ModuleFunction2 =
    extern "C" fn(*const crate::closure::ClosureHeader, f64, f64) -> f64;

#[derive(Clone, Copy)]
pub(crate) struct ModuleLoaderHookEntry {
    pub(crate) id: u64,
    pub(crate) resolve: f64,
    pub(crate) load: f64,
    pub(crate) active: bool,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum ProcessFinalizationKind {
    Exit,
    BeforeExit,
}

#[derive(Clone, Copy)]
pub(crate) struct ProcessFinalizationEntry {
    pub(crate) obj: f64,
    pub(crate) callback: f64,
    pub(crate) kind: ProcessFinalizationKind,
}

#[derive(Clone)]
pub(crate) struct ProcessPermissionDrop {
    pub(crate) scope: String,
    pub(crate) reference: Option<String>,
}

thread_local! {
    pub(crate) static PROCESS_FINALIZATION_REGISTRY: RefCell<Vec<ProcessFinalizationEntry>> =
        const { RefCell::new(Vec::new()) };
    pub(crate) static PROCESS_FINALIZATION_BEFORE_EXIT_RAN: Cell<bool> = const { Cell::new(false) };
    pub(crate) static PROCESS_FINALIZATION_EXIT_RAN: Cell<bool> = const { Cell::new(false) };
    pub(crate) static PROCESS_FINALIZATION_OBJECT: Cell<f64> = const { Cell::new(0.0) };
    pub(crate) static PROCESS_FINALIZATION_BEFORE_EXIT_LISTENER:
        Cell<*const crate::closure::ClosureHeader> = const { Cell::new(std::ptr::null()) };
    pub(crate) static PROCESS_FINALIZATION_BEFORE_EXIT_LISTENER_INSTALLED: Cell<bool> =
        const { Cell::new(false) };
    pub(crate) static MODULE_LOADER_HOOKS: RefCell<Vec<ModuleLoaderHookEntry>> =
        const { RefCell::new(Vec::new()) };
    pub(crate) static MODULE_LOADER_HOOK_NEXT_ID: Cell<u64> = const { Cell::new(1) };
    pub(crate) static PROCESS_PERMISSION_DROPS: RefCell<Vec<ProcessPermissionDrop>> =
        const { RefCell::new(Vec::new()) };
    pub(crate) static MODULE_LOADER_NEXT_RESOLVE: Cell<*const crate::closure::ClosureHeader> =
        const { Cell::new(std::ptr::null()) };
    pub(crate) static MODULE_LOADER_NEXT_LOAD: Cell<*const crate::closure::ClosureHeader> =
        const { Cell::new(std::ptr::null()) };
}

pub(crate) fn module_function1(name: &str, thunk: ModuleFunction1, length: u32) -> f64 {
    let func_ptr = thunk as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 1);
    crate::closure::js_register_closure_length(func_ptr, length);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    crate::object::set_builtin_closure_length(closure as usize, length);
    crate::value::js_nanbox_pointer(closure as i64)
}

pub(crate) fn module_function2(name: &str, thunk: ModuleFunction2, length: u32) -> f64 {
    let func_ptr = thunk as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 2);
    crate::closure::js_register_closure_length(func_ptr, length);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    crate::object::set_builtin_closure_length(closure as usize, length);
    crate::value::js_nanbox_pointer(closure as i64)
}

pub(crate) fn module_array_value(items: &[&str]) -> f64 {
    let arr = crate::array::js_array_alloc_with_length(items.len() as u32);
    for (i, item) in items.iter().enumerate() {
        crate::array::js_array_set_f64(arr, i as u32, module_string_value(item));
    }
    f64::from_bits(JSValue::array_ptr(arr).bits())
}

pub(crate) fn module_set_value(items: &[&str]) -> f64 {
    let mut set = crate::set::js_set_alloc(items.len() as u32);
    for item in items {
        set = crate::set::js_set_add(set, module_string_value(item));
    }
    crate::value::js_nanbox_pointer(set as i64)
}

pub(crate) fn process_argv0_string() -> String {
    std::env::args().next().unwrap_or_default()
}

pub(crate) fn node_arch_name() -> &'static str {
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

pub(crate) fn node_platform_name() -> &'static str {
    match std::env::consts::OS {
        "macos" | "ios" => "darwin",
        "windows" => "win32",
        "linux" => "linux",
        "freebsd" => "freebsd",
        other => other,
    }
}

pub(crate) fn empty_object_value() -> f64 {
    module_object_value(crate::object::js_object_alloc(0, 0))
}

#[cfg(unix)]
pub(crate) fn read_process_cpu_micros() -> (f64, f64) {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } != 0 {
        return (0.0, 0.0);
    }
    let user = (usage.ru_utime.tv_sec as f64) * 1_000_000.0 + usage.ru_utime.tv_usec as f64;
    let system = (usage.ru_stime.tv_sec as f64) * 1_000_000.0 + usage.ru_stime.tv_usec as f64;
    (user, system)
}

#[cfg(not(unix))]
pub(crate) fn read_process_cpu_micros() -> (f64, f64) {
    (0.0, 0.0)
}

/// Read the current thread's CPU time as (user_us, system_us). The split
/// isn't directly available from CLOCK_THREAD_CPUTIME_ID — that clock
/// reports total. Node returns the user/system split when libuv can
/// produce it (Linux/macOS via getrusage(RUSAGE_THREAD)/thread_info), but
/// for Perry we report all of it as `user` and 0 for `system`. The exact
/// split is uncommon to depend on in tests; the shape is what matters.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn read_thread_cpu_micros() -> (f64, f64) {
    let mut ts: libc::timespec = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    if ok != 0 {
        return (0.0, 0.0);
    }
    let total_us = ((ts.tv_sec as f64) * 1_000_000.0 + (ts.tv_nsec as f64) / 1_000.0).floor();
    (total_us, 0.0)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) fn read_thread_cpu_micros() -> (f64, f64) {
    (0.0, 0.0)
}

/// Get resident set size (RSS) in bytes using platform-specific APIs.
///
/// 2026-07-09 audit: the mach `task_info` path is identical on every Apple
/// OS, but was cfg-gated to macOS only — so RSS read 0 on iOS/tvOS/watchOS/
/// visionOS and every RSS-pressure GC heuristic was silently dead exactly
/// where memory is scarcest. Android reads the same procfs file as Linux.
pub(crate) fn get_rss_bytes() -> u64 {
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos"
    ))]
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
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        // Read /proc/self/statm - second field is RSS in pages.
        // Page size must be queried: 16 K (many Android/Asahi kernels) and
        // 64 K (some aarch64 distros) pages under-reported RSS 4-16× with
        // the old hardcoded 4096, inflating every RSS threshold to match.
        if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
            let parts: Vec<&str> = statm.split_whitespace().collect();
            if parts.len() >= 2 {
                if let Ok(pages) = parts[1].parse::<u64>() {
                    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
                    let page_size = if page_size > 0 {
                        page_size as u64
                    } else {
                        4096
                    };
                    return pages * page_size;
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
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos",
        target_os = "linux",
        target_os = "android",
        target_os = "windows"
    )))]
    {
        0
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared value-coercion helpers (used by env_misc / node_module / permission).
// ─────────────────────────────────────────────────────────────────────────────

pub(crate) fn module_value_to_string(value: f64) -> Option<String> {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_any_string() {
        return None;
    }
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    module_string_header_to_string(ptr)
}

pub(crate) fn module_value_to_string_or_buffer(value: f64) -> Option<String> {
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

pub(crate) fn module_string_header_to_string(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return Some(String::new());
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        Some(String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned())
    }
}

pub(crate) fn module_object_ptr(value: f64) -> Option<*const crate::object::ObjectHeader> {
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

pub(crate) fn module_required_options_object(
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

pub(crate) fn module_get_named_field(obj: *const crate::object::ObjectHeader, name: &str) -> f64 {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_get_field_by_name_f64(obj, key)
}

pub(crate) fn module_throw_plain_type_error(message: &str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

pub(crate) fn module_throw_syntax_error_with_code(message: &str, code: &'static str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    let err = crate::error::js_syntaxerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

pub(crate) fn module_validate_bool_property(value: f64, name: &str) -> Option<bool> {
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

// ─────────────────────────────────────────────────────────────────────────────
// `process.*` metadata-property dispatcher and the process-title cell.
// ─────────────────────────────────────────────────────────────────────────────

pub fn process_metadata_property(property: &str) -> Option<f64> {
    Some(match property {
        // #4987: core value-properties. The bare `process` identifier lowers
        // these to codegen intrinsics, but `import process from
        // 'node:process'` and `globalThis.process` resolve through the
        // native-module runtime dispatcher, which lands here. Serve them from
        // the same runtime constructors the intrinsics call so all three
        // forms observe the same values (env/stdout are live singletons).
        "env" => js_process_env(),
        "argv" => f64::from_bits(JSValue::array_ptr(crate::os::js_process_argv()).bits()),
        "platform" => f64::from_bits(JSValue::string_ptr(crate::os::js_os_platform()).bits()),
        "arch" => f64::from_bits(JSValue::string_ptr(crate::os::js_os_arch()).bits()),
        "pid" => crate::os::js_process_pid(),
        "ppid" => crate::os::js_process_ppid(),
        "version" => f64::from_bits(JSValue::string_ptr(crate::os::js_process_version()).bits()),
        "versions" => crate::os::js_process_versions(),
        "stdin" => crate::os::js_process_stdin(),
        "stdout" => crate::os::js_process_stdout(),
        "stderr" => crate::os::js_process_stderr(),
        "allowedNodeEnvironmentFlags" => report::process_allowed_flags_value(),
        "argv0" | "execPath" => module_string_value(&process_argv0_string()),
        "config" => report::process_config_value(),
        "debugPort" => 9229.0,
        "execArgv" | "moduleLoadList" => module_array_value(&[]),
        "features" => report::process_features_value(),
        "finalization" => finalization::process_finalization_value(),
        "permission" => permission::process_permission_value()?,
        "release" => report::process_release_value(),
        "report" => report::process_report_value(),
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

// ─────────────────────────────────────────────────────────────────────────────
// #3108 — source-maps toggle state, shared by `process` + `node:module`.
// ─────────────────────────────────────────────────────────────────────────────
//
// Node exposes a live boolean toggle: `setSourceMapsEnabled(true|false)`
// flips the flag and returns `undefined`, the getter reflects it, and a
// non-boolean setter argument throws `TypeError [ERR_INVALID_ARG_TYPE]`.
// Perry compiles AOT and ships no source-map resolver, so the flag drives
// nothing observable beyond its own state — but mirroring Node's round-trip
// + validation lets feature-detecting libraries (and the parity suite)
// behave identically. The flag starts `false`, matching a fresh Node process
// launched without `--enable-source-maps`.
pub(crate) static SOURCE_MAPS_ENABLED: AtomicBool = AtomicBool::new(false);
pub(crate) static SOURCE_MAPS_NODE_MODULES: AtomicBool = AtomicBool::new(false);
pub(crate) static SOURCE_MAPS_GENERATED_CODE: AtomicBool = AtomicBool::new(false);
pub(crate) static MODULE_COMPILE_CACHE_DIR: std::sync::Mutex<Option<String>> =
    std::sync::Mutex::new(None);

/// Thread-local cell holding the process title set via `process.title = X`
/// (#1401). `None` means "not assigned yet, fall back to argv[0]". The
/// setter records the value here; on Linux it also calls `prctl(PR_SET_NAME)`
/// so `/proc/<pid>/comm` reflects the new value. macOS has no per-process
/// analog — the assignment is still observable via subsequent `process.title`
/// reads, matching Node's best-effort semantics.
thread_local! {
    pub(crate) static PROCESS_TITLE: std::cell::RefCell<Option<String>> = const {
        std::cell::RefCell::new(None)
    };
}
