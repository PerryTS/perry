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
use std::process::{Command, Stdio};

/// `child_process.fork(modulePath[, args][, options])`. `module_ptr`/`args_ptr`
/// are raw (unboxed) `StringHeader` / `ArrayHeader` pointers; `opts_ptr` is a
/// raw heap pointer (or 0). Returns a NaN-boxed ChildProcess.
#[no_mangle]
pub extern "C" fn js_child_process_fork(module_ptr: i64, args_ptr: i64, opts_ptr: i64) -> f64 {
    cp_register_arities();

    reactor::cp_register_reactor_arities();

    let module = unsafe { cp_read_string_header(module_ptr) };
    let arg_strs = unsafe { cp_read_arg_strings(args_ptr) };
    let opts_val = if opts_ptr > 0x10000 {
        cp_box_ptr(opts_ptr as *const u8)
    } else {
        cp_undefined()
    };

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

    // spawnargs = [execPath, ...execArgv, module, ...args] (matches Node).
    let mut spawnargs = crate::array::js_array_alloc((arg_strs.len() + exec_argv.len() + 2) as u32);
    spawnargs = crate::array::js_array_push_f64(spawnargs, cp_box_string(&exec_path));
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
        ("send", cp_cast2(cp_method_send)),
        ("disconnect", cp_cast0(cp_method_disconnect)),
    ];
    // Distinct shape band from spawn's ChildProcess (which carries no send/disconnect).
    let cp_obj = cp_build_object(&cp_methods, CP_SHAPE_ID + 0x20 + cp_methods.len() as u32);
    let cp = cp_box_ptr(cp_obj as *const u8);

    cp_set_field(cp, b"stdout", stdout_obj);
    cp_set_field(cp, b"stderr", stderr_obj);
    cp_set_field(cp, b"stdin", stdin_obj);
    let mut stdio = crate::array::js_array_alloc(4);
    stdio = crate::array::js_array_push_f64(stdio, stdin_obj);
    stdio = crate::array::js_array_push_f64(stdio, stdout_obj);
    stdio = crate::array::js_array_push_f64(stdio, stderr_obj);
    stdio = crate::array::js_array_push_f64(stdio, TAG_NULL_F64); // fd 3 = ipc
    cp_set_field(cp, b"stdio", cp_box_ptr(stdio as *const u8));
    cp_set_field(cp, b"exitCode", TAG_NULL_F64);
    cp_set_field(cp, b"signalCode", TAG_NULL_F64);
    cp_set_field(cp, b"killed", TAG_FALSE_F64);
    cp_set_field(cp, b"connected", TAG_FALSE_F64);
    cp_set_field(cp, b"channel", TAG_NULL_F64);
    cp_set_field(cp, b"spawnargs", cp_box_ptr(spawnargs as *const u8));
    cp_set_field(cp, b"spawnfile", cp_box_string(&exec_path));

    // Build the command: <execPath> [execArgv] <module> [args].
    let mut command = Command::new(&exec_path);
    command.args(&exec_argv);
    command.arg(&module);
    command.args(&arg_strs);
    cp_apply_options(&mut command, opts_val);
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let launched = fork_launch(cp, stdout_obj, stderr_obj, stdin_obj, command, advanced);
    if !launched {
        // Spawn failure: emit a deferred `error`, leave `connected` false.
        let msg = format!("fork failed: {exec_path}");
        let mp = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err = crate::error::js_error_new_with_message(mp);
        cp_set_field(
            cp,
            b"__cpError",
            crate::value::js_nanbox_pointer(err as i64),
        );
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
    mut command: Command,
    advanced: bool,
) -> bool {
    use std::os::unix::io::AsRawFd;
    use std::os::unix::net::UnixStream;
    use std::os::unix::process::CommandExt;

    let (parent_sock, child_sock) = match UnixStream::pair() {
        Ok(p) => p,
        Err(_) => return false,
    };

    // The child inherits `child_sock` across fork; dup it onto fd 3 (which
    // `dup2` leaves without CLOEXEC, so it survives exec) and advertise it via
    // NODE_CHANNEL_FD — the convention a Node child reads to enable
    // `process.send` / `process.on('message')`.
    let child_fd = child_sock.as_raw_fd();
    command.env("NODE_CHANNEL_FD", "3");
    // #2130: tell a node child to use V8 structured-clone framing on the channel.
    command.env(
        "NODE_CHANNEL_SERIALIZATION_MODE",
        if advanced { "advanced" } else { "json" },
    );
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(child_fd, 3) < 0 {
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
            let channel = crate::object::js_object_alloc(0, 0);
            cp_set_field(cp, b"channel", cp_box_ptr(channel as *const u8));
            reactor::cp_register_live_child(
                cp,
                stdout_obj,
                stderr_obj,
                stdin_obj,
                child,
                Some(parent_sock),
                advanced,
            );
            true
        }
        Err(_) => false,
    }
}

#[cfg(not(unix))]
fn fork_launch(
    cp: f64,
    stdout_obj: f64,
    stderr_obj: f64,
    stdin_obj: f64,
    mut command: Command,
    advanced: bool,
) -> bool {
    let _ = advanced;
    match command.spawn() {
        Ok(child) => {
            reactor::cp_register_live_child(
                cp, stdout_obj, stderr_obj, stdin_obj, child, None, false,
            );
            true
        }
        Err(_) => false,
    }
}
