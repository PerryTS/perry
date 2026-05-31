//! Child Process module - provides process spawning capabilities

// #1934: live-streaming `spawn` reactor (non-blocking child, stdout/stderr
// pumped through the event loop, live `stdin.write()` / `kill()`).
pub mod reactor;
// #1933: `fork()` + IPC channel (parent `send`/`'message'`/`disconnect`, child
// `process.send`/`process.on('message')`).
pub mod fork;
// #2130: V8 structured-clone codec for `serialization: 'advanced'` IPC.
pub(crate) mod v8_serde;

// #3137: reuse the codec for the public `node:v8` serialize/deserialize API.
pub(crate) use v8_serde::{v8_deserialize, v8_serialize};

use std::collections::HashMap;
use std::fs::File;
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};

use crate::closure::{
    js_closure_alloc, js_closure_get_capture_ptr, js_closure_set_capture_ptr, js_native_call_value,
    js_register_closure_arity, ClosureHeader,
};
use crate::object::{
    js_implicit_this_get, js_implicit_this_set, js_object_alloc_with_shape,
    js_object_get_field_by_name_f64, js_object_set_field, js_object_set_field_by_name,
    ObjectHeader,
};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;

// ============================================================================
// Background Process Registry
// ============================================================================

static NEXT_HANDLE_ID: AtomicU64 = AtomicU64::new(1);

lazy_static::lazy_static! {
    static ref PROCESS_REGISTRY: Mutex<HashMap<u64, std::process::Child>> = Mutex::new(HashMap::new());
}

// NaN-boxing tag constants (inline to avoid pub(crate) visibility issues)
const TAG_NULL_BITS: u64 = 0x7FFC_0000_0000_0002;
const TAG_UNDEFINED_BITS: u64 = 0x7FFC_0000_0000_0001;
const TAG_TRUE_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0004u64);
const TAG_FALSE_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0003u64);
const TAG_NULL_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0002u64);

/// Helper: extract a Rust string from a NaN-boxed f64 string value
unsafe fn extract_string_from_nanboxed(val: f64) -> Option<String> {
    use crate::value::POINTER_MASK;
    let bits = val.to_bits();
    let ptr = (bits & POINTER_MASK) as *const StringHeader;
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

/// Build an object with two f64 fields and named keys.
unsafe fn make_two_field_object(
    first_key: &str,
    first_val: f64,
    second_key: &str,
    second_val: f64,
) -> *mut ObjectHeader {
    use crate::array::{js_array_alloc, js_array_push_f64};
    use crate::value::js_nanbox_string;

    let obj = crate::object::js_object_alloc(0, 2);
    crate::object::js_object_set_field_f64(obj, 0, first_val);
    crate::object::js_object_set_field_f64(obj, 1, second_val);

    // Build keys array so named property access works
    let keys = js_array_alloc(2);
    let k1 = js_string_from_bytes(first_key.as_ptr(), first_key.len() as u32);
    let k2 = js_string_from_bytes(second_key.as_ptr(), second_key.len() as u32);
    let k1_boxed = js_nanbox_string(k1 as i64);
    let k2_boxed = js_nanbox_string(k2 as i64);
    js_array_push_f64(keys, k1_boxed);
    js_array_push_f64(keys, k2_boxed);
    crate::object::js_object_set_keys(obj, keys);

    obj
}

/// Spawn a process in the background (non-blocking).
/// cmd_val: NaN-boxed string (command path)
/// args_ptr: raw pointer to ArrayHeader of string args (0 = none)
/// log_file_val: NaN-boxed string (path to redirect stdout+stderr)
/// env_json_val: NaN-boxed string (JSON {"KEY":"VAL"}) or null/undefined
/// Returns: object {pid: number, handleId: number} or null on error
#[no_mangle]
pub extern "C" fn js_child_process_spawn_background(
    cmd_val: f64,
    args_ptr: i64,
    log_file_val: f64,
    env_json_val: f64,
) -> *mut ObjectHeader {
    unsafe {
        let cmd_str = match extract_string_from_nanboxed(cmd_val) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };
        let log_file_str = match extract_string_from_nanboxed(log_file_val) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };

        let mut command = Command::new(&cmd_str);

        // Add arguments if provided
        if args_ptr != 0 {
            let arr_ptr = args_ptr as *const crate::array::ArrayHeader;
            let args_len = (*arr_ptr).length as usize;
            let args_data = (arr_ptr as *const u8)
                .add(std::mem::size_of::<crate::array::ArrayHeader>())
                as *const f64;
            for i in 0..args_len {
                let arg_val = *args_data.add(i);
                if let Some(arg_str) = extract_string_from_nanboxed(arg_val) {
                    command.arg(arg_str);
                }
            }
        }

        // Parse env JSON if provided (not null/undefined)
        let env_bits = env_json_val.to_bits();
        if env_bits != TAG_NULL_BITS && env_bits != TAG_UNDEFINED_BITS {
            if let Some(env_json) = extract_string_from_nanboxed(env_json_val) {
                if let Ok(map) =
                    serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&env_json)
                {
                    for (k, v) in map {
                        if let Some(val_str) = v.as_str() {
                            command.env(k, val_str);
                        }
                    }
                }
            }
        }

        // Redirect stdout+stderr to log file (try_clone for stderr)
        match File::create(&log_file_str) {
            Ok(stdout_file) => match stdout_file.try_clone() {
                Ok(stderr_file) => {
                    command.stdout(Stdio::from(stdout_file));
                    command.stderr(Stdio::from(stderr_file));
                }
                Err(_) => {
                    command.stdout(Stdio::from(stdout_file));
                    command.stderr(Stdio::null());
                }
            },
            Err(_) => {
                command.stdout(Stdio::null());
                command.stderr(Stdio::null());
            }
        }

        match command.spawn() {
            Ok(child) => {
                let pid = child.id() as f64;
                let handle_id = NEXT_HANDLE_ID.fetch_add(1, Ordering::SeqCst);
                if let Ok(mut registry) = PROCESS_REGISTRY.lock() {
                    registry.insert(handle_id, child);
                }
                make_two_field_object("pid", pid, "handleId", handle_id as f64)
            }
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Spawn `cmd` fully detached from the parent process (orphaned — survives
/// parent exit). Stdin/stdout/stderr go to the OS's null device.
///
/// This is the shared detach implementation used by both `js_child_process_spawn_detached`
/// (the user-facing FFI) and `perry-updater`'s relaunch path. Keep the
/// per-OS detachment logic (Unix `setsid`, Windows `DETACHED_PROCESS |
/// CREATE_NEW_PROCESS_GROUP`) in this one place — it's subtle and easy to
/// get wrong if duplicated.
///
/// Returns the spawned child's PID on success, or `None` on failure (caller
/// chooses how to surface that — `-1.0`/`-1` etc.).
pub fn spawn_detached_command(cmd: &str, args: &[&str], cwd: Option<&str>) -> Option<u32> {
    let mut command = Command::new(cmd);
    for a in args {
        command.arg(a);
    }
    if let Some(d) = cwd {
        command.current_dir(d);
    }

    // Detach stdio so the child doesn't inherit the parent's terminal.
    command.stdin(Stdio::null());
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());

    // Detach from process group so parent exit doesn't take the child with it.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            command.pre_exec(|| {
                // setsid creates a new session + new process group and detaches
                // from the controlling terminal.
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
        // DETACHED_PROCESS = 0x00000008, CREATE_NEW_PROCESS_GROUP = 0x00000200
        command.creation_flags(0x00000008 | 0x00000200);
    }

    match command.spawn() {
        Ok(child) => {
            let pid = child.id();
            // Drop the Child handle without wait() — the OS reaps it.
            std::mem::drop(child);
            Some(pid)
        }
        Err(_) => None,
    }
}

