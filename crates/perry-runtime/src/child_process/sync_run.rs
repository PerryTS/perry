#[cfg(unix)]
use std::fs::File;
use std::io::Write;
#[cfg(unix)]
use std::os::fd::FromRawFd;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::value::JSValue;

use super::{
    cp_array_ptr, cp_decode_status, cp_get_field, cp_io_error_code, cp_js_string_value,
    cp_object_ptr, cp_value_to_bytes, CpExit,
};

const CP_DEFAULT_MAX_BUFFER: usize = 1024 * 1024;
const CP_SIGTERM: i32 = 15;

/// Options that affect buffered sync execution after the `Command` is built.
pub(super) struct CpRunOptions {
    input: Option<Vec<u8>>,
    timeout: Option<Duration>,
    pub(super) max_buffer: usize,
    stdio: [CpSyncStdio; 3],
}

impl Default for CpRunOptions {
    fn default() -> Self {
        Self {
            input: None,
            timeout: None,
            max_buffer: CP_DEFAULT_MAX_BUFFER,
            stdio: [CpSyncStdio::Pipe; 3],
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CpSyncStdio {
    Pipe,
    Ignore,
    Inherit,
    Fd(i32),
}

impl CpRunOptions {
    pub(super) fn stdout_piped(&self) -> bool {
        self.stdio[1] == CpSyncStdio::Pipe
    }

    pub(super) fn stderr_piped(&self) -> bool {
        self.stdio[2] == CpSyncStdio::Pipe
    }
}

fn cp_read_option_number(opts_val: f64, key: &[u8]) -> Option<f64> {
    if cp_object_ptr(opts_val).is_none() {
        return None;
    }
    let value = cp_get_field(opts_val, key);
    let js_value = JSValue::from_bits(value.to_bits());
    if js_value.is_undefined() || js_value.is_null() {
        return None;
    }
    let n = js_value.to_number();
    if n.is_finite() {
        Some(n)
    } else {
        None
    }
}

fn cp_is_input_byte_value(value: f64) -> bool {
    let js_value = JSValue::from_bits(value.to_bits());
    if js_value.is_any_string() {
        return true;
    }
    if !js_value.is_pointer() {
        return false;
    }

    let raw = (value.to_bits() & crate::value::POINTER_MASK) as usize;
    if raw < 0x10000 {
        return false;
    }
    if crate::typedarray::lookup_typed_array_kind(raw).is_some() {
        return true;
    }
    crate::buffer::is_registered_buffer(raw) && !crate::buffer::is_any_array_buffer(raw)
}

fn cp_read_input_bytes(value: f64) -> Option<Vec<u8>> {
    let js_value = JSValue::from_bits(value.to_bits());
    if js_value.is_undefined() || js_value.is_null() {
        return None;
    }
    if cp_is_input_byte_value(value) {
        return Some(cp_value_to_bytes(value));
    }

    let message = format!(
        "The \"options.stdio[0]\" property must be of type string or an instance of Buffer, TypedArray, or DataView. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

/// Read sync-only buffered options. `input` follows Node's byte-value
/// validation; non-finite numeric limits are ignored so legacy callers keep the
/// previous behavior.
pub(super) fn cp_read_run_options(opts_val: f64) -> CpRunOptions {
    let mut options = CpRunOptions::default();
    if cp_object_ptr(opts_val).is_none() {
        return options;
    }

    if let Some(input) = cp_read_input_bytes(cp_get_field(opts_val, b"input")) {
        options.input = Some(input);
    }
    options.stdio = cp_read_sync_stdio(opts_val);

    cp_read_timing_and_buffer_options(opts_val, &mut options);
    options
}

/// Read async buffered limits. Unlike the sync helpers, async `exec` and
/// `execFile` do not have a documented `input` option, so only timing and
/// buffer limits are consumed here.
pub(super) fn cp_read_async_run_options(opts_val: f64) -> CpRunOptions {
    let mut options = CpRunOptions::default();
    if cp_object_ptr(opts_val).is_none() {
        return options;
    }
    cp_read_timing_and_buffer_options(opts_val, &mut options);
    options
}

fn cp_read_timing_and_buffer_options(opts_val: f64, options: &mut CpRunOptions) {
    if let Some(timeout) = cp_read_option_number(opts_val, b"timeout") {
        if timeout > 0.0 {
            options.timeout = Some(Duration::from_millis(timeout as u64));
        }
    }

    if let Some(max_buffer) = cp_read_option_number(opts_val, b"maxBuffer") {
        if max_buffer >= 0.0 {
            options.max_buffer = max_buffer.min(usize::MAX as f64) as usize;
        }
    }
}

fn cp_sync_stdio_kind(value: f64) -> CpSyncStdio {
    match cp_js_string_value(value).as_deref() {
        Some("ignore") => return CpSyncStdio::Ignore,
        Some("inherit") => return CpSyncStdio::Inherit,
        Some("pipe") => return CpSyncStdio::Pipe,
        _ => {}
    }

    let js_value = JSValue::from_bits(value.to_bits());
    if js_value.is_number() || js_value.is_int32() {
        let n = js_value.to_number();
        if n.is_finite() && n >= 0.0 && n.fract() == 0.0 && n <= i32::MAX as f64 {
            return CpSyncStdio::Fd(n as i32);
        }
    }

    CpSyncStdio::Pipe
}

fn cp_read_sync_stdio(opts_val: f64) -> [CpSyncStdio; 3] {
    let mut out = [CpSyncStdio::Pipe; 3];
    let stdio = cp_get_field(opts_val, b"stdio");

    if let Some(s) = cp_js_string_value(stdio) {
        match s.as_str() {
            "ignore" => out = [CpSyncStdio::Ignore; 3],
            "inherit" => out = [CpSyncStdio::Inherit; 3],
            "pipe" => {}
            _ => {}
        }
        return out;
    }

    let Some(arr) = cp_array_ptr(stdio) else {
        return out;
    };
    let n = unsafe { (*arr).length }.min(3);
    for i in 0..n {
        out[i as usize] = cp_sync_stdio_kind(crate::array::js_array_get_f64(arr, i));
    }
    out
}

#[cfg(unix)]
fn cp_fd_stdio(fd: i32) -> Option<Stdio> {
    if let Some(file) = crate::fs::clone_registered_fd(fd) {
        return Some(Stdio::from(file));
    }

    let dup_fd = unsafe { libc::dup(fd) };
    if dup_fd < 0 {
        return None;
    }
    let file = unsafe { File::from_raw_fd(dup_fd) };
    Some(Stdio::from(file))
}

#[cfg(not(unix))]
fn cp_fd_stdio(fd: i32) -> Option<Stdio> {
    crate::fs::clone_registered_fd(fd).map(Stdio::from)
}

fn cp_stdio_from_kind(kind: CpSyncStdio, pipe: Stdio) -> Stdio {
    match kind {
        CpSyncStdio::Pipe => pipe,
        CpSyncStdio::Ignore => Stdio::null(),
        CpSyncStdio::Inherit => Stdio::inherit(),
        CpSyncStdio::Fd(fd) => cp_fd_stdio(fd).unwrap_or_else(Stdio::null),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CpRunError {
    MaxBuffer,
    Timeout,
}

impl CpRunError {
    pub(super) fn code(self) -> &'static str {
        match self {
            Self::MaxBuffer => "ENOBUFS",
            Self::Timeout => "ETIMEDOUT",
        }
    }
}

/// Outcome of running a child to completion (buffered).
pub(super) struct CpRun {
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
    pub(super) code: Option<i32>,
    pub(super) signal: Option<i32>,
    pub(super) pid: Option<u32>,
    /// `Some((code, message))` when the child could not be spawned at all.
    pub(super) spawn_error: Option<(&'static str, String)>,
    /// `Some` for deterministic buffered execution failures after spawn.
    pub(super) run_error: Option<CpRunError>,
}

impl CpRun {
    pub(super) fn success(&self) -> bool {
        self.spawn_error.is_none() && self.run_error.is_none() && self.code == Some(0)
    }
}

/// Spawn `command` with piped stdout/stderr (and a closed stdin so children
/// that read stdin see EOF instead of blocking), run it to completion, and
/// capture stdout/stderr/exit/pid. Used by the synchronous + buffered-callback
/// entry points.
pub(super) fn cp_run_to_completion(mut command: Command, options: &CpRunOptions) -> CpRun {
    let pipe_stdin = if options.input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    };
    command.stdin(cp_stdio_from_kind(options.stdio[0], pipe_stdin));
    command.stdout(cp_stdio_from_kind(options.stdio[1], Stdio::piped()));
    command.stderr(cp_stdio_from_kind(options.stdio[2], Stdio::piped()));
    match command.spawn() {
        Ok(mut child) => {
            let pid = child.id();
            if let Some(input) = &options.input {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(input);
                }
            }
            let mut run_error = cp_wait_for_timeout(&mut child, options.timeout);
            match child.wait_with_output() {
                Ok(o) => {
                    let CpExit { code, signal } = cp_decode_status(&o.status);
                    if run_error.is_none()
                        && (o.stdout.len() > options.max_buffer
                            || o.stderr.len() > options.max_buffer)
                    {
                        run_error = Some(CpRunError::MaxBuffer);
                    }
                    let (code, signal) = match run_error {
                        Some(CpRunError::Timeout) => (None, Some(CP_SIGTERM)),
                        _ => (code, signal),
                    };
                    CpRun {
                        stdout: o.stdout,
                        stderr: o.stderr,
                        code,
                        signal,
                        pid: Some(pid),
                        spawn_error: None,
                        run_error,
                    }
                }
                Err(e) => CpRun {
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    code: None,
                    signal: None,
                    pid: Some(pid),
                    spawn_error: Some((cp_io_error_code(&e), e.to_string())),
                    run_error: None,
                },
            }
        }
        Err(e) => CpRun {
            stdout: Vec::new(),
            stderr: Vec::new(),
            code: None,
            signal: None,
            pid: None,
            spawn_error: Some((cp_io_error_code(&e), e.to_string())),
            run_error: None,
        },
    }
}

fn cp_wait_for_timeout(
    child: &mut std::process::Child,
    timeout: Option<Duration>,
) -> Option<CpRunError> {
    let timeout = timeout?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return None,
            Ok(None) => {
                if Instant::now() >= deadline {
                    cp_terminate_child(child);
                    return Some(CpRunError::Timeout);
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                std::thread::sleep(remaining.min(Duration::from_millis(5)));
            }
            Err(_) => return None,
        }
    }
}

fn cp_terminate_child(child: &mut std::process::Child) {
    #[cfg(unix)]
    unsafe {
        let _ = libc::kill(child.id() as i32, libc::SIGTERM);
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
}
