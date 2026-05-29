//! Node-compatible argument validation for the `node:net` surface (#2013):
//! `server.listen(port)` / `socket.connect(port)` port range checks,
//! `net.createServer(options)` options-type check, and
//! `socket.setTimeout(msecs)` number/range check.
//!
//! Node throws synchronously on these bad arguments with a specific `.code`
//! (`ERR_SOCKET_BAD_PORT` / `ERR_INVALID_ARG_TYPE` / `ERR_OUT_OF_RANGE`);
//! Perry previously coerced silently (`as u16` / ignored), so
//! `assert.throws`-style tests saw "Missing expected exception". These helpers
//! reuse the generic Node-error primitives in [`crate::fs::validate`]
//! (`is_numeric`, `describe_received`, `throw_type_error_with_code`,
//! `throw_range_error_named`) — the shared validation home the issue calls out.
//!
//! The port/timeout validators are plain Rust fns called from the `perry-ext-net`
//! socket entry points (which already link `perry-runtime`); the
//! `createServer` validator is `#[no_mangle]` so the `Expr::NetCreateServer`
//! codegen can call it by symbol, mirroring the Buffer factory validators.

use crate::fs::validate::{
    describe_received, is_numeric, throw_range_error_named, throw_type_error_with_code,
};
use crate::value::JSValue;

/// Render a finite/non-finite number the way Node prints the bare `Received …`
/// clause of an `ERR_OUT_OF_RANGE` message (no `type number (...)` wrapper).
fn format_received_number(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        }
        .to_string();
    }
    if n.fract() == 0.0 && n.abs() < 1e21 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

/// Node's `validatePort`: a numeric `port` must be an integer in `[0, 65535]`
/// (`>= 0 && < 65536`); `NaN`, negatives, out-of-range, and non-integers throw
/// `RangeError [ERR_SOCKET_BAD_PORT]`. Non-numeric values (a string is a pipe
/// path, `undefined` requests a random port, an object is an options bag) are
/// left untouched for the caller's existing arg-shape handling — only *numbers*
/// are range-checked here, matching Node, where `listen('x')` does not throw.
///
/// `listen` is `true` for `server.listen` (whose message is prefixed
/// `options.port`) and `false` for `socket.connect` (prefixed `Port`).
fn validate_net_port(value: f64, listen: bool) {
    let jv = JSValue::from_bits(value.to_bits());
    if !is_numeric(jv) {
        return;
    }
    let n = if jv.is_int32() {
        jv.as_int32() as f64
    } else {
        jv.as_number()
    };
    if n.is_finite() && n.fract() == 0.0 && (0.0..65536.0).contains(&n) {
        return;
    }
    let prefix = if listen { "options.port" } else { "Port" };
    // The bad value is always numeric here (we returned early otherwise), so
    // render the `type number (...)` clause directly — this avoids depending on
    // `describe_received`'s string/bigint/Infinity handling.
    let message = format!(
        "{prefix} should be >= 0 and < 65536. Received type number ({}).",
        format_received_number(n)
    );
    throw_range_error_named(&message, "ERR_SOCKET_BAD_PORT");
}

/// C-ABI entry for `server.listen(port)` port validation. `perry-ext-net`
/// declares and calls this by symbol (it has no Cargo dependency on
/// `perry-runtime` — the #2041 C-ABI pattern).
#[no_mangle]
pub extern "C" fn js_net_validate_listen_port(value: f64) {
    validate_net_port(value, true);
}

/// C-ABI entry for `socket.connect(port)` / `net.connect({ port })` port
/// validation.
#[no_mangle]
pub extern "C" fn js_net_validate_connect_port(value: f64) {
    validate_net_port(value, false);
}

