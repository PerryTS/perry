use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::value::JSValue;

use super::{
    cp_decode_status, cp_get_field, cp_io_error_code, cp_object_ptr, cp_read_kill_signal,
    cp_read_stdio, cp_value_to_bytes, CpExit, CpStdio, CP_SIGTERM,
};

const CP_DEFAULT_MAX_BUFFER: usize = 1024 * 1024;

/// Options that affect buffered sync execution after the `Command` is built.
pub(super) struct CpRunOptions {
    input: Option<Vec<u8>>,
    timeout: Option<Duration>,
    kill_signal: i32,
    pub(super) max_buffer: usize,
    stdio: [CpStdio; 3],
}

impl CpRunOptions {
    /// Signal used to terminate the child on `timeout` / `maxBuffer` overrun.
    /// Needed by the async exec reactor (#4912) to kill on a maxBuffer breach.
    pub(super) fn kill_signal(&self) -> i32 {
        self.kill_signal
    }

    /// The `timeout` limit, if any — the async exec reactor (#4912) arms a
    /// timeout thread with it.
    pub(super) fn timeout(&self) -> Option<Duration> {
        self.timeout
    }
}

impl Default for CpRunOptions {
    fn default() -> Self {
        Self {
            input: None,
            timeout: None,
            kill_signal: CP_SIGTERM,
            max_buffer: CP_DEFAULT_MAX_BUFFER,
            stdio: [CpStdio::Pipe; 3],
        }
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

    cp_read_timing_and_buffer_options(opts_val, &mut options);
    options
}

/// Read spawnSync options, including the stdio capture policy used to shape
/// `stdout`/`stderr`/`output`.
pub(super) fn cp_read_spawn_sync_run_options(opts_val: f64) -> CpRunOptions {
    let mut options = cp_read_run_options(opts_val);
    let stdio = cp_read_stdio(opts_val, 3);
    options.stdio = [
        stdio.first().copied().unwrap_or(CpStdio::Pipe),
        stdio.get(1).copied().unwrap_or(CpStdio::Pipe),
        stdio.get(2).copied().unwrap_or(CpStdio::Pipe),
    ];
    options
}

/// Read sync exec options, including explicit stdio policies that affect
/// return and thrown-error output slots.
pub(super) fn cp_read_sync_stdio_run_options(opts_val: f64) -> CpRunOptions {
    let mut options = cp_read_run_options(opts_val);
    let stdio = cp_read_stdio(opts_val, 3);
    options.stdio = [
        stdio.first().copied().unwrap_or(CpStdio::Pipe),
        stdio.get(1).copied().unwrap_or(CpStdio::Pipe),
        stdio.get(2).copied().unwrap_or(CpStdio::Pipe),
    ];
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
    options.kill_signal = cp_read_kill_signal(opts_val);

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
    pub(super) stdout_piped: bool,
    pub(super) stderr_piped: bool,
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

/// Spawn `command`, run it to completion, and capture the configured stdio.
/// Piped stdin without input is closed so children that read stdin see EOF
/// instead of blocking. Used by synchronous + buffered-callback entry points.
pub(super) fn cp_run_to_completion(mut command: Command, options: &CpRunOptions) -> CpRun {
    let stdin_piped = matches!(options.stdio[0], CpStdio::Pipe) && options.input.is_some();
    let stdout_piped = matches!(options.stdio[1], CpStdio::Pipe);
    let stderr_piped = matches!(options.stdio[2], CpStdio::Pipe);
    command.stdin(match options.stdio[0] {
        CpStdio::Pipe if options.input.is_some() => Stdio::piped(),
        CpStdio::Pipe | CpStdio::Ignore => Stdio::null(),
        CpStdio::Inherit => Stdio::inherit(),
        CpStdio::Fd(fd) => super::cp_stdio_from_fd(fd),
    });
    command.stdout(match options.stdio[1] {
        CpStdio::Pipe => Stdio::piped(),
        CpStdio::Ignore => Stdio::null(),
        CpStdio::Inherit => Stdio::inherit(),
        CpStdio::Fd(fd) => super::cp_stdio_from_fd(fd),
    });
    command.stderr(match options.stdio[2] {
        CpStdio::Pipe => Stdio::piped(),
        CpStdio::Ignore => Stdio::null(),
        CpStdio::Inherit => Stdio::inherit(),
        CpStdio::Fd(fd) => super::cp_stdio_from_fd(fd),
    });
    match command.spawn() {
        Ok(mut child) => {
            let pid = child.id();
            if stdin_piped {
                if let (Some(input), Some(mut stdin)) = (&options.input, child.stdin.take()) {
                    let _ = stdin.write_all(input);
                }
            }
            let mut run_error =
                cp_wait_for_timeout(&mut child, options.timeout, options.kill_signal);
            match child.wait_with_output() {
                Ok(o) => {
                    let CpExit { code, signal } = cp_decode_status(&o.status);
                    if run_error.is_none()
                        && ((stdout_piped && o.stdout.len() > options.max_buffer)
                            || (stderr_piped && o.stderr.len() > options.max_buffer))
                    {
                        run_error = Some(CpRunError::MaxBuffer);
                    }
                    let (code, signal) = match run_error {
                        Some(CpRunError::Timeout) => (None, Some(options.kill_signal)),
                        _ => (code, signal),
                    };
                    CpRun {
                        stdout: o.stdout,
                        stderr: o.stderr,
                        stdout_piped,
                        stderr_piped,
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
                    stdout_piped,
                    stderr_piped,
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
            stdout_piped,
            stderr_piped,
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
    kill_signal: i32,
) -> Option<CpRunError> {
    let timeout = timeout?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return None,
            Ok(None) => {
                if Instant::now() >= deadline {
                    cp_terminate_child(child, kill_signal);
                    return Some(CpRunError::Timeout);
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                std::thread::sleep(remaining.min(Duration::from_millis(5)));
            }
            Err(_) => return None,
        }
    }
}

fn cp_terminate_child(child: &mut std::process::Child, kill_signal: i32) {
    #[cfg(unix)]
    unsafe {
        let _ = libc::kill(child.id() as i32, kill_signal);
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
}
