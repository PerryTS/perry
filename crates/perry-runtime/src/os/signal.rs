use crate::fs::validate::{describe_received, is_numeric, throw_type_error_with_code};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::{JSValue, TAG_TRUE};
#[cfg(unix)]
use std::sync::atomic::AtomicI32;
#[cfg(any(unix, windows))]
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

fn signal_number_by_name(name: &str) -> Option<i32> {
    #[cfg(unix)]
    {
        match name {
            "SIGHUP" => Some(libc::SIGHUP),
            "SIGINT" => Some(libc::SIGINT),
            "SIGQUIT" => Some(libc::SIGQUIT),
            "SIGILL" => Some(libc::SIGILL),
            "SIGTRAP" => Some(libc::SIGTRAP),
            "SIGABRT" => Some(libc::SIGABRT),
            "SIGIOT" => Some(libc::SIGABRT),
            "SIGBUS" => Some(libc::SIGBUS),
            "SIGFPE" => Some(libc::SIGFPE),
            "SIGKILL" => Some(libc::SIGKILL),
            "SIGUSR1" => Some(libc::SIGUSR1),
            "SIGSEGV" => Some(libc::SIGSEGV),
            "SIGUSR2" => Some(libc::SIGUSR2),
            "SIGPIPE" => Some(libc::SIGPIPE),
            "SIGALRM" => Some(libc::SIGALRM),
            "SIGTERM" => Some(libc::SIGTERM),
            "SIGCHLD" => Some(libc::SIGCHLD),
            #[cfg(target_os = "linux")]
            "SIGSTKFLT" => Some(libc::SIGSTKFLT),
            "SIGCONT" => Some(libc::SIGCONT),
            "SIGSTOP" => Some(libc::SIGSTOP),
            "SIGTSTP" => Some(libc::SIGTSTP),
            "SIGTTIN" => Some(libc::SIGTTIN),
            "SIGTTOU" => Some(libc::SIGTTOU),
            "SIGURG" => Some(libc::SIGURG),
            "SIGXCPU" => Some(libc::SIGXCPU),
            "SIGXFSZ" => Some(libc::SIGXFSZ),
            "SIGVTALRM" => Some(libc::SIGVTALRM),
            "SIGPROF" => Some(libc::SIGPROF),
            "SIGWINCH" => Some(libc::SIGWINCH),
            "SIGIO" => Some(libc::SIGIO),
            #[cfg(any(target_os = "linux", target_os = "android"))]
            "SIGPOLL" => Some(libc::SIGPOLL),
            #[cfg(target_os = "linux")]
            "SIGPWR" => Some(libc::SIGPWR),
            "SIGSYS" => Some(libc::SIGSYS),
            #[cfg(target_os = "macos")]
            "SIGINFO" => Some(29),
            _ => None,
        }
    }
    #[cfg(not(unix))]
    {
        match name {
            "SIGHUP" => Some(1),
            "SIGINT" => Some(2),
            "SIGILL" => Some(4),
            "SIGABRT" => Some(22),
            "SIGFPE" => Some(8),
            "SIGKILL" => Some(9),
            "SIGSEGV" => Some(11),
            "SIGTERM" => Some(15),
            "SIGBREAK" => Some(21),
            _ => None,
        }
    }
}

#[cfg(unix)]
static SIGNAL_WAKE_WRITE_FD: AtomicI32 = AtomicI32::new(-1);
#[cfg(unix)]
static SIGNAL_WAKE_THREAD_STARTED: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
static SIGHUP_PENDING: AtomicUsize = AtomicUsize::new(0);
#[cfg(unix)]
static SIGHUP_LISTENERS: AtomicUsize = AtomicUsize::new(0);
#[cfg(unix)]
static SIGHUP_INSTALLED: AtomicBool = AtomicBool::new(false);
#[cfg(unix)]
static SIGINT_PENDING: AtomicUsize = AtomicUsize::new(0);
#[cfg(unix)]
static SIGINT_LISTENERS: AtomicUsize = AtomicUsize::new(0);
#[cfg(unix)]
static SIGINT_INSTALLED: AtomicBool = AtomicBool::new(false);
#[cfg(unix)]
static SIGQUIT_PENDING: AtomicUsize = AtomicUsize::new(0);
#[cfg(unix)]
static SIGQUIT_LISTENERS: AtomicUsize = AtomicUsize::new(0);
#[cfg(unix)]
static SIGQUIT_INSTALLED: AtomicBool = AtomicBool::new(false);
#[cfg(unix)]
static SIGABRT_PENDING: AtomicUsize = AtomicUsize::new(0);
#[cfg(unix)]
static SIGABRT_LISTENERS: AtomicUsize = AtomicUsize::new(0);
#[cfg(unix)]
static SIGABRT_INSTALLED: AtomicBool = AtomicBool::new(false);
#[cfg(unix)]
static SIGBUS_PENDING: AtomicUsize = AtomicUsize::new(0);
#[cfg(unix)]
static SIGBUS_LISTENERS: AtomicUsize = AtomicUsize::new(0);
#[cfg(unix)]
static SIGBUS_INSTALLED: AtomicBool = AtomicBool::new(false);
#[cfg(unix)]
static SIGPIPE_PENDING: AtomicUsize = AtomicUsize::new(0);
#[cfg(unix)]
static SIGPIPE_LISTENERS: AtomicUsize = AtomicUsize::new(0);
#[cfg(unix)]
static SIGPIPE_INSTALLED: AtomicBool = AtomicBool::new(false);
#[cfg(unix)]
static SIGTERM_PENDING: AtomicUsize = AtomicUsize::new(0);
#[cfg(unix)]
static SIGTERM_LISTENERS: AtomicUsize = AtomicUsize::new(0);
#[cfg(unix)]
static SIGTERM_INSTALLED: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
struct ProcessSignalSlot {
    name: &'static str,
    number: libc::c_int,
    pending: &'static AtomicUsize,
    listeners: &'static AtomicUsize,
    installed: &'static AtomicBool,
}

