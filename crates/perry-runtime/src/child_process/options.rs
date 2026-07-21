use super::*;

use std::process::{Command, Stdio};

use crate::value::JSValue;

pub(crate) fn cp_read_uid_gid_option(opts_val: f64, key: &[u8]) -> Option<u32> {
    let value = cp_get_field(opts_val, key);
    let js_value = JSValue::from_bits(value.to_bits());
    if js_value.is_undefined() || js_value.is_null() {
        return None;
    }
    if !js_value.is_number() && !js_value.is_int32() {
        return None;
    }
    let id = js_value.to_number();
    if id.is_finite() && id >= 0.0 && id.fract() == 0.0 && id <= u32::MAX as f64 {
        Some(id as u32)
    } else {
        None
    }
}

pub(crate) fn cp_apply_uid_gid(command: &mut Command, opts_val: f64) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        if let Some(gid) = cp_read_uid_gid_option(opts_val, b"gid") {
            command.gid(gid);
        }
        if let Some(uid) = cp_read_uid_gid_option(opts_val, b"uid") {
            command.uid(uid);
        }
    }

    #[cfg(not(unix))]
    {
        let _ = (command, opts_val);
    }
}

/// Apply shared command options to `command`. `cwd` and `env` are portable;
/// `uid` and `gid` are applied on Unix targets. `opts_val` is a NaN-boxed
/// options object (or undefined/null/non-object — then a no-op). Node
/// semantics: `env` *replaces* the child's environment wholesale, so when an
/// `env` object is provided we `env_clear()` first and skip keys whose value is
/// `undefined`. #1780.
pub(crate) fn cp_apply_options(command: &mut Command, opts_val: f64) {
    if cp_object_ptr(opts_val).is_none() {
        return;
    }

    if let Some(dir) = cp_value_to_string(cp_get_field(opts_val, b"cwd")) {
        if !dir.is_empty() {
            command.current_dir(dir);
        }
    }

    let env_val = cp_get_field(opts_val, b"env");
    if let Some(env_obj) = cp_object_ptr(env_val) {
        command.env_clear();
        let keys = crate::object::js_object_keys(env_obj);
        if !keys.is_null() {
            let n = crate::array::js_array_length(keys);
            for i in 0..n {
                let key = match cp_value_to_string(crate::array::js_array_get_f64(keys, i)) {
                    Some(k) => k,
                    None => continue,
                };
                let v = cp_get_field(env_val, key.as_bytes());
                if JSValue::from_bits(v.to_bits()).is_undefined() {
                    continue; // Node omits keys whose value is `undefined`.
                }
                command.env(&key, cp_coerce_string(v));
            }
        }
    }

    cp_apply_uid_gid(command, opts_val);
}

pub(crate) fn cp_read_argv0(opts_val: f64) -> Option<String> {
    cp_object_ptr(opts_val)?;
    cp_value_to_string(cp_get_field(opts_val, b"argv0"))
}

