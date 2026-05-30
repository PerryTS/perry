//! Process module - provides access to environment and process information

use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;

/// Exit the process with the given exit code
/// process.exit(code?: number) -> never
/// Uses libc::_exit() to bypass cleanup handlers that can cause SIGILL
/// during async event loop drain and V8 isolate destruction.
#[no_mangle]
pub extern "C" fn js_process_exit(code: f64) {
    // #2013 — `process.exit('foo')` throws TypeError ERR_INVALID_ARG_TYPE
    // in Node (the exit code must be a number, null, or undefined). The
    // numeric default of `1` for NaN/Infinity in the pre-fix code only
    // matters for the inferred numeric-coerce path; keep that fallback
    // for the (degenerate) numeric NaN/Infinity case so existing call
    // sites that pass a parsed-out number aren't surprised.
    let jv = JSValue::from_bits(code.to_bits());
    if !jv.is_undefined() && !jv.is_null() && !crate::fs::validate::is_numeric(jv) {
        let message = format!(
            "The \"code\" argument must be of type number or null. Received {}",
            crate::fs::validate::describe_received(code)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
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

fn supported_builtin_module_name(name: &str) -> Option<&str> {
    match name {
        "assert" | "assert/strict" | "async_hooks" | "buffer" | "child_process" | "cluster"
        | "console" | "constants" | "crypto" | "events" | "fs" | "http" | "http2" | "https"
        | "net" | "os" | "path" | "perf_hooks" | "process" | "punycode" | "querystring"
        | "readline" | "stream" | "stream/promises" | "string_decoder" | "sys" | "timers"
        | "timers/promises" | "tty" | "url" | "util" | "util/types" | "worker_threads" | "zlib" => {
            Some(name)
        }
        _ => None,
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
    let name = specifier.strip_prefix("node:").unwrap_or(specifier);
    let Some(module_name) = supported_builtin_module_name(name) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    crate::object::js_create_native_module_namespace(module_name.as_ptr(), module_name.len())
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

/// POSIX credential accessors (#1408). On non-unix targets each returns 0.
#[no_mangle]
pub extern "C" fn js_process_getuid() -> f64 {
    #[cfg(unix)]
    unsafe {
        libc::getuid() as f64
    }
    #[cfg(not(unix))]
    {
        0.0
    }
}
#[no_mangle]
pub extern "C" fn js_process_geteuid() -> f64 {
    #[cfg(unix)]
    unsafe {
        libc::geteuid() as f64
    }
    #[cfg(not(unix))]
    {
        0.0
    }
}
#[no_mangle]
pub extern "C" fn js_process_getgid() -> f64 {
    #[cfg(unix)]
    unsafe {
        libc::getgid() as f64
    }
    #[cfg(not(unix))]
    {
        0.0
    }
}
#[no_mangle]
pub extern "C" fn js_process_getegid() -> f64 {
    #[cfg(unix)]
    unsafe {
        libc::getegid() as f64
    }
    #[cfg(not(unix))]
    {
        0.0
    }
}

/// POSIX credential setters (#2135). Each wraps the matching `libc::set*id`
/// call; non-numeric arguments and errors are silently dropped (the call
/// returns undefined), matching the "no-op stub" shape Perry uses for the
/// other unimplemented privileged process methods. On non-unix targets the
/// setters are unconditional no-ops. The runtime ignores ID-by-username
/// forms (Node accepts `process.setuid("alice")` and resolves via NSS);
/// passing a string here is a no-op — supporting the username form needs
/// `getpwnam_r` plumbing that's out of scope for the surface-level fix.
fn unix_id_arg(value: f64) -> Option<u32> {
    let v = value;
    if v.is_finite() {
        let n = v as i64;
        if n >= 0 && n <= u32::MAX as i64 {
            return Some(n as u32);
        }
    }
    None
}

#[no_mangle]
pub extern "C" fn js_process_setuid(uid: f64) {
    #[cfg(unix)]
    {
        if let Some(id) = unix_id_arg(uid) {
            unsafe {
                libc::setuid(id as libc::uid_t);
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = uid;
    }
}

#[no_mangle]
pub extern "C" fn js_process_seteuid(uid: f64) {
    #[cfg(unix)]
    {
        if let Some(id) = unix_id_arg(uid) {
            unsafe {
                libc::seteuid(id as libc::uid_t);
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = uid;
    }
}

#[no_mangle]
pub extern "C" fn js_process_setgid(gid: f64) {
    #[cfg(unix)]
    {
        if let Some(id) = unix_id_arg(gid) {
            unsafe {
                libc::setgid(id as libc::gid_t);
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = gid;
    }
}

#[no_mangle]
pub extern "C" fn js_process_setegid(gid: f64) {
    #[cfg(unix)]
    {
        if let Some(id) = unix_id_arg(gid) {
            unsafe {
                libc::setegid(id as libc::gid_t);
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = gid;
    }
}

/// `process.setgroups(groups)` — replace the calling process's
/// supplementary GID list with the IDs from a JS number array. Each non-
/// finite / out-of-range / non-numeric entry is silently skipped. On
/// non-unix targets this is a no-op (#2135).
#[no_mangle]
pub extern "C" fn js_process_setgroups(groups: f64) {
    let arr_jsval = JSValue::from_bits(groups.to_bits());
    if !arr_jsval.is_pointer() {
        return;
    }
    let arr_ptr = arr_jsval.as_pointer::<crate::array::ArrayHeader>();
    if arr_ptr.is_null() {
        return;
    }
    let len = unsafe { crate::array::js_array_length(arr_ptr) } as u32;
    #[cfg(unix)]
    {
        let mut gids: Vec<libc::gid_t> = Vec::with_capacity(len as usize);
        for i in 0..len {
            let v = unsafe { crate::array::js_array_get_f64(arr_ptr, i) };
            if let Some(id) = unix_id_arg(v) {
                gids.push(id as libc::gid_t);
            }
        }
        if !gids.is_empty() {
            unsafe {
                libc::setgroups(gids.len() as _, gids.as_ptr());
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = len;
    }
}

/// `process.initgroups(user, extra_gid)` — initialize the supplementary
/// group access list using `getgrouplist(3)`-style semantics. The first
/// argument is a username string (or numeric UID); the second is an
/// extra group ID to include. Perry today only accepts the username-as-
/// string + numeric extra_gid form via `libc::initgroups`; numeric user
/// or non-string first argument silently no-ops. Non-unix targets no-op
/// entirely (#2135).
#[no_mangle]
pub extern "C" fn js_process_initgroups(user: f64, extra_gid: f64) {
    #[cfg(unix)]
    {
        let user_jsval = JSValue::from_bits(user.to_bits());
        if !user_jsval.is_any_string() {
            return;
        }
        let user_ptr = crate::value::js_get_string_pointer_unified(user);
        if user_ptr == 0 {
            return;
        }
        let user_str_ptr = user_ptr as *const StringHeader;
        let len = unsafe { (*user_str_ptr).byte_len } as usize;
        let data = unsafe { (user_str_ptr as *const u8).add(std::mem::size_of::<StringHeader>()) };
        let bytes = unsafe { std::slice::from_raw_parts(data, len) };
        let Ok(name) = std::str::from_utf8(bytes) else {
            return;
        };
        let Some(extra) = unix_id_arg(extra_gid) else {
            return;
        };
        let Ok(cname) = std::ffi::CString::new(name) else {
            return;
        };
        unsafe {
            libc::initgroups(cname.as_ptr(), extra as _);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (user, extra_gid);
    }
}

/// `process.getgroups()` — supplementary group IDs the process is a member
/// of, as a JS array of numbers. Wraps `libc::getgroups(2)`; on non-unix
/// targets returns an empty array (Node throws there, but Perry's existing
/// `getuid`/etc. family already returns `0` rather than throwing on
/// Windows, so matching that shape keeps the surface consistent). #2135.
#[no_mangle]
pub extern "C" fn js_process_getgroups() -> f64 {
    #[cfg(unix)]
    let gids: Vec<u32> = unsafe {
        let count = libc::getgroups(0, std::ptr::null_mut());
        if count <= 0 {
            Vec::new()
        } else {
            let mut buf: Vec<libc::gid_t> = vec![0; count as usize];
            let got = libc::getgroups(count, buf.as_mut_ptr());
            if got <= 0 {
                Vec::new()
            } else {
                buf.truncate(got as usize);
                buf.into_iter().map(|g| g as u32).collect()
            }
        }
    };
    #[cfg(not(unix))]
    let gids: Vec<u32> = Vec::new();
    let arr = crate::array::js_array_alloc(gids.len() as u32);
    for g in gids {
        crate::array::js_array_push_f64(arr, g as f64);
    }
    f64::from_bits(JSValue::array_ptr(arr).bits())
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
/// TCPSERVERWRAP, ...). Perry doesn't surface that introspection yet —
/// return an empty array. The surface is callable so
/// `typeof process.getActiveResourcesInfo === "function"` holds.
#[no_mangle]
pub extern "C" fn js_process_active_resources_info() -> f64 {
    let arr = crate::array::js_array_alloc(0);
    f64::from_bits(JSValue::pointer(arr as *const u8).bits())
}

/// process.cpuUsage(prior?) -> { user, system } µs.
/// Reads CPU time consumed by the process via getrusage(RUSAGE_SELF) on
/// unix. With a `prior` object, returns the diff (clamped to >= 0).
/// Non-unix targets return `{ user: 0, system: 0 }`.
#[no_mangle]
pub extern "C" fn js_process_cpu_usage(prior: f64) -> f64 {
    // #2013 — Node throws TypeError ERR_INVALID_ARG_TYPE when `prior`
    // is supplied but isn't an object (e.g. `process.cpuUsage('abc')`).
    // Undefined / null fall through to the no-prior baseline read.
    let prior_jv = JSValue::from_bits(prior.to_bits());
    if !prior_jv.is_undefined() && !prior_jv.is_null() && !prior_jv.is_pointer() {
        let message = format!(
            "The \"prevValue\" argument must be of type object. Received {}",
            crate::fs::validate::describe_received(prior)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    let (mut user_us, mut system_us) = read_process_cpu_micros();
    let undef_bits = crate::value::TAG_UNDEFINED;
    if prior.to_bits() != undef_bits && !prior_jv.is_null() {
        let (prev_user, prev_system) = extract_cpu_pair(prior);
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

/// Read `.user` and `.system` (numbers, microseconds) from a JS object
/// — used by `js_process_cpu_usage` to compute diffs. Missing fields
/// or non-numeric values count as 0.
fn extract_cpu_pair(value: f64) -> (f64, f64) {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return (0.0, 0.0);
    }
    let obj_ptr = jv.as_pointer::<u8>() as *mut crate::object::ObjectHeader;
    if obj_ptr.is_null() {
        return (0.0, 0.0);
    }
    let get_num = |name: &str| -> f64 {
        let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let v = crate::object::js_object_get_field_by_name_f64(obj_ptr, key);
        if v.is_nan() {
            0.0
        } else {
            v
        }
    };
    (get_num("user"), get_num("system"))
}

/// process.emitWarning(warning[, type, code, ctor]) -> undefined.
/// Writes a formatted warning to stderr matching Node's shape:
/// `(node:<pid>) <Type> [CODE]: <message>`. Anything that can't be
/// coerced to a string is rendered via `js_jsvalue_to_string`. The 4th
/// `ctor` arg (Node's trace anchor) is accepted but ignored — Perry
/// doesn't capture stack traces here.
#[no_mangle]
pub extern "C" fn js_process_emit_warning(warning: f64, type_name: f64, code: f64) {
    use std::io::Write;
    let undef_bits = crate::value::TAG_UNDEFINED;
    let value_to_string = |v: f64| -> String {
        if v.to_bits() == undef_bits {
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
    };

    let msg = value_to_string(warning);
    let raw_type = value_to_string(type_name);
    let raw_code = value_to_string(code);
    let label = if raw_type.is_empty() {
        "Warning".to_string()
    } else {
        raw_type
    };
    let code_part = if raw_code.is_empty() {
        String::new()
    } else {
        format!(" [{}]", raw_code)
    };
    let pid = std::process::id();
    let line = format!("(node:{}) {}{}: {}\n", pid, label, code_part, msg);
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(line.as_bytes());
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

/// process.loadEnvFile(path?) — read a `.env`-formatted file from disk and
/// merge its `KEY=value` entries into `process.env`. Node 20.12+. With no
/// path, the default is `.env` in the current working directory. Throws a
/// Node-shaped `Error` (`code: "ENOENT"`, `syscall: "open"`) when the file
/// can't be opened. #2135 (#1399 follow-through): previously a no-op that
/// returned undefined so probe-and-call sites didn't crash; with
/// `process.env.X = v` now persisting via std::env (#1344), eager loading
/// is meaningful.
#[no_mangle]
pub extern "C" fn js_process_load_env_file(path_ptr: *const StringHeader) {
    let target = unsafe {
        if path_ptr.is_null() {
            ".env".to_string()
        } else {
            let len = (*path_ptr).byte_len as usize;
            let data = (path_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            let bytes = std::slice::from_raw_parts(data, len);
            match std::str::from_utf8(bytes) {
                Ok(s) => s.to_string(),
                Err(_) => return,
            }
        }
    };
    let contents = match std::fs::read_to_string(&target) {
        Ok(s) => s,
        Err(err) => unsafe {
            throw_load_env_file_open_error(&err, &target);
        },
    };
    for line in contents.lines() {
        let trimmed = line.trim_start();
        // Comments and blank lines.
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((raw_key, raw_value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = raw_key.trim();
        if key.is_empty() {
            continue;
        }
        // Strip a matched surrounding quote pair on the trimmed value;
        // otherwise keep the trimmed text verbatim (so unquoted spaces
        // around `=` are dropped but inner `=` survives — see Node's
        // built-in `.env` parser).
        let value_trimmed = raw_value.trim();
        let value = strip_matched_quotes(value_trimmed);
        std::env::set_var(key, value);
    }
}

fn strip_matched_quotes(s: &str) -> &str {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' || first == b'\'') && first == last {
            return &s[1..s.len() - 1];
        }
    }
    s
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
            "The \"path\" argument must be of type string. Received {}",
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
