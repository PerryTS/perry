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
        // NOTE: a `pre_exec` closure forces std onto the `fork`+`exec` path
        // (posix_spawn cannot run arbitrary code), so `detached` children do not
        // benefit from the posix_spawn fork/dyld-deadlock fix. `setsid` is not
        // expressible through std's posix_spawn wrapper (no `POSIX_SPAWN_SETSID`
        // knob), and `detached` is a rare, deliberate full-session-detach —
        // unlike the common `exec`/`spawn` paths, it is not converted here.
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

/// Fallback search path when a child's environment carries no `PATH` — the same
/// default `execvp(3)` uses (`_PATH_DEFPATH`).
#[cfg(unix)]
const CP_DEFAULT_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

/// The child's effective `PATH` for resolving a bare command name. When an `env`
/// option is present the child's environment *replaces* the parent's (Node
/// semantics), so its `PATH` — not the parent's — governs the lookup; a missing
/// `PATH` falls back to the `execvp` default. With no `env` option the parent's
/// `PATH` applies.
#[cfg(unix)]
fn cp_effective_path(opts_val: f64) -> String {
    if cp_object_ptr(opts_val).is_some() {
        let env_val = cp_get_field(opts_val, b"env");
        if cp_object_ptr(env_val).is_some() {
            if let Some(p) = cp_value_to_string(cp_get_field(env_val, b"PATH")) {
                if !p.is_empty() {
                    return p;
                }
            }
            return CP_DEFAULT_PATH.to_string();
        }
    }
    std::env::var("PATH").unwrap_or_else(|_| CP_DEFAULT_PATH.to_string())
}

/// Whether `path` names an executable regular file (following symlinks).
#[cfg(unix)]
fn cp_is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && (m.permissions().mode() & 0o111 != 0))
        .unwrap_or(false)
}

/// Resolve a bare command name to an absolute path by walking `path` (a
/// colon-separated `PATH`), returning the first executable match. An empty
/// `PATH` element means the current directory (POSIX `execvp` semantics).
#[cfg(unix)]
pub(crate) fn cp_resolve_program_path(program: &str, path: &str) -> Option<String> {
    for dir in path.split(':') {
        let base = if dir.is_empty() { "." } else { dir };
        let candidate = std::path::Path::new(base).join(program);
        if cp_is_executable(&candidate) {
            return candidate.into_os_string().into_string().ok();
        }
    }
    None
}

