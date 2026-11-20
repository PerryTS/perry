//! Async `exec`/`execFile` — run off the main thread, call back on a later
//! tick (#4912).
//!
//! Split out of `reactor.rs` only to keep that file under the repository's
//! 2000-line cap (`scripts/check_file_size.sh`); same subject as before, just
//! its own file (mirrors how `stdin.rs` / `streams.rs` were split out).

use super::*;

// ============================================================================
// Async `exec` / `execFile` — run off the main thread, call back on a later
// tick (#4912).
// ============================================================================

/// Launch `command` (already shaped as `sh -c <cmd>` for `exec` or `file
/// args…` for `execFile`, with `cwd`/`env` applied) without blocking the main
/// thread, capturing stdout/stderr (unix: the pipe sockets' `'data'`). When the child
/// exits, the pump fires `cb_val(err, stdout, stderr)` on a later event-loop
/// tick. A spawn failure (e.g. `ENOENT`) is reported the same way, deferred to
/// the next tick. Always returns `undefined` (the callback form's return).
pub(in crate::child_process) fn cp_exec_async(
    command: Command,
    cmd_str: String,
    public_spawnfile: Option<String>,
    cb_val: f64,
    run_options: CpRunOptions,
    mode: CpOutput,
) -> f64 {
    // The program actually launched (`sh` for exec, the file for execFile) —
    // Node's spawn-failure error keys `syscall`/`path`/message off this, not
    // off the display command string.
    let file = {
        let program = command.get_program().to_string_lossy();
        #[cfg(unix)]
        if program == "sh" {
            "/bin/sh".to_string()
        } else {
            program.into_owned()
        }
        #[cfg(not(unix))]
        program.into_owned()
    };

    let timeout = run_options.timeout();
    let kill_signal = run_options.kill_signal();
    let methods: [(&str, CpFn); 11] = [
        ("on", crate::fn_info!(cp_method_on, 2; with_declared(2))),
        ("once", crate::fn_info!(cp_method_on, 2; with_declared(2))),
        (
            "addListener",
            crate::fn_info!(cp_method_on, 2; with_declared(2)),
        ),
        (
            "prependListener",
            crate::fn_info!(cp_method_on, 2; with_declared(2)),
        ),
        (
            "removeListener",
            crate::fn_info!(cp_method_remove_listener, 2; with_declared(2)),
        ),
        (
            "off",
            crate::fn_info!(cp_method_remove_listener, 2; with_declared(2)),
        ),
        (
            "removeAllListeners",
            crate::fn_info!(cp_method_remove_all_listeners, 1; with_declared(1)),
        ),
        ("emit", crate::fn_info!(cp_method_emit, 2; with_declared(2))),
        ("kill", crate::fn_info!(cp_method_kill, 1; with_declared(1))),
        ("ref", crate::fn_info!(cp_method_ref, 0; with_declared(0))),
        (
            "unref",
            crate::fn_info!(cp_method_unref, 0; with_declared(0)),
        ),
    ];
    let cp = cp_box_ptr(cp_build_object(&methods, CP_SHAPE_ID + methods.len() as u32) as *const u8);
    cp_set_field(cp, b"exitCode", TAG_NULL_F64);
    cp_set_field(cp, b"signalCode", TAG_NULL_F64);
    cp_set_field(cp, b"killed", TAG_FALSE_F64);
    cp_set_field(cp, b"connected", TAG_FALSE_F64);
    // `cp_command_for_program` may resolve a bare executable against PATH to
    // keep macOS on `posix_spawn`, but Node exposes the caller's original
    // `execFile` spelling through `ChildProcess.spawnfile`. Keep the resolved
    // path above for launch-error metadata and publish the explicit spelling
    // when that API supplied one. `exec` continues to expose its real shell.
    cp_set_field(
        cp,
        b"spawnfile",
        cp_box_string(public_spawnfile.as_deref().unwrap_or(&file)),
    );

    let exec = Box::new(CpExecPending {
        cb_bits: cb_val.to_bits(),
        stdout: Vec::new(),
        stderr: Vec::new(),
        run_options,
        mode,
        cmd: cmd_str,
        file,
        exceeded: false,
        timed_out: false,
    });
    cp_exec_launch(cp, command, exec, timeout, kill_signal)
}

