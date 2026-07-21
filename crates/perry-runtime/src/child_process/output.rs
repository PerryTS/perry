use super::*;

use sync_run::{CpRun, CpRunError, CpRunOptions};

use crate::object::js_object_set_field_by_name;
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;

pub(crate) const CP_ABORT_ERROR_CLASS_ID: u32 = 0x7FFF_FDC0;

// ============================================================================
// Output encoding + error shape — #1935 / #1936 / #1937 / #1938
// ============================================================================
//
// These helpers are shared by exec / execFile and the synchronous forms.
// `exec`/`execFile` default to `"utf8"` (callback stdout/stderr are strings);
// `execSync`/`execFileSync`/`spawnSync` default to `"buffer"`. `encoding:
// "buffer"` or `null` always yields Buffers; any other named encoding decodes
// the bytes with it. On a non-zero exit Node attaches diagnostic properties to
// the error (`code`/`signal`/`killed`/`cmd` for the callback form;
// `status`/`signal`/`pid`/`output`/`stdout`/`stderr`/`cmd` for the sync throw).

/// Resolved form for captured stdout/stderr bytes.
pub(crate) enum CpOutput {
    Buffer,
    Text(String),
}

/// Read the `encoding` option off a NaN-boxed options value. `default_text`
/// picks the default when `encoding` is absent (exec/execFile → utf8 text;
/// the sync forms → Buffer). `null` / `"buffer"` always mean Buffer.
pub(crate) fn cp_read_output_mode(opts_val: f64, default_text: bool) -> CpOutput {
    let enc = cp_get_field(opts_val, b"encoding");
    let bits = enc.to_bits();
    if JSValue::from_bits(bits).is_undefined() {
        return if default_text {
            CpOutput::Text("utf8".to_string())
        } else {
            CpOutput::Buffer
        };
    }
    if bits == TAG_NULL_BITS {
        return CpOutput::Buffer;
    }
    match cp_value_to_string(enc) {
        Some(s) if s.eq_ignore_ascii_case("buffer") => CpOutput::Buffer,
        Some(s) => CpOutput::Text(s),
        // Non-string, non-null, non-undefined encoding — fall back to Buffer.
        None => CpOutput::Buffer,
    }
}

/// Decode raw bytes to a `StringHeader` using a Node encoding name.
fn cp_encode_text(bytes: &[u8], enc: &str) -> *mut StringHeader {
    match enc.to_ascii_lowercase().as_str() {
        "hex" => crate::buffer::hex_encode_into_string(bytes),
        "base64" => crate::buffer::base64_encode_into_string(bytes),
        "base64url" => crate::buffer::base64url_encode_into_string(bytes),
        "latin1" | "binary" => {
            // latin1: each byte maps to a code point in U+0000..U+00FF.
            let s: String = bytes.iter().map(|&b| b as char).collect();
            js_string_from_bytes(s.as_ptr(), s.len() as u32)
        }
        // utf8 / utf-8 / ascii / unknown — store as UTF-8 (lossy for invalid).
        _ => {
            let s = String::from_utf8_lossy(bytes);
            js_string_from_bytes(s.as_ptr(), s.len() as u32)
        }
    }
}

/// Box captured bytes per the resolved output mode (Buffer or decoded string).
pub(crate) fn cp_box_output(bytes: &[u8], mode: &CpOutput) -> f64 {
    match mode {
        CpOutput::Buffer => cp_make_buffer(bytes),
        CpOutput::Text(enc) => crate::value::js_nanbox_string(cp_encode_text(bytes, enc) as i64),
    }
}

pub(crate) fn cp_box_run_output(bytes: &[u8], piped: bool, mode: &CpOutput) -> f64 {
    if piped {
        cp_box_output(bytes, mode)
    } else {
        TAG_NULL_F64
    }
}

/// Decoded exit disposition of a finished child.
pub(crate) struct CpExit {
    /// Exit code when the child exited normally; `None` when killed by signal.
    pub(crate) code: Option<i32>,
    /// Signal number when the child was killed by a signal (Unix only).
    pub(crate) signal: Option<i32>,
}

