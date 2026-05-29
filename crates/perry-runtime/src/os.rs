//! OS module - provides operating system related utility functions

use crate::array::ArrayHeader;
use crate::object::ObjectHeader;
use crate::string::{js_string_from_bytes, StringHeader};
use std::cell::RefCell;
use std::collections::HashMap;
#[cfg(unix)]
use std::ffi::CStr;
use std::net::{Ipv4Addr, Ipv6Addr};
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

#[derive(Clone, Copy)]
struct ProcessListener {
    callback: *const crate::closure::ClosureHeader,
    once: bool,
}

struct ProcessEmitter {
    events: HashMap<String, Vec<ProcessListener>>,
    event_order: Vec<String>,
    max_listeners: i32,
}

impl ProcessEmitter {
    fn new() -> Self {
        Self {
            events: HashMap::new(),
            event_order: Vec::new(),
            max_listeners: 10,
        }
    }

    fn ensure_event_order(&mut self, event: &str) {
        if !self.event_order.iter().any(|name| name == event) {
            self.event_order.push(event.to_string());
        }
    }

    fn prune_event_if_empty(&mut self, event: &str) {
        if self
            .events
            .get(event)
            .map(|listeners| listeners.is_empty())
            .unwrap_or(false)
        {
            self.events.remove(event);
            self.event_order.retain(|name| name != event);
        }
    }
}

thread_local! {
    static PROCESS_EMITTER: RefCell<ProcessEmitter> = RefCell::new(ProcessEmitter::new());
}

