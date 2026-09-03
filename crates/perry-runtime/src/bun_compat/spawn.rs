//! Bun child-process facade (#9601).
//!
//! The process engine stays in `child_process::reactor` and the POSIX PTY
//! engine stays in `pty`; this module translates Bun's option shapes and adds
//! the Bun-facing objects (`Subprocess`, stream consumers, and `Terminal`).

use super::*;
use crate::child_process::{
    cp_array_ptr, cp_box_ptr, cp_box_string, cp_build_object, cp_cast0, cp_cast1, cp_cast2,
    cp_get_field, cp_object_ptr, cp_set_field, cp_undefined, CpFn, TAG_FALSE_F64, TAG_NULL_F64,
    TAG_TRUE_F64,
};
use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64,
    js_register_closure_arity, ClosureHeader,
};
use crate::value::JSValue;

#[cfg(unix)]
use std::os::fd::AsRawFd;

const BUN_TERMINAL_SHAPE_ID: u32 = 0x7FFF_FC40;
const BUN_PTY_SUBPROCESS_SHAPE_ID: u32 = 0x7FFF_FC60;

const TERMINAL_MARKER: &[u8] = b"__perryBunTerminal";
const TERMINAL_CURRENT: &[u8] = b"__perryBunTerminalCurrent";
const TERMINAL_DATA_CB: &[u8] = b"__perryBunTerminalData";
const TERMINAL_EXIT_CB: &[u8] = b"__perryBunTerminalExit";
const TERMINAL_DRAIN_CB: &[u8] = b"__perryBunTerminalDrain";
const TERMINAL_REFED: &[u8] = b"__perryBunTerminalRefed";
const SUBPROCESS_ON_EXIT: &[u8] = b"__perryBunOnExit";

fn is_undefined(value: f64) -> bool {
    JSValue::from_bits(value.to_bits()).is_undefined()
}

fn is_nullish(value: f64) -> bool {
    let value = JSValue::from_bits(value.to_bits());
    value.is_undefined() || value.is_null()
}

fn is_callable(value: f64) -> bool {
    !crate::fs::extract_closure_ptr(value).is_null()
}

fn number_i32(value: f64) -> Option<i32> {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_int32() {
        return Some(js.as_int32());
    }
    if js.is_number()
        && value.is_finite()
        && value.fract() == 0.0
        && value >= i32::MIN as f64
        && value <= i32::MAX as f64
    {
        return Some(value as i32);
    }
    None
}

fn bool_field(object: f64, key: &[u8]) -> bool {
    cp_get_field(object, key).to_bits() == TAG_TRUE_F64.to_bits()
}

fn call_value(callback: f64, args: &[f64]) -> f64 {
    if !is_callable(callback) {
        return cp_undefined();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback = scope.root_nanbox_f64(callback);
    let args = scope.root_nanbox_f64_slice(args);
    let args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&args);
    unsafe {
        crate::closure::js_native_call_value(callback.get_nanbox_f64(), args.as_ptr(), args.len())
    }
}

fn call_method(receiver: f64, name: &[u8], args: &[f64]) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let method = cp_get_field(receiver.get_nanbox_f64(), name);
    if !is_callable(method) {
        return cp_undefined();
    }
    let method = scope.root_nanbox_f64(method);
    let args = scope.root_nanbox_f64_slice(args);
    let previous = scope.root_nanbox_f64(crate::object::js_implicit_this_set(
        receiver.get_nanbox_f64(),
    ));
    let args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&args);
    let result = unsafe {
        crate::closure::js_native_call_value(method.get_nanbox_f64(), args.as_ptr(), args.len())
    };
    crate::object::js_implicit_this_set(previous.get_nanbox_f64());
    result
}

fn closure_with_captures(func: *const u8, arity: u32, captures: &[f64]) -> f64 {
    js_register_closure_arity(func, arity);
    let scope = crate::gc::RuntimeHandleScope::new();
    let captures = scope.root_nanbox_f64_slice(captures);
    let closure = js_closure_alloc(func, captures.len() as u32);
    for (index, value) in captures.iter().enumerate() {
        js_closure_set_capture_f64(closure, index as u32, value.get_nanbox_f64());
    }
    cp_box_ptr(closure as *const u8)
}

fn register_bun_spawn_arities() {
    js_register_closure_arity(bun_terminal_write as *const u8, 1);
    js_register_closure_arity(bun_terminal_resize as *const u8, 2);
    js_register_closure_arity(bun_terminal_set_raw_mode as *const u8, 1);
    js_register_closure_arity(bun_terminal_ref as *const u8, 0);
    js_register_closure_arity(bun_terminal_unref as *const u8, 0);
    js_register_closure_arity(bun_terminal_close as *const u8, 0);
    #[cfg(unix)]
    {
        js_register_closure_arity(bun_pty_subprocess_kill as *const u8, 1);
        js_register_closure_arity(bun_pty_subprocess_ref as *const u8, 0);
        js_register_closure_arity(bun_pty_subprocess_unref as *const u8, 0);
        js_register_closure_arity(bun_pty_subprocess_dispose as *const u8, 0);
    }
}

fn captured_at(closure: *const ClosureHeader, index: u32) -> f64 {
    js_closure_get_capture_f64(closure, index)
}

fn promise_pointer(value: f64) -> *mut crate::promise::Promise {
    crate::value::js_nanbox_get_pointer(value) as *mut crate::promise::Promise
}

fn new_pending_promise_value() -> f64 {
    cp_box_ptr(crate::promise::js_promise_new() as *const u8)
}

