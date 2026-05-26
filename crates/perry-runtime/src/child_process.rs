//! Child Process module - provides process spawning capabilities

use std::collections::HashMap;
use std::fs::File;
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};

use crate::object::ObjectHeader;
use crate::string::{js_string_from_bytes, StringHeader};

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

/// Execute a command synchronously and return stdout as a buffer/string
/// Returns: Buffer containing stdout, or null on error
#[no_mangle]
pub extern "C" fn js_child_process_exec_sync(
    cmd_ptr: *const StringHeader,
    _options_ptr: *const ObjectHeader,
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

        // Execute the command using shell
        #[cfg(unix)]
        let output = Command::new("sh").arg("-c").arg(cmd_str).output();

        #[cfg(windows)]
        let output = Command::new("cmd").arg("/C").arg(cmd_str).output();

        match output {
            Ok(output) => {
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
    _options_ptr: *const ObjectHeader,
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

    let box_string = |bytes: &[u8]| -> f64 {
        let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        crate::value::js_nanbox_string(ptr as i64)
    };

    if cmd_ptr.is_null() {
        if cb.is_null() {
            return box_string(b"");
        }
        crate::closure::js_closure_call3(cb, TAG_NULL_F64, box_string(b""), box_string(b""));
        return f64::from_bits(TAG_UNDEFINED_BITS);
    }

    let (stdout_bytes, stderr_bytes, success) = unsafe {
        let len = (*cmd_ptr).byte_len as usize;
        let data_ptr = (cmd_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let cmd_bytes = std::slice::from_raw_parts(data_ptr, len);
        match std::str::from_utf8(cmd_bytes) {
            Ok(cmd_str) => {
                #[cfg(unix)]
                let output = Command::new("sh").arg("-c").arg(cmd_str).output();
                #[cfg(windows)]
                let output = Command::new("cmd").arg("/C").arg(cmd_str).output();
                match output {
                    Ok(o) => (o.stdout, o.stderr, o.status.success()),
                    Err(e) => (Vec::new(), e.to_string().into_bytes(), false),
                }
            }
            Err(_) => (Vec::new(), Vec::new(), false),
        }
    };

    let stdout_box = box_string(&stdout_bytes);
    if cb.is_null() {
        // Legacy no-callback shape — return the stdout string.
        return stdout_box;
    }

    let stderr_box = box_string(&stderr_bytes);
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