pub(crate) fn cp_decode_status(status: &std::process::ExitStatus) -> CpExit {
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        status.signal()
    };
    #[cfg(not(unix))]
    let signal: Option<i32> = None;
    CpExit {
        code: status.code(),
        signal,
    }
}

/// Map a spawn-failure `io::Error` to the Node errno-style `code` string.
pub(crate) fn cp_io_error_code(e: &std::io::Error) -> &'static str {
    use std::io::ErrorKind;
    match e.kind() {
        ErrorKind::NotFound => "ENOENT",
        ErrorKind::PermissionDenied => "EACCES",
        ErrorKind::AlreadyExists => "EEXIST",
        ErrorKind::BrokenPipe => "EPIPE",
        ErrorKind::TimedOut => "ETIMEDOUT",
        ErrorKind::ConnectionRefused => "ECONNREFUSED",
        _ => "UNKNOWN",
    }
}

/// Node's `errno` is the negative libc errno value for the failure code.
pub(crate) fn cp_errno_number(code: &str) -> f64 {
    #[cfg(unix)]
    let n = match code {
        "ENOENT" => libc::ENOENT,
        "EACCES" => libc::EACCES,
        "EEXIST" => libc::EEXIST,
        "EPIPE" => libc::EPIPE,
        "ENOBUFS" => libc::ENOBUFS,
        "ETIMEDOUT" => libc::ETIMEDOUT,
        "ECONNREFUSED" => libc::ECONNREFUSED,
        _ => 0,
    };
    #[cfg(not(unix))]
    let n = 0;
    -(n as f64)
}

/// Build an error-like heap object. `ErrorHeader` rejects dynamic-property
/// writes, so for the rich shape Node attaches we use a regular object whose
/// class extends `Error` (so `instanceof Error` / `typeof` still report
/// error-ish) and set the props by name. Returns a NaN-boxed pointer.
fn cp_make_error_with_class(
    class_id: u32,
    name: &str,
    message: &str,
    extra: &[(&str, f64)],
) -> f64 {
    crate::object::js_register_class_extends_error(class_id);
    let obj = crate::object::js_object_alloc(class_id, (extra.len() + 2) as u32);
    let set = |key: &str, value: f64| {
        let kp = js_string_from_bytes(key.as_ptr(), key.len() as u32);
        js_object_set_field_by_name(obj, kp, value);
    };
    set("name", cp_box_string(name));
    set("message", cp_box_string(message));
    // `name`/`message` are non-enumerable on a Node Error (only the diagnostic
    // props are enumerable), so keep them out of `Object.keys(err)`.
    let attrs = crate::object::PropertyAttrs::new(true, false, true);
    crate::object::set_property_attrs(obj as usize, "name".to_string(), attrs);
    crate::object::set_property_attrs(obj as usize, "message".to_string(), attrs);
    for (k, v) in extra {
        set(k, *v);
    }
    cp_box_ptr(obj as *const u8)
}

pub(crate) fn cp_make_error(message: &str, extra: &[(&str, f64)]) -> f64 {
    cp_make_error_with_class(crate::error::CLASS_ID_ERROR, "Error", message, extra)
}

fn cp_make_range_error(message: &str, extra: &[(&str, f64)]) -> f64 {
    cp_make_error_with_class(
        crate::error::CLASS_ID_RANGE_ERROR,
        "RangeError",
        message,
        extra,
    )
}

pub(crate) fn cp_abort_error(cmd: Option<&str>) -> f64 {
    crate::object::js_register_class_extends_error(CP_ABORT_ERROR_CLASS_ID);
    let obj = crate::object::js_object_alloc(CP_ABORT_ERROR_CLASS_ID, 4);
    let set = |key: &str, value: f64| {
        let kp = js_string_from_bytes(key.as_ptr(), key.len() as u32);
        js_object_set_field_by_name(obj, kp, value);
    };
    set("code", cp_box_string("ABORT_ERR"));
    set("name", cp_box_string("AbortError"));
    set("message", cp_box_string("The operation was aborted"));
    if let Some(cmd) = cmd {
        set("cmd", cp_box_string(cmd));
    }
    let hidden = crate::object::PropertyAttrs::new(true, false, true);
    crate::object::set_property_attrs(obj as usize, "message".to_string(), hidden);
    cp_box_ptr(obj as *const u8)
}