fn type_error(message: &str) -> f64 {
    let message = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    cp_box_ptr(crate::error::js_typeerror_new(message) as *const u8)
}

fn plain_object(capacity: u32) -> f64 {
    cp_box_ptr(js_object_alloc(0, capacity) as *const u8)
}

/// Clone the enumerable own fields of an option bag. Bun's stdio translation
/// must not mutate the caller's object.
fn clone_options(options: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let source = scope.root_nanbox_f64(options);
    let out = scope.root_nanbox_f64(plain_object(16));
    let Some(source_obj) = cp_object_ptr(source.get_nanbox_f64()) else {
        return out.get_nanbox_f64();
    };
    let keys = crate::object::js_object_keys(source_obj);
    if keys.is_null() {
        return out.get_nanbox_f64();
    }
    let keys = scope.root_nanbox_f64(cp_box_ptr(keys as *const u8));
    let length = cp_array_ptr(keys.get_nanbox_f64())
        .map(|array| crate::array::js_array_length(array))
        .unwrap_or(0);
    for index in 0..length {
        let Some(keys_array) = cp_array_ptr(keys.get_nanbox_f64()) else {
            break;
        };
        let key_value = crate::array::js_array_get_f64(keys_array, index);
        let Some(key) = crate::child_process::cp_value_to_string(key_value) else {
            continue;
        };
        let value = scope.root_nanbox_f64(cp_get_field(source.get_nanbox_f64(), key.as_bytes()));
        cp_set_field(out.get_nanbox_f64(), key.as_bytes(), value.get_nanbox_f64());
    }
    out.get_nanbox_f64()
}

// -------------------------------------------------------------------------
// Readable / writable stream facade
// -------------------------------------------------------------------------

extern "C" fn bun_readable_text(closure: *const ClosureHeader) -> f64 {
    crate::node_submodules::consume_text(captured_at(closure, 0))
}

extern "C" fn bun_readable_json(closure: *const ClosureHeader) -> f64 {
    crate::node_submodules::consume_json(captured_at(closure, 0))
}

extern "C" fn bun_readable_array_buffer(closure: *const ClosureHeader) -> f64 {
    crate::node_submodules::consume_array_buffer(captured_at(closure, 0))
}

extern "C" fn bun_readable_bytes(closure: *const ClosureHeader) -> f64 {
    crate::node_submodules::consume_bytes(captured_at(closure, 0))
}

fn decorate_readable(stream: f64) -> f64 {
    if cp_object_ptr(stream).is_none() {
        return stream;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    for (name, function) in [
        (b"text".as_slice(), bun_readable_text as *const u8),
        (b"json".as_slice(), bun_readable_json as *const u8),
        (
            b"arrayBuffer".as_slice(),
            bun_readable_array_buffer as *const u8,
        ),
        (b"bytes".as_slice(), bun_readable_bytes as *const u8),
    ] {
        let method = scope.root_nanbox_f64(closure_with_captures(
            function,
            0,
            &[stream.get_nanbox_f64()],
        ));
        cp_set_field(stream.get_nanbox_f64(), name, method.get_nanbox_f64());
    }
    stream.get_nanbox_f64()
}

extern "C" fn bun_sink_flush(_closure: *const ClosureHeader, _wait: f64) -> f64 {
    0.0
}

fn decorate_sink(stream: f64) -> f64 {
    if cp_object_ptr(stream).is_some() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let stream = scope.root_nanbox_f64(stream);
        let flush = scope.root_nanbox_f64(closure_with_captures(
            bun_sink_flush as *const u8,
            1,
            &[stream.get_nanbox_f64()],
        ));
        cp_set_field(stream.get_nanbox_f64(), b"flush", flush.get_nanbox_f64());
        return stream.get_nanbox_f64();
    }
    stream
}

// -------------------------------------------------------------------------
// Bun stdio translation
// -------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum StdioSpec {
    Pipe,
    Ignore,
    Inherit,
    Fd(i32),
}

fn stdio_spec(
    value: f64,
    fd_index: usize,
    held: &mut Vec<std::fs::File>,
) -> Result<StdioSpec, f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let value_now = value.get_nanbox_f64();
    let js = JSValue::from_bits(value_now.to_bits());
    if js.is_undefined() {
        return Ok(match fd_index {
            0 => StdioSpec::Ignore,
            1 => StdioSpec::Pipe,
            _ => StdioSpec::Inherit,
        });
    }
    if js.is_null() {
        return Ok(StdioSpec::Ignore);
    }
    if let Some(fd) = number_i32(value_now).filter(|fd| *fd >= 0) {
        return Ok(StdioSpec::Fd(fd));
    }
    if js.is_any_string() {
        return Ok(match value_to_string(value_now).as_str() {
            "ignore" => StdioSpec::Ignore,
            "inherit" => StdioSpec::Inherit,
            _ => StdioSpec::Pipe,
        });
    }
    if cp_object_ptr(value_now).is_some() {
        let std_fd = cp_get_field(value.get_nanbox_f64(), BUN_STD_FD_KEY);
        if let Some(fd) = number_i32(std_fd).filter(|fd| *fd >= 0) {
            return Ok(StdioSpec::Fd(fd));
        }
        let path = cp_get_field(value.get_nanbox_f64(), BUN_FILE_PATH_KEY);
        if !is_undefined(path) {
            let path = value_to_string(path);
            #[cfg(unix)]
            {
                let opened = if fd_index == 0 {
                    std::fs::File::open(&path)
                } else {
                    if let Some(parent) = std::path::Path::new(&path).parent() {
                        if !parent.as_os_str().is_empty() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                    }
                    std::fs::OpenOptions::new()
                        .create(true)
                        .truncate(true)
                        .write(true)
                        .open(&path)
                };
                return match opened {
                    Ok(file) => {
                        let fd = file.as_raw_fd();
                        held.push(file);
                        Ok(StdioSpec::Fd(fd))
                    }
                    Err(error) => Err(unsafe {
                        crate::fs::build_fs_error_value(
                            &error,
                            if fd_index == 0 { "open" } else { "write" },
                            &path,
                        )
                    }),
                };
            }
            #[cfg(not(unix))]
            {
                let _ = (fd_index, held);
                return Err(type_error(&format!(
                    "Bun.spawn: Bun.file stdio is not supported on this platform ({path})"
                )));
            }
        }
    }
    Ok(StdioSpec::Pipe)
}

