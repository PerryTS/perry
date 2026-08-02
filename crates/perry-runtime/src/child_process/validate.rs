//! Setup-time `command` / `file` / `args` input validation — #3079.
//!
//! Node validates `command` / `file` / `args` synchronously before launching a
//! child and throws `TypeError [ERR_INVALID_ARG_TYPE]` on a bad shape (missing
//! command, non-string command/file, non-array/non-object args). Perry's codegen
//! strips the command/file value to a raw `*const StringHeader` (so the runtime
//! can no longer tell `undefined` apart from a non-string), and the args readers
//! silently coerce a missing/non-array value to an empty list — diverging from
//! Node. These `#[no_mangle]` validators receive the *original* NaN-boxed value
//! (codegen already has it as the boxed `cmd_box`/`file_box`/args value) and
//! throw Node's exact message before any of the existing entry points run.

use crate::value::JSValue;

use super::{
    cp_array_ptr, cp_get_field, cp_object_ptr, cp_signal_is_valid, cp_stdio_stream_fd,
    cp_value_to_string,
};

fn cp_throw_null_bytes() -> ! {
    crate::fs::validate::throw_type_error_with_code(
        "The argument must not contain null bytes",
        "ERR_INVALID_ARG_VALUE",
    );
}

/// Validate a `command` / `file` argument. `value` is the original NaN-boxed
/// JS value; `name` is `"command"` (exec/execSync) or `"file"` (execFile /
/// execFileSync / spawn / spawnSync). Throws `TypeError [ERR_INVALID_ARG_TYPE]`
/// with Node's `The "<name>" argument must be of type string. Received …`
/// message when `value` is not a string. A no-op for any string.
pub(crate) fn cp_validate_command(value: f64, name: &str) {
    if JSValue::from_bits(value.to_bits()).is_any_string() {
        if cp_value_to_string(value).is_some_and(|value| value.contains('\0')) {
            cp_throw_null_bytes();
        }
        return;
    }
    let message = format!(
        "The \"{name}\" argument must be of type string. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

/// Validate an `args` argument. `value` is the original NaN-boxed JS value.
/// Node accepts `undefined` / `null` (treated as an empty list) and any object
/// (arrays and plain objects), but throws `TypeError [ERR_INVALID_ARG_TYPE]`
/// with `The "args" argument must be of type object. Received …` for a
/// primitive such as a string, number, boolean, bigint, or symbol.
fn cp_validate_args(value: f64) {
    let jv = JSValue::from_bits(value.to_bits());
    // Node's `normalizeSpawnArguments` / `normalizeExecFileArgs` reject the
    // args slot only when it is a non-nullish *primitive* (string, number,
    // boolean, bigint, symbol). `undefined`/`null` become an empty list;
    // arrays and plain objects are the args/options forms; a function is the
    // callback (execFile) or tolerated (execFileSync). So reject by primitive
    // type and accept everything else. Note: codegen lowers the args slot
    // positionally, so for `execFile(file, cb)` the callback lands here — it
    // must NOT throw.
    let is_rejected_primitive = jv.is_any_string()
        || crate::fs::validate::is_numeric(jv)
        || jv.is_bool()
        || jv.is_bigint()
        || unsafe { crate::symbol::js_is_symbol(value) != 0 };
    if !is_rejected_primitive {
        if let Some(args) = cp_array_ptr(value) {
            for i in 0..unsafe { (*args).length } {
                let value = crate::array::js_array_get_f64(args, i);
                if cp_value_to_string(value).is_some_and(|value| value.contains('\0')) {
                    cp_throw_null_bytes();
                }
            }
        }
        return;
    }
    let message = format!(
        "The \"args\" argument must be of type object. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

fn cp_throw_option_type(name: &str, value: f64) -> ! {
    let message = format!(
        "The \"options.{name}\" property must be of type {}. Received {}",
        if name == "shell" {
            "boolean or string"
        } else {
            "boolean"
        },
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

fn cp_is_undefined(value: f64) -> bool {
    JSValue::from_bits(value.to_bits()).is_undefined()
}

fn cp_is_number(value: f64) -> bool {
    crate::fs::validate::is_numeric(JSValue::from_bits(value.to_bits()))
}

fn cp_validate_stdio_entry(
    value: f64,
    sync: bool,
    in_array: bool,
    fd_index: usize,
    ipc_count: &mut u32,
) {
    if JSValue::from_bits(value.to_bits()).is_null() || cp_is_undefined(value) {
        return;
    }
    if JSValue::from_bits(value.to_bits()).is_any_string() {
        let name = cp_value_to_string(value).unwrap_or_default();
        match name.as_str() {
            "pipe" | "ignore" | "inherit" | "overlapped" => return,
            "ipc" => {
                *ipc_count += 1;
                if sync {
                    crate::fs::validate::throw_error_with_code(
                        "IPC cannot be used with spawnSync",
                        "ERR_IPC_SYNC_FORK",
                    );
                }
                if *ipc_count > 1 {
                    crate::fs::validate::throw_error_with_code(
                        "Child process can have only one IPC pipe",
                        "ERR_IPC_ONE_PIPE",
                    );
                }
                return;
            }
            // Node routes invalid entries in a stdio array through the shared
            // getValidStdio error, even for asynchronous spawn.
            _ if sync || in_array => crate::fs::validate::throw_type_error_with_code(
                "Invalid stdio option",
                "ERR_INVALID_SYNC_FORK_INPUT",
            ),
            _ => crate::fs::validate::throw_type_error_with_code(
                "Invalid stdio option",
                "ERR_INVALID_ARG_VALUE",
            ),
        }
    }
    if in_array && cp_is_number(value) {
        let number = JSValue::from_bits(value.to_bits()).to_number();
        if number.is_finite() && number >= 0.0 && number.fract() == 0.0 && number <= i32::MAX as f64
        {
            return;
        }
    }
    if cp_stdio_stream_fd(value, fd_index).is_some() {
        return;
    }
    crate::fs::validate::throw_type_error_with_code(
        "Invalid stdio option",
        "ERR_INVALID_ARG_VALUE",
    );
}

/// Validate the common spawn/fork option bag before codegen strips NaN-box
/// tags. `sync` selects the Node-specific synchronous stdio error codes;
/// `allow_null` is used by fork, whose options overload accepts `null`.
fn cp_validate_options(value: f64, sync: bool, allow_null: bool) {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_undefined() || (allow_null && js.is_null()) {
        return;
    }
    if cp_object_ptr(value).is_none() {
        let message = format!(
            "The \"options\" argument must be of type object. Received {}",
            crate::fs::validate::describe_received(value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }

    // Node normalizes stdio before the remaining spawn options. Preserve that
    // precedence when a cumulative cluster.settings snapshot contains more
    // than one invalid field.
    let stdio = cp_get_field(value, b"stdio");
    if !cp_is_undefined(stdio) {
        let mut ipc_count = 0;
        if let Some(arr) = cp_array_ptr(stdio) {
            for index in 0..unsafe { (*arr).length } {
                cp_validate_stdio_entry(
                    crate::array::js_array_get_f64(arr, index),
                    sync,
                    true,
                    index as usize,
                    &mut ipc_count,
                );
            }
        } else {
            cp_validate_stdio_entry(stdio, sync, false, 0, &mut ipc_count);
        }
    }

    let required_string = ["cwd", "argv0"];
    for name in required_string {
        let item = cp_get_field(value, name.as_bytes());
        if !cp_is_undefined(item)
            && !JSValue::from_bits(item.to_bits()).is_null()
            && !JSValue::from_bits(item.to_bits()).is_any_string()
        {
            let message = format!(
                "The \"options.{name}\" property must be of type string. Received {}",
                crate::fs::validate::describe_received(item)
            );
            crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
        }
    }

    for name in ["detached"] {
        let item = cp_get_field(value, name.as_bytes());
        if !cp_is_undefined(item) && !JSValue::from_bits(item.to_bits()).is_bool() {
            cp_throw_option_type(name, item);
        }
    }
    let shell = cp_get_field(value, b"shell");
    if !cp_is_undefined(shell)
        && !JSValue::from_bits(shell.to_bits()).is_bool()
        && !JSValue::from_bits(shell.to_bits()).is_any_string()
    {
        cp_throw_option_type("shell", shell);
    }

    for name in ["timeout", "maxBuffer"] {
        let item = cp_get_field(value, name.as_bytes());
        if cp_is_undefined(item) {
            continue;
        }
        if !cp_is_number(item) {
            let message = format!(
                "The \"options.{name}\" property must be of type number. Received {}",
                crate::fs::validate::describe_received(item)
            );
            crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
        }
        let number = JSValue::from_bits(item.to_bits()).to_number();
        if number < 0.0 || number.is_nan() || (name == "timeout" && !number.is_finite()) {
            crate::fs::validate::throw_range_error_with_code(&format!(
                "The value of \"options.{name}\" is out of range"
            ));
        }
    }

    for name in ["uid", "gid"] {
        let item = cp_get_field(value, name.as_bytes());
        if !cp_is_undefined(item) && !cp_is_number(item) {
            let message = format!(
                "The \"options.{name}\" property must be of type number. Received {}",
                crate::fs::validate::describe_received(item)
            );
            crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE");
        }
    }

    let inspect_port = cp_get_field(value, b"inspectPort");
    if JSValue::from_bits(inspect_port.to_bits()).is_null() {
        crate::fs::validate::throw_range_error_named(
            "The value of \"options.inspectPort\" is out of range",
            "ERR_SOCKET_BAD_PORT",
        );
    }
    if !cp_is_undefined(inspect_port)
        && !cp_is_number(inspect_port)
        && crate::fs::extract_closure_ptr(inspect_port).is_null()
    {
        crate::fs::validate::throw_type_error_with_code(
            "The \"options.inspectPort\" property must be of type number or function",
            "ERR_INVALID_ARG_TYPE",
        );
    }

    let signal = cp_get_field(value, b"killSignal");
    if !cp_is_undefined(signal) && !cp_signal_is_valid(signal) {
        crate::fs::validate::throw_type_error_with_code("Unknown signal", "ERR_UNKNOWN_SIGNAL");
    }

    let serialization = cp_get_field(value, b"serialization");
    if !cp_is_undefined(serialization)
        && (!JSValue::from_bits(serialization.to_bits()).is_any_string()
            || !matches!(
                cp_value_to_string(serialization).as_deref(),
                Some("json" | "advanced")
            ))
    {
        crate::fs::validate::throw_type_error_with_code(
            "The \"options.serialization\" property must be one of: 'json', 'advanced'",
            "ERR_INVALID_ARG_VALUE",
        );
    }
}

/// `new ChildProcess().spawn(options)` exposes the low-level Node constructor
/// API. It takes one object containing a required string `file` and array
/// `args`; validate this public boundary before the reactor ever sees it.
pub(crate) fn cp_validate_child_process_spawn(value: f64) {
    if cp_object_ptr(value).is_none() {
        crate::fs::validate::throw_type_error_with_code(
            "The \"options\" argument must be of type object",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    cp_validate_command(cp_get_field(value, b"file"), "options.file");
    if cp_array_ptr(cp_get_field(value, b"args")).is_none() {
        crate::fs::validate::throw_type_error_with_code(
            "The \"options.args\" property must be an array",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    cp_validate_options(value, false, false);
}

/// Codegen-invoked `command`/`file` validator (#3079). `value` is the original
/// NaN-boxed JS value; `name_ptr`/`name_len` describe the static argument name
/// (`"command"` or `"file"`). Diverges via `js_throw` on a non-string value.
///
/// # Safety
/// `name_ptr`/`name_len` must describe a valid UTF-8 byte range.
#[no_mangle]
pub unsafe extern "C" fn js_child_process_validate_command(
    value: f64,
    name_ptr: *const u8,
    name_len: u32,
) -> f64 {
    let name = if name_ptr.is_null() || name_len == 0 {
        "command".to_string()
    } else {
        let bytes = std::slice::from_raw_parts(name_ptr, name_len as usize);
        String::from_utf8_lossy(bytes).into_owned()
    };
    cp_validate_command(value, &name);
    value
}

/// `fork()` accepts strings, Buffers, and WHATWG URL objects. This runs before
/// codegen converts the value to a raw string pointer so primitives cannot be
/// silently string-coerced into module paths.
#[no_mangle]
pub extern "C" fn js_child_process_validate_fork_module(value: f64) -> f64 {
    let js = JSValue::from_bits(value.to_bits());
    let raw = (value.to_bits() & crate::value::POINTER_MASK) as usize;
    let is_url = cp_object_ptr(value).is_some_and(crate::url::is_url_object_shape);
    if js.is_any_string()
        || (raw >= 0x10000
            && crate::buffer::is_registered_buffer(raw)
            && !crate::buffer::is_any_array_buffer(raw))
        || is_url
    {
        return value;
    }
    let message = format!(
        "The \"modulePath\" argument must be of type string or an instance of Buffer or URL. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

/// Codegen-invoked `args` validator (#3079). `value` is the original NaN-boxed
/// JS value passed in the args slot. Diverges via `js_throw` on a primitive.
#[no_mangle]
pub extern "C" fn js_child_process_validate_args(value: f64) -> f64 {
    cp_validate_args(value);
    value
}

/// `spawn*`/`fork` interpret a non-array object in the args slot as their
/// options overload. Validate it here before codegen removes its type tag.
#[no_mangle]
pub extern "C" fn js_child_process_validate_spawn_args(
    value: f64,
    sync: i32,
    allow_null: i32,
) -> f64 {
    cp_validate_args(value);
    if cp_array_ptr(value).is_none() && cp_object_ptr(value).is_some() {
        cp_validate_options(value, sync != 0, allow_null != 0);
    }
    value
}

/// Codegen-invoked shared option validation. `sync` is non-zero for
/// `spawnSync`; `allow_null` is non-zero for `fork`.
#[no_mangle]
pub extern "C" fn js_child_process_validate_options(value: f64, sync: i32, allow_null: i32) -> f64 {
    cp_validate_options(value, sync != 0, allow_null != 0);
    value
}

/// Feature-gated (`keepalive-anchors`) `#[used]` anchors so the whole-program
/// bitcode-LTO link does not dead-strip these codegen-invoked `#[no_mangle]`
/// entry points (see project_auto_optimize_keepalive_3320). They are
/// referenced only from generated `.o`, so the bitcode internalizer would
/// otherwise drop them there; the classic link keeps them via the program's
/// own undefined references and builds without the anchors.
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_CP_VALIDATE_COMMAND: unsafe extern "C" fn(f64, *const u8, u32) -> f64 =
    js_child_process_validate_command;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_CP_VALIDATE_ARGS: extern "C" fn(f64) -> f64 = js_child_process_validate_args;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_CP_VALIDATE_FORK_MODULE: extern "C" fn(f64) -> f64 =
    js_child_process_validate_fork_module;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_CP_VALIDATE_OPTIONS: extern "C" fn(f64, i32, i32) -> f64 =
    js_child_process_validate_options;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_CP_VALIDATE_SPAWN_ARGS: extern "C" fn(f64, i32, i32) -> f64 =
    js_child_process_validate_spawn_args;