/// Unix: Node's `execFile` is a `spawn` with the default stdio (three pipes,
/// so `stdin`/`stdout`/`stderr` are sockets); the callback buffers what the
/// stdout/stderr sockets read and fires on `'close'`.
#[cfg(unix)]
fn cp_exec_launch(
    cp: f64,
    mut command: Command,
    exec: Box<CpExecPending>,
    timeout: Option<Duration>,
    kill_signal: i32,
) -> f64 {
    let kinds = [CpStdio::Pipe, CpStdio::Pipe, CpStdio::Pipe];
    let launch = cp_apply_live_stdio(&mut command, &kinds)
        .map_err(|error| (error, Vec::new()))
        .and_then(|pipes| match command.spawn() {
            Ok(child) => Ok((child, pipes)),
            Err(error) => Err((error, pipes)),
        });
    match launch {
        Ok((child, parents)) => {
            let sockets = cp_attach_pipe_sockets(cp, &kinds, parents);
            let handle = cp_register_live_child(
                cp,
                sockets.slot(1),
                sockets.slot(2),
                sockets.slot(0),
                sockets.readable.len() as u32,
                child,
                None,
                false,
                timeout,
                kill_signal,
            );
            // The reactor only runs on a later pump, so the pending callback is
            // in place before any event of this child is handled.
            if let Some(lc) = cp_live_lock().as_mut().and_then(|map| map.get_mut(&handle)) {
                lc.exec = Some(exec);
            }
            cp_watch_exec_output(handle, &sockets);
            cp_watch_pipe_closes(handle, &sockets);
            cp
        }
        Err((error, pipes)) => {
            cp_attach_pipe_sockets(cp, &kinds, pipes);
            cp_exec_spawn_failed(cp, exec, &error)
        }
    }
}

