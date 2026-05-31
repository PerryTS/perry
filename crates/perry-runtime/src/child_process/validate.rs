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

/// Validate a `command` / `file` argument. `value` is the original NaN-boxed
/// JS value; `name` is `"command"` (exec/execSync) or `"file"` (execFile /
/// execFileSync / spawn / spawnSync). Throws `TypeError [ERR_INVALID_ARG_TYPE]`
/// with Node's `The "<name>" argument must be of type string. Received …`
/// message when `value` is not a string. A no-op for any string.
fn cp_validate_command(value: f64, name: &str) {
    if JSValue::from_bits(value.to_bits()).is_any_string() {
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
        return;
    }
    let message = format!(
        "The \"args\" argument must be of type object. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
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

/// Codegen-invoked `args` validator (#3079). `value` is the original NaN-boxed
/// JS value passed in the args slot. Diverges via `js_throw` on a primitive.
#[no_mangle]
pub extern "C" fn js_child_process_validate_args(value: f64) -> f64 {
    cp_validate_args(value);
    value
}

/// `#[used]` keepalive anchors so the auto-optimize whole-program-LLVM rebuild
/// does not dead-strip these codegen-invoked `#[no_mangle]` entry points (see
/// project_auto_optimize_keepalive_3320). They are referenced only from
/// generated `.o`, so without an anchor the bitcode internalizer drops them and
/// the default `perry file.ts -o out` link fails.
#[used]
static KEEP_JS_CP_VALIDATE_COMMAND: unsafe extern "C" fn(f64, *const u8, u32) -> f64 =
    js_child_process_validate_command;
#[used]
static KEEP_JS_CP_VALIDATE_ARGS: extern "C" fn(f64) -> f64 = js_child_process_validate_args;