/// Spawn a process fully detached from the parent (orphaned, survives parent exit).
/// Used by the auto-updater to relaunch the new binary before this process exits.
/// cmd_val: NaN-boxed string (command path)
/// args_ptr: raw pointer to ArrayHeader of string args (0 = none)
/// cwd_val: NaN-boxed string (working directory) or null/undefined for cwd inheritance
/// Returns: pid as f64 on success, -1.0 on error
#[no_mangle]
pub extern "C" fn js_child_process_spawn_detached(
    cmd_val: f64,
    args_ptr: i64,
    cwd_val: f64,
) -> f64 {
    unsafe {
        let cmd_str = match extract_string_from_nanboxed(cmd_val) {
            Some(s) => s,
            None => return -1.0,
        };

        let mut owned_args: Vec<String> = Vec::new();
        if args_ptr != 0 {
            let arr_ptr = args_ptr as *const crate::array::ArrayHeader;
            let args_len = (*arr_ptr).length as usize;
            let args_data = (arr_ptr as *const u8)
                .add(std::mem::size_of::<crate::array::ArrayHeader>())
                as *const f64;
            for i in 0..args_len {
                let arg_val = *args_data.add(i);
                if let Some(arg_str) = extract_string_from_nanboxed(arg_val) {
                    owned_args.push(arg_str);
                }
            }
        }
        let args_refs: Vec<&str> = owned_args.iter().map(String::as_str).collect();

        let cwd_bits = cwd_val.to_bits();
        let cwd_owned = if cwd_bits != TAG_NULL_BITS && cwd_bits != TAG_UNDEFINED_BITS {
            extract_string_from_nanboxed(cwd_val)
        } else {
            None
        };
        let cwd_ref: Option<&str> = cwd_owned.as_deref();

        match spawn_detached_command(&cmd_str, &args_refs, cwd_ref) {
            Some(pid) => pid as f64,
            None => -1.0,
        }
    }
}

/// Get the status of a background process (non-blocking).
/// Returns: object {alive: boolean, exitCode: number | null}
#[no_mangle]
pub extern "C" fn js_child_process_get_process_status(handle_id_val: f64) -> *mut ObjectHeader {
    let handle_id = handle_id_val as u64;

    unsafe {
        if let Ok(mut registry) = PROCESS_REGISTRY.lock() {
            if let Some(child) = registry.get_mut(&handle_id) {
                match child.try_wait() {
                    Ok(None) => {
                        // Still running
                        make_two_field_object("alive", TAG_TRUE_F64, "exitCode", TAG_NULL_F64)
                    }
                    Ok(Some(status)) => {
                        let exit_code = status.code().unwrap_or(-1) as f64;
                        registry.remove(&handle_id);
                        make_two_field_object("alive", TAG_FALSE_F64, "exitCode", exit_code)
                    }
                    Err(_) => make_two_field_object("alive", TAG_FALSE_F64, "exitCode", -1.0f64),
                }
            } else {
                // Handle not found — process already exited/cleaned up
                make_two_field_object("alive", TAG_FALSE_F64, "exitCode", TAG_NULL_F64)
            }
        } else {
            std::ptr::null_mut()
        }
    }
}