fn stdio_value(spec: StdioSpec) -> f64 {
    match spec {
        StdioSpec::Pipe => cp_box_string("pipe"),
        StdioSpec::Ignore => cp_box_string("ignore"),
        StdioSpec::Inherit => cp_box_string("inherit"),
        StdioSpec::Fd(fd) => fd as f64,
    }
}

fn normalized_options(options: f64) -> Result<(f64, Vec<std::fs::File>), f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let source = scope.root_nanbox_f64(options);
    let out = scope.root_nanbox_f64(clone_options(source.get_nanbox_f64()));
    let mut held = Vec::new();
    let mut specs = [StdioSpec::Ignore, StdioSpec::Pipe, StdioSpec::Inherit];

    let stdio = cp_get_field(source.get_nanbox_f64(), b"stdio");
    if let Some(array) = cp_array_ptr(stdio) {
        let count = crate::array::js_array_length(array).min(3);
        for index in 0..count {
            let Some(array) = cp_array_ptr(cp_get_field(source.get_nanbox_f64(), b"stdio")) else {
                break;
            };
            specs[index as usize] = stdio_spec(
                crate::array::js_array_get_f64(array, index),
                index as usize,
                &mut held,
            )?;
        }
    }

    for (index, name) in [
        b"stdin".as_slice(),
        b"stdout".as_slice(),
        b"stderr".as_slice(),
    ]
    .into_iter()
    .enumerate()
    {
        let value = cp_get_field(source.get_nanbox_f64(), name);
        if !is_undefined(value) {
            specs[index] = stdio_spec(value, index, &mut held)?;
        }
    }

    let stdio_array =
        scope.root_nanbox_f64(cp_box_ptr(crate::array::js_array_alloc(3) as *const u8));
    for spec in specs {
        let value = scope.root_nanbox_f64(stdio_value(spec));
        let Some(array) = cp_array_ptr(stdio_array.get_nanbox_f64()) else {
            break;
        };
        let array = crate::array::js_array_push_f64(array, value.get_nanbox_f64());
        stdio_array.set_nanbox_f64(cp_box_ptr(array as *const u8));
    }
    cp_set_field(out.get_nanbox_f64(), b"stdio", stdio_array.get_nanbox_f64());
    Ok((out.get_nanbox_f64(), held))
}

// -------------------------------------------------------------------------
// Non-PTY Subprocess facade
// -------------------------------------------------------------------------

extern "C" fn bun_subprocess_exit(closure: *const ClosureHeader, code: f64, signal: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let subprocess = scope.root_nanbox_f64(captured_at(closure, 0));
    let code = scope.root_nanbox_f64(code);
    let signal = scope.root_nanbox_f64(signal);
    let resolved = if number_i32(code.get_nanbox_f64()).is_some() {
        code.get_nanbox_f64()
    } else {
        1.0
    };
    let promise = cp_get_field(subprocess.get_nanbox_f64(), b"exited");
    let promise = promise_pointer(promise);
    if !promise.is_null() {
        crate::promise::js_promise_resolve(promise, resolved);
    }
    let callback = cp_get_field(subprocess.get_nanbox_f64(), SUBPROCESS_ON_EXIT);
    if is_callable(callback) {
        call_value(
            callback,
            &[
                subprocess.get_nanbox_f64(),
                code.get_nanbox_f64(),
                signal.get_nanbox_f64(),
                cp_undefined(),
            ],
        );
    }
    cp_undefined()
}

fn install_dispose_aliases(object: f64, method: f64, include_async: bool) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_nanbox_f64(object);
    let method = scope.root_nanbox_f64(method);
    cp_set_field(
        object.get_nanbox_f64(),
        b"__perry_dispose__",
        method.get_nanbox_f64(),
    );
    cp_set_field(
        object.get_nanbox_f64(),
        b"@@__perry_wk_dispose",
        method.get_nanbox_f64(),
    );
    let dispose = crate::symbol::well_known_symbol("dispose");
    if !dispose.is_null() {
        unsafe {
            crate::symbol::js_object_set_symbol_property(
                object.get_nanbox_f64(),
                cp_box_ptr(dispose as *const u8),
                method.get_nanbox_f64(),
            );
        }
    }
    if include_async {
        cp_set_field(
            object.get_nanbox_f64(),
            b"__perry_async_dispose__",
            method.get_nanbox_f64(),
        );
        cp_set_field(
            object.get_nanbox_f64(),
            b"@@__perry_wk_asyncDispose",
            method.get_nanbox_f64(),
        );
        let async_dispose = crate::symbol::well_known_symbol("asyncDispose");
        if !async_dispose.is_null() {
            unsafe {
                crate::symbol::js_object_set_symbol_property(
                    object.get_nanbox_f64(),
                    cp_box_ptr(async_dispose as *const u8),
                    method.get_nanbox_f64(),
                );
            }
        }
    }
}

