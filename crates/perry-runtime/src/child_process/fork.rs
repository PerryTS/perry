//! `child_process.fork(modulePath[, args][, options])` + IPC channel — #1933.
//!
//! `fork` is `spawn` plus a duplex IPC channel. Perry compiles ahead-of-time and
//! has no embedded interpreter to "fork into", so — like Node, whose `fork`
//! launches `process.execPath` (the `node` binary) on the module — we launch a
//! configurable interpreter (`options.execPath`, else `$PERRY_FORK_EXECPATH`,
//! else `node`) on `modulePath`. The IPC channel is a `socketpair(2)`: the
//! parent keeps one end; the child inherits the other as fd 3 with
//! `NODE_CHANNEL_FD=3` (Node's convention), so a Node child's
//! `process.send` / `process.on('message')` interoperate out of the box.
//!
//! The returned ChildProcess reuses the #1934 reactor for its lifecycle
//! (`spawn`/`data`/`end`/`exit`/`close`, live `kill`) and adds
//! `send` / `disconnect` / `connected` / `channel` plus `'message'` delivery.
//! Messages are newline-delimited JSON (Node's default `'json'` IPC
//! serialization). The IPC wiring is Unix-only; on other platforms `fork`
//! launches the child but reports `connected: false`.

use super::*;
use std::process::Command;
use std::time::Duration;

