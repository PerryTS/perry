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