fn finish_non_pty_subprocess(subprocess: f64, options: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let subprocess = scope.root_nanbox_f64(subprocess);
    let options = scope.root_nanbox_f64(options);

    let error = cp_get_field(subprocess.get_nanbox_f64(), b"__cpError");
    if !is_undefined(error) {
        crate::exception::js_throw(error);
    }

    let stdout = scope.root_nanbox_f64(decorate_readable(cp_get_field(
        subprocess.get_nanbox_f64(),
        b"stdout",
    )));
    let stderr = scope.root_nanbox_f64(decorate_readable(cp_get_field(
        subprocess.get_nanbox_f64(),
        b"stderr",
    )));
    let stdin = scope.root_nanbox_f64(decorate_sink(cp_get_field(
        subprocess.get_nanbox_f64(),
        b"stdin",
    )));
    cp_set_field(
        subprocess.get_nanbox_f64(),
        b"stdout",
        stdout.get_nanbox_f64(),
    );
    cp_set_field(
        subprocess.get_nanbox_f64(),
        b"stderr",
        stderr.get_nanbox_f64(),
    );
    cp_set_field(
        subprocess.get_nanbox_f64(),
        b"stdin",
        stdin.get_nanbox_f64(),
    );
    cp_set_field(
        subprocess.get_nanbox_f64(),
        b"readable",
        stdout.get_nanbox_f64(),
    );
    cp_set_field(subprocess.get_nanbox_f64(), b"terminal", cp_undefined());
    let dispose = cp_get_field(subprocess.get_nanbox_f64(), b"__perry_dispose__");
    install_dispose_aliases(subprocess.get_nanbox_f64(), dispose, true);

    let exited = scope.root_nanbox_f64(new_pending_promise_value());
    cp_set_field(
        subprocess.get_nanbox_f64(),
        b"exited",
        exited.get_nanbox_f64(),
    );
    let on_exit = scope.root_nanbox_f64(cp_get_field(options.get_nanbox_f64(), b"onExit"));
    cp_set_field(
        subprocess.get_nanbox_f64(),
        SUBPROCESS_ON_EXIT,
        on_exit.get_nanbox_f64(),
    );
    let listener = scope.root_nanbox_f64(closure_with_captures(
        bun_subprocess_exit as *const u8,
        2,
        &[subprocess.get_nanbox_f64()],
    ));
    let event = scope.root_nanbox_f64(cp_box_string("exit"));
    crate::child_process::cp_register(
        subprocess.get_nanbox_f64(),
        event.get_nanbox_f64(),
        listener.get_nanbox_f64(),
    );
    subprocess.get_nanbox_f64()
}

// -------------------------------------------------------------------------
// Bun.Terminal and PTY-backed Subprocess
// -------------------------------------------------------------------------

#[cfg(unix)]
fn terminal_current(terminal: f64) -> Option<f64> {
    let current = cp_get_field(terminal, TERMINAL_CURRENT);
    (!is_nullish(current)).then_some(current)
}

#[cfg(unix)]
fn terminal_set_refed(terminal: f64, refed: bool) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let terminal = scope.root_nanbox_f64(terminal);
    if let Some(current) = terminal_current(terminal.get_nanbox_f64()) {
        if let Some(handle) = crate::pty::pty_handle_of(current) {
            crate::pty::reactor::pty_live_set_refed(handle, refed);
        }
    }
    cp_set_field(
        terminal.get_nanbox_f64(),
        TERMINAL_REFED,
        if refed { TAG_TRUE_F64 } else { TAG_FALSE_F64 },
    );
}

extern "C" fn bun_terminal_write(closure: *const ClosureHeader, data: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let terminal = scope.root_nanbox_f64(captured_at(closure, 0));
    let data = scope.root_nanbox_f64(data);
    #[cfg(unix)]
    if let Some(current) = terminal_current(terminal.get_nanbox_f64()) {
        let current = scope.root_nanbox_f64(current);
        let bytes = crate::child_process::cp_value_to_bytes(data.get_nanbox_f64());
        let written = bytes.len() as f64;
        let _ = call_method(current.get_nanbox_f64(), b"write", &[data.get_nanbox_f64()]);
        let drain = cp_get_field(terminal.get_nanbox_f64(), TERMINAL_DRAIN_CB);
        if is_callable(drain) {
            call_value(drain, &[terminal.get_nanbox_f64()]);
        }
        return written;
    }
    0.0
}

extern "C" fn bun_terminal_resize(closure: *const ClosureHeader, columns: f64, rows: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let terminal = scope.root_nanbox_f64(captured_at(closure, 0));
    let columns = number_i32(columns).unwrap_or(0);
    let rows = number_i32(rows).unwrap_or(0);
    if columns <= 0 || rows <= 0 || columns > u16::MAX as i32 || rows > u16::MAX as i32 {
        crate::exception::js_throw(type_error(
            "Terminal.resize expects positive columns and rows",
        ));
    }
    cp_set_field(terminal.get_nanbox_f64(), b"cols", columns as f64);
    cp_set_field(terminal.get_nanbox_f64(), b"rows", rows as f64);
    #[cfg(unix)]
    if let Some(current) = terminal_current(terminal.get_nanbox_f64()) {
        let _ = call_method(current, b"resize", &[columns as f64, rows as f64]);
    }
    cp_undefined()
}