/// `child_process.fork(modulePath[, args][, options])`. `module_val` stays
/// raw string/array pointers; `opts_ptr` is a raw heap pointer (or 0). The
/// codegen boundary string-coerces a URL module path before calling this.
#[no_mangle]
pub extern "C" fn js_child_process_fork(module_ptr: i64, args_ptr: i64, opts_ptr: i64) -> f64 {
    cp_register_arities();

    reactor::cp_register_reactor_arities();

    let module_raw = unsafe { cp_read_string_header(module_ptr) };
    if module_raw.contains('\0') {
        crate::fs::validate::throw_type_error_with_code(
            "The argument must not contain null bytes",
            "ERR_INVALID_ARG_VALUE",
        );
    }
    let module = if module_raw.starts_with("file:") {
        crate::url::node_compat::module_base_to_path(cp_box_string(&module_raw))
            .unwrap_or(module_raw)
    } else {
        module_raw
    };
    let arg_strs = unsafe { cp_read_arg_strings(args_ptr) };
    let opts_val = cp_options_from_raw_args(args_ptr, opts_ptr);
    let abort_signal = cp_read_abort_signal(opts_val);

    // Launch interpreter: options.execPath → $PERRY_FORK_EXECPATH → "node".
    let exec_path = cp_value_to_string(cp_get_field(opts_val, b"execPath"))
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("PERRY_FORK_EXECPATH")
                .ok()
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "node".to_string());

    // serialization: 'advanced' switches the IPC channel from newline JSON to
    // V8 structured-clone framing (#2130). Node reads NODE_CHANNEL_FD and
    // NODE_CHANNEL_SERIALIZATION_MODE during bootstrap (then deletes them from
    // process.env), so a node child honors the env var below.
    let advanced = cp_value_to_string(cp_get_field(opts_val, b"serialization"))
        .map(|s| s == "advanced")
        .unwrap_or(false);

    // execArgv (defaults to `--experimental-strip-types` for a `.ts` module
    // under node, so TS workers run without extra config).
    let mut exec_argv = cp_args_from_value(cp_get_field(opts_val, b"execArgv"));
    if exec_argv.is_empty() && module.ends_with(".ts") && exec_path.contains("node") {
        exec_argv.push("--experimental-strip-types".to_string());
    }

    // ChildProcess object: EventEmitter + stdio sub-objects + send/disconnect.
    let stdout_obj = cp_build_readable();
    let stderr_obj = cp_build_readable();
    let stdin_obj = cp_build_writable();
    let stdio_field = cp_get_field(opts_val, b"stdio");
    let ipc_fd = cp_array_ptr(stdio_field)
        .and_then(|stdio| {
            (0..crate::array::js_array_length(stdio)).find(|&fd| {
                cp_value_to_string(crate::array::js_array_get_f64(stdio, fd)).as_deref()
                    == Some("ipc")
            })
        })
        .unwrap_or(3) as usize;
    let stdio_count = cp_array_ptr(stdio_field)
        .map(|stdio| crate::array::js_array_length(stdio) as usize)
        .unwrap_or(4)
        .max(4);
    let mut stdio_kinds = cp_read_stdio(opts_val, stdio_count);
    // The IPC socket is installed by fork_launch after live stdio setup.
    stdio_kinds[ipc_fd] = CpStdio::Ignore;
    // Node fork semantics: `silent: false` (cluster.fork's default) inherits
    // stdin/stdout/stderr from the parent; `silent: true` pipes them. Only an
    // explicit `silent: false` overrides — an absent `silent` keeps Perry's
    // historical pipe default, and an explicit `stdio` option wins (#4914).
    if crate::value::JSValue::from_bits(stdio_field.to_bits()).is_undefined()
        && cp_get_field(opts_val, b"silent").to_bits() == crate::value::TAG_FALSE
    {
        stdio_kinds.fill(CpStdio::Inherit);
    }
    let timeout = cp_read_timeout(opts_val);
    let kill_signal = cp_read_kill_signal(opts_val);

    // spawnargs = [argv0 ?? execPath, ...execArgv, module, ...args] (matches Node).
    let mut spawnargs = crate::array::js_array_alloc((arg_strs.len() + exec_argv.len() + 2) as u32);
    let argv0 = cp_spawnargs_argv0(&exec_path, opts_val);
    spawnargs = crate::array::js_array_push_f64(spawnargs, cp_box_string(&argv0));
    for a in &exec_argv {
        spawnargs = crate::array::js_array_push_f64(spawnargs, cp_box_string(a));
    }
    spawnargs = crate::array::js_array_push_f64(spawnargs, cp_box_string(&module));
    for a in &arg_strs {
        spawnargs = crate::array::js_array_push_f64(spawnargs, cp_box_string(a));
    }

    let cp_methods: [(&str, CpFn); 13] = [
        ("on", cp_cast2(cp_method_on)),
        ("once", cp_cast2(cp_method_on)),
        ("addListener", cp_cast2(cp_method_on)),
        ("prependListener", cp_cast2(cp_method_on)),
        ("removeListener", cp_cast2(cp_method_remove_listener)),
        ("off", cp_cast2(cp_method_remove_listener)),
        (
            "removeAllListeners",
            cp_cast1(cp_method_remove_all_listeners),
        ),
        ("emit", cp_cast2(cp_method_emit)),
        ("kill", cp_cast1(cp_method_kill)),
        ("ref", cp_cast0(cp_method_this0)),
        ("unref", cp_cast0(cp_method_this0)),
        ("send", cp_cast4(cp_method_send)),
        ("disconnect", cp_cast0(cp_method_disconnect)),
    ];
    // Distinct shape band from spawn's ChildProcess (which carries no send/disconnect).
    let cp_obj = cp_build_object(&cp_methods, CP_SHAPE_ID + 0x20 + cp_methods.len() as u32);
    let cp = cp_box_ptr(cp_obj as *const u8);
    cp_install_dispose(cp);

    cp_set_field(cp, b"stdout", cp_stdio_js_value(stdio_kinds[1], stdout_obj));
    cp_set_field(cp, b"stderr", cp_stdio_js_value(stdio_kinds[2], stderr_obj));
    cp_set_field(cp, b"stdin", cp_stdio_js_value(stdio_kinds[0], stdin_obj));
    let mut stdio = crate::array::js_array_alloc(stdio_count as u32);
    stdio = crate::array::js_array_push_f64(stdio, cp_stdio_js_value(stdio_kinds[0], stdin_obj));
    stdio = crate::array::js_array_push_f64(stdio, cp_stdio_js_value(stdio_kinds[1], stdout_obj));
    stdio = crate::array::js_array_push_f64(stdio, cp_stdio_js_value(stdio_kinds[2], stderr_obj));
    let mut extra_streams = Vec::new();
    for (fd, kind) in stdio_kinds.iter().copied().enumerate().skip(3) {
        let stream = if fd != ipc_fd && kind == CpStdio::Pipe {
            cp_build_readable()
        } else {
            TAG_NULL_F64
        };
        if fd != ipc_fd && kind == CpStdio::Pipe {
            extra_streams.push((fd, stream));
        }
        stdio = crate::array::js_array_push_f64(
            stdio,
            if fd == ipc_fd {
                TAG_NULL_F64
            } else {
                cp_stdio_js_value(kind, stream)
            },
        );
    }
    cp_set_field(cp, b"stdio", cp_box_ptr(stdio as *const u8));
    cp_set_field(cp, b"exitCode", TAG_NULL_F64);
    cp_set_field(cp, b"signalCode", TAG_NULL_F64);
    cp_set_field(cp, b"killed", TAG_FALSE_F64);
    cp_set_field(cp, b"connected", TAG_FALSE_F64);
    cp_set_field(cp, b"channel", TAG_NULL_F64);
    cp_set_field(cp, b"spawnargs", cp_box_ptr(spawnargs as *const u8));
    cp_set_field(cp, b"spawnfile", cp_box_string(&exec_path));

    // Build the command: <execPath> [execArgv] <module> [args].
    let native_self = std::env::current_exe()
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
        .is_some_and(|path| path == exec_path && module == exec_path);
    let mut command = Command::new(&exec_path);
    let native_exec_argv = if native_self {
        command.args(&arg_strs);
        Some(serde_json::to_string(&exec_argv).unwrap_or_else(|_| "[]".to_string()))
    } else {
        command.args(&exec_argv);
        command.arg(&module);
        command.args(&arg_strs);
        None
    };
    cp_apply_argv0(&mut command, opts_val);
    cp_apply_options(&mut command, opts_val);
    if let Some(exec_argv) = native_exec_argv {
        command.env("PERRY_PROCESS_EXEC_ARGV", exec_argv);
    }
    cp_apply_detached(&mut command, opts_val);
    let launch = match cp_apply_live_stdio(&mut command, &stdio_kinds) {
        Ok(extra_readers) => fork_launch(
            cp,
            stdout_obj,
            stderr_obj,
            stdin_obj,
            extra_readers
                .into_iter()
                .filter_map(|(fd, pipe)| {
                    extra_streams
                        .iter()
                        .find(|(stream_fd, _)| *stream_fd == fd)
                        .map(|(_, stream)| (fd, *stream, pipe))
                })
                .collect(),
            command,
            advanced,
            timeout,
            kill_signal,
            abort_signal,
            opts_val,
            ipc_fd,
        ),
        Err(error) => Err(error),
    };
    if let Err(error) = launch {
        // Spawn failure: emit a deferred `error`, leave `connected` false.
        let code = cp_io_error_code(&error);
        let syscall = format!("spawn {exec_path}");
        let err = cp_make_error(
            &format!("{syscall} {code}"),
            &[
                ("errno", cp_errno_number(code)),
                ("code", cp_box_string(code)),
                ("syscall", cp_box_string(&syscall)),
                ("path", cp_box_string(&exec_path)),
            ],
        );
        cp_set_field(cp, b"__cpError", err);
        let emit_closure =
            crate::closure::js_closure_alloc(reactor::cp_emit_spawn_error as *const u8, 1);
        crate::closure::js_closure_set_capture_ptr(emit_closure, 0, cp.to_bits() as i64);
        crate::timer::js_set_immediate_callback(emit_closure as i64);
    }
    cp
}

