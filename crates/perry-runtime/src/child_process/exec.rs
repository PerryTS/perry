use super::*;

use std::process::Command;

use sync_run::{
    cp_read_async_run_options, cp_read_spawn_sync_run_options, cp_read_sync_stdio_run_options,
    cp_run_to_completion,
};

use crate::closure::{
    js_closure_alloc, js_closure_get_capture_ptr, js_closure_set_capture_ptr,
    js_register_closure_arity, ClosureHeader,
};
use crate::object::{js_object_set_field_by_name, ObjectHeader};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;

/// `child_process.execSync(command[, options])` — run through the shell and
/// return stdout (a Buffer by default, a string with an `encoding` option).
/// On a non-zero exit (or spawn failure) Node throws an Error carrying
/// `status`/`signal`/`pid`/`output`/`stdout`/`stderr`/`cmd`, so this diverges
/// via `js_throw` rather than returning. Returns a NaN-boxed value. #1937/#1938.
#[no_mangle]
pub extern "C" fn js_child_process_exec_sync(
    cmd_ptr: *const StringHeader,
    options_ptr: *const ObjectHeader,
) -> f64 {
    let opts_val = if options_ptr.is_null() {
        cp_undefined()
    } else {
        cp_box_ptr(options_ptr as *const u8)
    };
    let mode = cp_read_output_mode(opts_val, false);

    if cmd_ptr.is_null() {
        return cp_box_output(b"", &mode);
    }

    let cmd_str = unsafe {
        let len = (*cmd_ptr).byte_len as usize;
        let data_ptr = (cmd_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let cmd_bytes = std::slice::from_raw_parts(data_ptr, len);
        String::from_utf8_lossy(cmd_bytes).into_owned()
    };

    // Execute the command using the shell, honoring `cwd`/`env` options.
    #[cfg(unix)]
    let mut command = {
        let mut c = Command::new("sh");
        c.arg("-c").arg(&cmd_str);
        c
    };
    #[cfg(windows)]
    let mut command = {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(&cmd_str);
        c
    };
    cp_apply_options(&mut command, opts_val);

    let run_options = cp_read_sync_stdio_run_options(opts_val);
    let run = cp_run_to_completion(command, &run_options);
    let stdout_box = cp_box_run_output(&run.stdout, run.stdout_piped, &mode);
    if run.success() {
        return stdout_box;
    }
    let stderr_box = cp_box_run_output(&run.stderr, run.stderr_piped, &mode);
    cp_sync_throw_error(&run, &cmd_str, stdout_box, stderr_box);
}

/// `child_process.spawnSync(command[, args][, options])` — run the file
/// directly and return the full Node result object: `status`, `signal`,
/// `output` (`[null, stdout, stderr]`), `pid`, `stdout`, `stderr`, and
/// `error` (first, only on spawn failure). `stdout`/`stderr` are Buffers by default
/// (strings with an `encoding` option). #1936/#1937.
#[no_mangle]
pub extern "C" fn js_child_process_spawn_sync(
    cmd_ptr: *const StringHeader,
    args_ptr: *const crate::array::ArrayHeader,
    options_ptr: *const ObjectHeader,
) -> *mut ObjectHeader {
    if cmd_ptr.is_null() {
        return std::ptr::null_mut();
    }

    let cmd_str = unsafe {
        let cmd_len = (*cmd_ptr).byte_len as usize;
        let cmd_data = (cmd_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(cmd_data, cmd_len)).into_owned()
    };

    let opts_val = if options_ptr.is_null() {
        cp_undefined()
    } else {
        cp_box_ptr(options_ptr as *const u8)
    };
    let mode = cp_read_output_mode(opts_val, false);

    // Build command (run the file directly — spawnSync does not use a shell
    // unless `shell` is set).
    let arg_strs = unsafe { cp_read_arg_strings(args_ptr as i64) };
    let command = cp_build_command(&cmd_str, &arg_strs, opts_val);
    let run_options = cp_read_spawn_sync_run_options(opts_val);
    let run = cp_run_to_completion(command, &run_options);

    let spawn_failed_before_pid = run.spawn_error.is_some() && run.pid.is_none();
    let stdout_box = if spawn_failed_before_pid {
        cp_undefined()
    } else if !run.stdout_piped {
        TAG_NULL_F64
    } else {
        cp_box_output(&run.stdout, &mode)
    };
    let stderr_box = if spawn_failed_before_pid {
        cp_undefined()
    } else if !run.stderr_piped {
        TAG_NULL_F64
    } else {
        cp_box_output(&run.stderr, &mode)
    };
    let output = if spawn_failed_before_pid {
        TAG_NULL_F64
    } else {
        cp_output_array(stdout_box, stderr_box)
    };
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
        None if spawn_failed_before_pid => 0.0,
        None => TAG_NULL_F64,
    };

    // Assemble the result object. `error` is present only on spawn failure
    // (Node omits it otherwise), and is inserted before the standard result
    // fields. Node's observable order is error,status,signal,output,pid,stdout,
    // stderr for spawn failures and status,signal,output,pid,stdout,stderr
    // otherwise.
    let result = crate::object::js_object_alloc(0, 7);
    let set = |key: &str, value: f64| {
        let kp = js_string_from_bytes(key.as_ptr(), key.len() as u32);
        js_object_set_field_by_name(result, kp, value);
    };
    if let Some((code, msg)) = &run.spawn_error {
        let syscall = format!("spawnSync {cmd_str}");
        let err = cp_make_error(
            msg,
            &[
                ("code", cp_box_string(code)),
                ("errno", cp_errno_number(code)),
                ("syscall", cp_box_string(&syscall)),
                ("path", cp_box_string(&cmd_str)),
            ],
        );
        set("error", err);
    } else if let Some(run_error) = run.run_error {
        let code = run_error.code();
        let syscall = format!("spawnSync {cmd_str}");
        let message = format!("{syscall} {code}");
        let err = cp_make_error(
            &message,
            &[
                ("code", cp_box_string(code)),
                ("errno", cp_errno_number(code)),
                ("syscall", cp_box_string(&syscall)),
            ],
        );
        set("error", err);
    }
    set("status", status);
    set("signal", signal);
    set("output", output);
    set("pid", pid);
    set("stdout", stdout_box);
    set("stderr", stderr_box);
    result
}

