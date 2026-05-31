//! `node:zlib` option-object helpers shared with the `perry-ext-zlib` codec
//! crate.
//!
//! The one-shot codecs (`gzipSync`/`deflateSync`/…) live in `perry-ext-zlib`
//! (a `#[no_mangle]` C-ABI crate, see `well_known_bindings.toml`). That crate
//! has no access to Perry's by-name object reader or the RangeError-throwing
//! machinery, so it calls back into this helper to resolve + validate the
//! `level` option (#2935). Keeping validation here means an invalid `level`
//! throws a Node-compatible `RangeError [ERR_OUT_OF_RANGE]` via the normal
//! `js_throw` path rather than silently clamping inside the ext crate.

/// Resolve a `node:zlib` `{ level }` option to a `flate2` compression level
/// (`0..=9`), validating against Node's `-1..=9` accepted range.
///
/// `opts` is the raw NaN-boxed options value passed to a one-shot codec. When
/// it is not an object, or carries no (or `undefined`) `level`, the zlib
/// default level (`6`) is returned. Node's `Z_DEFAULT_COMPRESSION` (`-1`) maps
/// to the same default. A `level` outside `-1..=9` throws
/// `RangeError [ERR_OUT_OF_RANGE]` before any compression runs.
#[no_mangle]
pub extern "C" fn js_zlib_resolve_level(opts: f64) -> i32 {
    const DEFAULT_LEVEL: i32 = 6;

    let jv = crate::value::JSValue::from_bits(opts.to_bits());
    if !jv.is_pointer() {
        return DEFAULT_LEVEL;
    }
    let ptr = jv.as_pointer::<crate::object::ObjectHeader>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return DEFAULT_LEVEL;
    }

    let key = crate::string::js_string_from_bytes(b"level".as_ptr(), 5);
    let level_value = crate::object::js_object_get_field_by_name_f64(ptr, key);
    let lv = crate::value::JSValue::from_bits(level_value.to_bits());
    if lv.is_undefined() || lv.is_null() {
        return DEFAULT_LEVEL;
    }

    let level = if lv.is_int32() {
        lv.as_int32()
    } else if lv.is_number() {
        f64::from_bits(level_value.to_bits()) as i32
    } else {
        // Non-numeric `level` — fall back to the default rather than throwing
        // a type error (the parity surface here is numeric out-of-range).
        return DEFAULT_LEVEL;
    };

    if !(-1..=9).contains(&level) {
        let message = format!(
            "The value of \"options.level\" is out of range. It must be >= -1 and <= 9. Received {level}"
        );
        crate::fs::validate::throw_range_error_with_code(&message);
    }

    if level < 0 {
        DEFAULT_LEVEL
    } else {
        level
    }
}

/// Validate the `level`/`strategy` arguments to a zlib stream's
/// `.params(level, strategy, cb)` (#3285) and return the clamped flate2
/// compression level (`0..=9`).
///
/// Both args arrive NaN-boxed exactly as passed from JS. Mirroring Node:
/// a non-numeric `level` or `strategy` throws `TypeError [ERR_INVALID_ARG_TYPE]`;
/// a `level` outside `-1..=9` or a `strategy` outside `0..=4` throws
/// `RangeError [ERR_OUT_OF_RANGE]`. The level argument is validated first.
/// `Z_DEFAULT_COMPRESSION` (`-1`) maps to the zlib default level (`6`). The
/// `strategy` value is validated but not otherwise applied (flate2 exposes no
/// strategy knob); validation parity is the observable behavior.
///
/// The ext-zlib crate can't reach Perry's number/string typing or the
/// `js_throw` machinery, so it calls back here just like `js_zlib_resolve_level`.
#[no_mangle]
pub extern "C" fn js_zlib_validate_params(level: f64, strategy: f64) -> i32 {
    const DEFAULT_LEVEL: i32 = 6;

    fn as_number(v: f64, arg: &str) -> f64 {
        let jv = crate::value::JSValue::from_bits(v.to_bits());
        if jv.is_int32() {
            jv.as_int32() as f64
        } else if jv.is_number() {
            f64::from_bits(v.to_bits())
        } else {
            let received = crate::fs::validate::describe_received(v);
            let message =
                format!("The \"{arg}\" argument must be of type number. Received {received}");
            crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
        }
    }

    let level_num = as_number(level, "level");
    let strategy_num = as_number(strategy, "strategy");

    let level_i = level_num as i32;
    if !(-1..=9).contains(&level_i) {
        let message = format!(
            "The value of \"level\" is out of range. It must be >= -1 and <= 9. Received {level_i}"
        );
        crate::fs::validate::throw_range_error_with_code(&message);
    }

    let strategy_i = strategy_num as i32;
    if !(0..=4).contains(&strategy_i) {
        let message = format!(
            "The value of \"strategy\" is out of range. It must be >= 0 and <= 4. Received {strategy_i}"
        );
        crate::fs::validate::throw_range_error_with_code(&message);
    }

    if level_i < 0 {
        DEFAULT_LEVEL
    } else {
        level_i
    }
}

/// Keep the codegen-emitted symbol alive through the whole-program LLVM
/// bitcode rebuild performed by auto-optimize (see
/// `project_auto_optimize_keepalive_3320`). Called only from generated `.o` /
/// `perry-ext-zlib`, so without an explicit anchor the dead-stripper drops it.
#[used]
static KEEP_JS_ZLIB_RESOLVE_LEVEL: extern "C" fn(f64) -> i32 = js_zlib_resolve_level;
#[used]
static KEEP_JS_ZLIB_VALIDATE_PARAMS: extern "C" fn(f64, f64) -> i32 = js_zlib_validate_params;
