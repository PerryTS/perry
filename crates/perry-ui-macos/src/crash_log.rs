//! Crash log persistence — writes panic/signal info to ~/.hone/crash.log
//! so the TypeScript telemetry layer can report it via Chirp on next launch.
//!
//! Flow:
//! 1. `install_crash_hooks()` is called early in `app_run()`.
//! 2. A global panic hook writes crash details to `~/.hone/crash.log`.
//! 3. If `catch_callback_panic()` catches the panic (non-fatal), it clears the log.
//! 4. Signal handlers (SIGSEGV/SIGBUS/SIGABRT) write a minimal marker using only
//!    async-signal-safe syscalls.
//! 5. On next launch, the TypeScript side reads the file, sends it to Chirp, deletes it.

use std::io::Write;

/// Pre-computed crash log path as a null-terminated C string for signal-handler safety.
/// Written once at startup, read-only from signal handlers.
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
            // Format: type:message (single line, no PII)
            let _ = writeln!(f, "{}:{}", crash_type, message);
        }
    }
}

/// Clear the crash log (called after catch_callback_panic catches a non-fatal panic).
pub fn clear_crash_log() {
    if let Some(path) = crash_log_path() {
        let _ = std::fs::remove_file(&path);
    }
}

/// Install global panic hook and signal handlers.
/// Must be called once, early in app startup (before `app.run()`).
pub fn install_crash_hooks() {
    // Pre-compute the path as a C string for signal handler use
    if let Some(path) = crash_log_path() {
        let bytes = path.as_bytes();
        let len = bytes.len().min(511);
        unsafe {
            CRASH_LOG_PATH_BUF[..len].copy_from_slice(&bytes[..len]);
            CRASH_LOG_PATH_BUF[len] = 0; // null terminate
            CRASH_LOG_PATH_LEN = len;
        }
    }

    // Global panic hook — fires for ALL panics (caught and uncaught).
    // If catch_callback_panic subsequently catches it, it clears the log.
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
        eprintln!("[hone] FATAL: {}{}", msg, location);
    }));

    // Signal handlers for catastrophic failures (segfault, bus error, abort).
    // These bypass Rust's panic machinery entirely.
    unsafe {
        libc::signal(libc::SIGSEGV, signal_handler as libc::sighandler_t);
        libc::signal(libc::SIGBUS, signal_handler as libc::sighandler_t);
        libc::signal(libc::SIGABRT, signal_handler as libc::sighandler_t);
    }
}

/// Async-signal-safe crash handler. Uses only raw syscalls — no allocations,
/// no locks, no Rust formatting.
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

        // Re-raise with default handler so the OS generates the expected exit status
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
}