#[cfg(unix)]
static PROCESS_SIGNAL_SLOTS: &[ProcessSignalSlot] = &[
    ProcessSignalSlot {
        name: "SIGHUP",
        number: libc::SIGHUP,
        pending: &SIGHUP_PENDING,
        listeners: &SIGHUP_LISTENERS,
        installed: &SIGHUP_INSTALLED,
    },
    ProcessSignalSlot {
        name: "SIGINT",
        number: libc::SIGINT,
        pending: &SIGINT_PENDING,
        listeners: &SIGINT_LISTENERS,
        installed: &SIGINT_INSTALLED,
    },
    ProcessSignalSlot {
        name: "SIGQUIT",
        number: libc::SIGQUIT,
        pending: &SIGQUIT_PENDING,
        listeners: &SIGQUIT_LISTENERS,
        installed: &SIGQUIT_INSTALLED,
    },
    ProcessSignalSlot {
        name: "SIGABRT",
        number: libc::SIGABRT,
        pending: &SIGABRT_PENDING,
        listeners: &SIGABRT_LISTENERS,
        installed: &SIGABRT_INSTALLED,
    },
    ProcessSignalSlot {
        name: "SIGBUS",
        number: libc::SIGBUS,
        pending: &SIGBUS_PENDING,
        listeners: &SIGBUS_LISTENERS,
        installed: &SIGBUS_INSTALLED,
    },
    ProcessSignalSlot {
        name: "SIGPIPE",
        number: libc::SIGPIPE,
        pending: &SIGPIPE_PENDING,
        listeners: &SIGPIPE_LISTENERS,
        installed: &SIGPIPE_INSTALLED,
    },
    ProcessSignalSlot {
        name: "SIGTERM",
        number: libc::SIGTERM,
        pending: &SIGTERM_PENDING,
        listeners: &SIGTERM_LISTENERS,
        installed: &SIGTERM_INSTALLED,
    },
];

#[cfg(unix)]
fn slot_by_name(name: &str) -> Option<&'static ProcessSignalSlot> {
    PROCESS_SIGNAL_SLOTS.iter().find(|slot| slot.name == name)
}

#[cfg(unix)]
fn slot_by_number(number: libc::c_int) -> Option<&'static ProcessSignalSlot> {
    PROCESS_SIGNAL_SLOTS
        .iter()
        .find(|slot| slot.number == number)
}

#[cfg(unix)]
extern "C" fn process_signal_handler(sig: libc::c_int) {
    if let Some(slot) = slot_by_number(sig) {
        slot.pending.fetch_add(1, Ordering::Release);
        let fd = SIGNAL_WAKE_WRITE_FD.load(Ordering::Relaxed);
        if fd >= 0 {
            let byte = [sig as u8];
            unsafe {
                let _ = libc::write(fd, byte.as_ptr() as *const _, 1);
            }
        }
    }
}

#[cfg(unix)]
fn set_fd_cloexec(fd: libc::c_int) {
    unsafe {
        let current = libc::fcntl(fd, libc::F_GETFD);
        if current >= 0 {
            let _ = libc::fcntl(fd, libc::F_SETFD, current | libc::FD_CLOEXEC);
        }
    }
}

#[cfg(unix)]
fn set_fd_nonblocking(fd: libc::c_int) {
    unsafe {
        let current = libc::fcntl(fd, libc::F_GETFL);
        if current >= 0 {
            let _ = libc::fcntl(fd, libc::F_SETFL, current | libc::O_NONBLOCK);
        }
    }
}

