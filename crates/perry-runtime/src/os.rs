//! OS module - provides operating system related utility functions

use crate::array::ArrayHeader;
use crate::object::ObjectHeader;
use crate::string::{js_string_from_bytes, StringHeader};
use std::sync::OnceLock;
use std::time::Instant;

/// Process start time for uptime calculation
static PROCESS_START: OnceLock<Instant> = OnceLock::new();

fn get_process_start() -> &'static Instant {
    PROCESS_START.get_or_init(Instant::now)
}

/// Process start time for hrtime() (used as monotonic baseline)
static HRTIME_START: OnceLock<Instant> = OnceLock::new();

fn get_hrtime_start() -> &'static Instant {
    HRTIME_START.get_or_init(Instant::now)
}

/// Get the operating system platform
/// Returns: "darwin", "linux", "win32", "freebsd", etc.
#[no_mangle]
pub extern "C" fn js_os_platform() -> *mut StringHeader {
    #[cfg(target_os = "macos")]
    let platform = "darwin";
    #[cfg(target_os = "ios")]
    let platform = "darwin";
    #[cfg(target_os = "linux")]
    let platform = "linux";
    #[cfg(target_os = "windows")]
    let platform = "win32";
    #[cfg(target_os = "freebsd")]
    let platform = "freebsd";
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "windows",
        target_os = "freebsd"
    )))]
    let platform = "unknown";

    let bytes = platform.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// Get the operating system CPU architecture
/// Returns: "x64", "arm64", "ia32", etc.
#[no_mangle]
pub extern "C" fn js_os_arch() -> *mut StringHeader {
    #[cfg(target_arch = "x86_64")]
    let arch = "x64";
    #[cfg(target_arch = "aarch64")]
    let arch = "arm64";
    #[cfg(target_arch = "x86")]
    let arch = "ia32";
    #[cfg(target_arch = "arm")]
    let arch = "arm";
    #[cfg(not(any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "x86",
        target_arch = "arm"
    )))]
    let arch = "unknown";

    let bytes = arch.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// Get the hostname of the operating system
#[no_mangle]
pub extern "C" fn js_os_hostname() -> *mut StringHeader {
    #[cfg(feature = "full")]
    {
        match hostname::get() {
            Ok(hostname) => {
                let hostname_str = hostname.to_string_lossy();
                let bytes = hostname_str.as_bytes();
                js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
            }
            Err(_) => {
                let default = "localhost";
                js_string_from_bytes(default.as_ptr(), default.len() as u32)
            }
        }
    }
    #[cfg(not(feature = "full"))]
    {
        let default = "localhost";
        js_string_from_bytes(default.as_ptr(), default.len() as u32)
    }
}

/// Get the home directory for the current user
#[no_mangle]
pub extern "C" fn js_os_homedir() -> *mut StringHeader {
    #[cfg(feature = "full")]
    {
        match dirs::home_dir() {
            Some(path) => {
                let path_str = path.to_string_lossy();
                let bytes = path_str.as_bytes();
                js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
            }
            None => {
                // Fallback
                #[cfg(unix)]
                let fallback = "/home";
                #[cfg(windows)]
                let fallback = "C:\\Users";
                #[cfg(not(any(unix, windows)))]
                let fallback = "/";
                js_string_from_bytes(fallback.as_ptr(), fallback.len() as u32)
            }
        }
    }
    #[cfg(not(feature = "full"))]
    {
        let fallback = "/";
        js_string_from_bytes(fallback.as_ptr(), fallback.len() as u32)
    }
}