/// Elsewhere stdout/stderr are read on background reader threads and stdin is
/// not fed.
#[cfg(not(unix))]
fn cp_exec_launch(
    cp: f64,
    mut command: Command,
    exec: Box<CpExecPending>,
    timeout: Option<Duration>,
    kill_signal: i32,
) -> f64 {
    // exec/execFile capture stdout+stderr and never feed stdin.
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    let stdout_obj = cp_build_readable();
    let stderr_obj = cp_build_readable();
    cp_set_field(cp, b"stdout", stdout_obj);
    cp_set_field(cp, b"stderr", stderr_obj);
    cp_set_field(cp, b"stdin", TAG_NULL_F64);
    let mut stdio = crate::array::js_array_alloc(3);
    stdio = crate::array::js_array_push_f64(stdio, TAG_NULL_F64);
    stdio = crate::array::js_array_push_f64(stdio, stdout_obj);
    stdio = crate::array::js_array_push_f64(stdio, stderr_obj);
    cp_set_field(cp, b"stdio", cp_box_ptr(stdio as *const u8));

    #[cfg(not(windows))]
    let launch = command.spawn();
    #[cfg(windows)]
    let launch = super::super::windows_child::spawn(
        &mut command,
        &[CpStdio::Ignore, CpStdio::Pipe, CpStdio::Pipe],
        exec.run_options.argv0.as_deref(),
        exec.run_options.clear_environment,
        exec.run_options.detached,
    );
    match launch {
        Ok(mut child) => {
            let pid = child.id();
            // Duplicate the process handle BEFORE `child` moves to the waiter
            // thread — the kill paths act on it instead of the recyclable pid.
            #[cfg(windows)]
            let win_proc_handle = cp_win_dup_proc_handle(&child);
            let stdout_pipe = child.stdout.take();
            let stderr_pipe = child.stderr.take();
            let stdout_open = stdout_pipe.is_some();
            let stderr_open = stderr_pipe.is_some();
            let handle = CP_NEXT_LIVE_ID.fetch_add(1, Ordering::SeqCst);
            cp_set_field(cp, b"pid", pid as f64);
            cp_set_field(cp, b"__cpHandle", handle as f64);
            cp_set_field(stdout_obj, b"__cpHandle", handle as f64);
            cp_set_field(stderr_obj, b"__cpHandle", handle as f64);
            let closes_needed = 1 + stdout_open as u32 + stderr_open as u32;
            cp_set_field(cp, b"_closesNeeded", closes_needed as f64);
            cp_set_field(cp, b"_closesGot", 0.0);

            let (process_ids, pipe_ids) = cp_init_async_resources(cp, None, stdout_obj, stderr_obj);

            {
                register_thread_exit_release();
                let mut guard = cp_live_lock();
                let map = guard.get_or_insert_with(HashMap::new);
                map.insert(
                    handle,
                    LiveChild {
                        owner: crate::agent::current_agent(),
                        cp_bits: cp.to_bits(),
                        process_ids,
                        pipe_ids,
                        pipe_bits: [
                            TAG_NULL_F64.to_bits(),
                            stdout_obj.to_bits(),
                            stderr_obj.to_bits(),
                        ],
                        pid: pid as i32,
                        stdin: None,
                        stdout_open,
                        stderr_open,
                        stdout_eof_pending: false,
                        extra_open: Vec::new(),
                        loop_streams: Vec::new(),
                        spawned: false,
                        exited: None,
                        exit_emitted: false,
                        closes_needed,
                        closes_got: 0,
                        closed: false,
                        refed: true,
                        ipc_send: None,
                        ipc_advanced: false,
                        abort_signal_bits: 0,
                        abort_listener_bits: 0,
                        abort_kill_signal: libc_sigterm(),
                        #[cfg(windows)]
                        win_kill_signal: None,
                        #[cfg(windows)]
                        win_proc_handle,
                        exec: Some(exec),
                    },
                );
            }
            crate::stdlib_pump::register_runtime_pump(0, cp_reactor_pump_extern);
            CP_LIVE_COUNT.fetch_add(1, Ordering::SeqCst);
            CP_REFED_COUNT.fetch_add(1, Ordering::SeqCst);

            if let Some(o) = stdout_pipe {
                cp_spawn_reader(handle, cp_pipe_from_child_stdout(o), 1);
            }
            if let Some(e) = stderr_pipe {
                cp_spawn_reader(handle, cp_pipe_from_child_stderr(e), 2);
            }
            let waiter: CpWaiter = Box::new(move || match child.wait() {
                Ok(status) => (status.code(), None),
                Err(_) => (Some(-1), None),
            });
            cp_spawn_waiter(
                handle,
                waiter,
                timeout.map(|timeout| (timeout, kill_signal)),
            );
            crate::event_pump::js_notify_main_thread();
            cp
        }
        Err(error) => cp_exec_spawn_failed(cp, exec, &error),
    }
}

/// Could not spawn at all (ENOENT, EACCES…). Build the same callback error
/// the sync path produced and fire it on a later tick.
fn cp_exec_spawn_failed(cp: f64, exec: Box<CpExecPending>, error: &std::io::Error) -> f64 {
    let run = CpRun {
        stdout: Vec::new(),
        stderr: Vec::new(),
        stdout_piped: true,
        stderr_piped: true,
        code: None,
        signal: None,
        pid: None,
        spawn_error: Some((super::super::cp_io_error_code(error), error.to_string())),
        run_error: None,
    };
    let (err, out, errout) = super::super::cp_exec_callback_args(
        &run,
        &exec.run_options,
        &exec.cmd,
        &exec.file,
        &exec.mode,
    );
    cp_defer_exec_callback(f64::from_bits(exec.cb_bits), err, out, errout);
    cp
}