extern "C" fn bun_terminal_set_raw_mode(closure: *const ClosureHeader, enabled: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let terminal = scope.root_nanbox_f64(captured_at(closure, 0));
    let enabled = crate::value::js_is_truthy(enabled) != 0;
    cp_set_field(
        terminal.get_nanbox_f64(),
        b"rawMode",
        if enabled { TAG_TRUE_F64 } else { TAG_FALSE_F64 },
    );
    #[cfg(unix)]
    if let Some(current) = terminal_current(terminal.get_nanbox_f64()) {
        if let Some(handle) = crate::pty::pty_handle_of(current) {
            crate::pty::reactor::pty_live_set_raw_mode(handle, enabled);
        }
    }
    cp_undefined()
}

extern "C" fn bun_terminal_ref(closure: *const ClosureHeader) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let terminal = scope.root_nanbox_f64(captured_at(closure, 0));
    #[cfg(unix)]
    terminal_set_refed(terminal.get_nanbox_f64(), true);
    terminal.get_nanbox_f64()
}

extern "C" fn bun_terminal_unref(closure: *const ClosureHeader) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let terminal = scope.root_nanbox_f64(captured_at(closure, 0));
    #[cfg(unix)]
    terminal_set_refed(terminal.get_nanbox_f64(), false);
    terminal.get_nanbox_f64()
}

extern "C" fn bun_terminal_close(closure: *const ClosureHeader) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let terminal = scope.root_nanbox_f64(captured_at(closure, 0));
    if bool_field(terminal.get_nanbox_f64(), b"closed") {
        return cp_undefined();
    }
    #[cfg(unix)]
    if let Some(current) = terminal_current(terminal.get_nanbox_f64()) {
        let current = scope.root_nanbox_f64(current);
        let signal = scope.root_nanbox_f64(cp_box_string("SIGHUP"));
        let _ = call_method(
            current.get_nanbox_f64(),
            b"kill",
            &[signal.get_nanbox_f64()],
        );
    }
    cp_set_field(terminal.get_nanbox_f64(), b"closed", TAG_TRUE_F64);
    cp_set_field(terminal.get_nanbox_f64(), TERMINAL_CURRENT, cp_undefined());
    cp_undefined()
}

fn terminal_options_i32(options: f64, key: &[u8], default: i32) -> i32 {
    number_i32(cp_get_field(options, key))
        .filter(|value| *value > 0 && *value <= u16::MAX as i32)
        .unwrap_or(default)
}

/// `new Bun.Terminal(options)` — a reusable POSIX terminal configuration and
/// callback owner. The underlying PTY is attached lazily by `Bun.spawn`.
#[no_mangle]
pub extern "C" fn js_bun_terminal_new(options: f64) -> f64 {
    register_bun_spawn_arities();
    #[cfg(not(unix))]
    {
        let _ = options;
        crate::exception::js_throw(crate::child_process::cp_make_error(
            "Bun.Terminal is only supported on POSIX platforms by Perry",
            &[],
        ));
    }

    #[cfg(unix)]
    {
        let scope = crate::gc::RuntimeHandleScope::new();
        let options = scope.root_nanbox_f64(options);
        let methods: [(&str, CpFn); 6] = [
            ("write", cp_cast1(bun_terminal_write)),
            ("resize", cp_cast2(bun_terminal_resize)),
            ("setRawMode", cp_cast1(bun_terminal_set_raw_mode)),
            ("ref", cp_cast0(bun_terminal_ref)),
            ("unref", cp_cast0(bun_terminal_unref)),
            ("close", cp_cast0(bun_terminal_close)),
        ];
        let terminal = scope.root_nanbox_f64(cp_box_ptr(cp_build_object(
            &methods,
            BUN_TERMINAL_SHAPE_ID + methods.len() as u32,
        ) as *const u8));
        cp_set_field(terminal.get_nanbox_f64(), TERMINAL_MARKER, TAG_TRUE_F64);
        cp_set_field(terminal.get_nanbox_f64(), TERMINAL_CURRENT, cp_undefined());
        cp_set_field(terminal.get_nanbox_f64(), TERMINAL_REFED, TAG_TRUE_F64);
        cp_set_field(terminal.get_nanbox_f64(), b"closed", TAG_FALSE_F64);
        cp_set_field(terminal.get_nanbox_f64(), b"rawMode", TAG_FALSE_F64);
        cp_set_field(
            terminal.get_nanbox_f64(),
            b"cols",
            terminal_options_i32(options.get_nanbox_f64(), b"cols", 80) as f64,
        );
        cp_set_field(
            terminal.get_nanbox_f64(),
            b"rows",
            terminal_options_i32(options.get_nanbox_f64(), b"rows", 24) as f64,
        );
        let name = cp_get_field(options.get_nanbox_f64(), b"name");
        let name = scope.root_nanbox_f64(if is_undefined(name) {
            cp_box_string("xterm-256color")
        } else {
            name
        });
        cp_set_field(terminal.get_nanbox_f64(), b"name", name.get_nanbox_f64());
        for (target, source) in [
            (TERMINAL_DATA_CB, b"data".as_slice()),
            (TERMINAL_EXIT_CB, b"exit".as_slice()),
            (TERMINAL_DRAIN_CB, b"drain".as_slice()),
        ] {
            let callback = scope.root_nanbox_f64(cp_get_field(options.get_nanbox_f64(), source));
            cp_set_field(terminal.get_nanbox_f64(), target, callback.get_nanbox_f64());
        }
        cp_set_field(terminal.get_nanbox_f64(), b"inputFlags", 0.0);
        cp_set_field(terminal.get_nanbox_f64(), b"outputFlags", 0.0);
        cp_set_field(terminal.get_nanbox_f64(), b"localFlags", 0.0);
        cp_set_field(terminal.get_nanbox_f64(), b"controlFlags", 0.0);
        let close = cp_get_field(terminal.get_nanbox_f64(), b"close");
        install_dispose_aliases(terminal.get_nanbox_f64(), close, true);
        terminal.get_nanbox_f64()
    }
}