/// Kill a background process and remove from registry.
/// Returns: 1 on success, 0 on failure
#[no_mangle]
pub extern "C" fn js_child_process_kill_process(handle_id_val: f64) -> i32 {
    let handle_id = handle_id_val as u64;
    if let Ok(mut registry) = PROCESS_REGISTRY.lock() {
        if let Some(mut child) = registry.remove(&handle_id) {
            let _ = child.kill();
            return 1;
        }
    }
    0
}

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

    let run = cp_run_to_completion(command);
    let stdout_box = cp_box_output(&run.stdout, &mode);
    if run.success() {
        return stdout_box;
    }
    let stderr_box = cp_box_output(&run.stderr, &mode);
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
    let run = cp_run_to_completion(command);

    let spawn_failed_before_pid = run.spawn_error.is_some() && run.pid.is_none();
    let stdout_box = if spawn_failed_before_pid {
        cp_undefined()
    } else {
        cp_box_output(&run.stdout, &mode)
    };
    let stderr_box = if spawn_failed_before_pid {
        cp_undefined()
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
    // TODO: Implement async spawn with proper ChildProcess handle
    // For now, return null - async child processes need event loop integration
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
    let cb = {
        let c2 = extract_closure_ptr(arg2);
        if !c2.is_null() {
            c2
        } else {
            extract_closure_ptr(arg1)
        }
    };

    // `exec` defaults to utf8 (callback stdout/stderr are strings); the options
    // sit in the `arg1` slot, so the encoding is read from there. When `arg1`
    // is the callback the lookup no-ops and the default applies.
    let mode = cp_read_output_mode(arg1, true);

    if cmd_ptr.is_null() {
        let empty = cp_box_output(b"", &mode);
        if cb.is_null() {
            return empty;
        }
        crate::closure::js_closure_call3(cb, TAG_NULL_F64, empty, cp_box_output(b"", &mode));
        return f64::from_bits(TAG_UNDEFINED_BITS);
    }

    let cmd_str = unsafe {
        let len = (*cmd_ptr).byte_len as usize;
        let data_ptr = (cmd_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let cmd_bytes = std::slice::from_raw_parts(data_ptr, len);
        String::from_utf8_lossy(cmd_bytes).into_owned()
    };

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
    let run = cp_run_to_completion(command);

    let stdout_box = cp_box_output(&run.stdout, &mode);
    if cb.is_null() {
        // Legacy no-callback shape — return stdout (Buffer or string per
        // `encoding`).
        return stdout_box;
    }

    let stderr_box = cp_box_output(&run.stderr, &mode);
    let err_val = if run.success() {
        TAG_NULL_F64
    } else {
        cp_exec_callback_error(&run, &cmd_str)
    };
    crate::closure::js_closure_call3(cb, err_val, stdout_box, stderr_box);
    f64::from_bits(TAG_UNDEFINED_BITS)
}

// ============================================================================
// Streaming spawn — a real ChildProcess (EventEmitter + Readable stdout/stderr)
// ============================================================================
//
// `spawn(cmd, args)` runs the command, buffers its stdout/stderr, and returns a
// heap ChildProcess object whose methods are real closures (the closure-fields
// pattern from `node_stream.rs::build_object`). Event delivery (`spawn` /
// `data` / `end` / `exit` / `close`) is deferred to a `setImmediate` macrotask,
// so handlers registered synchronously after the `spawn()` call — e.g. inside a
// Promise executor before the first `await`, as the parity test does — are
// present when the events fire.
//
// Perry has no async subprocess reactor, so the child's output is captured
// synchronously at spawn time. For the short-lived commands these APIs are used
// with, that is observationally identical to Node's async pipe model once the
// deferred emission runs on the next event-loop tick. #1780.

// Shape-id band kept clear of node_stream (0x7FFF_FE60+), fs streams
// (0x7FFF_FE40), and weakref (0x7FFF_FE10+).
const CP_SHAPE_ID: u32 = 0x7FFF_FD00;
const CP_READABLE_SHAPE_ID: u32 = 0x7FFF_FD40;
const CP_WRITABLE_SHAPE_ID: u32 = 0x7FFF_FD80;

#[inline]
fn cp_undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED_BITS)
}

#[inline]
fn cp_box_ptr(ptr: *const u8) -> f64 {
    f64::from_bits(JSValue::pointer(ptr).bits())
}

/// Recover the host object value captured in closure slot 0 by `cp_build_object`.
#[inline]
fn cp_this(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return js_implicit_this_get();
    }
    f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64)
}

/// Resolve a NaN-boxed value to an `ObjectHeader*` iff it is a heap object.
fn cp_object_ptr(value: f64) -> Option<*mut ObjectHeader> {
    let bits = value.to_bits();
    if !JSValue::from_bits(bits).is_pointer() {
        return None;
    }
    let raw = (bits & crate::value::POINTER_MASK) as usize;
    if raw < 0x10000 || crate::buffer::is_registered_buffer(raw) {
        return None;
    }
    unsafe {
        let header =
            (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*header).obj_type != crate::gc::GC_TYPE_OBJECT {
            return None;
        }
    }
    Some(raw as *mut ObjectHeader)
}

