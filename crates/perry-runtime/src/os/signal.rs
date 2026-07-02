use crate::fs::validate::{describe_received, is_numeric, throw_type_error_with_code};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::{JSValue, TAG_TRUE};
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};

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

pub(crate) fn is_process_signal_name(name: &str) -> bool {
    #[cfg(unix)]
    {
        slot_by_name(name).is_some()
    }
    #[cfg(not(unix))]
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
    #[cfg(not(unix))]
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
    #[cfg(not(unix))]
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
    #[cfg(not(unix))]
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

#[cfg(unix)]
fn throw_kill_error(errno: i32) -> ! {
    let code = kill_errno_code(errno);
    let message = format!("kill {code}");
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    crate::node_submodules::register_error_syscall(msg, "kill");
    let err = crate::error::js_error_new_with_message(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
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
        let _ = (pid_i, sig_i);
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
}
