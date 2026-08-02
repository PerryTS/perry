//! Mutable/queryable process attributes: `process.resourceUsage()`,
//! `process.title` (get/set) and `process.umask()` (get/set).
//!
//! Split verbatim out of the sibling [`super::env_misc`] module to keep that
//! file under the 2000-line gate. Pure code move — no behavior change.

use super::env_misc::read_js_string_lossy;
use super::*;
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;

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
