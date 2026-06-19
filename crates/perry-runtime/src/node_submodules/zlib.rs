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

/// Read an options-object field by name as a raw NaN-boxed `f64`. Returns the
/// `undefined` sentinel when `ptr` carries no such property.
fn read_option_field(ptr: *const crate::object::ObjectHeader, name: &[u8]) -> f64 {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_get_field_by_name_f64(ptr, key)
}

/// Decode an options field to its numeric `f64`, validating Node's type
/// contract: an absent/`undefined`/`null` value is skipped (`None`), a numeric
/// value is returned, and anything else throws `TypeError [ERR_INVALID_ARG_TYPE]`
/// with Node's `The "<name>" property must be of type number. Received …` shape.
fn option_number(value: f64, name: &str) -> Option<f64> {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_undefined() || jv.is_null() {
        return None;
    }
    if jv.is_int32() {
        return Some(jv.as_int32() as f64);
    }
    if jv.is_number() {
        return Some(f64::from_bits(value.to_bits()));
    }
    let received = crate::fs::validate::describe_received(value);
    let message = format!("The \"{name}\" property must be of type number. Received {received}");
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

/// Validate a `>= min && <= max` zlib option (`level`/`memLevel`/`strategy`/
/// `windowBits`/`flush`). Mirrors Node's `checkRangesOrGetDefault`: a present
/// numeric value outside `[min, max]` throws `RangeError [ERR_OUT_OF_RANGE]`.
fn validate_option_range(
    ptr: *const crate::object::ObjectHeader,
    name: &[u8],
    display: &str,
    min: i64,
    max: i64,
) {
    let Some(n) = option_number(read_option_field(ptr, name), display) else {
        return;
    };
    if n < min as f64 || n > max as f64 {
        let received = crate::fs::validate::format_received_number(n);
        let message = format!(
            "The value of \"{display}\" is out of range. It must be >= {min} and <= {max}. Received {received}"
        );
        crate::fs::validate::throw_range_error_with_code(&message);
    }
}

/// Validate the `chunkSize` option. Node only enforces a lower bound
/// (`Z_MIN_CHUNK` = 64) and rejects `NaN`, with a `It must be >= 64` message
/// that omits the upper bound the ranged options carry.
fn validate_option_chunk_size(ptr: *const crate::object::ObjectHeader) {
    let Some(n) = option_number(read_option_field(ptr, b"chunkSize"), "options.chunkSize") else {
        return;
    };
    if n < 64.0 || n.is_nan() {
        let received = crate::fs::validate::format_received_number(n);
        let message = format!(
            "The value of \"options.chunkSize\" is out of range. It must be >= 64. Received {received}"
        );
        crate::fs::validate::throw_range_error_with_code(&message);
    }
}

/// Validate a `node:zlib` options object the way Node's `Zlib`/`ZlibBase`
/// constructors do, throwing the spec-mandated `TypeError`/`RangeError` before
/// any compression runs (#3662). Shared by the one-shot sync codecs and the
/// `createGzip`/`createDeflate`/… stream factories, and by both the in-tree
/// (`perry-stdlib`) and out-of-tree (`perry-ext-zlib`) codec crates.
///
/// `min_window_bits` is the lower `windowBits` bound, which differs by codec:
/// gzip compression requires `>= 9`, every other codec accepts `>= 8`.
///
/// The field order matches Node exactly (`windowBits`, `level`, `memLevel`,
/// `strategy`, `chunkSize`, `flush`) so that an object with several bad options
/// reports the same first offender Node does.
#[no_mangle]
pub extern "C" fn js_zlib_validate_options(opts: f64, min_window_bits: i32) {
    let jv = crate::value::JSValue::from_bits(opts.to_bits());
    if !jv.is_pointer() {
        return;
    }
    let ptr = jv.as_pointer::<crate::object::ObjectHeader>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return;
    }

    validate_option_range(
        ptr,
        b"windowBits",
        "options.windowBits",
        min_window_bits as i64,
        15,
    );
    validate_option_range(ptr, b"level", "options.level", -1, 9);
    validate_option_range(ptr, b"memLevel", "options.memLevel", 1, 9);
    validate_option_range(ptr, b"strategy", "options.strategy", 0, 4);
    validate_option_chunk_size(ptr);
    validate_option_range(ptr, b"flush", "options.flush", 0, 5);

    // #4917: `level` is honored and the ranged options above are validated,
    // but a preset `dictionary` is not threaded through flate2. Silently
    // dropping it would mis-compress against peers that expect it to apply,
    // so warn once instead.
    let dict = read_option_field(ptr, b"dictionary");
    let dv = crate::value::JSValue::from_bits(dict.to_bits());
    if !dv.is_undefined() && !dv.is_null() {
        crate::stub_diag::perry_stub_warn(
            "zlib options.dictionary",
            "the preset dictionary option is accepted but not applied",
            Some("#4917"),
        );
    }
}