/// Build a `Command` for `program`, resolving a bare command name to its
/// absolute path in the child's effective `PATH`.
///
/// This is the macOS fork/dyld deadlock fix. `std::process::Command` uses
/// `posix_spawn` only when the program is given as a path *and* no
/// `pre_exec`/uid/gid closures are set; a bare command name combined with an
/// `env` option (which triggers `env_clear()`) drops it onto the `fork`+`exec`
/// fallback (see `library/std/src/sys/pal/unix/process/process_unix.rs`,
/// `env_saw_path() && !program_is_path()`). On macOS a `fork` from Perry's
/// multithreaded runtime (async reactor + GC/worker threads) can deadlock the
/// child post-`exec` in dyld (`RemoteNotificationResponder::
/// blockOnSynchronousEvent`) when the process is being observed by a Mach
/// notification port (telemetry, a crash reporter, a debugger): the child
/// inherits locks/Mach state from parent threads that don't exist after
/// `fork`. Resolving the name to an absolute path here keeps std on the
/// `posix_spawn` fast path.
///
/// The original name is preserved as `argv[0]` (`arg0`) so the child sees the
/// same `process.argv[0]` it would have gotten from the bare name. When nothing
/// resolves we fall back to the bare name unchanged — a genuine `ENOENT` never
/// `exec`s a real image, so it cannot hit the dyld hang, and the error surface
/// stays identical.
pub(crate) fn cp_command_for_program(program: &str, opts_val: f64) -> Command {
    #[cfg(unix)]
    {
        if !program.is_empty() && !program.contains('/') {
            let path = cp_effective_path(opts_val);
            if let Some(abs) = cp_resolve_program_path(program, &path) {
                use std::os::unix::process::CommandExt;
                let mut command = Command::new(abs);
                command.arg0(program);
                return command;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = opts_val;
    }
    Command::new(program)
}

/// Whether a self-launch uses a Node CLI mode that evaluates source text.
fn cp_should_use_node_interpreter(cmd: &str, args: &[String]) -> bool {
    let is_self = std::env::args().next().as_deref() == Some(cmd)
        || std::env::current_exe().is_ok_and(|current| current == std::path::Path::new(cmd));
    is_self
        && args
            .iter()
            .take_while(|arg| arg.as_str() != "--" && arg.starts_with('-'))
            .any(|arg| {
                matches!(arg.as_str(), "-e" | "--eval" | "-p" | "--print")
                    || arg.starts_with("--eval=")
                    || arg.starts_with("--print=")
            })
}

/// Node interpreter used for source-evaluating self-launches.
fn cp_default_node_interpreter() -> String {
    std::env::var("PERRY_FORK_EXECPATH")
        .ok()
        .filter(|path| !path.is_empty())
        .unwrap_or_else(|| "node".to_string())
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

    // A compiled Perry program is not a Node CLI, so relaunching itself with
    // `-e` would rerun its AOT entry point. Use the same configurable Node
    // interpreter as `fork()` for any eval source passed through execPath.
    let program = if cp_should_use_node_interpreter(cmd, args) {
        cp_default_node_interpreter()
    } else {
        cmd.to_string()
    };

    let mut command = if crate::value::js_is_truthy(shell) != 0 {
        // `shell: "<path>"` picks the binary; `shell: true` uses the default.
        let shell_bin = match cp_value_to_string(shell) {
            Some(s) if !s.is_empty() => s,
            _ => cp_default_shell(),
        };
        let mut line = program.clone();
        for a in args {
            line.push(' ');
            line.push_str(a);
        }
        // Resolve a bare shell name to an absolute path so std stays on
        // `posix_spawn` (see `cp_command_for_program`).
        let mut c = cp_command_for_program(&shell_bin, opts_val);
        #[cfg(windows)]
        c.arg("/d").arg("/s").arg("/c").arg(line);
        #[cfg(not(windows))]
        c.arg("-c").arg(line);
        c
    } else {
        let mut c = cp_command_for_program(&program, opts_val);
        c.args(args);
        c
    };

    cp_apply_argv0(&mut command, opts_val);
    cp_apply_options(&mut command, opts_val);
    cp_apply_detached(&mut command, opts_val);
    command
}

#[cfg(all(test, unix))]
mod posix_spawn_tests {
    use super::{cp_command_for_program, cp_resolve_program_path};
    use crate::child_process::cp_undefined;

    /// A bare command name in a real `PATH` resolves to an executable absolute
    /// path — the precondition std needs to pick `posix_spawn` over `fork`.
    #[test]
    fn resolves_bare_name_to_absolute_executable() {
        let resolved = cp_resolve_program_path("sh", "/nonexistent:/bin:/usr/bin")
            .expect("sh should resolve on a POSIX system");
        assert!(
            resolved.starts_with('/'),
            "expected an absolute path, got {resolved}"
        );
        assert!(resolved.ends_with("/sh"));
        assert!(std::path::Path::new(&resolved).exists());
    }

    /// A name that cannot be found returns `None` (caller falls back to the bare
    /// name, which then fails ENOENT before exec'ing any real image).
    #[test]
    fn missing_program_does_not_resolve() {
        assert!(cp_resolve_program_path("perry-definitely-missing-xyz", "/bin:/usr/bin").is_none());
    }

    /// `cp_command_for_program` rewrites a bare name to an absolute path so std
    /// stays on `posix_spawn`; an already-absolute program is passed through
    /// unchanged.
    #[test]
    fn command_program_is_absolute_for_bare_name() {
        let cmd = cp_command_for_program("sh", cp_undefined());
        let program = cmd.get_program().to_string_lossy().into_owned();
        assert!(
            program.starts_with('/') && program.ends_with("/sh"),
            "bare name should resolve to an absolute path; got {program}"
        );

        let passthrough = cp_command_for_program("/bin/sh", cp_undefined());
        assert_eq!(passthrough.get_program().to_string_lossy(), "/bin/sh");
    }

    /// End-to-end: the resolved (absolute-path, no-`pre_exec`) command spawns via
    /// std's `posix_spawn` path and captures output correctly. This is exactly
    /// the shape that deadlocked in dyld when std took the `fork`+`exec` fallback
    /// on macOS. (Full GC-stress N/N verification uses the compiled repro under
    /// `PERRY_GC_FORCE_EVACUATE=1`; a raw unit test cannot drive Perry's
    /// thread-local GC without runtime init.)
    #[test]
    fn resolved_command_runs_and_captures_output() {
        let mut cmd = cp_command_for_program("sh", cp_undefined());
        assert!(
            cmd.get_program().to_string_lossy().starts_with('/'),
            "resolved program must be an absolute path to keep std on posix_spawn"
        );
        let out = cmd
            .arg("-c")
            .arg("printf ok-%s 42")
            .output()
            .expect("spawn resolved sh");
        assert!(out.status.success());
        assert_eq!(out.stdout, b"ok-42");
    }
}

#[cfg(test)]
mod tests {
    use super::cp_should_use_node_interpreter;

    #[test]
    fn self_exec_node_cli_eval_modes_use_node_interpreter() {
        let current = std::env::current_exe().expect("current executable");
        let current = current.to_string_lossy();

        assert!(cp_should_use_node_interpreter(
            &current,
            &["-e".to_string(), "console.log(42)".to_string()],
        ));
        assert!(cp_should_use_node_interpreter(
            &current,
            &["--eval=console.log(43)".to_string()],
        ));
        assert!(cp_should_use_node_interpreter(
            &current,
            &[
                "--no-warnings".to_string(),
                "--eval".to_string(),
                "console.log(44)".to_string(),
            ],
        ));
        for flag in ["-p", "--print"] {
            assert!(cp_should_use_node_interpreter(
                &current,
                &[flag.to_string(), "40 + 2".to_string()],
            ));
        }
        assert!(cp_should_use_node_interpreter(
            &current,
            &["--print=40 + 2".to_string()],
        ));
        assert!(!cp_should_use_node_interpreter(
            &current,
            &["ordinary-argument".to_string()],
        ));
        assert!(!cp_should_use_node_interpreter(
            "some-other-program",
            &["-e".to_string(), "console.log(45)".to_string()],
        ));
    }
}