pub(crate) fn cp_read_abort_signal(opts_val: f64) -> Option<f64> {
    cp_object_ptr(opts_val)?;
    let signal = cp_get_field(opts_val, b"signal");
    if JSValue::from_bits(signal.to_bits()).is_undefined() {
        return None;
    }
    if crate::url::abort::abort_signal_ptr_from_value(signal).is_some() {
        return Some(signal);
    }
    let message = format!(
        "The \"options.signal\" property must be an instance of AbortSignal. Received {}",
        crate::fs::validate::describe_received(signal)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

pub(crate) fn cp_abort_signal_is_aborted(signal: f64) -> bool {
    crate::url::abort::abort_signal_ptr_from_value(signal)
        .is_some_and(|ptr| crate::url::js_abort_signal_is_aborted(ptr) != 0)
}

pub(crate) fn cp_spawnargs_argv0(default: &str, opts_val: f64) -> String {
    cp_read_argv0(opts_val).unwrap_or_else(|| default.to_string())
}

pub(crate) fn cp_apply_argv0(command: &mut Command, opts_val: f64) {
    let Some(argv0) = cp_read_argv0(opts_val) else {
        return;
    };
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.arg0(argv0);
    }
    #[cfg(not(unix))]
    {
        let _ = (command, argv0);
    }
}

fn cp_option_detached(opts_val: f64) -> bool {
    if cp_object_ptr(opts_val).is_none() {
        return false;
    }
    cp_get_field(opts_val, b"detached").to_bits() == TAG_TRUE_F64.to_bits()
}

pub(crate) fn cp_apply_detached(command: &mut Command, opts_val: f64) {
    if !cp_option_detached(opts_val) {
        return;
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x00000008 | 0x00000200);
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = command;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CpStdio {
    Pipe,
    Ignore,
    Inherit,
    Fd(i32),
}

fn cp_stdio_number_fd(value: f64) -> Option<i32> {
    let js_value = JSValue::from_bits(value.to_bits());
    if js_value.is_int32() {
        Some(js_value.as_int32())
    } else if js_value.is_number() {
        let n = js_value.as_number();
        if n.is_finite() && n >= 0.0 && n.fract() == 0.0 && n <= i32::MAX as f64 {
            Some(n as i32)
        } else {
            None
        }
    } else {
        None
    }
}

fn cp_stdio_stream_fd(value: f64, fd_index: usize) -> Option<i32> {
    let expected_stream = match fd_index {
        0 => crate::fs::is_fs_stream_instance_value(value, "ReadStream"),
        1 | 2 => crate::fs::is_fs_stream_instance_value(value, "WriteStream"),
        _ => false,
    };
    if !expected_stream {
        return None;
    }
    let fd = cp_get_field(value, b"fd");
    cp_stdio_number_fd(fd).filter(|fd| crate::fs::fd_is_registered(*fd))
}

fn cp_stdio_kind(value: f64, fd_index: usize) -> CpStdio {
    if let Some(fd) = cp_stdio_number_fd(value) {
        return CpStdio::Fd(fd);
    }
    if let Some(fd) = cp_stdio_stream_fd(value, fd_index) {
        return CpStdio::Fd(fd);
    }

    match cp_value_to_string(value).as_deref() {
        Some("ignore") => CpStdio::Ignore,
        Some("inherit") => CpStdio::Inherit,
        _ => CpStdio::Pipe,
    }
}

/// Read the deterministic live-stdio subset: `pipe` (default), `ignore`,
/// `inherit`, numeric fd entries, and opened fs stream objects backed by a
/// registered fd.
pub(crate) fn cp_read_stdio(opts_val: f64, fds: usize) -> Vec<CpStdio> {
    let mut out = vec![CpStdio::Pipe; fds];
    if cp_object_ptr(opts_val).is_none() {
        return out;
    }

    let stdio = cp_get_field(opts_val, b"stdio");
    if let Some(arr) = cp_array_ptr(stdio) {
        let n = crate::array::js_array_length(arr).min(fds as u32);
        for i in 0..n {
            out[i as usize] = cp_stdio_kind(crate::array::js_array_get_f64(arr, i), i as usize);
        }
        return out;
    }

    if let Some(s) = cp_value_to_string(stdio) {
        match s.as_str() {
            "ignore" => out.fill(CpStdio::Ignore),
            "inherit" => out.fill(CpStdio::Inherit),
            _ => {}
        }
        return out;
    }
    out
}

pub(crate) fn cp_stdio_js_value(kind: CpStdio, pipe_obj: f64) -> f64 {
    match kind {
        CpStdio::Pipe => pipe_obj,
        CpStdio::Ignore | CpStdio::Inherit | CpStdio::Fd(_) => TAG_NULL_F64,
    }
}

pub(crate) fn cp_apply_live_stdio(command: &mut Command, stdio: &[CpStdio]) {
    let to_stdio = |kind: CpStdio| match kind {
        CpStdio::Pipe => Stdio::piped(),
        CpStdio::Ignore => Stdio::null(),
        CpStdio::Inherit => Stdio::inherit(),
        CpStdio::Fd(fd) => cp_stdio_from_fd(fd),
    };
    command.stdin(to_stdio(stdio.first().copied().unwrap_or(CpStdio::Pipe)));
    command.stdout(to_stdio(stdio.get(1).copied().unwrap_or(CpStdio::Pipe)));
    command.stderr(to_stdio(stdio.get(2).copied().unwrap_or(CpStdio::Pipe)));
}

#[cfg(unix)]
pub(crate) fn cp_stdio_from_fd(fd: i32) -> Stdio {
    use std::os::fd::FromRawFd;

    if let Some(file) = crate::fs::try_clone_registered_fd(fd) {
        return Stdio::from(file);
    }

    let dup_fd = unsafe { libc::dup(fd) };
    if dup_fd < 0 {
        return Stdio::null();
    }
    unsafe { Stdio::from_raw_fd(dup_fd) }
}

#[cfg(not(unix))]
pub(crate) fn cp_stdio_from_fd(_fd: i32) -> Stdio {
    Stdio::null()
}

/// Default shell for `{ shell: true }` (`shell: "<path>"` overrides it).
fn cp_default_shell() -> String {
    #[cfg(windows)]
    {
        std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".to_string())
    }
    #[cfg(not(windows))]
    {
        "/bin/sh".to_string()
    }
}

/// Build a `Command` for `spawn(cmd, args, opts)`, honoring the `shell` option
/// (Node joins `cmd` + `args` into a single line passed to `<shell> -c`) and
/// then applying `cwd`/`env`. With no `shell` the file is run directly. #1780.
pub(crate) fn cp_build_command(cmd: &str, args: &[String], opts_val: f64) -> Command {
    let shell = if cp_object_ptr(opts_val).is_some() {
        cp_get_field(opts_val, b"shell")
    } else {
        cp_undefined()
    };

    let mut command = if crate::value::js_is_truthy(shell) != 0 {
        // `shell: "<path>"` picks the binary; `shell: true` uses the default.
        let shell_bin = match cp_value_to_string(shell) {
            Some(s) if !s.is_empty() => s,
            _ => cp_default_shell(),
        };
        let mut line = String::from(cmd);
        for a in args {
            line.push(' ');
            line.push_str(a);
        }
        let mut c = Command::new(shell_bin);
        #[cfg(windows)]
        c.arg("/d").arg("/s").arg("/c").arg(line);
        #[cfg(not(windows))]
        c.arg("-c").arg(line);
        c
    } else {
        let mut c = Command::new(cmd);
        c.args(args);
        c
    };

    cp_apply_argv0(&mut command, opts_val);
    cp_apply_options(&mut command, opts_val);
    cp_apply_detached(&mut command, opts_val);
    command
}