fn read_event_name(event_ptr: *const StringHeader) -> Option<String> {
    if event_ptr.is_null() {
        return None;
    }
    unsafe {
        let len = (*event_ptr).byte_len as usize;
        let data = (event_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        std::str::from_utf8(bytes).ok().map(|s| s.to_string())
    }
}

fn process_namespace_value() -> f64 {
    crate::object::js_create_native_module_namespace(b"process".as_ptr(), "process".len())
}

fn register_process_listener(
    event_ptr: *const StringHeader,
    callback: *const crate::closure::ClosureHeader,
    once: bool,
    prepend: bool,
) -> f64 {
    let Some(event) = read_event_name(event_ptr) else {
        return process_namespace_value();
    };
    if callback.is_null() {
        return process_namespace_value();
    }

    PROCESS_EMITTER.with(|emitter| {
        let mut emitter = emitter.borrow_mut();
        emitter.ensure_event_order(&event);
        let listener = ProcessListener { callback, once };
        let listeners = emitter.events.entry(event).or_default();
        if prepend {
            listeners.insert(0, listener);
        } else {
            listeners.push(listener);
        }
    });
    process_namespace_value()
}

fn boxed_bool(value: bool) -> f64 {
    f64::from_bits(if value {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    })
}

fn listener_array(event_ptr: *const StringHeader, _raw: bool) -> *mut ArrayHeader {
    let Some(event) = read_event_name(event_ptr) else {
        return crate::array::js_array_alloc(0);
    };
    let callbacks = PROCESS_EMITTER.with(|emitter| {
        emitter
            .borrow()
            .events
            .get(&event)
            .map(|listeners| {
                listeners
                    .iter()
                    .map(|listener| listener.callback)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    let mut arr = crate::array::js_array_alloc(callbacks.len() as u32);
    for callback in callbacks {
        arr =
            crate::array::js_array_push(arr, crate::value::JSValue::pointer(callback as *const u8));
    }
    arr
}

fn collect_emit_args(args: *const ArrayHeader) -> Vec<f64> {
    if args.is_null() {
        return Vec::new();
    }
    let len = crate::array::js_array_length(args) as usize;
    let mut values = Vec::with_capacity(len);
    for i in 0..len {
        values.push(crate::array::js_array_get_f64(args, i as u32));
    }
    values
}

fn emit_process_event(event: &str, args: &[f64]) -> bool {
    let listeners = PROCESS_EMITTER.with(|emitter| {
        let mut emitter = emitter.borrow_mut();
        let Some(listeners) = emitter.events.get_mut(event) else {
            return Vec::new();
        };
        let snapshot = listeners.clone();
        if snapshot.iter().any(|listener| listener.once) {
            listeners.retain(|listener| !listener.once);
        }
        emitter.prune_event_if_empty(event);
        snapshot
    });

    if listeners.is_empty() {
        if event == "error" {
            crate::exception::js_throw(
                args.first()
                    .copied()
                    .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED)),
            );
        }
        return false;
    }

    for listener in listeners {
        unsafe {
            crate::closure::js_closure_call_array(
                listener.callback as i64,
                args.as_ptr(),
                args.len() as i64,
            );
        }
    }
    true
}

/// process.on(event, handler) — register an event listener.
#[no_mangle]
pub extern "C" fn js_process_on(
    event_ptr: *const StringHeader,
    handler: *const crate::closure::ClosureHeader,
) -> f64 {
    register_process_listener(event_ptr, handler, false, false)
}

/// process.addListener(event, handler) — alias for on().
#[no_mangle]
pub extern "C" fn js_process_add_listener(
    event_ptr: *const StringHeader,
    handler: *const crate::closure::ClosureHeader,
) -> f64 {
    register_process_listener(event_ptr, handler, false, false)
}

/// process.once(event, handler) — one-shot listener (Node parity).
#[no_mangle]
pub extern "C" fn js_process_once(
    event_ptr: *const StringHeader,
    handler: *const crate::closure::ClosureHeader,
) -> f64 {
    register_process_listener(event_ptr, handler, true, false)
}

#[no_mangle]
pub extern "C" fn js_process_prepend_listener(
    event_ptr: *const StringHeader,
    handler: *const crate::closure::ClosureHeader,
) -> f64 {
    register_process_listener(event_ptr, handler, false, true)
}

#[no_mangle]
pub extern "C" fn js_process_prepend_once_listener(
    event_ptr: *const StringHeader,
    handler: *const crate::closure::ClosureHeader,
) -> f64 {
    register_process_listener(event_ptr, handler, true, true)
}

/// Emit the synthetic `beforeExit` event with the would-be exit code as a
/// numeric argument. Called from the codegen-emitted event-loop epilogue
/// once the loop has drained all real work (#2135). Node's semantics fire
/// this hook *only* when the event loop is exiting on its own; an explicit
/// `process.exit()` skips it. Perry's `js_process_exit` calls `libc::_exit`
/// directly without going through this hook, so that contract is preserved.
#[no_mangle]
pub extern "C" fn js_process_emit_before_exit(code: f64) {
    let _ = emit_process_event("beforeExit", &[code]);
}

#[no_mangle]
pub extern "C" fn js_process_emit(event_ptr: *const StringHeader, args: *const ArrayHeader) -> f64 {
    let Some(event) = read_event_name(event_ptr) else {
        return boxed_bool(false);
    };
    let values = collect_emit_args(args);
    boxed_bool(emit_process_event(&event, &values))
}

#[no_mangle]
pub extern "C" fn js_process_remove_listener(
    event_ptr: *const StringHeader,
    handler: *const crate::closure::ClosureHeader,
) -> f64 {
    if let Some(event) = read_event_name(event_ptr) {
        PROCESS_EMITTER.with(|emitter| {
            let mut emitter = emitter.borrow_mut();
            if let Some(listeners) = emitter.events.get_mut(&event) {
                if let Some(pos) = listeners
                    .iter()
                    .rposition(|listener| listener.callback == handler)
                {
                    listeners.remove(pos);
                }
            }
            emitter.prune_event_if_empty(&event);
        });
    }
    process_namespace_value()
}

#[no_mangle]
pub extern "C" fn js_process_off(
    event_ptr: *const StringHeader,
    handler: *const crate::closure::ClosureHeader,
) -> f64 {
    js_process_remove_listener(event_ptr, handler)
}

#[no_mangle]
pub extern "C" fn js_process_remove_all_listeners(event_ptr: *const StringHeader) -> f64 {
    PROCESS_EMITTER.with(|emitter| {
        let mut emitter = emitter.borrow_mut();
        if let Some(event) = read_event_name(event_ptr) {
            emitter.events.remove(&event);
            emitter.event_order.retain(|name| name != &event);
        } else {
            emitter.events.clear();
            emitter.event_order.clear();
        }
    });
    process_namespace_value()
}

#[no_mangle]
pub extern "C" fn js_process_listener_count(
    event_ptr: *const StringHeader,
    handler: *const crate::closure::ClosureHeader,
) -> f64 {
    let Some(event) = read_event_name(event_ptr) else {
        return 0.0;
    };
    PROCESS_EMITTER.with(|emitter| {
        let emitter = emitter.borrow();
        let Some(listeners) = emitter.events.get(&event) else {
            return 0.0;
        };
        if handler.is_null() {
            listeners.len() as f64
        } else {
            listeners
                .iter()
                .filter(|listener| listener.callback == handler)
                .count() as f64
        }
    })
}

#[no_mangle]
pub extern "C" fn js_process_listeners(event_ptr: *const StringHeader) -> *mut ArrayHeader {
    listener_array(event_ptr, false)
}

#[no_mangle]
pub extern "C" fn js_process_raw_listeners(event_ptr: *const StringHeader) -> *mut ArrayHeader {
    listener_array(event_ptr, true)
}

#[no_mangle]
pub extern "C" fn js_process_event_names() -> *mut ArrayHeader {
    let names = PROCESS_EMITTER.with(|emitter| emitter.borrow().event_order.clone());
    let mut arr = crate::array::js_array_alloc(names.len() as u32);
    for name in names {
        let s = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        arr = crate::array::js_array_push(arr, crate::value::JSValue::string_ptr(s));
    }
    arr
}

#[no_mangle]
pub extern "C" fn js_process_set_max_listeners(value: f64) -> f64 {
    if value.is_finite() && value >= 0.0 {
        PROCESS_EMITTER.with(|emitter| {
            emitter.borrow_mut().max_listeners = value as i32;
        });
    }
    process_namespace_value()
}

#[no_mangle]
pub extern "C" fn js_process_get_max_listeners() -> f64 {
    PROCESS_EMITTER.with(|emitter| emitter.borrow().max_listeners as f64)
}

pub fn scan_process_event_listener_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    PROCESS_EMITTER.with(|emitter| {
        let mut emitter = emitter.borrow_mut();
        for listeners in emitter.events.values_mut() {
            for listener in listeners {
                visitor.visit_raw_const_ptr_slot(&mut listener.callback);
            }
        }
    });
}

#[cfg(test)]
pub(crate) fn test_clear_process_event_listeners() {
    PROCESS_EMITTER.with(|emitter| {
        *emitter.borrow_mut() = ProcessEmitter::new();
    });
}

#[cfg(test)]
pub(crate) fn test_seed_process_event_listener_root(
    callback: *const crate::closure::ClosureHeader,
) {
    PROCESS_EMITTER.with(|emitter| {
        let mut emitter = emitter.borrow_mut();
        emitter.ensure_event_order("__test__");
        emitter.events.insert(
            "__test__".to_string(),
            vec![ProcessListener {
                callback,
                once: false,
            }],
        );
    });
}

#[cfg(test)]
pub(crate) fn test_process_event_listener_root_snapshot() -> usize {
    PROCESS_EMITTER.with(|emitter| {
        emitter
            .borrow()
            .events
            .get("__test__")
            .and_then(|listeners| listeners.first())
            .map(|listener| listener.callback as usize)
            .unwrap_or(0)
    })
}

pub fn emit_process_uncaught_exception(error: f64) {
    emit_process_event("uncaughtException", &[error]);
}

/// process.nextTick(callback) — schedule callback as a microtask.
#[no_mangle]
pub extern "C" fn js_process_next_tick(callback: *const crate::closure::ClosureHeader) {
    crate::builtins::js_queue_next_tick(callback as i64);
}

// `process.chdir()` + its Node-shaped error live in the `chdir` submodule
// (#2135); split out to keep this file under the 2000-line gate.
mod chdir;
pub use chdir::js_process_chdir;

/// process.kill(pid, signal?) — send signal to process. signal=0 means existence check.
#[no_mangle]
pub extern "C" fn js_process_kill(pid: f64, signal: f64) {
    // #2013 — pid must be a number (`validateInteger(pid, 'pid')`); a
    // non-numeric pid throws TypeError ERR_INVALID_ARG_TYPE before any
    // syscall runs. Signal must be a string-or-number; an object/array
    // throws TypeError ERR_UNKNOWN_SIGNAL (Node's specific shape — the
    // signal-name lookup table runs before the numeric coercion).
    use crate::fs::validate::{describe_received, is_numeric, throw_type_error_with_code};
    let pid_jv = crate::value::JSValue::from_bits(pid.to_bits());
    if !is_numeric(pid_jv) {
        let message = format!(
            "The \"pid\" argument must be of type number. Received {}",
            describe_received(pid)
        );
        throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    let sig_jv = crate::value::JSValue::from_bits(signal.to_bits());
    if !sig_jv.is_undefined() && !is_numeric(sig_jv) && !sig_jv.is_any_string() {
        // Match Node's "Unknown signal: <stringify>" message for the
        // object/array/boolean shapes that don't reach the numeric path.
        let received = describe_received(signal);
        let message = format!("Unknown signal: {}", received);
        throw_type_error_with_code(&message, "ERR_UNKNOWN_SIGNAL");
    }
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

extern "C" fn process_stream_emit_stub(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    f64::from_bits(0x7FFC_0000_0000_0004) // true
}

extern "C" fn process_stream_on_once_stub(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

thread_local! {
    static STDIN_STREAM_SINGLETON: RefCell<usize> = const { RefCell::new(0) };
    static STDOUT_STREAM_SINGLETON: RefCell<usize> = const { RefCell::new(0) };
    static STDERR_STREAM_SINGLETON: RefCell<usize> = const { RefCell::new(0) };
}

pub fn scan_process_stream_singleton_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut visit_slot = |slot: &RefCell<usize>| {
        let mut value = slot.borrow_mut();
        if *value != 0 {
            let mut ptr = *value as *mut crate::object::ObjectHeader;
            if visitor.visit_raw_mut_ptr_slot(&mut ptr) {
                *value = ptr as usize;
            }
        }
    };
    STDIN_STREAM_SINGLETON.with(&mut visit_slot);
    STDOUT_STREAM_SINGLETON.with(&mut visit_slot);
    STDERR_STREAM_SINGLETON.with(&mut visit_slot);
}

/// Build a stream object with a `write` field bound to the given stub.
/// Each invocation of `process.stdout` / `process.stderr` returns a fresh
/// object whose `write` closure points at the matching fd.
fn build_stream_object_with_write(
    write_stub: extern "C" fn(*const crate::closure::ClosureHeader, f64) -> f64,
    fd: f64,
    writable: f64,
) -> *mut crate::object::ObjectHeader {
    use crate::closure::js_closure_alloc;
    use crate::object::{js_object_alloc_with_shape, js_object_set_field};
    use crate::value::JSValue;

    let packed = b"write\0fd\0emit\0on\0once\0writable\0";
    let obj = js_object_alloc_with_shape(0x7FFF_FF22, 6, packed.as_ptr(), packed.len() as u32);
    let closure = js_closure_alloc(write_stub as *const u8, 0);
    let cval = JSValue::pointer(closure as *const u8);
    js_object_set_field(obj, 0, cval);
    js_object_set_field(obj, 1, JSValue::number(fd));
    let emit = js_closure_alloc(process_stream_emit_stub as *const u8, 0);
    js_object_set_field(obj, 2, JSValue::pointer(emit as *const u8));
    let on = js_closure_alloc(process_stream_on_once_stub as *const u8, 0);
    js_object_set_field(obj, 3, JSValue::pointer(on as *const u8));
    let once = js_closure_alloc(process_stream_on_once_stub as *const u8, 0);
    js_object_set_field(obj, 4, JSValue::pointer(once as *const u8));
    js_object_set_field(obj, 5, JSValue::from_bits(writable.to_bits()));
    obj
}

/// process.stdin -> stream object whose `.write(...)` is a no-op (writing
/// to stdin from the program side has no useful semantics — kept as a
/// crash-free placeholder).
#[no_mangle]
pub extern "C" fn js_process_stdin() -> f64 {
    use crate::value::JSValue;
    let obj = STDIN_STREAM_SINGLETON.with(|slot| {
        let mut slot = slot.borrow_mut();
        if *slot == 0 {
            *slot = build_stream_object_with_write(
                process_stdin_write_noop_stub,
                0.0,
                f64::from_bits(crate::value::TAG_UNDEFINED),
            ) as usize;
        }
        *slot as *mut crate::object::ObjectHeader
    });
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// process.stdout -> stream object whose `.write(s)` writes `s` to fd 1
/// without appending a newline, matching Node.js semantics.
#[no_mangle]
pub extern "C" fn js_process_stdout() -> f64 {
    use crate::value::JSValue;
    let obj = STDOUT_STREAM_SINGLETON.with(|slot| {
        let mut slot = slot.borrow_mut();
        if *slot == 0 {
            *slot = build_stream_object_with_write(
                process_stdout_write_stub,
                1.0,
                f64::from_bits(crate::value::TAG_TRUE),
            ) as usize;
        }
        *slot as *mut crate::object::ObjectHeader
    });
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// process.stderr -> stream object whose `.write(s)` writes `s` to fd 2
/// without appending a newline, matching Node.js semantics.
#[no_mangle]
pub extern "C" fn js_process_stderr() -> f64 {
    use crate::value::JSValue;
    let obj = STDERR_STREAM_SINGLETON.with(|slot| {
        let mut slot = slot.borrow_mut();
        if *slot == 0 {
            *slot = build_stream_object_with_write(
                process_stderr_write_stub,
                2.0,
                f64::from_bits(crate::value::TAG_TRUE),
            ) as usize;
        }
        *slot as *mut crate::object::ObjectHeader
    });
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

    #[cfg(not(target_os = "linux"))]
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

const OS_NETWORK_IPV4_SHAPE_ID: u32 = 0x7FFF_FF27;
const OS_NETWORK_IPV6_SHAPE_ID: u32 = 0x7FFF_FF28;

struct OsNetworkAddress {
    address: String,
    netmask: String,
    family: &'static str,
    mac: String,
    internal: bool,
    cidr: String,
    scopeid: Option<u32>,
}

fn prefix_len_ipv4(netmask: Ipv4Addr) -> u32 {
    u32::from(netmask).count_ones()
}

fn prefix_len_ipv6(netmask: Ipv6Addr) -> u32 {
    netmask.octets().iter().map(|byte| byte.count_ones()).sum()
}

fn fallback_mac_address() -> String {
    "00:00:00:00:00:00".to_string()
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn interface_mac_address(name: &str) -> String {
    let path = format!("/sys/class/net/{name}/address");
    std::fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(fallback_mac_address)
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn interface_mac_address(_name: &str) -> String {
    fallback_mac_address()
}

#[cfg(unix)]
unsafe fn ipv4_from_sockaddr(addr: *const libc::sockaddr) -> Ipv4Addr {
    let sin = &*(addr as *const libc::sockaddr_in);
    Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr))
}

#[cfg(unix)]
unsafe fn ipv6_from_sockaddr(addr: *const libc::sockaddr) -> Ipv6Addr {
    let sin6 = &*(addr as *const libc::sockaddr_in6);
    Ipv6Addr::from(sin6.sin6_addr.s6_addr)
}

#[cfg(unix)]
fn collect_network_interfaces() -> HashMap<String, Vec<OsNetworkAddress>> {
    let mut first: *mut libc::ifaddrs = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut first) } != 0 {
        return HashMap::new();
    }

    let mut interfaces: HashMap<String, Vec<OsNetworkAddress>> = HashMap::new();
    let mut macs: HashMap<String, String> = HashMap::new();
    let mut current = first;
    while !current.is_null() {
        let ifa = unsafe { &*current };
        if !ifa.ifa_addr.is_null() && !ifa.ifa_name.is_null() {
            let family = unsafe { (*ifa.ifa_addr).sa_family as i32 };
            let internal = (ifa.ifa_flags & (libc::IFF_LOOPBACK as libc::c_uint)) != 0;
            let name = unsafe { std::ffi::CStr::from_ptr(ifa.ifa_name) }
                .to_string_lossy()
                .into_owned();
            let mac = macs
                .entry(name.clone())
                .or_insert_with(|| interface_mac_address(&name))
                .clone();

            match family {
                libc::AF_INET => {
                    let address = unsafe { ipv4_from_sockaddr(ifa.ifa_addr) };
                    let netmask = if ifa.ifa_netmask.is_null() {
                        Ipv4Addr::UNSPECIFIED
                    } else {
                        unsafe { ipv4_from_sockaddr(ifa.ifa_netmask) }
                    };
                    let prefix = prefix_len_ipv4(netmask);
                    interfaces.entry(name).or_default().push(OsNetworkAddress {
                        address: address.to_string(),
                        netmask: netmask.to_string(),
                        family: "IPv4",
                        mac,
                        internal,
                        cidr: format!("{address}/{prefix}"),
                        scopeid: None,
                    });
                }
                libc::AF_INET6 => {
                    let sin6 = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in6) };
                    let address = unsafe { ipv6_from_sockaddr(ifa.ifa_addr) };
                    let netmask = if ifa.ifa_netmask.is_null() {
                        Ipv6Addr::UNSPECIFIED
                    } else {
                        unsafe { ipv6_from_sockaddr(ifa.ifa_netmask) }
                    };
                    let prefix = prefix_len_ipv6(netmask);
                    interfaces.entry(name).or_default().push(OsNetworkAddress {
                        address: address.to_string(),
                        netmask: netmask.to_string(),
                        family: "IPv6",
                        mac,
                        internal,
                        cidr: format!("{address}/{prefix}"),
                        scopeid: Some(sin6.sin6_scope_id),
                    });
                }
                _ => {}
            }
        }
        current = ifa.ifa_next;
    }

    unsafe { libc::freeifaddrs(first) };
    interfaces
}

#[cfg(not(unix))]
fn collect_network_interfaces() -> HashMap<String, Vec<OsNetworkAddress>> {
    HashMap::new()
}

fn js_string_value(value: &str) -> crate::value::JSValue {
    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    crate::value::JSValue::string_ptr(ptr)
}

fn build_network_address_object(address: &OsNetworkAddress) -> *mut ObjectHeader {
    use crate::object::{js_object_alloc_with_shape, js_object_set_field};
    use crate::value::JSValue;

    let (shape_id, packed, field_count) = if address.family == "IPv6" {
        (
            OS_NETWORK_IPV6_SHAPE_ID,
            b"address\0netmask\0family\0mac\0internal\0cidr\0scopeid\0".as_slice(),
            7,
        )
    } else {
        (
            OS_NETWORK_IPV4_SHAPE_ID,
            b"address\0netmask\0family\0mac\0internal\0cidr\0".as_slice(),
            6,
        )
    };
    let obj =
        js_object_alloc_with_shape(shape_id, field_count, packed.as_ptr(), packed.len() as u32);
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);

    js_object_set_field(
        obj_handle.get_raw_mut_ptr(),
        0,
        js_string_value(&address.address),
    );
    js_object_set_field(
        obj_handle.get_raw_mut_ptr(),
        1,
        js_string_value(&address.netmask),
    );
    js_object_set_field(
        obj_handle.get_raw_mut_ptr(),
        2,
        js_string_value(address.family),
    );
    js_object_set_field(
        obj_handle.get_raw_mut_ptr(),
        3,
        js_string_value(&address.mac),
    );
    js_object_set_field(
        obj_handle.get_raw_mut_ptr(),
        4,
        JSValue::bool(address.internal),
    );
    js_object_set_field(
        obj_handle.get_raw_mut_ptr(),
        5,
        js_string_value(&address.cidr),
    );
    if let Some(scopeid) = address.scopeid {
        js_object_set_field(
            obj_handle.get_raw_mut_ptr(),
            6,
            JSValue::number(scopeid as f64),
        );
    }

    obj_handle.get_raw_mut_ptr()
}

/// Get network interfaces information
/// Returns an object with interface names as keys
#[no_mangle]
pub extern "C" fn js_os_network_interfaces() -> *mut ObjectHeader {
    use crate::object::{js_object_alloc, js_object_set_field_by_name};
    use crate::value::JSValue;

    let interfaces = collect_network_interfaces();
    let scope = crate::gc::RuntimeHandleScope::new();
    let result = js_object_alloc(0, 0);
    let result_handle = scope.root_raw_mut_ptr(result);

    for (name, addresses) in interfaces {
        let arr = crate::array::js_array_alloc(addresses.len() as u32);
        let arr_handle = scope.root_raw_mut_ptr(arr);

        for address in addresses {
            let entry = build_network_address_object(&address);
            let entry_handle = scope.root_raw_mut_ptr(entry);
            let pushed = crate::array::js_array_push(
                arr_handle.get_raw_mut_ptr(),
                JSValue::pointer(entry_handle.get_raw_mut_ptr() as *const u8),
            );
            arr_handle.set_raw_mut_ptr(pushed);
        }

        let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let key_handle = scope.root_string_ptr(key);
        js_object_set_field_by_name(
            result_handle.get_raw_mut_ptr(),
            key_handle.get_raw_const_ptr(),
            f64::from_bits(JSValue::pointer(arr_handle.get_raw_mut_ptr() as *const u8).bits()),
        );
    }

    result_handle.get_raw_mut_ptr()
}

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