#[cfg(unix)]
fn ensure_signal_wake_thread() {
    if SIGNAL_WAKE_THREAD_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    unsafe {
        let mut fds = [0; 2];
        if libc::pipe(fds.as_mut_ptr()) != 0 {
            SIGNAL_WAKE_THREAD_STARTED.store(false, Ordering::Release);
            return;
        }
        set_fd_cloexec(fds[0]);
        set_fd_cloexec(fds[1]);
        set_fd_nonblocking(fds[1]);
        SIGNAL_WAKE_WRITE_FD.store(fds[1], Ordering::Release);
        let read_fd = fds[0];
        let _ = std::thread::Builder::new()
            .name("perry-signal-wake".to_string())
            .spawn(move || {
                let mut buf = [0u8; 64];
                loop {
                    let n = libc::read(read_fd, buf.as_mut_ptr() as *mut _, buf.len());
                    if n > 0 {
                        crate::event_pump::js_notify_main_thread();
                    } else if n == 0 {
                        break;
                    } else {
                        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
                        if errno != libc::EINTR {
                            std::thread::sleep(std::time::Duration::from_millis(10));
                        }
                    }
                }
            });
    }
}

#[cfg(unix)]
fn install_process_signal_handler(slot: &'static ProcessSignalSlot) {
    if slot
        .installed
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    ensure_signal_wake_thread();
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = process_signal_handler as *const () as usize;
        sa.sa_flags = libc::SA_RESTART;
        libc::sigemptyset(&mut sa.sa_mask);
        if libc::sigaction(slot.number, &sa, std::ptr::null_mut()) != 0 {
            slot.installed.store(false, Ordering::Release);
        }
    }
}

#[cfg(unix)]
fn uninstall_process_signal_handler(slot: &'static ProcessSignalSlot) {
    slot.pending.store(0, Ordering::Release);
    if slot
        .installed
        .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = libc::SIG_DFL;
        libc::sigemptyset(&mut sa.sa_mask);
        let _ = libc::sigaction(slot.number, &sa, std::ptr::null_mut());
    }
}

// ============================================================================
// Windows: console control events → process signal events (#6609 audit).
//
// Mirrors the Unix structure above, with `SetConsoleCtrlHandler` standing in
// for `sigaction` and the mapping Node/libuv use on Windows:
//
//   CTRL_C_EVENT     → 'SIGINT'
//   CTRL_BREAK_EVENT → 'SIGBREAK'
//   CTRL_CLOSE_EVENT → 'SIGHUP'   (console closed; ~5s grace before force-kill)
//   'SIGTERM'        → registerable but source-less (no console event raises
//                      it — Node accepts the listener and it simply never
//                      fires from the console).
//
// The control handler runs on a Windows-spawned thread, never the JS main
// thread, so it must not touch the JS heap: it only bumps the slot's
// `pending` atomic and calls `js_notify_main_thread()` (atomics + condvar,
// documented safe from any thread). The main thread drains via the same
// platform-neutral path Unix uses: `take_pending_process_signals()` from
// `js_process_signal_drain` on the next event-loop tick.
#[cfg(windows)]
static WIN_SIGINT_PENDING: AtomicUsize = AtomicUsize::new(0);
#[cfg(windows)]
static WIN_SIGINT_LISTENERS: AtomicUsize = AtomicUsize::new(0);
#[cfg(windows)]
static WIN_SIGBREAK_PENDING: AtomicUsize = AtomicUsize::new(0);
#[cfg(windows)]
static WIN_SIGBREAK_LISTENERS: AtomicUsize = AtomicUsize::new(0);
#[cfg(windows)]
static WIN_SIGHUP_PENDING: AtomicUsize = AtomicUsize::new(0);
#[cfg(windows)]
static WIN_SIGHUP_LISTENERS: AtomicUsize = AtomicUsize::new(0);
#[cfg(windows)]
static WIN_SIGTERM_PENDING: AtomicUsize = AtomicUsize::new(0);
#[cfg(windows)]
static WIN_SIGTERM_LISTENERS: AtomicUsize = AtomicUsize::new(0);
#[cfg(windows)]
static WIN_CTRL_HANDLER_INSTALLED: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
struct WinSignalSlot {
    name: &'static str,
    /// Console control event that maps to this signal, or `None` when the
    /// signal has no console source on Windows (SIGTERM).
    ctrl_event: Option<u32>,
    pending: &'static AtomicUsize,
    listeners: &'static AtomicUsize,
}

