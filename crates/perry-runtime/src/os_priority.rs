use crate::js_string_from_bytes;

fn throw_os_type_error(message: String) -> ! {
    let msg_ptr = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg_ptr, "ERR_INVALID_ARG_TYPE");
    let err = crate::error::js_typeerror_new(msg_ptr);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn throw_os_range_error(message: String) -> ! {
    let msg_ptr = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg_ptr, "ERR_OUT_OF_RANGE");
    let err = crate::error::js_rangeerror_new(msg_ptr);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn throw_os_system_error(syscall: &'static str, errno: i32) -> ! {
    let message = if errno != 0 {
        format!("A system error occurred: {syscall} failed with errno {errno}")
    } else {
        format!("A system error occurred: {syscall} failed")
    };
    let msg_ptr = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg_ptr, "ERR_SYSTEM_ERROR");
    crate::node_submodules::register_error_syscall(msg_ptr, syscall);
    let err = crate::error::js_error_new_with_name_message(b"SystemError", msg_ptr);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn js_value_to_i32(value: f64, name: &str, default: Option<i32>) -> i32 {
    let js_value = crate::JSValue::from_bits(value.to_bits());
    if js_value.is_undefined() {
        if let Some(default) = default {
            return default;
        }
        throw_os_type_error(format!("The \"{name}\" argument must be of type number"));
    }

    let number = if js_value.is_int32() {
        js_value.as_int32() as f64
    } else if js_value.is_number() {
        js_value.as_number()
    } else {
        throw_os_type_error(format!("The \"{name}\" argument must be of type number"));
    };

    if !number.is_finite()
        || number.fract() != 0.0
        || number < i32::MIN as f64
        || number > i32::MAX as f64
    {
        throw_os_range_error(format!("The value of \"{name}\" is out of range"));
    }
    number as i32
}

#[cfg(target_os = "linux")]
unsafe fn os_errno_location() -> *mut libc::c_int {
    libc::__errno_location()
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
unsafe fn os_errno_location() -> *mut libc::c_int {
    libc::__error()
}

#[cfg(all(
    unix,
    not(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd"
    ))
))]
unsafe fn os_errno_location() -> *mut libc::c_int {
    std::ptr::null_mut()
}

#[cfg(unix)]
fn clear_os_errno() {
    unsafe {
        let errno = os_errno_location();
        if !errno.is_null() {
            *errno = 0;
        }
    }
}

#[cfg(unix)]
fn current_os_errno() -> i32 {
    unsafe {
        let errno = os_errno_location();
        if errno.is_null() {
            0
        } else {
            *errno
        }
    }
}

#[cfg(unix)]
fn os_get_priority(pid: i32) -> Result<i32, i32> {
    clear_os_errno();
    let priority = unsafe { libc::getpriority(libc::PRIO_PROCESS, pid as libc::id_t) };
    let errno = current_os_errno();
    if priority == -1 && errno != 0 {
        Err(errno)
    } else {
        Ok(priority)
    }
}

#[cfg(unix)]
fn os_set_priority(pid: i32, priority: i32) -> Result<(), i32> {
    let rc = unsafe {
        libc::setpriority(
            libc::PRIO_PROCESS,
            pid as libc::id_t,
            priority as libc::c_int,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(0))
    }
}

#[cfg(not(unix))]
fn os_get_priority(pid: i32) -> Result<i32, i32> {
    if pid == 0 {
        Ok(0)
    } else {
        Err(0)
    }
}

#[cfg(not(unix))]
fn os_set_priority(pid: i32, _priority: i32) -> Result<(), i32> {
    if pid == 0 {
        Ok(())
    } else {
        Err(0)
    }
}

/// Get the scheduling priority for a process. `undefined` defaults to the
/// current process, matching Node/Deno's `pid = 0` behavior.
#[no_mangle]
pub extern "C" fn js_os_get_priority(pid_value: f64) -> f64 {
    let pid = js_value_to_i32(pid_value, "pid", Some(0));
    match os_get_priority(pid) {
        Ok(priority) => priority as f64,
        Err(errno) => throw_os_system_error("uv_os_getpriority", errno),
    }
}

/// Set process priority. One argument is treated as `priority`; two arguments
/// are `pid, priority`.
#[no_mangle]
pub extern "C" fn js_os_set_priority(pid_or_priority: f64, priority_value: f64) -> f64 {
    let priority_arg = crate::JSValue::from_bits(priority_value.to_bits());
    let (pid, priority) = if priority_arg.is_undefined() {
        (0, js_value_to_i32(pid_or_priority, "priority", None))
    } else {
        (
            js_value_to_i32(pid_or_priority, "pid", None),
            js_value_to_i32(priority_value, "priority", None),
        )
    };

    if !(-20..=19).contains(&priority) {
        throw_os_range_error("The value of \"priority\" is out of range".to_string());
    }

    match os_set_priority(pid, priority) {
        Ok(()) => f64::from_bits(crate::value::TAG_UNDEFINED),
        Err(errno) => throw_os_system_error("uv_os_setpriority", errno),
    }
}