/// Get the operating system's default directory for temporary files
#[no_mangle]
pub extern "C" fn js_os_tmpdir() -> *mut StringHeader {
    let tmp = std::env::temp_dir();
    let tmp_str = tmp.to_string_lossy();
    let bytes = tmp_str.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// Get the total amount of system memory in bytes
#[no_mangle]
pub extern "C" fn js_os_totalmem() -> f64 {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        use std::mem;
        let mut memsize: u64 = 0;
        let mut size = mem::size_of::<u64>();
        let mib = [libc::CTL_HW, libc::HW_MEMSIZE];
        unsafe {
            libc::sysctl(
                mib.as_ptr() as *mut _,
                2,
                &mut memsize as *mut u64 as *mut _,
                &mut size,
                std::ptr::null_mut(),
                0,
            );
        }
        memsize as f64
    }
    #[cfg(target_os = "linux")]
    {
        unsafe {
            let mut info: libc::sysinfo = std::mem::zeroed();
            libc::sysinfo(&mut info);
            (info.totalram as u64 * info.mem_unit as u64) as f64
        }
    }
    #[cfg(target_os = "windows")]
    {
        #[repr(C)]
        struct MEMORYSTATUSEX {
            dw_length: u32,
            dw_memory_load: u32,
            ull_total_phys: u64,
            ull_avail_phys: u64,
            ull_total_page_file: u64,
            ull_avail_page_file: u64,
            ull_total_virtual: u64,
            ull_avail_virtual: u64,
            ull_avail_extended_virtual: u64,
        }
        extern "system" {
            fn GlobalMemoryStatusEx(lpBuffer: *mut MEMORYSTATUSEX) -> i32;
        }
        unsafe {
            let mut statex: MEMORYSTATUSEX = std::mem::zeroed();
            statex.dw_length = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
            if GlobalMemoryStatusEx(&mut statex) != 0 {
                statex.ull_total_phys as f64
            } else {
                0.0
            }
        }
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "windows"
    )))]
    {
        0.0
    }
}

/// Get the amount of free system memory in bytes
#[no_mangle]
pub extern "C" fn js_os_freemem() -> f64 {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        unsafe {
            let mut vm_info: libc::vm_statistics64 = std::mem::zeroed();
            let mut count = (std::mem::size_of::<libc::vm_statistics64>()
                / std::mem::size_of::<libc::integer_t>()) as u32;
            let ret = libc::host_statistics64(
                libc::mach_host_self(),
                libc::HOST_VM_INFO64,
                &mut vm_info as *mut _ as *mut _,
                &mut count,
            );
            if ret != libc::KERN_SUCCESS {
                return 0.0;
            }
            let page_size = libc::vm_page_size;
            (vm_info.free_count as u64 * page_size as u64) as f64
        }
    }
    #[cfg(target_os = "linux")]
    {
        unsafe {
            let mut info: libc::sysinfo = std::mem::zeroed();
            libc::sysinfo(&mut info);
            (info.freeram as u64 * info.mem_unit as u64) as f64
        }
    }
    #[cfg(target_os = "windows")]
    {
        #[repr(C)]
        struct MEMORYSTATUSEX {
            dw_length: u32,
            dw_memory_load: u32,
            ull_total_phys: u64,
            ull_avail_phys: u64,
            ull_total_page_file: u64,
            ull_avail_page_file: u64,
            ull_total_virtual: u64,
            ull_avail_virtual: u64,
            ull_avail_extended_virtual: u64,
        }
        extern "system" {
            fn GlobalMemoryStatusEx(lpBuffer: *mut MEMORYSTATUSEX) -> i32;
        }
        unsafe {
            let mut statex: MEMORYSTATUSEX = std::mem::zeroed();
            statex.dw_length = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
            if GlobalMemoryStatusEx(&mut statex) != 0 {
                statex.ull_avail_phys as f64
            } else {
                0.0
            }
        }
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "windows"
    )))]
    {
        0.0
    }
}