#[cfg(windows)]
static WIN_SIGNAL_SLOTS: &[WinSignalSlot] = &[
    WinSignalSlot {
        name: "SIGINT",
        ctrl_event: Some(windows_sys::Win32::System::Console::CTRL_C_EVENT),
        pending: &WIN_SIGINT_PENDING,
        listeners: &WIN_SIGINT_LISTENERS,
    },
    WinSignalSlot {
        name: "SIGBREAK",
        ctrl_event: Some(windows_sys::Win32::System::Console::CTRL_BREAK_EVENT),
        pending: &WIN_SIGBREAK_PENDING,
        listeners: &WIN_SIGBREAK_LISTENERS,
    },
    WinSignalSlot {
        name: "SIGHUP",
        ctrl_event: Some(windows_sys::Win32::System::Console::CTRL_CLOSE_EVENT),
        pending: &WIN_SIGHUP_PENDING,
        listeners: &WIN_SIGHUP_LISTENERS,
    },
    WinSignalSlot {
        name: "SIGTERM",
        ctrl_event: None,
        pending: &WIN_SIGTERM_PENDING,
        listeners: &WIN_SIGTERM_LISTENERS,
    },
];

#[cfg(windows)]
fn win_slot_by_name(name: &str) -> Option<&'static WinSignalSlot> {
    WIN_SIGNAL_SLOTS.iter().find(|slot| slot.name == name)
}

