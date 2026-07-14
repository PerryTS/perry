//! OS module - provides operating system related utility functions

use crate::array::ArrayHeader;
use crate::object::ObjectHeader;
use crate::string::{js_string_from_bytes, StringHeader};
use std::cell::RefCell;
use std::collections::HashMap;
#[cfg(unix)]
use std::ffi::CStr;
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

#[path = "os_priority.rs"]
mod os_priority;
pub use os_priority::{js_os_get_priority, js_os_set_priority};

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

/// Get the recommended amount of parallelism for the current process.
#[no_mangle]
pub extern "C" fn js_os_available_parallelism() -> f64 {
    std::thread::available_parallelism()
        .map(|n| n.get() as f64)
        .unwrap_or(1.0)
}

/// Get the CPU endianness Node was compiled for.
#[no_mangle]
pub extern "C" fn js_os_endianness() -> *mut StringHeader {
    let value = if cfg!(target_endian = "little") {
        "LE"
    } else {
        "BE"
    };
    let bytes = value.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// Get the platform-specific null device path.
#[no_mangle]
pub extern "C" fn js_os_dev_null() -> *mut StringHeader {
    let value = if cfg!(windows) {
        "\\\\.\\nul"
    } else {
        "/dev/null"
    };
    let bytes = value.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// Get the machine hardware name.
#[no_mangle]
pub extern "C" fn js_os_machine() -> *mut StringHeader {
    #[cfg(target_arch = "x86_64")]
    let machine = "x86_64";
    #[cfg(target_arch = "aarch64")]
    let machine = "arm64";
    #[cfg(target_arch = "x86")]
    let machine = "i386";
    #[cfg(target_arch = "arm")]
    let machine = "arm";
    #[cfg(target_arch = "riscv64")]
    let machine = "riscv64";
    #[cfg(not(any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "x86",
        target_arch = "arm",
        target_arch = "riscv64"
    )))]
    let machine = "unknown";

    let bytes = machine.as_bytes();
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

