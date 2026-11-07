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
/// options object (or undefined/null/non-object for inherited defaults). Node
/// semantics: `env` *replaces* the child's environment wholesale, so when an
/// `env` object is provided we `env_clear()` first and skip keys whose value is
/// `undefined`. #1780.
pub(crate) fn cp_apply_options(command: &mut Command, opts_val: f64) {
    if cp_object_ptr(opts_val).is_none() {
        cp_apply_default_environment(command);
        return;
    }

    if let Some(dir) = cp_value_to_string(cp_get_field(opts_val, b"cwd")) {
        validate::cp_validate_path_no_null_bytes("options.cwd", &dir);
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
                let property = format!("options.env['{key}']");
                validate::cp_validate_no_null_bytes(&property, &key);
                let value = cp_coerce_string(v);
                validate::cp_validate_no_null_bytes(&property, &value);
                command.env(&key, value);
            }
        }
    } else {
        cp_apply_default_environment(command);
    }

    cp_apply_uid_gid(command, opts_val);
}

fn cp_apply_default_environment(command: &mut Command) {
    if let Some(entries) = crate::process::private_environment_snapshot() {
        command.env_clear().envs(entries);
    }
}

pub(crate) fn cp_read_argv0(opts_val: f64) -> Option<String> {
    cp_object_ptr(opts_val)?;
    let argv0 = cp_value_to_string(cp_get_field(opts_val, b"argv0"))?;
    validate::cp_validate_no_null_bytes("options.argv0", &argv0);
    Some(argv0)
}