/// `socket.setTimeout(msecs)` — Node `validateNumber` + non-negative-finite
/// range check: a non-number throws `ERR_INVALID_ARG_TYPE`; `NaN`, `Infinity`,
/// or a negative value throws `ERR_OUT_OF_RANGE`. No-op on a valid value.
#[no_mangle]
pub extern "C" fn js_net_validate_socket_timeout(value: f64) {
    let jv = JSValue::from_bits(value.to_bits());
    if !is_numeric(jv) {
        let message = format!(
            "The \"msecs\" argument must be of type number. Received {}",
            describe_received(value)
        );
        throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    let n = if jv.is_int32() {
        jv.as_int32() as f64
    } else {
        jv.as_number()
    };
    if !(n.is_finite() && n >= 0.0) {
        let message = format!(
            "The value of \"msecs\" is out of range. It must be a non-negative finite number. Received {}",
            format_received_number(n)
        );
        throw_range_error_named(&message, "ERR_OUT_OF_RANGE");
    }
}

/// `net.createServer(options?, connectionListener?)` — the first positional
/// argument must be either a function (the connection listener) or an object
/// (the options bag); `null`/`undefined` are accepted as "no options". A
/// number/boolean/string/bigint throws `TypeError [ERR_INVALID_ARG_TYPE]`,
/// matching Node's `Server` constructor. Closures and objects (incl. arrays)
/// are both `POINTER_TAG`, so a single `is_pointer` check covers the accepted
/// reference kinds. Diverges via `js_throw` on a bad value; no-op otherwise.
#[no_mangle]
pub extern "C" fn js_net_validate_create_server_options(value: f64) {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() || jv.is_null() || jv.is_pointer() {
        return;
    }
    let message = format!(
        "The \"options\" argument must be of type object. Received {}",
        describe_received(value)
    );
    throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

fn pointer_addr(value: f64) -> Option<usize> {
    let bits = value.to_bits();
    let jv = JSValue::from_bits(bits);
    if jv.is_pointer() {
        Some(jv.as_pointer::<u8>() as usize)
    } else if (bits >> 48) == 0 {
        Some(bits as usize)
    } else {
        None
    }
}

unsafe fn array_from_value(value: f64) -> *const crate::array::ArrayHeader {
    let Some(addr) = pointer_addr(value) else {
        return std::ptr::null();
    };
    if addr < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return std::ptr::null();
    }
    let gc_header =
        ((addr as *const u8).sub(crate::gc::GC_HEADER_SIZE)) as *const crate::gc::GcHeader;
    if (*gc_header).obj_type != crate::gc::GC_TYPE_ARRAY {
        return std::ptr::null();
    }
    addr as *const crate::array::ArrayHeader
}

fn is_function_value(value: f64) -> bool {
    pointer_addr(value)
        .map(crate::closure::is_closure_ptr)
        .unwrap_or(false)
}

fn boxed_object(ptr: *mut crate::object::ObjectHeader) -> f64 {
    f64::from_bits(JSValue::pointer(ptr as *mut u8).bits())
}

unsafe fn object_from_shape(
    shape_id: u32,
    packed_keys: &'static [u8],
    values: &[f64],
) -> *mut crate::object::ObjectHeader {
    let obj = crate::object::js_object_alloc_with_shape(
        shape_id,
        values.len() as u32,
        packed_keys.as_ptr(),
        packed_keys.len() as u32,
    );
    for (index, value) in values.iter().enumerate() {
        crate::object::js_object_set_field(obj, index as u32, JSValue::from_bits(value.to_bits()));
    }
    obj
}

/// Node's legacy `net._normalizeArgs(args)` helper. It accepts the raw
/// overload argument array and returns `[options, callback]`.
#[no_mangle]
pub unsafe extern "C" fn js_net_normalize_args(args_value: f64) -> f64 {
    let args = array_from_value(args_value);
    let len = if args.is_null() {
        0
    } else {
        crate::array::js_array_length(args)
    };
    let arg = |index: u32| -> f64 {
        if !args.is_null() && index < len {
            crate::array::js_array_get_f64(args, index)
        } else {
            f64::from_bits(JSValue::undefined().bits())
        }
    };

    let first = arg(0);
    let first_value = JSValue::from_bits(first.to_bits());
    let first_is_string = first_value.is_string() || first_value.is_short_string();
    let first_is_object_options = first_value.is_pointer() && !is_function_value(first);

    let options = if len == 0 {
        boxed_object(object_from_shape(0x4E45_5400, b"", &[]))
    } else if first_is_object_options {
        first
    } else if first_is_string {
        boxed_object(object_from_shape(0x4E45_5401, b"path\0", &[first]))
    } else {
        let second = arg(1);
        let second_value = JSValue::from_bits(second.to_bits());
        if second_value.is_string() || second_value.is_short_string() {
            boxed_object(object_from_shape(
                0x4E45_5402,
                b"port\0host\0",
                &[first, second],
            ))
        } else {
            boxed_object(object_from_shape(0x4E45_5403, b"port\0", &[first]))
        }
    };

    let callback = if is_function_value(arg(2)) {
        arg(2)
    } else if is_function_value(arg(1)) {
        arg(1)
    } else {
        f64::from_bits(JSValue::null().bits())
    };

    let mut result = crate::array::js_array_alloc(2);
    result = crate::array::js_array_push_f64(result, options);
    result = crate::array::js_array_push_f64(result, callback);
    f64::from_bits(JSValue::pointer(result as *mut u8).bits())
}

/// Function-shaped placeholder for Node's internal helper. Full handle
/// creation is covered by the public `createServer`/`Server` paths.
#[no_mangle]
pub extern "C" fn js_net_create_server_handle_stub(
    _address: f64,
    _port: f64,
    _address_type: f64,
    _fd: f64,
    _flags: f64,
) -> f64 {
    f64::from_bits(JSValue::undefined().bits())
}