/// Get the system uptime in seconds
#[no_mangle]
pub extern "C" fn js_os_uptime() -> f64 {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        use std::mem;
        let mut boottime: libc::timeval = unsafe { std::mem::zeroed() };
        let mut size = mem::size_of::<libc::timeval>();
        let mib = [libc::CTL_KERN, libc::KERN_BOOTTIME];
        unsafe {
            libc::sysctl(
                mib.as_ptr() as *mut _,
                2,
                &mut boottime as *mut libc::timeval as *mut _,
                &mut size,
                std::ptr::null_mut(),
                0,
            );
        }
        let mut now: libc::timeval = unsafe { std::mem::zeroed() };
        unsafe { libc::gettimeofday(&mut now, std::ptr::null_mut()) };
        (now.tv_sec - boottime.tv_sec) as f64
    }
    #[cfg(target_os = "linux")]
    {
        unsafe {
            let mut info: libc::sysinfo = std::mem::zeroed();
            libc::sysinfo(&mut info);
            info.uptime as f64
        }
    }
    #[cfg(target_os = "windows")]
    {
        extern "system" {
            fn GetTickCount64() -> u64;
        }
        unsafe { (GetTickCount64() / 1000) as f64 }
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "windows"
    )))]
    {
        0.0
    }
}

/// Get the process uptime in seconds (time since process started)
#[no_mangle]
pub extern "C" fn js_process_uptime() -> f64 {
    get_process_start().elapsed().as_secs_f64()
}

/// Get the current working directory
#[no_mangle]
pub extern "C" fn js_process_cwd() -> *mut StringHeader {
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| String::new());
    let bytes = cwd.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// Get command line arguments as an array of strings
/// Returns: string[] (array of NaN-boxed string pointers)
#[no_mangle]
pub extern "C" fn js_process_argv() -> *mut ArrayHeader {
    use crate::array::{js_array_alloc, js_array_push_f64};
    use crate::value::js_nanbox_string;

    let args: Vec<String> = std::env::args().collect();
    // Match Node.js behavior: argv[0] = binary path (like node path),
    // argv[1] = binary path again (like script path), argv[2+] = user args.
    // Node.js: ["/usr/bin/node", "/path/to/script.js", ...user_args]
    // Compiled: ["/path/to/binary", ...user_args]
    // We insert the binary path twice to shift user args to index 2+.
    let arr = js_array_alloc((args.len() + 1) as u32);

    let mut result = arr;
    if let Some(binary_path) = args.first() {
        // argv[0]: binary path (mimics node executable path)
        let bytes = binary_path.as_bytes();
        let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        let nanboxed = js_nanbox_string(str_ptr as i64);
        result = js_array_push_f64(result, nanboxed);
        // argv[1]: binary path again (mimics script path)
        let str_ptr2 = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        let nanboxed2 = js_nanbox_string(str_ptr2 as i64);
        result = js_array_push_f64(result, nanboxed2);
    }
    // argv[2+]: user arguments
    for arg in args.iter().skip(1) {
        let bytes = arg.as_bytes();
        let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        let nanboxed = js_nanbox_string(str_ptr as i64);
        result = js_array_push_f64(result, nanboxed);
    }

    result
}

/// Get the current process ID (process.pid)
#[no_mangle]
pub extern "C" fn js_process_pid() -> f64 {
    #[cfg(unix)]
    unsafe {
        libc::getpid() as f64
    }
    #[cfg(windows)]
    {
        extern "system" {
            fn GetCurrentProcessId() -> u32;
        }
        unsafe { GetCurrentProcessId() as f64 }
    }
    #[cfg(not(any(unix, windows)))]
    {
        0.0
    }
}

/// Get the parent process ID (process.ppid)
#[no_mangle]
pub extern "C" fn js_process_ppid() -> f64 {
    #[cfg(unix)]
    unsafe {
        libc::getppid() as f64
    }
    #[cfg(windows)]
    {
        // Fallback: return 1 (system process)
        1.0
    }
    #[cfg(not(any(unix, windows)))]
    {
        0.0
    }
}