/// Get the operating system's default directory for temporary files.
///
/// #3005 — match Node's documented `os.tmpdir()` environment handling rather
/// than delegating to Rust's `std::env::temp_dir()`:
///
/// - **POSIX**: check `TMPDIR`, then `TMP`, then `TEMP` (first non-empty
///   wins), falling back to `/tmp`. A single trailing path separator is
///   stripped, except when the path is just a separator (root).
/// - **Windows**: check `TEMP`, then `TMP`, then `%SystemRoot%`/`%windir%`
///   `\temp`, then `C:\temp`. A single trailing separator (`/` or `\`) is
///   stripped, except for root-like `X:\` paths.
///
/// Empty-string env values are ignored (Node tests `process.env.X` truthiness
/// after coercion, so `TMPDIR=""` skips to the next candidate).
#[no_mangle]
pub extern "C" fn js_os_tmpdir() -> *mut StringHeader {
    let path = compute_tmpdir();
    let bytes = path.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn compute_tmpdir() -> String {
    #[cfg(not(windows))]
    {
        let raw = nonempty_env("TMPDIR")
            .or_else(|| nonempty_env("TMP"))
            .or_else(|| nonempty_env("TEMP"))
            .unwrap_or_else(|| "/tmp".to_string());
        trim_one_trailing_sep_posix(&raw)
    }
    #[cfg(windows)]
    {
        let raw = nonempty_env("TEMP")
            .or_else(|| nonempty_env("TMP"))
            .or_else(|| {
                nonempty_env("SystemRoot")
                    .or_else(|| nonempty_env("windir"))
                    .map(|root| format!("{root}\\temp"))
            })
            .unwrap_or_else(|| "C:\\temp".to_string());
        trim_one_trailing_sep_windows(&raw)
    }
}

/// Strip a single trailing `/`, leaving a lone `/` (root) untouched. Matches
/// Node's `path.length > 1 && path[len-1] === sep` check on POSIX.
#[cfg(not(windows))]
fn trim_one_trailing_sep_posix(path: &str) -> String {
    if path.len() > 1 && path.ends_with('/') {
        path[..path.len() - 1].to_string()
    } else {
        path.to_string()
    }
}

/// Windows variant: strip one trailing `/` or `\`, but leave root-like paths
/// (`X:\`, `\`) intact. Node only trims when the char before the separator is
/// not itself a separator and the result wouldn't be a bare drive root.
#[cfg(windows)]
fn trim_one_trailing_sep_windows(path: &str) -> String {
    let bytes = path.as_bytes();
    let len = bytes.len();
    if len > 1 {
        let last = bytes[len - 1];
        if last == b'/' || last == b'\\' {
            // Do not strip the slash of a drive root like `C:\`.
            let is_drive_root = len == 3 && bytes[1] == b':';
            if !is_drive_root {
                return path[..len - 1].to_string();
            }
        }
    }
    path.to_string()
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
        // #855: libc::mach_host_self is deprecated upstream — call
        // `mach2::mach_init::mach_host_self()` instead. The rest of
        // the host-statistics surface (host_statistics64,
        // vm_statistics64, HOST_VM_INFO64) is still libc-provided
        // because mach2 hasn't surfaced it yet (as of mach2 0.6).
        unsafe {
            let mut vm_info: libc::vm_statistics64 = std::mem::zeroed();
            let mut count = (std::mem::size_of::<libc::vm_statistics64>()
                / std::mem::size_of::<libc::integer_t>()) as u32;
            let ret = libc::host_statistics64(
                mach2::mach_init::mach_host_self(),
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

/// Get 1, 5, and 15 minute system load averages.
#[no_mangle]
pub extern "C" fn js_os_loadavg() -> *mut ArrayHeader {
    use crate::array::{js_array_alloc, js_array_push_f64};

    let arr = js_array_alloc(3);
    #[cfg(all(unix, not(target_os = "android")))]
    {
        let mut loads = [0.0_f64; 3];
        unsafe {
            if libc::getloadavg(loads.as_mut_ptr(), 3) != 3 {
                loads = [0.0, 0.0, 0.0];
            }
        }
        let mut result = arr;
        for load in loads {
            result = js_array_push_f64(result, load);
        }
        result
    }
    #[cfg(not(all(unix, not(target_os = "android"))))]
    {
        // Android's bionic libc does not provide getloadavg(3); Node's
        // os.loadavg() returns [0, 0, 0] there too, so match that.
        let mut result = arr;
        for _ in 0..3 {
            result = js_array_push_f64(result, 0.0);
        }
        result
    }
}

/// Get the operating system version string.
#[no_mangle]
pub extern "C" fn js_os_version() -> *mut StringHeader {
    #[cfg(unix)]
    {
        unsafe {
            let mut info: libc::utsname = std::mem::zeroed();
            if libc::uname(&mut info) == 0 {
                let version = CStr::from_ptr(info.version.as_ptr()).to_string_lossy();
                let bytes = version.as_bytes();
                return js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            }
        }

        let fallback = "unknown";
        js_string_from_bytes(fallback.as_ptr(), fallback.len() as u32)
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
                let version = format!(
                    "Windows {}.{}.{}",
                    info.dw_major_version, info.dw_minor_version, info.dw_build_number
                );
                let bytes = version.as_bytes();
                return js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            }
        }

        let fallback = "Windows";
        js_string_from_bytes(fallback.as_ptr(), fallback.len() as u32)
    }
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        let fallback = "unknown";
        js_string_from_bytes(fallback.as_ptr(), fallback.len() as u32)
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

/// process.versions -> populated `{ node, v8, perry, ...node sub-fields }`.
///
/// Issue #1381: shape-only consumers feature-detect on individual fields
/// (`typeof process.versions.uv === "string"`, `process.versions.modules`
/// for the NAPI ABI gate, etc.). Pre-fix Perry only exposed `node` / `v8`
/// / `perry`, so every other sub-field came back `undefined` and a long
/// tail of npm packages assumed they were running on an outdated Node.
///
/// Values are strings; consumers parse them with `parseInt` / `semver`.
/// Perry's runtime does not embed libuv / OpenSSL / etc., so versions
/// reflect the upstream toolchain spec we target (Node 22) rather than
/// what is statically linked. `"0"` for ABI counters (NAPI, modules)
/// flags "do not assume this is a real Node host" without breaking
/// consumers that only ever check `typeof`.
#[no_mangle]
pub extern "C" fn js_process_versions() -> f64 {
    use crate::object::{js_object_alloc_with_shape, js_object_set_field};
    use crate::value::{js_nanbox_string, JSValue};

    // Build the object via shape with packed keys. Order MUST match the
    // js_object_set_field calls below — slot indices are positional.
    let packed = b"node\0v8\0perry\0uv\0modules\0openssl\0zlib\0ares\0icu\0unicode\0napi\0llhttp\0nghttp2\0undici\0";
    let field_count: u32 = 14;
    let obj = js_object_alloc_with_shape(
        0x7FFF_FF21,
        field_count,
        packed.as_ptr(),
        packed.len() as u32,
    );

    let nb = |s: &str| -> JSValue {
        let bytes = s.as_bytes();
        let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        JSValue::from_bits(js_nanbox_string(ptr as i64).to_bits())
    };

    js_object_set_field(obj, 0, nb("22.0.0"));
    js_object_set_field(obj, 1, nb("12.4.254.21"));
    js_object_set_field(obj, 2, nb("0.4.71"));
    js_object_set_field(obj, 3, nb("1.51.0")); // uv (target spec; Perry doesn't link libuv)
    js_object_set_field(obj, 4, nb("0")); // modules (NAPI ABI counter — "do not assume real Node host")
    js_object_set_field(obj, 5, nb("0")); // openssl (Perry's TLS uses rustls; no OpenSSL link)
    js_object_set_field(obj, 6, nb("1.3.0")); // zlib
    js_object_set_field(obj, 7, nb("0")); // ares
    js_object_set_field(obj, 8, nb("0")); // icu
    js_object_set_field(obj, 9, nb("16.0")); // unicode
    js_object_set_field(obj, 10, nb("0")); // napi
    js_object_set_field(obj, 11, nb("0")); // llhttp
    js_object_set_field(obj, 12, nb("0")); // nghttp2
    js_object_set_field(obj, 13, nb("0")); // undici

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

/// process.hrtime(prior?) -> [seconds, nanoseconds] integer array.
/// Uses the same monotonic baseline as `hrtime.bigint()` (`get_hrtime_start`)
/// — they share a single point of origin, so successive readings on
/// either form are comparable. With a prior `[secs, nanos]` array, the
/// diff is returned (clamped to non-negative).
#[no_mangle]
pub extern "C" fn js_process_hrtime(prior: f64) -> f64 {
    use crate::value::JSValue;
    // #2013 — Node throws TypeError ERR_INVALID_ARG_TYPE when `prior`
    // is supplied but isn't an Array (e.g. `process.hrtime('abc')`).
    // Undefined falls through to the no-prior baseline read; an array
    // with NaN-y entries is silently zeroed by `extract_hrtime_prior`,
    // matching Node's lenient numeric coercion inside the array.
    let prior_jv = JSValue::from_bits(prior.to_bits());
    if !prior_jv.is_undefined() && !crate::process::is_array_value(prior_jv) {
        let message = format!(
            "The \"time\" argument must be an instance of Array. Received {}",
            crate::fs::validate::describe_received(prior)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    // #3039 — a supplied prior tuple must have exactly two elements; Node
    // throws RangeError [ERR_OUT_OF_RANGE] ("It must be 2. Received <len>")
    // for any other length.
    if !prior_jv.is_undefined() {
        let arr = prior_jv.as_pointer::<crate::array::ArrayHeader>();
        let len = crate::array::js_array_length(arr);
        if len != 2 {
            let message = format!(
                "The value of \"time\" is out of range. It must be 2. Received {}",
                len
            );
            crate::fs::validate::throw_range_error_with_code(&message);
        }
    }
    let elapsed = get_hrtime_start().elapsed();
    let total_ns = elapsed.as_nanos() as u64 + 1_000_000_000;
    let mut secs = (total_ns / 1_000_000_000) as i64;
    let mut nanos = (total_ns % 1_000_000_000) as i64;

    let undef_bits = crate::value::TAG_UNDEFINED;
    if prior.to_bits() != undef_bits {
        let (prev_s, prev_ns) = extract_hrtime_prior(prior);
        let mut diff_s = secs - prev_s;
        let mut diff_ns = nanos - prev_ns;
        if diff_ns < 0 {
            diff_s -= 1;
            diff_ns += 1_000_000_000;
        }
        if diff_s < 0 {
            diff_s = 0;
            diff_ns = 0;
        }
        secs = diff_s;
        nanos = diff_ns;
    }

    let arr = crate::array::js_array_alloc(2);
    let arr = crate::array::js_array_push(arr, JSValue::number(secs as f64));
    let arr = crate::array::js_array_push(arr, JSValue::number(nanos as f64));
    f64::from_bits(JSValue::pointer(arr as *const u8).bits())
}

/// Read the two numeric fields of a prior hrtime tuple (an array). The
/// array's two leading slots are coerced to integer seconds and nanos.
fn extract_hrtime_prior(value: f64) -> (i64, i64) {
    use crate::value::JSValue;
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return (0, 0);
    }
    let arr = jv.as_pointer::<crate::array::ArrayHeader>();
    if arr.is_null() {
        return (0, 0);
    }
    let secs = crate::array::js_array_get(arr, 0).to_number();
    let nanos = crate::array::js_array_get(arr, 1).to_number();
    let to_i64 = |v: f64| -> i64 {
        if v.is_nan() || v.is_infinite() {
            0
        } else {
            v as i64
        }
    };
    (to_i64(secs), to_i64(nanos))
}

// `process` EventEmitter surface lives in the `os_process_emitter`
// submodule; split out to keep this file under the 2000-line gate.
mod os_process_emitter;
pub use os_process_emitter::*;

// `process.chdir()` + its Node-shaped error live in the `chdir` submodule
// (#2135); split out to keep this file under the 2000-line gate.
mod chdir;
pub use chdir::js_process_chdir;

// Signal normalization is shared with `util.convertProcessSignalToExitCode`.
mod signal;
pub use signal::{js_process_kill, js_util_convert_process_signal_to_exit_code};

#[path = "os_process_streams.rs"]
mod process_streams;
pub use process_streams::{
    js_process_stderr, js_process_stdin, js_process_stdout, mark_process_stdin_destroyed,
    pump_process_stdin, scan_process_stream_singleton_roots_mut, set_process_stdin_raw_state,
    stdin_chunk_jsvalue, stdin_has_encoding, stdin_is_detached, stdin_push_bytes,
};

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
#[no_mangle]
pub extern "C" fn js_os_cpus() -> *mut ArrayHeader {
    use crate::array::{js_array_alloc, js_array_push};
    use crate::object::{js_object_alloc_with_shape, js_object_set_field};
    use crate::value::{js_nanbox_string, JSValue};

    const CPU_TIMES_SHAPE_ID: u32 = 0x7FFF_FF25;
    const CPU_INFO_SHAPE_ID: u32 = 0x7FFF_FF26;

    #[derive(Clone, Copy, Default)]
    struct CpuTimes {
        user: f64,
        nice: f64,
        sys: f64,
        idle: f64,
        irq: f64,
    }

    struct CpuInfo {
        model: String,
        speed: f64,
        times: CpuTimes,
    }

    fn nanbox_string_value(s: &str) -> JSValue {
        let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
        JSValue::from_bits(js_nanbox_string(ptr as i64).to_bits())
    }

    fn cpu_count() -> usize {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .max(1)
    }

    #[cfg(target_os = "linux")]
    fn host_cpu_infos() -> Vec<CpuInfo> {
        use std::io::Read;

        let mut infos = Vec::new();
        if let Ok(mut stat) = std::fs::File::open("/proc/stat") {
            let mut contents = String::new();
            let _ = stat.read_to_string(&mut contents);
            for line in contents.lines() {
                let mut parts = line.split_whitespace();
                let Some(label) = parts.next() else {
                    continue;
                };
                if !label.starts_with("cpu")
                    || label == "cpu"
                    || !label[3..].chars().all(|c| c.is_ascii_digit())
                {
                    continue;
                }
                let read =
                    |slot: Option<&str>| slot.and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                let user = read(parts.next());
                let nice = read(parts.next());
                let sys = read(parts.next());
                let idle = read(parts.next());
                let _iowait = read(parts.next());
                let irq = read(parts.next());
                // Linux /proc/stat values are clock ticks. Deno and Node expose
                // milliseconds; 10ms is correct on common Linux HZ=100 hosts and
                // is enough for Perry's current shape-level parity tests.
                infos.push(CpuInfo {
                    model: String::new(),
                    speed: 0.0,
                    times: CpuTimes {
                        user: user * 10.0,
                        nice: nice * 10.0,
                        sys: sys * 10.0,
                        idle: idle * 10.0,
                        irq: irq * 10.0,
                    },
                });
            }
        }

        let mut models = Vec::new();
        if let Ok(cpuinfo) = std::fs::read_to_string("/proc/cpuinfo") {
            for line in cpuinfo.lines() {
                if let Some((key, value)) = line.split_once(':') {
                    if key.trim() == "model name" {
                        models.push(value.trim().to_string());
                    }
                }
            }
        }

        for (index, info) in infos.iter_mut().enumerate() {
            info.model = models
                .get(index)
                .cloned()
                .unwrap_or_else(|| "unknown".to_string());
            let speed_path = format!("/sys/devices/system/cpu/cpu{index}/cpufreq/scaling_cur_freq");
            info.speed = std::fs::read_to_string(speed_path)
                .ok()
                .and_then(|s| s.trim().parse::<f64>().ok())
                .map(|khz| khz / 1000.0)
                .unwrap_or(0.0);
        }

        if infos.is_empty() {
            fallback_cpu_infos()
        } else {
            infos
        }
    }

    // #3007 — populate real per-core CPU data on macOS instead of returning
    // shape-only fallback zeros. Model comes from `machdep.cpu.brand_string`
    // (e.g. "Apple M1 Max"); speed from `hw.cpufrequency` in MHz, with a
    // libuv-style 2400 MHz fallback on Apple Silicon where the sysctl is
    // absent; per-core `times` from `host_processor_info` /
    // `PROCESSOR_CPU_LOAD_INFO` (CPU ticks → milliseconds via the 100 Hz
    // clock, matching how Node/libuv expose the counters).
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    fn host_cpu_infos() -> Vec<CpuInfo> {
        fn sysctl_string(name: &str) -> Option<String> {
            let cname = std::ffi::CString::new(name).ok()?;
            let mut size: usize = 0;
            unsafe {
                if libc::sysctlbyname(
                    cname.as_ptr(),
                    std::ptr::null_mut(),
                    &mut size,
                    std::ptr::null_mut(),
                    0,
                ) != 0
                    || size == 0
                {
                    return None;
                }
                let mut buf = vec![0u8; size];
                if libc::sysctlbyname(
                    cname.as_ptr(),
                    buf.as_mut_ptr() as *mut _,
                    &mut size,
                    std::ptr::null_mut(),
                    0,
                ) != 0
                {
                    return None;
                }
                // Drop the trailing NUL terminator sysctl includes in `size`.
                while buf.last() == Some(&0) {
                    buf.pop();
                }
                String::from_utf8(buf).ok()
            }
        }

        fn sysctl_u64(name: &str) -> Option<u64> {
            let cname = std::ffi::CString::new(name).ok()?;
            let mut value: u64 = 0;
            let mut size = std::mem::size_of::<u64>();
            unsafe {
                if libc::sysctlbyname(
                    cname.as_ptr(),
                    &mut value as *mut u64 as *mut _,
                    &mut size,
                    std::ptr::null_mut(),
                    0,
                ) != 0
                {
                    return None;
                }
            }
            Some(value)
        }

        let model = sysctl_string("machdep.cpu.brand_string")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown".to_string());
        // `hw.cpufrequency` is in Hz on Intel; absent on Apple Silicon — libuv
        // reports 2400 MHz there, so mirror that to keep `speed > 0`.
        let speed = sysctl_u64("hw.cpufrequency")
            .map(|hz| (hz / 1_000_000) as f64)
            .filter(|mhz| *mhz > 0.0)
            .unwrap_or(2400.0);

        // Per-core tick counters via the Mach host processor info API.
        let mut proc_count: libc::natural_t = 0;
        let mut info_array: libc::processor_info_array_t = std::ptr::null_mut();
        let mut info_count: libc::mach_msg_type_number_t = 0;
        let kr = unsafe {
            libc::host_processor_info(
                mach2::mach_init::mach_host_self(),
                libc::PROCESSOR_CPU_LOAD_INFO,
                &mut proc_count,
                &mut info_array,
                &mut info_count,
            )
        };
        if kr != libc::KERN_SUCCESS || info_array.is_null() || proc_count == 0 {
            // Could not read tick counters; still return real model/speed.
            return (0..cpu_count())
                .map(|_| CpuInfo {
                    model: model.clone(),
                    speed,
                    times: CpuTimes::default(),
                })
                .collect();
        }

        const CPU_STATE_MAX: usize = libc::CPU_STATE_MAX as usize;
        let mut infos = Vec::with_capacity(proc_count as usize);
        // Ticks → milliseconds: macOS clock is 100 Hz, so 1 tick = 10 ms.
        let tick_ms = 10.0;
        for i in 0..proc_count as usize {
            let base = unsafe { info_array.add(i * CPU_STATE_MAX) };
            let read =
                |state: usize| -> f64 { unsafe { *base.add(state) as u32 as f64 * tick_ms } };
            infos.push(CpuInfo {
                model: model.clone(),
                speed,
                times: CpuTimes {
                    user: read(libc::CPU_STATE_USER as usize),
                    nice: read(libc::CPU_STATE_NICE as usize),
                    sys: read(libc::CPU_STATE_SYSTEM as usize),
                    idle: read(libc::CPU_STATE_IDLE as usize),
                    irq: 0.0,
                },
            });
        }

        // Release the vm_allocate'd processor info buffer.
        unsafe {
            libc::vm_deallocate(
                mach2::traps::mach_task_self(),
                info_array as libc::vm_address_t,
                (info_count as usize * std::mem::size_of::<libc::integer_t>()) as libc::vm_size_t,
            );
        }

        if infos.is_empty() {
            fallback_cpu_infos()
        } else {
            infos
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "ios")))]
    fn host_cpu_infos() -> Vec<CpuInfo> {
        fallback_cpu_infos()
    }

    fn fallback_cpu_infos() -> Vec<CpuInfo> {
        (0..cpu_count())
            .map(|_| CpuInfo {
                model: "unknown".to_string(),
                speed: 0.0,
                times: CpuTimes::default(),
            })
            .collect()
    }

    fn build_times_object(times: CpuTimes) -> *mut ObjectHeader {
        let packed = b"user\0nice\0sys\0idle\0irq\0";
        let obj =
            js_object_alloc_with_shape(CPU_TIMES_SHAPE_ID, 5, packed.as_ptr(), packed.len() as u32);
        js_object_set_field(obj, 0, JSValue::number(times.user));
        js_object_set_field(obj, 1, JSValue::number(times.nice));
        js_object_set_field(obj, 2, JSValue::number(times.sys));
        js_object_set_field(obj, 3, JSValue::number(times.idle));
        js_object_set_field(obj, 4, JSValue::number(times.irq));
        obj
    }

    fn build_cpu_object(info: CpuInfo) -> *mut ObjectHeader {
        let scope = crate::gc::RuntimeHandleScope::new();
        let model = nanbox_string_value(&info.model);
        let model_handle = scope.root_nanbox_u64(model.bits());
        let times = build_times_object(info.times);
        let times_handle = scope.root_raw_mut_ptr(times);

        let packed = b"model\0speed\0times\0";
        let obj =
            js_object_alloc_with_shape(CPU_INFO_SHAPE_ID, 3, packed.as_ptr(), packed.len() as u32);
        let times = times_handle.get_raw_mut_ptr::<ObjectHeader>();
        js_object_set_field(obj, 0, JSValue::from_bits(model_handle.get_nanbox_u64()));
        js_object_set_field(obj, 1, JSValue::number(info.speed));
        js_object_set_field(obj, 2, JSValue::pointer(times as *const u8));
        obj
    }

    let infos = host_cpu_infos();
    let mut arr = js_array_alloc(infos.len() as u32);
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(arr);
    for info in infos {
        let obj = build_cpu_object(info);
        let obj_handle = scope.root_raw_mut_ptr(obj);
        let current = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
        arr = js_array_push(
            current,
            JSValue::pointer(obj_handle.get_raw_mut_ptr::<ObjectHeader>() as *const u8),
        );
        arr_handle.set_raw_mut_ptr(arr);
    }
    arr_handle.get_raw_mut_ptr::<ArrayHeader>()
}

// `os.networkInterfaces()` lives in a sibling module to keep this file under
// the 2000-line source gate (#3006).
#[path = "os_network.rs"]
mod os_network;
pub use os_network::js_os_network_interfaces;

/// Get information about the current user
/// Returns an object with username, uid, gid, shell, homedir
/// TODO: Implement properly when dynamic object properties are supported
#[no_mangle]
pub extern "C" fn js_os_user_info() -> *mut ObjectHeader {
    js_os_user_info_impl(false)
}

#[no_mangle]
pub extern "C" fn js_os_user_info_buffer() -> *mut ObjectHeader {
    js_os_user_info_impl(true)
}

/// `os.userInfo(options)` with a runtime-supplied `options` value (#3004).
///
/// The static-literal `{ encoding: "buffer" }` form lowers directly to
/// `js_os_user_info_buffer`, but when the options object comes from a
/// variable, a function return, or a computed key, the call reaches the
/// dynamic native-module dispatch path with the raw NaN-boxed argument. Node
/// inspects `options.encoding` at runtime and returns Buffer text fields only
/// when it is exactly the string `"buffer"`; every other value (including a
/// missing/`undefined` options object or an unrecognized encoding) returns
/// strings. We mirror that: read `options.encoding`, and switch to the buffer
/// impl only on an exact `"buffer"` match.
///
/// # Safety
/// `opts_bits` must be a valid NaN-boxed JS value (or `undefined`).
#[no_mangle]
pub extern "C" fn js_os_user_info_options(opts_bits: i64) -> *mut ObjectHeader {
    js_os_user_info_impl(options_request_buffer(opts_bits))
}

/// Return true when a runtime `options` value selects the `"buffer"` encoding.
/// Non-object options, a missing `encoding`, or any non-`"buffer"` encoding all
/// yield strings (false), matching Node's lenient handling.
fn options_request_buffer(opts_bits: i64) -> bool {
    let value = f64::from_bits(opts_bits as u64);
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let obj = jv.as_pointer::<ObjectHeader>();
    if obj.is_null() {
        return false;
    }
    let key = "encoding";
    let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let encoding = crate::object::js_object_get_field_by_name(obj, key_ptr);
    if !encoding.is_string() && !encoding.is_short_string() {
        return false;
    }
    let enc_ptr = crate::value::js_get_string_pointer_unified(f64::from_bits(encoding.bits()))
        as *const StringHeader;
    read_event_name(enc_ptr).as_deref() == Some("buffer")
}

#[used]
static KEEP_OS_USER_INFO_OPTIONS: extern "C" fn(i64) -> *mut ObjectHeader = js_os_user_info_options;

fn js_os_user_info_impl(buffer_encoding: bool) -> *mut ObjectHeader {
    use crate::object::{js_object_alloc_with_shape, js_object_set_field};
    use crate::value::JSValue;

    let packed = b"uid\0gid\0username\0homedir\0shell\0";
    let obj = js_object_alloc_with_shape(0x7FFF_FF24, 5, packed.as_ptr(), packed.len() as u32);

    #[cfg(unix)]
    let uid = unsafe { libc::geteuid() as f64 };
    #[cfg(not(unix))]
    let uid = -1.0;
    #[cfg(unix)]
    let gid = unsafe { libc::getegid() as f64 };
    #[cfg(not(unix))]
    let gid = -1.0;

    let username = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default();
    let homedir = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| {
            let ptr = js_os_homedir();
            unsafe {
                let len = (*ptr).byte_len as usize;
                let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
            }
        });
    #[cfg(unix)]
    let shell = std::env::var("SHELL").unwrap_or_default();
    #[cfg(not(unix))]
    let shell = String::new();

    let string_value = |s: &str| -> JSValue {
        let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
        JSValue::string_ptr(ptr)
    };
    let buffer_value = |s: &str| -> JSValue {
        let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
        let buf = crate::buffer::js_buffer_from_string(ptr as *const StringHeader, 0);
        JSValue::pointer(buf as *const u8)
    };
    let text_value = |s: &str| -> JSValue {
        if buffer_encoding {
            buffer_value(s)
        } else {
            string_value(s)
        }
    };

    js_object_set_field(obj, 0, JSValue::number(uid));
    js_object_set_field(obj, 1, JSValue::number(gid));
    js_object_set_field(obj, 2, text_value(&username));
    js_object_set_field(obj, 3, text_value(&homedir));
    #[cfg(unix)]
    js_object_set_field(obj, 4, text_value(&shell));
    #[cfg(not(unix))]
    js_object_set_field(obj, 4, JSValue::null());

    obj
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
    fn test_os_version() {
        let version = js_os_version();
        assert!(!version.is_null());
    }

    #[test]
    fn test_os_eol() {
        let eol = js_os_eol();
        assert!(!eol.is_null());
    }
}