/// Append a chunk to an exec child's captured stdout/stderr. Returns
/// `Some(kill_signal)` the first time the buffer overruns `maxBuffer`, so the
/// caller terminates the child (Node kills on a `maxBuffer` breach). Once
/// overrun, further chunks are dropped to bound memory — the one chunk that
/// crossed the limit keeps `len > maxBuffer`, which the output/error shapers
/// use to truncate and to name the offending stream.
pub(super) fn cp_exec_accumulate(
    exec: &mut CpExecPending,
    stderr: bool,
    bytes: &[u8],
) -> Option<i32> {
    if exec.exceeded {
        return None;
    }
    let max = exec.run_options.max_buffer;
    let buf = if stderr {
        &mut exec.stderr
    } else {
        &mut exec.stdout
    };
    buf.extend_from_slice(bytes);
    if buf.len() > max {
        exec.exceeded = true;
        return Some(exec.run_options.kill_signal());
    }
    None
}

/// Build the `CpRun` for a finished exec child and fire its callback.
pub(super) fn cp_exec_fire_close(
    exec: Box<CpExecPending>,
    code: Option<i32>,
    signal: Option<i32>,
    pid: i32,
) {
    let exec = *exec;
    let run_error = if exec.exceeded {
        Some(CpRunError::MaxBuffer)
    } else if exec.timed_out {
        Some(CpRunError::Timeout)
    } else {
        None
    };
    // A timeout maps to Node's `(code: null, signal: killSignal)` shape.
    let (code, signal) = if exec.timed_out {
        (None, Some(exec.run_options.kill_signal()))
    } else {
        (code, signal)
    };
    let run = CpRun {
        stdout: exec.stdout,
        stderr: exec.stderr,
        stdout_piped: true,
        stderr_piped: true,
        code,
        signal,
        pid: Some(pid as u32),
        spawn_error: None,
        run_error,
    };
    let (err, out, errout) = super::super::cp_exec_callback_args(
        &run,
        &exec.run_options,
        &exec.cmd,
        &exec.file,
        &exec.mode,
    );
    let cb = crate::fs::extract_closure_ptr(f64::from_bits(exec.cb_bits));
    if !cb.is_null() {
        crate::closure::js_closure_call3(
            cb,
            crate::closure::plain_call_receiver(),
            err,
            out,
            errout,
        );
    }
}

/// Schedule a deferred `cb(err, stdout, stderr)` on the next `setImmediate`
/// macrotask — used for the empty-command, already-aborted, and spawn-failure
/// exec paths, which have no live child to ride the reactor close. The boxed
/// values are kept reachable by the immediate closure's captures.
pub(in crate::child_process) fn cp_defer_exec_callback(
    cb_val: f64,
    err: f64,
    stdout: f64,
    stderr: f64,
) {
    let deferred = js_closure_alloc(crate::fn_info!(cp_exec_cb_thunk, 0; with_declared(0)), 4);
    js_closure_set_capture_ptr(deferred, 0, cb_val.to_bits() as i64);
    js_closure_set_capture_ptr(deferred, 1, err.to_bits() as i64);
    js_closure_set_capture_ptr(deferred, 2, stdout.to_bits() as i64);
    js_closure_set_capture_ptr(deferred, 3, stderr.to_bits() as i64);
    crate::timer::schedule_native_immediate(deferred as i64, &[], "PIPEWRAP");
}

/// The deferred-callback thunk: slots 0..4 capture `cb`, `err`, `stdout`,
/// `stderr`. Takes no JS args.
pub(super) extern "C" fn cp_exec_cb_thunk(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let cb = f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64);
    let err = f64::from_bits(js_closure_get_capture_ptr(closure, 1) as u64);
    let out = f64::from_bits(js_closure_get_capture_ptr(closure, 2) as u64);
    let errout = f64::from_bits(js_closure_get_capture_ptr(closure, 3) as u64);
    let cbptr = crate::fs::extract_closure_ptr(cb);
    if !cbptr.is_null() {
        crate::closure::js_closure_call3(
            cbptr,
            crate::closure::plain_call_receiver(),
            err,
            out,
            errout,
        );
    }
    cp_undefined()
}