/// Resolve a NaN-boxed value to an `ArrayHeader*` iff it is a heap array.
fn cp_array_ptr(value: f64) -> Option<*mut crate::array::ArrayHeader> {
    let bits = value.to_bits();
    if !JSValue::from_bits(bits).is_pointer() {
        return None;
    }
    let raw = (bits & crate::value::POINTER_MASK) as usize;
    if raw < 0x10000 {
        return None;
    }
    unsafe {
        let header =
            (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let t = (*header).obj_type;
        if t == crate::gc::GC_TYPE_ARRAY || t == crate::gc::GC_TYPE_LAZY_ARRAY {
            Some(raw as *mut crate::array::ArrayHeader)
        } else {
            None
        }
    }
}

#[inline]
fn cp_str_key(bytes: &[u8]) -> *mut StringHeader {
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn cp_get_field(value: f64, name: &[u8]) -> f64 {
    match cp_object_ptr(value) {
        Some(obj) => js_object_get_field_by_name_f64(obj, cp_str_key(name)),
        None => cp_undefined(),
    }
}

fn cp_set_field(value: f64, name: &[u8], field_value: f64) {
    if let Some(obj) = cp_object_ptr(value) {
        js_object_set_field_by_name(obj, cp_str_key(name), field_value);
    }
}

#[inline]
fn cp_box_string(s: &str) -> f64 {
    let sh = js_string_from_bytes(s.as_ptr(), s.len() as u32);
    crate::value::js_nanbox_string(sh as i64)
}

/// SSO-safe extraction of a JS string value to an owned Rust string. The fixed
/// child_process event names (`data`/`end`/`exit`/`close`/`spawn`/`error`) and
/// many argv entries are ≤5 bytes — i.e. SSO short strings — which the file's
/// `extract_string_from_nanboxed` (STRING_TAG + StringHeader only) misses, so
/// route through the unified accessor which materializes SSO bytes.
fn cp_value_to_string(value: f64) -> Option<String> {
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return None;
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        std::str::from_utf8(std::slice::from_raw_parts(data, len))
            .ok()
            .map(|s| s.to_string())
    }
}

/// Hidden field key holding the listener array for `event`.
fn cp_listener_key(event: &str) -> Vec<u8> {
    let mut k = b"__cpL_".to_vec();
    k.extend_from_slice(event.as_bytes());
    k
}

/// Append a listener closure to `target`'s `event` list (the `.on` body).
fn cp_register(target: f64, event: f64, cb: f64) {
    let name = match cp_value_to_string(event) {
        Some(n) => n,
        None => return,
    };
    let key = cp_listener_key(&name);
    let arr = match cp_array_ptr(cp_get_field(target, &key)) {
        Some(a) => a,
        None => crate::array::js_array_alloc(2),
    };
    let arr = crate::array::js_array_push_f64(arr, cb);
    cp_set_field(target, &key, cp_box_ptr(arr as *const u8));
}

/// Invoke every listener registered on `target` for `event`. Returns whether
/// any fired. The listener array is re-read each iteration so a moving GC
/// during a handler call can't strand us on a stale array pointer.
fn cp_emit(target: f64, event: &str, args: &[f64]) -> bool {
    if event == "message"
        && args
            .first()
            .copied()
            .is_some_and(|msg| crate::cluster::consume_internal_message(target, msg))
    {
        return true;
    }

    let key = cp_listener_key(event);
    let mut i: u32 = 0;
    let mut fired = false;
    loop {
        let arr = match cp_array_ptr(cp_get_field(target, &key)) {
            Some(a) => a,
            None => break,
        };
        if i >= crate::array::js_array_length(arr) {
            break;
        }
        let cb = crate::array::js_array_get_f64(arr, i);
        let prev = js_implicit_this_set(target);
        unsafe {
            let _ = js_native_call_value(cb, args.as_ptr(), args.len());
        }
        js_implicit_this_set(prev);
        fired = true;
        i += 1;
    }
    fired
}

fn cp_signal_name(sig: i32) -> &'static str {
    match sig {
        1 => "SIGHUP",
        2 => "SIGINT",
        6 => "SIGABRT",
        9 => "SIGKILL",
        11 => "SIGSEGV",
        15 => "SIGTERM",
        _ => "SIGTERM",
    }
}

// ----- method bodies (each receives the closure; slot 0 = host `this`) -----

extern "C" fn cp_method_on(closure: *const ClosureHeader, event: f64, cb: f64) -> f64 {
    let this = cp_this(closure);
    cp_register(this, event, cb);
    this
}
extern "C" fn cp_method_emit(closure: *const ClosureHeader, event: f64, arg: f64) -> f64 {
    let this = cp_this(closure);
    let name = match cp_value_to_string(event) {
        Some(n) => n,
        None => return TAG_FALSE_F64,
    };
    if cp_emit(this, &name, &[arg]) {
        TAG_TRUE_F64
    } else {
        TAG_FALSE_F64
    }
}
extern "C" fn cp_method_this0(closure: *const ClosureHeader) -> f64 {
    cp_this(closure)
}
extern "C" fn cp_method_this1(closure: *const ClosureHeader, _a: f64) -> f64 {
    cp_this(closure)
}
extern "C" fn cp_method_kill(closure: *const ClosureHeader, signal: f64) -> f64 {
    let this = cp_this(closure);
    cp_set_field(this, b"killed", TAG_TRUE_F64);
    // #1934: signal the live child if one is still running. `__cpHandle` is the
    // reactor registry key set by `spawn`. Returns true when the signal was
    // delivered (Node's `kill()` returns a boolean).
    if let Some(handle) = cp_handle_of(this) {
        if reactor::cp_live_kill(handle, signal) {
            return TAG_TRUE_F64;
        }
    }
    TAG_TRUE_F64
}
/// `removeListener(event, cb)` / `off(event, cb)` — rebuild the `event`
/// listener array without the matching closure (compared by NaN-boxed bits).
/// #1780.
extern "C" fn cp_method_remove_listener(closure: *const ClosureHeader, event: f64, cb: f64) -> f64 {
    let this = cp_this(closure);
    if let Some(name) = cp_value_to_string(event) {
        let key = cp_listener_key(&name);
        if let Some(arr) = cp_array_ptr(cp_get_field(this, &key)) {
            let n = crate::array::js_array_length(arr);
            let mut out = crate::array::js_array_alloc(n);
            for i in 0..n {
                let v = crate::array::js_array_get_f64(arr, i);
                if v.to_bits() != cb.to_bits() {
                    out = crate::array::js_array_push_f64(out, v);
                }
            }
            cp_set_field(this, &key, cp_box_ptr(out as *const u8));
        }
    }
    this
}

/// `removeAllListeners([event])` — clear one event's listener list, or every
/// `__cpL_*` list when called with no event. #1780.
extern "C" fn cp_method_remove_all_listeners(closure: *const ClosureHeader, event: f64) -> f64 {
    let this = cp_this(closure);
    if let Some(name) = cp_value_to_string(event) {
        let key = cp_listener_key(&name);
        let empty = crate::array::js_array_alloc(0);
        cp_set_field(this, &key, cp_box_ptr(empty as *const u8));
        return this;
    }
    // No event argument: clear every listener array on the object.
    if let Some(obj) = cp_object_ptr(this) {
        let keys = crate::object::js_object_keys(obj);
        if !keys.is_null() {
            let n = crate::array::js_array_length(keys);
            for i in 0..n {
                if let Some(k) = cp_value_to_string(crate::array::js_array_get_f64(keys, i)) {
                    if k.as_bytes().starts_with(b"__cpL_") {
                        let empty = crate::array::js_array_alloc(0);
                        cp_set_field(this, k.as_bytes(), cp_box_ptr(empty as *const u8));
                    }
                }
            }
        }
    }
    this
}