/// Spawn a process asynchronously
/// Note: This returns a simplified handle for now
/// Full async support would require integration with the async runtime
#[no_mangle]
pub extern "C" fn js_child_process_spawn(
    _cmd_ptr: *const StringHeader,
    _args_ptr: *const crate::array::ArrayHeader,
    _options_ptr: *const ObjectHeader,
) -> *mut ObjectHeader {
    // DEAD/LEGACY path: user-level `child_process.spawn(...)` no longer
    // routes here. It lowers to `Expr::ChildProcessSpawn`
    // (crates/perry-codegen/src/expr/child_proc.rs), which builds a real
    // streaming ChildProcess (stdin/stdout/stderr Readable streams, pid,
    // kill(), spawn/exit/close/error events) — issue #1780. This FFI
    // symbol predates that and is retained only so the dispatch table
    // stays link-complete; it is not reachable from emitted code. (The
    // stub-elimination audit's #4912 "spawn returns null" premise was
    // stale against current main — spawn is real; #4912 closed the
    // remaining `exec`/`execFile` "secretly synchronous" gap: both now run
    // off the main thread and call back on a later tick via the reactor.)
    std::ptr::null_mut()
}

/// `child_process.exec(command[, options], callback)`.
///
/// In Node this runs on the libuv threadpool and fires the callback on a
/// later tick. Perry has no subprocess streaming / event-loop integration for
/// child_process yet (full `spawn` with piped stdout/stderr + EventEmitter is
/// still unimplemented — see #1780), but the dominant
/// `exec(cmd, (err, stdout, stderr) => …)` shape only needs the *buffered*
/// result. Run the command synchronously through the shell (like `execSync`)
/// and invoke the callback immediately with `(err, stdout, stderr)` — the same
/// immediate-callback model the async fs wrappers use. `exec` defaults to utf8
/// encoding, so stdout/stderr are passed as strings.
///
/// `arg1`/`arg2` carry `(options, callback)`. The callback can sit in either
/// slot — `exec(cmd, cb)` puts it in `arg1`, `exec(cmd, options, cb)` in
/// `arg2` — so it's located the same way the fs callbacks disambiguate. With
/// no callback we preserve the legacy behavior of returning the stdout string.
#[no_mangle]
pub extern "C" fn js_child_process_exec(cmd_ptr: *const StringHeader, arg1: f64, arg2: f64) -> f64 {
    use crate::fs::extract_closure_ptr;
    // The callback is whichever argument is a closure; prefer the later slot.
    // Keep the NaN-boxed value too — the async path (#4912) GC-roots it while
    // the call is deferred to the reactor.
    let (cb, cb_val) = {
        let c2 = extract_closure_ptr(arg2);
        if !c2.is_null() {
            (c2, arg2)
        } else {
            (extract_closure_ptr(arg1), arg1)
        }
    };

    // `exec` defaults to utf8 (callback stdout/stderr are strings); the options
    // sit in the `arg1` slot, so the encoding is read from there. When `arg1`
    // is the callback the lookup no-ops and the default applies.
    let mode = cp_read_output_mode(arg1, true);
    let abort_signal = cp_read_abort_signal(arg1);

    if cmd_ptr.is_null() {
        let empty = cp_box_output(b"", &mode);
        if cb.is_null() {
            return empty;
        }
        // Node fires `exec`'s callback on a later tick, never synchronously.
        reactor::cp_defer_exec_callback(cb_val, TAG_NULL_F64, empty, cp_box_output(b"", &mode));
        return f64::from_bits(TAG_UNDEFINED_BITS);
    }

    let cmd_str = unsafe {
        let len = (*cmd_ptr).byte_len as usize;
        let data_ptr = (cmd_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let cmd_bytes = std::slice::from_raw_parts(data_ptr, len);
        String::from_utf8_lossy(cmd_bytes).into_owned()
    };

    if abort_signal.is_some_and(cp_abort_signal_is_aborted) {
        let stdout_box = cp_box_output(b"", &mode);
        if cb.is_null() {
            return stdout_box;
        }
        let stderr_box = cp_box_output(b"", &mode);
        reactor::cp_defer_exec_callback(
            cb_val,
            cp_abort_error(Some(&cmd_str)),
            stdout_box,
            stderr_box,
        );
        return f64::from_bits(TAG_UNDEFINED_BITS);
    }

    // `exec` always runs through the shell. The options object sits in the
    // `arg1` slot (`exec(cmd, options, cb)`); when `arg1` is the callback
    // (`exec(cmd, cb)`) it's a closure, so `cp_apply_options` no-ops. `cwd`/
    // `env` from the options are applied here.
    #[cfg(unix)]
    let mut command = {
        let mut c = Command::new("sh");
        c.arg("-c").arg(&cmd_str);
        c
    };
    #[cfg(windows)]
    let mut command = {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(&cmd_str);
        c
    };
    cp_apply_options(&mut command, arg1);
    let run_options = cp_read_async_run_options(arg1);

    if cb.is_null() {
        // Legacy no-callback shape — run synchronously and return stdout
        // (Buffer or string per `encoding`). Node returns a ChildProcess here;
        // Perry keeps the historical buffered-stdout return for this form.
        let run = cp_run_to_completion(command, &run_options);
        let (stdout_bytes, _) = cp_exec_callback_output_bytes(&run, &run_options);
        return cp_box_output(stdout_bytes, &mode);
    }

    // With a callback, run asynchronously: off the main thread, with the
    // callback fired on a later event-loop tick (#4912).
    reactor::cp_exec_async(command, cmd_str, cb_val, run_options, mode)
}

/// `child_process.execFile(file[, args][, options][, callback])` — like `exec`
/// but runs `file` directly (no shell). The callback fires with
/// `(err, stdout, stderr)`; with no callback the stdout (Buffer/string per
/// `encoding`) is returned. The callback may sit in the options slot
/// (`execFile(file, args, cb)`), so it is located the same way `exec`
/// disambiguates. On failure the error carries `code`/`signal`/`killed`/`cmd`.
/// #1780/#1935/#1937.
#[no_mangle]
pub extern "C" fn js_child_process_exec_file(
    file_ptr: i64,
    args_val: f64,
    opts_val: f64,
    cb_val: f64,
) -> f64 {
    use crate::fs::extract_closure_ptr;
    // Locate the callback and keep its NaN-boxed value for GC rooting while the
    // async run is in flight (#4912).
    let (cb, cb_nanbox) = {
        let c = extract_closure_ptr(cb_val);
        if !c.is_null() {
            (c, cb_val)
        } else {
            (extract_closure_ptr(opts_val), opts_val)
        }
    };

    let file_str = unsafe { cp_read_string_header(file_ptr) };
    let arg_strs = cp_args_from_value(args_val);
    // execFile defaults to utf8 (callback stdout/stderr are strings).
    let mode = cp_read_output_mode(opts_val, true);
    let abort_signal = cp_read_abort_signal(opts_val);

    if abort_signal.is_some_and(cp_abort_signal_is_aborted) {
        let stdout_box = cp_box_output(b"", &mode);
        if cb.is_null() {
            return stdout_box;
        }
        let stderr_box = cp_box_output(b"", &mode);
        reactor::cp_defer_exec_callback(
            cb_nanbox,
            cp_abort_error(Some(&cp_file_cmd_display(&file_str, &arg_strs))),
            stdout_box,
            stderr_box,
        );
        return f64::from_bits(TAG_UNDEFINED_BITS);
    }

    // `cwd`/`env` come from the options slot; when `opts_val` is the callback
    // (`execFile(file, args, cb)`) it's a closure, so the helper no-ops.
    let mut command = Command::new(&file_str);
    command.args(&arg_strs);
    cp_apply_options(&mut command, opts_val);
    let run_options = cp_read_async_run_options(opts_val);

    if cb.is_null() {
        // Legacy no-callback shape — run synchronously, return stdout.
        let run = cp_run_to_completion(command, &run_options);
        let (stdout_bytes, _) = cp_exec_callback_output_bytes(&run, &run_options);
        return cp_box_output(stdout_bytes, &mode);
    }

    // With a callback, run asynchronously: off the main thread, callback on a
    // later event-loop tick (#4912).
    reactor::cp_exec_async(
        command,
        cp_file_cmd_display(&file_str, &arg_strs),
        cb_nanbox,
        run_options,
        mode,
    )
}

/// `child_process.execFileSync(file[, args][, options])` — runs `file`
/// directly (no shell) and returns its stdout (Buffer by default, string with
/// an `encoding` option). Throws on a non-zero exit / spawn failure, carrying
/// the same shape as `execSync`. Returns a NaN-boxed value. #1780/#1937/#1938.
#[no_mangle]
pub extern "C" fn js_child_process_exec_file_sync(
    file_ptr: i64,
    args_val: f64,
    opts_val: f64,
) -> f64 {
    let file_str = unsafe { cp_read_string_header(file_ptr) };
    let mode = cp_read_output_mode(opts_val, false);
    if file_str.is_empty() {
        return cp_box_output(b"", &mode);
    }
    let arg_strs = cp_args_from_value(args_val);
    let mut command = Command::new(&file_str);
    command.args(&arg_strs);
    cp_apply_argv0(&mut command, opts_val);
    cp_apply_options(&mut command, opts_val);
    let run_options = cp_read_sync_stdio_run_options(opts_val);
    let run = cp_run_to_completion(command, &run_options);

    let stdout_box = cp_box_run_output(&run.stdout, run.stdout_piped, &mode);
    if run.success() {
        return stdout_box;
    }
    let stderr_box = cp_box_run_output(&run.stderr, run.stderr_piped, &mode);
    cp_sync_throw_error(
        &run,
        &cp_file_cmd_display(&file_str, &arg_strs),
        stdout_box,
        stderr_box,
    );
}

// ============================================================================
// util.promisify(child_process.exec / execFile) — #1857
// ============================================================================
//
// Node attaches a custom `util.promisify` hook to exec/execFile so the
// promisified form resolves to `{ stdout, stderr }` (not just stdout). The
// `("util","promisify")` dispatch arm detects the bound exec/execFile export
// and routes here; we return a wrapper closure that runs the command (Perry's
// synchronous model) and yields an already-resolved Promise of
// `{ stdout, stderr }` (or a rejected Promise on failure).

/// Settle the pending promise captured in slot 0 from an exec/execFile
/// callback's `(err, stdout, stderr)`. On success → resolve `{ stdout, stderr
/// }` (Node's custom-promisify shape); on failure → attach `stdout`/`stderr` to
/// the error and reject with it. Arity 3. #4912/#1857.
extern "C" fn cp_promise_settle_cb(
    closure: *const ClosureHeader,
    err: f64,
    stdout: f64,
    stderr: f64,
) -> f64 {
    let promise_val = f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64);
    let promise =
        (promise_val.to_bits() & crate::value::POINTER_MASK) as *mut crate::promise::Promise;
    if promise.is_null() {
        return f64::from_bits(TAG_UNDEFINED_BITS);
    }
    if JSValue::from_bits(err.to_bits()).is_null() {
        let obj = unsafe { make_two_field_object("stdout", stdout, "stderr", stderr) };
        crate::promise::js_promise_resolve(promise, cp_box_ptr(obj as *const u8));
    } else {
        // Node's promisify(exec) rejects with the same Error the callback got,
        // with `stdout`/`stderr` attached.
        cp_set_field(err, b"stdout", stdout);
        cp_set_field(err, b"stderr", stderr);
        crate::promise::js_promise_reject(promise, err);
    }
    f64::from_bits(TAG_UNDEFINED_BITS)
}