/// Validate the `buffer` argument to a one-shot zlib codec (`gzipSync`,
/// `deflateSync`, `gunzipSync`, `inflateSync`, …) the way Node does (#3662):
/// the value must be a string or an instance of Buffer/TypedArray/DataView/
/// ArrayBuffer, otherwise `TypeError [ERR_INVALID_ARG_TYPE]` is thrown.
///
/// `data_bits` is the raw NaN-box bit pattern of the data argument (the codecs
/// receive it as a full value via `NA_F64`). The in-tree `perry-stdlib` codecs
/// already perform this check inside `codec_bytes`; this shared helper lets the
/// out-of-tree `perry-ext-zlib` codecs (used by the auto-optimize build) reject
/// the same inputs without duplicating the runtime's value typing.
#[no_mangle]
pub extern "C" fn js_zlib_validate_buffer_arg(data_bits: i64) {
    let value = f64::from_bits(data_bits as u64);
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_any_string() {
        return;
    }
    // Buffer / TypedArray / DataView / ArrayBuffer all live behind a heap
    // pointer; recover the raw address (pointer/string-tagged values mask off
    // the tag, a bare small pointer is used as-is) and probe the registries.
    let bits = value.to_bits();
    let raw = if jv.is_pointer() || jv.is_string() {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if !value.is_nan() && (0x1000..0x0001_0000_0000_0000).contains(&bits) {
        bits as usize
    } else {
        0
    };
    if raw >= 0x1000 {
        if crate::typedarray::lookup_typed_array_kind(raw).is_some() {
            return;
        }
        if crate::buffer::is_registered_buffer(raw) {
            return;
        }
    }
    let received = crate::fs::validate::describe_received(value);
    let message = format!(
        "The \"buffer\" argument must be of type string or an instance of Buffer, TypedArray, DataView, or ArrayBuffer. Received {received}"
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

/// Validate the required callback for async one-shot zlib helpers
/// (`gzip(data, cb)`, `gunzip(data, cb)`, `brotliDecompress(data, cb)`, ...).
/// Node throws synchronously before queuing codec work when the callback is
/// missing or not callable.
#[no_mangle]
pub extern "C" fn js_zlib_validate_callback(callback: f64) -> i64 {
    crate::fs::validate::validate_required_callback("callback", callback) as i64
}

/// Keep the codegen-emitted symbol alive through the whole-program LLVM
/// bitcode rebuild performed by auto-optimize (see
/// `project_auto_optimize_keepalive_3320`). Called only from generated `.o` /
/// `perry-ext-zlib`, so without an explicit anchor the dead-stripper drops it.
#[used]
static KEEP_JS_ZLIB_RESOLVE_LEVEL: extern "C" fn(f64) -> i32 = js_zlib_resolve_level;
#[used]
static KEEP_JS_ZLIB_VALIDATE_PARAMS: extern "C" fn(f64, f64) -> i32 = js_zlib_validate_params;
#[used]
static KEEP_JS_ZLIB_VALIDATE_OPTIONS: extern "C" fn(f64, i32) = js_zlib_validate_options;
#[used]
static KEEP_JS_ZLIB_VALIDATE_BUFFER_ARG: extern "C" fn(i64) = js_zlib_validate_buffer_arg;
#[used]
static KEEP_JS_ZLIB_VALIDATE_CALLBACK: extern "C" fn(f64) -> i64 = js_zlib_validate_callback;