extern "C" fn cp_method_read(_closure: *const ClosureHeader, _n: f64) -> f64 {
    TAG_NULL_F64
}
extern "C" fn cp_method_pipe(_closure: *const ClosureHeader, dest: f64) -> f64 {
    dest
}
/// `child.stdin.write(chunk[, encoding][, callback])` — #1934. The `this` is
/// the stdin Writable; route the bytes to the live child's stdin via the
/// reactor. Returns `true` (Node's `write` returns whether the buffer can take
/// more — `true` for our synchronous pipe write).
extern "C" fn cp_method_write2(closure: *const ClosureHeader, chunk: f64, _enc: f64) -> f64 {
    let this = cp_this(closure);
    if let Some(handle) = cp_handle_of(this) {
        let bytes = cp_value_to_bytes(chunk);
        reactor::cp_live_stdin_write(handle, &bytes);
    }
    TAG_TRUE_F64
}

/// `child.send(message[, sendHandle][, options][, callback])` — serialize
/// `message` and write it to the IPC channel of a `fork()`ed child (#1933). The
/// `this` is the ChildProcess. Returns `true` (Node returns whether the channel
/// can take more — always `true` for our synchronous write).
extern "C" fn cp_method_send(closure: *const ClosureHeader, message: f64, _arg: f64) -> f64 {
    let this = cp_this(closure);
    if let Some(handle) = cp_handle_of(this) {
        reactor::cp_ipc_send(handle, message);
    }
    TAG_TRUE_F64
}

/// `child.disconnect()` — close the IPC channel (#1933). Flips `connected` to
/// `false`, `channel` to `null`, and emits a `disconnect` event.
extern "C" fn cp_method_disconnect(closure: *const ClosureHeader) -> f64 {
    let this = cp_this(closure);
    if let Some(handle) = cp_handle_of(this) {
        reactor::cp_ipc_disconnect(handle);
    }
    cp_set_field(this, b"connected", TAG_FALSE_F64);
    cp_set_field(this, b"channel", TAG_NULL_F64);
    cp_emit(this, "disconnect", &[]);
    cp_undefined()
}

/// `child.stdin.end([chunk])` — write the optional final chunk, then close the
/// pipe so the child sees EOF (#1934). The `this` is the stdin Writable.
extern "C" fn cp_method_stdin_end(closure: *const ClosureHeader, chunk: f64) -> f64 {
    let this = cp_this(closure);
    if let Some(handle) = cp_handle_of(this) {
        // Optional final data chunk. Skip `undefined`, the `0.0` arg-padding
        // sentinel, and a callback argument (`end(cb)`).
        let bits = chunk.to_bits();
        if !JSValue::from_bits(bits).is_undefined()
            && bits != 0
            && crate::fs::extract_closure_ptr(chunk).is_null()
        {
            let bytes = cp_value_to_bytes(chunk);
            if !bytes.is_empty() {
                reactor::cp_live_stdin_write(handle, &bytes);
            }
        }
        reactor::cp_live_stdin_close(handle);
    }
    this
}

/// Read the reactor registry key (`__cpHandle`) off a ChildProcess / stdio
/// sub-object, set by `spawn`. `None` when absent (e.g. a buffered child).
fn cp_handle_of(this: f64) -> Option<u64> {
    let h = cp_get_field(this, b"__cpHandle");
    if JSValue::from_bits(h.to_bits()).is_undefined() {
        return None;
    }
    if h.is_finite() && h >= 0.0 {
        Some(h as u64)
    } else {
        None
    }
}

/// Best-effort decode of a `write()` chunk (Buffer or string) to raw bytes.
fn cp_value_to_bytes(value: f64) -> Vec<u8> {
    // Buffer fast-path.
    let bits = value.to_bits();
    if JSValue::from_bits(bits).is_pointer() {
        let raw = (bits & crate::value::POINTER_MASK) as usize;
        if raw >= 0x10000 && crate::buffer::is_registered_buffer(raw) {
            let buf = raw as *const crate::buffer::BufferHeader;
            unsafe {
                let len = (*buf).length as usize;
                let data =
                    (buf as *const u8).add(std::mem::size_of::<crate::buffer::BufferHeader>());
                return std::slice::from_raw_parts(data, len).to_vec();
            }
        }
    }
    // Otherwise stringify.
    cp_value_to_string(value)
        .or_else(|| Some(cp_coerce_string(value)))
        .unwrap_or_default()
        .into_bytes()
}

// ----- object construction -----

type CpFn = unsafe extern "C" fn();
#[allow(clippy::missing_transmute_annotations)]
fn cp_cast0(f: extern "C" fn(*const ClosureHeader) -> f64) -> CpFn {
    unsafe { std::mem::transmute(f) }
}
#[allow(clippy::missing_transmute_annotations)]
fn cp_cast1(f: extern "C" fn(*const ClosureHeader, f64) -> f64) -> CpFn {
    unsafe { std::mem::transmute(f) }
}
#[allow(clippy::missing_transmute_annotations)]
fn cp_cast2(f: extern "C" fn(*const ClosureHeader, f64, f64) -> f64) -> CpFn {
    unsafe { std::mem::transmute(f) }
}

fn cp_register_arities() {
    js_register_closure_arity(cp_method_on as *const u8, 2);
    js_register_closure_arity(cp_method_emit as *const u8, 2);
    js_register_closure_arity(cp_method_this0 as *const u8, 0);
    js_register_closure_arity(cp_method_this1 as *const u8, 1);
    js_register_closure_arity(cp_method_remove_listener as *const u8, 2);
    js_register_closure_arity(cp_method_remove_all_listeners as *const u8, 1);
    js_register_closure_arity(cp_method_kill as *const u8, 1);
    js_register_closure_arity(cp_method_read as *const u8, 1);
    js_register_closure_arity(cp_method_pipe as *const u8, 1);
    js_register_closure_arity(cp_method_write2 as *const u8, 2);
    js_register_closure_arity(cp_method_stdin_end as *const u8, 1);
    js_register_closure_arity(cp_method_send as *const u8, 2);
    js_register_closure_arity(cp_method_disconnect as *const u8, 0);
}