#[cfg(unix)]
extern "C" fn bun_terminal_data_bridge(closure: *const ClosureHeader, text: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let terminal = scope.root_nanbox_f64(captured_at(closure, 0));
    let text = scope.root_nanbox_f64(text);
    let callback = cp_get_field(terminal.get_nanbox_f64(), TERMINAL_DATA_CB);
    if is_callable(callback) {
        let callback = scope.root_nanbox_f64(callback);
        let bytes = crate::child_process::cp_value_to_bytes(text.get_nanbox_f64());
        let bytes = scope.root_nanbox_f64(uint8_array_from_bytes(&bytes));
        call_value(
            callback.get_nanbox_f64(),
            &[terminal.get_nanbox_f64(), bytes.get_nanbox_f64()],
        );
    }
    cp_undefined()
}

#[cfg(unix)]
extern "C" fn bun_pty_subprocess_exit(closure: *const ClosureHeader, payload: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let subprocess = scope.root_nanbox_f64(captured_at(closure, 0));
    let terminal = scope.root_nanbox_f64(captured_at(closure, 1));
    let payload = scope.root_nanbox_f64(payload);
    let code = scope.root_nanbox_f64(cp_get_field(payload.get_nanbox_f64(), b"exitCode"));
    let signal_number = cp_get_field(payload.get_nanbox_f64(), b"signal");
    let signal_number_i32 = number_i32(signal_number);
    let signal = scope.root_nanbox_f64(
        signal_number_i32
            .map(|number| cp_box_string(crate::child_process::cp_signal_name(number)))
            .unwrap_or(TAG_NULL_F64),
    );
    let exit_code = if signal_number_i32.is_some() {
        TAG_NULL_F64
    } else {
        code.get_nanbox_f64()
    };
    cp_set_field(subprocess.get_nanbox_f64(), b"exitCode", exit_code);
    cp_set_field(
        subprocess.get_nanbox_f64(),
        b"signalCode",
        signal.get_nanbox_f64(),
    );
    cp_set_field(terminal.get_nanbox_f64(), TERMINAL_CURRENT, cp_undefined());
    let promise = promise_pointer(cp_get_field(subprocess.get_nanbox_f64(), b"exited"));
    if !promise.is_null() {
        let resolved = signal_number_i32
            .map(|number| (128 + number) as f64)
            .unwrap_or(code.get_nanbox_f64());
        crate::promise::js_promise_resolve(promise, resolved);
    }
    let callback = cp_get_field(subprocess.get_nanbox_f64(), SUBPROCESS_ON_EXIT);
    if is_callable(callback) {
        call_value(
            callback,
            &[
                subprocess.get_nanbox_f64(),
                exit_code,
                signal.get_nanbox_f64(),
                cp_undefined(),
            ],
        );
    }
    let terminal_callback = cp_get_field(terminal.get_nanbox_f64(), TERMINAL_EXIT_CB);
    if is_callable(terminal_callback) {
        // Bun.Terminal reports the PTY stream lifecycle here (0 = EOF),
        // independently from the subprocess status exposed by `onExit` and
        // `exited`.
        call_value(
            terminal_callback,
            &[terminal.get_nanbox_f64(), 0.0, TAG_NULL_F64],
        );
    }
    cp_undefined()
}

#[cfg(unix)]
extern "C" fn bun_pty_subprocess_kill(closure: *const ClosureHeader, signal: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let subprocess = scope.root_nanbox_f64(captured_at(closure, 0));
    let terminal = scope.root_nanbox_f64(cp_get_field(subprocess.get_nanbox_f64(), b"terminal"));
    let signal = scope.root_nanbox_f64(signal);
    if let Some(current) = terminal_current(terminal.get_nanbox_f64()) {
        let current = scope.root_nanbox_f64(current);
        let signal =
            if is_undefined(signal.get_nanbox_f64()) || signal.get_nanbox_f64().to_bits() == 0 {
                cp_box_string("SIGTERM")
            } else {
                signal.get_nanbox_f64()
            };
        let signal = scope.root_nanbox_f64(signal);
        let _ = call_method(
            current.get_nanbox_f64(),
            b"kill",
            &[signal.get_nanbox_f64()],
        );
        cp_set_field(subprocess.get_nanbox_f64(), b"killed", TAG_TRUE_F64);
    }
    cp_undefined()
}

#[cfg(unix)]
extern "C" fn bun_pty_subprocess_ref(closure: *const ClosureHeader) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let subprocess = scope.root_nanbox_f64(captured_at(closure, 0));
    terminal_set_refed(cp_get_field(subprocess.get_nanbox_f64(), b"terminal"), true);
    subprocess.get_nanbox_f64()
}

#[cfg(unix)]
extern "C" fn bun_pty_subprocess_unref(closure: *const ClosureHeader) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let subprocess = scope.root_nanbox_f64(captured_at(closure, 0));
    terminal_set_refed(
        cp_get_field(subprocess.get_nanbox_f64(), b"terminal"),
        false,
    );
    subprocess.get_nanbox_f64()
}

