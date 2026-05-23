//! Process module - provides access to environment and process information

use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;

/// Exit the process with the given exit code
/// process.exit(code?: number) -> never
/// Uses libc::_exit() to bypass cleanup handlers that can cause SIGILL
/// during async event loop drain and V8 isolate destruction.
#[no_mangle]
pub extern "C" fn js_process_exit(code: f64) {
    let exit_code = if code.is_nan() || code.is_infinite() {
        1 // Default to 1 for invalid codes
    } else {
        code as i32
    };
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

/// process.umask(mask) -> number. Sets the file-mode creation mask to
/// `mask` (coerced to integer) and returns the previous value.
#[no_mangle]
pub extern "C" fn js_process_umask_set(mask: f64) -> f64 {
    #[cfg(unix)]
    unsafe {
        let m = if mask.is_nan() || mask.is_infinite() {
            0
        } else {
            mask as libc::mode_t
        };
        libc::umask(m) as f64
    }
    #[cfg(not(unix))]
    {
        let _ = mask;
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

/// process.threadCpuUsage() -> object { user, system } in microseconds.
/// CPU time consumed by the current thread. Uses CLOCK_THREAD_CPUTIME_ID
/// (available on macOS 10.12+ and Linux). Platforms without the clock get
/// 0.0 for both fields.
#[no_mangle]
pub extern "C" fn js_process_thread_cpu_usage() -> f64 {
    let (user_us, system_us) = read_thread_cpu_micros();

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
    let total_us = (ts.tv_sec as f64) * 1_000_000.0 + (ts.tv_nsec as f64) / 1_000.0;
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