/// Allocate a heap object whose method-name fields each hold a closure capturing
/// the object itself in slot 0 (so method bodies recover `this`).
fn cp_build_object(methods: &[(&str, CpFn)], shape_id: u32) -> *mut ObjectHeader {
    let mut packed: Vec<u8> = Vec::new();
    for (name, _) in methods {
        packed.extend_from_slice(name.as_bytes());
        packed.push(0);
    }
    let obj = js_object_alloc_with_shape(
        shape_id,
        methods.len() as u32,
        packed.as_ptr(),
        packed.len() as u32,
    );
    let this_bits = JSValue::pointer(obj as *const u8).bits();
    for (i, (_name, func)) in methods.iter().enumerate() {
        let closure = js_closure_alloc(*func as *const u8, 1);
        js_closure_set_capture_ptr(closure, 0, this_bits as i64);
        js_object_set_field(obj, i as u32, JSValue::pointer(closure as *const u8));
    }
    obj
}

/// Build a stdout/stderr Readable-shaped EventEmitter.
fn cp_build_readable() -> f64 {
    let methods: [(&str, CpFn); 13] = [
        ("on", cp_cast2(cp_method_on)),
        ("once", cp_cast2(cp_method_on)),
        ("addListener", cp_cast2(cp_method_on)),
        ("prependListener", cp_cast2(cp_method_on)),
        ("off", cp_cast2(cp_method_remove_listener)),
        ("removeListener", cp_cast2(cp_method_remove_listener)),
        ("emit", cp_cast2(cp_method_emit)),
        ("pause", cp_cast0(cp_method_this0)),
        ("resume", cp_cast0(cp_method_this0)),
        ("destroy", cp_cast0(cp_method_this0)),
        ("setEncoding", cp_cast1(cp_method_this1)),
        ("read", cp_cast1(cp_method_read)),
        ("pipe", cp_cast1(cp_method_pipe)),
    ];
    let obj = cp_build_object(&methods, CP_READABLE_SHAPE_ID + methods.len() as u32);
    let val = cp_box_ptr(obj as *const u8);
    cp_set_field(val, b"readable", TAG_TRUE_F64);
    cp_set_field(val, b"destroyed", TAG_FALSE_F64);
    val
}

/// Build a stdin Writable-shaped EventEmitter.
fn cp_build_writable() -> f64 {
    let methods: [(&str, CpFn); 11] = [
        ("on", cp_cast2(cp_method_on)),
        ("once", cp_cast2(cp_method_on)),
        ("addListener", cp_cast2(cp_method_on)),
        ("removeListener", cp_cast2(cp_method_remove_listener)),
        ("off", cp_cast2(cp_method_remove_listener)),
        ("emit", cp_cast2(cp_method_emit)),
        ("write", cp_cast2(cp_method_write2)),
        ("end", cp_cast1(cp_method_stdin_end)),
        ("destroy", cp_cast0(cp_method_this0)),
        ("cork", cp_cast0(cp_method_this0)),
        ("uncork", cp_cast0(cp_method_this0)),
    ];
    let obj = cp_build_object(&methods, CP_WRITABLE_SHAPE_ID + methods.len() as u32);
    let val = cp_box_ptr(obj as *const u8);
    cp_set_field(val, b"writable", TAG_TRUE_F64);
    cp_set_field(val, b"destroyed", TAG_FALSE_F64);
    val
}

/// NaN-boxed `Buffer` value holding `bytes`.
fn cp_make_buffer(bytes: &[u8]) -> f64 {
    let buf = crate::buffer::js_buffer_alloc(bytes.len() as i32, 0);
    if buf.is_null() {
        return cp_undefined();
    }
    unsafe {
        let data = (buf as *mut u8).add(std::mem::size_of::<crate::buffer::BufferHeader>());
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
        (*buf).length = bytes.len() as u32;
    }
    cp_box_ptr(buf as *const u8)
}

unsafe fn cp_read_string_header(ptr: i64) -> String {
    if ptr == 0 {
        return String::new();
    }
    let sh = ptr as *const StringHeader;
    let len = (*sh).byte_len as usize;
    let data = (sh as *const u8).add(std::mem::size_of::<StringHeader>());
    String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
}

