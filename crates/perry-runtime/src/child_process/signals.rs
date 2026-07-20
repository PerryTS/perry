use super::*;

use std::collections::HashMap;
use std::fs::File;
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};

use sync_run::{
    cp_read_async_run_options, cp_read_spawn_sync_run_options, cp_read_sync_stdio_run_options,
    cp_run_to_completion, CpRun, CpRunError, CpRunOptions,
};

use crate::closure::{
    js_closure_alloc, js_closure_get_capture_ptr, js_closure_set_capture_ptr, js_native_call_value,
    js_register_closure_arity, ClosureHeader,
};
use crate::object::{
    js_implicit_this_get, js_implicit_this_set, js_object_alloc_with_shape,
    js_object_get_field_by_name_f64, js_object_set_field, js_object_set_field_by_name,
    ObjectHeader,
};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;

pub(crate) const CP_SIGTERM: i32 = 15;

#[cfg(unix)]
pub(crate) fn cp_signal_name(sig: i32) -> &'static str {
    match sig {
        x if x == libc::SIGHUP => "SIGHUP",
        x if x == libc::SIGINT => "SIGINT",
        x if x == libc::SIGQUIT => "SIGQUIT",
        x if x == libc::SIGILL => "SIGILL",
        x if x == libc::SIGTRAP => "SIGTRAP",
        x if x == libc::SIGABRT => "SIGABRT",
        x if x == libc::SIGBUS => "SIGBUS",
        x if x == libc::SIGFPE => "SIGFPE",
        x if x == libc::SIGKILL => "SIGKILL",
        x if x == libc::SIGUSR1 => "SIGUSR1",
        x if x == libc::SIGSEGV => "SIGSEGV",
        x if x == libc::SIGUSR2 => "SIGUSR2",
        x if x == libc::SIGPIPE => "SIGPIPE",
        x if x == libc::SIGALRM => "SIGALRM",
        x if x == libc::SIGTERM => "SIGTERM",
        x if x == libc::SIGSTOP => "SIGSTOP",
        x if x == libc::SIGCONT => "SIGCONT",
        _ => "SIGTERM",
    }
}

#[cfg(not(unix))]
pub(crate) fn cp_signal_name(sig: i32) -> &'static str {
    match sig {
        1 => "SIGHUP",
        2 => "SIGINT",
        6 => "SIGABRT",
        9 => "SIGKILL",
        11 => "SIGSEGV",
        15 => "SIGTERM",
        _ => "SIGTERM",
    }
}

#[cfg(unix)]
pub(crate) fn cp_signal_number(name: &str) -> Option<i32> {
    Some(match name {
        "SIGHUP" => libc::SIGHUP,
        "SIGINT" => libc::SIGINT,
        "SIGQUIT" => libc::SIGQUIT,
        "SIGILL" => libc::SIGILL,
        "SIGTRAP" => libc::SIGTRAP,
        "SIGABRT" => libc::SIGABRT,
        "SIGBUS" => libc::SIGBUS,
        "SIGFPE" => libc::SIGFPE,
        "SIGKILL" => libc::SIGKILL,
        "SIGUSR1" => libc::SIGUSR1,
        "SIGSEGV" => libc::SIGSEGV,
        "SIGUSR2" => libc::SIGUSR2,
        "SIGPIPE" => libc::SIGPIPE,
        "SIGALRM" => libc::SIGALRM,
        "SIGTERM" => libc::SIGTERM,
        "SIGSTOP" => libc::SIGSTOP,
        "SIGCONT" => libc::SIGCONT,
        _ => return None,
    })
}

/// Non-unix inverse of `cp_signal_name`, using the conventional POSIX numbers
/// for the names the non-unix `cp_signal_name` can report back. The number is
/// only a reporting token on Windows — every terminating signal degrades to
/// `TerminateProcess` at the kill site, exactly like Node — so the table
/// deliberately mirrors `cp_signal_name` above and nothing more.
#[cfg(not(unix))]
pub(crate) fn cp_signal_number(name: &str) -> Option<i32> {
    Some(match name {
        "SIGHUP" => 1,
        "SIGINT" => 2,
        "SIGABRT" => 6,
        "SIGKILL" => 9,
        "SIGSEGV" => 11,
        "SIGTERM" => 15,
        _ => return None,
    })
}

pub(crate) fn cp_signal_from_value(signal: f64) -> i32 {
    let js = JSValue::from_bits(signal.to_bits());
    if js.is_undefined() || js.is_null() {
        return CP_SIGTERM;
    }
    // `kill(9)` — numeric forms must be checked BEFORE the string lookup:
    // `cp_value_to_string` routes through the unified accessor, which coerces
    // numbers to their string form ("9"), and "9" is not a signal name. An
    // int32 can also arrive NaN-boxed, which a raw `is_finite()` misses.
    if js.is_int32() {
        let n = js.as_int32();
        return if n == 0 { CP_SIGTERM } else { n };
    }
    if signal.is_finite() {
        let n = signal as i32;
        return if n == 0 { CP_SIGTERM } else { n };
    }
    if let Some(name) = cp_value_to_string(signal) {
        return cp_signal_number(&name).unwrap_or(CP_SIGTERM);
    }
    CP_SIGTERM
}

pub(crate) fn cp_read_kill_signal(opts_val: f64) -> i32 {
    if cp_object_ptr(opts_val).is_none() {
        return CP_SIGTERM;
    }
    cp_signal_from_value(cp_get_field(opts_val, b"killSignal"))
}

pub(crate) fn cp_read_timeout(opts_val: f64) -> Option<std::time::Duration> {
    cp_object_ptr(opts_val)?;
    let value = cp_get_field(opts_val, b"timeout");
    let js = JSValue::from_bits(value.to_bits());
    if js.is_undefined() || js.is_null() {
        return None;
    }
    let timeout = js.to_number();
    if timeout.is_finite() && timeout > 0.0 {
        Some(std::time::Duration::from_millis(timeout as u64))
    } else {
        None
    }
}