/// Wire up the IPC socketpair, launch the child, and register it with the
/// reactor. Returns whether the child spawned. Unix-only IPC; elsewhere the
/// child is launched without a channel.
#[cfg(unix)]
fn fork_launch(
    cp: f64,
    stdout_obj: f64,
    stderr_obj: f64,
    stdin_obj: f64,
    extra_pipes: Vec<(usize, f64, std::fs::File)>,
    mut command: Command,
    advanced: bool,
    timeout: Option<Duration>,
    kill_signal: i32,
    abort_signal: Option<f64>,
    opts_val: f64,
    ipc_fd: usize,
) -> std::io::Result<()> {
    use std::os::unix::io::AsRawFd;
    use std::os::unix::net::UnixStream;
    use std::os::unix::process::CommandExt;

    let (parent_sock, child_sock) = match UnixStream::pair() {
        Ok(p) => p,
        Err(error) => return Err(error),
    };

    // The child inherits `child_sock` across fork; dup it onto the IPC fd (which
    // `dup2` leaves without CLOEXEC, so it survives exec) and advertise it via
    // NODE_CHANNEL_FD — the convention a Node child reads to enable
    // `process.send` / `process.on('message')`.
    let child_fd = child_sock.as_raw_fd();
    command.env("NODE_CHANNEL_FD", ipc_fd.to_string());
    // #2130: tell a node child to use V8 structured-clone framing on the channel.
    command.env(
        "NODE_CHANNEL_SERIALIZATION_MODE",
        if advanced { "advanced" } else { "json" },
    );
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(child_fd, ipc_fd as i32) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    match command.spawn() {
        Ok(child) => {
            // The child now holds fd 3; the parent keeps `parent_sock`.
            drop(child_sock);
            cp_set_field(cp, b"connected", TAG_TRUE_F64);
            let channel = cp_build_object(
                &[
                    ("ref", cp_cast0(cp_method_this0)),
                    ("unref", cp_cast0(cp_method_this0)),
                ],
                CP_SHAPE_ID + 0x40,
            );
            cp_set_field(cp, b"channel", cp_box_ptr(channel as *const u8));
            let handle = reactor::cp_register_live_child(
                cp,
                stdout_obj,
                stderr_obj,
                stdin_obj,
                extra_pipes,
                child,
                Some(parent_sock),
                advanced,
                timeout,
                kill_signal,
            );
            reactor::cp_install_abort_signal(handle, abort_signal, opts_val);
            Ok(())
        }
        Err(error) => Err(error),
    }
}

#[cfg(not(unix))]
fn fork_launch(
    cp: f64,
    stdout_obj: f64,
    stderr_obj: f64,
    stdin_obj: f64,
    extra_pipes: Vec<(usize, f64, std::fs::File)>,
    mut command: Command,
    advanced: bool,
    timeout: Option<Duration>,
    kill_signal: i32,
    abort_signal: Option<f64>,
    opts_val: f64,
    _ipc_fd: usize,
) -> std::io::Result<()> {
    let _ = advanced;
    match command.spawn() {
        Ok(child) => {
            let handle = reactor::cp_register_live_child(
                cp,
                stdout_obj,
                stderr_obj,
                stdin_obj,
                extra_pipes,
                child,
                None,
                false,
                timeout,
                kill_signal,
            );
            reactor::cp_install_abort_signal(handle, abort_signal, opts_val);
            Ok(())
        }
        Err(error) => Err(error),
    }
}