/// `[null, stdout, stderr]` — the Node `output` array shared by spawnSync and
/// the execSync throw error.
pub(crate) fn cp_output_array(stdout: f64, stderr: f64) -> f64 {
    let mut arr = crate::array::js_array_alloc(3);
    arr = crate::array::js_array_push_f64(arr, TAG_NULL_F64);
    arr = crate::array::js_array_push_f64(arr, stdout);
    arr = crate::array::js_array_push_f64(arr, stderr);
    cp_box_ptr(arr as *const u8)
}

/// The `(code, signal, killed)` callback-error fields, matching Node: `code` is
/// the numeric exit code, or the signal name when the child was killed by a
/// signal (and on spawn failure, the errno string); `signal` is the signal name
/// or `null`; `killed` is `true` only when terminated by a signal.
fn cp_error_code_signal(run: &CpRun) -> (f64, f64, f64) {
    if let Some((errno_code, _)) = run.spawn_error {
        return (cp_box_string(errno_code), TAG_NULL_F64, TAG_FALSE_F64);
    }
    match (run.code, run.signal) {
        (_, Some(sig)) => {
            let name = cp_box_string(cp_signal_name(sig));
            (name, name, TAG_TRUE_F64)
        }
        (Some(c), None) => (c as f64, TAG_NULL_F64, TAG_FALSE_F64),
        (None, None) => (TAG_NULL_F64, TAG_NULL_F64, TAG_FALSE_F64),
    }
}

/// Build the `(err, stdout, stderr)` callback error for a failed exec/execFile
/// run — Node attaches `code`/`signal`/`killed`/`cmd` (plus `errno`/`syscall`/
/// `path` on spawn failure). `cmd` is the human-readable command string;
/// `file` is the program actually launched (Node's spawn-failure `syscall`/
/// `path`/message use the file alone, while `.cmd` keeps the display string —
/// `execFile("x", ["a"])` ENOENT reads `syscall: "spawn x"`, `cmd: "x a"`). #1935.
fn cp_exec_callback_error(run: &CpRun, options: &CpRunOptions, cmd: &str, file: &str) -> f64 {
    if let Some((errno_code, _)) = run.spawn_error {
        let syscall = format!("spawn {file}");
        let message = format!("{syscall} {errno_code}");
        return cp_make_error(
            &message,
            &[
                ("code", cp_box_string(errno_code)),
                ("errno", cp_errno_number(errno_code)),
                ("syscall", cp_box_string(&syscall)),
                ("path", cp_box_string(file)),
                ("cmd", cp_box_string(cmd)),
                ("killed", TAG_FALSE_F64),
                ("signal", TAG_NULL_F64),
            ],
        );
    }
    if let Some(run_error) = run.run_error {
        match run_error {
            CpRunError::MaxBuffer => {
                let stream = if run.stdout.len() > options.max_buffer {
                    "stdout"
                } else {
                    "stderr"
                };
                let message = format!("{stream} maxBuffer length exceeded");
                return cp_make_range_error(
                    &message,
                    &[
                        ("code", cp_box_string("ERR_CHILD_PROCESS_STDIO_MAXBUFFER")),
                        ("cmd", cp_box_string(cmd)),
                    ],
                );
            }
            CpRunError::Timeout => {
                let signal = run.signal.map(cp_signal_name).unwrap_or("SIGTERM");
                let message = format!(
                    "Command failed: {cmd}\n{}",
                    String::from_utf8_lossy(&run.stderr)
                );
                return cp_make_error(
                    &message,
                    &[
                        ("code", TAG_NULL_F64),
                        ("killed", TAG_TRUE_F64),
                        ("signal", cp_box_string(signal)),
                        ("cmd", cp_box_string(cmd)),
                    ],
                );
            }
        }
    }
    let (code, signal, killed) = cp_error_code_signal(run);
    // Node's message is `Command failed: <cmd>\n<stderr>`.
    let message = format!(
        "Command failed: {cmd}\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    cp_make_error(
        &message,
        &[
            ("code", code),
            ("killed", killed),
            ("signal", signal),
            ("cmd", cp_box_string(cmd)),
        ],
    )
}

