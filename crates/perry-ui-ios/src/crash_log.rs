//! Crash log persistence — writes panic/signal info to ~/.hone/crash.log
//! so the TypeScript telemetry layer can report it via Chirp on next launch.
//!
//! iOS variant: $HOME points to the app sandbox container, so
//! ~/.hone/crash.log lands inside the sandbox (no permissions issues).

use std::io::Write;

/// Pre-computed crash log path as a null-terminated C string for signal-handler safety.
static mut CRASH_LOG_PATH_BUF: [u8; 512] = [0u8; 512];
static mut CRASH_LOG_PATH_LEN: usize = 0;

/// Get the crash log path (~/.hone/crash.log), ensuring the directory exists.
fn crash_log_path() -> Option<String> {
    if let Ok(home) = std::env::var("HOME") {
        let dir = format!("{}/.hone", home);
        let _ = std::fs::create_dir_all(&dir);
        Some(format!("{}/crash.log", dir))
    } else {
        None
    }
}

/// Write a crash entry to the crash log file (overwrites any previous entry).
pub fn write_crash(crash_type: &str, message: &str) {
    if let Some(path) = crash_log_path() {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
        {
            let _ = writeln!(f, "{}:{}", crash_type, message);
        }
    }
}

/// Clear the crash log (called after a panic is caught non-fatally).
pub fn clear_crash_log() {
    if let Some(path) = crash_log_path() {
        let _ = std::fs::remove_file(&path);
    }
}

/// Install global panic hook and signal handlers.
pub fn install_crash_hooks() {
    // Pre-compute the path as a C string for signal handler use
    if let Some(path) = crash_log_path() {
        let bytes = path.as_bytes();
        let len = bytes.len().min(511);
        unsafe {
            CRASH_LOG_PATH_BUF[..len].copy_from_slice(&bytes[..len]);
            CRASH_LOG_PATH_BUF[len] = 0;
            CRASH_LOG_PATH_LEN = len;
        }
    }

    std::panic::set_hook(Box::new(|info| {
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown".to_string()
        };

        let location = if let Some(loc) = info.location() {
            format!(" at {}:{}", loc.file(), loc.line())
        } else {
            String::new()
        };

        write_crash("panic", &format!("{}{}", msg, location));
        // Also log to file since iOS eprintln is invisible
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/hone-crash.log")
        {
            let _ = writeln!(f, "[hone] FATAL: {}{}", msg, location);
        }
    }));

    unsafe {
        libc::signal(libc::SIGSEGV, signal_handler as libc::sighandler_t);
        libc::signal(libc::SIGBUS, signal_handler as libc::sighandler_t);
        libc::signal(libc::SIGABRT, signal_handler as libc::sighandler_t);
        NSSetUncaughtExceptionHandler(Some(uncaught_objc_exception));
    }
}

type UncaughtExceptionHandler = extern "C" fn(*mut objc2::runtime::AnyObject);

extern "C" {
    fn NSSetUncaughtExceptionHandler(handler: Option<UncaughtExceptionHandler>);
}

/// Report an uncaught Objective-C exception with its **throw-time** backtrace.
///
/// Without this the process aborts through `SIGABRT` and all that survives is
/// `libc++abi: terminating due to uncaught exception`, plus a list of raw
/// return addresses with no load address to symbolicate against. `NSException`
/// captures `callStackSymbols` at the moment it is *raised*, so reading it here
/// recovers the frames that actually threw — already symbolicated by the
/// Objective-C runtime, with no need for a matching dSYM or an offline pass
/// over an `.ips`.
///
/// Written to stderr because `perry run` streams the device's stderr back over
/// `devicectl --console`, which is the only channel available during an
/// interactive device session.
extern "C" fn uncaught_objc_exception(exc: *mut objc2::runtime::AnyObject) {
    use objc2::runtime::AnyObject;
    if exc.is_null() {
        eprintln!("[perry-crash] uncaught Objective-C exception (null)");
        return;
    }

    // Everything below is best-effort: this runs while the process is already
    // terminating, so a failure to read one field must not prevent the rest
    // from being reported.
    unsafe {
        let ns_string_utf8 = |s: *mut AnyObject| -> String {
            if s.is_null() {
                return String::from("<null>");
            }
            let c: *const std::ffi::c_char = objc2::msg_send![s, UTF8String];
            if c.is_null() {
                return String::from("<null>");
            }
            std::ffi::CStr::from_ptr(c).to_string_lossy().into_owned()
        };

        let name: *mut AnyObject = objc2::msg_send![exc, name];
        let reason: *mut AnyObject = objc2::msg_send![exc, reason];
        eprintln!(
            "[perry-crash] uncaught {}: {}",
            ns_string_utf8(name),
            ns_string_utf8(reason)
        );

        let symbols: *mut AnyObject = objc2::msg_send![exc, callStackSymbols];
        if symbols.is_null() {
            eprintln!("[perry-crash] no callStackSymbols available");
            return;
        }
        let count: usize = objc2::msg_send![symbols, count];
        eprintln!("[perry-crash] throw-time backtrace ({count} frames):");
        for i in 0..count {
            let frame: *mut AnyObject = objc2::msg_send![symbols, objectAtIndex: i];
            eprintln!("[perry-crash]   {}", ns_string_utf8(frame));
        }
    }
}

extern "C" fn signal_handler(sig: libc::c_int) {
    let msg: &[u8] = match sig {
        libc::SIGSEGV => b"signal:SIGSEGV\n",
        libc::SIGBUS => b"signal:SIGBUS\n",
        libc::SIGABRT => b"signal:SIGABRT\n",
        _ => b"signal:unknown\n",
    };

    unsafe {
        if CRASH_LOG_PATH_LEN > 0 {
            let fd = libc::open(
                CRASH_LOG_PATH_BUF.as_ptr() as *const libc::c_char,
                libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC,
                0o644,
            );
            if fd >= 0 {
                libc::write(fd, msg.as_ptr() as *const libc::c_void, msg.len());
                libc::close(fd);
            }
        }

        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
}