unsafe fn cp_read_arg_strings(args_ptr: i64) -> Vec<String> {
    let mut out = Vec::new();
    if args_ptr == 0 {
        return out;
    }
    let arr = args_ptr as *const crate::array::ArrayHeader;
    let n = (*arr).length as usize;
    let data =
        (arr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64;
    for i in 0..n {
        if let Some(s) = cp_value_to_string(*data.add(i)) {
            out.push(s);
        }
    }
    out
}

/// Collect a NaN-boxed args value (array of strings) into owned Rust strings.
fn cp_args_from_value(value: f64) -> Vec<String> {
    match cp_array_ptr(value) {
        Some(arr) => {
            let n = unsafe { (*arr).length };
            let mut out = Vec::with_capacity(n as usize);
            for i in 0..n {
                if let Some(s) = cp_value_to_string(crate::array::js_array_get_f64(arr, i)) {
                    out.push(s);
                }
            }
            out
        }
        None => Vec::new(),
    }
}

// ============================================================================
// Spawn / exec options: `cwd`, `env`, `shell` — #1780
// ============================================================================
//
// The streaming-spawn / exec / execFile entry points previously ignored their
// options object entirely. These helpers read the common, host-portable
// options off a NaN-boxed options value and apply them to a `std::process::
// Command`. Anything not listed here (`stdio`, `timeout`, `maxBuffer`, …) is
// still ignored.

/// Coerce any JS value to an owned Rust string — string fast-path, else
/// `js_jsvalue_to_string`. Used for `env` values, which Node stringifies.
fn cp_coerce_string(value: f64) -> String {
    if let Some(s) = cp_value_to_string(value) {
        return s;
    }
    let p = crate::value::js_jsvalue_to_string(value);
    if p.is_null() {
        return String::new();
    }
    unsafe { cp_read_string_header(p as i64) }
}

/// Apply the host-portable `{ cwd, env }` options to `command`. `opts_val` is a
/// NaN-boxed options object (or undefined/null/non-object — then a no-op). Node
/// semantics: `env` *replaces* the child's environment wholesale, so when an
/// `env` object is provided we `env_clear()` first and skip keys whose value is
/// `undefined`. #1780.
fn cp_apply_options(command: &mut Command, opts_val: f64) {
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
fn cp_build_command(cmd: &str, args: &[String], opts_val: f64) -> Command {
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

    cp_apply_options(&mut command, opts_val);
    command
}

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
enum CpOutput {
    Buffer,
    Text(String),
}

/// Read the `encoding` option off a NaN-boxed options value. `default_text`
/// picks the default when `encoding` is absent (exec/execFile → utf8 text;
/// the sync forms → Buffer). `null` / `"buffer"` always mean Buffer.
fn cp_read_output_mode(opts_val: f64, default_text: bool) -> CpOutput {
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
fn cp_box_output(bytes: &[u8], mode: &CpOutput) -> f64 {
    match mode {
        CpOutput::Buffer => cp_make_buffer(bytes),
        CpOutput::Text(enc) => crate::value::js_nanbox_string(cp_encode_text(bytes, enc) as i64),
    }
}

/// Decoded exit disposition of a finished child.
struct CpExit {
    /// Exit code when the child exited normally; `None` when killed by signal.
    code: Option<i32>,
    /// Signal number when the child was killed by a signal (Unix only).
    signal: Option<i32>,
}

fn cp_decode_status(status: &std::process::ExitStatus) -> CpExit {
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
fn cp_io_error_code(e: &std::io::Error) -> &'static str {
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
fn cp_errno_number(code: &str) -> f64 {
    #[cfg(unix)]
    let n = match code {
        "ENOENT" => libc::ENOENT,
        "EACCES" => libc::EACCES,
        "EEXIST" => libc::EEXIST,
        "EPIPE" => libc::EPIPE,
        "ETIMEDOUT" => libc::ETIMEDOUT,
        "ECONNREFUSED" => libc::ECONNREFUSED,
        _ => 0,
    };
    #[cfg(not(unix))]
    let n = 0;
    -(n as f64)
}

/// Outcome of running a child to completion (buffered).
struct CpRun {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    code: Option<i32>,
    signal: Option<i32>,
    pid: Option<u32>,
    /// `Some((code, message))` when the child could not be spawned at all.
    spawn_error: Option<(&'static str, String)>,
}

impl CpRun {
    fn success(&self) -> bool {
        self.spawn_error.is_none() && self.code == Some(0)
    }
}

/// Spawn `command` with piped stdout/stderr (and a closed stdin so children
/// that read stdin see EOF instead of blocking), run it to completion, and
/// capture stdout/stderr/exit/pid. Used by the synchronous + buffered-callback
/// entry points.
fn cp_run_to_completion(mut command: Command) -> CpRun {
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    match command.spawn() {
        Ok(child) => {
            let pid = child.id();
            match child.wait_with_output() {
                Ok(o) => {
                    let CpExit { code, signal } = cp_decode_status(&o.status);
                    CpRun {
                        stdout: o.stdout,
                        stderr: o.stderr,
                        code,
                        signal,
                        pid: Some(pid),
                        spawn_error: None,
                    }
                }
                Err(e) => CpRun {
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    code: None,
                    signal: None,
                    pid: Some(pid),
                    spawn_error: Some((cp_io_error_code(&e), e.to_string())),
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
        },
    }
}

/// Build an error-like heap object. `ErrorHeader` rejects dynamic-property
/// writes, so for the rich shape Node attaches we use a regular object whose
/// class extends `Error` (so `instanceof Error` / `typeof` still report
/// error-ish) and set the props by name. Returns a NaN-boxed pointer.
fn cp_make_error(message: &str, extra: &[(&str, f64)]) -> f64 {
    static REGISTER_ERROR: std::sync::Once = std::sync::Once::new();
    REGISTER_ERROR.call_once(|| {
        crate::object::js_register_class_extends_error(crate::error::CLASS_ID_ERROR);
    });
    let obj =
        crate::object::js_object_alloc(crate::error::CLASS_ID_ERROR, (extra.len() + 2) as u32);
    let set = |key: &str, value: f64| {
        let kp = js_string_from_bytes(key.as_ptr(), key.len() as u32);
        js_object_set_field_by_name(obj, kp, value);
    };
    set("name", cp_box_string("Error"));
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

/// `[null, stdout, stderr]` — the Node `output` array shared by spawnSync and
/// the execSync throw error.
fn cp_output_array(stdout: f64, stderr: f64) -> f64 {
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
/// `path` on spawn failure). `cmd` is the human-readable command string. #1935.
fn cp_exec_callback_error(run: &CpRun, cmd: &str) -> f64 {
    if let Some((errno_code, _)) = run.spawn_error {
        let syscall = format!("spawn {cmd}");
        let message = format!("{syscall} {errno_code}");
        return cp_make_error(
            &message,
            &[
                ("code", cp_box_string(errno_code)),
                ("errno", cp_errno_number(errno_code)),
                ("syscall", cp_box_string(&syscall)),
                ("path", cp_box_string(cmd)),
                ("cmd", cp_box_string(cmd)),
                ("killed", TAG_FALSE_F64),
                ("signal", TAG_NULL_F64),
            ],
        );
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

/// Throw the error Node raises from a failed execSync/execFileSync — carries
/// `status`/`signal`/`pid`/`output`/`stdout`/`stderr`/`cmd`. Diverges. #1938.
fn cp_sync_throw_error(run: &CpRun, cmd: &str, stdout: f64, stderr: f64) -> ! {
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
fn cp_file_cmd_display(file: &str, args: &[String]) -> String {
    if args.is_empty() {
        file.to_string()
    } else {
        format!("{} {}", file, args.join(" "))
    }
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
    let cb = {
        let c = extract_closure_ptr(cb_val);
        if !c.is_null() {
            c
        } else {
            extract_closure_ptr(opts_val)
        }
    };

    let file_str = unsafe { cp_read_string_header(file_ptr) };
    let arg_strs = cp_args_from_value(args_val);
    // execFile defaults to utf8 (callback stdout/stderr are strings).
    let mode = cp_read_output_mode(opts_val, true);

    // `cwd`/`env` come from the options slot; when `opts_val` is the callback
    // (`execFile(file, args, cb)`) it's a closure, so the helper no-ops.
    let mut command = Command::new(&file_str);
    command.args(&arg_strs);
    cp_apply_options(&mut command, opts_val);
    let run = cp_run_to_completion(command);

    let stdout_box = cp_box_output(&run.stdout, &mode);
    if cb.is_null() {
        return stdout_box;
    }
    let stderr_box = cp_box_output(&run.stderr, &mode);
    let err_val = if run.success() {
        TAG_NULL_F64
    } else {
        cp_exec_callback_error(&run, &cp_file_cmd_display(&file_str, &arg_strs))
    };
    crate::closure::js_closure_call3(cb, err_val, stdout_box, stderr_box);
    f64::from_bits(TAG_UNDEFINED_BITS)
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
    cp_apply_options(&mut command, opts_val);
    let run = cp_run_to_completion(command);

    let stdout_box = cp_box_output(&run.stdout, &mode);
    if run.success() {
        return stdout_box;
    }
    let stderr_box = cp_box_output(&run.stderr, &mode);
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

#[inline]
fn cp_box_string_bytes(bytes: &[u8]) -> f64 {
    let p = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    crate::value::js_nanbox_string(p as i64)
}

#[inline]
fn cp_error_value(msg: &str) -> f64 {
    let mp = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_error_new_with_message(mp);
    crate::value::js_nanbox_pointer(err as i64)
}

fn cp_exec_result_promise(output: std::io::Result<std::process::Output>) -> f64 {
    match output {
        Ok(o) => {
            let stdout_b = cp_box_string_bytes(&o.stdout);
            let stderr_b = cp_box_string_bytes(&o.stderr);
            let obj = unsafe { make_two_field_object("stdout", stdout_b, "stderr", stderr_b) };
            let obj_val = cp_box_ptr(obj as *const u8);
            let promise = if o.status.success() {
                crate::promise::js_promise_resolved(obj_val)
            } else {
                // Node rejects with an Error carrying .stdout/.stderr; the
                // minimal shape (an Error) is enough for the supported cases.
                crate::promise::js_promise_rejected(cp_error_value("Command failed"))
            };
            crate::value::js_nanbox_pointer(promise as i64)
        }
        Err(e) => {
            let promise = crate::promise::js_promise_rejected(cp_error_value(&e.to_string()));
            crate::value::js_nanbox_pointer(promise as i64)
        }
    }
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
    cp_exec_result_promise(command.output())
}

extern "C" fn cp_promisified_exec_file(
    _closure: *const ClosureHeader,
    file_val: f64,
    args_val: f64,
) -> f64 {
    let file = cp_value_to_string(file_val).unwrap_or_default();
    let arg_strs = cp_args_from_value(args_val);
    cp_exec_result_promise(Command::new(&file).args(&arg_strs).output())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exec_sync_echo() {
        let cmd = "echo hello";
        let cmd_ptr = js_string_from_bytes(cmd.as_ptr(), cmd.len() as u32);
        let result = js_child_process_exec_sync(cmd_ptr, std::ptr::null());

        // #1937: execSync returns a Buffer by default; verify it carries the
        // echoed bytes.
        assert!(JSValue::from_bits(result.to_bits()).is_pointer());
        let buf =
            (result.to_bits() & crate::value::POINTER_MASK) as *const crate::buffer::BufferHeader;
        assert!(!buf.is_null());
        unsafe {
            assert!((*buf).length > 0);
        }
    }

    #[test]
    fn test_spawn_sync_result_fields() {
        // #1936: spawnSync result carries pid / output / stdout / stderr /
        // status / signal.
        let cmd = "echo";
        let cmd_ptr = js_string_from_bytes(cmd.as_ptr(), cmd.len() as u32);
        let args = crate::array::js_array_alloc(1);
        let hi = js_string_from_bytes(b"hi".as_ptr(), 2);
        crate::array::js_array_push_f64(args, crate::value::js_nanbox_string(hi as i64));

        let result = js_child_process_spawn_sync(cmd_ptr, args, std::ptr::null());
        assert!(!result.is_null());
        let get = |name: &[u8]| -> f64 {
            let k = js_string_from_bytes(name.as_ptr(), name.len() as u32);
            js_object_get_field_by_name_f64(result, k)
        };
        // status should be the numeric exit code 0.
        assert_eq!(get(b"status"), 0.0);
        // pid is a positive number.
        assert!(get(b"pid") > 0.0);
        // output / stdout / stderr are present (pointers, not undefined).
        for f in [
            b"output".as_slice(),
            b"stdout".as_slice(),
            b"stderr".as_slice(),
        ] {
            assert!(JSValue::from_bits(get(f).to_bits()).is_pointer());
        }
        // signal is null on a clean exit.
        assert_eq!(get(b"signal").to_bits(), TAG_NULL_BITS);
    }
}