/// Console control handler — runs on a thread Windows injects, NOT the JS
/// main thread. Async-context rules mirror the Unix `process_signal_handler`:
/// only atomics + the any-thread-safe event-pump notify; no JS heap access,
/// no JS calls.
///
/// Returns TRUE (1, "handled") iff at least one JS listener is registered
/// for the mapped signal at this moment; otherwise FALSE (0) so the default
/// behavior is preserved — plain Ctrl+C on a program with no `SIGINT`
/// listener still terminates it, and removing the last listener restores
/// default behavior without needing to unregister the handler.
#[cfg(windows)]
unsafe extern "system" fn win_console_ctrl_handler(ctrl_type: u32) -> windows_sys::core::BOOL {
    let Some(slot) = WIN_SIGNAL_SLOTS
        .iter()
        .find(|slot| slot.ctrl_event == Some(ctrl_type))
    else {
        // Not a signal we map (logoff/shutdown/…): defer to the default.
        return 0;
    };
    if slot.listeners.load(Ordering::Acquire) == 0 {
        return 0;
    }
    slot.pending.fetch_add(1, Ordering::Release);
    crate::event_pump::js_notify_main_thread();
    if ctrl_type == windows_sys::Win32::System::Console::CTRL_CLOSE_EVENT {
        // Mirror libuv: for CTRL_CLOSE_EVENT Windows terminates the process
        // as soon as this handler *returns*, but grants ~5s while it runs.
        // Park this handler thread so the main thread gets the full grace
        // window to run the JS 'SIGHUP' listeners; the OS force-kills the
        // process afterwards (or earlier, when the main thread exits).
        loop {
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
    }
    1
}

/// Install the console control handler once, on first listener registration.
/// It stays installed for the process lifetime; the per-slot listener-count
/// check inside the handler restores default behavior when the last listener
/// is removed (no unregister needed, so there is no install/remove race).
#[cfg(windows)]
fn ensure_win_console_ctrl_handler() {
    if WIN_CTRL_HANDLER_INSTALLED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let ok = unsafe {
        windows_sys::Win32::System::Console::SetConsoleCtrlHandler(
            Some(win_console_ctrl_handler),
            1,
        )
    };
    if ok == 0 {
        WIN_CTRL_HANDLER_INSTALLED.store(false, Ordering::Release);
    }
}

pub(crate) fn is_process_signal_name(name: &str) -> bool {
    #[cfg(unix)]
    {
        slot_by_name(name).is_some()
    }
    #[cfg(windows)]
    {
        win_slot_by_name(name).is_some()
    }
    #[cfg(not(any(unix, windows)))]
    {
        matches!(name, "SIGINT" | "SIGTERM")
    }
}

pub(crate) fn set_process_signal_listener_count(name: &str, count: usize) {
    #[cfg(unix)]
    {
        let Some(slot) = slot_by_name(name) else {
            return;
        };
        slot.listeners.store(count, Ordering::Release);
        if count > 0 {
            install_process_signal_handler(slot);
        } else {
            uninstall_process_signal_handler(slot);
        }
    }
    #[cfg(windows)]
    {
        let Some(slot) = win_slot_by_name(name) else {
            return;
        };
        slot.listeners.store(count, Ordering::Release);
        if count == 0 {
            // Mirror the Unix uninstall path: drop undelivered signals so a
            // stale pending count can't keep the event loop alive or fire
            // into a re-registered listener later.
            slot.pending.store(0, Ordering::Release);
        } else if slot.ctrl_event.is_some() {
            ensure_win_console_ctrl_handler();
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (name, count);
    }
}

/// Whether a *pending, undelivered* signal is waiting to be drained.
///
/// Node semantics: a registered `process.on('SIGINT', …)` listener is
/// ref-NEUTRAL — it does NOT keep the event loop alive on its own (the
/// docs' own example calls `process.stdin.resume()` precisely because a
/// signal listener alone won't keep the process running). The loop must
/// only be held open when a signal has actually arrived and its
/// listener callbacks still need to fire on the main thread.
///
/// Pre-fix this returned `listeners > 0`, so any CLI that installs a
/// SIGINT/SIGTERM/SIGHUP handler at startup (the common graceful-shutdown
/// pattern) pinned the event loop forever: once its real work drained,
/// the loop had no microtasks/timers/async ops left, yet
/// `js_stdlib_has_active_handles` kept returning 1 from this check and
/// the program hung at idle instead of exiting. Now we gate on a pending
/// signal so the listener registration alone no longer keeps the loop
/// alive; a delivered signal still wakes the loop via the self-pipe
/// notify and is drained by `js_process_signal_drain` on the next tick.
pub(crate) fn has_active_process_signal_listeners() -> bool {
    #[cfg(unix)]
    {
        PROCESS_SIGNAL_SLOTS.iter().any(|slot| {
            slot.pending.load(Ordering::Acquire) > 0 && slot.listeners.load(Ordering::Acquire) > 0
        })
    }
    #[cfg(windows)]
    {
        WIN_SIGNAL_SLOTS.iter().any(|slot| {
            slot.pending.load(Ordering::Acquire) > 0 && slot.listeners.load(Ordering::Acquire) > 0
        })
    }
    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

pub(crate) fn take_pending_process_signals() -> Vec<&'static str> {
    #[cfg(unix)]
    {
        let mut signals = Vec::new();
        for slot in PROCESS_SIGNAL_SLOTS {
            let count = slot.pending.swap(0, Ordering::AcqRel);
            if count == 0 || slot.listeners.load(Ordering::Acquire) == 0 {
                continue;
            }
            signals.extend(std::iter::repeat_n(slot.name, count));
        }
        signals
    }
    #[cfg(windows)]
    {
        let mut signals = Vec::new();
        for slot in WIN_SIGNAL_SLOTS {
            let count = slot.pending.swap(0, Ordering::AcqRel);
            if count == 0 || slot.listeners.load(Ordering::Acquire) == 0 {
                continue;
            }
            signals.extend(std::iter::repeat_n(slot.name, count));
        }
        signals
    }
    #[cfg(not(any(unix, windows)))]
    {
        Vec::new()
    }
}

fn signal_names() -> Vec<&'static str> {
    let mut names = vec![
        "SIGHUP", "SIGINT", "SIGQUIT", "SIGILL", "SIGTRAP", "SIGABRT", "SIGIOT", "SIGBUS",
        "SIGFPE", "SIGKILL", "SIGUSR1", "SIGSEGV", "SIGUSR2", "SIGPIPE", "SIGALRM", "SIGTERM",
        "SIGCHLD",
    ];
    #[cfg(target_os = "linux")]
    names.push("SIGSTKFLT");
    names.extend([
        "SIGCONT",
        "SIGSTOP",
        "SIGTSTP",
        "SIGTTIN",
        "SIGTTOU",
        "SIGURG",
        "SIGXCPU",
        "SIGXFSZ",
        "SIGVTALRM",
        "SIGPROF",
        "SIGWINCH",
        "SIGIO",
    ]);
    #[cfg(any(target_os = "linux", target_os = "android"))]
    names.push("SIGPOLL");
    #[cfg(target_os = "linux")]
    names.push("SIGPWR");
    names.push("SIGSYS");
    #[cfg(target_os = "macos")]
    names.push("SIGINFO");
    names
}

fn read_js_string(value: f64) -> Option<String> {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_any_string() {
        return None;
    }
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    if ptr.is_null() {
        return None;
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        Some(String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned())
    }
}

fn numeric_value(jv: JSValue) -> Option<f64> {
    if jv.is_int32() {
        Some(jv.as_int32() as f64)
    } else if jv.is_number() {
        Some(jv.as_number())
    } else {
        None
    }
}

fn is_array_value(jv: JSValue) -> bool {
    if !jv.is_pointer() {
        return false;
    }
    let ptr = jv.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return false;
    }
    let header = unsafe { &*(ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader) };
    header.obj_type == crate::gc::GC_TYPE_ARRAY
}

fn display_value(value: f64) -> String {
    if let Some(s) = read_js_string(value) {
        return s;
    }
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return "undefined".to_string();
    }
    if jv.is_null() {
        return "null".to_string();
    }
    if jv.is_bool() {
        return jv.as_bool().to_string();
    }
    if let Some(n) = numeric_value(jv) {
        if n.is_nan() {
            return "NaN".to_string();
        }
        if n == f64::INFINITY {
            return "Infinity".to_string();
        }
        if n == f64::NEG_INFINITY {
            return "-Infinity".to_string();
        }
        if n.is_finite() && n.fract() == 0.0 {
            return format!("{}", n as i64);
        }
        return format!("{n}");
    }
    if is_array_value(jv) {
        return "[]".to_string();
    }
    if jv.is_pointer() {
        return "{}".to_string();
    }
    describe_received(value)
}