#[cfg(unix)]
extern "C" fn bun_pty_subprocess_dispose(closure: *const ClosureHeader) -> f64 {
    let _ = bun_pty_subprocess_kill(closure, cp_undefined());
    cp_undefined()
}

#[cfg(unix)]
fn finish_pty_subprocess(command: &str, args: &[String], options: f64, terminal: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let terminal = scope.root_nanbox_f64(terminal);
    let options = scope.root_nanbox_f64(options);
    if bool_field(terminal.get_nanbox_f64(), b"closed") {
        crate::exception::js_throw(type_error("Bun.spawn cannot attach a closed Terminal"));
    }
    if terminal_current(terminal.get_nanbox_f64()).is_some() {
        crate::exception::js_throw(type_error(
            "Bun.Terminal is already attached to a subprocess",
        ));
    }

    let pty_options = scope.root_nanbox_f64(clone_options(options.get_nanbox_f64()));
    for field in [b"cols".as_slice(), b"rows".as_slice(), b"name".as_slice()] {
        let value = scope.root_nanbox_f64(cp_get_field(terminal.get_nanbox_f64(), field));
        cp_set_field(pty_options.get_nanbox_f64(), field, value.get_nanbox_f64());
    }

    let command_value = scope.root_nanbox_f64(cp_box_string(command));
    let args_array = scope.root_nanbox_f64(cp_box_ptr(
        crate::array::js_array_alloc(args.len() as u32) as *const u8,
    ));
    for arg in args {
        let arg = scope.root_nanbox_f64(cp_box_string(arg));
        let Some(array) = cp_array_ptr(args_array.get_nanbox_f64()) else {
            break;
        };
        let array = crate::array::js_array_push_f64(array, arg.get_nanbox_f64());
        args_array.set_nanbox_f64(cp_box_ptr(array as *const u8));
    }
    let ipty = crate::pty::js_pty_spawn(
        command_value.get_nanbox_f64().to_bits() as i64,
        args_array.get_nanbox_f64().to_bits() as i64,
        pty_options.get_nanbox_f64().to_bits() as i64,
    );
    let ipty = scope.root_nanbox_f64(ipty);
    cp_set_field(
        terminal.get_nanbox_f64(),
        TERMINAL_CURRENT,
        ipty.get_nanbox_f64(),
    );

    let methods: [(&str, CpFn); 4] = [
        ("kill", cp_cast1(bun_pty_subprocess_kill)),
        ("ref", cp_cast0(bun_pty_subprocess_ref)),
        ("unref", cp_cast0(bun_pty_subprocess_unref)),
        ("dispose", cp_cast0(bun_pty_subprocess_dispose)),
    ];
    let subprocess = scope.root_nanbox_f64(cp_box_ptr(cp_build_object(
        &methods,
        BUN_PTY_SUBPROCESS_SHAPE_ID + methods.len() as u32,
    ) as *const u8));
    cp_set_field(
        subprocess.get_nanbox_f64(),
        b"pid",
        cp_get_field(ipty.get_nanbox_f64(), b"pid"),
    );
    cp_set_field(subprocess.get_nanbox_f64(), b"stdin", TAG_NULL_F64);
    cp_set_field(subprocess.get_nanbox_f64(), b"stdout", TAG_NULL_F64);
    cp_set_field(subprocess.get_nanbox_f64(), b"stderr", TAG_NULL_F64);
    cp_set_field(subprocess.get_nanbox_f64(), b"readable", TAG_NULL_F64);
    cp_set_field(
        subprocess.get_nanbox_f64(),
        b"terminal",
        terminal.get_nanbox_f64(),
    );
    cp_set_field(subprocess.get_nanbox_f64(), b"exitCode", TAG_NULL_F64);
    cp_set_field(subprocess.get_nanbox_f64(), b"signalCode", TAG_NULL_F64);
    cp_set_field(subprocess.get_nanbox_f64(), b"killed", TAG_FALSE_F64);
    cp_set_field(
        subprocess.get_nanbox_f64(),
        b"exited",
        scope
            .root_nanbox_f64(new_pending_promise_value())
            .get_nanbox_f64(),
    );
    let on_exit = scope.root_nanbox_f64(cp_get_field(options.get_nanbox_f64(), b"onExit"));
    cp_set_field(
        subprocess.get_nanbox_f64(),
        SUBPROCESS_ON_EXIT,
        on_exit.get_nanbox_f64(),
    );
    let dispose = cp_get_field(subprocess.get_nanbox_f64(), b"dispose");
    install_dispose_aliases(subprocess.get_nanbox_f64(), dispose, true);

    let data_bridge = scope.root_nanbox_f64(closure_with_captures(
        bun_terminal_data_bridge as *const u8,
        1,
        &[terminal.get_nanbox_f64()],
    ));
    crate::pty::pty_register(ipty.get_nanbox_f64(), "data", data_bridge.get_nanbox_f64());
    let exit_bridge = scope.root_nanbox_f64(closure_with_captures(
        bun_pty_subprocess_exit as *const u8,
        1,
        &[subprocess.get_nanbox_f64(), terminal.get_nanbox_f64()],
    ));
    crate::pty::pty_register(ipty.get_nanbox_f64(), "exit", exit_bridge.get_nanbox_f64());
    if bool_field(terminal.get_nanbox_f64(), b"rawMode") {
        if let Some(handle) = crate::pty::pty_handle_of(ipty.get_nanbox_f64()) {
            crate::pty::reactor::pty_live_set_raw_mode(handle, true);
        }
    }
    if !bool_field(terminal.get_nanbox_f64(), TERMINAL_REFED) {
        if let Some(handle) = crate::pty::pty_handle_of(ipty.get_nanbox_f64()) {
            crate::pty::reactor::pty_live_set_refed(handle, false);
        }
    }
    subprocess.get_nanbox_f64()
}