pub(crate) fn cp_exec_callback_output_bytes<'a>(
    run: &'a CpRun,
    options: &CpRunOptions,
) -> (&'a [u8], &'a [u8]) {
    if run.run_error != Some(CpRunError::MaxBuffer) {
        return (&run.stdout, &run.stderr);
    }
    if run.stdout.len() > options.max_buffer {
        let limit = options.max_buffer.min(run.stdout.len());
        return (&run.stdout[..limit], &run.stderr);
    }
    if run.stderr.len() > options.max_buffer {
        let limit = options.max_buffer.min(run.stderr.len());
        return (&run.stdout, &run.stderr[..limit]);
    }
    (&run.stdout, &run.stderr)
}

/// Build the `(err, stdout, stderr)` triple an exec/execFile callback receives
/// from a finished (or failed) run, boxed per `mode`. Shared by the synchronous
/// no-op-callback fast paths and the async reactor (#4912), so a deferred
/// callback is byte-identical to the former immediate one.
pub(crate) fn cp_exec_callback_args(
    run: &CpRun,
    options: &CpRunOptions,
    cmd: &str,
    file: &str,
    mode: &CpOutput,
) -> (f64, f64, f64) {
    let (stdout_bytes, stderr_bytes) = cp_exec_callback_output_bytes(run, options);
    let stdout_box = cp_box_output(stdout_bytes, mode);
    let stderr_box = cp_box_output(stderr_bytes, mode);
    let err_val = if run.success() {
        TAG_NULL_F64
    } else {
        cp_exec_callback_error(run, options, cmd, file)
    };
    (err_val, stdout_box, stderr_box)
}

/// Throw the error Node raises from a failed execSync/execFileSync — carries
/// `status`/`signal`/`pid`/`output`/`stdout`/`stderr`/`cmd`. Diverges. #1938.
pub(crate) fn cp_sync_throw_error(run: &CpRun, cmd: &str, stdout: f64, stderr: f64) -> ! {
    let status = match run.code {
        Some(c) => c as f64,
        None => TAG_NULL_F64,
    };
    let signal = match run.signal {
        Some(s) => cp_box_string(cp_signal_name(s)),
        None => TAG_NULL_F64,
    };
    let pid = match run.pid {
        Some(p) => p as f64,
        None => TAG_NULL_F64,
    };
    let output = cp_output_array(stdout, stderr);
    if let Some(run_error) = run.run_error {
        let code = run_error.code();
        let syscall = format!("spawnSync {cmd}");
        let message = format!("{syscall} {code}");
        let err = cp_make_error(
            &message,
            &[
                ("code", cp_box_string(code)),
                ("errno", cp_errno_number(code)),
                ("syscall", cp_box_string(&syscall)),
                ("status", status),
                ("signal", signal),
                ("output", output),
                ("pid", pid),
                ("stdout", stdout),
                ("stderr", stderr),
            ],
        );
        crate::exception::js_throw(err)
    }
    // Node's execSync/execFileSync error enumerates exactly
    // status/signal/output/pid/stdout/stderr (no `cmd` own prop — that is on the
    // async exec callback error). The command is still surfaced in `message`.
    let message = match &run.spawn_error {
        Some((code, _)) => format!("Command failed: {cmd} {code}"),
        None => format!("Command failed: {cmd}"),
    };
    // Field order matches Node's insertion order (status, signal, output, pid,
    // stdout, stderr) so `Object.keys(err)` is byte-identical.
    let err = cp_make_error(
        &message,
        &[
            ("status", status),
            ("signal", signal),
            ("output", output),
            ("pid", pid),
            ("stdout", stdout),
            ("stderr", stderr),
        ],
    );
    crate::exception::js_throw(err)
}

/// `file arg1 arg2…` — the human-readable command string Node uses for the
/// execFile error `.cmd`.
pub(crate) fn cp_file_cmd_display(file: &str, args: &[String]) -> String {
    if args.is_empty() {
        file.to_string()
    } else {
        format!("{} {}", file, args.join(" "))
    }
}