fn throw_unknown_signal(value: f64) -> ! {
    let message = format!("Unknown signal: {}", display_value(value));
    throw_type_error_with_code(&message, "ERR_UNKNOWN_SIGNAL")
}

fn throw_invalid_signal_code(value: f64) -> ! {
    let expected = signal_names()
        .into_iter()
        .map(|name| format!("'{name}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let received = if let Some(s) = read_js_string(value) {
        format!("'{s}'")
    } else {
        display_value(value)
    };
    let message =
        format!("The argument 'signalCode' must be one of: {expected}. Received {received}");
    throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE")
}

fn normalize_process_signal(signal: f64) -> i32 {
    let jv = JSValue::from_bits(signal.to_bits());
    if jv.is_undefined() {
        return signal_number_by_name("SIGTERM").unwrap_or(15);
    }
    if jv.is_null() {
        return 0;
    }
    if let Some(name) = read_js_string(signal) {
        return signal_number_by_name(&name).unwrap_or_else(|| throw_unknown_signal(signal));
    }
    if let Some(n) = numeric_value(jv) {
        if n.is_nan() || n == 0.0 {
            return 0;
        }
        if !n.is_finite() || n.fract() != 0.0 || n < i32::MIN as f64 || n > i32::MAX as f64 {
            throw_unknown_signal(signal);
        }
        return n as i32;
    }
    throw_unknown_signal(signal)
}

#[cfg(unix)]
fn kill_errno_code(errno: i32) -> &'static str {
    match errno {
        x if x == libc::EINVAL => "EINVAL",
        x if x == libc::ESRCH => "ESRCH",
        x if x == libc::EPERM => "EPERM",
        x if x == libc::EINTR => "EINTR",
        _ => "EIO",
    }
}

/// Throw the Node-shaped `process.kill` failure: an `Error` whose message is
/// `kill <CODE>` with `.code = <CODE>` and `.syscall = "kill"`. Shared by the
/// Unix (errno-derived) and Windows (Win32-error-derived) arms so both report
/// failures identically.
#[cfg(any(unix, windows))]
fn throw_kill_error_code(code: &'static str) -> ! {
    let message = format!("kill {code}");
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    crate::node_submodules::register_error_syscall(msg, "kill");
    let err = crate::error::js_error_new_with_message(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[cfg(unix)]
fn throw_kill_error(errno: i32) -> ! {
    throw_kill_error_code(kill_errno_code(errno))
}

/// Map a Win32 error from the `process.kill` syscalls to the Node errno code
/// the Unix arm would surface for the analogous failure (libuv's
/// `uv_translate_sys_error` conventions: invalid pid → ESRCH, access denied →
/// EPERM, anything else falls back to the file's existing EIO convention).
#[cfg(windows)]
fn win_error_to_kill_code(err: u32) -> &'static str {
    use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER};
    match err {
        ERROR_INVALID_PARAMETER => "ESRCH",
        ERROR_ACCESS_DENIED => "EPERM",
        _ => "EIO",
    }
}