// -------------------------------------------------------------------------
// Public Bun.spawn entry
// -------------------------------------------------------------------------

fn parse_command(command_or_options: f64, options: f64) -> (String, Vec<String>, f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let command_or_options = scope.root_nanbox_f64(command_or_options);
    let options_arg = scope.root_nanbox_f64(options);
    let (command, options) = if cp_array_ptr(command_or_options.get_nanbox_f64()).is_some() {
        (
            command_or_options.get_nanbox_f64(),
            options_arg.get_nanbox_f64(),
        )
    } else if cp_object_ptr(command_or_options.get_nanbox_f64()).is_some() {
        (
            cp_get_field(command_or_options.get_nanbox_f64(), b"cmd"),
            command_or_options.get_nanbox_f64(),
        )
    } else {
        crate::exception::js_throw(type_error(
            "Bun.spawn expects a command array or an options object with cmd",
        ));
    };
    let command = scope.root_nanbox_f64(command);
    let Some(command_array) = cp_array_ptr(command.get_nanbox_f64()) else {
        crate::exception::js_throw(type_error("Bun.spawn cmd must be an array"));
    };
    let length = crate::array::js_array_length(command_array);
    if length == 0 {
        crate::exception::js_throw(type_error("Bun.spawn cmd must not be empty"));
    }
    let mut values = Vec::with_capacity(length as usize);
    for index in 0..length {
        let Some(command_array) = cp_array_ptr(command.get_nanbox_f64()) else {
            break;
        };
        let value = crate::array::js_array_get_f64(command_array, index);
        let Some(value) = crate::child_process::cp_value_to_string(value) else {
            crate::exception::js_throw(type_error("Bun.spawn command entries must be strings"));
        };
        values.push(value);
    }
    let command = values.remove(0);
    (command, values, options)
}

/// `Bun.spawn(cmd, options?)` / `Bun.spawn({ cmd, ...options })`.
#[no_mangle]
pub extern "C" fn js_bun_spawn(command_or_options: f64, options: f64) -> f64 {
    register_bun_spawn_arities();
    let scope = crate::gc::RuntimeHandleScope::new();
    let command_or_options = scope.root_nanbox_f64(command_or_options);
    let options_arg = scope.root_nanbox_f64(options);
    let (command, args, options) = parse_command(
        command_or_options.get_nanbox_f64(),
        options_arg.get_nanbox_f64(),
    );
    let options = scope.root_nanbox_f64(options);

    let terminal_option =
        scope.root_nanbox_f64(cp_get_field(options.get_nanbox_f64(), b"terminal"));
    if !is_nullish(terminal_option.get_nanbox_f64()) {
        #[cfg(unix)]
        {
            let terminal = if bool_field(terminal_option.get_nanbox_f64(), TERMINAL_MARKER) {
                terminal_option.get_nanbox_f64()
            } else if cp_object_ptr(terminal_option.get_nanbox_f64()).is_some() {
                js_bun_terminal_new(terminal_option.get_nanbox_f64())
            } else {
                crate::exception::js_throw(type_error(
                    "Bun.spawn terminal must be a Bun.Terminal or Terminal options object",
                ));
            };
            return finish_pty_subprocess(&command, &args, options.get_nanbox_f64(), terminal);
        }
        #[cfg(not(unix))]
        {
            crate::exception::js_throw(crate::child_process::cp_make_error(
                "Bun.spawn terminal is only supported on POSIX platforms by Perry",
                &[],
            ));
        }
    }

    let (normalized, held_files) = match normalized_options(options.get_nanbox_f64()) {
        Ok(value) => value,
        Err(error) => crate::exception::js_throw(error),
    };
    let normalized = scope.root_nanbox_f64(normalized);
    let args_array = scope.root_nanbox_f64(cp_box_ptr(
        crate::array::js_array_alloc(args.len() as u32) as *const u8,
    ));
    for arg in &args {
        let arg = scope.root_nanbox_f64(cp_box_string(arg));
        let Some(array) = cp_array_ptr(args_array.get_nanbox_f64()) else {
            break;
        };
        let array = crate::array::js_array_push_f64(array, arg.get_nanbox_f64());
        args_array.set_nanbox_f64(cp_box_ptr(array as *const u8));
    }
    let command_value = scope.root_nanbox_f64(cp_box_string(&command));
    let command_ptr = crate::value::js_nanbox_get_pointer(command_value.get_nanbox_f64());
    let options_ptr = cp_object_ptr(normalized.get_nanbox_f64())
        .map(|ptr| ptr as i64)
        .unwrap_or(0);
    let subprocess = crate::child_process::reactor::js_child_process_spawn_streams(
        command_ptr,
        cp_array_ptr(args_array.get_nanbox_f64())
            .map(|ptr| ptr as i64)
            .unwrap_or(0),
        options_ptr,
    );
    // Keep BunFile-backed descriptors open through `Command::spawn`; the
    // child-process layer duplicates them before this vector is dropped.
    drop(held_files);
    finish_non_pty_subprocess(subprocess, options.get_nanbox_f64())
}