/// process.version -> string (e.g., "v22.0.0")
#[no_mangle]
pub extern "C" fn js_process_version() -> *mut StringHeader {
    let version = "v22.0.0";
    let bytes = version.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// process.versions -> { node, v8, perry }
#[no_mangle]
pub extern "C" fn js_process_versions() -> f64 {
    use crate::object::{js_object_alloc_with_shape, js_object_set_field};
    use crate::value::{js_nanbox_string, JSValue};

    // Build the object via shape with packed keys
    let packed = b"node\0v8\0perry\0";
    let obj = js_object_alloc_with_shape(0x7FFF_FF21, 3, packed.as_ptr(), packed.len() as u32);

    let nb = |s: &str| -> JSValue {
        let bytes = s.as_bytes();
        let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        JSValue::from_bits(js_nanbox_string(ptr as i64).to_bits())
    };

    js_object_set_field(obj, 0, nb("22.0.0"));
    js_object_set_field(obj, 1, nb("12.4.254.21"));
    js_object_set_field(obj, 2, nb("0.4.71"));

    // Return as NaN-boxed pointer
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// process.hrtime.bigint() -> bigint of nanoseconds
#[no_mangle]
pub extern "C" fn js_process_hrtime_bigint() -> f64 {
    use crate::bigint::js_bigint_from_u64;
    use crate::value::js_nanbox_bigint;

    let elapsed = get_hrtime_start().elapsed();
    // Add a base offset so the value is always > 0 even on the first call
    let nanos = elapsed.as_nanos() as u64 + 1_000_000_000;
    let bi = js_bigint_from_u64(nanos);
    js_nanbox_bigint(bi as i64)
}

/// Storage for process.on('exit', handler) callbacks.
/// We just store the handler pointers; they don't actually fire on real exit.
thread_local! {
    static EXIT_HANDLERS: std::cell::RefCell<Vec<*const crate::closure::ClosureHeader>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// process.on(event, handler) — register an event listener.
#[no_mangle]
pub extern "C" fn js_process_on(
    _event_ptr: *const StringHeader,
    handler: *const crate::closure::ClosureHeader,
) {
    EXIT_HANDLERS.with(|h| h.borrow_mut().push(handler));
}

/// process.nextTick(callback) — schedule callback as a microtask.
#[no_mangle]
pub extern "C" fn js_process_next_tick(callback: *const crate::closure::ClosureHeader) {
    use crate::promise::{js_promise_new, js_promise_schedule_resolve, js_promise_then};
    use crate::value::JSValue;

    let p = js_promise_new();
    let _chain = js_promise_then(p, callback, std::ptr::null());
    js_promise_schedule_resolve(p, f64::from_bits(JSValue::undefined().bits()));
}

/// process.chdir(directory) — change working directory.
#[no_mangle]
pub extern "C" fn js_process_chdir(dir_ptr: *const StringHeader) {
    unsafe {
        if dir_ptr.is_null() {
            return;
        }
        let len = (*dir_ptr).byte_len as usize;
        let data = (dir_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        if let Ok(s) = std::str::from_utf8(bytes) {
            let _ = std::env::set_current_dir(s);
        }
    }
}

/// process.kill(pid, signal?) — send signal to process. signal=0 means existence check.
#[no_mangle]
pub extern "C" fn js_process_kill(pid: f64, signal: f64) {
    let pid_i = pid as i32;
    let sig_i = if signal.is_nan() || signal == 0.0 {
        0
    } else {
        signal as i32
    };
    #[cfg(unix)]
    unsafe {
        libc::kill(pid_i, sig_i);
    }
    #[cfg(windows)]
    {
        let _ = (pid_i, sig_i);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (pid_i, sig_i);
    }
}

/// Coerce a NaN-boxed JSValue to its display bytes, suitable for raw
/// stream writes. Used by `process.stdout.write` / `process.stderr.write`.
/// Mirrors Node's behavior: numbers/booleans/null/undefined coerce to
/// their string form; strings pass through verbatim.
fn jsvalue_to_write_bytes(value: f64) -> Vec<u8> {
    let s_ptr = crate::value::js_jsvalue_to_string(value);
    if s_ptr.is_null() {
        return Vec::new();
    }
    unsafe {
        let header = &*s_ptr;
        let len = header.byte_len as usize;
        let data = (s_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        std::slice::from_raw_parts(data, len).to_vec()
    }
}

/// `write` impl for process.stdout — writes the value's display bytes to
/// fd 1 without appending a newline (matching Node.js semantics — the
/// caller is responsible for `\n`).
extern "C" fn process_stdout_write_stub(
    _closure: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    use std::io::Write;
    let bytes = jsvalue_to_write_bytes(arg);
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    let _ = handle.write_all(&bytes);
    let _ = handle.flush();
    f64::from_bits(0x7FFC_0000_0000_0004) // TAG_TRUE
}

/// `write` impl for process.stderr — same as the stdout stub but
/// targeting fd 2.
extern "C" fn process_stderr_write_stub(
    _closure: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    use std::io::Write;
    let bytes = jsvalue_to_write_bytes(arg);
    let stderr = std::io::stderr();
    let mut handle = stderr.lock();
    let _ = handle.write_all(&bytes);
    let _ = handle.flush();
    f64::from_bits(0x7FFC_0000_0000_0004) // TAG_TRUE
}

/// `write` impl for process.stdin — reading from stdin via `.write` is
/// nonsensical; keep it as a no-op that returns `true` so existing code
/// that calls `process.stdin.write(...)` (rare) doesn't crash.
extern "C" fn process_stdin_write_noop_stub(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    f64::from_bits(0x7FFC_0000_0000_0004) // TAG_TRUE
}

/// Build a stream object with a `write` field bound to the given stub.
/// Each invocation of `process.stdout` / `process.stderr` returns a fresh
/// object whose `write` closure points at the matching fd.
fn build_stream_object_with_write(
    write_stub: extern "C" fn(*const crate::closure::ClosureHeader, f64) -> f64,
) -> *mut crate::object::ObjectHeader {
    use crate::closure::js_closure_alloc;
    use crate::object::{js_object_alloc_with_shape, js_object_set_field};
    use crate::value::JSValue;

    let packed = b"write\0";
    let obj = js_object_alloc_with_shape(0x7FFF_FF22, 1, packed.as_ptr(), packed.len() as u32);
    let closure = js_closure_alloc(write_stub as *const u8, 0);
    let cval = JSValue::pointer(closure as *const u8);
    js_object_set_field(obj, 0, cval);
    obj
}

/// process.stdin -> stream object whose `.write(...)` is a no-op (writing
/// to stdin from the program side has no useful semantics — kept as a
/// crash-free placeholder).
#[no_mangle]
pub extern "C" fn js_process_stdin() -> f64 {
    use crate::value::JSValue;
    let obj = build_stream_object_with_write(process_stdin_write_noop_stub);
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// process.stdout -> stream object whose `.write(s)` writes `s` to fd 1
/// without appending a newline, matching Node.js semantics.
#[no_mangle]
pub extern "C" fn js_process_stdout() -> f64 {
    use crate::value::JSValue;
    let obj = build_stream_object_with_write(process_stdout_write_stub);
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// process.stderr -> stream object whose `.write(s)` writes `s` to fd 2
/// without appending a newline, matching Node.js semantics.
#[no_mangle]
pub extern "C" fn js_process_stderr() -> f64 {
    use crate::value::JSValue;
    let obj = build_stream_object_with_write(process_stderr_write_stub);
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// Get the operating system name
/// Returns: "Darwin", "Linux", "Windows_NT", etc.
#[no_mangle]
pub extern "C" fn js_os_type() -> *mut StringHeader {
    #[cfg(target_os = "macos")]
    let os_type = "Darwin";
    #[cfg(target_os = "ios")]
    let os_type = "Darwin";
    #[cfg(target_os = "linux")]
    let os_type = "Linux";
    #[cfg(target_os = "windows")]
    let os_type = "Windows_NT";
    #[cfg(target_os = "freebsd")]
    let os_type = "FreeBSD";
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "windows",
        target_os = "freebsd"
    )))]
    let os_type = "Unknown";

    let bytes = os_type.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// Get the operating system release
#[no_mangle]
pub extern "C" fn js_os_release() -> *mut StringHeader {
    #[cfg(unix)]
    {
        unsafe {
            let mut info: libc::utsname = std::mem::zeroed();
            if libc::uname(&mut info) == 0 {
                let release = std::ffi::CStr::from_ptr(info.release.as_ptr());
                let release_str = release.to_string_lossy();
                let bytes = release_str.as_bytes();
                js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
            } else {
                let fallback = "unknown";
                js_string_from_bytes(fallback.as_ptr(), fallback.len() as u32)
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        #[repr(C)]
        struct RTL_OSVERSIONINFOW {
            dw_os_version_info_size: u32,
            dw_major_version: u32,
            dw_minor_version: u32,
            dw_build_number: u32,
            dw_platform_id: u32,
            sz_csd_version: [u16; 128],
        }
        extern "system" {
            fn RtlGetVersion(lpVersionInformation: *mut RTL_OSVERSIONINFOW) -> i32;
        }
        unsafe {
            let mut info: RTL_OSVERSIONINFOW = std::mem::zeroed();
            info.dw_os_version_info_size = std::mem::size_of::<RTL_OSVERSIONINFOW>() as u32;
            if RtlGetVersion(&mut info) == 0 {
                let release = format!(
                    "{}.{}.{}",
                    info.dw_major_version, info.dw_minor_version, info.dw_build_number
                );
                let bytes = release.as_bytes();
                js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
            } else {
                let fallback = "unknown";
                js_string_from_bytes(fallback.as_ptr(), fallback.len() as u32)
            }
        }
    }
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        let release = "unknown";
        js_string_from_bytes(release.as_ptr(), release.len() as u32)
    }
}

/// Get the end-of-line marker for the current operating system
#[no_mangle]
pub extern "C" fn js_os_eol() -> *mut StringHeader {
    #[cfg(windows)]
    let eol = "\r\n";
    #[cfg(not(windows))]
    let eol = "\n";

    let bytes = eol.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// Get information about CPUs
/// Returns an array of CPU info objects
/// TODO: Implement properly when dynamic object properties are supported
#[no_mangle]
pub extern "C" fn js_os_cpus() -> *mut ArrayHeader {
    // Return empty array for now - dynamic object properties need different API
    crate::array::js_array_alloc(0)
}

/// Get network interfaces information
/// Returns an object with interface names as keys
/// TODO: Implement properly when dynamic object properties are supported
#[no_mangle]
pub extern "C" fn js_os_network_interfaces() -> *mut ObjectHeader {
    // Return empty object for now - dynamic object properties need different API
    crate::object::js_object_alloc(0, 0)
}

/// Get information about the current user
/// Returns an object with username, uid, gid, shell, homedir
/// TODO: Implement properly when dynamic object properties are supported
#[no_mangle]
pub extern "C" fn js_os_user_info() -> *mut ObjectHeader {
    // Return empty object for now - dynamic object properties need different API
    crate::object::js_object_alloc(0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_os_platform() {
        let platform = js_os_platform();
        assert!(!platform.is_null());
    }

    #[test]
    fn test_os_arch() {
        let arch = js_os_arch();
        assert!(!arch.is_null());
    }

    #[test]
    fn test_os_hostname() {
        let hostname = js_os_hostname();
        assert!(!hostname.is_null());
    }

    #[test]
    fn test_os_homedir() {
        let homedir = js_os_homedir();
        assert!(!homedir.is_null());
    }

    #[test]
    fn test_os_tmpdir() {
        let tmpdir = js_os_tmpdir();
        assert!(!tmpdir.is_null());
    }

    #[test]
    fn test_os_totalmem() {
        let mem = js_os_totalmem();
        assert!(mem > 0.0);
    }

    #[test]
    fn test_os_freemem() {
        let mem = js_os_freemem();
        assert!(mem > 0.0);
    }

    #[test]
    fn test_os_uptime() {
        let uptime = js_os_uptime();
        assert!(uptime >= 0.0);
    }

    #[test]
    fn test_os_type() {
        let os_type = js_os_type();
        assert!(!os_type.is_null());
    }

    #[test]
    fn test_os_release() {
        let release = js_os_release();
        assert!(!release.is_null());
    }

    #[test]
    fn test_os_eol() {
        let eol = js_os_eol();
        assert!(!eol.is_null());
    }
}