/// Create the pending promise + a settle closure that fulfils it, then run
/// `command` through the async exec reactor (#4912). Returns the NaN-boxed
/// pending promise. The settle closure (and through it the promise) is kept
/// alive by the reactor's exec-callback GC root.
fn cp_promisified_run(command: Command, cmd_str: String, opts: f64) -> f64 {
    let run_options = cp_read_async_run_options(opts);
    // promisify(exec)/promisify(execFile) yield string stdout/stderr (utf8).
    let mode = cp_read_output_mode(opts, true);
    let promise = crate::promise::js_promise_new();
    js_register_closure_arity(cp_promise_settle_cb as *const u8, 3);
    let cb = js_closure_alloc(cp_promise_settle_cb as *const u8, 1);
    js_closure_set_capture_ptr(cb, 0, cp_box_ptr(promise as *const u8).to_bits() as i64);
    let cb_val = crate::value::js_nanbox_pointer(cb as i64);
    reactor::cp_exec_async(command, cmd_str, cb_val, run_options, mode);
    crate::value::js_nanbox_pointer(promise as i64)
}

extern "C" fn cp_promisified_exec(_closure: *const ClosureHeader, cmd_val: f64, opts: f64) -> f64 {
    let cmd = cp_value_to_string(cmd_val).unwrap_or_default();
    #[cfg(unix)]
    let mut command = {
        let mut c = Command::new("sh");
        c.arg("-c").arg(&cmd);
        c
    };
    #[cfg(windows)]
    let mut command = {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(&cmd);
        c
    };
    cp_apply_options(&mut command, opts);
    cp_promisified_run(command, cmd, opts)
}

