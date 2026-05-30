use crate::fs::validate::{describe_received, is_numeric, throw_type_error_with_code};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::{JSValue, TAG_TRUE};

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