pub(crate) fn cp_read_abort_signal(opts_val: f64) -> Option<f64> {
    cp_object_ptr(opts_val)?;
    let signal = cp_get_field(opts_val, b"signal");
    if JSValue::from_bits(signal.to_bits()).is_undefined() {
        return None;
    }
    if crate::event_target_hook_slots::abort_signal_ptr_from_value(signal).is_some() {
        return Some(signal);
    }
    let message = format!(
        "The \"options.signal\" property must be an instance of AbortSignal. Received {}",
        crate::fs::validate::describe_received(signal)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

pub(crate) fn cp_abort_signal_is_aborted(signal: f64) -> bool {
    crate::event_target_hook_slots::abort_signal_ptr_from_value(signal)
        .is_some_and(|ptr| crate::event_target_hook_slots::js_abort_signal_is_aborted(ptr) != 0)
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

pub(crate) fn cp_stdio_stream_fd(value: f64, fd_index: usize) -> Option<i32> {
    let expected_stream = match fd_index {
        0 => crate::fs_hook_slots::is_fs_stream_instance_value(value, "ReadStream"),
        1 | 2 => crate::fs_hook_slots::is_fs_stream_instance_value(value, "WriteStream"),
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

#[cfg(not(unix))]
pub(crate) fn cp_stdio_js_value(kind: CpStdio, pipe_obj: f64) -> f64 {
    match kind {
        CpStdio::Pipe => pipe_obj,
        CpStdio::Ignore | CpStdio::Inherit | CpStdio::Fd(_) => TAG_NULL_F64,
    }
}

/// The parent end of a child's stdio pipe slot.
#[cfg(unix)]
pub(crate) type CpParentPipe = std::os::fd::OwnedFd;
#[cfg(not(unix))]
pub(crate) type CpParentPipe = std::fs::File;

/// A connected `AF_UNIX` stream pair, both ends close-on-exec: Node's pipe
/// slots are sockets in the child too (`fstat` on fd 0/1/3 reports a socket).
#[cfg(unix)]
pub(crate) fn cp_socketpair() -> std::io::Result<(std::os::fd::OwnedFd, std::os::fd::OwnedFd)> {
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    use std::os::fd::AsRawFd;
    use std::os::fd::{FromRawFd, OwnedFd};
    let mut pair = [0; 2];
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let kind = libc::SOCK_STREAM | libc::SOCK_CLOEXEC;
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    let kind = libc::SOCK_STREAM;
    // SAFETY: `pair` has room for the two descriptors socketpair writes.
    if unsafe { libc::socketpair(libc::AF_UNIX, kind, 0, pair.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: socketpair succeeded, so both descriptors are open and ours.
    let (parent, child) = unsafe { (OwnedFd::from_raw_fd(pair[0]), OwnedFd::from_raw_fd(pair[1])) };
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    for end in [&parent, &child] {
        // SAFETY: `end` is an open descriptor owned above.
        if unsafe { libc::fcntl(end.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok((parent, child))
}

/// Apply every stdio slot to `command`. Unix: each `'pipe'` slot is a
/// socketpair whose child end becomes the slot's descriptor; the parent ends
/// come back as `(slot, end)` for every pipe slot, for the caller to wrap in
/// `net.Socket`s once the spawn succeeded. Elsewhere 0-2 are `Stdio::piped()`
/// and only the parent ends of pipe slots beyond fd 2 come back.
pub(crate) fn cp_apply_live_stdio(
    command: &mut Command,
    stdio: &[CpStdio],
) -> std::io::Result<Vec<(usize, CpParentPipe)>> {
    let slot = |fd: usize| stdio.get(fd).copied().unwrap_or(CpStdio::Pipe);

    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        use std::os::unix::process::CommandExt;

        let mut parents = Vec::new();
        for fd in 0..stdio.len().max(3) {
            let kind = slot(fd);
            if fd < 3 {
                let child_stdio = match kind {
                    CpStdio::Pipe => {
                        let (parent, child) = cp_socketpair()?;
                        parents.push((fd, parent));
                        Stdio::from(child)
                    }
                    CpStdio::Ignore => Stdio::null(),
                    CpStdio::Inherit => Stdio::inherit(),
                    CpStdio::Fd(source) => cp_stdio_from_fd(source),
                };
                match fd {
                    0 => command.stdin(child_stdio),
                    1 => command.stdout(child_stdio),
                    _ => command.stderr(child_stdio),
                };
                continue;
            }
            // The source descriptor dup2'd onto `fd` in the child. Owned ones
            // move into the hook, so the parent's copy closes with `command`.
            let source: Option<(i32, Option<std::os::fd::OwnedFd>)> = match kind {
                CpStdio::Pipe => {
                    let (parent, child) = cp_socketpair()?;
                    parents.push((fd, parent));
                    Some((child.as_raw_fd(), Some(child)))
                }
                CpStdio::Ignore => {
                    let null = std::fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open("/dev/null")?;
                    let null = std::os::fd::OwnedFd::from(null);
                    Some((null.as_raw_fd(), Some(null)))
                }
                CpStdio::Fd(source) => Some((source, None)),
                CpStdio::Inherit => None,
            };
            if let Some((source, owned)) = source {
                // SAFETY: the hook only calls async-signal-safe dup2/fcntl.
                unsafe {
                    command.pre_exec(move || {
                        let _ = &owned;
                        if libc::dup2(source, fd as i32) < 0
                            || libc::fcntl(fd as i32, libc::F_SETFD, 0) < 0
                        {
                            return Err(std::io::Error::last_os_error());
                        }
                        Ok(())
                    });
                }
            }
        }
        return Ok(parents);
    }

    #[cfg(not(unix))]
    {
        let to_stdio = |kind: CpStdio| match kind {
            CpStdio::Pipe => Stdio::piped(),
            CpStdio::Ignore => Stdio::null(),
            CpStdio::Inherit => Stdio::inherit(),
            CpStdio::Fd(fd) => cp_stdio_from_fd(fd),
        };
        command.stdin(to_stdio(slot(0)));
        command.stdout(to_stdio(slot(1)));
        command.stderr(to_stdio(slot(2)));
    }

    #[cfg(not(unix))]
    {
        Ok(Vec::new())
    }
}

#[cfg(unix)]
pub(crate) fn cp_stdio_from_fd(fd: i32) -> Stdio {
    use std::os::fd::FromRawFd;

    if let Some(file) = crate::fs_hook_slots::try_clone_registered_fd(fd) {
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
    crate::process::effective_environment_var("PATH").unwrap_or_else(|| CP_DEFAULT_PATH.to_string())
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
    cp_require_process_support();
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
    let is_self = crate::process::process_args_lossy().next().as_deref() == Some(cmd)
        || crate::process::process_exec_path_string() == cmd;
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

pub(super) fn cp_shell_command(line: &str, opts_val: f64) -> Command {
    let shell = cp_get_field(opts_val, b"shell");
    let shell_bin = if JSValue::from_bits(shell.to_bits()).is_any_string() {
        cp_value_to_string(shell).unwrap_or_else(cp_default_shell)
    } else {
        cp_default_shell()
    };
    validate::cp_validate_no_null_bytes("options.shell", &shell_bin);
    let mut command = cp_command_for_program(&shell_bin, opts_val);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let name = std::path::Path::new(&shell_bin)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        if name.eq_ignore_ascii_case("cmd") || name.eq_ignore_ascii_case("cmd.exe") {
            command
                .arg("/d")
                .arg("/s")
                .arg("/c")
                .raw_arg(format!("\"{}\"", line));
        } else {
            command.arg("-c").arg(line);
        }
    }
    #[cfg(not(windows))]
    command.arg("-c").arg(line);
    command
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
        let line = std::iter::once(program.as_str())
            .chain(args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ");
        cp_shell_command(&line, opts_val)
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

    #[cfg(unix)]
    #[test]
    fn a_pipe_slot_is_a_close_on_exec_socket_pair() {
        use std::io::{Read, Write};
        use std::os::fd::AsRawFd;
        let (parent, child) = super::cp_socketpair().expect("socketpair");
        for end in [&parent, &child] {
            // SAFETY: `end` is an open descriptor and `stat` is plain old data.
            let mut stat: libc::stat = unsafe { std::mem::zeroed() };
            assert_eq!(unsafe { libc::fstat(end.as_raw_fd(), &mut stat) }, 0);
            assert_eq!(stat.st_mode & libc::S_IFMT, libc::S_IFSOCK);
            // SAFETY: F_GETFD on an open descriptor.
            let flags = unsafe { libc::fcntl(end.as_raw_fd(), libc::F_GETFD) };
            assert_ne!(flags & libc::FD_CLOEXEC, 0, "both ends close on exec");
        }
        let mut parent = std::fs::File::from(parent);
        let mut child = std::fs::File::from(child);
        child.write_all(b"up").unwrap();
        parent.write_all(b"down").unwrap();
        let mut buf = [0; 4];
        parent.read_exact(&mut buf[..2]).unwrap();
        assert_eq!(&buf[..2], b"up");
        child.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"down");
    }

    #[cfg(unix)]
    #[test]
    fn every_pipe_slot_reaches_the_child_as_a_socket() {
        use super::{cp_apply_live_stdio, CpStdio};
        use std::io::Read;
        let mut command = std::process::Command::new("/bin/sh");
        command.args([
            "-c",
            "for fd in 0 1 2 3; do [ -S /dev/fd/$fd ] || exit $((10 + fd)); done; printf ok >&3",
        ]);
        let parents = cp_apply_live_stdio(&mut command, &[CpStdio::Pipe; 4]).expect("stdio");
        assert_eq!(
            parents.iter().map(|(fd, _)| *fd).collect::<Vec<_>>(),
            [0, 1, 2, 3],
            "one parent end per pipe slot, stdio included"
        );
        let mut child = command.spawn().expect("spawn");
        // The child ends live in `command`; drop them so slot 3 reads to EOF.
        drop(command);
        let mut out = String::new();
        let slot3 = parents.into_iter().find(|(fd, _)| *fd == 3).unwrap().1;
        std::fs::File::from(slot3).read_to_string(&mut out).unwrap();
        assert_eq!(child.wait().unwrap().code(), Some(0));
        assert_eq!(out, "ok");
    }

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

#[cfg(test)]
mod environment_tests {
    use super::*;
    use crate::process::WorkerProcessState;
    use std::ffi::OsStr;

    #[test]
    fn private_worker_default_child_environment_uses_native_backing() {
        let state =
            WorkerProcessState::capture(Some(&[("PERRY_CHILD_PRIVATE".into(), "private".into())]));
        std::thread::spawn(move || {
            state.install();
            let scope = crate::gc::RuntimeHandleScope::new();
            let options =
                scope.root_nanbox_f64(cp_box_ptr(crate::object::js_object_alloc(0, 0).cast()));
            for options in [cp_undefined(), options.get_nanbox_f64()] {
                let command = cp_build_command("/usr/bin/env", &[], options);
                assert_eq!(
                    command.get_envs().collect::<Vec<_>>(),
                    [(
                        OsStr::new("PERRY_CHILD_PRIVATE"),
                        Some(OsStr::new("private"))
                    )]
                );
            }
        })
        .join()
        .unwrap();
    }

    #[test]
    fn main_child_defaults_preserve_raw_os_inheritance() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let options =
            scope.root_nanbox_f64(cp_box_ptr(crate::object::js_object_alloc(0, 0).cast()));
        for options in [cp_undefined(), options.get_nanbox_f64()] {
            let command = cp_build_command("native-child", &[], options);
            assert_eq!(
                command.get_envs().count(),
                0,
                "OS defaults must not become a lossy string replacement"
            );
        }
    }

    fn explicit_options(scope: &crate::gc::RuntimeHandleScope) -> f64 {
        let env = scope.root_nanbox_f64(cp_box_ptr(crate::object::js_object_alloc(0, 0).cast()));
        let value = cp_box_string("explicit");
        cp_set_field(env.get_nanbox_f64(), b"ONLY", value);
        cp_set_field(env.get_nanbox_f64(), b"OMITTED", cp_undefined());
        cp_set_field(env.get_nanbox_f64(), b"BOOL", TAG_FALSE_F64);
        let options =
            scope.root_nanbox_f64(cp_box_ptr(crate::object::js_object_alloc(0, 0).cast()));
        cp_set_field(options.get_nanbox_f64(), b"env", env.get_nanbox_f64());
        options.get_nanbox_f64()
    }

    #[test]
    fn explicit_child_environment_replaces_private_defaults() {
        let state = WorkerProcessState::capture(Some(&[("PRIVATE".into(), "parent".into())]));
        std::thread::spawn(move || {
            state.install();
            let scope = crate::gc::RuntimeHandleScope::new();
            let options = scope.root_nanbox_f64(explicit_options(&scope));
            let command = cp_build_command("native-child", &[], options.get_nanbox_f64());
            assert_eq!(
                command.get_envs().collect::<Vec<_>>(),
                [
                    (OsStr::new("BOOL"), Some(OsStr::new("false"))),
                    (OsStr::new("ONLY"), Some(OsStr::new("explicit"))),
                ]
            );
        })
        .join()
        .unwrap();
    }

    #[test]
    fn fork_bootstrap_fields_overlay_the_selected_environment() {
        let state = WorkerProcessState::capture(Some(&[
            ("PRIVATE".into(), "parent".into()),
            ("NODE_CHANNEL_FD".into(), "old".into()),
        ]));
        std::thread::spawn(move || {
            state.install();
            let scope = crate::gc::RuntimeHandleScope::new();
            let options = scope.root_nanbox_f64(explicit_options(&scope));
            for (options, base_key) in [
                (cp_undefined(), "PRIVATE"),
                (options.get_nanbox_f64(), "ONLY"),
            ] {
                let mut command = Command::new("native-child");
                crate::child_process::fork::cp_apply_fork_environment(
                    &mut command,
                    options,
                    7,
                    true,
                    Some("[]"),
                );
                let environment: std::collections::BTreeMap<_, _> = command.get_envs().collect();
                assert!(environment.contains_key(OsStr::new(base_key)));
                assert_eq!(
                    environment.get(OsStr::new("NODE_CHANNEL_FD")),
                    Some(&Some(OsStr::new("7")))
                );
                assert_eq!(
                    environment.get(OsStr::new("NODE_CHANNEL_SERIALIZATION_MODE")),
                    Some(&Some(OsStr::new("advanced")))
                );
                assert_eq!(
                    environment.get(OsStr::new("PERRY_PROCESS_EXEC_ARGV")),
                    Some(&Some(OsStr::new("[]")))
                );
            }
        })
        .join()
        .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn private_worker_default_path_resolves_its_own_executable() {
        let directory =
            std::env::temp_dir().join(format!("perry-child-private-path-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let executable = directory.join("perry-private-shell");
        std::os::unix::fs::symlink("/bin/sh", &executable).unwrap();
        let state = WorkerProcessState::capture(Some(&[(
            "PATH".into(),
            directory.to_str().unwrap().into(),
        )]));
        std::thread::spawn(move || {
            state.install();
            let command = cp_command_for_program("perry-private-shell", cp_undefined());
            assert_eq!(command.get_program(), executable.as_os_str());
        })
        .join()
        .unwrap();
    }
}