extern "C" fn cp_promisified_exec_file(
    _closure: *const ClosureHeader,
    file_val: f64,
    args_val: f64,
) -> f64 {
    let file = cp_value_to_string(file_val).unwrap_or_default();
    let arg_strs = cp_args_from_value(args_val);
    let mut command = Command::new(&file);
    command.args(&arg_strs);
    // The 2-arg promisify(execFile) wrapper has no options slot.
    cp_promisified_run(
        command,
        cp_file_cmd_display(&file, &arg_strs),
        f64::from_bits(TAG_UNDEFINED_BITS),
    )
}

/// Build the wrapper function returned by `util.promisify(child_process.exec)`
/// / `promisify(execFile)` — `method` is `"exec"` or `"execFile"`. Node's
/// custom-promisify hook resolves these to `{ stdout, stderr }`, which the
/// general `util.promisify` path (resolving the single first-result value)
/// can't reproduce; `util_promisify::js_util_promisify` detects the bound
/// export and delegates here. #1857.
pub(crate) fn make_promisified_child_process(method: &str) -> f64 {
    let func: *const u8 = if method == "execFile" {
        js_register_closure_arity(cp_promisified_exec_file as *const u8, 2);
        cp_promisified_exec_file as *const u8
    } else {
        js_register_closure_arity(cp_promisified_exec as *const u8, 2);
        cp_promisified_exec as *const u8
    };
    let closure = js_closure_alloc(func, 0);
    crate::value::js_nanbox_pointer(closure as i64)
}