/// Win32 half of `process.kill(pid, sig)`, mirroring libuv's `uv_kill`
/// (`src/win/process.c`). Returns `Ok(())` on success or the Node errno code
/// for `throw_kill_error_code`. Kept JS-free so unit tests can exercise it
/// directly.
///
/// * `sig == 0` — existence probe: `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)`
///   then `GetExitCodeProcess`; only a `STILL_ACTIVE` process counts as alive
///   (an exited process whose pid is pinned by an open handle reports ESRCH,
///   like Unix `kill(pid, 0)` on a reaped pid).
/// * SIGHUP/SIGINT/SIGQUIT/SIGKILL/SIGTERM — `OpenProcess(PROCESS_TERMINATE)`
///   + `TerminateProcess(h, 1)`. There is no graceful termination on Windows;
///   this is exactly what libuv does for all of them.
/// * Other signals in `0..NSIG` — ENOSYS (libuv's "unsupported signal");
///   outside that range — EINVAL.
#[cfg(windows)]
fn win_process_kill(pid: i32, sig: i32) -> Result<(), &'static str> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetExitCodeProcess, OpenProcess, TerminateProcess,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    };

    // MSVC CRT signal numbers (crt/signal.h); NSIG bound mirrors libuv.
    const SIGHUP_N: i32 = 1;
    const SIGINT_N: i32 = 2;
    const SIGQUIT_N: i32 = 3;
    const SIGKILL_N: i32 = 9;
    const SIGTERM_N: i32 = 15;
    const NSIG: i32 = 23;

    if !(0..NSIG).contains(&sig) {
        return Err("EINVAL");
    }
    let desired_access = match sig {
        0 => PROCESS_QUERY_LIMITED_INFORMATION,
        // QUERY_LIMITED alongside TERMINATE (libuv does the same): the
        // already-exited fallback below needs GetExitCodeProcess, which
        // fails with ERROR_ACCESS_DENIED on a TERMINATE-only handle.
        SIGHUP_N | SIGINT_N | SIGQUIT_N | SIGKILL_N | SIGTERM_N => {
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION
        }
        _ => return Err("ENOSYS"),
    };

    unsafe {
        // libuv: pid 0 targets the current process via the pseudo-handle
        // (Windows has no Unix-style process groups for kill).
        let is_self = pid == 0;
        let handle = if is_self {
            GetCurrentProcess()
        } else {
            OpenProcess(desired_access, 0, pid as u32)
        };
        if handle.is_null() {
            // ERROR_INVALID_PARAMETER == no such pid → ESRCH.
            return Err(win_error_to_kill_code(GetLastError()));
        }
        // The pseudo-handle from GetCurrentProcess needs no CloseHandle.
        let close = |h| {
            if !is_self {
                CloseHandle(h);
            }
        };

        if sig == 0 {
            let mut status: u32 = 0;
            if GetExitCodeProcess(handle, &mut status) == 0 {
                let err = GetLastError();
                close(handle);
                return Err(win_error_to_kill_code(err));
            }
            close(handle);
            if status != STILL_ACTIVE as u32 {
                return Err("ESRCH");
            }
            return Ok(());
        }

        if TerminateProcess(handle, 1) == 0 {
            let err = GetLastError();
            // libuv: TerminateProcess fails with ERROR_ACCESS_DENIED when the
            // target already exited — report that as ESRCH, not EPERM.
            if err == windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED {
                let mut status: u32 = 0;
                if GetExitCodeProcess(handle, &mut status) != 0 && status != STILL_ACTIVE as u32 {
                    close(handle);
                    return Err("ESRCH");
                }
            }
            close(handle);
            return Err(win_error_to_kill_code(err));
        }
        close(handle);
    }
    Ok(())
}

