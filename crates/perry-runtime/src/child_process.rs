//! Child Process module - provides process spawning capabilities

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

fn cp_run_command_capture(mut command: Command) -> std::io::Result<(std::process::Output, u32)> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = command.spawn()?;
    let pid = child.id();
    let output = child.wait_with_output()?;
    Ok((output, pid))
}

#[cfg(unix)]
fn cp_exit_signal(status: std::process::ExitStatus) -> Option<&'static str> {
    use std::os::unix::process::ExitStatusExt;
    status.signal().map(cp_signal_name)
}

#[cfg(not(unix))]
fn cp_exit_signal(_status: std::process::ExitStatus) -> Option<&'static str> {
    None
}

fn cp_output_array(stdout: &[u8], stderr: &[u8]) -> f64 {
    use crate::array::{js_array_alloc, js_array_push_f64};

    let output = js_array_alloc(3);
    js_array_push_f64(output, TAG_NULL_F64);
    js_array_push_f64(output, cp_box_string_bytes(stdout));
    js_array_push_f64(output, cp_box_string_bytes(stderr));
    crate::value::js_nanbox_pointer(output as i64)
}

fn cp_throw_sync_nonzero_exit(output: std::process::Output, pid: u32) -> ! {
    let obj = crate::object::js_object_alloc(crate::error::CLASS_ID_ERROR, 6);
    let status = output
        .status
        .code()
        .map_or(TAG_NULL_F64, |code| code as f64);
    let signal = cp_exit_signal(output.status).map_or(TAG_NULL_F64, cp_box_string);
    js_object_set_field_by_name(obj, cp_str_key(b"status"), status);
    js_object_set_field_by_name(obj, cp_str_key(b"signal"), signal);
    js_object_set_field_by_name(
        obj,
        cp_str_key(b"output"),
        cp_output_array(&output.stdout, &output.stderr),
    );
    js_object_set_field_by_name(obj, cp_str_key(b"pid"), pid as f64);
    js_object_set_field_by_name(
        obj,
        cp_str_key(b"stdout"),
        cp_box_string_bytes(&output.stdout),
    );
    js_object_set_field_by_name(
        obj,
        cp_str_key(b"stderr"),
        cp_box_string_bytes(&output.stderr),
    );
    crate::exception::js_throw(crate::value::js_nanbox_pointer(obj as i64))
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

/// Execute a command synchronously and return stdout as a buffer/string
/// Returns: Buffer containing stdout, or null on error
#[no_mangle]
pub extern "C" fn js_child_process_exec_sync(
    cmd_ptr: *const StringHeader,
    options_ptr: *const ObjectHeader,
) -> *mut StringHeader {
    if cmd_ptr.is_null() {
        return js_string_from_bytes(b"".as_ptr(), 0);
    }

    unsafe {
        let len = (*cmd_ptr).byte_len as usize;
        let data_ptr = (cmd_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let cmd_bytes = std::slice::from_raw_parts(data_ptr, len);

        let cmd_str = match std::str::from_utf8(cmd_bytes) {
            Ok(s) => s,
            Err(_) => return js_string_from_bytes(b"".as_ptr(), 0),
        };

        // Execute the command using the shell, honoring `cwd`/`env` options.
        #[cfg(unix)]
        let mut command = {
            let mut c = Command::new("sh");
            c.arg("-c").arg(cmd_str);
            c
        };
        #[cfg(windows)]
        let mut command = {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(cmd_str);
            c
        };
        if !options_ptr.is_null() {
            cp_apply_options(&mut command, cp_box_ptr(options_ptr as *const u8));
        }

        match cp_run_command_capture(command) {
            Ok(output) => {
                let (output, pid) = output;
                if !output.status.success() {
                    cp_throw_sync_nonzero_exit(output, pid);
                }
                // Return stdout as a string
                let stdout = &output.stdout;
                js_string_from_bytes(stdout.as_ptr(), stdout.len() as u32)
            }
            Err(_) => js_string_from_bytes(b"".as_ptr(), 0),
        }
    }
}

/// Execute a command synchronously with more control (spawnSync)
/// Returns: Object with stdout, stderr, status, etc.
#[no_mangle]
pub extern "C" fn js_child_process_spawn_sync(
    cmd_ptr: *const StringHeader,
    args_ptr: *const crate::array::ArrayHeader,
    options_ptr: *const ObjectHeader,
) -> *mut ObjectHeader {
    if cmd_ptr.is_null() {
        return std::ptr::null_mut();
    }

    unsafe {
        // Get command string
        let cmd_len = (*cmd_ptr).byte_len as usize;
        let cmd_data = (cmd_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let cmd_bytes = std::slice::from_raw_parts(cmd_data, cmd_len);

        let cmd_str = match std::str::from_utf8(cmd_bytes) {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };

        // Build command
        let mut command = Command::new(cmd_str);

        // Add arguments if provided
        if !args_ptr.is_null() {
            let args_len = (*args_ptr).length as usize;
            let args_data = (args_ptr as *const u8)
                .add(std::mem::size_of::<crate::array::ArrayHeader>())
                as *const f64;

            for i in 0..args_len {
                let arg_val = *args_data.add(i);
                if let Some(arg_str) = extract_string_from_nanboxed(arg_val) {
                    command.arg(arg_str);
                }
            }
        }

        // Honor `cwd`/`env` options.
        if !options_ptr.is_null() {
            cp_apply_options(&mut command, cp_box_ptr(options_ptr as *const u8));
        }

        // Execute the command
        match command.output() {
            Ok(output) => {
                use crate::array::{js_array_alloc, js_array_push_f64};
                use crate::value::js_nanbox_string;

                // Create result object with stdout, stderr, status (3 fields)
                let result = crate::object::js_object_alloc(0, 3);

                // Set stdout as string (field 0)
                let stdout_str =
                    js_string_from_bytes(output.stdout.as_ptr(), output.stdout.len() as u32);
                let stdout_boxed = js_nanbox_string(stdout_str as i64);
                crate::object::js_object_set_field_f64(result, 0, stdout_boxed);

                // Set stderr as string (field 1)
                let stderr_str =
                    js_string_from_bytes(output.stderr.as_ptr(), output.stderr.len() as u32);
                let stderr_boxed = js_nanbox_string(stderr_str as i64);
                crate::object::js_object_set_field_f64(result, 1, stderr_boxed);

                // Set status (field 2)
                let status = output.status.code().unwrap_or(-1) as f64;
                crate::object::js_object_set_field_f64(result, 2, status);

                // Build keys array for named property access
                let keys = js_array_alloc(3);
                let k_stdout = js_string_from_bytes(b"stdout".as_ptr(), 6);
                let k_stderr = js_string_from_bytes(b"stderr".as_ptr(), 6);
                let k_status = js_string_from_bytes(b"status".as_ptr(), 6);
                js_array_push_f64(keys, js_nanbox_string(k_stdout as i64));
                js_array_push_f64(keys, js_nanbox_string(k_stderr as i64));
                js_array_push_f64(keys, js_nanbox_string(k_status as i64));
                crate::object::js_object_set_keys(result, keys);

                result
            }
            Err(_) => std::ptr::null_mut(),
        }
    }
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

    if cmd_ptr.is_null() {
        if cb.is_null() {
            return cp_box_output(b"", arg1);
        }
        let output = cp_box_output(b"", arg1);
        crate::closure::js_closure_call3(cb, TAG_NULL_F64, output, output);
        return f64::from_bits(TAG_UNDEFINED_BITS);
    }

    let (stdout_bytes, stderr_bytes, success) = unsafe {
        let len = (*cmd_ptr).byte_len as usize;
        let data_ptr = (cmd_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let cmd_bytes = std::slice::from_raw_parts(data_ptr, len);
        match std::str::from_utf8(cmd_bytes) {
            Ok(cmd_str) => {
                // `exec` always runs through the shell. The options object sits
                // in the `arg1` slot (`exec(cmd, options, cb)`); when `arg1` is
                // the callback (`exec(cmd, cb)`) it's a closure, so the helper
                // no-ops. `cwd`/`env` from the options are applied here.
                #[cfg(unix)]
                let mut command = {
                    let mut c = Command::new("sh");
                    c.arg("-c").arg(cmd_str);
                    c
                };
                #[cfg(windows)]
                let mut command = {
                    let mut c = Command::new("cmd");
                    c.arg("/C").arg(cmd_str);
                    c
                };
                cp_apply_options(&mut command, arg1);
                match command.output() {
                    Ok(o) => (o.stdout, o.stderr, o.status.success()),
                    Err(e) => (Vec::new(), e.to_string().into_bytes(), false),
                }
            }
            Err(_) => (Vec::new(), Vec::new(), false),
        }
    };

    let stdout_box = cp_box_output(&stdout_bytes, arg1);
    if cb.is_null() {
        // Legacy no-callback shape — return the stdout string.
        return stdout_box;
    }

    let stderr_box = cp_box_output(&stderr_bytes, arg1);
    let err_val = if success {
        TAG_NULL_F64
    } else {
        let msg = "Command failed";
        let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err_ptr = crate::error::js_error_new_with_message(msg_ptr);
        crate::value::js_nanbox_pointer(err_ptr as i64)
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

fn cp_encoding_wants_buffer(opts_val: f64) -> bool {
    if cp_object_ptr(opts_val).is_none() {
        return false;
    }
    let encoding = cp_get_field(opts_val, b"encoding");
    if encoding.to_bits() == TAG_NULL_BITS {
        return true;
    }
    match cp_value_to_string(encoding) {
        Some(s) => s.eq_ignore_ascii_case("buffer"),
        None => false,
    }
}

fn cp_box_output(bytes: &[u8], opts_val: f64) -> f64 {
    if cp_encoding_wants_buffer(opts_val) {
        cp_make_buffer(bytes)
    } else {
        cp_box_string_bytes(bytes)
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
extern "C" fn cp_method_kill(closure: *const ClosureHeader, _signal: f64) -> f64 {
    cp_set_field(cp_this(closure), b"killed", TAG_TRUE_F64);
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
extern "C" fn cp_method_write2(_closure: *const ClosureHeader, _chunk: f64, _enc: f64) -> f64 {
    TAG_TRUE_F64
}

/// Deferred emission body, scheduled via `setImmediate` from `spawn`. Slot 0
/// captures the ChildProcess value.
extern "C" fn cp_emit_all(closure: *const ClosureHeader) -> f64 {
    let cp = cp_this(closure);

    // A spawn failure (e.g. ENOENT) surfaces as a single `error` event; Node
    // never fires `spawn`/`exit` in that case.
    let err = cp_get_field(cp, b"__cpError");
    if !JSValue::from_bits(err.to_bits()).is_undefined() {
        cp_emit(cp, "error", &[err]);
        return cp_undefined();
    }

    cp_emit(cp, "spawn", &[]);

    // Flush each output stream: a single `data` chunk (when non-empty) then `end`.
    for field in [b"stdout".as_slice(), b"stderr".as_slice()] {
        let stream = cp_get_field(cp, field);
        if cp_object_ptr(stream).is_none() {
            continue;
        }
        let chunk = cp_get_field(stream, b"__cpChunk");
        if !JSValue::from_bits(chunk.to_bits()).is_undefined() {
            cp_emit(stream, "data", &[chunk]);
        }
        cp_emit(stream, "end", &[]);
    }

    // Node populates `exitCode`/`signalCode` before emitting `exit`, then `close`.
    let code = cp_get_field(cp, b"__cpExitCode");
    let signal = cp_get_field(cp, b"__cpSignal");
    cp_set_field(cp, b"exitCode", code);
    cp_set_field(cp, b"signalCode", signal);
    cp_emit(cp, "exit", &[code, signal]);
    cp_emit(cp, "close", &[code, signal]);
    cp_undefined()
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
    js_register_closure_arity(cp_emit_all as *const u8, 0);
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
        ("end", cp_cast1(cp_method_this1)),
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

/// `child_process.spawn(command[, args][, options])` — returns a streaming
/// ChildProcess. `cmd_ptr`/`args_ptr` are raw (unboxed) `StringHeader` /
/// `ArrayHeader` pointers; `opts_ptr` is currently unused (no shell, default
/// stdio). #1780.
#[no_mangle]
pub extern "C" fn js_child_process_spawn_streams(
    cmd_ptr: i64,
    args_ptr: i64,
    opts_ptr: i64,
) -> f64 {
    cp_register_arities();

    let (cmd_str, arg_strs) = unsafe {
        (
            cp_read_string_header(cmd_ptr),
            cp_read_arg_strings(args_ptr),
        )
    };

    // `opts_ptr` arrives as a raw (unboxed) heap pointer; re-box it so the
    // options helpers can read `cwd`/`env`/`shell`. Small values mean
    // no-options (codegen passes 0) — leave it undefined then.
    let opts_val = if opts_ptr > 0x10000 {
        cp_box_ptr(opts_ptr as *const u8)
    } else {
        cp_undefined()
    };

    // Build the command (honoring `shell`/`cwd`/`env`) and capture its output
    // synchronously.
    let mut command = cp_build_command(&cmd_str, &arg_strs, opts_val);
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let (pid, stdout_bytes, stderr_bytes, exit_code, signal_opt, spawn_err) = match command.spawn()
    {
        Ok(child) => {
            let pid = child.id();
            match child.wait_with_output() {
                Ok(out) => {
                    let code = out.status.code();
                    #[cfg(unix)]
                    let sig = {
                        use std::os::unix::process::ExitStatusExt;
                        out.status.signal()
                    };
                    #[cfg(not(unix))]
                    let sig: Option<i32> = None;
                    (Some(pid), out.stdout, out.stderr, code, sig, None)
                }
                Err(e) => (
                    Some(pid),
                    Vec::new(),
                    Vec::new(),
                    None,
                    None,
                    Some(e.to_string()),
                ),
            }
        }
        Err(e) => (
            None,
            Vec::new(),
            Vec::new(),
            None,
            None,
            Some(e.to_string()),
        ),
    };

    // stdout/stderr Readable + stdin Writable sub-objects.
    let stdout_obj = cp_build_readable();
    let stderr_obj = cp_build_readable();
    let stdin_obj = cp_build_writable();
    if !stdout_bytes.is_empty() {
        cp_set_field(stdout_obj, b"__cpChunk", cp_make_buffer(&stdout_bytes));
    }
    if !stderr_bytes.is_empty() {
        cp_set_field(stderr_obj, b"__cpChunk", cp_make_buffer(&stderr_bytes));
    }

    // spawnargs = [command, ...args]
    let mut spawnargs = crate::array::js_array_alloc((arg_strs.len() + 1) as u32);
    spawnargs = crate::array::js_array_push_f64(spawnargs, cp_box_string(&cmd_str));
    for a in &arg_strs {
        spawnargs = crate::array::js_array_push_f64(spawnargs, cp_box_string(a));
    }

    let cp_methods: [(&str, CpFn); 11] = [
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
    ];
    let cp_obj = cp_build_object(&cp_methods, CP_SHAPE_ID + cp_methods.len() as u32);
    let cp = cp_box_ptr(cp_obj as *const u8);

    cp_set_field(cp, b"stdout", stdout_obj);
    cp_set_field(cp, b"stderr", stderr_obj);
    cp_set_field(cp, b"stdin", stdin_obj);

    let mut stdio = crate::array::js_array_alloc(3);
    stdio = crate::array::js_array_push_f64(stdio, stdin_obj);
    stdio = crate::array::js_array_push_f64(stdio, stdout_obj);
    stdio = crate::array::js_array_push_f64(stdio, stderr_obj);
    cp_set_field(cp, b"stdio", cp_box_ptr(stdio as *const u8));

    cp_set_field(
        cp,
        b"pid",
        match pid {
            Some(p) => p as f64,
            None => cp_undefined(),
        },
    );
    cp_set_field(cp, b"exitCode", TAG_NULL_F64);
    cp_set_field(cp, b"signalCode", TAG_NULL_F64);
    cp_set_field(cp, b"killed", TAG_FALSE_F64);
    cp_set_field(cp, b"connected", TAG_FALSE_F64);
    cp_set_field(cp, b"spawnargs", cp_box_ptr(spawnargs as *const u8));
    cp_set_field(cp, b"spawnfile", cp_box_string(&cmd_str));

    // Hidden state consumed by the deferred emission.
    cp_set_field(
        cp,
        b"__cpExitCode",
        match exit_code {
            Some(c) => c as f64,
            None => TAG_NULL_F64,
        },
    );
    cp_set_field(
        cp,
        b"__cpSignal",
        match signal_opt {
            Some(s) => cp_box_string(cp_signal_name(s)),
            None => TAG_NULL_F64,
        },
    );
    if let Some(msg) = spawn_err {
        let mp = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err = crate::error::js_error_new_with_message(mp);
        cp_set_field(
            cp,
            b"__cpError",
            crate::value::js_nanbox_pointer(err as i64),
        );
    }

    // Defer spawn/data/end/exit/close until the next tick so handlers registered
    // synchronously after this call are present when the events fire.
    let emit_closure = js_closure_alloc(cp_emit_all as *const u8, 1);
    js_closure_set_capture_ptr(emit_closure, 0, cp.to_bits() as i64);
    crate::timer::js_set_immediate_callback(emit_closure as i64);

    cp
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

/// `child_process.execFile(file[, args][, options][, callback])` — like `exec`
/// but runs `file` directly (no shell). The callback fires with
/// `(err, stdout, stderr)`; with no callback the stdout string is returned.
/// The callback may sit in the options slot (`execFile(file, args, cb)`), so it
/// is located the same way `exec` disambiguates. #1780.
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

    // `cwd`/`env` come from the options slot; when `opts_val` is the callback
    // (`execFile(file, args, cb)`) it's a closure, so the helper no-ops.
    let mut command = Command::new(&file_str);
    command.args(&arg_strs);
    cp_apply_options(&mut command, opts_val);
    let (stdout_bytes, stderr_bytes, success) = match command.output() {
        Ok(o) => (o.stdout, o.stderr, o.status.success()),
        Err(e) => (Vec::new(), e.to_string().into_bytes(), false),
    };

    let stdout_box = cp_box_output(&stdout_bytes, opts_val);
    if cb.is_null() {
        return stdout_box;
    }
    let stderr_box = cp_box_output(&stderr_bytes, opts_val);
    let err_val = if success {
        TAG_NULL_F64
    } else {
        let msg = "Command failed";
        let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err_ptr = crate::error::js_error_new_with_message(msg_ptr);
        crate::value::js_nanbox_pointer(err_ptr as i64)
    };
    crate::closure::js_closure_call3(cb, err_val, stdout_box, stderr_box);
    f64::from_bits(TAG_UNDEFINED_BITS)
}

/// `child_process.execFileSync(file[, args][, options])` — runs `file`
/// directly (no shell) and returns its stdout. #1780.
#[no_mangle]
pub extern "C" fn js_child_process_exec_file_sync(
    file_ptr: i64,
    args_val: f64,
    opts_val: f64,
) -> *mut StringHeader {
    let file_str = unsafe { cp_read_string_header(file_ptr) };
    if file_str.is_empty() {
        return js_string_from_bytes(b"".as_ptr(), 0);
    }
    let arg_strs = cp_args_from_value(args_val);
    let mut command = Command::new(&file_str);
    command.args(&arg_strs);
    cp_apply_options(&mut command, opts_val);
    match cp_run_command_capture(command) {
        Ok((o, pid)) => {
            if !o.status.success() {
                cp_throw_sync_nonzero_exit(o, pid);
            }
            js_string_from_bytes(o.stdout.as_ptr(), o.stdout.len() as u32)
        }
        Err(_) => js_string_from_bytes(b"".as_ptr(), 0),
    }
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

        assert!(!result.is_null());
        unsafe {
            assert!((*result).byte_len > 0);
        }
    }
}