/// process.kill(pid, signal?) — send signal to process. signal=0 means
/// existence check, and omitted/undefined signal defaults to SIGTERM.
#[no_mangle]
pub extern "C" fn js_process_kill(pid: f64, signal: f64) -> f64 {
    let pid_jv = JSValue::from_bits(pid.to_bits());
    if !is_numeric(pid_jv) {
        let message = format!(
            "The \"pid\" argument must be of type number. Received {}",
            describe_received(pid)
        );
        throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }

    let pid_i = if pid_jv.is_int32() {
        pid_jv.as_int32()
    } else {
        pid_jv.as_number() as i32
    };
    let sig_i = normalize_process_signal(signal);
    #[cfg(unix)]
    unsafe {
        if libc::kill(pid_i, sig_i) != 0 {
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            throw_kill_error(errno);
        }
    }
    #[cfg(windows)]
    {
        if let Err(code) = win_process_kill(pid_i, sig_i) {
            throw_kill_error_code(code);
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (pid_i, sig_i);
    }
    f64::from_bits(TAG_TRUE)
}

#[no_mangle]
pub extern "C" fn js_util_convert_process_signal_to_exit_code(signal_code: f64) -> f64 {
    let Some(name) = read_js_string(signal_code) else {
        throw_invalid_signal_code(signal_code);
    };
    let Some(signal) = signal_number_by_name(&name) else {
        throw_invalid_signal_code(signal_code);
    };
    (128 + signal) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_exit_codes_match_linux_node() {
        assert_eq!(signal_number_by_name("SIGTERM"), Some(15));
        assert_eq!(signal_number_by_name("SIGINT"), Some(2));
        assert_eq!(signal_number_by_name("SIGKILL"), Some(9));
        assert_eq!(signal_number_by_name("sigterm"), None);
    }

    /// Spawn a child that blocks until killed: `cmd /c pause` waits for a
    /// byte on stdin, which we pipe and never write.
    #[cfg(windows)]
    fn spawn_blocked_child() -> std::process::Child {
        std::process::Command::new("cmd")
            .args(["/c", "pause"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn cmd /c pause")
    }

    #[cfg(windows)]
    #[test]
    fn win_kill_terminates_child_and_sig0_tracks_liveness() {
        let mut child = spawn_blocked_child();
        let pid = child.id() as i32;

        // sig 0 existence probe: alive → Ok.
        assert_eq!(win_process_kill(pid, 0), Ok(()));

        // SIGTERM → TerminateProcess(h, 1): child exits with code 1.
        assert_eq!(win_process_kill(pid, 15), Ok(()));
        let status = child.wait().expect("wait on terminated child");
        assert_eq!(status.code(), Some(1));

        // `child` (still in scope) holds the process handle, so the pid
        // cannot be recycled: the probe must now see the exited process
        // (GetExitCodeProcess != STILL_ACTIVE) and report ESRCH.
        assert_eq!(win_process_kill(pid, 0), Err("ESRCH"));
        // Terminating an already-exited process also reports ESRCH
        // (libuv's ERROR_ACCESS_DENIED + !STILL_ACTIVE special case).
        assert_eq!(win_process_kill(pid, 15), Err("ESRCH"));
    }

    #[cfg(windows)]
    #[test]
    fn win_kill_reports_esrch_for_nonexistent_pid() {
        // Negative pid → OpenProcess(ERROR_INVALID_PARAMETER) → ESRCH,
        // for both the probe and the terminate path.
        assert_eq!(win_process_kill(-1, 0), Err("ESRCH"));
        assert_eq!(win_process_kill(-1, 15), Err("ESRCH"));
    }

    #[cfg(windows)]
    #[test]
    fn win_kill_rejects_unsupported_signals() {
        // SIGBREAK(21) is deliverable via the console but not via kill —
        // libuv reports ENOSYS. Out-of-range numbers are EINVAL. Both are
        // rejected before any handle is opened, so pid 0 (self) is safe.
        assert_eq!(win_process_kill(0, 21), Err("ENOSYS"));
        assert_eq!(win_process_kill(0, -3), Err("EINVAL"));
        assert_eq!(win_process_kill(0, 99), Err("EINVAL"));
    }

    /// Serializes the tests that mutate the process-wide signal slots
    /// (listener counts, pending counts, the global drain) — cargo runs
    /// tests on parallel threads and interleaved drains would be flaky.
    #[cfg(windows)]
    static SIGNAL_SLOT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Drive the console control handler directly (it is an ordinary
    /// `extern "system"` fn) — deterministic coverage of the mapping,
    /// TRUE/FALSE return semantics, and the pending→drain marshal without
    /// touching the real console. A live `GenerateConsoleCtrlEvent`
    /// round-trip is deliberately NOT tested here: group 0 would hit every
    /// process sharing the test console (cargo, the shell), and a fresh
    /// process group needs a perry-compiled child binary — verified
    /// manually instead (see PR body).
    #[cfg(windows)]
    #[test]
    fn win_console_ctrl_handler_semantics() {
        use windows_sys::Win32::System::Console::{
            CTRL_BREAK_EVENT, CTRL_CLOSE_EVENT, CTRL_C_EVENT,
        };
        let _guard = SIGNAL_SLOT_TEST_LOCK.lock().unwrap();

        // No listeners registered → FALSE for every event: the default
        // terminate behavior must be preserved.
        assert_eq!(unsafe { win_console_ctrl_handler(CTRL_C_EVENT) }, 0);
        assert_eq!(unsafe { win_console_ctrl_handler(CTRL_BREAK_EVENT) }, 0);
        assert_eq!(unsafe { win_console_ctrl_handler(CTRL_CLOSE_EVENT) }, 0);
        // Unmapped control types (logoff/shutdown) → FALSE always.
        assert_eq!(unsafe { win_console_ctrl_handler(5) }, 0);
        assert_eq!(unsafe { win_console_ctrl_handler(6) }, 0);

        // With a SIGINT listener: CTRL_C → TRUE, records exactly one
        // pending SIGINT, which keeps the loop alive until drained.
        set_process_signal_listener_count("SIGINT", 1);
        assert_eq!(unsafe { win_console_ctrl_handler(CTRL_C_EVENT) }, 1);
        // CTRL_BREAK maps to SIGBREAK, which has no listener → FALSE.
        assert_eq!(unsafe { win_console_ctrl_handler(CTRL_BREAK_EVENT) }, 0);
        assert!(has_active_process_signal_listeners());
        assert_eq!(take_pending_process_signals(), vec!["SIGINT"]);
        assert!(!has_active_process_signal_listeners());

        // Removing the last listener restores default behavior and drops
        // any undelivered pending count.
        assert_eq!(unsafe { win_console_ctrl_handler(CTRL_C_EVENT) }, 1);
        set_process_signal_listener_count("SIGINT", 0);
        assert_eq!(unsafe { win_console_ctrl_handler(CTRL_C_EVENT) }, 0);
        assert!(take_pending_process_signals().is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn win_sigterm_registration_is_accepted_and_inert() {
        let _guard = SIGNAL_SLOT_TEST_LOCK.lock().unwrap();
        // Node allows process.on('SIGTERM') on Windows; it just never fires
        // from the console. Registration must not error, must not pin the
        // event loop, and must never produce a pending signal.
        assert!(is_process_signal_name("SIGTERM"));
        assert!(is_process_signal_name("SIGBREAK"));
        assert!(is_process_signal_name("SIGHUP"));
        set_process_signal_listener_count("SIGTERM", 1);
        assert!(!has_active_process_signal_listeners());
        assert!(take_pending_process_signals().is_empty());
        set_process_signal_listener_count("SIGTERM", 0);
    }
}
